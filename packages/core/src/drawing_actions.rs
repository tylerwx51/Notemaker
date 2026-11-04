//! Port of `DrawingActions.java`: reversible drawing tools expressed only as
//! existing atomic operation batches.

use crate::actions;
use crate::coverage::CoverageFactory;
use crate::draw_item::DrawItem;
use crate::error::{require, Error, Result};
use crate::geometry::{DocPoint, Erasures, Polygon, Radius, Segment};
use crate::ids::{self, EntityId, EntityKind, LayerId, LayerPosition};
use crate::image_asset;
use crate::jfmt;
use crate::jmath;
use crate::json::{self, Map, Value};
use crate::layer_editor::{self, LayerEditor};
use crate::model::{self, Model};
use crate::object;
use crate::region_fill;
use crate::render;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

const STRIPPED: [&str; 8] = ["id", "actor", "seq", "at", "action", "session", "bases", "visibility"];

fn clone_op(op: &Map) -> Map {
    let mut r = op.clone();
    for k in STRIPPED {
        r.shift_remove(k);
    }
    r
}

fn opt(v: &Map, k: &str, fallback: f64) -> Result<f64> {
    if v.contains_key(k) { json::number(v, k) } else { Ok(fallback) }
}

fn checked(result: Vec<Value>) -> Result<Vec<Value>> {
    require(!result.is_empty(), "No matching editable content")?;
    require(result.len() <= 200, "Operation exceeds 200 actions; select less content")?;
    for op in &result {
        Model::validate(json::map(op)?)?;
    }
    Ok(result)
}

fn layer_or_base(input: &Map) -> Result<LayerId> {
    Ok(LayerId::from(json::string_or(input, "layer", "base")?.ok_or_else(Error::null_pointer)?))
}

/// `copySelection(model, entities)`: clipboard snapshot; detached connectors keep their geometry.
pub fn copy_selection(model: &Model, entities: &[Value]) -> Result<Vec<Value>> {
    require(!entities.is_empty() && entities.len() <= 100, "Clipboard needs 1–100 entities")?;
    let mut nodes: HashSet<String> = HashSet::new();
    for value in entities {
        let e = json::map(value)?;
        if e.get("kind").and_then(Value::as_str) == Some("object") {
            nodes.insert(json::string(e, "id")?.to_string());
        }
    }
    let node = |v: Option<&Value>| json::key_of(v).is_some_and(|k| nodes.contains(k));
    let mut out = Vec::new();
    for value in entities {
        let e = json::map(value)?;
        let kind = json::string(e, "kind")?;
        let id = json::string(e, "id")?;
        require(kind == "object" || kind == "stroke", "Clipboard supports objects and strokes")?;
        let original = if kind == "object" { model.objects.get(id) } else { model.strokes.get(id) };
        require(original.is_some(), "Unknown clipboard entity")?;
        let original = original.unwrap();
        let mut next = clone_op(original);
        if kind == "object" && (original.contains_key("from") || original.contains_key("to")) && (!node(original.get("from")) || !node(original.get("to"))) {
            let ends = render::endpoints(model, original)?;
            next.insert("x".into(), json::double(ends.start.x));
            next.insert("y".into(), json::double(ends.start.y));
            next.insert("w".into(), json::double(ends.end.x - ends.start.x));
            next.insert("h".into(), json::double(ends.end.y - ends.start.y));
            next.shift_remove("from");
            next.shift_remove("to");
            next.insert("rotation".into(), Value::from(0));
            next.insert("flipX".into(), Value::Bool(false));
            next.insert("flipY".into(), Value::Bool(false));
        }
        out.push(Value::Object(next));
    }
    Ok(out)
}

