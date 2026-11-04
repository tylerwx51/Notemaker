//! Port of `SvgAsset.java`: portable, self-contained static SVG profile.
//!
//! Java parsed with a namespace-aware Xerces DOM and re-serialized the root
//! element with the JDK identity `Transformer` (Xalan `DOM2TO` + `ToStream`).
//! Here `roxmltree` decides well-formedness, and a small tokenizer builds the
//! tree that the Java rules are applied to and that is serialized with the
//! Xalan escaping rules (attributes sorted by qualified name, namespace
//! declarations first, redundant declarations dropped, CDATA kept, non-BMP
//! characters as decimal character references).

use crate::error::{Error, Result};
use crate::geometry::{DocPoint, Radius};
use crate::jfmt;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};

pub const MAX_BYTES: usize = 262144;

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

const TAGS: &[&str] = &[
    "svg", "g", "defs", "title", "desc", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "text",
    "tspan", "linearGradient", "radialGradient", "stop", "clipPath", "marker", "style", "use", "pattern",
];
const ATTRS: &[&str] = &[
    "id", "class", "style", "version", "x", "y", "x1", "y1", "x2", "y2", "dx", "dy", "width", "height", "viewBox",
    "preserveAspectRatio", "transform", "d", "points", "rx", "ry", "r", "cx", "cy", "fx", "fy", "fr", "fill",
    "fill-opacity", "fill-rule", "stroke", "stroke-width", "stroke-opacity", "stroke-linecap", "stroke-linejoin",
    "stroke-miterlimit", "stroke-dasharray", "stroke-dashoffset", "opacity", "color", "font-family", "font-size",
    "font-style", "font-weight", "text-anchor", "dominant-baseline", "alignment-baseline", "baseline-shift",
    "textLength", "lengthAdjust", "display", "visibility", "clip-path", "clip-rule", "gradientUnits",
    "gradientTransform", "spreadMethod", "offset", "stop-color", "stop-opacity", "href", "marker-start",
    "marker-mid", "marker-end", "markerWidth", "markerHeight", "markerUnits", "refX", "refY", "orient", "type",
    "space", "patternUnits", "patternContentUnits", "patternTransform",
];

/// `SvgAsset.DotGrid`: parsed coordinates of the exact generated dot-grid profile.
#[derive(Debug, Clone)]
pub struct DotGrid {
    pub width: f64,
    pub height: f64,
    pub stroke_width: f64,
    pub stroke_opacity: f64,
    pub color: i32,
    circles: Vec<f64>,
}

impl DotGrid {
    /// `circles()`: each dot's centre and radius.
    pub fn circles(&self) -> impl Iterator<Item = DotCircle> + '_ {
        self.circles.chunks_exact(3).map(|c| DotCircle { center: DocPoint { x: c[0], y: c[1] }, radius: Radius(c[2]) })
    }
}

/// One dot of a [`DotGrid`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DotCircle {
    pub center: DocPoint,
    pub radius: Radius,
}

struct Asset {
    size: [f64; 2],
    normalized: String,
    dots: Option<Arc<DotGrid>>,
}

/// Access-ordered LRU of 16 entries (`LinkedHashMap(16,.75f,true)` + `removeEldestEntry`).
static CACHE: Mutex<Option<IndexMap<String, Arc<Asset>>>> = Mutex::new(None);

fn cache_get(cache: &mut IndexMap<String, Arc<Asset>>, source: &str) -> Option<Arc<Asset>> {
    let index = cache.get_index_of(source)?;
    let (key, value) = cache.shift_remove_index(index)?;
    cache.insert(key, value.clone());
    Some(value)
}

fn cache_put(cache: &mut IndexMap<String, Arc<Asset>>, source: &str, asset: Arc<Asset>) {
    cache.shift_remove(source);
    cache.insert(source.to_string(), asset);
    if cache.len() > 16 {
        cache.shift_remove_index(0);
    }
}

fn with_cache<T>(f: impl FnOnce(&mut IndexMap<String, Arc<Asset>>) -> T) -> T {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(IndexMap::new))
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::illegal_argument(message))
    }
}

// ---------------------------------------------------------------------------
// Generated dot grid fast path

fn after(source: &str, start: isize, literal: &str) -> isize {
    if start < 0 || start as usize > source.len() {
        return -1;
    }
    if source.as_bytes()[start as usize..].starts_with(literal.as_bytes()) {
        start + literal.len() as isize
    } else {
        -1
    }
}

fn index_of_quote(source: &str, from: usize) -> isize {
    match source.as_bytes().get(from..).and_then(|b| b.iter().position(|&c| c == b'"')) {
        Some(i) => (from + i) as isize,
        None => -1,
    }
}

