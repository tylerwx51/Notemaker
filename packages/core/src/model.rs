//! Port of `Model.java`: deterministic replay. Server order, followed by
//! unacknowledged local operations.
//!
//! Entity snapshots (`layers`, `strokes`, `objects`) are `Arc<Map>`: like the
//! Java maps they are immutable after publication and shared between model
//! copies, and the indexes themselves are copy-on-write ([`Shared`]), so
//! `Model::clone` is O(1) until a copy mutates.

use crate::error::{require, require_with, Error, Result};
use crate::ids::{LayerId, PageNumber, ShapeId};
use crate::ids;
use crate::jmath;
use crate::json::{self, jlen, Map, Value};
use crate::object;
use crate::{gif_asset, image_asset, shape_catalog, svg_asset};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// An immutable entity snapshot (layer, stroke or object operation).
pub type Entity = Arc<Map>;

/// A copy-on-write model index. Java's `new Model(other)` copied each map's
/// spine of shared references; a clone here shares the whole map until one
/// side mutates it (`DerefMut` → `Arc::make_mut`), so the several model
/// copies per edit and per read cost O(1) instead of O(entities) each.
/// Equality short-circuits on identity like Java's `equals`.
#[derive(Default)]
pub struct Shared<T: Clone>(Arc<T>);

impl<T: Clone> Shared<T> {
    pub fn new(value: T) -> Shared<T> {
        Shared(Arc::new(value))
    }
    /// Whether both share one map (Java reference equality).
    pub fn ptr_eq(a: &Shared<T>, b: &Shared<T>) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }
}

impl<T: Clone> Clone for Shared<T> {
    fn clone(&self) -> Shared<T> {
        Shared(self.0.clone())
    }
}

impl<T: Clone> std::ops::Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> std::ops::DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut T {
        Arc::make_mut(&mut self.0)
    }
}

impl<T: Clone + PartialEq> PartialEq for Shared<T> {
    fn eq(&self, other: &Shared<T>) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || *self.0 == *other.0
    }
}

impl<T: Clone + PartialEq> PartialEq<T> for Shared<T> {
    fn eq(&self, other: &T) -> bool {
        *self.0 == *other
    }
}

impl<T: Clone + std::fmt::Debug> std::fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: Clone> From<T> for Shared<T> {
    fn from(value: T) -> Shared<T> {
        Shared::new(value)
    }
}

impl<'a, T: Clone> IntoIterator for &'a Shared<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        (&*self.0).into_iter()
    }
}

#[derive(Default)]
struct ItemIndex {
    item_index: Option<HashMap<String, IndexMap<String, Entity>>>,
    item_order: HashMap<String, i32>,
    ordered_items: HashMap<String, Arc<Vec<Entity>>>,
}

/// `Model`.
pub struct Model {
    /// Java field `type`.
    pub type_: String,
    document_seen: bool,
    pub deleted: bool,
    pub versions: Shared<IndexMap<String, String>>,
    pub version_actors: Shared<IndexMap<String, String>>,
    pub conflicts: Shared<IndexMap<String, Map>>,
    pub conflict_active: Shared<IndexMap<String, bool>>,
    pub layers: Shared<IndexMap<String, Entity>>,
    pub strokes: Shared<IndexMap<String, Entity>>,
    pub objects: Shared<IndexMap<String, Entity>>,
    /// Java field `active` (stroke visibility).
    pub active: Shared<IndexMap<String, bool>>,
    pub object_active: Shared<IndexMap<String, bool>>,
    pub layer_active: Shared<IndexMap<String, bool>>,
    index: Mutex<ItemIndex>,
}

impl Default for Model {
    fn default() -> Self {
        Model::new()
    }
}

impl Clone for Model {
    /// `new Model(other)`: entity snapshots are shared; indexes start empty.
    fn clone(&self) -> Model {
        Model {
            type_: self.type_.clone(),
            document_seen: self.document_seen,
            deleted: self.deleted,
            versions: self.versions.clone(),
            version_actors: self.version_actors.clone(),
            conflicts: self.conflicts.clone(),
            conflict_active: self.conflict_active.clone(),
            layers: self.layers.clone(),
            strokes: self.strokes.clone(),
            objects: self.objects.clone(),
            active: self.active.clone(),
            object_active: self.object_active.clone(),
            layer_active: self.layer_active.clone(),
            index: Mutex::new(ItemIndex::default()),
        }
    }
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Model({})", json::write_map(&self.json()))
    }
}

