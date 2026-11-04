//! Port of `Json.java`: small strict JSON codec plus the `Map`/`List` helpers.
//!
//! Java's `Map<String,Object>`/`List<Object>` model is `serde_json::Value`
//! with `preserve_order` (insertion order like `LinkedHashMap`). Java `Long`
//! and `Integer` values are `i64` numbers; Java `Double` values are `f64`
//! numbers. Never call `Map::remove` (it reorders); use `shift_remove`.

use crate::error::{Error, Result};
use crate::jfmt;

pub use serde_json::Value;
/// Java `Map<String,Object>` (a `LinkedHashMap`).
pub type Map = serde_json::Map<String, Value>;
/// Java `List<Object>`.
pub type List = Vec<Value>;

/// Java `Json.object(k1,v1,k2,v2,...)`.
#[macro_export]
macro_rules! object {
    () => { $crate::json::Map::new() };
    ($($k:expr => $v:expr),+ $(,)?) => {{
        let mut m = $crate::json::Map::new();
        $( m.insert(::std::string::String::from($k), $crate::json::Value::from($v)); )+
        m
    }};
}

/// Number value from a Java `double`. Non-finite values become `null`
/// (Java kept them in memory and later rejected them as non-finite).
#[inline]
pub fn double(v: f64) -> Value {
    serde_json::Number::from_f64(v).map(Value::Number).unwrap_or(Value::Null)
}

/// Number value from a Java `float`: the double whose Java `Double.toString`
/// equals the float's `Float.toString` (how a Java `Float` prints in JSON).
pub fn from_float(v: f32) -> Value {
    match jfmt::float(v).parse::<f64>() {
        Ok(d) => double(d),
        Err(_) => Value::Null,
    }
}

/// `Json.map(value)`.
pub fn map(value: &Value) -> Result<&Map> {
    value.as_object().ok_or_else(|| Error::illegal_argument("Expected JSON object"))
}
/// `Json.map(value)` for mutation.
pub fn map_mut(value: &mut Value) -> Result<&mut Map> {
    value.as_object_mut().ok_or_else(|| Error::illegal_argument("Expected JSON object"))
}
/// `Json.map(m.get(key))` where a missing key is `null`.
pub fn map_at<'a>(m: &'a Map, key: &str) -> Result<&'a Map> {
    match m.get(key) {
        Some(Value::Object(o)) => Ok(o),
        _ => Err(Error::illegal_argument("Expected JSON object")),
    }
}
/// `Json.list(value)`.
pub fn list(value: &Value) -> Result<&List> {
    value.as_array().ok_or_else(|| Error::illegal_argument("Expected JSON array"))
}
/// `Json.list(m.get(key))` where a missing key is `null`.
pub fn list_at<'a>(m: &'a Map, key: &str) -> Result<&'a List> {
    match m.get(key) {
        Some(Value::Array(a)) => Ok(a),
        _ => Err(Error::illegal_argument("Expected JSON array")),
    }
}
/// Mutable `Json.list(m.get(key))`.
pub fn list_at_mut<'a>(m: &'a mut Map, key: &str) -> Result<&'a mut List> {
    match m.get_mut(key) {
        Some(Value::Array(a)) => Ok(a),
        _ => Err(Error::illegal_argument("Expected JSON array")),
    }
}
/// Mutable `Json.map(m.get(key))`.
pub fn map_at_mut<'a>(m: &'a mut Map, key: &str) -> Result<&'a mut Map> {
    match m.get_mut(key) {
        Some(Value::Object(o)) => Ok(o),
        _ => Err(Error::illegal_argument("Expected JSON object")),
    }
}

/// `Json.string(m, key)`.
pub fn string<'a>(m: &'a Map, key: &str) -> Result<&'a str> {
    match m.get(key) {
        Some(Value::String(s)) => Ok(s),
        _ => Err(Error::illegal_argument(format!("Expected string: {}", key))),
    }
}

/// `Json.number(m, key)`: finite number as double.
pub fn number(m: &Map, key: &str) -> Result<f64> {
    match m.get(key) {
        Some(Value::Number(n)) => {
            let d = number_double(n);
            if d.is_finite() {
                Ok(d)
            } else {
                Err(Error::illegal_argument(format!("Expected finite number: {}", key)))
            }
        }
        _ => Err(Error::illegal_argument(format!("Expected finite number: {}", key))),
    }
}

