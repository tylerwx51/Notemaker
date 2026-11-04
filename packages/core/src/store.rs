//! Port of `Store.java`: per-document cached replica with durable journal
//! transactions and shared process locks.

use crate::actions::{self, Build};
use crate::collaboration;
use crate::endpoint::Endpoint;
use crate::error::{require, require_with, Error, Result};
use crate::clock::EpochMillis;
use crate::ids::{self, ActorId, ConflictChoice, ConflictId, DocumentName, SessionId};
use crate::java_port::file::canonical_path;
pub use crate::java_port::url::url_encode;
use crate::journal::{self, Edit, EditScope};
use crate::json::{self, jlen, Map, Value};
use crate::model::Model;
use crate::object;
use crate::timing::{mark, TimingPhase};
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::ThreadId;

/// Which Java class a state map was (decides how [`model`] materializes it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateKind {
    /// A plain `LinkedHashMap` (e.g. parsed JSON).
    Plain,
    /// `Store.Snapshot`: carries an immutable materialization.
    Snapshot,
    /// `Journal.State`: a journal read, carrying the effective model.
    Journal,
}

/// A replica state map (`Map<String,Object>` in Java), optionally carrying
/// the materialized model like `Store.Snapshot` / `Journal.State`.
/// Dereferences to the JSON map.
#[derive(Clone, Debug)]
pub struct State {
    /// Copy-on-write: Java's journal snapshots share their ops (`new
    /// ArrayList<>(list)` copies only references), so reads and edit
    /// snapshots stay cheap as a document grows. Clones share the map until
    /// one side mutates it (`DerefMut` → `Arc::make_mut`).
    pub map: Arc<Map>,
    model: Option<Model>,
    kind: StateKind,
}

impl State {
    /// A state sharing `map` until either side mutates it.
    pub(crate) fn shared(map: Arc<Map>, model: Option<Model>, kind: StateKind) -> State {
        State { map, model, kind }
    }
    /// A plain JSON state map.
    pub fn plain(map: Map) -> State {
        State { map: Arc::new(map), model: None, kind: StateKind::Plain }
    }
    /// `new Store.Snapshot(model)`: empty map retaining a copy of `model`.
    pub fn snapshot(model: &Model) -> State {
        State { map: Arc::new(Map::new()), model: Some(model.clone()), kind: StateKind::Snapshot }
    }
    pub fn kind(&self) -> StateKind {
        self.kind
    }
    /// The JSON map without the in-process materialization.
    pub fn into_map(self) -> Map {
        Arc::try_unwrap(self.map).unwrap_or_else(|shared| (*shared).clone())
    }
}

impl Deref for State {
    type Target = Map;
    fn deref(&self) -> &Map {
        &self.map
    }
}

impl DerefMut for State {
    fn deref_mut(&mut self) -> &mut Map {
        Arc::make_mut(&mut self.map)
    }
}

/// Which history stack [`Store::history`] takes the last action from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryDirection {
    Undo,
    Redo,
}

/// Whether [`Store::sync`] may upload pending operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMode {
    Upload,
    DownloadOnly,
}

/// A position in a replica's committed or provisional operation list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReplicaCursor(pub i32);

/// `Store.model(state)`.
pub fn model(state: &State) -> Result<Model> {
    if state.kind == StateKind::Snapshot {
        if let Some(m) = &state.model {
            return Ok(m.clone());
        }
    }
    if state.contains_key("effectiveModel") {
        return Model::from_json(json::map_at(state, "effectiveModel")?);
    }
    if state.kind == StateKind::Journal {
        if let Some(m) = &state.model {
            return Ok(m.clone());
        }
    }
    Model::from_state(state)
}

/// `Store.model(map)` for a plain JSON map.
pub fn model_of_map(state: &Map) -> Result<Model> {
    if state.contains_key("effectiveModel") {
        return Model::from_json(json::map_at(state, "effectiveModel")?);
    }
    Model::from_state(state)
}

/// A reentrant lock (Java `synchronized`).
#[derive(Default)]
struct ReentrantLock {
    owner: Mutex<(Option<ThreadId>, usize)>,
    released: Condvar,
}

struct ReentrantGuard<'a>(&'a ReentrantLock);

impl ReentrantLock {
    fn lock(&self) -> ReentrantGuard<'_> {
        let me = std::thread::current().id();
        let mut owner = self.owner.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match owner.0 {
                None => {
                    *owner = (Some(me), 1);
                    break;
                }
                Some(t) if t == me => {
                    owner.1 += 1;
                    break;
                }
                Some(_) => owner = self.released.wait(owner).unwrap_or_else(|e| e.into_inner()),
            }
        }
        ReentrantGuard(self)
    }
}

impl Drop for ReentrantGuard<'_> {
    fn drop(&mut self) {
        let mut owner = self.0.owner.lock().unwrap_or_else(|e| e.into_inner());
        owner.1 -= 1;
        if owner.1 == 0 {
            owner.0 = None;
            self.0.released.notify_one();
        }
    }
}