/// `paste(model, input)`: fresh IDs, offset by (`dx`, `dy`) (default 24).
pub fn paste(model: &Model, input: &Map) -> Result<Vec<Value>> {
    let entities = json::list_at(input, "entities")?;
    require(!entities.is_empty() && entities.len() <= 100, "Clipboard needs 1–100 entities")?;
    let dx = opt(input, "dx", 24.0)?;
    let dy = opt(input, "dy", 24.0)?;
    require(dx.is_finite() && dy.is_finite() && dx.abs() <= 1000000.0 && dy.abs() <= 1000000.0, "Invalid paste offset")?;
    let layer = layer_or_base(input)?;
    require(model.layers.contains_key(layer.as_ref()), "Unknown paste layer")?;
    let mut ids: HashMap<String, String> = HashMap::new();
    for value in entities {
        let source = json::map(value)?;
        let kind = json::string(source, "kind")?;
        require(kind == "object" || kind == "stroke", "Clipboard supports objects and strokes")?;
        Model::validate(source)?;
        let key = format!("{}:{}", kind, json::string(source, kind)?);
        require(!ids.contains_key(&key), "Duplicate clipboard entity")?;
        ids.insert(key, String::from(ids::Uuid::random()));
    }
    let mut out = Vec::new();
    for value in entities {
        let source = json::map(value)?;
        let mut next = clone_op(source);
        let kind = json::string(source, "kind")?;
        let new_id = ids.get(&format!("{}:{}", kind, json::string(source, kind)?)).cloned();
        next.insert(kind.to_string(), new_id.map(Value::String).unwrap_or(Value::Null));
        next.insert("layer".into(), Value::from(&layer));
        if input.contains_key("page") {
            next.insert("page".into(), input.get("page").cloned().unwrap_or(Value::Null));
        }
        if kind == "object" {
            next.insert("x".into(), json::double(json::number(source, "x")? + dx));
            next.insert("y".into(), json::double(json::number(source, "y")? + dy));
            for key in ["from", "to"] {
                if source.contains_key(key) {
                    let target = format!("object:{}", json::java_string(source.get(key)));
                    require(ids.contains_key(&target), "Clipboard connector refers to a missing node")?;
                    next.insert(key.into(), Value::String(ids[&target].clone()));
                }
            }
        } else {
            let mut points = Vec::new();
            for point in json::list_at(source, "points")? {
                let p = json::list(point)?;
                points.push(Value::Array(vec![
                    json::double(json::as_double(p.first())? + dx),
                    json::double(json::as_double(p.get(1))? + dy),
                    p.get(2).cloned().unwrap_or(Value::Null),
                ]));
            }
            next.insert("points".into(), Value::Array(points));
        }
        out.push(Value::Object(next));
    }
    checked(out)
}

/// `duplicateLayer(m, id)`: a copy of the layer and its active content.
pub fn duplicate_layer(m: &Model, id: &LayerId) -> Result<Vec<Value>> {
    let id: &str = id;
    require(m.layers.contains_key(id), "Unknown layer")?;
    let target = String::from(ids::Uuid::random());
    let mut result = Vec::new();
    let mut layer = clone_op(&m.layers[id]);
    layer.insert("layer".into(), Value::String(target.clone()));
    let name = format!("{} copy", json::java_string(layer.get("name")));
    layer.insert("name".into(), Value::String(name));
    result.push(Value::Object(layer));
    let mut ids: HashMap<String, String> = HashMap::new();
    for o in m.objects.values() {
        let object_id = json::key_of(o.get("object"));
        if o.get("layer").and_then(Value::as_str) == Some(id) && object_id.and_then(|k| m.object_active.get(k)) != Some(&false) {
            ids.insert(json::string(o, "object")?.to_string(), String::from(ids::Uuid::random()));
        }
    }
    for o in m.objects.values() {
        if let Some(new_id) = json::key_of(o.get("object")).and_then(|k| ids.get(k)) {
            let mut n = clone_op(o);
            n.insert("layer".into(), Value::String(target.clone()));
            n.insert("object".into(), Value::String(new_id.clone()));
            for k in ["from", "to"] {
                if let Some(mapped) = json::key_of(o.get(k)).and_then(|v| ids.get(v)) {
                    n.insert(k.into(), Value::String(mapped.clone()));
                }
            }
            result.push(Value::Object(n));
        }
    }
    for s in m.strokes.values() {
        let stroke_id = json::key_of(s.get("stroke"));
        if s.get("layer").and_then(Value::as_str) == Some(id) && stroke_id.and_then(|k| m.active.get(k)) != Some(&false) {
            let mut n = clone_op(s);
            n.insert("layer".into(), Value::String(target.clone()));
            n.insert("stroke".into(), Value::String(String::from(ids::Uuid::random())));
            result.push(Value::Object(n));
        }
    }
    checked(result)
}