/// `Number.doubleValue()` of a JSON number.
#[inline]
pub fn number_double(n: &serde_json::Number) -> f64 {
    if let Some(i) = n.as_i64() {
        i as f64
    } else if let Some(u) = n.as_u64() {
        u as f64
    } else {
        n.as_f64().unwrap_or(f64::NAN)
    }
}

/// `((Number) value).doubleValue()`: `NullPointerException` for null/missing,
/// `ClassCastException` for other types.
pub fn as_double(value: Option<&Value>) -> Result<f64> {
    match value {
        Some(Value::Number(n)) => Ok(number_double(n)),
        None | Some(Value::Null) => Err(Error::null_pointer()),
        Some(other) => Err(class_cast(other, "java.lang.Number")),
    }
}

/// `((Number) value).intValue()`: `Long.intValue` wraps, `Double.intValue` saturates.
pub fn as_int(value: Option<&Value>) -> Result<i32> {
    match value {
        Some(Value::Number(n)) => Ok(number_int(n)),
        None | Some(Value::Null) => Err(Error::null_pointer()),
        Some(other) => Err(class_cast(other, "java.lang.Number")),
    }
}

/// `((Number) value).longValue()`.
pub fn as_long(value: Option<&Value>) -> Result<i64> {
    match value {
        Some(Value::Number(n)) => Ok(number_long(n)),
        None | Some(Value::Null) => Err(Error::null_pointer()),
        Some(other) => Err(class_cast(other, "java.lang.Number")),
    }
}

/// `Number.intValue()`.
pub fn number_int(n: &serde_json::Number) -> i32 {
    if let Some(i) = n.as_i64() {
        i as i32
    } else if let Some(u) = n.as_u64() {
        u as i64 as i32
    } else {
        n.as_f64().map(|d| d as i32).unwrap_or(0)
    }
}

/// `Number.longValue()`.
fn number_long(n: &serde_json::Number) -> i64 {
    if let Some(i) = n.as_i64() {
        i
    } else if let Some(u) = n.as_u64() {
        u as i64
    } else {
        n.as_f64().map(|d| d as i64).unwrap_or(0)
    }
}

/// `(Boolean) value` unboxed: `NullPointerException` for null, `ClassCastException` otherwise.
pub fn as_bool(value: Option<&Value>) -> Result<bool> {
    match value {
        Some(Value::Bool(b)) => Ok(*b),
        None | Some(Value::Null) => Err(Error::null_pointer()),
        Some(other) => Err(class_cast(other, "java.lang.Boolean")),
    }
}

/// `(String) value` (null allowed → `None`).
pub fn as_string_opt(value: Option<&Value>) -> Result<Option<&str>> {
    match value {
        Some(Value::String(s)) => Ok(Some(s)),
        None | Some(Value::Null) => Ok(None),
        Some(other) => Err(class_cast(other, "java.lang.String")),
    }
}

/// True for `instanceof Number`.
#[inline]
pub fn is_number(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Number(_)))
}

/// `Boolean.TRUE.equals(value)`.
#[inline]
pub fn is_true(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
}

/// `Objects.equals(a, b)` for optional values, where Java `null` and a
/// missing key are the same.
pub fn objects_equal(a: Option<&Value>, b: Option<&Value>) -> bool {
    let a = a.filter(|v| !v.is_null());
    let b = b.filter(|v| !v.is_null());
    a == b
}

fn class_cast(value: &Value, target: &str) -> Error {
    let source = match value {
        Value::Null => "null",
        Value::Bool(_) => "java.lang.Boolean",
        Value::Number(n) => {
            if n.is_f64() {
                "java.lang.Double"
            } else {
                "java.lang.Long"
            }
        }
        Value::String(_) => "java.lang.String",
        Value::Array(_) => "java.util.ArrayList",
        Value::Object(_) => "java.util.LinkedHashMap",
    };
    Error::class_cast(format!("class {} cannot be cast to class {}", source, target))
}

/// `Json.copy(value)` (deep copy).
#[inline]
pub fn copy(value: &Value) -> Value {
    value.clone()
}

