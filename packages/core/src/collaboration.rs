//! Port of `Collaboration.java`: selection lease and transform rules.

use crate::error::{require, require_with, Error, Result};
use crate::clock::EpochMillis;
use crate::geometry::Polygon;
use crate::ids::{ActorId, EntityId, EntityKey, EntityKind, LayerId, PageNumber, SessionId};
use crate::jmath;
use crate::json::{self, Map, Value};
use crate::layer_editor::LayerEditor;
use crate::model::Model;
use crate::object;
use crate::render;
use crate::store::{self, State};
use std::collections::HashSet;

/// `Collaboration.LEASE_MILLIS`.
pub const LEASE_MILLIS: i64 = 15000;

/// A uniform scale about the selection centre followed by a translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionTransform {
    pub scale: f64,
    pub dx: f64,
    pub dy: f64,
}

fn leases_of(state: &Map) -> Result<Map> {
    match state.get("leases") {
        None => Ok(Map::new()),
        Some(v) => Ok(json::map(v)?.clone()),
    }
}

/// `requireSelection(state, model, actor, session, entities)`: resolve a
/// session's entire batch inside its layer before generating changes.
pub fn require_selection(state: &mut Map, model: &Model, actor: &ActorId, session: &SessionId, entities: &[Value]) -> Result<()> {
    let (actor, session): (&str, &str) = (actor, session);
    expire(state, EpochMillis::now())?;
    let leases = leases_of(state)?;
    let mut editor: Option<LayerEditor> = None;
    require(!entities.is_empty(), "Selection needs entities")?;
    let zero = Value::from(0);
    let empty = Value::Object(Map::new());
    for value in entities {
        let r = json::map(value)?;
        let k = key_str(json::string(r, "kind")?, json::string(r, "id")?, Some(json::get_or_default(r, "page", &zero)));
        let lease = json::map(json::get_or_default(&leases, &k, &empty))?;
        require(
            lease.get("session").and_then(Value::as_str) == Some(session) && lease.get("actor").and_then(Value::as_str) == Some(actor),
            "Edit requires an active selection lease for every entity",
        )?;
        let layer = json::string_or(lease, "layer", "base")?.ok_or_else(Error::null_pointer)?.to_string();
        let page = json::as_int(Some(json::get_or_default(lease, "page", &zero)))?;
        match &editor {
            None => editor = Some(LayerEditor::new(model, &LayerId::from(&layer), PageNumber(page))?),
            Some(e) => require(e.layer == layer.as_str() && e.page == PageNumber(page), "Selection spans layers/pages")?,
        }
    }
    editor.unwrap().resolve_refs(entities)?;
    Ok(())
}

/// `Collaboration.key(kind, id, page)`: `page:kind:id` (page `0` when null).
pub fn key(kind: &EntityKind, id: &EntityId, page: Option<&Value>) -> EntityKey {
    EntityKey::from(key_str(kind.as_ref(), id.as_ref(), page))
}

fn key_str(kind: &str, id: &str, page: Option<&Value>) -> String {
    let page = match page {
        None | Some(Value::Null) => "0".to_string(),
        Some(p) => json::java_string(Some(p)),
    };
    format!("{}:{}:{}", page, kind, id)
}