/// `mergeLayer(m, source, target)`: move content to `target` and hide `source`.
pub fn merge_layer(m: &Model, source: &LayerId, target: &LayerId) -> Result<Vec<Value>> {
    let (source, target): (&str, &str) = (source, target);
    require(source != target && source != "base", "Select a non-base source and a different target")?;
    require(m.layers.contains_key(source) && m.layers.contains_key(target), "Unknown layer")?;
    require(m.layer_active.get(target) != Some(&false), "Target layer is hidden")?;
    let mut result = Vec::new();
    for group in [&m.strokes, &m.objects] {
        for o in group.values() {
            if o.get("layer").and_then(Value::as_str) == Some(source) {
                let mut n = clone_op(o);
                n.insert("layer".into(), Value::String(target.to_string()));
                result.push(Value::Object(n));
            }
        }
    }
    result.push(Value::Object(object! {"kind" => "layer-active", "layer" => source, "active" => false}));
    checked(result)
}

/// `reorderLayer(m, id, index)`: renumber every layer's `order`.
pub fn reorder_layer(m: &Model, id: &LayerId, index: LayerPosition) -> Result<Vec<Value>> {
    let (id, index) = (&**id, index.0);
    require(m.layers.contains_key(id) && index >= 0 && (index as usize) < m.layers.len(), "Invalid layer position")?;
    let mut layers: Vec<&Map> = m.layers.values().map(|l| &**l).collect();
    let mut indices: HashMap<String, i32> = HashMap::new();
    for (i, l) in layers.iter().enumerate() {
        indices.insert(json::string(l, "layer")?.to_string(), i as i32);
    }
    let mut keyed: Vec<(f64, &Map)> = Vec::with_capacity(layers.len());
    for l in &layers {
        let fallback = indices[json::string(l, "layer")?] as f64;
        keyed.push((opt(l, "order", fallback)?, l));
    }
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    layers = keyed.into_iter().map(|(_, l)| l).collect();
    let moving: &Map = &m.layers[id];
    let position = layers.iter().position(|l| std::ptr::eq(*l, moving) || *l == moving).unwrap();
    layers.remove(position);
    layers.insert(index as usize, moving);
    let mut result = Vec::new();
    for (i, l) in layers.iter().enumerate() {
        let mut n = clone_op(l);
        n.insert("order".into(), Value::from(i as i64));
        result.push(Value::Object(n));
    }
    checked(result)
}

/// `image(m, bytes, placement)`: embedded PNG/JPEG object.
pub fn image(_m: &Model, bytes: &[u8], placement: &Map) -> Result<Vec<Value>> {
    let mut o = placement.clone();
    o.insert("shape".into(), Value::String("image".into()));
    o.insert("image".into(), Value::String(image_asset::from_bytes(bytes)?));
    let layer = layer_or_base(&o)?;
    checked(vec![Value::Object(actions::drawing(&o, &layer)?)])
}