/// `SvgAsset.decimal(source, start, end)`.
fn decimal(source: &str, start: isize, end: isize) -> f64 {
    if start < 0 || end <= start {
        return f64::NAN;
    }
    let b = source.as_bytes();
    let (start, end) = (start as usize, end as usize);
    let mut i = start;
    if b[i] == b'-' {
        i += 1;
    }
    let mut digits = false;
    while i < end && b[i].is_ascii_digit() {
        digits = true;
        i += 1;
    }
    if i < end && b[i] == b'.' {
        i += 1;
        let mut fraction = false;
        while i < end && b[i].is_ascii_digit() {
            fraction = true;
            i += 1;
        }
        if !digits && !fraction {
            return f64::NAN;
        }
    } else if !digits {
        return f64::NAN;
    }
    if i < end && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < end && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exponent = i;
        while i < end && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exponent {
            return f64::NAN;
        }
    }
    if i != end {
        return f64::NAN;
    }
    jfmt::parse_double(&source[start..end]).unwrap_or(f64::NAN)
}

/// Match the `DECIMAL` regex greedily at `i`; returns the end offset.
fn match_decimal(b: &[u8], mut i: usize) -> Option<usize> {
    if i < b.len() && b[i] == b'-' {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i > digits_start {
        if i < b.len() && b[i] == b'.' {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
    } else {
        if i >= b.len() || b[i] != b'.' {
            return None;
        }
        let f = i + 1;
        let mut j = f;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == f {
            return None;
        }
        i = j;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let f = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > f {
            i = j;
        }
    }
    Some(i)
}

/// `DOT_ROOT.matcher(source).lookingAt()`: groups 1..7 and the match end.
fn dot_root(source: &str) -> Option<([&str; 7], usize)> {
    let b = source.as_bytes();
    let mut p = 0usize;
    let mut groups: [&str; 7] = [""; 7];
    let lit = |p: &mut usize, s: &str| -> bool {
        if b[*p..].starts_with(s.as_bytes()) {
            *p += s.len();
            true
        } else {
            false
        }
    };
    let pieces: [(&str, u8); 7] = [
        ("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 ", b'd'),
        (" ", b'd'),
        ("\" width=\"", b'd'),
        ("\" height=\"", b'd'),
        ("\"><g stroke=\"", b'c'),
        ("\" stroke-opacity=\"", b'd'),
        ("\" stroke-width=\"", b'd'),
    ];
    for (n, (literal, kind)) in pieces.iter().enumerate() {
        if !lit(&mut p, literal) {
            return None;
        }
        let start = p;
        if *kind == b'd' {
            p = match_decimal(b, p)?;
        } else {
            if p + 7 > b.len() || b[p] != b'#' || !b[p + 1..p + 7].iter().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c)) {
                return None;
            }
            p += 7;
        }
        groups[n] = &source[start..p];
    }
    if !lit(&mut p, "\">") {
        return None;
    }
    Some((groups, p))
}

/// Recognize only the exact, self-contained SVG emitted by the dot-grid action.
fn generated_dots(source: &str) -> Option<Asset> {
    if source.len() > MAX_BYTES {
        return None;
    }
    let (g, root_end) = dot_root(source)?;
    if g[0] != g[2] || g[1] != g[3] {
        return None;
    }
    let width = jfmt::parse_double(g[0]).ok()?;
    let height = jfmt::parse_double(g[1]).ok()?;
    let alpha = jfmt::parse_double(g[5]).ok()?;
    let stroke = jfmt::parse_double(g[6]).ok()?;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 || width > 1000000.0 || height > 1000000.0
        || !(0.0..=1.0).contains(&alpha) || !stroke.is_finite() || stroke <= 0.0 || stroke > 1000000.0
    {
        return None;
    }
    let color_literal = g[4];
    let mut offset = root_end as isize;
    let mut count = 0usize;
    let mut circles: Vec<f64> = Vec::with_capacity(96);
    while (offset as usize) < source.len() && source.as_bytes()[offset as usize..].starts_with(b"<circle ") {
        let mut position = after(source, offset, "<circle cx=\"");
        if position < 0 {
            return None;
        }
        let mut end = index_of_quote(source, position as usize);
        let x = decimal(source, position, end);
        position = if end < 0 { -1 } else { after(source, end, "\" cy=\"") };
        end = if position < 0 { -1 } else { index_of_quote(source, position as usize) };
        let y = decimal(source, position, end);
        position = if end < 0 { -1 } else { after(source, end, "\" r=\"") };
        end = if position < 0 { -1 } else { index_of_quote(source, position as usize) };
        let radius = decimal(source, position, end);
        position = if end < 0 { -1 } else { after(source, end, "\" fill=\"") };
        if position < 0 || !source.as_bytes()[position as usize..].starts_with(color_literal.as_bytes()) {
            return None;
        }
        offset = after(source, position + color_literal.len() as isize, "\"/>");
        if offset < 0 || !x.is_finite() || !y.is_finite() || !radius.is_finite() || x < 0.0 || y < 0.0 || radius <= 0.0
            || x > 1000000.0 || y > 1000000.0 || radius > 1000000.0
        {
            return None;
        }
        count += 1;
        if count > 9998 {
            return None; // Root and group also count toward the 10,000-node SVG limit.
        }
        circles.push(x);
        circles.push(y);
        circles.push(radius);
    }
    let offset = offset as usize;
    if count == 0 || !source.as_bytes()[offset..].starts_with(b"</g></svg>") || offset + 10 != source.len() {
        return None;
    }
    let group = source.find("<g stroke=").unwrap_or(0);
    let normalized = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"100%\" height=\"100%\">{}",
        g[0],
        g[1],
        &source[group..]
    );
    let color = i32::from_str_radix(&color_literal[1..], 16).ok()? | (0xff000000u32 as i32);
    let grid = DotGrid { width, height, stroke_width: stroke, stroke_opacity: alpha, color, circles };
    Some(Asset { size: [width, height], normalized, dots: Some(Arc::new(grid)) })
}