/// `Json.equivalent(a, b)`: numbers compare by decimal value.
pub fn equivalent(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => decimal_cmp(x, y) == std::cmp::Ordering::Equal,
        (Value::Object(x), Value::Object(y)) => {
            if x.len() != y.len() || !x.keys().all(|k| y.contains_key(k)) {
                return false;
            }
            x.iter().all(|(k, v)| equivalent(v, &y[k]))
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| equivalent(p, q))
        }
        _ => a == b,
    }
}

/// `new BigDecimal(a.toString()).compareTo(new BigDecimal(b.toString()))`.
fn decimal_cmp(a: &serde_json::Number, b: &serde_json::Number) -> std::cmp::Ordering {
    let x = Decimal::parse(&number_string(a));
    let y = Decimal::parse(&number_string(b));
    x.cmp(&y)
}

/// Exact decimal from a Java number string (`-1.25E-7`).
#[derive(Debug)]
struct Decimal {
    negative: bool,
    /// Significant digits without leading/trailing zeros; empty for zero.
    digits: Vec<u8>,
    /// Value = 0.digits × 10^exponent.
    exponent: i64,
}

impl Decimal {
    fn parse(s: &str) -> Decimal {
        let (negative, s) = match s.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, s),
        };
        let (mantissa, exp) = match s.find(['e', 'E']) {
            Some(i) => (&s[..i], s[i + 1..].parse::<i64>().unwrap_or(0)),
            None => (s, 0),
        };
        let (int_part, frac_part) = match mantissa.find('.') {
            Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
            None => (mantissa, ""),
        };
        let mut digits: Vec<u8> = int_part.bytes().chain(frac_part.bytes()).map(|c| c - b'0').collect();
        let mut exponent = int_part.len() as i64 + exp;
        while digits.first() == Some(&0) {
            digits.remove(0);
            exponent -= 1;
        }
        while digits.last() == Some(&0) {
            digits.pop();
        }
        if digits.is_empty() {
            exponent = 0;
        }
        Decimal { negative: negative && !digits.is_empty(), digits, exponent }
    }
    fn cmp(&self, other: &Decimal) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        let sign = |d: &Decimal| if d.digits.is_empty() { 0 } else if d.negative { -1 } else { 1 };
        let (sa, sb) = (sign(self), sign(other));
        if sa != sb {
            return sa.cmp(&sb);
        }
        if sa == 0 {
            return Equal;
        }
        let magnitude = if self.exponent != other.exponent {
            self.exponent.cmp(&other.exponent)
        } else {
            self.digits.cmp(&other.digits)
        };
        if sa < 0 { magnitude.reverse() } else { magnitude }
    }
}

/// Java `Number.toString()` of a JSON number.
fn number_string(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        i.to_string()
    } else if let Some(u) = n.as_u64() {
        u.to_string()
    } else {
        jfmt::double(n.as_f64().unwrap_or(f64::NAN))
    }
}

/// Java `String.valueOf(value)` for JSON data (`null`, `1`, `1.0`, `{a=1}`, `[1, 2]`).
pub fn java_string(value: Option<&Value>) -> String {
    let mut out = String::new();
    append_java_string(&mut out, value.unwrap_or(&Value::Null));
    out
}

fn append_java_string(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number_string(n)),
        Value::String(s) => out.push_str(s),
        Value::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                append_java_string(out, v);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(k);
                out.push('=');
                append_java_string(out, v);
            }
            out.push('}');
        }
    }
}

/// Java `String.length()` (UTF-16 code units).
#[inline]
pub fn jlen(s: &str) -> usize {
    if s.is_ascii() { s.len() } else { s.encode_utf16().count() }
}

/// `Json.write(value)`.
pub fn write(value: &Value) -> String {
    let mut out = String::new();
    append(&mut out, value);
    out
}

/// `Json.write(map)`.
pub fn write_map(value: &Map) -> String {
    let mut out = String::new();
    append_map(&mut out, value);
    out
}

/// `Json.write(list)`.
pub fn write_list(value: &[Value]) -> String {
    let mut out = String::new();
    append_list(&mut out, value);
    out
}

/// Append the JSON encoding of a string (exact Java escaping rules).
fn append_string(out: &mut String, text: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    let mut start = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'"' || b == b'\\' || b < 32 {
            out.push_str(&text[start..i]);
            if b < 32 {
                out.push_str("\\u00");
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 15) as usize] as char);
            } else {
                out.push('\\');
                out.push(b as char);
            }
            start = i + 1;
        }
    }
    out.push_str(&text[start..]);
    out.push('"');
}

