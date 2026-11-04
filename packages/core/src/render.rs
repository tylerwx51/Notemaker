//! Port of `Render.java`: shared scene traversal for Android Canvas, desktop
//! PNG and SVG, plus the `Render.Svg` surface.

use crate::draw_item::DrawItem;
use crate::error::{Error, Result};
use crate::geometry::{Bounds, Degrees, DocPoint, DocRect, InkPasses, Polygon, Segment};
use crate::geometry::Erasures;
use crate::ids::{LayerId, PageNumber};
use crate::style::{ArgbColor, FontFamily, FontSize, Flip, PathEnd, RectShape, ShapePaint, StrokeWidth, TextAlignment, TextContent, TextStyle};
use crate::jfmt;
use crate::jmath;
use crate::json::{self, Map, Value};
use crate::model::{Entity, Model};
use crate::wire::EntityContent;
use crate::ids::ShapeId;
use crate::shape_catalog;
use crate::svg_asset;
use std::collections::HashSet;
use std::f64::consts::PI;
use std::fmt::Write as _;

/// `Render.Surface`. Object safe; default methods match the Java defaults.
/// Methods return `Result` because Java implementations may throw
/// (unchecked) exceptions, e.g. an invalid SVG in [`Svg::svg`].
pub trait Surface {
    fn begin_erase(&mut self, erasures: &Erasures) -> Result<()> {
        if !erasures.is_empty() {
            return Err(Error::unsupported("Erasure masks unsupported"));
        }
        Ok(())
    }
    fn end_erase(&mut self) -> Result<()> {
        Ok(())
    }
    fn polygon(&mut self, _points: &Polygon, _paint: ShapePaint, _end: PathEnd) -> Result<()> {
        Err(Error::unsupported("Polygon paint unsupported"))
    }
    /// Continuous ribbon passes. Raster surfaces merge shared edges before compositing ink.
    fn ink(&mut self, passes: &InkPasses, color: ArgbColor) -> Result<()> {
        for p in passes {
            self.polygon(p, ShapePaint { color, width: StrokeWidth(0.0), fill: Some(color) }, PathEnd::Closed)?;
        }
        Ok(())
    }
    fn line(&mut self, segment: Segment, color: ArgbColor, width: StrokeWidth) -> Result<()>;
    fn rect(&mut self, rect: DocRect, paint: ShapePaint, shape: RectShape) -> Result<()>;
    /// Text with its baseline starting at `origin`.
    fn text(&mut self, origin: DocPoint, text: &TextContent, color: ArgbColor, size: FontSize) -> Result<()>;
    #[allow(clippy::too_many_arguments)]
    fn styled_text(
        &mut self,
        origin: DocPoint,
        text: &TextContent,
        color: ArgbColor,
        size: FontSize,
        _family: &FontFamily,
        _style: TextStyle,
        _alignment: &TextAlignment,
    ) -> Result<()> {
        self.text(origin, text, color, size)
    }
    fn svg(&mut self, rect: DocRect, source: &str) -> Result<()>;
    fn save_transform(&mut self, _center: DocPoint, _rotation: Degrees, _flip: Flip) -> Result<()> {
        Ok(())
    }
    fn restore_transform(&mut self) -> Result<()> {
        Ok(())
    }
    fn image(&mut self, rect: DocRect, data: &str) -> Result<()> {
        let source = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\" viewBox=\"0 0 100 100\"><image width=\"100\" height=\"100\" preserveAspectRatio=\"none\" href=\"{}\"/></svg>",
            data
        );
        self.svg(rect, &source)
    }
}

/// Layers the user has hidden; forced layers are drawn anyway.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HiddenLayers(pub HashSet<String>);

fn shape_paint(color: i32, width: f64, fill: Option<i32>) -> ShapePaint {
    ShapePaint { color: ArgbColor(color), width: StrokeWidth(width), fill: fill.map(ArgbColor) }
}

/// Java `try { body } finally { cleanup }` with `Result`s: the body's error wins.
fn finally<T>(body: Result<T>, cleanup: Result<()>) -> Result<T> {
    match body {
        Err(e) => Err(e),
        Ok(v) => cleanup.map(|_| v),
    }
}

/// `Render.draw(m, page, hidden, c)`.
pub fn draw(m: &Model, page: PageNumber, hidden: &HiddenLayers, c: &mut dyn Surface) -> Result<()> {
    let mut ordered: Vec<(&String, &Entity)> = m.layers.iter().collect();
    let order = |e: &Entity| -> f64 {
        match e.get("order") {
            Some(Value::Number(n)) => json::number_double(n),
            _ => 0.0,
        }
    };
    ordered.sort_by(|a, b| order(a.1).total_cmp(&order(b.1)));
    for (id, layer) in ordered {
        if m.layer_active.get(id) == Some(&false) || (hidden.0.contains(id) && !json::is_true(layer.get("force"))) {
            continue;
        }
        for content in m.layer_items(&LayerId::from(id), page).iter() {
            let item = DrawItem::new(m, content.clone())?;
            if item.active() {
                draw_item(m, content, c)?;
            }
        }
    }
    Ok(())
}