// ---------------------------------------------------------------------------
// Tree built from the (already well-formed) source

#[derive(Debug, Clone)]
enum Node {
    Element(Element),
    Text(String),
    CData(String),
    Comment(String),
    Pi,
}

#[derive(Debug, Clone)]
struct Element {
    qname: String,
    /// Attributes in qualified-name order (Xerces `AttributeMap` order).
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
    /// Resolved namespace of the element.
    ns: Option<String>,
    /// Resolved namespace per attribute (parallel to `attrs`).
    attr_ns: Vec<Option<String>>,
}

impl Element {
    fn local_name(&self) -> &str {
        local(&self.qname)
    }
    fn attr(&self, qname: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == qname).map(|(_, v)| v.as_str())
    }
    /// `setAttribute(name, value)`: replace in place or insert in sorted position.
    fn set_attr(&mut self, qname: &str, value: String) {
        if let Some(entry) = self.attrs.iter_mut().find(|(k, _)| k == qname) {
            entry.1 = value;
            return;
        }
        let at = self.attrs.iter().position(|(k, _)| utf16_cmp(qname, k) == std::cmp::Ordering::Less).unwrap_or(self.attrs.len());
        self.attrs.insert(at, (qname.to_string(), value));
        self.attr_ns.insert(at, None);
    }
}

fn local(qname: &str) -> &str {
    match qname.find(':') {
        Some(i) => &qname[i + 1..],
        None => qname,
    }
}

fn utf16_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

struct Doc {
    /// Document-level nodes in order (comments, PIs, the root element).
    nodes: Vec<Node>,
}

struct Tokenizer<'a> {
    s: &'a str,
    p: usize,
}