fn item_scope(o: &Map) -> String {
    format!("{}:{}", json::java_string(o.get("layer")), json::as_int(o.get("page")).unwrap_or(0))
}

/// `getOrDefault(key, default)` where a present `null` stays `null`.
fn get_or_default<'a>(m: &'a Map, key: &str, default: &'a Value) -> &'a Value {
    match m.get(key) {
        Some(v) => v,
        None => default,
    }
}

impl Model {
    /// `new Model()`: one base layer.
    pub fn new() -> Model {
        let mut layers = IndexMap::new();
        layers.insert(
            "base".to_string(),
            Arc::new(object! {"kind" => "layer", "layer" => "base", "name" => "Layer 1", "force" => false}),
        );
        Model {
            type_: "fixed".to_string(),
            document_seen: false,
            deleted: false,
            versions: Shared::default(),
            version_actors: Shared::default(),
            conflicts: Shared::default(),
            conflict_active: Shared::default(),
            layers: Shared::new(layers),
            strokes: Shared::default(),
            objects: Shared::default(),
            active: Shared::default(),
            object_active: Shared::default(),
            layer_active: Shared::default(),
            index: Mutex::new(ItemIndex::default()),
        }
    }

    fn index_item(idx: &mut ItemIndex, o: &Entity) -> Result<()> {
        let kind = json::string(o, "kind")?;
        let key = format!("{}:{}", kind, json::string(o, kind)?);
        if !idx.item_order.contains_key(&key) {
            let n = idx.item_order.len() as i32;
            idx.item_order.insert(key.clone(), n);
        }
        idx.item_index.get_or_insert_with(HashMap::new).entry(item_scope(o)).or_default().insert(key, o.clone());
        Ok(())
    }

    /// `layerItems(layer, page)`: strokes first, then objects, each in first-seen order.
    pub fn layer_items(&self, layer: &LayerId, page: PageNumber) -> Arc<Vec<Entity>> {
        let (layer, page) = (&**layer, page.0);
        let mut idx = self.index.lock().unwrap_or_else(|e| e.into_inner());
        if idx.item_index.is_none() {
            idx.item_index = Some(HashMap::new());
            for o in self.strokes.values().chain(self.objects.values()) {
                let _ = Model::index_item(&mut idx, o);
            }
        }
        let scope = format!("{}:{}", layer, page);
        if let Some(cached) = idx.ordered_items.get(&scope) {
            return cached.clone();
        }
        let mut ordered: Vec<Entity> = match idx.item_index.as_ref().unwrap().get(&scope) {
            Some(indexed) => indexed.values().cloned().collect(),
            None => Vec::new(),
        };
        let order = &idx.item_order;
        let rank = |o: &Entity| -> (i32, i32) {
            let kind = json::java_string(o.get("kind"));
            let key = format!("{}:{}", kind, json::java_string(o.get(kind.as_str())));
            (if kind == "stroke" { 0 } else { 1 }, *order.get(&key).unwrap_or(&0))
        };
        ordered.sort_by_key(|o| rank(o));
        let cached = Arc::new(ordered);
        idx.ordered_items.insert(scope, cached.clone());
        cached
    }

    fn put_item(&mut self, o: Map, kind: &str) -> Result<()> {
        let id = json::string(&o, kind)?.to_string();
        let o = Arc::new(o);
        let table = if kind == "stroke" { &mut self.strokes } else { &mut self.objects };
        let old = table.insert(id.clone(), o.clone());
        let idx = self.index.get_mut().unwrap_or_else(|e| e.into_inner());
        if idx.item_index.is_some() {
            if let Some(old) = old {
                let old_scope = item_scope(&old);
                idx.ordered_items.remove(&old_scope);
                if old_scope != item_scope(&o) {
                    if let Some(scoped) = idx.item_index.as_mut().unwrap().get_mut(&old_scope) {
                        scoped.shift_remove(&format!("{}:{}", kind, id));
                    }
                }
            }
            idx.ordered_items.remove(&item_scope(&o));
            Model::index_item(idx, &o)?;
        }
        Ok(())
    }