/// Process-local selection leases and active sessions (never journaled).
#[derive(Default)]
struct Coordination {
    lock: ReentrantLock,
    data: Mutex<(Map, Map)>,
}

impl Coordination {
    fn leases(&self) -> Map {
        self.data.lock().unwrap_or_else(|e| e.into_inner()).0.clone()
    }
    fn sessions(&self) -> Map {
        self.data.lock().unwrap_or_else(|e| e.into_inner()).1.clone()
    }
    fn set(&self, leases: Map, sessions: Map) {
        *self.data.lock().unwrap_or_else(|e| e.into_inner()) = (leases, sessions);
    }
}

fn coordinations() -> &'static Mutex<HashMap<String, Arc<Coordination>>> {
    static C: OnceLock<Mutex<HashMap<String, Arc<Coordination>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `Store.UploadGate`: called once before the first upload request.
pub type UploadGate<'a> = dyn FnMut() -> Result<()> + 'a;

/// `Store`.
pub struct Store {
    pub file: PathBuf,
    pub document: DocumentName,
    writable: bool,
    coordination: Arc<Coordination>,
}

fn bytes_len(value: &Value) -> usize {
    json::write(value).len()
}

impl Store {
    /// `new Store(directory, document)`: read-only access.
    pub fn new(directory: &Path, document: &DocumentName) -> Result<Store> {
        Store::open(directory, document.as_ref(), false)
    }

    /// `Store.writer(directory, document)`: the selected proxy's writable store.
    pub fn writer(directory: &Path, document: &DocumentName) -> Result<Store> {
        Store::open(directory, document.as_ref(), true)
    }

    fn open(directory: &Path, document: &str, writable: bool) -> Result<Store> {
        require(!document.is_empty() && jlen(document) <= 200, "Document name needs 1–200 characters")?;
        if writable {
            std::fs::create_dir_all(directory).map_err(|e| Error::io(format!("{} ({})", directory.display(), crate::error::io_message(&e))))?;
        }
        let file = directory.join(format!("document-{}.json", String::from(ids::Uuid::from_name(document))));
        let key = canonical_path(&file);
        let coordination = coordinations().lock().unwrap_or_else(|e| e.into_inner()).entry(key).or_default().clone();
        Ok(Store { file, document: DocumentName::from(document), writable, coordination })
    }

    fn journal_mode(&self) -> journal::JournalMode {
        if self.writable { journal::JournalMode::Writable } else { journal::JournalMode::ReadOnly }
    }

    fn locked(&self, edit: Option<&mut Edit<'_>>) -> Result<State> {
        require(edit.is_none() || self.writable, "Writes require the selected proxy")?;
        journal::access(&self.file, &self.document, edit, EditScope::Full, self.journal_mode(), None)
    }

    /// `model()`: the effective model of the current state.
    pub fn model(&self) -> Result<Model> {
        model(&self.read()?)
    }

    /// Java `overlay(state)` on a state that is already a private copy.
    fn overlay(&self, mut state: State) -> State {
        state.shift_remove("leases");
        state.shift_remove("activeSessions");
        state.insert("leases".into(), Value::Object(self.coordination.leases()));
        state.insert("activeSessions".into(), Value::Object(self.coordination.sessions()));
        state
    }

    /// `overlay(locked(edit))`: the journal applies the overlay to its cached
    /// state in place (see `journal::access`), so this shares that map.
    fn locked_overlaid(&self, edit: Option<&mut Edit<'_>>, scope: EditScope, values: &journal::Overlay<'_>) -> Result<State> {
        require(edit.is_none() || self.writable, "Writes require the selected proxy")?;
        journal::access(&self.file, &self.document, edit, scope, self.journal_mode(), Some(values))
    }

    /// `read()`.
    pub fn read(&self) -> Result<State> {
        let _sync = self.coordination.lock.lock();
        let current = || (self.coordination.leases(), self.coordination.sessions());
        self.locked_overlaid(None, EditScope::Full, &current)
    }

    /// `checkpoint()`: save the materialized replica as a durable base.
    pub fn checkpoint(&self) -> Result<Map> {
        let _sync = self.coordination.lock.lock();
        require(self.writable, "Checkpoint requires the selected proxy")?;
        journal::checkpoint(&self.file, &self.document)
    }

    /// `edit(edit)`: one durable journal transaction.
    pub fn edit(&self, edit: &mut Edit<'_>) -> Result<State> {
        self.edit_scoped(edit, EditScope::Full)
    }

    /// `edit(edit)` for Store's own action edits, which fit
    /// [`EditScope::Append`]: they read only the carried model, coordination
    /// metadata and the acting actor's history, push new pending operations
    /// and pop/push history tails.
    fn edit_appending(&self, edit: &mut Edit<'_>) -> Result<State> {
        self.edit_scoped(edit, EditScope::Append)
    }