impl<'a> Tokenizer<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.p..]
    }
    fn starts(&self, lit: &str) -> bool {
        self.rest().starts_with(lit)
    }
    fn skip_ws(&mut self) {
        while let Some(c) = self.rest().chars().next() {
            if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
                self.p += 1;
            } else {
                break;
            }
        }
    }
    fn until(&mut self, lit: &str) -> &'a str {
        let rest = self.rest();
        match rest.find(lit) {
            Some(i) => {
                self.p += i + lit.len();
                &rest[..i]
            }
            None => {
                self.p = self.s.len();
                rest
            }
        }
    }
    fn name(&mut self) -> String {
        let rest = self.rest();
        let end = rest
            .find([' ', '\t', '\n', '\r', '/', '>', '='])
            .unwrap_or(rest.len());
        self.p += end;
        rest[..end].to_string()
    }
    fn document(&mut self) -> Doc {
        let mut nodes = Vec::new();
        let mut first = true;
        while self.p < self.s.len() {
            if self.starts("<?") {
                let body = self.until("?>");
                let is_decl = first && (body.starts_with("<?xml ") || body.starts_with("<?xml\t") || body.starts_with("<?xml\n") || body == "<?xml");
                if !is_decl {
                    nodes.push(Node::Pi);
                }
            } else if self.starts("<!--") {
                self.p += 4;
                nodes.push(Node::Comment(self.until("-->").to_string()));
            } else if self.starts("<") {
                nodes.push(Node::Element(self.element()));
            } else {
                // Document-level whitespace has no DOM node.
                let next = self.rest().find('<').unwrap_or(self.rest().len());
                self.p += next;
            }
            first = false;
        }
        Doc { nodes }
    }
    fn element(&mut self) -> Element {
        self.p += 1; // '<'
        let qname = self.name();
        let mut attrs: Vec<(String, String)> = Vec::new();
        loop {
            self.skip_ws();
            if self.starts("/>") {
                self.p += 2;
                return finish_element(qname, attrs, Vec::new());
            }
            if self.starts(">") {
                self.p += 1;
                break;
            }
            let name = self.name();
            self.skip_ws();
            self.p += 1; // '='
            self.skip_ws();
            let quote = &self.rest()[..1];
            self.p += 1;
            let raw = self.until(quote);
            attrs.push((name, decode(raw, true)));
        }
        let mut children = Vec::new();
        let mut text = String::new();
        loop {
            if self.p >= self.s.len() {
                break;
            }
            if self.starts("</") {
                flush_text(&mut text, &mut children);
                self.until(">");
                break;
            }
            if self.starts("<![CDATA[") {
                flush_text(&mut text, &mut children);
                self.p += 9;
                children.push(Node::CData(self.until("]]>").to_string()));
            } else if self.starts("<!--") {
                flush_text(&mut text, &mut children);
                self.p += 4;
                children.push(Node::Comment(self.until("-->").to_string()));
            } else if self.starts("<?") {
                flush_text(&mut text, &mut children);
                self.until("?>");
                children.push(Node::Pi);
            } else if self.starts("<") {
                flush_text(&mut text, &mut children);
                children.push(Node::Element(self.element()));
            } else {
                let rest = self.rest();
                let end = rest.find('<').unwrap_or(rest.len());
                text.push_str(&decode(&rest[..end], false));
                self.p += end;
            }
        }
        finish_element(qname, attrs, children)
    }
}

fn flush_text(text: &mut String, children: &mut Vec<Node>) {
    if !text.is_empty() {
        children.push(Node::Text(std::mem::take(text)));
    }
}

fn finish_element(qname: String, mut attrs: Vec<(String, String)>, children: Vec<Node>) -> Element {
    attrs.sort_by(|a, b| utf16_cmp(&a.0, &b.0));
    let n = attrs.len();
    Element { qname, attrs, children, ns: None, attr_ns: vec![None; n] }
}

/// Entity/char-reference decoding; attribute values also get whitespace normalization.
fn decode(raw: &str, attribute: bool) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find(|c: char| c == '&' || (attribute && (c == '\t' || c == '\n' || c == '\r'))) {
        out.push_str(&rest[..i]);
        let c = rest.as_bytes()[i];
        if c != b'&' {
            out.push(' ');
            rest = &rest[i + 1..];
            continue;
        }
        let tail = &rest[i + 1..];
        let semi = tail.find(';').unwrap_or(tail.len());
        let name = &tail[..semi];
        let decoded = match name {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "apos" => Some('\''),
            "quot" => Some('"'),
            _ if name.starts_with("#x") => u32::from_str_radix(&name[2..], 16).ok().and_then(char::from_u32),
            _ if name.starts_with('#') => name[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(ch) => out.push(ch),
            None => {
                out.push('&');
                out.push_str(name);
                out.push(';');
            }
        }
        rest = &tail[(semi + 1).min(tail.len())..];
    }
    out.push_str(rest);
    out
}

/// Namespace resolution with in-scope prefix bindings.
fn resolve(e: &mut Element, scope: &mut Vec<(String, String)>) -> Result<()> {
    let mark = scope.len();
    for (k, v) in &e.attrs {
        if k == "xmlns" {
            scope.push((String::new(), v.clone()));
        } else if let Some(p) = k.strip_prefix("xmlns:") {
            scope.push((p.to_string(), v.clone()));
        }
    }
    let lookup = |scope: &Vec<(String, String)>, prefix: &str| -> Option<String> {
        if prefix == "xml" {
            return Some(XML_NS.to_string());
        }
        scope.iter().rev().find(|(p, _)| p == prefix).map(|(_, u)| u.clone()).filter(|u| !u.is_empty())
    };
    e.ns = match e.qname.find(':') {
        Some(i) => lookup(scope, &e.qname[..i]),
        None => lookup(scope, ""),
    };
    for (index, (k, _)) in e.attrs.iter().enumerate() {
        e.attr_ns[index] = if k == "xmlns" || k.starts_with("xmlns:") {
            Some(XMLNS_NS.to_string())
        } else {
            match k.find(':') {
                Some(i) => lookup(scope, &k[..i]),
                None => None,
            }
        };
    }
    for child in &mut e.children {
        if let Node::Element(c) = child {
            resolve(c, scope)?;
        }
    }
    scope.truncate(mark);
    Ok(())
}