/// `template(m, input)`: page template SVG, or an infinite-canvas guide object.
pub fn template(m: &Model, input: &Map) -> Result<Vec<Value>> {
    let kind = json::string(input, "template")?;
    require(matches!(kind, "blank" | "ruled" | "grid" | "dots" | "planner"), "Unknown template")?;
    let w = opt(input, "w", 1000.0)?;
    let h = opt(input, "h", 1400.0)?;
    let spacing = opt(input, "spacing", 80.0)?;
    require(
        w > 0.0 && h > 0.0 && w <= 1000000.0 && h <= 1000000.0 && (8.0..=1000000.0).contains(&spacing),
        "Invalid template dimensions or spacing",
    )?;
    require((w / spacing + 1.0) * (h / spacing + 1.0) <= 10000.0, "Template has too many marks")?;
    if m.type_ == "infinite" && matches!(kind, "ruled" | "grid" | "dots") {
        let color = match input.get("color") {
            None => 0xff777777u32 as i32,
            v => json::as_int(v)?,
        };
        let page = layer_editor::page(input)?;
        let mut guide = object! {
            "shape" => "template", "template" => kind, "spacing" => json::double(spacing),
            "x" => 0, "y" => 0, "w" => 1, "h" => 1, "color" => color, "width" => 1, "page" => page,
        };
        for key in ["object", "layer"] {
            if input.contains_key(key) {
                guide.insert(key.into(), input.get(key).cloned().unwrap_or(Value::Null));
            }
        }
        let mut result = Vec::new();
        if !is_false(input.get("background")) {
            let layer = format!("template-{}", layer_editor::page(input)?);
            let mut order = -1.0f64;
            for existing in m.layers.values() {
                order = jmath::min(order, opt(existing, "order", 0.0)? - 1.0);
            }
            if !m.layers.contains_key(&layer) {
                result.push(Value::Object(object! {"kind" => "layer", "layer" => layer.clone(), "name" => "Canvas template", "force" => false, "order" => json::double(order)}));
            } else if m.layer_active.get(&layer) == Some(&false) {
                result.push(Value::Object(object! {"kind" => "layer-active", "layer" => layer.clone(), "active" => true}));
            }
            guide.insert("layer".into(), Value::String(layer.clone()));
            json::put_if_absent(&mut guide, "object", Value::String(format!("{}-background", layer)));
        }
        let guide_layer = layer_or_base(&guide)?;
        result.push(Value::Object(actions::drawing(&guide, &guide_layer)?));
        return checked(result);
    }
    let color = match input.get("color") {
        None => -3355444,
        v => json::as_int(v)?,
    };
    let hex = crate::style::ArgbColor(color).to_hex();
    let scale = jmath::max(1.0, jmath::max(w, h) / 1000.0);
    let f = jfmt::double;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\"><g stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\">",
        f(w / scale),
        f(h / scale),
        f(w / scale),
        f(h / scale),
        hex,
        f(((color as u32) >> 24) as f64 / 255.0),
        f(1.0 / scale)
    );
    if kind == "ruled" || kind == "grid" {
        let mut y = spacing;
        while y < h {
            let _ = write!(svg, "<path d=\"M0 {} H{}\"/>", f(y / scale), f(w / scale));
            y += spacing;
        }
    }
    if kind == "grid" {
        let mut x = spacing;
        while x < w {
            let _ = write!(svg, "<path d=\"M{} 0 V{}\"/>", f(x / scale), f(h / scale));
            x += spacing;
        }
    }
    if kind == "dots" {
        let mut y = spacing;
        while y < h {
            let mut x = spacing;
            while x < w {
                let _ = write!(svg, "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>", f(x / scale), f(y / scale), f(2.0 / scale), hex);
                x += spacing;
            }
            y += spacing;
        }
    }
    if kind == "planner" {
        for row in 0..=7 {
            let _ = write!(svg, "<path d=\"M0 {} H{}\"/>", f(row as f64 * h / 7.0 / scale), f(w / scale));
        }
        let _ = write!(svg, "<path d=\"M{} 0 V{}\"/>", f(w * 0.2 / scale), f(h / scale));
    }
    svg.push_str("</g></svg>");
    let mut o = object! {"shape" => "svg", "svg" => svg, "w" => json::double(w), "h" => json::double(h)};
    for key in ["object", "layer", "page", "x", "y"] {
        if input.contains_key(key) {
            o.insert(key.into(), input.get(key).cloned().unwrap_or(Value::Null));
        }
    }
    let mut result = Vec::new();
    if !is_false(input.get("background")) {
        let layer = format!("template-{}", layer_editor::page(input)?);
        let mut order = -1.0f64;
        for existing in m.layers.values() {
            order = jmath::min(order, opt(existing, "order", 0.0)? - 1.0);
        }
        if !m.layers.contains_key(&layer) {
            result.push(Value::Object(object! {"kind" => "layer", "layer" => layer.clone(), "name" => "Page template", "force" => false, "order" => json::double(order)}));
        } else if m.layer_active.get(&layer) == Some(&false) {
            result.push(Value::Object(object! {"kind" => "layer-active", "layer" => layer.clone(), "active" => true}));
        }
        o.insert("layer".into(), Value::String(layer.clone()));
        json::put_if_absent(&mut o, "object", Value::String(format!("{}-background", layer)));
    }
    let object_layer = layer_or_base(&o)?;
    let drawing = actions::drawing(&o, &object_layer)?;
    let drawing_id = drawing.get("object").cloned().unwrap_or(Value::Null);
    result.push(Value::Object(drawing));
    if json::key_of(Some(&drawing_id)).and_then(|k| m.object_active.get(k)) == Some(&false) {
        result.push(Value::Object(object! {"kind" => "object-active", "object" => drawing_id, "active" => true}));
    }
    checked(result)
}