/// `Render.drawItem(m, o, c)`.
pub fn draw_item(m: &Model, o: &EntityContent, c: &mut dyn Surface) -> Result<()> {
    if o.get("kind").and_then(Value::as_str) == Some("stroke") {
        draw_stroke(o, c)
    } else {
        draw_object(m, o, c)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct StrokePoint {
    x: f64,
    y: f64,
    pressure: f64,
    distance: f64,
    tx: f64,
    ty: f64,
    half_width: f64,
}

impl StrokePoint {
    fn new(x: f64, y: f64, pressure: f64, distance: f64) -> StrokePoint {
        StrokePoint { x, y, pressure, distance, ..Default::default() }
    }
}

/// `Render.drawStroke(s, c)`.
pub fn draw_stroke(s: &EntityContent, c: &mut dyn Surface) -> Result<()> {
    c.begin_erase(&DrawItem::erasures(s)?)?;
    let body = draw_stroke_body(s, c);
    let cleanup = c.end_erase();
    finally(body, cleanup)
}

fn draw_stroke_body(s: &Map, c: &mut dyn Surface) -> Result<()> {
    let brush = match s.get("brush") {
        None => "pen",
        v => json::as_string_opt(v)?.ok_or_else(Error::null_pointer)?,
    };
    let color = json::as_int(s.get("color"))?;
    let material = jmath::max(0.0, jmath::min(100.0, if s.contains_key("material") { json::number(s, "material")? } else { 50.0 })) / 100.0;
    let path = stroke_path(s, brush, material)?;
    if path.is_empty() {
        return Ok(());
    }
    let widths: Vec<f64> = path.iter().map(|p| p.half_width).collect();
    match brush {
        "marker" => fill_ribbon(c, &path, &widths, 0.0, material_color(color, brush, material, 1.0), true, 1.0)?,
        "pencil" => {
            fill_ribbon(c, &path, &widths, 0.0, material_color(color, brush, material, 0.8), true, 1.0)?;
            draw_grain(c, &path, &widths, color, brush, material, 2 + jmath::round(material * 4.0) as i32, 0.52)?;
        }
        "brush" => draw_bristles(c, &path, &widths, color, material)?,
        "oily" => {
            fill_ribbon(c, &path, &widths, 0.0, material_color(color, brush, material, 1.0), true, 1.0)?;
            fill_ribbon(c, &path, &widths, 0.24, tint(material_color(color, brush, material, 0.7), 0.5), false, 0.12)?;
        }
        "neoBrush" => fill_ribbon(c, &path, &widths, 0.0, material_color(color, brush, material, 0.10 + 0.22 * material), true, 1.0 + 0.55 * material)?,
        "charcoal" => {
            fill_ribbon(c, &path, &widths, 0.0, material_color(color, brush, material, 1.0), true, 1.0)?;
            draw_grain(c, &path, &widths, color, brush, material, 3 + jmath::round(material * 5.0) as i32, 0.7)?;
        }
        _ => fill_ribbon(c, &path, &widths, 0.0, material_color(color, brush, material, 1.0), true, 1.0)?,
    }
    Ok(())
}

fn stroke_path(stroke: &Map, brush: &str, material: f64) -> Result<Vec<StrokePoint>> {
    let source = json::list_at(stroke, "points")?;
    let mut raw: Vec<StrokePoint> = Vec::new();
    let (mut total, mut px, mut py) = (0.0f64, 0.0f64, 0.0f64);
    for item in source {
        let p = json::list(item)?;
        let x = json::as_double(p.first())?;
        let y = json::as_double(p.get(1))?;
        let pressure = json::as_double(p.get(2))?;
        if !raw.is_empty() {
            let d = jmath::hypot(x - px, y - py);
            if d < 1e-5 {
                continue;
            }
            total += d;
        }
        raw.push(StrokePoint::new(x, y, pressure, total));
        px = x;
        py = y;
    }
    let width = json::number(stroke, "width")?;
    let factor = match brush {
        "marker" => 1.8,
        "neoBrush" => 1.4,
        "brush" | "charcoal" => 1.35,
        "pencil" | "oily" => 0.82,
        "latinCalligraphy" | "asiaCalligraphy" => 1.5,
        _ => 1.0,
    };
    if raw.is_empty() {
        return Ok(raw);
    }
    if raw.len() == 1 {
        let p = &mut raw[0];
        p.tx = 1.0;
        p.half_width = jmath::max(0.3, width * (0.25 + 0.75 * p.pressure) * factor / 2.0);
        return Ok(raw);
    }
    let mut step = jmath::max(0.75, jmath::min(2.0, width * factor * 0.2));
    step = jmath::max(step, total / 8191.0);
    let count = 1.max(jmath::d2i((total / step).ceil()));
    let mut path: Vec<StrokePoint> = Vec::with_capacity(count as usize + 1);
    let mut segment = 0usize;
    for i in 0..=count {
        let distance = if i == count { total } else { jmath::min(total, i as f64 * step) };
        while segment + 2 < raw.len() && raw[segment + 1].distance < distance {
            segment += 1;
        }
        let a = raw[segment];
        let b = raw[(segment + 1).min(raw.len() - 1)];
        let span = b.distance - a.distance;
        let t = if span == 0.0 { 0.0 } else { (distance - a.distance) / span };
        path.push(StrokePoint::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.pressure + (b.pressure - a.pressure) * t, distance));
    }
    for i in 0..path.len() {
        let before = path[i.saturating_sub(1)];
        let after = path[(i + 1).min(path.len() - 1)];
        let dx = after.x - before.x;
        let dy = after.y - before.y;
        let length = jmath::hypot(dx, dy);
        if length < 1e-8 {
            path[i].tx = if i == 0 { 1.0 } else { path[i - 1].tx };
            path[i].ty = if i == 0 { 0.0 } else { path[i - 1].ty };
        } else {
            path[i].tx = dx / length;
            path[i].ty = dy / length;
        }
        let p = path[i];
        let pressure = if brush == "fountain" { 0.25 + (0.25 + 0.75 * material) * 0.75 * p.pressure } else { 0.25 + 0.75 * p.pressure };
        let mut w = width * pressure * factor;
        if brush == "latinCalligraphy" || brush == "asiaCalligraphy" {
            let nib = jmath::to_radians((if brush == "latinCalligraphy" { 25.0 } else { 40.0 }) + material * 40.0);
            let angle = jmath::atan2(p.ty, p.tx);
            w *= 0.12 + 0.88 * jmath::sin(angle - nib).abs();
        }
        path[i].half_width = jmath::max(0.3, w / 2.0);
    }
    Ok(path)
}

#[allow(clippy::too_many_arguments)]
fn fill_ribbon(c: &mut dyn Surface, path: &[StrokePoint], base_widths: &[f64], shift: f64, color: i32, round_caps: bool, width_scale: f64) -> Result<()> {
    if path.len() == 1 {
        let p = path[0];
        let n = 24;
        let mut circle = Polygon::from(vec![DocPoint::default(); n]);
        for (i, v) in circle.iter_mut().enumerate() {
            let a = 2.0 * PI * i as f64 / n as f64;
            let r = base_widths[0] * width_scale;
            v.x = p.x + r * jmath::cos(a);
            v.y = p.y + r * jmath::sin(a);
        }
        let color = ArgbColor(color);
        return c.polygon(&circle, ShapePaint { color, width: StrokeWidth(0.0), fill: Some(color) }, PathEnd::Closed);
    }
    // Partition the centerline into direction cones so crossings belong to
    // separate passes and receive a second ink deposit.
    let mut passes = InkPasses::default();
    let mut start = 0usize;
    let mut turn = 0.0f64;
    for i in 1..path.len() {
        let before = path[i - 1];
        let point = path[i];
        turn += jmath::atan2(before.tx * point.ty - before.ty * point.tx, before.tx * point.tx + before.ty * point.ty).abs();
        if turn > PI / 2.0 && i - 1 > start {
            passes.push(ribbon(path, base_widths, shift, width_scale, start, i - 1, start == 0 && round_caps, false, !round_caps));
            start = i - 1;
            turn = 0.0;
        }
    }
    passes.push(ribbon(path, base_widths, shift, width_scale, start, path.len() - 1, start == 0 && round_caps, round_caps, !round_caps));
    c.ink(&passes, ArgbColor(color))
}

#[allow(clippy::too_many_arguments)]
fn ribbon(path: &[StrokePoint], widths: &[f64], shift: f64, scale: f64, start: usize, end: usize, start_cap: bool, end_cap: bool, square_ends: bool) -> Polygon {
    let mut outline = Polygon::from(Vec::with_capacity((end - start + 1) * 2 + 20));
    for i in start..=end {
        let p = path[i];
        let r = jmath::max(0.25, widths[i] * scale);
        let offset = widths[i] * shift + r;
        outline.push(DocPoint { x: p.x - p.ty * offset, y: p.y + p.tx * offset });
    }
    let last = path[end];
    let first = path[start];
    if square_ends && end == path.len() - 1 {
        let radius = jmath::max(0.25, widths[end] * scale);
        let edge = outline.last_mut().unwrap();
        edge.x += last.tx * radius;
        edge.y += last.ty * radius;
    }
    if end_cap {
        add_cap(&mut outline, last.x - last.ty * widths[end] * shift, last.y + last.tx * widths[end] * shift, &last, jmath::max(0.25, widths[end] * scale), false);
    }
    for i in (start..=end).rev() {
        let p = path[i];
        let r = jmath::max(0.25, widths[i] * scale);
        let offset = widths[i] * shift - r;
        outline.push(DocPoint { x: p.x - p.ty * offset, y: p.y + p.tx * offset });
    }
    if square_ends && start == 0 {
        let radius = jmath::max(0.25, widths[start] * scale);
        let edge = outline.last_mut().unwrap();
        edge.x -= first.tx * radius;
        edge.y -= first.ty * radius;
    }
    if start_cap {
        add_cap(&mut outline, first.x - first.ty * widths[start] * shift, first.y + first.tx * widths[start] * shift, &first, jmath::max(0.25, widths[start] * scale), true);
    }
    outline
}

fn add_cap(out: &mut Polygon, cx: f64, cy: f64, p: &StrokePoint, radius: f64, start: bool) {
    let normal = jmath::atan2(p.tx, -p.ty);
    let angle = if start { normal + PI } else { normal };
    let steps = 10;
    for i in 1..=steps {
        let a = angle - PI * i as f64 / steps as f64;
        out.push(DocPoint { x: cx + radius * jmath::cos(a), y: cy + radius * jmath::sin(a) });
    }
}

fn draw_bristles(c: &mut dyn Surface, path: &[StrokePoint], widths: &[f64], color: i32, material: f64) -> Result<()> {
    let count = 2 + jmath::round(material * 8.0) as i32;
    let spread = 0.82;
    let lane_width = jmath::min(0.12, 0.34 * (2.0 * spread) / (1.max(count - 1) as f64));
    for lane in 0..count {
        let shift = if count == 1 { 0.0 } else { -spread + 2.0 * spread * lane as f64 / (count as f64 - 1.0) };
        let ink = material_color(color, "brush", material, 0.9);
        fill_ribbon(c, path, widths, shift, ink, true, lane_width)?;
    }
    Ok(())
}

fn material_color(color: i32, brush: &str, material: f64, opacity: f64) -> i32 {
    let mut amount = opacity;
    match brush {
        "marker" => amount *= 0.20 + 0.50 * material,
        "pencil" => amount *= 0.22 + 0.42 * material,
        "charcoal" => amount *= 0.35 + 0.60 * material,
        "brush" => amount *= 0.30 + 0.38 * material,
        "oily" => amount *= 0.76 + 0.24 * material,
        _ => {}
    }
    let alpha = 0i64.max(255i64.min(jmath::round((((color as u32) >> 24) & 255) as f64 * amount))) as i32;
    (color & 0x00ffffff) | (alpha << 24)
}

fn tint(color: i32, amount: f64) -> i32 {
    let mut r = (color >> 16) & 255;
    let mut g = (color >> 8) & 255;
    let mut b = color & 255;
    r += jmath::d2i((255 - r) as f64 * amount);
    g += jmath::d2i((255 - g) as f64 * amount);
    b += jmath::d2i((255 - b) as f64 * amount);
    (color & (0xff000000u32 as i32)) | (r << 16) | (g << 8) | b
}

#[allow(clippy::too_many_arguments)]
fn draw_grain(c: &mut dyn Surface, path: &[StrokePoint], widths: &[f64], color: i32, brush: &str, material: f64, count: i32, opacity: f64) -> Result<()> {
    let total = path[path.len() - 1].distance;
    let mut typical = 0.0;
    for w in widths {
        typical += w;
    }
    typical /= 1.max(widths.len()) as f64;
    let spacing = jmath::max(0.75, typical * 4.0 / 1.max(count) as f64);
    let mut index = 0i32;
    let mut d = spacing * 0.5;
    while d < total {
        let p = at_distance(path, d);
        let phase = (index as f64 * 2.399963229728653) % (2.0 * PI);
        let side = jmath::sin(phase) * 0.72;
        let along = jmath::cos(phase) * typical * 0.25;
        let cx = p.x - p.ty * side * typical * 0.25 + p.tx * along;
        let cy = p.y + p.tx * side * typical * 0.25 + p.ty * along;
        let len = typical * (0.2 + 0.25 * jmath::sin(phase).abs());
        let ink = material_color(color, brush, material, opacity);
        c.line(
            Segment { start: DocPoint { x: cx - p.tx * len / 2.0, y: cy - p.ty * len / 2.0 }, end: DocPoint { x: cx + p.tx * len / 2.0, y: cy + p.ty * len / 2.0 } },
            ArgbColor(ink),
            StrokeWidth(jmath::max(0.25, typical * 0.045)),
        )?;
        index += 1;
        d += spacing;
    }
    Ok(())
}

fn at_distance(path: &[StrokePoint], distance: f64) -> StrokePoint {
    let mut lo = 0usize;
    let mut hi = path.len() - 1;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if path[mid].distance < distance {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == 0 {
        return path[0];
    }
    let a = path[lo - 1];
    let b = path[lo];
    let t = (distance - a.distance) / jmath::max(1e-9, b.distance - a.distance);
    let mut p = StrokePoint::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.pressure + (b.pressure - a.pressure) * t, distance);
    p.tx = a.tx + (b.tx - a.tx) * t;
    p.ty = a.ty + (b.ty - a.ty) * t;
    let n = jmath::hypot(p.tx, p.ty);
    if n > 0.0 {
        p.tx /= n;
        p.ty /= n;
    }
    p.half_width = a.half_width + (b.half_width - a.half_width) * t;
    p
}

fn rotation_of(o: &Map) -> Result<f64> {
    match o.get("rotation") {
        None => Ok(0.0),
        v => json::as_double(v),
    }
}

fn is_attached_connector(o: &Map) -> bool {
    matches!(o.get("shape").and_then(Value::as_str), Some("arrow") | Some("arrowLine")) && (o.contains_key("from") || o.contains_key("to"))
}

/// `Render.drawObject(m, o, c)`.
pub fn draw_object(m: &Model, o: &EntityContent, c: &mut dyn Surface) -> Result<()> {
    if is_attached_connector(o) {
        let item = DrawItem::from_map(m, o.clone())?;
        return draw_object_masked(m, item.geometry(), c);
    }
    let rotation = rotation_of(o)?;
    let flip_x = json::is_true(o.get("flipX"));
    let flip_y = json::is_true(o.get("flipY"));
    if rotation != 0.0 || flip_x || flip_y {
        c.save_transform(
            DocPoint { x: json::number(o, "x")? + json::number(o, "w")? / 2.0, y: json::number(o, "y")? + json::number(o, "h")? / 2.0 },
            Degrees(rotation),
            Flip { horizontal: flip_x, vertical: flip_y },
        )?;
        let body = draw_object_masked(m, o, c);
        let cleanup = c.restore_transform();
        finally(body, cleanup)
    } else {
        draw_object_masked(m, o, c)
    }
}

fn draw_object_masked(m: &Model, o: &Map, c: &mut dyn Surface) -> Result<()> {
    c.begin_erase(&DrawItem::erasures(o.into())?)?;
    let body = draw_object_raw(m, o, c);
    let cleanup = c.end_erase();
    finally(body, cleanup)
}

fn fill_of(o: &Map) -> Result<Option<i32>> {
    if o.contains_key("fill") { Ok(Some(json::as_int(o.get("fill"))?)) } else { Ok(None) }
}

fn draw_object_raw(m: &Model, o: &Map, c: &mut dyn Surface) -> Result<()> {
    let mut x = json::number(o, "x")?;
    let mut y = json::number(o, "y")?;
    let w = json::number(o, "w")?;
    let h = json::number(o, "h")?;
    let width = json::number(o, "width")?;
    let color = json::as_int(o.get("color"))?;
    let shape = json::string(o, "shape")?;
    if shape == "template" {
        return Ok(()); // Infinite guides are generated against the live viewport by the tablet renderer.
    }
    if shape == "svg" {
        return c.svg(DocRect { x, y, width: w, height: h }, json::string(o, "svg")?);
    }
    if shape == "image" {
        return c.image(DocRect { x, y, width: w, height: h }, json::string(o, "image")?);
    }
    if shape == "gif" {
        return c.image(DocRect { x, y, width: w, height: h }, json::string(o, "gif")?);
    }
    if shape == "arrow" || shape == "arrowLine" {
        let ends = endpoints(m, o.into())?;
        x = ends.start.x;
        y = ends.start.y;
        let ex = ends.end.x;
        let ey = ends.end.y;
        let (ink, line_width) = (ArgbColor(color), StrokeWidth(width));
        c.line(ends, ink, line_width)?;
        let angle = jmath::atan2(ey - y, ex - x);
        let length = jmath::max(12.0, width * 4.0);
        let tip = DocPoint { x: ex, y: ey };
        c.line(Segment { start: tip, end: DocPoint { x: ex - length * jmath::cos(angle - 0.45), y: ey - length * jmath::sin(angle - 0.45) } }, ink, line_width)?;
        c.line(Segment { start: tip, end: DocPoint { x: ex - length * jmath::cos(angle + 0.45), y: ey - length * jmath::sin(angle + 0.45) } }, ink, line_width)?;
        x = (x + ex) / 2.0;
        y = (y + ey) / 2.0;
    } else if shape == "line" {
        c.line(Segment { start: DocPoint { x, y }, end: DocPoint { x: x + w, y: y + h } }, ArgbColor(color), StrokeWidth(width))?;
        x += w / 2.0;
        y += h / 2.0;
    } else if shape_catalog::polygon(&ShapeId::from(shape)) {
        let vertices = shape_catalog::vertices(&ShapeId::from(shape))?;
        let points: Polygon = vertices.iter().map(|v| DocPoint { x: x + v.x * w, y: y + v.y * h }).collect();
        let end = if shape != "polyline" { PathEnd::Closed } else { PathEnd::Open };
        c.polygon(&points, shape_paint(color, width, fill_of(o)?), end)?;
    } else if matches!(shape, "cube" | "cylinder" | "cone" | "sphere" | "pyramid" | "wave" | "waveLine" | "curve") {
        let pad = width / 2.0 + 1.0;
        c.svg(DocRect { x: x - pad, y: y - pad, width: w + 2.0 * pad, height: h + 2.0 * pad }, &detail_svg(shape, color, width, fill_of(o)?)?)?;
    } else if shape != "text" {
        let outline = if shape == "ellipse" || shape == "circle" { RectShape::Ellipse } else { RectShape::Rectangle };
        c.rect(DocRect { x, y, width: w, height: h }, shape_paint(color, width, fill_of(o)?), outline)?;
    }
    let text = match o.get("text") {
        None => "",
        v => json::as_string_opt(v)?.ok_or_else(Error::null_pointer)?,
    };
    let size = match o.get("fontSize") {
        None => 24.0,
        v => json::as_double(v)?,
    };
    if shape == "ellipse" {
        x += 25.0;
        y += h / 2.0 - size;
    }
    let family = match o.get("fontFamily") {
        None => "sans-serif",
        v => json::as_string_opt(v)?.ok_or_else(Error::null_pointer)?,
    };
    let bold = json::is_true(o.get("bold"));
    let italic = json::is_true(o.get("italic"));
    let underline = json::is_true(o.get("underline"));
    let alignment = match o.get("alignment") {
        None => Some("left"),
        v => json::as_string_opt(v)?,
    };
    let align_offset = match alignment {
        Some("center") => w / 2.0,
        Some("right") => w,
        _ => 0.0,
    };
    let text_x = x + align_offset + if shape == "text" { 0.0 } else { 10.0 };
    let family = FontFamily::from(family);
    let style = TextStyle { bold, italic, underline };
    let alignment = TextAlignment::from(alignment.unwrap_or("null"));
    for (i, line) in text.split('\n').enumerate() {
        c.styled_text(
            DocPoint { x: text_x, y: y + size + i as f64 * size * 1.2 },
            &TextContent::from(line),
            ArgbColor(color),
            FontSize(size),
            &family,
            style,
            &alignment,
        )?;
    }
    Ok(())
}

fn detail_svg(shape: &str, stroke: i32, width: f64, fill: Option<i32>) -> Result<String> {
    let raw = shape_catalog::drawing_svg(&ShapeId::from(shape))?;
    let color = ArgbColor(stroke).to_hex();
    let f = match fill {
        None => "none".to_string(),
        Some(v) => String::from(ArgbColor(v).to_hex()),
    };
    let fill_opacity = match fill {
        None => 1.0,
        Some(v) => ((v as u32) >> 24) as f64 / 255.0,
    };
    let replacement = format!(
        "fill=\"{}\" fill-opacity=\"{}\" stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"",
        f,
        jfmt::double(fill_opacity),
        color,
        jfmt::double(((stroke as u32) >> 24) as f64 / 255.0),
        jfmt::double(width)
    );
    Ok(raw.replace("fill=\"none\"", &replacement))
}

/// `Render.endpoints(m, o)`: connector attachments follow node geometry.
pub fn endpoints(m: &Model, o: &EntityContent) -> Result<Segment> {
    let mut x = json::number(o, "x")?;
    let mut y = json::number(o, "y")?;
    let mut ex = x + json::number(o, "w")?;
    let mut ey = y + json::number(o, "h")?;
    let a = json::key_of(o.get("from")).and_then(|k| m.objects.get(k));
    let b = json::key_of(o.get("to")).and_then(|k| m.objects.get(k));
    if let Some(a) = a {
        x = json::number(a, "x")? + json::number(a, "w")? / 2.0;
        y = json::number(a, "y")? + json::number(a, "h")? / 2.0;
    }
    if let Some(b) = b {
        ex = json::number(b, "x")? + json::number(b, "w")? / 2.0;
        ey = json::number(b, "y")? + json::number(b, "h")? / 2.0;
    }
    let start = boundary(a.map(|e| &***e), x, y, ex, ey)?;
    let end = boundary(b.map(|e| &***e), ex, ey, x, y)?;
    Ok(Segment { start: DocPoint { x: start[0], y: start[1] }, end: DocPoint { x: end[0], y: end[1] } })
}

fn boundary(node: Option<&Map>, x: f64, y: f64, tx: f64, ty: f64) -> Result<[f64; 2]> {
    let node = match node {
        Some(n) if !(x == tx && y == ty) => n,
        _ => return Ok([x, y]),
    };
    let dx = tx - x;
    let dy = ty - y;
    let angle = jmath::to_radians(rotation_of(node)?);
    let cos = jmath::cos(angle);
    let sin = jmath::sin(angle);
    let flip_x = json::is_true(node.get("flipX"));
    let flip_y = json::is_true(node.get("flipY"));
    let lx = (dx * cos + dy * sin) * if flip_x { -1.0 } else { 1.0 };
    let ly = (-dx * sin + dy * cos) * if flip_y { -1.0 } else { 1.0 };
    let rx = json::number(node, "w")? / 2.0;
    let ry = json::number(node, "h")? / 2.0;
    let factor = if matches!(json::string(node, "shape")?, "ellipse" | "circle") {
        1.0 / (lx * lx / (rx * rx) + ly * ly / (ry * ry)).sqrt()
    } else {
        jmath::min(if lx == 0.0 { f64::INFINITY } else { rx / lx.abs() }, if ly == 0.0 { f64::INFINITY } else { ry / ly.abs() })
    };
    let fx = lx * factor * if flip_x { -1.0 } else { 1.0 };
    let fy = ly * factor * if flip_y { -1.0 } else { 1.0 };
    Ok([x + fx * cos - fy * sin, y + fx * sin + fy * cos])
}

/// `Render.Svg`: SVG document surface.
pub struct Svg {
    s: String,
    clip_id: i32,
    clip_counts: Vec<usize>,
}

fn d(out: &mut String, v: f64) {
    jfmt::append_double(out, v);
}

fn svg_color(c: i32) -> String {
    String::from(ArgbColor(c).to_hex())
}

fn svg_alpha(c: i32) -> f64 {
    ((c as u32) >> 24) as f64 / 255.0
}

fn svg_path(p: &[DocPoint], closed: bool) -> String {
    let mut out = String::with_capacity(p.len() * 24);
    for (i, v) in p.iter().enumerate() {
        out.push(if i == 0 { 'M' } else { 'L' });
        d(&mut out, v.x);
        out.push(' ');
        d(&mut out, v.y);
    }
    if closed {
        out.push('Z');
    }
    out
}

fn side(a: DocPoint, b: DocPoint, p: DocPoint) -> f64 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

fn strictly_inside(polygon: &Polygon, point: DocPoint) -> bool {
    if !polygon.contains(point) {
        return false;
    }
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        if side(a, b, point).abs() < 1e-7
            && point.x >= jmath::min(a.x, b.x) - 1e-8
            && point.x <= jmath::max(a.x, b.x) + 1e-8
            && point.y >= jmath::min(a.y, b.y) - 1e-8
            && point.y <= jmath::max(a.y, b.y) + 1e-8
        {
            return false;
        }
    }
    true
}