// ---------------------------------------------------------------------------
// Validation rules

fn visit_node(node: &Node, depth: usize, count: &mut usize) -> Result<()> {
    match node {
        Node::Pi => require(false, "SVG processing instructions/entities are unsupported"),
        Node::Element(e) => visit_element(e, depth, count),
        _ => Ok(()),
    }
}

fn visit_element(e: &Element, depth: usize, count: &mut usize) -> Result<()> {
    let ok = depth <= 64 && {
        *count += 1;
        *count <= 10000
    };
    require(ok, "SVG is too complex")?;
    let tag = e.local_name();
    if !(e.ns.as_deref() == Some(SVG_NS) && TAGS.contains(&tag)) {
        return Err(Error::illegal_argument(format!("Unsupported SVG element: {}", tag)));
    }
    for (index, (qname, value)) in e.attrs.iter().enumerate() {
        let name = local(qname);
        let ans = e.attr_ns[index].as_deref();
        if ans == Some(XMLNS_NS) {
            continue;
        }
        require(
            ans.is_none() || (ans == Some(XLINK_NS) && name == "href") || (ans == Some(XML_NS) && name == "space"),
            "Unsupported SVG attribute namespace",
        )?;
        if !ATTRS.contains(&name) {
            return Err(Error::illegal_argument(format!("Unsupported SVG attribute: {}", name)));
        }
        if name == "href" {
            require(local_fragment(value), "SVG references must be local fragments")?;
        }
        if name == "style" || name == "fill" || name == "stroke" || name == "clip-path" || name.starts_with("marker-") {
            css(value)?;
        }
    }
    if tag == "style" {
        let mut text = String::new();
        text_content(e, &mut text);
        css(&text)?;
    }
    for child in &e.children {
        visit_node(child, depth + 1, count)?;
    }
    Ok(())
}

fn text_content(e: &Element, out: &mut String) {
    for child in &e.children {
        match child {
            Node::Text(t) | Node::CData(t) => out.push_str(t),
            Node::Element(c) => text_content(c, out),
            _ => {}
        }
    }
}

/// `value.matches("#[A-Za-z_][A-Za-z0-9_.:-]*")`.
fn local_fragment(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() >= 2
        && b[0] == b'#'
        && (b[1].is_ascii_alphabetic() || b[1] == b'_')
        && b[2..].iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b':' | b'-'))
}

fn is_java_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// Match `url\(\s*['"]?#[a-z_][a-z0-9_.:-]*['"]?\s*\)` at `i`.
fn match_local_url(b: &[u8], i: usize) -> Option<usize> {
    if !b[i..].starts_with(b"url(") {
        return None;
    }
    let mut j = i + 4;
    while j < b.len() && is_java_space(b[j]) {
        j += 1;
    }
    if j < b.len() && (b[j] == b'\'' || b[j] == b'"') {
        j += 1;
    }
    if j >= b.len() || b[j] != b'#' {
        return None;
    }
    j += 1;
    if j >= b.len() || !(b[j].is_ascii_lowercase() || b[j] == b'_') {
        return None;
    }
    j += 1;
    while j < b.len() && (b[j].is_ascii_lowercase() || b[j].is_ascii_digit() || matches!(b[j], b'_' | b'.' | b':' | b'-')) {
        j += 1;
    }
    if j < b.len() && (b[j] == b'\'' || b[j] == b'"') {
        j += 1;
    }
    while j < b.len() && is_java_space(b[j]) {
        j += 1;
    }
    if j < b.len() && b[j] == b')' {
        Some(j + 1)
    } else {
        None
    }
}

fn css(value: &str) -> Result<()> {
    // replaceAll("(?s)/\\*.*?\\*/", "")
    let mut stripped = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(open) = rest.find("/*") {
        match rest[open + 2..].find("*/") {
            Some(close) => {
                stripped.push_str(&rest[..open]);
                rest = &rest[open + 2 + close + 2..];
            }
            None => break,
        }
    }
    stripped.push_str(rest);
    let s = stripped.to_lowercase();
    require(!s.contains('\\') && !s.contains('@') && !s.contains("/*"), "SVG CSS escapes/imports are unsupported")?;
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if let Some(end) = match_local_url(b, i) {
            i = end;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    let s = String::from_utf8_lossy(&out);
    require(!s.contains("url") && !s.contains("expression("), "SVG resources must be local fragments")
}

fn collect<'a>(e: &'a Element, local_name: &str, out: &mut Vec<&'a Element>) {
    for child in &e.children {
        if let Node::Element(c) = child {
            if c.ns.as_deref() == Some(SVG_NS) && c.local_name() == local_name {
                out.push(c);
            }
            collect(c, local_name, out);
        }
    }
}