fn append_map(out: &mut String, m: &Map) {
    out.push('{');
    let mut comma = false;
    for (k, v) in m {
        if comma {
            out.push(',');
        }
        append_string(out, k);
        out.push(':');
        append(out, v);
        comma = true;
    }
    out.push('}');
}

fn append_list(out: &mut String, a: &[Value]) {
    out.push('[');
    let mut comma = false;
    for v in a {
        if comma {
            out.push(',');
        }
        append(out, v);
        comma = true;
    }
    out.push(']');
}

fn append(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::String(s) => append_string(out, s),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else {
                jfmt::append_double(out, n.as_f64().unwrap_or(f64::NAN));
            }
        }
        Value::Object(m) => append_map(out, m),
        Value::Array(a) => append_list(out, a),
    }
}

/// `Json.parse(text)`.
pub fn parse(text: &str) -> Result<Value> {
    let mut p = Parser { s: text, b: text.as_bytes(), i: 0 };
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err(p.error());
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    /// Byte offset; error messages report the Java (UTF-16) offset.
    i: usize,
}

impl<'a> Parser<'a> {
    fn utf16_offset(&self, byte: usize) -> usize {
        let mut end = byte.min(self.s.len());
        while !self.s.is_char_boundary(end) {
            end -= 1;
        }
        jlen(&self.s[..end])
    }
    fn error(&self) -> Error {
        self.error_at_units(self.utf16_offset(self.i))
    }
    fn error_at_units(&self, offset: usize) -> Error {
        Error::illegal_argument(format!("Invalid JSON at offset {}", offset))
    }
    fn ws(&mut self) {
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c != b' ' && c != b'\t' && c != b'\r' && c != b'\n' {
                break;
            }
            self.i += 1;
        }
    }
    fn take(&mut self, c: u8) -> bool {
        self.ws();
        if self.i < self.b.len() && self.b[self.i] == c {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn value(&mut self, depth: i32) -> Result<Value> {
        if depth > 64 {
            return Err(self.error());
        }
        self.ws();
        if self.i == self.b.len() {
            return Err(self.error());
        }
        let c = self.b[self.i];
        if c == b'"' {
            return Ok(Value::String(self.str()?));
        }
        if self.take(b'{') {
            let mut m = Map::new();
            if self.take(b'}') {
                return Ok(Value::Object(m));
            }
            loop {
                self.ws();
                let k = self.str()?;
                if !self.take(b':') || m.contains_key(&k) {
                    return Err(self.error());
                }
                let v = self.value(depth + 1)?;
                m.insert(k, v);
                if !self.take(b',') {
                    break;
                }
            }
            if !self.take(b'}') {
                return Err(self.error());
            }
            return Ok(Value::Object(m));
        }
        if self.take(b'[') {
            let mut a = Vec::new();
            if self.take(b']') {
                return Ok(Value::Array(a));
            }
            loop {
                a.push(self.value(depth + 1)?);
                if !self.take(b',') {
                    break;
                }
            }
            if !self.take(b']') {
                return Err(self.error());
            }
            return Ok(Value::Array(a));
        }
        let rest = &self.b[self.i..];
        if c == b't' && rest.starts_with(b"true") {
            self.i += 4;
            return Ok(Value::Bool(true));
        }
        if c == b'f' && rest.starts_with(b"false") {
            self.i += 5;
            return Ok(Value::Bool(false));
        }
        if c == b'n' && rest.starts_with(b"null") {
            self.i += 4;
            return Ok(Value::Null);
        }
        self.number()
    }
    fn number(&mut self) -> Result<Value> {
        let b = self.b;
        let start = self.i;
        let len = b.len();
        if self.i < len && b[self.i] == b'-' {
            self.i += 1;
        }
        if self.i == len {
            return Err(self.error());
        }
        if b[self.i] == b'0' {
            self.i += 1;
        } else {
            let first = self.i;
            while self.i < len && b[self.i].is_ascii_digit() {
                self.i += 1;
            }
            if self.i == first {
                return Err(self.error());
            }
        }
        let mut decimal = false;
        if self.i < len && b[self.i] == b'.' {
            decimal = true;
            self.i += 1;
            let first = self.i;
            while self.i < len && b[self.i].is_ascii_digit() {
                self.i += 1;
            }
            if self.i == first {
                return Err(self.error());
            }
        }
        if self.i < len && (b[self.i] == b'e' || b[self.i] == b'E') {
            decimal = true;
            self.i += 1;
            if self.i < len && (b[self.i] == b'+' || b[self.i] == b'-') {
                self.i += 1;
            }
            let first = self.i;
            while self.i < len && b[self.i].is_ascii_digit() {
                self.i += 1;
            }
            if self.i == first {
                return Err(self.error());
            }
        }
        let n = &self.s[start..self.i];
        if !decimal {
            return match n.parse::<i64>() {
                Ok(v) => Ok(Value::from(v)),
                Err(_) => Err(self.error()),
            };
        }
        match n.parse::<f64>() {
            Ok(d) if d.is_finite() => Ok(double(d)),
            _ => Err(self.error()),
        }
    }
    fn str(&mut self) -> Result<String> {
        if self.i >= self.b.len() {
            return Err(self.error());
        }
        if self.b[self.i] != b'"' {
            // Java advanced past the offending char (one UTF-16 unit) before failing.
            return Err(self.error_at_units(self.utf16_offset(self.i) + 1));
        }
        self.i += 1;
        let mut start = self.i;
        let mut out: Option<String> = None;
        let mut pending_high: Option<u16> = None;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            self.i += 1;
            if c == b'"' {
                let tail = &self.s[start..self.i - 1];
                return Ok(match out {
                    None => tail.to_string(),
                    Some(mut o) => {
                        flush_high(&mut o, &mut pending_high, true);
                        o.push_str(tail);
                        o
                    }
                });
            }
            if c < 32 {
                return Err(self.error());
            }
            if c == b'\\' {
                let o = out.get_or_insert_with(String::new);
                let chunk = &self.s[start..self.i - 1];
                flush_high(o, &mut pending_high, !chunk.is_empty());
                o.push_str(chunk);
                if self.i == self.b.len() {
                    return Err(self.error());
                }
                let e = self.b[self.i];
                let escape_pos = self.i;
                self.i += 1;
                let unit: u16 = match e {
                    b'"' => b'"' as u16,
                    b'\\' => b'\\' as u16,
                    b'/' => b'/' as u16,
                    b'b' => 8,
                    b'f' => 12,
                    b'n' => b'\n' as u16,
                    b'r' => b'\r' as u16,
                    b't' => b'\t' as u16,
                    b'u' => {
                        let units: Vec<u16> = self.s[self.i..].encode_utf16().take(4).collect();
                        if units.len() < 4 {
                            return Err(self.error());
                        }
                        let parsed = parse_hex_int(&units);
                        match parsed {
                            Some(v) => {
                                self.i += 4;
                                v as u16
                            }
                            None => return Err(self.error()),
                        }
                    }
                    _ => {
                        // Offset after the escape character (one UTF-16 unit).
                        return Err(self.error_at_units(self.utf16_offset(escape_pos) + 1));
                    }
                };
                let o = out.as_mut().unwrap();
                push_unit(o, &mut pending_high, unit);
                start = self.i;
            } else if c >= 0x80 {
                // Skip the remaining bytes of this UTF-8 character.
                while self.i < self.b.len() && (self.b[self.i] & 0xC0) == 0x80 {
                    self.i += 1;
                }
            }
        }
        Err(self.error())
    }
}