fn interior_overlap(a: &Polygon, b: &Polygon) -> bool {
    let x: Bounds = a.bounds();
    let y: Bounds = b.bounds();
    if x.right <= y.left || y.right <= x.left || x.bottom <= y.top || y.bottom <= x.top {
        return false;
    }
    for p in a {
        if strictly_inside(b, *p) {
            return true;
        }
    }
    for p in b {
        if strictly_inside(a, *p) {
            return true;
        }
    }
    for i in 0..a.len() {
        for j in 0..b.len() {
            let p = a[i];
            let q = a[(i + 1) % a.len()];
            let r = b[j];
            let t = b[(j + 1) % b.len()];
            if jmath::max(p.x, q.x) < jmath::min(r.x, t.x)
                || jmath::max(r.x, t.x) < jmath::min(p.x, q.x)
                || jmath::max(p.y, q.y) < jmath::min(r.y, t.y)
                || jmath::max(r.y, t.y) < jmath::min(p.y, q.y)
            {
                continue;
            }
            let u = side(p, q, r);
            let v = side(p, q, t);
            let w = side(r, t, p);
            let z = side(r, t, q);
            if ((u > 1e-7 && v < -1e-7) || (u < -1e-7 && v > 1e-7)) && ((w > 1e-7 && z < -1e-7) || (w < -1e-7 && z > 1e-7)) {
                return true;
            }
        }
    }
    // Coincident/retraced boundaries need an interior witness: no vertex is
    // strictly inside and no edges cross in that case.
    for i in 0..a.len() {
        let p = a[i];
        let q = a[(i + 1) % a.len()];
        let dx = q.x - p.x;
        let dy = q.y - p.y;
        let length = jmath::hypot(dx, dy);
        if length < 1e-8 {
            continue;
        }
        let epsilon = jmath::min(length * 0.001, 1e-4);
        let cx = (p.x + q.x) / 2.0;
        let cy = (p.y + q.y) / 2.0;
        for sign in [-1.0f64, 1.0] {
            let witness = DocPoint { x: cx - sign * dy / length * epsilon, y: cy + sign * dx / length * epsilon };
            if strictly_inside(a, witness) && strictly_inside(b, witness) {
                return true;
            }
        }
    }
    false
}