/// Matplotlib reuses glyph and marker paths. Restrict targets to paths so use cannot recurse.
fn validate_uses(root: &Element) -> Result<()> {
    let mut paths = Vec::new();
    collect(root, "path", &mut paths);
    let ids: std::collections::HashSet<&str> = paths.iter().filter_map(|p| p.attr("id")).collect();
    let mut uses = Vec::new();
    collect(root, "use", &mut uses);
    for u in uses {
        let href = match u.attr("href") {
            Some(h) => h,
            None => u
                .attrs
                .iter()
                .enumerate()
                .find(|(i, (k, _))| local(k) == "href" && u.attr_ns[*i].as_deref() == Some(XLINK_NS))
                .map(|(_, (_, v))| v.as_str())
                .unwrap_or(""),
        };
        require(href.starts_with('#') && ids.contains(&href[1..]), "SVG use must reference a local path")?;
    }
    Ok(())
}

/// Java `String.split("[\\s,]+")` of an already trimmed string.
fn split_view_box(s: &str) -> Vec<&str> {
    let is_sep = |c: char| matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ',');
    let mut parts: Vec<&str> = Vec::new();
    let mut start = 0;
    let mut in_sep = false;
    for (i, c) in s.char_indices() {
        if is_sep(c) {
            if !in_sep {
                parts.push(&s[start..i]);
                in_sep = true;
            }
        } else if in_sep {
            start = i;
            in_sep = false;
        }
    }
    if !in_sep {
        parts.push(&s[start..]);
    } else {
        parts.push("");
    }
    // A leading empty string stays only for a positive-width match at 0 (s non-empty);
    // trailing empty strings are removed.
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    if s.is_empty() {
        return vec![""];
    }
    parts
}

/// `Double.parseDouble(s.trim().replaceFirst("px$",""))`.
fn length(s: &str) -> Result<f64> {
    let t = jfmt::java_trim(s);
    let t = t.strip_suffix("px").unwrap_or(t);
    jfmt::parse_double(t)
}

fn validate_document(source: &str) -> Result<Asset> {
    if let Err(e) = roxmltree::Document::parse(source) {
        return Err(Error::illegal_argument(format!("Invalid SVG: {}", e)));
    }
    let normalized_source = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut doc = Tokenizer { s: &normalized_source, p: 0 }.document();
    let root_index = doc.nodes.iter().position(|n| matches!(n, Node::Element(_))).ok_or_else(|| Error::illegal_argument("Invalid SVG: no root element"))?;
    if let Node::Element(root) = &mut doc.nodes[root_index] {
        resolve(root, &mut Vec::new())?;
    }
    let inner = |doc: &mut Doc| -> Result<Asset> {
        let root = match &doc.nodes[root_index] {
            Node::Element(e) => e,
            _ => unreachable!(),
        };
        require(root.local_name() == "svg", "Expected SVG root")?;
        let mut count = 0;
        for node in &doc.nodes {
            visit_node(node, 1, &mut count)?;
        }
        validate_uses(root)?;
        let size: [f64; 2];
        let has_view_box = root.attr("viewBox").is_some();
        if let Some(view_box) = root.attr("viewBox") {
            let values = split_view_box(jfmt::java_trim(view_box));
            require(values.len() == 4, "SVG viewBox needs four numbers")?;
            let mut b = [0f64; 4];
            for i in 0..4 {
                b[i] = jfmt::parse_double(values[i])?;
                require(b[i].is_finite() && b[i].abs() <= 1000000.0, "Invalid SVG viewBox")?;
            }
            size = [b[2], b[3]];
        } else {
            size = [length(root.attr("width").unwrap_or(""))?, length(root.attr("height").unwrap_or(""))?];
        }
        require(
            size[0] > 0.0 && size[1] > 0.0 && size[0] <= 1000000.0 && size[1] <= 1000000.0,
            "SVG needs positive viewBox or pixel width/height",
        )?;
        let root = match &mut doc.nodes[root_index] {
            Node::Element(e) => e,
            _ => unreachable!(),
        };
        if !has_view_box {
            root.set_attr("viewBox", format!("0 0 {} {}", jfmt::double(size[0]), jfmt::double(size[1])));
        }
        root.set_attr("width", "100%".to_string());
        root.set_attr("height", "100%".to_string());
        let mut out = String::with_capacity(source.len() + 64);
        let mut scope = Namespaces::new();
        serialize_element(root, &mut out, &mut scope, 1);
        Ok(Asset { size, normalized: out, dots: None })
    };
    inner(&mut doc).map_err(|e| Error::new(e.kind, format!("Invalid SVG: {}", e.message())))
}