    /// `Model.fromJson(value)`: rebuild from `json()` output.
    pub fn from_json(value: &Map) -> Result<Model> {
        let mut model = Model::new();
        model.type_ = json::string(value, "type")?.to_string();
        model.document_seen = true;
        model.layers.clear();
        for key in ["layers", "strokes", "objects"] {
            for op in json::list(value.get(key).unwrap_or(&Value::Null))? {
                model.apply(json::map(op)?)?;
            }
        }
        for (k, v) in json::map_at(value, "strokeActive")? {
            model.active.insert(k.clone(), json::as_bool(Some(v))?);
        }
        for (k, v) in json::map_at(value, "objectActive")? {
            model.object_active.insert(k.clone(), json::as_bool(Some(v))?);
        }
        for (k, v) in json::map_at(value, "layerActive")? {
            model.layer_active.insert(k.clone(), json::as_bool(Some(v))?);
        }
        let empty = Value::Object(Map::new());
        for (k, v) in json::map(get_or_default(value, "versions", &empty))? {
            model.versions.insert(k.clone(), json::as_string_opt(Some(v))?.ok_or_else(Error::null_pointer)?.to_string());
        }
        for (k, v) in json::map(get_or_default(value, "versionActors", &empty))? {
            model.version_actors.insert(k.clone(), json::as_string_opt(Some(v))?.ok_or_else(Error::null_pointer)?.to_string());
        }
        let no_conflicts = Value::Array(Vec::new());
        for c in json::list(get_or_default(value, "conflicts", &no_conflicts))? {
            let c = json::map(c)?;
            model.conflicts.insert(json::string(c, "id")?.to_string(), c.clone());
        }
        model.deleted = json::is_true(value.get("deleted"));
        Ok(model)
    }

    /// `new Model(Map state)`: replay `events` then `pending`.
    pub fn from_state(state: &Map) -> Result<Model> {
        let mut model = Model::new();
        for v in json::list(state.get("events").unwrap_or(&Value::Null))? {
            model.apply(json::map(v)?)?;
        }
        for v in json::list(state.get("pending").unwrap_or(&Value::Null))? {
            model.apply(json::map(v)?)?;
        }
        Ok(model)
    }