    fn edit_scoped(&self, edit: &mut Edit<'_>, scope: EditScope) -> Result<State> {
        let _sync = self.coordination.lock.lock();
        // The coordination data this edit produces. The returned state is
        // overlaid with it; it is published only after the commit succeeded.
        let produced: std::cell::RefCell<(Map, Map)> = std::cell::RefCell::new((Map::new(), Map::new()));
        let current_leases = self.coordination.leases();
        let current_sessions = self.coordination.sessions();
        let mut wrapped = |s: &mut State| -> Result<()> {
            s.shift_remove("leases");
            s.shift_remove("activeSessions");
            s.insert("leases".into(), Value::Object(current_leases.clone()));
            s.insert("activeSessions".into(), Value::Object(current_sessions.clone()));
            edit(s)?;
            let leases = json::map(s.get("leases").unwrap_or(&Value::Null))?.clone();
            let sessions = json::map(s.get("activeSessions").unwrap_or(&Value::Null))?.clone();
            *produced.borrow_mut() = (leases, sessions);
            s.shift_remove("leases");
            s.shift_remove("activeSessions");
            Ok(())
        };
        let values = || produced.borrow().clone();
        let state = self.locked_overlaid(Some(&mut wrapped), scope, &values)?;
        let (leases, sessions) = produced.into_inner();
        self.coordination.set(leases, sessions);
        Ok(state)
    }

    /// `editTransient(edit)`: update coordination metadata without a journal revision.
    pub fn edit_transient(&self, edit: &mut Edit<'_>) -> Result<State> {
        let _sync = self.coordination.lock.lock();
        require(self.writable, "Writes require the selected proxy")?;
        let mut state = self.overlay(self.locked(None)?);
        edit(&mut state)?;
        let leases = json::map(state.get("leases").unwrap_or(&Value::Null))?.clone();
        let sessions = json::map(state.get("activeSessions").unwrap_or(&Value::Null))?.clone();
        self.coordination.set(leases, sessions);
        Ok(self.overlay(state))
    }

    /// `append(operation, actor)`.
    pub fn append(&self, operation: &Map, actor: &ActorId) -> Result<State> {
        self.append_with_settings(operation, actor, None)
    }

    /// `Store.validateSettings(preferences)`.
    pub fn validate_settings(preferences: &Map) -> Result<()> {
        for key in preferences.keys() {
            require(matches!(key.as_str(), "hidden" | "undo" | "redo" | "selected" | "page" | "clipboard"), "Unknown local preference")?;
            if matches!(key.as_str(), "hidden" | "undo" | "redo") {
                for value in json::list(&preferences[key])? {
                    require(value.is_string(), "Expected preference strings")?;
                }
            }
            if key == "selected" {
                json::string(preferences, key)?;
            }
            if key == "page" {
                let n = json::number(preferences, key)?;
                require((0.0..=10000.0).contains(&n) && n == crate::jmath::rint(n), "Invalid page")?;
            }
            if key == "clipboard" {
                let clipboard = json::map(&preferences[key])?;
                let entities = json::list_at(clipboard, "entities")?;
                require(entities.len() <= 100, "Clipboard exceeds 100 entities")?;
                for entity in entities {
                    Model::validate(json::map(entity)?)?;
                }
            }
        }
        Ok(())
    }

    /// `appendWithSettings(operation, actor, preferences)`.
    pub fn append_with_settings(&self, operation: &Map, actor: &ActorId, preferences: Option<&Map>) -> Result<State> {
        let actor: &str = actor;
        require(!actor.is_empty() && jlen(actor) <= 200, "Invalid actor")?;
        Model::validate(operation)?;
        if let Some(p) = preferences {
            Store::validate_settings(p)?;
        }
        require(json::write_map(operation).len() <= 3000000, "Action exceeds 3 MB")?;
        self.edit_appending(&mut |state: &mut State| {
            // Lease checks leave the document model unchanged; reuse this snapshot for version bases.
            let current = model(state)?;
            require(!current.deleted, "Document is deleted")?;
            collaboration::require_unlocked(state, &ActorId::from(actor), &[Value::Object(operation.clone())], EpochMillis::now())?;
            let mut op = operation.clone();
            op.insert("id".into(), Value::String(String::from(ids::Uuid::random())));
            op.insert("actor".into(), Value::String(actor.to_string()));
            Model::with_bases(&mut op, &current)?;
            let session = active_session(state, actor)?;
            if let Some(session) = session {
                op.insert("session".into(), session);
            }
            json::list_at_mut(state, "pending")?.push(Value::Object(op));
            if let Some(p) = preferences {
                for (k, v) in p {
                    state.insert(k.clone(), v.clone());
                }
            }
            Ok(())
        })
    }