// ---------------------------------------------------------------------------
// Xalan ToStream serialization

/// `NamespaceMappings`: prefix → (uri, declaration depth) stacks.
struct Namespaces {
    stack: Vec<(String, String, usize)>,
}

impl Namespaces {
    fn new() -> Namespaces {
        Namespaces { stack: vec![(String::new(), String::new(), 0), ("xml".into(), XML_NS.into(), 0)] }
    }
    /// `pushNamespace(prefix, uri, depth)`.
    fn push(&mut self, prefix: &str, uri: &str, depth: usize) -> bool {
        if prefix.starts_with("xml") {
            return false;
        }
        if let Some((_, u, d)) = self.stack.iter().rev().find(|(p, _, _)| p == prefix) {
            if u == uri || *d == depth {
                return false;
            }
        }
        self.stack.push((prefix.to_string(), uri.to_string(), depth));
        true
    }
    fn pop(&mut self, depth: usize) {
        while matches!(self.stack.last(), Some((_, _, d)) if *d == depth && depth > 0) {
            self.stack.pop();
        }
    }
}

fn char_ref(out: &mut String, c: char) {
    out.push_str("&#");
    out.push_str(&(c as u32).to_string());
    out.push(';');
}

fn write_attr_value(out: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c if (c as u32) < 0x20 => char_ref(out, c),
            c if (c as u32) > 0xFFFF => char_ref(out, c),
            c => out.push(c),
        }
    }
}

fn write_text(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            '\n' | '\t' => out.push(c),
            c if (c as u32) < 0x20 || (0x7F..=0x9F).contains(&(c as u32)) => char_ref(out, c),
            c if (c as u32) > 0xFFFF => char_ref(out, c),
            c => out.push(c),
        }
    }
}

/// `ToStream.cdata` + `writeNormalizedChars(isCData=true)`. Supplementary
/// characters are written raw (UTF-8 encodes the pair), but a section that
/// starts with one does not open its `<![CDATA[` bracket until the next char.
fn write_cdata(out: &mut String, text: &str) {
    let chars: Vec<char> = text.chars().collect();
    let mut open = false;
    if (chars[0] as u32) <= 0xFFFF {
        out.push_str("<![CDATA[");
        open = true;
    }
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (c as u32) > 0xFFFF {
            out.push(c);
        } else if i + 2 < chars.len() && c == ']' && chars[i + 1] == ']' && chars[i + 2] == '>' {
            out.push_str("]]]]><![CDATA[>");
            i += 2;
        } else {
            if !open {
                out.push_str("<![CDATA[");
                open = true;
            }
            out.push(c);
        }
        i += 1;
    }
    if open {
        out.push_str("]]>");
    }
}

fn serialize_element(e: &Element, out: &mut String, ns: &mut Namespaces, depth: usize) {
    out.push('<');
    out.push_str(&e.qname);
    let mut attrs: Vec<(String, &str)> = Vec::new();
    for (qname, value) in &e.attrs {
        if qname.starts_with("xmlns") {
            let prefix = match qname.rfind(':') {
                Some(i) if i > 0 => &qname[i + 1..],
                _ => "",
            };
            if ns.push(prefix, value, depth) {
                if prefix.is_empty() {
                    attrs.push(("xmlns".into(), value));
                } else if !value.is_empty() {
                    attrs.push((format!("xmlns:{}", prefix), value));
                }
            }
        }
    }
    for (index, (qname, value)) in e.attrs.iter().enumerate() {
        if qname.starts_with("xmlns") {
            continue;
        }
        if let (Some(uri), Some(i)) = (e.attr_ns[index].as_deref(), qname.rfind(':')) {
            let prefix = &qname[..i];
            if ns.push(prefix, uri, depth) {
                attrs.push((format!("xmlns:{}", prefix), uri));
            }
        }
        attrs.push((qname.clone(), value));
    }
    if let Some(uri) = e.ns.as_deref() {
        let prefix = match e.qname.rfind(':') {
            Some(i) if i > 0 => &e.qname[..i],
            _ => "",
        };
        if ns.push(prefix, uri, depth) {
            attrs.push((if prefix.is_empty() { "xmlns".into() } else { format!("xmlns:{}", prefix) }, uri));
        }
    }
    for (name, value) in &attrs {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        write_attr_value(out, value);
        out.push('"');
    }
    let mut open = true;
    for child in &e.children {
        let produces = match child {
            Node::CData(t) | Node::Text(t) => !t.is_empty(),
            Node::Pi => false,
            _ => true,
        };
        if produces && open {
            out.push('>');
            open = false;
        }
        match child {
            Node::Element(c) => serialize_element(c, out, ns, depth + 1),
            Node::Text(t) => write_text(out, t),
            Node::CData(t) if !t.is_empty() => write_cdata(out, t),
            Node::Comment(t) => {
                out.push_str("<!--");
                out.push_str(t);
                out.push_str("-->");
            }
            _ => {}
        }
    }
    if open {
        out.push_str("/>");
    } else {
        out.push_str("</");
        out.push_str(&e.qname);
        out.push('>');
    }
    ns.pop(depth);
}