/// `Integer.parseInt(text, 16)` for exactly four UTF-16 units (ASCII digits only).
fn parse_hex_int(units: &[u16]) -> Option<i32> {
    let mut i = 0;
    let mut negative = false;
    if units[0] == b'-' as u16 || units[0] == b'+' as u16 {
        negative = units[0] == b'-' as u16;
        i = 1;
    }
    if i == units.len() {
        return None;
    }
    let mut v: i32 = 0;
    for &u in &units[i..] {
        let d = char::from_u32(u as u32)?.to_digit(16)? as i32;
        v = v * 16 + d;
    }
    Some(if negative { -v } else { v })
}

/// Lone surrogates encode as `?` in Java's UTF-8 output; pairs combine.
fn push_unit(out: &mut String, pending_high: &mut Option<u16>, unit: u16) {
    if (0xDC00..0xE000).contains(&unit) {
        if let Some(h) = pending_high.take() {
            let c = 0x10000 + (((h as u32) - 0xD800) << 10) + ((unit as u32) - 0xDC00);
            out.push(char::from_u32(c).unwrap_or('?'));
        } else {
            out.push('?');
        }
        return;
    }
    if pending_high.take().is_some() {
        out.push('?');
    }
    if (0xD800..0xDC00).contains(&unit) {
        *pending_high = Some(unit);
    } else {
        out.push(char::from_u32(unit as u32).unwrap_or('?'));
    }
}