impl Svg {
    /// `new Render.Svg(x, y, w, h)`: white background over the viewBox.
    pub fn new(view_box: DocRect) -> Svg {
        let DocRect { x, y, width: w, height: h } = view_box;
        let mut s = String::new();
        s.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"");
        d(&mut s, w);
        s.push_str("\" height=\"");
        d(&mut s, h);
        s.push_str("\" viewBox=\"");
        d(&mut s, x);
        s.push(' ');
        d(&mut s, y);
        s.push(' ');
        d(&mut s, w);
        s.push(' ');
        d(&mut s, h);
        s.push_str("\"><rect x=\"");
        d(&mut s, x);
        s.push_str("\" y=\"");
        d(&mut s, y);
        s.push_str("\" width=\"");
        d(&mut s, w);
        s.push_str("\" height=\"");
        d(&mut s, h);
        s.push_str("\" fill=\"white\"/>");
        Svg { s, clip_id: 0, clip_counts: Vec::new() }
    }
}

impl std::fmt::Display for Svg {
    /// `toString()`: the document plus `</svg>\n`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.s)?;
        f.write_str("</svg>\n")
    }
}

impl Surface for Svg {
    fn begin_erase(&mut self, polygons: &Erasures) -> Result<()> {
        self.clip_counts.push(polygons.len());
        for p in polygons {
            let id = format!("erase-{}", self.clip_id);
            self.clip_id += 1;
            let _ = write!(
                self.s,
                "<defs><clipPath id=\"{}\" clipPathUnits=\"userSpaceOnUse\"><path clip-rule=\"evenodd\" d=\"M-1000000000 -1000000000H1000000000V1000000000H-1000000000Z {}\"/></clipPath></defs><g clip-path=\"url(#{})\">",
                id,
                svg_path(p, true),
                id
            );
        }
        Ok(())
    }
    fn end_erase(&mut self) -> Result<()> {
        let count = self.clip_counts.pop().ok_or_else(|| Error::without_message(crate::error::ErrorKind::IllegalState))?;
        for _ in 0..count {
            self.s.push_str("</g>");
        }
        Ok(())
    }
    fn polygon(&mut self, points: &Polygon, paint: ShapePaint, end: PathEnd) -> Result<()> {
        let (color, width, fill) = (paint.color.0, paint.width.0, paint.fill.map(|f| f.0));
        let closed = end == PathEnd::Closed;
        let s = &mut self.s;
        s.push_str("<path d=\"");
        s.push_str(&svg_path(points, closed));
        s.push_str("\" stroke=\"");
        s.push_str(&if width > 0.0 { svg_color(color) } else { "none".to_string() });
        s.push_str("\" stroke-opacity=\"");
        d(s, svg_alpha(color));
        s.push_str("\" stroke-width=\"");
        d(s, width);
        s.push_str("\" stroke-linejoin=\"round\" stroke-linecap=\"round\" fill=\"");
        s.push_str(&match fill {
            None => "none".to_string(),
            Some(f) => svg_color(f),
        });
        s.push_str("\" fill-opacity=\"");
        d(s, match fill {
            None => 1.0,
            Some(f) => svg_alpha(f),
        });
        s.push_str("\"/>");
        Ok(())
    }
    fn ink(&mut self, passes: &InkPasses, ink: ArgbColor) -> Result<()> {
        if passes.len() == 1 {
            return self.polygon(&passes[0], ShapePaint { color: ink, width: StrokeWidth(0.0), fill: Some(ink) }, PathEnd::Closed);
        }
        let color = ink.0;
        // All passes have the same winding: paint their union once, then each
        // later pass clipped to prior coverage adds one ink deposit.
        let paths: Vec<String> = passes.iter().map(|p| svg_path(p, true)).collect();
        let union: String = paths.concat();
        let s = &mut self.s;
        s.push_str("<path d=\"");
        s.push_str(&union);
        s.push_str("\" fill-rule=\"nonzero\" fill=\"");
        s.push_str(&svg_color(color));
        s.push_str("\" fill-opacity=\"");
        d(s, svg_alpha(color));
        s.push_str("\"/>");
        if ((color as u32) >> 24) == 255 {
            return Ok(());
        }
        let prefix = format!("ink-{}-", self.clip_id);
        self.clip_id += 1;
        let s = &mut self.s;
        s.push_str("<defs xmlns:xlink=\"http://www.w3.org/1999/xlink\">");
        for (i, path) in paths.iter().enumerate().take(paths.len() - 1) {
            let _ = write!(s, "<path id=\"{}{}\" d=\"{}\"/>", prefix, i, path);
        }
        let mut overlaps: Vec<usize> = Vec::new();
        for i in 1..paths.len() {
            let prior: Vec<usize> = (0..i).filter(|&j| interior_overlap(&passes[i], &passes[j])).collect();
            // A shared cut has no overlap area. Omitting it also avoids
            // rasterizers applying clip-edge antialiasing as an ink seam.
            if prior.is_empty() {
                continue;
            }
            overlaps.push(i);
            let _ = write!(s, "<clipPath id=\"{}clip-{}\" clipPathUnits=\"userSpaceOnUse\">", prefix, i);
            for j in prior {
                let _ = write!(s, "<use xlink:href=\"#{}{}\"/>", prefix, j);
            }
            s.push_str("</clipPath>");
        }
        s.push_str("</defs>");
        for i in overlaps {
            let _ = write!(s, "<path d=\"{}\" clip-path=\"url(#{}clip-{})\" fill=\"{}\" fill-opacity=\"", paths[i], prefix, i, svg_color(color));
            d(s, svg_alpha(color));
            s.push_str("\"/>");
        }
        Ok(())
    }
    fn svg(&mut self, rect: DocRect, source: &str) -> Result<()> {
        let DocRect { x, y, width: w, height: h } = rect;
        let data = jfmt::base64_encode(svg_asset::viewport(source)?.as_bytes());
        let s = &mut self.s;
        s.push_str("<image x=\"");
        d(s, x);
        s.push_str("\" y=\"");
        d(s, y);
        s.push_str("\" width=\"");
        d(s, w);
        s.push_str("\" height=\"");
        d(s, h);
        s.push_str("\" preserveAspectRatio=\"none\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xlink:href=\"data:image/svg+xml;base64,");
        s.push_str(&data);
        s.push_str("\"/>");
        Ok(())
    }
    fn save_transform(&mut self, center: DocPoint, rotation: Degrees, flip: Flip) -> Result<()> {
        let DocPoint { x: cx, y: cy } = center;
        let (rotation, flip_x, flip_y) = (rotation.0, flip.horizontal, flip.vertical);
        let s = &mut self.s;
        s.push_str("<g transform=\"translate(");
        d(s, cx);
        s.push(' ');
        d(s, cy);
        s.push_str(") rotate(");
        d(s, rotation);
        s.push_str(") scale(");
        s.push_str(if flip_x { "-1" } else { "1" });
        s.push(' ');
        s.push_str(if flip_y { "-1" } else { "1" });
        s.push_str(") translate(");
        d(s, -cx);
        s.push(' ');
        d(s, -cy);
        s.push_str(")\">");
        Ok(())
    }
    fn restore_transform(&mut self) -> Result<()> {
        self.s.push_str("</g>");
        Ok(())
    }
    fn image(&mut self, rect: DocRect, data: &str) -> Result<()> {
        let DocRect { x, y, width: w, height: h } = rect;
        let s = &mut self.s;
        s.push_str("<image x=\"");
        d(s, x);
        s.push_str("\" y=\"");
        d(s, y);
        s.push_str("\" width=\"");
        d(s, w);
        s.push_str("\" height=\"");
        d(s, h);
        s.push_str("\" preserveAspectRatio=\"none\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xlink:href=\"");
        s.push_str(data);
        s.push_str("\"/>");
        Ok(())
    }
    fn line(&mut self, segment: Segment, color: ArgbColor, width: StrokeWidth) -> Result<()> {
        let (DocPoint { x, y }, DocPoint { x: ex, y: ey }) = (segment.start, segment.end);
        let (color, width) = (color.0, width.0);
        let s = &mut self.s;
        s.push_str("<line x1=\"");
        d(s, x);
        s.push_str("\" y1=\"");
        d(s, y);
        s.push_str("\" x2=\"");
        d(s, ex);
        s.push_str("\" y2=\"");
        d(s, ey);
        s.push_str("\" stroke=\"");
        s.push_str(&svg_color(color));
        s.push_str("\" stroke-opacity=\"");
        d(s, svg_alpha(color));
        s.push_str("\" stroke-width=\"");
        d(s, width);
        s.push_str("\" stroke-linecap=\"round\"/>");
        Ok(())
    }
    fn rect(&mut self, rect: DocRect, paint: ShapePaint, shape: RectShape) -> Result<()> {
        let DocRect { x, y, width: w, height: h } = rect;
        let (color, width, fill) = (paint.color.0, paint.width.0, paint.fill.map(|f| f.0));
        let ellipse = shape == RectShape::Ellipse;
        let s = &mut self.s;
        s.push_str(if ellipse { "<ellipse cx=\"" } else { "<rect x=\"" });
        d(s, if ellipse { x + w / 2.0 } else { x });
        s.push_str(if ellipse { "\" cy=\"" } else { "\" y=\"" });
        d(s, if ellipse { y + h / 2.0 } else { y });
        s.push_str(if ellipse { "\" rx=\"" } else { "\" width=\"" });
        d(s, if ellipse { w / 2.0 } else { w });
        s.push_str(if ellipse { "\" ry=\"" } else { "\" height=\"" });
        d(s, if ellipse { h / 2.0 } else { h });
        s.push_str("\" stroke=\"");
        s.push_str(&svg_color(color));
        s.push_str("\" stroke-opacity=\"");
        d(s, svg_alpha(color));
        s.push_str("\" stroke-width=\"");
        d(s, width);
        s.push_str("\" fill=\"");
        s.push_str(&match fill {
            None => "none".to_string(),
            Some(f) => svg_color(f),
        });
        s.push_str("\" fill-opacity=\"");
        d(s, match fill {
            None => 1.0,
            Some(f) => svg_alpha(f),
        });
        s.push_str("\"/>");
        Ok(())
    }
    fn text(&mut self, origin: DocPoint, text: &TextContent, color: ArgbColor, size: FontSize) -> Result<()> {
        self.styled_text(origin, text, color, size, &FontFamily::from("sans-serif"), TextStyle::default(), &TextAlignment::from("left"))
    }
    fn styled_text(
        &mut self,
        origin: DocPoint,
        text: &TextContent,
        color: ArgbColor,
        size: FontSize,
        family: &FontFamily,
        style: TextStyle,
        alignment: &TextAlignment,
    ) -> Result<()> {
        let DocPoint { x, y } = origin;
        let (text, color, size, family, alignment) = (text.as_ref(), color.0, size.0, family.as_ref(), alignment.as_ref());
        let TextStyle { bold, italic, underline } = style;
        let s = &mut self.s;
        s.push_str("<text x=\"");
        d(s, x);
        s.push_str("\" y=\"");
        d(s, y);
        s.push_str("\" text-anchor=\"");
        s.push_str(match alignment {
            "center" => "middle",
            "right" => "end",
            _ => "start",
        });
        s.push_str("\" fill=\"");
        s.push_str(&svg_color(color));
        s.push_str("\" fill-opacity=\"");
        d(s, svg_alpha(color));
        s.push_str("\" font-family=\"");
        s.push_str(&family.replace('&', "&amp;").replace('"', "&quot;"));
        s.push_str("\" font-size=\"");
        d(s, size);
        s.push_str("\" font-weight=\"");
        s.push_str(if bold { "bold" } else { "normal" });
        s.push_str("\" font-style=\"");
        s.push_str(if italic { "italic" } else { "normal" });
        s.push_str("\" text-decoration=\"");
        s.push_str(if underline { "underline" } else { "none" });
        s.push_str("\">");
        s.push_str(&text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"));
        s.push_str("</text>");
        Ok(())
    }
}