    /// `apply(op)`: apply one operation or batch, tracking versions and conflicts.
    pub fn apply(&mut self, op: &Map) -> Result<()> {
        if self.deleted {
            return Ok(());
        }
        let operation: Option<String> = json::as_string_opt(op.get("id"))?.map(str::to_string);
        let actions: Vec<&Map> = if op.get("kind").and_then(Value::as_str) == Some("batch") {
            json::list(op.get("actions").unwrap_or(&Value::Null))?.iter().map(json::map).collect::<Result<_>>()?
        } else {
            vec![op]
        };
        let empty = Value::Object(Map::new());
        let bases = json::map(get_or_default(op, "bases", &empty))?;
        let visibility = json::map(get_or_default(op, "visibility", &empty))?;
        let mut before: IndexMap<String, Option<Entity>> = IndexMap::new();
        let mut before_active: HashMap<String, bool> = HashMap::new();
        let mut old_versions: HashMap<String, String> = HashMap::new();
        for value in &actions {
            if let Some(key) = entity_key(value)? {
                if !before.contains_key(&key) {
                    before.insert(key.clone(), self.snapshot(&key));
                    before_active.insert(key.clone(), self.is_active(&key));
                    old_versions.insert(key.clone(), self.versions.get(&key).cloned().unwrap_or_default());
                }
            }
        }
        for value in &actions {
            self.apply_raw(value)?;
        }
        let Some(operation) = operation else { return Ok(()) };
        let empty_actor = Value::String(String::new());
        let actor_value = get_or_default(op, "actor", &empty_actor).clone();
        for (key, previous) in &before {
            let next = self.snapshot(key);
            let prior = old_versions.get(key).cloned().unwrap_or_default();
            let next_active = match visibility.get(key) {
                Some(v) => json::as_bool(Some(v))?,
                None => self.is_active(key),
            };
            let mut tracked_branch = false;
            if let (Some(next), true) = (&next, bases.contains_key(key)) {
                let keys: Vec<String> = self.conflicts.keys().cloned().collect();
                for entry_key in keys {
                    let conflict = &self.conflicts[&entry_key];
                    if conflict.get("key").and_then(Value::as_str) != Some(key.as_str())
                        || self.conflict_active.get(&entry_key) == Some(&false)
                    {
                        continue;
                    }
                    let base = bases.get(key);
                    let side = if json::objects_equal(base, conflict.get("localVersion")) {
                        Some("local")
                    } else if json::objects_equal(base, conflict.get("remoteVersion")) {
                        Some("remote")
                    } else {
                        None
                    };
                    if let Some(side) = side {
                        let mut updated = conflict.clone();
                        updated.insert(side.to_string(), Value::Object((**next).clone()));
                        updated.insert(format!("{}Active", side), Value::Bool(next_active));
                        updated.insert(format!("{}Version", side), Value::String(operation.clone()));
                        updated.insert(format!("{}Actor", side), actor_value.clone());
                        self.conflicts.insert(entry_key, updated);
                        tracked_branch = true;
                    }
                }
            }
            if !tracked_branch
                && bases.contains_key(key)
                && !json::objects_equal(bases.get(key), Some(&Value::String(prior.clone())))
            {
                if let (Some(previous), Some(next)) = (previous, &next) {
                    if previous != next || before_active[key] != self.is_active(key) {
                        let id = String::from(ids::Uuid::from_name(format!("{}:{}:{}", key, prior, operation)));
                        let colon = key.find(':').unwrap_or(0);
                        if !self.conflicts.contains_key(&id) {
                            let zero = Value::from(0);
                            let conflict = object! {
                                "id" => id.clone(),
                                "key" => key.clone(),
                                "kind" => &key[..colon],
                                "entity" => &key[colon + 1..],
                                "page" => get_or_default(next, "page", &zero).clone(),
                                "local" => Value::Object((**previous).clone()),
                                "remote" => Value::Object((**next).clone()),
                                "localActive" => before_active[key],
                                "remoteActive" => next_active,
                                "localVersion" => prior.clone(),
                                "remoteVersion" => operation.clone(),
                                "localActor" => self.version_actors.get(key).cloned().unwrap_or_default(),
                                "remoteActor" => actor_value.clone(),
                            };
                            self.conflicts.insert(id, conflict);
                        }
                    }
                }
            }
            self.versions.insert(key.clone(), operation.clone());
            let actor = json::as_string_opt(Some(&actor_value))?.unwrap_or("").to_string();
            self.version_actors.insert(key.clone(), actor);
        }
        Ok(())
    }

    fn snapshot(&self, key: &str) -> Option<Entity> {
        let id = &key[key.find(':').map(|i| i + 1).unwrap_or(0)..];
        if key.starts_with("object:") { self.objects.get(id).cloned() } else { self.strokes.get(id).cloned() }
    }

    fn is_active(&self, key: &str) -> bool {
        let id = &key[key.find(':').map(|i| i + 1).unwrap_or(0)..];
        let table = if key.starts_with("object:") { &self.object_active } else { &self.active };
        table.get(id) != Some(&false)
    }