/// Whether [`expire`] would change `state` (a missing `leases` key or an
/// expired lease). Lets copy-on-write readers skip the mutable borrow, since
/// `expire` otherwise rewrites `leases` with an equal value in place.
pub fn expire_changes(state: &Map, now: EpochMillis) -> Result<bool> {
    let now = now.0;
    let Some(leases) = state.get("leases") else {
        return Ok(true);
    };
    let zero = Value::from(0);
    for lease in json::map(leases)?.values() {
        if json::as_long(Some(json::get_or_default(json::map(lease)?, "expires", &zero)))? <= now {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `Collaboration.expire(state, now)`: drop expired leases.
pub fn expire(state: &mut Map, now: EpochMillis) -> Result<()> {
    let now = now.0;
    let mut leases = leases_of(state)?;
    let keys: Vec<String> = leases.keys().cloned().collect();
    let zero = Value::from(0);
    for k in keys {
        let lease = json::map(&leases[&k])?;
        if json::as_long(Some(json::get_or_default(lease, "expires", &zero)))? <= now {
            leases.shift_remove(&k);
        }
    }
    state.insert("leases".into(), Value::Object(leases));
    Ok(())
}

/// `Collaboration.conflicts(state, actor, entities)`: leases held by others.
pub fn conflicts(state: &Map, actor: &ActorId, entities: &[Value]) -> Result<Vec<Value>> {
    let actor: &str = actor;
    let mut blocked = Vec::new();
    let leases = leases_of(state)?;
    let sessions = match state.get("activeSessions") {
        None => Map::new(),
        Some(v) => json::map(v)?.clone(),
    };
    let active = json::as_string_opt(sessions.get(actor))?.map(str::to_string);
    let zero = Value::from(0);
    for value in entities {
        let entity = json::map(value)?;
        let k = key_str(json::string(entity, "kind")?, json::string(entity, "id")?, entity.get("page"));
        let suffix = format!(":{}:{}", json::java_string(entity.get("kind")), json::java_string(entity.get("id")));
        for (entry_key, entry_value) in &leases {
            if *entry_key != k && !entry_key.ends_with(&suffix) {
                continue;
            }
            let lease = json::map(entry_value)?;
            let session_matches = json::objects_equal(active.as_ref().map(|s| Value::String(s.clone())).as_ref(), lease.get("session"));
            if !lease.is_empty() && (lease.get("actor").and_then(Value::as_str) != Some(actor) || !session_matches) {
                blocked.push(Value::Object(object! {
                    "kind" => entity.get("kind").cloned().unwrap_or(Value::Null),
                    "id" => entity.get("id").cloned().unwrap_or(Value::Null),
                    "page" => json::get_or_default(entity, "page", &zero).clone(),
                    "actor" => lease.get("actor").cloned().unwrap_or(Value::Null),
                    "session" => lease.get("session").cloned().unwrap_or(Value::Null),
                }));
            }
        }
    }
    Ok(blocked)
}

/// `Collaboration.affected(operations)`: unique entity refs touched.
pub fn affected(operations: &[Value]) -> Result<Vec<Value>> {
    let mut unique = Map::new();
    for value in operations {
        let op = json::map(value)?;
        if json::java_string(op.get("kind")) == "batch" {
            for nested in json::list(op.get("actions").unwrap_or(&Value::Null))? {
                add(&mut unique, json::map(nested)?);
            }
        } else {
            add(&mut unique, op);
        }
    }
    Ok(unique.into_iter().map(|(_, v)| v).collect())
}

fn add(out: &mut Map, op: &Map) {
    let kind = json::java_string(op.get("kind"));
    let mut entity = match kind.as_str() {
        "object" | "object-active" => Some("object".to_string()),
        "stroke" | "active" => Some("stroke".to_string()),
        _ => None,
    };
    if entity.is_none() && (kind == "object" || kind == "stroke") && op.contains_key("id") {
        entity = Some(kind.clone());
    }
    if let Some(entity) = entity {
        let id_value = match op.get(entity.as_str()) {
            Some(v) => Some(v),
            None => op.get("id"),
        };
        let id = json::java_string(id_value);
        let mut r = object! {"kind" => entity.clone(), "id" => id.clone()};
        if op.contains_key("page") {
            r.insert("page".into(), op.get("page").cloned().unwrap_or(Value::Null));
        }
        json::put_if_absent(out, &key_str(&entity, &id, op.get("page")), Value::Object(r));
    }
}

fn expand_touching(model: &Model, expanded: &mut Vec<Map>) -> Result<()> {
    let mut touched: HashSet<String> = HashSet::new();
    for entity in expanded.iter() {
        if entity.get("kind").and_then(Value::as_str) == Some("object") {
            touched.insert(json::java_string(entity.get("id")));
        }
    }
    let zero = Value::from(0);
    for o in model.objects.values() {
        let hit = |k: &str| json::key_of(o.get(k)).is_some_and(|v| touched.contains(v));
        if hit("from") || hit("to") {
            expanded.push(object! {"kind" => "object", "id" => o.get("object").cloned().unwrap_or(Value::Null), "page" => json::get_or_default(o, "page", &zero).clone()});
        }
    }
    for entity in expanded.iter_mut() {
        if !entity.contains_key("page") {
            let kind = json::string(entity, "kind")?;
            let id = json::string(entity, "id")?;
            let table = if kind == "object" { &model.objects } else { &model.strokes };
            if let Some(old) = table.get(id) {
                let page = json::get_or_default(old, "page", &zero).clone();
                entity.insert("page".into(), page);
            }
        }
    }
    Ok(())
}

fn to_maps(values: Vec<Value>) -> Vec<Map> {
    values
        .into_iter()
        .map(|v| match v {
            Value::Object(m) => m,
            _ => Map::new(),
        })
        .collect()
}

/// `requireUnlocked(state, actor, operations, now)`.
pub fn require_unlocked(state: &mut State, actor: &ActorId, operations: &[Value], now: EpochMillis) -> Result<()> {
    expire(state, now)?;
    let model = store::model(state)?;
    let mut expanded = to_maps(affected(operations)?);
    for value in operations {
        let op = json::map(value)?;
        if op.get("kind").and_then(Value::as_str) == Some("batch") {
            for nested in json::list(op.get("actions").unwrap_or(&Value::Null))? {
                expand_layer(&model, json::map(nested)?, &mut expanded);
            }
        } else {
            expand_layer(&model, op, &mut expanded);
        }
    }
    expand_touching(&model, &mut expanded)?;
    let expanded: Vec<Value> = expanded.into_iter().map(Value::Object).collect();
    let blocked = conflicts(state, actor, &expanded)?;
    require_with(blocked.is_empty(), || format!("Locked entities: {}", json::write_list(&blocked)))
}

/// `requireIncomingUnlocked(state, operation, now)`: guard a child's raw ingress.
pub fn require_incoming_unlocked(state: &mut State, operation: &Map, now: EpochMillis) -> Result<()> {
    expire(state, now)?;
    let operations: Vec<Value> = if operation.get("kind").and_then(Value::as_str) == Some("batch") {
        json::list(operation.get("actions").unwrap_or(&Value::Null))?.clone()
    } else {
        vec![Value::Object(operation.clone())]
    };
    let entities = affected(&operations)?;
    let model = store::model(state)?;
    let mut expanded = to_maps(entities);
    for value in &operations {
        expand_layer(&model, json::map(value)?, &mut expanded);
    }
    expand_touching(&model, &mut expanded)?;
    let empty = Value::String(String::new());
    let actor = json::java_string(Some(json::get_or_default(operation, "actor", &empty)));
    let session = json::java_string(Some(json::get_or_default(operation, "session", &empty)));
    let leases = leases_of(state)?;
    let refs: Vec<String> = expanded
        .iter()
        .map(|e| key_str(&json::java_string(e.get("kind")), &json::java_string(e.get("id")), e.get("page")))
        .collect();
    // Java iterated a HashSet<String> here; reproduce its order for the message.
    let mut blocked = Vec::new();
    for r in json::java_hash_set_order(refs) {
        let suffix = format!(":{}", &r[r.find(':').map(|i| i + 1).unwrap_or(0)..]);
        for (entry_key, entry_value) in &leases {
            if *entry_key != r && !entry_key.ends_with(&suffix) {
                continue;
            }
            let lease = json::map(entry_value)?;
            if !lease.is_empty()
                && (lease.get("actor").and_then(Value::as_str) != Some(actor.as_str())
                    || lease.get("session").and_then(Value::as_str) != Some(session.as_str()))
            {
                blocked.push(Value::Object(object! {
                    "key" => entry_key.clone(),
                    "actor" => lease.get("actor").cloned().unwrap_or(Value::Null),
                    "session" => lease.get("session").cloned().unwrap_or(Value::Null),
                }));
            }
        }
    }
    require_with(blocked.is_empty(), || format!("Locked entities: {}", json::write_list(&blocked)))
}

fn expand_layer(model: &Model, op: &Map, out: &mut Vec<Map>) {
    let kind = json::java_string(op.get("kind"));
    let layer = json::java_string(op.get("layer"));
    let mut affects = kind == "layer-active";
    if kind == "layer" && op.contains_key("order") {
        let old = model.layers.get(&layer);
        affects = match old {
            None => true,
            Some(old) => !json::objects_equal(old.get("order"), op.get("order")),
        };
    }
    if !affects {
        return;
    }
    let zero = Value::from(0);
    for value in model.objects.values() {
        if value.get("layer").and_then(Value::as_str) == Some(layer.as_str()) {
            out.push(object! {"kind" => "object", "id" => value.get("object").cloned().unwrap_or(Value::Null), "page" => json::get_or_default(value, "page", &zero).clone()});
        }
    }
    for value in model.strokes.values() {
        if value.get("layer").and_then(Value::as_str) == Some(layer.as_str()) {
            out.push(object! {"kind" => "stroke", "id" => value.get("stroke").cloned().unwrap_or(Value::Null), "page" => json::get_or_default(value, "page", &zero).clone()});
        }
    }
}

/// `transform(model, entities, scale, dx, dy)`.
pub fn transform_simple(model: &Model, entities: &[Value], transform_by: SelectionTransform) -> Result<Vec<Value>> {
    let SelectionTransform { scale, dx, dy } = transform_by;
    transform(model, entities, &object! {"scale" => json::double(scale), "dx" => json::double(dx), "dy" => json::double(dy)})
}

fn num(o: &Map, k: &str) -> Result<f64> {
    json::as_double(o.get(k))
}

fn spec_number(spec: &Map, key: &str, default: f64) -> Result<f64> {
    match spec.get(key) {
        None => Ok(default),
        v => json::as_double(v),
    }
}

/// `transform(model, entities, spec)`: selected objects and strokes into replacement actions.
pub fn transform(model: &Model, entities: &[Value], spec: &Map) -> Result<Vec<Value>> {
    for key in ["scale", "dx", "dy", "rotation", "pivotX", "pivotY"] {
        if spec.contains_key(key) {
            require_with(json::is_number(spec.get(key)), || format!("Invalid transform field: {}", key))?;
        }
    }
    for key in ["flipX", "flipY"] {
        if spec.contains_key(key) {
            require_with(matches!(spec.get(key), Some(Value::Bool(_))), || format!("Invalid transform field: {}", key))?;
        }
    }
    let scale = spec_number(spec, "scale", 1.0)?;
    let dx = spec_number(spec, "dx", 0.0)?;
    let dy = spec_number(spec, "dy", 0.0)?;
    let rotation = spec_number(spec, "rotation", 0.0)?;
    let flip_x = json::is_true(spec.get("flipX"));
    let flip_y = json::is_true(spec.get("flipY"));
    require(scale.is_finite() && scale > 0.0 && scale <= 100.0, "Scale must be > 0 and <= 100")?;
    require(dx.is_finite() && dy.is_finite() && dx.abs() <= 1000000.0 && dy.abs() <= 1000000.0, "Invalid translation")?;
    require(rotation.is_finite() && rotation.abs() <= 360.0, "Rotation must be within 360 degrees")?;
    let mut selected_objects: HashSet<String> = HashSet::new();
    for value in entities {
        let e = json::map(value)?;
        if e.get("kind").and_then(Value::as_str) == Some("object") {
            selected_objects.insert(json::string(e, "id")?.to_string());
        }
    }
    let selected = |v: Option<&Value>| json::key_of(v).is_some_and(|k| selected_objects.contains(k));
    let attached_of = |old: &Map| {
        matches!(old.get("shape").and_then(Value::as_str), Some("arrow") | Some("arrowLine")) && (old.contains_key("from") || old.contains_key("to"))
    };
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for value in entities {
        let e = json::map(value)?;
        let kind = json::string(e, "kind")?;
        let id = json::string(e, "id")?;
        let old = if kind == "object" { model.objects.get(id) } else { model.strokes.get(id) };
        require_with(old.is_some(), || format!("Unknown transform entity: {}", id))?;
        let old = old.unwrap();
        if kind == "object" {
            let attached = attached_of(old);
            if attached && (selected(old.get("from")) || selected(old.get("to"))) {
                continue;
            }
            let (mut x, mut y, mut w, mut h) = (num(old, "x")?, num(old, "y")?, num(old, "w")?, num(old, "h")?);
            if attached {
                let ends = render::endpoints(model, old)?;
                x = ends.start.x;
                y = ends.start.y;
                w = ends.end.x - ends.start.x;
                h = ends.end.y - ends.start.y;
            }
            min_x = jmath::min(min_x, jmath::min(x, x + w));
            max_x = jmath::max(max_x, jmath::max(x, x + w));
            min_y = jmath::min(min_y, jmath::min(y, y + h));
            max_y = jmath::max(max_y, jmath::max(y, y + h));
        } else {
            for point in json::list_at(old, "points")? {
                let p = json::list(point)?;
                min_x = jmath::min(min_x, json::as_double(p.first())?);
                max_x = jmath::max(max_x, json::as_double(p.first())?);
                min_y = jmath::min(min_y, json::as_double(p.get(1))?);
                max_y = jmath::max(max_y, json::as_double(p.get(1))?);
            }
        }
    }
    require(min_x != f64::INFINITY, "Transform needs entities")?;
    let px = spec_number(spec, "pivotX", (min_x + max_x) / 2.0)?;
    let py = spec_number(spec, "pivotY", (min_y + max_y) / 2.0)?;
    require(px.is_finite() && py.is_finite() && px.abs() <= 1000000.0 && py.abs() <= 1000000.0, "Invalid transform pivot")?;
    let r = jmath::to_radians(rotation);
    let cos = jmath::cos(r);
    let sin = jmath::sin(r);
    let fx = if flip_x { -1.0 } else { 1.0 };
    let fy = if flip_y { -1.0 } else { 1.0 };
    let point = |p: [f64; 2]| -> [f64; 2] {
        let x = (p[0] - px) * fx * scale;
        let y = (p[1] - py) * fy * scale;
        [px + x * cos - y * sin + dx, py + x * sin + y * cos + dy]
    };
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for value in entities {
        let e = json::map(value)?;
        let kind = json::string(e, "kind")?;
        let id = json::string(e, "id")?;
        require(seen.insert(key_str(kind, id, e.get("page"))), "Duplicate transform entity")?;
        if kind == "object" {
            let old = model.objects.get(id);
            require_with(old.is_some(), || format!("Unknown object: {}", id))?;
            let old = old.unwrap();
            let attached = attached_of(old);
            if attached && (selected(old.get("from")) || selected(old.get("to"))) {
                continue;
            }
            let mut next: Map = (**old).clone();
            if attached {
                let ends = render::endpoints(model, old)?;
                next.insert("x".into(), json::double(ends.start.x));
                next.insert("y".into(), json::double(ends.start.y));
                next.insert("w".into(), json::double(ends.end.x - ends.start.x));
                next.insert("h".into(), json::double(ends.end.y - ends.start.y));
                next.shift_remove("from");
                next.shift_remove("to");
            }
            let (x, y, w, h) = (num(&next, "x")?, num(&next, "y")?, num(&next, "w")?, num(&next, "h")?);
            let center = point([x + w / 2.0, y + h / 2.0]);
            next.insert("x".into(), json::double(center[0] - w * scale / 2.0));
            next.insert("y".into(), json::double(center[1] - h * scale / 2.0));
            next.insert("w".into(), json::double(w * scale));
            next.insert("h".into(), json::double(h * scale));
            let old_rotation = match old.get("rotation") {
                None => 0.0,
                v => json::as_double(v)?,
            };
            next.insert("rotation".into(), json::double((rotation + if flip_x ^ flip_y { -old_rotation } else { old_rotation }) % 360.0));
            next.insert("flipX".into(), Value::Bool(json::is_true(old.get("flipX")) ^ flip_x));
            next.insert("flipY".into(), Value::Bool(json::is_true(old.get("flipY")) ^ flip_y));
            if old.contains_key("fontSize") {
                next.insert("fontSize".into(), json::double(num(old, "fontSize")? * scale));
            }
            Model::validate(&next)?;
            out.push(Value::Object(next));
        } else if kind == "stroke" {
            let old = model.strokes.get(id);
            require_with(old.is_some(), || format!("Unknown stroke: {}", id))?;
            let old = old.unwrap();
            let mut next: Map = (**old).clone();
            let mut points = Vec::new();
            for p in json::list_at(old, "points")? {
                let xyz = json::list(p)?;
                let target = point([json::as_double(xyz.first())?, json::as_double(xyz.get(1))?]);
                points.push(Value::Array(vec![json::double(target[0]), json::double(target[1]), xyz.get(2).cloned().unwrap_or(Value::Null)]));
            }
            next.insert("points".into(), Value::Array(points));
            next.insert("width".into(), json::double(num(old, "width")? * scale));
            if old.contains_key("erasures") {
                let mut masks = Vec::new();
                for raw in json::list_at(old, "erasures")? {
                    let mut polygon = Polygon::from_json(raw)?;
                    for v in polygon.iter_mut() {
                        let x = v.x * fx * scale;
                        let y = v.y * fy * scale;
                        v.x = x * cos - y * sin;
                        v.y = x * sin + y * cos;
                    }
                    masks.push(polygon.to_json());
                }
                next.insert("erasures".into(), Value::Array(masks));
            }
            Model::validate(&next)?;
            out.push(Value::Object(next));
        } else {
            return Err(Error::illegal_argument("Transform kind must be object or stroke"));
        }
    }
    require(!out.is_empty() && out.len() <= 200, "Transform needs 1–200 entities")?;
    Ok(out)
}