    /// `deleteDocument(actor)`: permanent, idempotent lifecycle action.
    pub fn delete_document(&self, actor: &ActorId) -> Result<Map> {
        let actor: &str = actor;
        require(!actor.is_empty() && jlen(actor) <= 200, "Invalid actor")?;
        let mut result = object! {"document" => self.document.clone(), "deleted" => true};
        self.edit_appending(&mut |state: &mut State| {
            if model(state)?.deleted {
                return Ok(());
            }
            let id = String::from(ids::Uuid::random());
            json::list_at_mut(state, "pending")?.push(Value::Object(object! {"kind" => "document-delete", "id" => id.clone(), "actor" => actor}));
            state.insert("leases".into(), Value::Object(Map::new()));
            state.insert("activeSessions".into(), Value::Object(Map::new()));
            result.insert("operation".into(), Value::String(id));
            Ok(())
        })?;
        Ok(result)
    }

    /// `commit(actor, build)`.
    pub fn commit(&self, actor: &ActorId, build: &mut Build<'_>) -> Result<Map> {
        self.commit_full(actor.as_ref(), build, &[], None, &[]).map(|(r, _)| r)
    }

    /// `commitWithState(actor, build)`: the result plus the durable post-edit
    /// state (Java put it under `result["state"]`).
    pub fn commit_with_state(&self, actor: &ActorId, build: &mut Build<'_>) -> Result<(Map, State)> {
        self.commit_full(actor.as_ref(), build, &[], None, &[])
    }

    /// `commit(actor, build, dependencies)`.
    pub fn commit_with_dependencies(&self, actor: &ActorId, build: &mut Build<'_>, dependencies: &[Value]) -> Result<Map> {
        self.commit_full(actor.as_ref(), build, dependencies, None, &[]).map(|(r, _)| r)
    }

    /// `commitSelected(actor, session, entities, build, dependencies)`.
    pub fn commit_selected(&self, actor: &ActorId, session: &SessionId, entities: &[Value], build: &mut Build<'_>, dependencies: &[Value]) -> Result<Map> {
        self.commit_full(actor.as_ref(), build, dependencies, Some(session.as_ref()), entities).map(|(r, _)| r)
    }

    fn commit_full(&self, actor: &str, build: &mut Build<'_>, dependencies: &[Value], required_session: Option<&str>, selected: &[Value]) -> Result<(Map, State)> {
        require(!actor.is_empty() && jlen(actor) <= 200, "Invalid actor")?;
        let mut result = Map::new();
        let after = self.edit_appending(&mut |state: &mut State| {
            let mut model = model(state)?;
            require(!model.deleted, "Document is deleted")?;
            if let Some(session) = required_session {
                collaboration::require_selection(state, &model, &ActorId::from(actor), &SessionId::from(session), selected)?;
            }
            let baseline = model.clone();
            mark(TimingPhase("geometry_begin"));
            let requested = build(&mut model)?;
            let mut ops: Vec<Value> = Vec::new();
            mark(TimingPhase("geometry_done"));
            for value in requested {
                let op = json::map(&value)?;
                let kind = json::string(op, "kind")?;
                let key = match kind {
                    "object" => Some("object"),
                    "layer" => Some("layer"),
                    "stroke" => Some("stroke"),
                    _ => None,
                };
                let extra = match key {
                    Some(key) => {
                        let table = match key {
                            "object" => &model.objects,
                            "layer" => &model.layers,
                            _ => &model.strokes,
                        };
                        if json::key_of(op.get(key)).is_some_and(|id| table.contains_key(id)) {
                            None
                        } else {
                            let k = if key == "stroke" { "active".to_string() } else { format!("{}-active", key) };
                            Some(Value::Object(object! {"kind" => k, key => op.get(key).cloned().unwrap_or(Value::Null), "active" => true}))
                        }
                    }
                    None => None,
                };
                ops.push(value);
                if let Some(extra) = extra {
                    ops.push(extra);
                }
            }
            require(!ops.is_empty() && ops.len() <= 200, "Action needs 1–200 operations")?;
            let mut inverse: Vec<Value> = Vec::new();
            for v in &ops {
                let op = json::map(v)?;
                let kind = op.get("kind").and_then(Value::as_str);
                require(!matches!(kind, Some("batch") | Some("document") | Some("document-delete")), "Use document create; nested batches are not allowed")?;
                Model::validate(op)?;
                inverse.insert(0, actions::inverse(&model, op)?);
                model.apply(op)?;
            }
            mark(TimingPhase("inverse_done"));
            collaboration::require_unlocked(state, &ActorId::from(actor), &ops, EpochMillis::now())?;
            collaboration::require_unlocked(state, &ActorId::from(actor), dependencies, EpochMillis::now())?;
            actions::validate_references(&model, &ops)?;
            mark(TimingPhase("locks_done"));
            let mut batch = object! {"kind" => "batch", "id" => String::from(ids::Uuid::random()), "actor" => actor, "actions" => Value::Array(ops.clone())};
            Model::with_bases(&mut batch, &baseline)?;
            if let Some(session) = active_session(state, actor)? {
                batch.insert("session".into(), session);
            }
            require(json::write_map(&batch).len() <= 3000000, "Action exceeds 3 MB")?;
            require(json::write_list(&inverse).len() <= 3000000, "Undo snapshot exceeds 3 MB; split this action")?;
            mark(TimingPhase("limits_done"));
            let batch_id = batch.get("id").cloned().unwrap_or(Value::Null);
            json::list_at_mut(state, "pending")?.push(Value::Object(batch));
            let history = history_of(state, actor)?;
            json::list_at_mut(history, "undo")?.push(Value::Object(object! {"forward" => Value::Array(ops.clone()), "inverse" => Value::Array(inverse)}));
            history.insert("redo".into(), Value::Array(Vec::new()));
            result.insert("operation".into(), batch_id);
            result.insert("actions".into(), Value::Array(ops));
            Ok(())
        })?;
        Ok((result, after))
    }