    /// `Model.withBases(batch, model)`: record base versions and visibility.
    pub fn with_bases(batch: &mut Map, model: &Model) -> Result<()> {
        let mut bases = Map::new();
        let mut visibility = Map::new();
        let actions: Vec<&Map> = if batch.get("kind").and_then(Value::as_str) == Some("batch") {
            json::list(batch.get("actions").unwrap_or(&Value::Null))?.iter().map(json::map).collect::<Result<_>>()?
        } else {
            vec![&*batch]
        };
        for op in actions {
            if let Some(key) = entity_key(op)? {
                bases.insert(key.clone(), Value::String(model.versions.get(&key).cloned().unwrap_or_default()));
                if !visibility.contains_key(&key) {
                    visibility.insert(key.clone(), Value::Bool(model.is_active(&key)));
                }
                let kind = op.get("kind").and_then(Value::as_str);
                if kind == Some("active") || kind == Some("object-active") {
                    visibility.insert(key, op.get("active").cloned().unwrap_or(Value::Null));
                }
            }
        }
        batch.insert("bases".to_string(), Value::Object(bases));
        batch.insert("visibility".to_string(), Value::Object(visibility));
        Ok(())
    }

    /// `visibleConflicts()`.
    pub fn visible_conflicts(&self) -> Vec<Value> {
        self.conflicts
            .iter()
            .filter(|(k, _)| self.conflict_active.get(*k) != Some(&false))
            .map(|(_, v)| Value::Object(v.clone()))
            .collect()
    }

    fn apply_raw(&mut self, op: &Map) -> Result<()> {
        match json::string(op, "kind")? {
            "batch" => {
                for a in json::list(op.get("actions").unwrap_or(&Value::Null))? {
                    self.apply_raw(json::map(a)?)?;
                }
            }
            "conflict-status" => {
                let k = json::string(op, "conflict")?.to_string();
                self.conflict_active.insert(k, json::as_bool(op.get("active"))?);
            }
            "document-delete" => {
                self.deleted = true;
                self.strokes.clear();
                self.objects.clear();
                self.layers.clear();
                self.active.clear();
                self.object_active.clear();
                self.layer_active.clear();
                self.conflicts.clear();
                let idx = self.index.get_mut().unwrap_or_else(|e| e.into_inner());
                idx.item_index = None;
                idx.ordered_items.clear();
            }
            "document" => {
                if !self.document_seen {
                    self.type_ = json::string(op, "type")?.to_string();
                    self.document_seen = true;
                }
            }
            "layer" => {
                let k = json::string(op, "layer")?.to_string();
                self.layers.insert(k, Arc::new(op.clone()));
            }
            "stroke" => self.put_item(op.clone(), "stroke")?,
            "object" => self.put_item(op.clone(), "object")?,
            "active" => {
                let k = json::string(op, "stroke")?.to_string();
                self.active.insert(k, json::as_bool(op.get("active"))?);
            }
            "object-active" => {
                let k = json::string(op, "object")?.to_string();
                self.object_active.insert(k, json::as_bool(op.get("active"))?);
            }
            "layer-active" => {
                let k = json::string(op, "layer")?.to_string();
                self.layer_active.insert(k, json::as_bool(op.get("active"))?);
            }
            _ => {
                return Err(Error::illegal_argument(format!(
                    "Unknown operation kind: {}",
                    json::java_string(op.get("kind"))
                )))
            }
        }
        Ok(())
    }

    /// `json()`.
    pub fn json(&self) -> Map {
        let bools = |m: &IndexMap<String, bool>| -> Value {
            Value::Object(m.iter().map(|(k, v)| (k.clone(), Value::Bool(*v))).collect())
        };
        let strings = |m: &IndexMap<String, String>| -> Value {
            Value::Object(m.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect())
        };
        let entities = |m: &IndexMap<String, Entity>| -> Value {
            Value::Array(m.values().map(|e| Value::Object((**e).clone())).collect())
        };
        object! {
            "type" => self.type_.clone(),
            "deleted" => self.deleted,
            "layers" => entities(&self.layers),
            "strokes" => entities(&self.strokes),
            "objects" => entities(&self.objects),
            "strokeActive" => bools(&self.active),
            "objectActive" => bools(&self.object_active),
            "layerActive" => bools(&self.layer_active),
            "versions" => strings(&self.versions),
            "versionActors" => strings(&self.version_actors),
            "conflicts" => Value::Array(self.visible_conflicts()),
        }
    }