/// `fill(m, input)`: region fill at (x, y), native object fill, or a closed stroke fill.
pub fn fill(m: &Model, input: &Map) -> Result<Vec<Value>> {
    if input.contains_key("x") && input.contains_key("y") {
        return region_fill::create(m, input);
    }
    model::color(input, "color")?;
    let color = json::as_int(input.get("color"))?;
    let layer = layer_or_base(input)?;
    if input.contains_key("object") {
        let editor = LayerEditor::new(m, &layer, layer_editor::page(input)?)?;
        let old = &editor.resolve(&EntityKind::object(), &EntityId::from(json::string(input, "object")?))?.content;
        require(
            !matches!(old.get("shape").and_then(Value::as_str), Some("svg" | "image" | "text" | "line" | "arrow" | "wave" | "curve" | "polyline")),
            "Object has no enclosed native fill",
        )?;
        let mut o = clone_op(old);
        o.insert("fill".into(), Value::from(color));
        return checked(vec![Value::Object(o)]);
    }
    let editor = LayerEditor::new(m, &layer, layer_editor::page(input)?)?;
    let s = editor.resolve(&EntityKind::stroke(), &EntityId::from(json::string(input, "stroke")?))?.content.clone();
    let p = json::list_at(&s, "points")?;
    require(p.len() >= 3, "Fill needs a closed outline")?;
    let first = point(&p[0])?;
    let last = point(&p[p.len() - 1])?;
    require(
        jmath::hypot(first[0] - last[0], first[1] - last[1]) <= jmath::max(20.0, json::number(&s, "width")? * 3.0),
        "Stroke outline is open",
    )?;
    let b = bounds(p)?;
    let pad = json::number(&s, "width")?;
    let w = b[2] - b[0] + 2.0 * pad;
    let h = b[3] - b[1] + 2.0 * pad;
    let scale = jmath::max(1.0, jmath::max(w, h) / 1000.0);
    let mut polygon = String::new();
    for v in p {
        let a = point(v)?;
        jfmt::append_double(&mut polygon, (a[0] - b[0] + pad) / scale);
        polygon.push(',');
        jfmt::append_double(&mut polygon, (a[1] - b[1] + pad) / scale);
        polygon.push(' ');
    }
    let f = jfmt::double;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"><polygon points=\"{}\" stroke=\"{}\" stroke-width=\"{}\" fill=\"{}\" fill-opacity=\"{}\"/></svg>",
        f(w / scale),
        f(h / scale),
        f(w / scale),
        f(h / scale),
        polygon,
        crate::style::ArgbColor(json::as_int(s.get("color"))?).to_hex(),
        f(json::number(&s, "width")? / scale),
        crate::style::ArgbColor(color).to_hex(),
        f(((color as u32) >> 24) as f64 / 255.0)
    );
    let mut filled = actions::drawing(
        &object! {
            "shape" => "svg", "svg" => svg, "x" => json::double(b[0] - pad), "y" => json::double(b[1] - pad),
            "w" => json::double(w), "h" => json::double(h), "page" => s.get("page").cloned().unwrap_or(Value::Null),
        },
        &LayerId::from(json::string(&s, "layer")?),
    )?;
    if s.contains_key("erasures") {
        filled = DrawItem::from_map(m, filled)?.erase(&DrawItem::erasures(&s)?)?;
    }
    checked(vec![Value::Object(filled)])
}