    /// `history(actor, redo)`: undo or redo the actor's last action.
    pub fn history(&self, actor: &ActorId, direction: HistoryDirection) -> Result<Map> {
        self.history_with_state(actor, direction).map(|(r, _)| r)
    }

    /// `history(actor, redo, true)`: the result plus the post-edit state.
    pub fn history_with_state(&self, actor: &ActorId, direction: HistoryDirection) -> Result<(Map, State)> {
        let (actor, redo) = (actor.as_ref(), direction == HistoryDirection::Redo);
        let mut result = Map::new();
        let (from_key, to_key, part) = if redo { ("redo", "undo", "forward") } else { ("undo", "redo", "inverse") };
        let after = self.edit_appending(&mut |state: &mut State| {
            let current = model(state)?;
            require(!current.deleted, "Document is deleted")?;
            let entry = {
                let h = history_of(state, actor)?;
                let from = json::list_at(h, from_key)?;
                json::list_at(h, to_key)?;
                require_with(!from.is_empty(), || format!("No {} for actor {}", if redo { "redo" } else { "undo" }, actor))?;
                from[from.len() - 1].clone()
            };
            let entry_map = json::map(&entry)?;
            let actions = json::list(entry_map.get(part).unwrap_or(&Value::Null))?.clone();
            collaboration::require_unlocked(state, &ActorId::from(actor), &actions, EpochMillis::now())?;
            {
                let h = history_of(state, actor)?;
                json::list_at_mut(h, from_key)?.pop();
                json::list_at_mut(h, to_key)?.push(entry.clone());
            }
            let id = String::from(ids::Uuid::random());
            let mut batch = object! {"kind" => "batch", "id" => id.clone(), "actor" => actor, "actions" => entry_map.get(part).cloned().unwrap_or(Value::Null)};
            Model::with_bases(&mut batch, &current)?;
            if let Some(session) = active_session(state, actor)? {
                batch.insert("session".into(), session);
            }
            json::list_at_mut(state, "pending")?.push(Value::Object(batch));
            result.insert("operation".into(), Value::String(id));
            Ok(())
        })?;
        Ok((result, after))
    }

    /// `accept(operations, root)`: immutable ingress log for downstream replicas.
    pub fn accept(&self, operations: &[Value], root: Option<&Value>) -> Result<Map> {
        let root = root.filter(|r| !r.is_null());
        let before = self.read()?;
        let mut known = index(&before)?;
        let mut fresh: Vec<Value> = Vec::new();
        require(root.is_none_or(Value::is_string), "Invalid root identity")?;
        require(
            root.is_none() || !before.contains_key("serverId") || Some(root.unwrap()) == before.get("serverId"),
            "Server database differs from this replica",
        )?;
        for value in operations {
            let op = json::map(value)?;
            Model::validate(op)?;
            let id = json::string(op, "id")?;
            require(!id.is_empty() && jlen(id) <= 200, "Invalid operation ID")?;
            require(bytes_len(value) <= 3000000, "Action exceeds 3 MB")?;
            if let Some(existing) = known.get(id) {
                require(json::equivalent(existing, value), "Operation id reused with different contents")?;
            } else {
                known.insert(id.to_string(), value.clone());
                fresh.push(value.clone());
            }
        }
        if !fresh.is_empty() || (root.is_some() && !before.contains_key("serverId")) {
            self.edit(&mut |state: &mut State| {
                require(
                    root.is_none() || !state.contains_key("serverId") || Some(root.unwrap()) == state.get("serverId"),
                    "Server database differs from this replica",
                )?;
                for op in &fresh {
                    collaboration::require_incoming_unlocked(state, json::map(op)?, EpochMillis::now())?;
                }
                if let Some(root) = root {
                    state.insert("serverId".into(), root.clone());
                }
                let mut current = index(state)?;
                for op in &fresh {
                    let id = json::string(json::map(op)?, "id")?.to_string();
                    if let Some(existing) = current.get(&id) {
                        require(json::equivalent(existing, op), "Operation id reused with different contents")?;
                    } else {
                        json::list_at_mut(state, "pending")?.push(op.clone());
                        current.insert(id, op.clone());
                    }
                }
                Ok(())
            })?;
        }
        let accepted: Vec<Value> = operations.iter().map(|op| op.as_object().and_then(|m| m.get("id")).cloned().unwrap_or(Value::Null)).collect();
        Ok(object! {"accepted" => Value::Array(accepted)})
    }