    /// `Model.validate(o)`: structural validation of one operation.
    pub fn validate(o: &Map) -> Result<()> {
        let kind = json::string(o, "kind")?;
        for key in ["actor", "session"] {
            if o.contains_key(key) {
                id(o, key)?;
            }
        }
        if o.contains_key("bases") {
            let bases = json::map_at(o, "bases")?;
            require(bases.len() <= 200, "Too many base versions")?;
            for (k, v) in bases {
                require(jlen(k) <= 220 && matches!(v, Value::String(s) if jlen(s) <= 200), "Invalid base version")?;
            }
        }
        if o.contains_key("visibility") {
            let visibility = json::map_at(o, "visibility")?;
            require(visibility.len() <= 200, "Too many visibility entries")?;
            for (k, v) in visibility {
                require(jlen(k) <= 220 && v.is_boolean(), "Invalid branch visibility")?;
            }
        }
        match kind {
            "batch" => {
                let actions = json::list_at(o, "actions")?;
                require(!actions.is_empty() && actions.len() <= 200, "Batch needs 1–200 actions")?;
                for a in actions {
                    let op = json::map(a)?;
                    let k = op.get("kind").and_then(Value::as_str);
                    require(k != Some("batch") && k != Some("document-delete"), "Nested batch or document deletion")?;
                    Model::validate(op)?;
                }
            }
            "document-delete" => {}
            "conflict-status" => {
                id(o, "conflict")?;
                bool_(o, "active")?;
            }
            "document" => {
                require(matches!(json::string(o, "type")?, "fixed" | "pages" | "infinite"), "Unknown document type")?;
            }
            "layer" => {
                id(o, "layer")?;
                id(o, "name")?;
                bool_(o, "force")?;
                if o.contains_key("order") {
                    range(o, "order", -1000000.0, 1000000.0)?;
                }
            }
            "active" => {
                id(o, "stroke")?;
                bool_(o, "active")?;
            }
            "object-active" => {
                id(o, "object")?;
                bool_(o, "active")?;
            }
            "layer-active" => {
                id(o, "layer")?;
                require(o.get("layer").and_then(Value::as_str) != Some("base"), "Cannot hide base layer globally")?;
                bool_(o, "active")?;
            }
            "stroke" => {
                id(o, "stroke")?;
                drawing(o)?;
                if o.contains_key("brush") {
                    require(
                        matches!(
                            json::string(o, "brush")?,
                            "pen" | "pencil" | "marker" | "brush" | "oily" | "fountain" | "charcoal" | "neoBrush"
                                | "latinCalligraphy" | "asiaCalligraphy" | "square"
                        ),
                        "Unknown brush",
                    )?;
                }
                if o.contains_key("material") {
                    range(o, "material", 0.0, 100.0)?;
                    require(json::number(o, "material")? == jmath::rint(json::number(o, "material")?), "Material must be an integer percentage")?;
                }
                let points = json::list_at(o, "points")?;
                require(!points.is_empty() && points.len() <= 10000, "Invalid points")?;
                for p in points {
                    let xyz = json::list(p)?;
                    require(xyz.len() == 3, "Point needs x, y, pressure")?;
                    for n in xyz {
                        require(
                            matches!(n, Value::Number(v) if { let d = json::number_double(v); d.is_finite() && d.abs() <= 1000000.0 }),
                            "Invalid point",
                        )?;
                    }
                    let pressure = json::as_double(Some(&xyz[2]))?;
                    require((0.0..=1.0).contains(&pressure), "Invalid pressure")?;
                }
            }
            "object" => {
                id(o, "object")?;
                drawing(o)?;
                let shape = json::string(o, "shape")?;
                require(shape_catalog::known(&ShapeId::from(shape)) || shape == "template", "Unknown shape")?;
                if shape == "svg" {
                    svg_asset::validate(json::string(o, "svg")?)?;
                }
                if shape == "template" {
                    require(matches!(json::string(o, "template")?, "ruled" | "grid" | "dots"), "Unknown infinite template")?;
                    range(o, "spacing", 8.0, 1000000.0)?;
                }
                if shape == "image" {
                    image_asset::validate(Some(json::string(o, "image")?))?;
                }
                if shape == "gif" {
                    gif_asset::validate(Some(json::string(o, "gif")?))?;
                }
                for key in ["bold", "italic", "underline", "flipX", "flipY"] {
                    if o.contains_key(key) {
                        bool_(o, key)?;
                    }
                }
                if o.contains_key("rotation") {
                    range(o, "rotation", -360.0, 360.0)?;
                }
                if o.contains_key("fontFamily") {
                    require(matches!(json::string(o, "fontFamily")?, "sans-serif" | "serif" | "monospace"), "Unknown font family")?;
                }
                if o.contains_key("alignment") {
                    require(matches!(json::string(o, "alignment")?, "left" | "center" | "right"), "Unknown alignment")?;
                }
                for key in ["x", "y", "w", "h"] {
                    range(o, key, -1000000.0, 1000000.0)?;
                }
                if !matches!(shape, "arrow" | "arrowLine" | "line") {
                    require(json::number(o, "w")? > 0.0 && json::number(o, "h")? > 0.0, "Positive object dimensions required")?;
                }
                if o.contains_key("text") {
                    require(jlen(json::string(o, "text")?) <= 10000, "Text too long")?;
                    range(o, "fontSize", 1.0, 500.0)?;
                }
                if o.contains_key("rotation") {
                    range(o, "rotation", -360.0, 360.0)?;
                }
                for key in ["flipX", "flipY"] {
                    if o.contains_key(key) {
                        bool_(o, key)?;
                    }
                }
                if o.contains_key("fill") {
                    color(o, "fill")?;
                }
                if o.contains_key("from") {
                    id(o, "from")?;
                }
                if o.contains_key("to") {
                    id(o, "to")?;
                }
            }
            _ => return Err(Error::illegal_argument(format!("Unknown operation kind: {}", kind))),
        }
        Ok(())
    }
}

