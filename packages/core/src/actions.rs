//! Port of `Actions.java`: shared action builders.

use crate::error::{require, Error, Result};
use crate::ids;
use crate::json::{self, Map, Value};
use crate::model::Model;
use crate::object;
use crate::{image_asset, svg_asset};
use std::collections::HashSet;

/// `Actions.Build`: produces the requested operations from the current model.
pub type Build<'a> = dyn FnMut(&mut Model) -> Result<Vec<Value>> + 'a;

/// `Actions.id()`.
pub fn id() -> String {
    ids::random_uuid()
}

/// `Actions.drawing(input, layer)`: object operation with defaults, validated.
pub fn drawing(input: &Map, layer: &str) -> Result<Map> {
    let mut input = input.clone();
    if input.get("shape").and_then(Value::as_str) == Some("svg") {
        let size = svg_asset::validate(json::string(&input, "svg")?)?;
        json::put_if_absent(&mut input, "w", json::double(size[0]));
        json::put_if_absent(&mut input, "h", json::double(size[1]));
    }
    if input.get("shape").and_then(Value::as_str) == Some("image") {
        let size = image_asset::validate(Some(json::string(&input, "image")?))?;
        json::put_if_absent(&mut input, "w", json::double(size[0]));
        json::put_if_absent(&mut input, "h", json::double(size[1]));
    }
    let mut o = input;
    o.insert("kind".into(), "object".into());
    json::put_if_absent(&mut o, "object", Value::String(id()));
    json::put_if_absent(&mut o, "layer", Value::String(layer.to_string()));
    json::put_if_absent(&mut o, "page", Value::from(0));
    json::put_if_absent(&mut o, "color", Value::from(-16777216));
    json::put_if_absent(&mut o, "width", Value::from(3));
    json::put_if_absent(&mut o, "fontSize", Value::from(24));
    json::put_if_absent(&mut o, "text", Value::from(""));
    json::put_if_absent(&mut o, "x", Value::from(0));
    json::put_if_absent(&mut o, "y", Value::from(0));
    json::put_if_absent(&mut o, "w", Value::from(160));
    json::put_if_absent(&mut o, "h", Value::from(80));
    Model::validate(&o)?;
    Ok(o)
}

/// `Actions.diagram(model, input)`: a new layer plus its nodes and connectors.
pub fn diagram(model: &Model, input: &Map) -> Result<Vec<Value>> {
    let generated = id();
    let layer = json::string_or(input, "layer", &generated)?.ok_or_else(Error::null_pointer)?.to_string();
    require(!model.layers.contains_key(&layer), "Diagram layer already exists")?;
    let name = json::get_or_default_owned(input, "name", Value::from("Diagram"));
    let mut result: Vec<Map> = vec![object! {"kind" => "layer", "layer" => layer.clone(), "name" => name, "force" => false}];
    let mut ids: HashSet<String> = model.objects.keys().cloned().collect();
    let objects = json::list_at(input, "objects")?;
    require(!objects.is_empty(), "Diagram needs objects")?;
    for v in objects {
        let mut o = drawing(json::map(v)?, &layer)?;
        o.insert("layer".into(), Value::String(layer.clone()));
        require(ids.insert(json::string(&o, "object")?.to_string()), "Duplicate object ID")?;
        result.push(o);
    }
    for o in &result {
        for k in ["from", "to"] {
            if let Some(reference) = o.get(k) {
                if reference.is_null() {
                    return Err(Error::null_pointer());
                }
                let mut target: Option<&Map> = None;
                for c in &result {
                    if Some(reference) == c.get("object") {
                        target = Some(c);
                    }
                }
                require(
                    matches!(target, Some(t) if t.get("shape").and_then(Value::as_str) != Some("arrow") && !json::objects_equal(t.get("object"), o.get("object"))),
                    "Connector endpoint must be a node in this diagram",
                )?;
                let target = target.unwrap();
                require(json::as_int(o.get("page"))? == json::as_int(target.get("page"))?, "Connector endpoints must be on the same page")?;
            }
        }
    }
    Ok(result.into_iter().map(Value::Object).collect())
}

/// `Actions.validateReferences(model, operations)`.
pub fn validate_references(model: &Model, operations: &[Value]) -> Result<()> {
    for v in operations {
        let op = json::map(v)?;
        let kind = json::string(op, "kind")?;
        if kind == "object" || kind == "stroke" {
            require(json::key_of(op.get("layer")).is_some_and(|l| model.layers.contains_key(l)), "Unknown layer")?;
            require(model.type_ == "pages" || json::number(op, "page")? == 0.0, "Only multipage documents support page > 0")?;
        }
    }
    for o in model.objects.values() {
        for key in ["from", "to"] {
            if o.contains_key(key) {
                let target = json::key_of(o.get(key)).and_then(|k| model.objects.get(k));
                let arrow = |m: &Map| matches!(m.get("shape").and_then(Value::as_str), Some("arrow") | Some("arrowLine"));
                require(arrow(o) && matches!(target, Some(t) if !arrow(t)), "Connector endpoint must be a non-arrow node")?;
                require(json::number(o, "page")? == json::number(target.unwrap(), "page")?, "Connector endpoints must be on the same page")?;
            }
        }
    }
    Ok(())
}

/// `Actions.inverse(model, op)`: the operation that undoes `op`.
pub fn inverse(model: &Model, op: &Map) -> Result<Value> {
    let k = json::string(op, "kind")?;
    if k == "conflict-status" {
        let active = json::key_of(op.get("conflict")).and_then(|c| model.conflict_active.get(c)) != Some(&false);
        return Ok(Value::Object(object! {"kind" => k, "conflict" => op.get("conflict").cloned().unwrap_or(Value::Null), "active" => active}));
    }
    let key = if k.starts_with("object") { "object" } else if k.starts_with("layer") { "layer" } else { "stroke" };
    let id = if op.contains_key(key) { json::string(op, key)?.to_string() } else { String::new() };
    if k == "document" {
        return Err(Error::illegal_argument("Document creation is not undoable"));
    }
    if k == "object" || k == "stroke" || k == "layer" {
        let table = match k {
            "object" => &model.objects,
            "layer" => &model.layers,
            _ => &model.strokes,
        };
        if let Some(old) = table.get(&id) {
            return Ok(Value::Object((**old).clone()));
        }
        let kind = if k == "stroke" { "active".to_string() } else { format!("{}-active", k) };
        return Ok(Value::Object(object! {"kind" => kind, key => id, "active" => false}));
    }
    let active = match k {
        "active" => &model.active,
        "object-active" => &model.object_active,
        _ => &model.layer_active,
    };
    let value = active.get(&id) != Some(&false);
    Ok(Value::Object(object! {"kind" => k, key => id, "active" => value}))
}