    /// `Store.page(values, start)`: at most 100 values / ~3 MB.
    pub fn page(values: &[Value], start: ReplicaCursor) -> Result<Vec<Value>> {
        let start = start.0;
        require(start >= 0 && start as usize <= values.len(), "Invalid replica cursor")?;
        let mut out = Vec::new();
        let mut bytes: i32 = 0;
        let mut i = start as usize;
        while i < values.len() && out.len() < 100 {
            let value = &values[i];
            let size = bytes_len(value) as i32;
            if !out.is_empty() && bytes.wrapping_add(size) > 3000000 {
                break;
            }
            out.push(value.clone());
            bytes = bytes.wrapping_add(size);
            i += 1;
        }
        Ok(out)
    }

    /// `replica(after, pendingAfter)`: committed and provisional pages.
    pub fn replica(&self, after: ReplicaCursor, pending_after: ReplicaCursor) -> Result<Map> {
        let (after, pending_after) = (after.0, pending_after.0);
        let state = self.read()?;
        let events = json::list_at(&state, "events")?;
        let empty = Value::Array(Vec::new());
        let ingress = json::list(json::get_or_default(&state, "relay", &empty))?;
        let committed = Store::page(events, ReplicaCursor(after))?;
        let provisional = Store::page(ingress, ReplicaCursor(pending_after))?;
        let after_next = after + committed.len() as i32;
        let pending_next = pending_after + provisional.len() as i32;
        Ok(object! {
            "events" => Value::Array(committed),
            "after" => after_next,
            "more" => (after_next as i64) < events.len() as i64,
            "provisional" => Value::Array(provisional),
            "pendingAfter" => pending_next,
            "pendingMore" => (pending_next as i64) < ingress.len() as i64,
            "rootId" => state.get("serverId").cloned().unwrap_or(Value::Null),
        })
    }