/// `entityKey(op)`: `object:<id>` / `stroke:<id>` or `None`.
fn entity_key(op: &Map) -> Result<Option<String>> {
    let kind = json::string(op, "kind")?;
    let entity = match kind {
        "object" | "object-active" => "object",
        "stroke" | "active" => "stroke",
        _ => return Ok(None),
    };
    Ok(Some(format!("{}:{}", entity, json::string(op, entity)?)))
}

/// `Model.drawing(o)`: shared stroke/object fields.
pub fn drawing(o: &Map) -> Result<()> {
    id(o, "layer")?;
    range(o, "page", 0.0, 10000.0)?;
    require(json::number(o, "page")? == jmath::rint(json::number(o, "page")?), "Page must be an integer")?;
    range(o, "width", 1.0, 100.0)?;
    color(o, "color")?;
    if o.contains_key("erasures") {
        let masks = json::list_at(o, "erasures")?;
        require(masks.len() <= 2000, "Too many erasure regions")?;
        let mut count = 0usize;
        for mask in masks {
            count += crate::geometry::Polygon::from_json(mask)?.len();
            require(count <= 64000, "Erasure mask too complex")?;
        }
    }
    Ok(())
}

/// `Model.color(o, k)`: signed ARGB integer.
pub fn color(o: &Map, k: &str) -> Result<()> {
    range(o, k, i32::MIN as f64, i32::MAX as f64)?;
    require(json::number(o, k)? == jmath::rint(json::number(o, k)?), "Color must be signed ARGB integer")
}

/// `Model.range(o, k, lo, hi)`.
pub fn range(o: &Map, k: &str, lo: f64, hi: f64) -> Result<()> {
    let v = json::number(o, k)?;
    require_with(v >= lo && v <= hi, || format!("Out of range: {}", k))
}

/// `Model.id(o, k)`: 1–200 character string.
pub fn id(o: &Map, k: &str) -> Result<()> {
    let s = json::string(o, k)?;
    require_with(!s.is_empty() && jlen(s) <= 200, || format!("Invalid {}", k))
}

/// `Model.bool(o, k)`.
pub fn bool_(o: &Map, k: &str) -> Result<()> {
    require_with(matches!(o.get(k), Some(Value::Bool(_))), || format!("Expected boolean: {}", k))
}

/// `Model.require(condition, message)`.
pub use crate::error::require as require_condition;