/// A pending high surrogate followed by literal text (or the string end) is lone.
fn flush_high(out: &mut String, pending_high: &mut Option<u16>, text_follows: bool) {
    if text_follows && pending_high.take().is_some() {
        out.push('?');
    }
}



/// Java `Map.putIfAbsent(key, value)`: also replaces a `null` mapping (in place).
pub fn put_if_absent(m: &mut Map, key: &str, value: Value) {
    match m.get_mut(key) {
        Some(existing) if !existing.is_null() => {}
        Some(existing) => *existing = value,
        None => {
            m.insert(key.to_string(), value);
        }
    }
}

/// Java `Map.getOrDefault(key, default)`: a present `null` stays `null`.
pub fn get_or_default<'a>(m: &'a Map, key: &str, default: &'a Value) -> &'a Value {
    match m.get(key) {
        Some(v) => v,
        None => default,
    }
}

/// `(String) m.getOrDefault(key, default)` for a string default.
pub fn string_or<'a>(m: &'a Map, key: &str, default: &'a str) -> Result<Option<&'a str>> {
    match m.get(key) {
        None => Ok(Some(default)),
        Some(v) => as_string_opt(Some(v)),
    }
}

/// The String key of a JSON value for `map.get(value)` lookups in
/// `Map<String,…>` tables (non-strings never match a key).
pub fn key_of(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// Java `String.hashCode()`.
pub fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as i32);
    }
    h
}

/// Iteration order of a `java.util.HashSet<String>` (default capacity 16,
/// load factor 0.75) after inserting `keys` in order: buckets by spread hash,
/// insertion order within a bucket (resizes preserve that relative order).
/// Used where Java output depended on HashSet order.
pub fn java_hash_set_order(keys: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut unique: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for k in keys {
        if seen.insert(k.clone()) {
            unique.push(k);
        }
    }
    let mut capacity: usize = 16;
    while unique.len() as f64 > capacity as f64 * 0.75 {
        capacity *= 2;
    }
    let bucket = |k: &String| {
        let h = java_string_hash(k);
        let spread = h ^ ((h as u32) >> 16) as i32;
        (spread as u32 as usize) & (capacity - 1)
    };
    let mut indexed: Vec<(usize, usize, String)> = unique.into_iter().enumerate().map(|(i, k)| (bucket(&k), i, k)).collect();
    indexed.sort_by_key(|(b, i, _)| (*b, *i));
    indexed.into_iter().map(|(_, _, k)| k).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trips() {
        for text in [
            r#"{"a":1,"b":1.0,"c":0.1,"d":1.0E-5,"e":1.0E21,"f":-0.0,"g":9223372036854775807}"#,
            r#"["\u0001\"\\x",true,false,null,[],{}]"#,
        ] {
            assert_eq!(write(&parse(text).unwrap()), text);
        }
        assert_eq!(write(&parse("1e-5").unwrap()), "1.0E-5");
        assert_eq!(write(&parse("\"\\ud83d\\ude00\"").unwrap()), "\"\u{1F600}\"");
        assert_eq!(parse("{1:2}").unwrap_err().message(), "Invalid JSON at offset 2");
        assert_eq!(parse("{\"a\":1,\"a\":2}").unwrap_err().message(), "Invalid JSON at offset 11");
        assert_eq!(parse("9223372036854775808").unwrap_err().message(), "Invalid JSON at offset 19");
        assert!(equivalent(&parse("1").unwrap(), &parse("1.0").unwrap()));
        assert_eq!(parse("1000000000000000000000").unwrap_err().message(), "Invalid JSON at offset 22");
    }
}