// ---------------------------------------------------------------------------
// Public API

fn validate_locked(cache: &mut IndexMap<String, Arc<Asset>>, source: &str) -> Result<[f64; 2]> {
    if let Some(cached) = cache_get(cache, source) {
        return Ok(cached.size);
    }
    if let Some(dots) = generated_dots(source) {
        let size = dots.size;
        cache_put(cache, source, Arc::new(dots));
        return Ok(size);
    }
    require(source.len() <= MAX_BYTES, "SVG exceeds 256 KiB")?;
    let upper = source.to_uppercase();
    require(!upper.contains("<!DOCTYPE") && !upper.contains("<!ENTITY"), "SVG DTDs and entities are unsupported")?;
    let asset = validate_document(source)?;
    let size = asset.size;
    cache_put(cache, source, Arc::new(asset));
    Ok(size)
}

/// `SvgAsset.validate(source)`: intrinsic viewBox dimensions. No external
/// resources or XML entities.
pub fn validate(source: &str) -> Result<crate::geometry::Size> {
    with_cache(|cache| validate_locked(cache, source)).map(|s| crate::geometry::Size { width: s[0], height: s[1] })
}

/// `SvgAsset.viewport(source)`: the normalized SVG with a 100% viewport wrapper.
pub fn viewport(source: &str) -> Result<String> {
    with_cache(|cache| {
        validate_locked(cache, source)?;
        Ok(cache_get(cache, source).map(|a| a.normalized.clone()).unwrap_or_default())
    })
}

/// `SvgAsset.dotGrid(source)`: parsed coordinates for the generated dot-grid
/// profile, or `None` for ordinary SVG.
pub fn dot_grid(source: &str) -> Result<Option<Arc<DotGrid>>> {
    with_cache(|cache| {
        let asset = match cache_get(cache, source) {
            Some(a) => a,
            None => {
                validate_locked(cache, source)?;
                cache_get(cache, source).expect("validated asset is cached")
            }
        };
        Ok(asset.dots.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_like_java() {
        let s = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"20px\" id=\"a\"><g fill=\"red\" class='x'>t &amp; &lt; &gt; \" ' \u{e9}<!-- c --><rect x=\"1\" y=\"2\"/></g></svg>";
        assert_eq!(
            viewport(s).unwrap(),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" height=\"100%\" id=\"a\" viewBox=\"0 0 10.0 20.0\" width=\"100%\"><g class=\"x\" fill=\"red\">t &amp; &lt; &gt; \" ' \u{e9}<!-- c --><rect x=\"1\" y=\"2\"/></g></svg>"
        );
        assert_eq!(validate(s).unwrap(), crate::geometry::Size { width: 10.0, height: 20.0 });
    }

    #[test]
    fn errors_match_java() {
        let e = |s: &str| validate(s).unwrap_err().message().to_string();
        assert_eq!(e("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1\"/>"), "Invalid SVG: SVG viewBox needs four numbers");
        assert_eq!(e("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\"/>"), "Invalid SVG: empty String");
        assert_eq!(e("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"><script/></svg>"), "Invalid SVG: Unsupported SVG element: script");
        assert_eq!(e("<!DOCTYPE svg><svg/>"), "SVG DTDs and entities are unsupported");
    }

    #[test]
    fn dot_grid_fast_path() {
        let s = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100.0 50.0\" width=\"100.0\" height=\"50.0\"><g stroke=\"#777777\" stroke-opacity=\"1.0\" stroke-width=\"1.0\"><circle cx=\"10.0\" cy=\"10.0\" r=\"2.0\" fill=\"#777777\"/></g></svg>";
        assert_eq!(validate(s).unwrap(), crate::geometry::Size { width: 100.0, height: 50.0 });
        let grid = dot_grid(s).unwrap().unwrap();
        assert_eq!(grid.circles().collect::<Vec<_>>(), vec![DotCircle { center: DocPoint { x: 10.0, y: 10.0 }, radius: Radius(2.0) }]);
        assert_eq!(grid.color, 0xff777777u32 as i32);
        assert!(viewport(s).unwrap().starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100.0 50.0\" width=\"100%\" height=\"100%\"><g stroke="));
    }
}