    /// `needsUpload(peer)`.
    pub fn needs_upload(&self, peer: &str) -> Result<bool> {
        let s = self.read()?;
        let mut sent: Vec<Value> = Vec::new();
        if s.get("peerId").and_then(Value::as_str) == Some(peer) {
            let empty = Value::Array(Vec::new());
            sent.extend(json::list(json::get_or_default(&s, "forwarded", &empty))?.iter().cloned());
        }
        for op in json::list_at(&s, "pending")? {
            let id = json::map(op)?.get("id").cloned().unwrap_or(Value::Null);
            if !sent.contains(&id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `sync(upstream, info, upload, gate)`: called only by the proxy's single sync worker.
    pub fn sync(&self, upstream: &Endpoint, info: &Map, mode: SyncMode, gate: &mut UploadGate<'_>) -> Result<Map> {
        let upload = mode == SyncMode::Upload;
        let peer = json::string(info, "id")?.to_string();
        let root = info.get("rootId").filter(|r| !r.is_null()).cloned();
        let initial = self.read()?;
        if let Some(root) = &root {
            if initial.contains_key("serverId") {
                require(Some(root) == initial.get("serverId"), "Server database differs from this replica; use a separate state directory")?;
            }
        }
        if initial.get("peerId").and_then(Value::as_str) != Some(peer.as_str()) {
            self.edit(&mut |s: &mut State| {
                s.insert("peerId".into(), Value::String(peer.clone()));
                s.insert("forwarded".into(), Value::Array(Vec::new()));
                s.insert("relayCursor".into(), Value::from(0));
                Ok(())
            })?;
        }
        let initial = self.read()?;
        let empty = Value::Array(Vec::new());
        let sent: Vec<Value> = json::list(json::get_or_default(&initial, "forwarded", &empty))?.clone();
        let mut batch: Vec<Value> = Vec::new();
        let mut bytes: i32 = 0;
        if upload {
            for op in json::list_at(&initial, "pending")? {
                let id = json::map(op)?.get("id").cloned().unwrap_or(Value::Null);
                if sent.contains(&id) {
                    continue;
                }
                let size = bytes_len(op) as i32;
                if batch.len() >= 100 || (!batch.is_empty() && bytes.wrapping_add(size) > 3000000) {
                    break;
                }
                batch.push(op.clone());
                bytes = bytes.wrapping_add(size);
            }
        }
        if !batch.is_empty() {
            gate()?;
            let root_id = root.clone().unwrap_or_else(|| initial.get("serverId").cloned().unwrap_or(Value::Null));
            let body = Value::Object(object! {"document" => self.document.clone(), "operations" => Value::Array(batch.clone()), "rootId" => root_id});
            let response = upstream.request("/api/events", Some(&body))?;
            let ack = json::list(response.get("accepted").unwrap_or(&Value::Null))?.clone();
            let submitted: Vec<Value> = batch.iter().map(|op| op.as_object().and_then(|m| m.get("id")).cloned().unwrap_or(Value::Null)).collect();
            require(ack.iter().all(|a| submitted.contains(a)), "Unexpected acknowledgement")?;
            self.edit(&mut |s: &mut State| {
                let empty = Value::Array(Vec::new());
                let mut forwarded = json::list(json::get_or_default(s, "forwarded", &empty))?.clone();
                for id in &ack {
                    if !forwarded.contains(id) {
                        forwarded.push(id.clone());
                    }
                }
                s.insert("forwarded".into(), Value::Array(forwarded));
                Ok(())
            })?;
        }
        let mut downloaded: i64 = 0;
        loop {
            let s = self.read()?;
            let after = json::list_at(&s, "events")?.len() as i32;
            let zero = Value::from(0);
            let pending_after = json::as_int(Some(json::get_or_default(&s, "relayCursor", &zero)))?;
            let response = upstream.request(
                &format!("/api/replica?document={}&after={}&pendingAfter={}", url_encode(&self.document), after, pending_after),
                None,
            )?;
            let identity = response.get("rootId").filter(|v| !v.is_null()).cloned();
            if let Some(identity) = &identity {
                if s.contains_key("serverId") {
                    require(Some(identity) == s.get("serverId"), "Server database differs from this replica")?;
                }
            }
            let events = json::list(response.get("events").unwrap_or(&Value::Null))?.clone();
            let provisional = json::list(response.get("provisional").unwrap_or(&Value::Null))?.clone();
            require(json::as_int(response.get("after"))? == after + events.len() as i32, "Invalid commit cursor")?;
            for op in events.iter().chain(provisional.iter()) {
                Model::validate(json::map(op)?)?;
            }
            let next = json::as_int(response.get("pendingAfter"))?;
            require(next == pending_after + provisional.len() as i32, "Invalid relay cursor")?;
            let identity_changed = identity.as_ref().is_some_and(|i| Some(i) != s.get("serverId"));
            if !events.is_empty() || !provisional.is_empty() || identity_changed {
                self.edit(&mut |state: &mut State| {
                    let mut known = index(state)?;
                    let mut committed: HashSet<String> = HashSet::new();
                    for op in json::list_at(state, "events")? {
                        committed.insert(json::java_string(json::map(op)?.get("id")));
                    }
                    for op in &events {
                        let id = json::string(json::map(op)?, "id")?.to_string();
                        require(!committed.contains(&id), "Duplicate committed operation")?;
                        if let Some(existing) = known.get(&id) {
                            require(json::equivalent(existing, op), "Operation id reused with different contents")?;
                        }
                        json::list_at_mut(state, "events")?.push(op.clone());
                        known.insert(id.clone(), op.clone());
                        committed.insert(id);
                    }
                    let empty = Value::Array(Vec::new());
                    let mut forwarded = json::list(json::get_or_default(state, "forwarded", &empty))?.clone();
                    for op in &provisional {
                        let id = json::string(json::map(op)?, "id")?.to_string();
                        if let Some(existing) = known.get(&id) {
                            require(json::equivalent(existing, op), "Operation id reused with different contents")?;
                        } else {
                            json::list_at_mut(state, "pending")?.push(op.clone());
                            known.insert(id.clone(), op.clone());
                        }
                        let id_value = Value::String(id.clone());
                        if !committed.contains(&id) && !forwarded.contains(&id_value) {
                            forwarded.push(id_value);
                        }
                    }
                    json::list_at_mut(state, "pending")?.retain(|op| {
                        !matches!(op.as_object().and_then(|m| m.get("id")), Some(Value::String(id)) if committed.contains(id))
                    });
                    forwarded.retain(|v| !matches!(v, Value::String(id) if committed.contains(id)));
                    state.insert("forwarded".into(), Value::Array(forwarded));
                    state.insert("relayCursor".into(), Value::from(next));
                    state.insert("cursor".into(), Value::from(after + events.len() as i32));
                    if let Some(identity) = &identity {
                        state.insert("serverId".into(), identity.clone());
                    }
                    Ok(())
                })?;
            }
            downloaded += events.len() as i64;
            if !json::is_true(response.get("more")) && !json::is_true(response.get("pendingMore")) {
                break;
            }
        }
        let local_locks = self.read()?;
        let empty_map = Value::Object(Map::new());
        let local_leases = json::get_or_default(&local_locks, "leases", &empty_map).clone();
        if !json::map(&local_leases)?.is_empty() {
            let body = Value::Object(object! {"document" => self.document.clone(), "leases" => local_leases, "remove" => Value::Array(Vec::new())});
            upstream.request("/api/locks", Some(&body))?;
        }
        let locks = upstream.request(&format!("/api/locks?document={}", url_encode(&self.document)), None)?;
        if locks.contains_key("leases") {
            let leases = json::map(&locks["leases"])?.clone();
            self.edit_transient(&mut |state: &mut State| {
                collaboration::expire(state, EpochMillis::now())?;
                state.insert("leases".into(), Value::Object(leases.clone()));
                Ok(())
            })?;
        }
        let pending = json::list_at(&*self.read()?, "pending")?.len() as i64;
        let cursor = self.read()?.get("cursor").cloned().unwrap_or(Value::Null);
        Ok(object! {
            "uploaded" => batch.len() as i64,
            "downloaded" => downloaded,
            "pending" => pending,
            "cursor" => cursor,
            "previews" => json::get_or_default(&locks, "previews", &empty_map).clone(),
        })
    }

    /// `conflicts()`.
    pub fn conflicts(&self) -> Result<Vec<Value>> {
        Ok(self.model()?.visible_conflicts())
    }

    /// `conflict(id, choice, actor)`: resolve with `local`, `remote` or `both`.
    /// Parse the choice with [`ConflictChoice::parse`] first, as Java
    /// validated it before anything else.
    pub fn conflict(&self, id: &ConflictId, choice: ConflictChoice, actor: &ActorId) -> Result<Map> {
        let id: &str = id;
        self.commit(actor, &mut |m: &mut Model| {
            let c = m.conflicts.get(id);
            require(c.is_some() && m.conflict_active.get(id) != Some(&false), "Unknown active conflict")?;
            let c = c.unwrap();
            let side = if choice == ConflictChoice::Remote { "remote" } else { "local" };
            let chosen = json::map(c.get(side).unwrap_or(&Value::Null))?.clone();
            let kind = json::string(&chosen, "kind")?.to_string();
            let active_kind = if kind == "stroke" { "active" } else { "object-active" };
            let mut actions = vec![
                Value::Object(chosen.clone()),
                Value::Object(object! {"kind" => active_kind, kind.as_str() => chosen.get(&kind).cloned().unwrap_or(Value::Null), "active" => c.get(&format!("{}Active", side)).cloned().unwrap_or(Value::Null)}),
            ];
            if choice == ConflictChoice::Both {
                let mut duplicate = json::map(c.get("remote").unwrap_or(&Value::Null))?.clone();
                duplicate.insert(kind.clone(), Value::String(String::from(ids::Uuid::random())));
                if kind == "object" {
                    let x = json::number(&duplicate, "x")? + 24.0;
                    let y = json::number(&duplicate, "y")? + 24.0;
                    duplicate.insert("x".into(), json::double(x));
                    duplicate.insert("y".into(), json::double(y));
                } else {
                    let mut points = Vec::new();
                    for value in json::list_at(&duplicate, "points")? {
                        let p = json::list(value)?;
                        points.push(Value::Array(vec![
                            json::double(json::as_double(p.first())? + 24.0),
                            json::double(json::as_double(p.get(1))? + 24.0),
                            p.get(2).cloned().unwrap_or(Value::Null),
                        ]));
                    }
                    duplicate.insert("points".into(), Value::Array(points));
                }
                let dup_id = duplicate.get(&kind).cloned().unwrap_or(Value::Null);
                actions.push(Value::Object(duplicate));
                actions.push(Value::Object(object! {"kind" => active_kind, kind.as_str() => dup_id, "active" => c.get("remoteActive").cloned().unwrap_or(Value::Null)}));
            }
            actions.push(Value::Object(object! {"kind" => "conflict-status", "conflict" => id, "active" => false}));
            Ok(actions)
        })
    }
}

fn active_session(state: &Map, actor: &str) -> Result<Option<Value>> {
    match state.get("activeSessions") {
        None => Ok(None),
        Some(v) => Ok(json::map(v)?.get(actor).filter(|s| !s.is_null()).cloned()),
    }
}

/// `history(state, actor)`: the actor's `{undo, redo}` stacks, created on demand.
fn history_of<'a>(state: &'a mut Map, actor: &str) -> Result<&'a mut Map> {
    let actors = json::map_at_mut(state, "actors")?;
    json::put_if_absent(actors, actor, Value::Object(object! {"undo" => Value::Array(Vec::new()), "redo" => Value::Array(Vec::new())}));
    json::map_mut(actors.get_mut(actor).unwrap())
}

/// `index(state)`: operation id → operation over `events` and `pending`.
fn index(state: &Map) -> Result<HashMap<String, Value>> {
    let mut out = HashMap::new();
    for key in ["events", "pending"] {
        for op in json::list(state.get(key).unwrap_or(&Value::Null))? {
            out.insert(json::string(json::map(op)?, "id")?.to_string(), op.clone());
        }
    }
    Ok(out)
}