fn point(p: &Value) -> Result<[f64; 3]> {
    let a = json::list(p)?;
    require(a.len() >= 2 && a.len() <= 3, "Point needs x/y")?;
    let r = [json::as_double(a.first())?, json::as_double(a.get(1))?, if a.len() == 3 { json::as_double(a.get(2))? } else { 1.0 }];
    for v in r {
        require(v.is_finite() && v.abs() <= 1000000.0, "Invalid eraser point")?;
    }
    Ok(r)
}

fn bounds(pts: &[Value]) -> Result<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for o in pts {
        let p = point(o)?;
        b[0] = jmath::min(b[0], p[0]);
        b[1] = jmath::min(b[1], p[1]);
        b[2] = jmath::max(b[2], p[0]);
        b[3] = jmath::max(b[3], p[1]);
    }
    Ok(b)
}

/// `erase(m, input, factory)`: hide, partially mask, or clear a page.
pub fn erase(m: &Model, input: &Map, factory: &dyn CoverageFactory) -> Result<Vec<Value>> {
    let mode = json::string_or(input, "mode", "stroke")?.ok_or_else(Error::null_pointer)?.to_string();
    require(matches!(mode.as_str(), "stroke" | "item" | "partial" | "area" | "page"), "Unknown eraser mode")?;
    let editor = LayerEditor::for_erase(m, &layer_or_base(input)?, layer_editor::page(input)?)?;
    let radius = opt(input, "radius", 12.0)?;
    require(radius.is_finite() && (1.0..=10000.0).contains(&radius), "Invalid eraser radius")?;
    let regions = if mode == "page" { Erasures::default() } else { erase_regions(input)? };
    let mut hits: Vec<(&DrawItem, Erasures)> = Vec::new();
    for item in editor.items() {
        let mut hit = mode == "page";
        let mut touching = Erasures::default();
        for region in &regions {
            if item.intersects(region, factory)? {
                hit = true;
                if mode == "partial" {
                    touching.push(region.clone());
                } else {
                    break;
                }
            }
        }
        if hit {
            hits.push((item, touching));
        }
    }
    let mut out = Vec::new();
    for (item, touching) in hits {
        out.push(Value::Object(if mode == "partial" {
            item.erase(&touching)?
        } else {
            let kind = if item.kind == "stroke" { "active" } else { "object-active" };
            object! {"kind" => kind, item.kind.as_str() => item.id.clone(), "active" => false}
        }));
    }
    checked(out)
}

/// `eraseRegions(input)`: exact swept capsules for feedback and durable erasing.
pub fn erase_regions(input: &Map) -> Result<Erasures> {
    let radius = opt(input, "radius", 12.0)?;
    require(radius.is_finite() && (1.0..=10000.0).contains(&radius), "Invalid eraser radius")?;
    let mut regions = Erasures::default();
    let path = json::list_at(input, "points")?;
    require(!path.is_empty() && path.len() <= 2000, "Eraser needs 1–2000 points")?;
    let mut distinct: Vec<[f64; 3]> = Vec::new();
    for raw in path {
        let p = point(raw)?;
        let size = distinct.len();
        if size > 0 {
            let last = distinct[size - 1];
            if p[0] == last[0] && p[1] == last[1] {
                continue;
            }
            if size > 1 {
                let a = distinct[size - 2];
                let ax = last[0] - a[0];
                let ay = last[1] - a[1];
                let bx = p[0] - last[0];
                let by = p[1] - last[1];
                if ax * by == ay * bx && ax * bx + ay * by >= 0.0 {
                    distinct[size - 1] = p;
                    continue;
                }
            }
        }
        distinct.push(p);
    }
    let mut prior = distinct[0];
    if distinct.len() == 1 {
        let at = DocPoint { x: prior[0], y: prior[1] };
        regions.push(Polygon::capsule(Segment { start: at, end: at }, Radius(radius)));
    }
    for next in distinct.iter().skip(1) {
        regions.push(Polygon::capsule(Segment { start: DocPoint { x: prior[0], y: prior[1] }, end: DocPoint { x: next[0], y: next[1] } }, Radius(radius)));
        prior = *next;
    }
    Ok(regions)
}

/// `Boolean.FALSE.equals(value)`.
#[inline]
fn is_false(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(false)))
}
