//! Port of `Journal.java`: locked append-only transactions plus a
//! process-local materialized replica.
//!
//! On-disk format (byte compatible with Java):
//! - `document-<uuid>.json`: JSON base state (`Json.write`, UTF-8).
//! - `document-<uuid>.json.journal`: records of `[i32 BE length][UTF-8 JSON delta][i32 BE CRC32]`.
//! - `document-<uuid>.json.lock`: `fcntl` write lock over `[0, Long.MAX_VALUE)`,
//!   the same lock `FileChannel.lock()` takes on Linux/Android.

use crate::checkpoint_point_prefixes;
use crate::ids::DocumentName;
use crate::java_port::file::canonical_path;
use crate::error::{io_message, require, Error, ErrorKind, Result};
use crate::json::{self, Map, Value};
use crate::model::Model;
use crate::object;
use crate::store::{State, StateKind};
use crate::timing::{mark, TimingPhase};
use crate::wire::ReplicaState;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

/// The edit callback (`Store.Edit`).
pub type Edit<'a> = dyn FnMut(&mut State) -> Result<()> + 'a;

/// What an edit may change, which decides how its working state is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditScope {
    /// Any change. The edit works on a private copy of the state, compared
    /// against the cached state afterwards (Java `snapshot()`/`difference`).
    Full,
    /// Store's own action edits (append, commit, history, document delete).
    /// They never read `events`, `pending` or `relay`, only push new
    /// operations onto `pending`, and change `actors` only by popping and
    /// pushing undo/redo stack tails, replacing a stack with a new list or
    /// creating a missing actor as `{undo, redo}`. The edit sees `events`
    /// and `pending` as empty lists, and its new `pending` entries are added
    /// after the existing ones. `actors` is moved into the edit instead of
    /// copied and moved back after the commit. Java shared these lists by
    /// reference, so its edits cost O(changed entries), not O(document);
    /// `serde_json::Value` cannot share, so this scope gets the same cost by
    /// not copying what these edits never touch. The journal record and the
    /// resulting state are the same as with [`EditScope::Full`].
    Append,
}

struct Journal {
    /// Shared with the [`State`]s handed out (see `State.map`).
    state: Option<Arc<ReplicaState>>,
    committed: Model,
    effective: Model,
    received_ids: HashSet<String>,
    offset: u64,
    stamp: i64,
    size: i64,
}

impl Journal {
    fn new() -> Journal {
        Journal {
            state: None,
            committed: Model::new(),
            effective: Model::new(),
            received_ids: HashSet::new(),
            offset: 0,
            stamp: -1,
            size: -1,
        }
    }
}

fn caches() -> &'static Mutex<HashMap<String, Arc<Mutex<Journal>>>> {
    static CACHES: OnceLock<Mutex<HashMap<String, Arc<Mutex<Journal>>>>> = OnceLock::new();
    CACHES.get_or_init(|| Mutex::new(HashMap::new()))
}

thread_local! {
    /// Journal paths locked by this thread (Java: reentrant `synchronized`
    /// followed by a second `FileChannel.lock()` → `OverlappingFileLockException`).
    static HELD: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

struct HeldGuard(String);
impl Drop for HeldGuard {
    fn drop(&mut self) {
        HELD.with(|h| {
            h.borrow_mut().remove(&self.0);
        });
    }
}

fn enter(path: &str) -> Result<HeldGuard> {
    let nested = HELD.with(|h| !h.borrow_mut().insert(path.to_string()));
    if nested {
        return Err(Error::without_message(ErrorKind::OverlappingFileLock));
    }
    Ok(HeldGuard(path.to_string()))
}

fn cache_for(path: &str) -> Arc<Mutex<Journal>> {
    let mut map = caches().lock().unwrap_or_else(|e| e.into_inner());
    map.entry(path.to_string()).or_insert_with(|| Arc::new(Mutex::new(Journal::new()))).clone()
}

/// `File.lastModified()` in milliseconds (0 when missing).
fn last_modified(file: &Path) -> i64 {
    fs::metadata(file)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0))
        .unwrap_or(0)
}

/// `File.length()` (0 when missing).
fn file_length(file: &Path) -> i64 {
    fs::metadata(file).map(|m| m.len() as i64).unwrap_or(0)
}

fn with_suffix(file: &Path, suffix: &str) -> PathBuf {
    let mut s = file.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

fn io_error(path: &Path, e: std::io::Error) -> Error {
    Error::io(format!("{} ({})", path.display(), io_message(&e)))
}

/// Whole-file `fcntl` write lock, released on drop (with the file).
struct FileLock {
    _file: File,
}

fn lock_file(path: &Path) -> Result<FileLock> {
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).map_err(|e| io_error(path, e))?;
    // Java FileChannel.lock() == lock(0L, Long.MAX_VALUE, false) → F_SETLKW.
    let mut fl: libc::flock = unsafe { std::mem::zeroed() };
    fl.l_type = libc::F_WRLCK as _;
    fl.l_whence = libc::SEEK_SET as _;
    fl.l_start = 0;
    fl.l_len = i64::MAX as _;
    loop {
        let rc = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLKW, &fl) };
        if rc == 0 {
            break;
        }
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::Interrupted {
            return Err(Error::io(io_message(&err)));
        }
    }
    Ok(FileLock { _file: file })
}

fn sync_directory(dir: &Path) -> Result<()> {
    let d = File::open(dir).map_err(|e| io_error(dir, e))?;
    d.sync_all().map_err(Error::from)
}

fn parent_of(file: &Path) -> PathBuf {
    match file.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Distinguishes an edit rejected by its callback (cache intact) from other failures.
enum Failure {
    Rejected(Error),
    Other(Error),
}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        Failure::Other(e)
    }
}

fn op_id_key(op: &Value) -> String {
    match op.as_object().and_then(|m| m.get("id")) {
        Some(Value::String(s)) => s.clone(),
        Some(other) => format!("\u{0}{}", json::write(other)),
        None => "\u{0}null".to_string(),
    }
}

/// Supplies the process-local `(leases, activeSessions)` that `Store`
/// overlays on every state it returns (Java `Store.overlay`).
pub type Overlay<'a> = dyn Fn() -> (Map, Map) + 'a;

/// Whether a journal access may write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalMode {
    ReadOnly,
    Writable,
}

/// `Journal.access(file, document, edit, writable)`, followed by Java's
/// `Store.overlay` when `overlay` is given. Java's overlay mutated the fresh
/// shallow copy each access returned. Here the overlay keys are kept at the
/// end of the cached journal state itself, updated in place under the journal
/// lock, so the returned state can share that map instead of deep-copying it;
/// they are stripped wherever the state is persisted.
pub fn access(file: &Path, document: &DocumentName, edit: Option<&mut Edit<'_>>, scope: EditScope, mode: JournalMode, overlay: Option<&Overlay<'_>>) -> Result<State> {
    let (document, writable): (&str, bool) = (document, mode == JournalMode::Writable);
    let path = canonical_path(file);
    let cache = cache_for(&path);
    let _held = enter(&path)?;
    let mut journal = cache.lock().unwrap_or_else(|e| e.into_inner());
    let overlaid = |journal: &mut Journal, state: State| -> State {
        match overlay {
            None => state,
            Some(values) => {
                drop(state);
                let (leases, sessions) = values();
                journal.overlay(leases, sessions);
                journal.snapshot()
            }
        }
    };
    if !parent_of(file).exists() {
        return match journal.access_locked(file, document, edit, scope, false) {
            Ok(s) => Ok(overlaid(&mut journal, s)),
            Err(Failure::Rejected(e)) | Err(Failure::Other(e)) => Err(e),
        };
    }
    let _lock = lock_file(Path::new(&format!("{}.lock", path)))?;
    match journal.access_locked(file, document, edit, scope, writable) {
        Ok(s) => Ok(overlaid(&mut journal, s)),
        Err(Failure::Rejected(e)) => Err(e),
        Err(Failure::Other(e)) => {
            journal.state = None;
            Err(e)
        }
    }
}

/// The state without `Store`'s process-local overlay keys (never persisted).
fn without_overlay(state: &Map) -> Map {
    let mut out = state.clone();
    out.shift_remove("leases");
    out.shift_remove("activeSessions");
    out
}

crate::json_object_newtype! {
    /// `checkpoint(…)`: `{"revision", "journalBytesBefore", "journalBytesAfter", …}`.
    pub struct CheckpointResult;
}

/// `Journal.checkpoint(file, document)`: fold the WAL into a new base file.
pub fn checkpoint(file: &Path, document: &DocumentName) -> Result<CheckpointResult> {
    let document: &str = document;
    let path = canonical_path(file);
    let cache = cache_for(&path);
    let _held = enter(&path)?;
    let mut journal = cache.lock().unwrap_or_else(|e| e.into_inner());
    let _lock = lock_file(Path::new(&format!("{}.lock", path)))?;
    let result = journal.checkpoint_locked(file, document);
    if result.is_err() {
        journal.state = None;
    }
    result.map(CheckpointResult)
}

impl Journal {
    fn checkpoint_locked(&mut self, file: &Path, document: &str) -> Result<Map> {
        match self.access_locked(file, document, None, EditScope::Full, true) {
            Ok(_) => {}
            Err(Failure::Rejected(e)) | Err(Failure::Other(e)) => return Err(e),
        }
        let wal = with_suffix(file, ".journal");
        let before = file_length(&wal);
        let state = self.state.as_ref().unwrap();
        if before == 0 {
            return Ok(object! {"revision" => state.get("revision").cloned().unwrap_or(Value::Null), "journalBytesBefore" => 0, "journalBytesAfter" => 0});
        }
        let dir = parent_of(file);
        let (temporary, mut out) = create_temp_file(&dir, "document-checkpoint-", ".tmp")?;
        let result = (|| -> Result<Map> {
            let mut persisted: Map = without_overlay(state);
            let relay = json::list_at(state, "relay")?;
            let events = json::list_at(state, "events")?;
            let pending = json::list_at(state, "pending")?;
            // An empty stored relay is reconstructed from events and pending on
            // read. If the same operations have a different relay order, keep
            // only their IDs in that order.
            if relay.len() == events.len() + pending.len() {
                let mut same = true;
                for (i, r) in relay.iter().enumerate() {
                    let expected = if i < events.len() { &events[i] } else { &pending[i - events.len()] };
                    if r != expected {
                        same = false;
                        break;
                    }
                }
                if same {
                    persisted.insert("relay".into(), Value::Array(Vec::new()));
                } else if let Some(order) = relay_ids(relay, events, pending)? {
                    persisted.insert("relay".into(), Value::Array(order));
                    persisted.insert("relayFormat".into(), Value::String("ids".into()));
                }
            }
            checkpoint_point_prefixes::encode((&mut persisted).into())?;
            out.write_all(json::write_map(&persisted).as_bytes()).map_err(|e| io_error(&temporary, e))?;
            out.sync_all().map_err(|e| io_error(&temporary, e))?;
            fs::rename(&temporary, file).map_err(|e| io_error(&temporary, e))?;
            sync_directory(&dir)?;
            // A crash before truncation leaves the old WAL in place. Its
            // revisions are already in the new base and replay skips them.
            let log = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&wal).map_err(|e| io_error(&wal, e))?;
            log.set_len(0).map_err(|e| io_error(&wal, e))?;
            log.sync_all().map_err(|e| io_error(&wal, e))?;
            Ok(object! {"revision" => state.get("revision").cloned().unwrap_or(Value::Null), "journalBytesBefore" => before, "journalBytesAfter" => 0})
        })();
        let _ = fs::remove_file(&temporary);
        let result = result?;
        self.offset = 0;
        self.stamp = last_modified(file);
        self.size = file_length(file);
        Ok(result)
    }

    fn access_locked(&mut self, file: &Path, document: &str, edit: Option<&mut Edit<'_>>, scope: EditScope, writable: bool) -> std::result::Result<State, Failure> {
        let wal = with_suffix(file, ".journal");
        if self.state.is_none() || self.stamp != last_modified(file) || self.size != file_length(file) || (file_length(&wal) as u64) < self.offset {
            self.load(file, document)?;
        }
        if !file.exists() && writable {
            let initial = with_suffix(file, ".initial");
            {
                let mut out = File::create(&initial).map_err(|e| io_error(&initial, e))?;
                out.write_all(json::write_map(&without_overlay(self.state.as_ref().unwrap())).as_bytes()).map_err(|e| io_error(&initial, e))?;
                out.sync_all().map_err(|e| io_error(&initial, e))?;
            }
            fs::rename(&initial, file).map_err(|e| io_error(&initial, e))?;
            self.stamp = last_modified(file);
            self.size = file_length(file);
        }
        if !wal.exists() && !writable {
            return Ok(self.snapshot());
        }
        let new_journal = !wal.exists();
        let mut log = if writable {
            OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&wal)
        } else {
            OpenOptions::new().read(true).open(&wal)
        }
        .map_err(|e| io_error(&wal, e))?;
        if new_journal {
            sync_directory(&parent_of(file))?;
        }
        let length = log.metadata().map_err(Error::from)?.len();
        let mut position = self.offset;
        log.seek(SeekFrom::Start(position)).map_err(Error::from)?;
        let mut replayed = false;
        while position < length {
            let start = position;
            if length - start < 4 {
                if writable {
                    log.set_len(start).map_err(Error::from)?;
                    log.sync_all().map_err(Error::from)?;
                }
                break;
            }
            let mut word = [0u8; 4];
            log.read_exact(&mut word).map_err(Error::from)?;
            position += 4;
            let record = i32::from_be_bytes(word);
            if !(0..=40000000).contains(&record) {
                return Err(Error::io(format!("Invalid journal record at {}", start)).into());
            }
            if (length - position) < record as u64 + 4 {
                if writable {
                    log.set_len(start).map_err(Error::from)?;
                    log.sync_all().map_err(Error::from)?;
                }
                break;
            }
            let mut bytes = vec![0u8; record as usize];
            log.read_exact(&mut bytes).map_err(Error::from)?;
            log.read_exact(&mut word).map_err(Error::from)?;
            position += record as u64 + 4;
            if crc32fast::hash(&bytes) as i32 != i32::from_be_bytes(word) {
                return Err(Error::io(format!("Journal checksum mismatch at {}", start)).into());
            }
            let delta_value = json::parse(&String::from_utf8_lossy(&bytes))?;
            let delta = json::map(&delta_value)?;
            let state_revision = json::as_long(self.state.as_ref().unwrap().get("revision"))?;
            if json::as_long(delta.get("revision"))? > state_revision {
                self.apply(delta, true)?;
                replayed = true;
            }
            self.offset = position;
        }
        if replayed {
            self.rebuild_effective()?;
        }
        mark(TimingPhase("journal_wal_ready"));
        if let Some(edit) = edit {
            mark(TimingPhase("journal_begin"));
            if scope == EditScope::Append {
                // Unshare first: copying a shared cache would give every
                // history entry a new identity.
                Arc::make_mut(self.state.as_mut().unwrap());
                if let Some(before) = self.actor_records() {
                    self.append_edit(edit, &before, &mut log, &wal)?;
                    return Ok(self.snapshot());
                }
            }
            let mut next = self.edit_snapshot();
            let prepared = (|| -> Result<(Map, Vec<u8>)> {
                mark(TimingPhase("edit_begin"));
                edit(&mut next)?;
                mark(TimingPhase("edit_done"));
                let delta = difference(self.state.as_ref().unwrap(), &next)?;
                mark(TimingPhase("delta_done"));
                let bytes = json::write_map(&delta).into_bytes();
                mark(TimingPhase("serialize_done"));
                require(bytes.len() <= 40000000, "Local transaction exceeds 40 MB")?;
                Ok((delta, bytes))
            })();
            let (delta, bytes) = prepared.map_err(Failure::Rejected)?;
            let written = self.write_record(&mut log, &wal, &bytes)?;
            self.apply(&delta, false)?;
            self.offset += written;
            mark(TimingPhase("apply_done"));
        }
        Ok(self.snapshot())
    }

    /// Appends one `[length][delta][crc]` record at the replayed offset and
    /// syncs it; returns the record length.
    fn write_record(&self, log: &mut File, wal: &Path, bytes: &[u8]) -> Result<u64> {
        let crc = crc32fast::hash(bytes) as i32;
        let mut record = Vec::with_capacity(bytes.len() + 8);
        record.extend_from_slice(&(bytes.len() as i32).to_be_bytes());
        record.extend_from_slice(bytes);
        record.extend_from_slice(&crc.to_be_bytes());
        log.seek(SeekFrom::Start(self.offset)).map_err(Error::from)?;
        log.write_all(&record).map_err(|e| io_error(wal, e))?;
        log.sync_all().map_err(|e| io_error(wal, e))?;
        mark(TimingPhase("sync_done"));
        Ok(record.len() as u64)
    }

    fn load(&mut self, file: &Path, document: &str) -> Result<()> {
        let mut state = if file.exists() {
            let bytes = fs::read(file).map_err(|e| io_error(file, e))?;
            mark(TimingPhase("journal_base_read"));
            let source = String::from_utf8_lossy(&bytes);
            mark(TimingPhase("journal_base_decoded"));
            match json::parse(&source)? {
                Value::Object(m) => m,
                _ => return Err(Error::illegal_argument("Expected JSON object")),
            }
        } else {
            object! {"events" => Value::Array(Vec::new()), "pending" => Value::Array(Vec::new()), "cursor" => 0}
        };
        mark(TimingPhase("journal_base_parsed"));
        checkpoint_point_prefixes::decode((&mut state).into())?;
        mark(TimingPhase("journal_points_expanded"));
        json::put_if_absent(&mut state, "relay", Value::Array(Vec::new()));
        let relay_format = state.shift_remove("relayFormat");
        if relay_format.as_ref().and_then(Value::as_str) == Some("ids") {
            let relay = expand_relay_ids(json::list_at(&state, "relay")?, json::list_at(&state, "events")?, json::list_at(&state, "pending")?)?;
            state.insert("relay".into(), Value::Array(relay));
        } else {
            require(matches!(relay_format, None | Some(Value::Null)), "Unknown relay format")?;
            if json::list_at(&state, "relay")?.is_empty() {
                let mut all = json::list_at(&state, "events")?.clone();
                all.extend(json::list_at(&state, "pending")?.iter().cloned());
                *json::list_at_mut(&mut state, "relay")? = all;
            }
        }
        let mut received = HashSet::new();
        for op in json::list_at(&state, "relay")? {
            json::map(op)?;
            received.insert(op_id_key(op));
        }
        json::put_if_absent(&mut state, "actors", Value::Object(Map::new()));
        json::put_if_absent(&mut state, "revision", Value::from(0));
        json::put_if_absent(&mut state, "document", Value::String(document.to_string()));
        mark(TimingPhase("journal_relay_ready"));
        let mut committed = Model::new();
        for op in json::list_at(&state, "events")? {
            committed.apply(op.try_into()?)?;
        }
        mark(TimingPhase("journal_committed_ready"));
        let mut effective = committed.clone();
        for op in json::list_at(&state, "pending")? {
            effective.apply(op.try_into()?)?;
        }
        mark(TimingPhase("journal_effective_ready"));
        self.state = Some(Arc::new(state.into()));
        self.received_ids = received;
        self.committed = committed;
        self.effective = effective;
        self.offset = 0;
        self.stamp = last_modified(file);
        self.size = file_length(file);
        Ok(())
    }

    fn rebuild_effective(&mut self) -> Result<()> {
        let mut effective = self.committed.clone();
        for op in json::list_at(self.state.as_ref().unwrap(), "pending")? {
            effective.apply(op.try_into()?)?;
        }
        self.effective = effective;
        Ok(())
    }

    /// Puts `leases`, `activeSessions` as the last two keys of the cached
    /// state (Java's overlay removed and re-appended them), mutating only
    /// when they differ.
    fn overlay(&mut self, leases: Map, sessions: Map) {
        let Some(state) = self.state.as_ref() else { return };
        let mut tail = state.iter().rev();
        let current = matches!(tail.next(), Some((k, Value::Object(v))) if k == "activeSessions" && *v == sessions)
            && matches!(tail.next(), Some((k, Value::Object(v))) if k == "leases" && *v == leases);
        if current {
            return;
        }
        let state = Arc::make_mut(self.state.as_mut().unwrap());
        state.shift_remove("leases");
        state.shift_remove("activeSessions");
        state.insert("leases".into(), Value::Object(leases));
        state.insert("activeSessions".into(), Value::Object(sessions));
    }

    /// The state an edit works on (Java: `snapshot()`). `relay` is
    /// journal-internal: no edit reads it and `difference` ignores it in
    /// `next`, so the copy carries an empty placeholder at the same position
    /// instead of a second deep copy of every operation.
    fn edit_snapshot(&self) -> State {
        let state = self.state.as_ref().unwrap();
        let mut next = Map::new();
        for (k, v) in state.iter() {
            next.insert(k.clone(), if k == "relay" { Value::Array(Vec::new()) } else { v.clone() });
        }
        State::shared(Arc::new(ReplicaState(next)), Some(self.effective.clone()), StateKind::Journal)
    }

    fn snapshot(&self) -> State {
        State::shared(self.state.as_ref().unwrap().clone(), Some(self.effective.clone()), StateKind::Journal)
    }

    fn apply(&mut self, delta: &Map, replay: bool) -> Result<()> {
        self.apply_with(delta, replay, true)
    }

    /// `apply(delta, replay)`; `history` false leaves `actors` to the caller
    /// (see [`Journal::append_edit`]).
    fn apply_with(&mut self, delta: &Map, replay: bool, history: bool) -> Result<()> {
        let state = Arc::make_mut(self.state.as_mut().unwrap());
        apply_state(state, delta, &mut self.received_ids, history)?;
        let events = json::list_at(delta, "events")?;
        let added = json::list_at(delta, "added")?;
        let removed = json::list_at(delta, "removed")?;
        for op in events {
            self.committed.apply(op.try_into()?)?;
        }
        if !replay {
            if !events.is_empty() || !removed.is_empty() {
                let mut effective = self.committed.clone();
                for op in json::list_at(self.state.as_ref().unwrap(), "pending")? {
                    effective.apply(op.try_into()?)?;
                }
                self.effective = effective;
            } else {
                for op in added {
                    self.effective.apply(op.try_into()?)?;
                }
            }
        }
        Ok(())
    }

    /// The actor histories before an [`EditScope::Append`] edit, or `None`
    /// when `actors` is not a map of `{undo, redo}` lists of non-empty
    /// entries (the edit then takes the [`EditScope::Full`] path, which
    /// reports malformed state exactly like Java).
    fn actor_records(&self) -> Option<Vec<ActorRecord>> {
        let actors = self.state.as_ref()?.get("actors")?.as_object()?;
        let mut out = Vec::with_capacity(actors.len());
        for (key, value) in actors {
            let actor = value.as_object()?;
            let mut record = ActorRecord { key: key.clone(), fields: actor.keys().cloned().collect(), extras: Map::new(), undo: None, redo: None };
            for (field, v) in actor {
                if field == "undo" || field == "redo" {
                    let mut ids = Vec::with_capacity(v.as_array()?.len());
                    for entry in v.as_array()? {
                        ids.push(identity(entry)?);
                    }
                    if field == "undo" {
                        record.undo = Some(ids);
                    } else {
                        record.redo = Some(ids);
                    }
                } else {
                    record.extras.insert(field.clone(), v.clone());
                }
            }
            out.push(record);
        }
        Some(out)
    }

    /// An [`EditScope::Append`] edit. The edit's state shares nothing large
    /// with the cache: `events`, `pending` and `relay` are empty lists and
    /// `actors` is moved out of the cache. After the commit the cache appends
    /// the new operations and takes `actors` back. The delta and the cached
    /// state equal what the Full path computes (checked under
    /// `debug_assertions` or `NOTEMAKER_VERIFY_APPEND=1`).
    fn append_edit(&mut self, edit: &mut Edit<'_>, before: &[ActorRecord], log: &mut File, wal: &Path) -> std::result::Result<(), Failure> {
        let original: Option<(Map, HashSet<String>)> = if verify_append() { Some((Map::clone(self.state.as_ref().unwrap()), self.received_ids.clone())) } else { None };
        let mut next = Map::new();
        {
            let state = Arc::make_mut(self.state.as_mut().unwrap());
            for (k, v) in state.iter_mut() {
                let value = match k.as_str() {
                    "relay" | "events" | "pending" => Value::Array(Vec::new()),
                    "actors" => std::mem::take(v),
                    _ => v.clone(),
                };
                next.insert(k.clone(), value);
            }
        }
        let mut next = State::shared(Arc::new(ReplicaState(next)), Some(self.effective.clone()), StateKind::Journal);
        let prepared = (|| -> Result<(Map, Vec<u8>)> {
            mark(TimingPhase("edit_begin"));
            edit(&mut next)?;
            mark(TimingPhase("edit_done"));
            let delta = self.append_difference(&next, before)?;
            mark(TimingPhase("delta_done"));
            let bytes = json::write_map(&delta).into_bytes();
            mark(TimingPhase("serialize_done"));
            require(bytes.len() <= 40000000, "Local transaction exceeds 40 MB")?;
            Ok((delta, bytes))
        })();
        let (delta, bytes) = match prepared {
            Ok(prepared) => prepared,
            Err(e) => {
                self.restore_actors(next, before);
                return Err(Failure::Rejected(e));
            }
        };
        let mut next = next.into_map();
        if let Some((original, _)) = &original {
            let mut full = next.clone();
            for key in ["events", "pending"] {
                let mut list = json::list_at(original, key)?.clone();
                list.extend(json::list_at(&next, key)?.iter().cloned());
                full.insert(key.into(), Value::Array(list));
            }
            let expected = difference(original, &full)?;
            let expected = json::write_map(&expected);
            assert!(expected.as_bytes() == bytes, "Append-scope delta differs from the full difference:\n{}\n{}", expected, String::from_utf8_lossy(&bytes));
        }
        let written = self.write_record(log, wal, &bytes)?;
        let actors = next.get_mut("actors").map(std::mem::take).unwrap_or(Value::Null);
        Arc::make_mut(self.state.as_mut().unwrap()).insert("actors".into(), actors);
        self.apply_with(&delta, false, false)?;
        self.offset += written;
        mark(TimingPhase("apply_done"));
        if let Some((mut original, mut received)) = original {
            apply_state(&mut original, &delta, &mut received, true)?;
            assert!(json::write_map(&original) == json::write_map(self.state.as_ref().unwrap()), "Append-scope state differs from the full apply");
        }
        Ok(())
    }

    /// After a rejected [`EditScope::Append`] edit: return `actors` to the
    /// cache if the edit left it as it was, otherwise reload on next access.
    fn restore_actors(&mut self, next: State, before: &[ActorRecord]) {
        let mut next = next.into_map();
        let actors = next.get_mut("actors").map(std::mem::take);
        match actors {
            Some(actors) if same_actors(&actors, before) => {
                Arc::make_mut(self.state.as_mut().unwrap()).insert("actors".into(), actors);
            }
            _ => self.state = None,
        }
    }

    /// `difference(next)` for an [`EditScope::Append`] edit: `next.events`
    /// and `next.pending` hold only what the edit added, and `next.actors`
    /// is compared with `before` by entry identity, which is what Java's
    /// `equals` on shared references amounted to.
    fn append_difference(&self, next: &Map, before: &[ActorRecord]) -> Result<Map> {
        let state = self.state.as_ref().unwrap();
        let new_events = json::list_at(next, "events")?;
        let old_pending = json::list_at(state, "pending")?;
        let mut after: indexmap::IndexMap<&str, &Value> = indexmap::IndexMap::new();
        for op in json::list_at(next, "pending")? {
            after.insert(json::string(json::map(op)?, "id")?, op);
        }
        let mut added = Vec::new();
        for (id, op) in &after {
            // `received_ids` covers every pending ID; only a reused one needs the scan.
            let existing = if self.received_ids.contains(*id) { old_pending.iter().find(|o| o.get("id").and_then(Value::as_str) == Some(*id)) } else { None };
            match existing {
                None => added.push((*op).clone()),
                Some(old) => require(old == *op, "Pending operations are immutable")?,
            }
        }
        let history = append_history_difference(before, json::map_at(next, "actors")?)?;
        let mut list_changes = Map::new();
        for key in ["undo", "redo"] {
            if next.contains_key(key) && state.contains_key(key) && !json::objects_equal(state.get(key), next.get(key)) {
                list_changes.insert(key.into(), Value::Object(history_splice(json::list_at(state, key)?, json::list_at(next, key)?)));
            }
        }
        let mut meta = Map::new();
        for (k, v) in next {
            match k.as_str() {
                "events" | "pending" | "relay" | "revision" => {}
                // Without a history splice the actors always differ (see
                // `append_history_difference`).
                "actors" => {
                    if history.is_none() {
                        meta.insert(k.clone(), v.clone());
                    }
                }
                _ => {
                    if !json::objects_equal(state.get(k), Some(v)) {
                        meta.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        for key in list_changes.keys() {
            meta.shift_remove(key);
        }
        let revision = json::as_long(state.get("revision"))? + 1;
        let mut delta = object! {
            "revision" => revision,
            "events" => Value::Array(new_events.to_vec()),
            "added" => Value::Array(added),
            "removed" => Value::Array(Vec::new()),
            "meta" => Value::Object(meta),
        };
        if let Some(history) = history {
            if !history.is_empty() {
                delta.insert("history".into(), Value::Object(history));
            }
        }
        if !list_changes.is_empty() {
            delta.insert("lists".into(), Value::Object(list_changes));
        }
        Ok(delta)
    }
}

fn difference(state: &Map, next: &Map) -> Result<Map> {
    let old_events = json::list_at(state, "events")?;
    let new_events = json::list_at(next, "events")?;
    require(new_events.len() >= old_events.len() && new_events[..old_events.len()] == old_events[..], "Committed events are append-only")?;
    // Borrowed like Java's LinkedHashMaps of shared op references.
    let mut before: indexmap::IndexMap<&str, &Value> = indexmap::IndexMap::new();
    for op in json::list_at(state, "pending")? {
        before.insert(json::string(json::map(op)?, "id")?, op);
    }
    let mut after: indexmap::IndexMap<&str, &Value> = indexmap::IndexMap::new();
    for op in json::list_at(next, "pending")? {
        after.insert(json::string(json::map(op)?, "id")?, op);
    }
    let mut added = Vec::new();
    let mut removed = Vec::new();
    for id in before.keys() {
        if !after.contains_key(id) {
            removed.push(Value::String(id.to_string()));
        }
    }
    for (k, v) in &after {
        match before.get(k) {
            None => added.push((*v).clone()),
            Some(old) => require(old == v, "Pending operations are immutable")?,
        }
    }
    let history = history_difference(json::map_at(state, "actors")?, json::map_at(next, "actors")?)?;
    let mut list_changes = Map::new();
    for key in ["undo", "redo"] {
        if next.contains_key(key) && state.contains_key(key) && !json::objects_equal(state.get(key), next.get(key)) {
            list_changes.insert(key.into(), Value::Object(history_splice(json::list_at(state, key)?, json::list_at(next, key)?)));
        }
    }
    let mut meta = Map::new();
    for (k, v) in next {
        if !matches!(k.as_str(), "events" | "pending" | "relay" | "revision") && !json::objects_equal(state.get(k), Some(v)) && (k != "actors" || history.is_none()) {
            meta.insert(k.clone(), v.clone());
        }
    }
    for key in list_changes.keys() {
        meta.shift_remove(key);
    }
    let revision = json::as_long(state.get("revision"))? + 1;
    let mut delta = object! {
        "revision" => revision,
        "events" => Value::Array(new_events[old_events.len()..].to_vec()),
        "added" => Value::Array(added),
        "removed" => Value::Array(removed),
        "meta" => Value::Object(meta),
    };
    if let Some(history) = history {
        if !history.is_empty() {
            delta.insert("history".into(), Value::Object(history));
        }
    }
    if !list_changes.is_empty() {
        delta.insert("lists".into(), Value::Object(list_changes));
    }
    Ok(delta)
}

/// The state part of `apply(delta)`. `history` false skips the actor
/// history splices.
fn apply_state(state: &mut Map, delta: &Map, received_ids: &mut HashSet<String>, history: bool) -> Result<()> {
    let revision = json::as_long(delta.get("revision"))?;
    require(revision == json::as_long(state.get("revision"))? + 1, "Journal revision gap")?;
    let events = json::list_at(delta, "events")?;
    let added = json::list_at(delta, "added")?;
    let removed = json::list_at(delta, "removed")?;
    for op in events.iter().chain(added.iter()) {
        json::map(op)?;
        if received_ids.insert(op_id_key(op)) {
            json::list_at_mut(state, "relay")?.push(op.clone());
        }
    }
    json::list_at_mut(state, "events")?.extend(events.iter().cloned());
    {
        let pending = json::list_at_mut(state, "pending")?;
        if !removed.is_empty() {
            let ack: HashSet<String> = removed.iter().map(|v| match v {
                Value::String(s) => s.clone(),
                other => format!("\u{0}{}", json::write(other)),
            }).collect();
            pending.retain(|o| !ack.contains(&op_id_key(o)));
        }
        pending.extend(added.iter().cloned());
    }
    for (k, v) in json::map_at(delta, "meta")? {
        state.insert(k.clone(), v.clone());
    }
    let empty = Value::Object(Map::new());
    for (k, v) in json::map(json::get_or_default(delta, "lists", &empty))? {
        require(k == "undo" || k == "redo", "Invalid settings list splice")?;
        apply_splice(json::list_at_mut(state, k)?, json::map(v)?)?;
    }
    if history {
        for (actor_key, changes) in json::map(json::get_or_default(delta, "history", &empty))? {
            let actors = json::map_at_mut(state, "actors")?;
            let actor_value = actors
                .entry(actor_key.clone())
                .or_insert_with(|| Value::Object(object! {"undo" => Value::Array(Vec::new()), "redo" => Value::Array(Vec::new())}));
            if actor_value.is_null() {
                *actor_value = Value::Object(object! {"undo" => Value::Array(Vec::new()), "redo" => Value::Array(Vec::new())});
            }
            let actor = json::map_mut(actor_value)?;
            let changes = json::map(changes)?;
            for stack in ["undo", "redo"] {
                let change = json::map(changes.get(stack).unwrap_or(&Value::Null))?;
                if change.is_empty() {
                    continue;
                }
                apply_splice(json::list_at_mut(actor, stack)?, change)?;
            }
        }
    }
    state.insert("revision".into(), Value::from(revision));
    Ok(())
}

/// An actor's history before an [`EditScope::Append`] edit.
struct ActorRecord {
    key: String,
    /// The actor map's key order.
    fields: Vec<String>,
    /// Fields other than `undo` and `redo`.
    extras: Map,
    /// Entry identities (see [`identity`]); `None` when the stack is absent.
    undo: Option<Vec<usize>>,
    redo: Option<Vec<usize>>,
}

/// A history entry's identity, standing in for Java's reference equality:
/// the address of the entry's first key. It survives moves of the entry and
/// of its stack, and differs for a copy. `None` for anything but a non-empty
/// object.
fn identity(entry: &Value) -> Option<usize> {
    match entry {
        Value::Object(m) => m.iter().next().map(|(k, _)| k as *const String as usize),
        _ => None,
    }
}

fn same_actors(actors: &Value, before: &[ActorRecord]) -> bool {
    let Some(actors) = actors.as_object() else { return false };
    if actors.len() != before.len() {
        return false;
    }
    let same_stack = |value: Option<&Value>, ids: &Option<Vec<usize>>| match (value, ids) {
        (None, None) => true,
        (Some(Value::Array(list)), Some(ids)) => list.len() == ids.len() && list.iter().zip(ids).all(|(entry, id)| identity(entry) == Some(*id)),
        _ => false,
    };
    actors.iter().zip(before).all(|((key, value), record)| {
        let Some(actor) = value.as_object() else { return false };
        *key == record.key
            && actor.keys().eq(record.fields.iter())
            && same_stack(actor.get("undo"), &record.undo)
            && same_stack(actor.get("redo"), &record.redo)
            && actor.iter().filter(|(k, _)| *k != "undo" && *k != "redo").all(|(k, v)| record.extras.get(k) == Some(v))
    })
}

/// `historyDifference(before, after)` against recorded identities.
fn append_history_difference(before: &[ActorRecord], after: &Map) -> Result<Option<Map>> {
    for record in before {
        if !after.contains_key(&record.key) {
            return Ok(None);
        }
    }
    let records: HashMap<&str, &ActorRecord> = before.iter().map(|r| (r.key.as_str(), r)).collect();
    let empty_map = Map::new();
    let mut result = Map::new();
    for (actor, current) in after {
        let prior = records.get(actor.as_str()).copied();
        let current = json::map(current)?;
        let extras: Map = current.iter().filter(|(k, _)| *k != "undo" && *k != "redo").map(|(k, v)| (k.clone(), v.clone())).collect();
        if extras != *prior.map(|r| &r.extras).unwrap_or(&empty_map) {
            return Ok(None);
        }
        let stack = |key: &str| -> Result<&[Value]> {
            match current.get(key) {
                None => Ok(&[]),
                Some(v) => Ok(json::list(v)?),
            }
        };
        let undo = identity_splice(prior.and_then(|r| r.undo.as_deref()).unwrap_or(&[]), stack("undo")?);
        let redo = identity_splice(prior.and_then(|r| r.redo.as_deref()).unwrap_or(&[]), stack("redo")?);
        if prior.is_none() && undo.is_empty() && redo.is_empty() {
            return Ok(None);
        }
        if !undo.is_empty() || !redo.is_empty() {
            result.insert(actor.clone(), Value::Object(object! {"undo" => Value::Object(undo), "redo" => Value::Object(redo)}));
        }
    }
    Ok(Some(result))
}

/// `historySplice` where the old entries are known by identity.
fn identity_splice(before: &[usize], after: &[Value]) -> Map {
    let mut shared = 0;
    while shared < before.len() && shared < after.len() && identity(&after[shared]) == Some(before[shared]) {
        shared += 1;
    }
    if shared == before.len() && shared == after.len() {
        Map::new()
    } else {
        object! {"pop" => (before.len() - shared) as i64, "add" => Value::Array(after[shared..].to_vec())}
    }
}

/// Debug builds, or `NOTEMAKER_VERIFY_APPEND=1`, cross-check every
/// [`EditScope::Append`] edit against the Full path.
fn verify_append() -> bool {
    static VERIFY: OnceLock<bool> = OnceLock::new();
    *VERIFY.get_or_init(|| cfg!(debug_assertions) || std::env::var("NOTEMAKER_VERIFY_APPEND").is_ok_and(|v| v == "1"))
}

fn relay_ids(relay: &[Value], events: &[Value], pending: &[Value]) -> Result<Option<Vec<Value>>> {
    let mut indexed: HashMap<String, &Value> = HashMap::new();
    for value in events.iter().chain(pending.iter()) {
        let op = json::map(value)?;
        let id = match op.get("id") {
            Some(Value::String(s)) => s,
            _ => return Ok(None),
        };
        if indexed.contains_key(id) {
            return Ok(None);
        }
        indexed.insert(id.clone(), value);
    }
    if relay.len() != indexed.len() {
        return Ok(None);
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut order = Vec::new();
    for value in relay {
        let op = json::map(value)?;
        let id = op.get("id");
        let key = match id {
            Some(Value::String(s)) => s.clone(),
            _ => return Ok(None),
        };
        if !seen.insert(key.clone()) || indexed.get(&key).map(|v| *v != value) != Some(false) {
            return Ok(None);
        }
        order.push(Value::String(key));
    }
    Ok(Some(order))
}

fn expand_relay_ids(ids: &[Value], events: &[Value], pending: &[Value]) -> Result<Vec<Value>> {
    let mut indexed: HashMap<String, Value> = HashMap::new();
    for value in events.iter().chain(pending.iter()) {
        let op = json::map(value)?;
        let ok = match op.get("id") {
            Some(Value::String(s)) if !indexed.contains_key(s) => {
                indexed.insert(s.clone(), value.clone());
                true
            }
            _ => false,
        };
        require(ok, "Duplicate or invalid checkpoint operation ID")?;
    }
    let mut relay = Vec::with_capacity(ids.len());
    for id in ids {
        let op = match id {
            Value::String(s) => indexed.remove(s),
            _ => None,
        };
        require(op.is_some(), "Invalid checkpoint relay ID")?;
        relay.push(op.unwrap());
    }
    require(indexed.is_empty(), "Checkpoint relay order omits operations")?;
    Ok(relay)
}

/// Append only the changed suffix of each actor's history; `None` falls back to metadata.
fn history_difference(before: &Map, after: &Map) -> Result<Option<Map>> {
    let mut result = Map::new();
    for actor in before.keys() {
        if !after.contains_key(actor) {
            return Ok(None);
        }
    }
    let empty_map = Map::new();
    for (actor, current) in after {
        let prior = match before.get(actor) {
            Some(v) => json::map(v)?,
            None => &empty_map,
        };
        let current = json::map(current)?;
        let extras_equal = {
            let strip = |m: &Map| -> Map { m.iter().filter(|(k, _)| *k != "undo" && *k != "redo").map(|(k, v)| (k.clone(), v.clone())).collect() };
            strip(prior) == strip(current)
        };
        if !extras_equal {
            return Ok(None);
        }
        let undo = history_splice(stack(prior, "undo")?, stack(current, "undo")?);
        let redo = history_splice(stack(prior, "redo")?, stack(current, "redo")?);
        if !before.contains_key(actor) && undo.is_empty() && redo.is_empty() {
            return Ok(None);
        }
        if !undo.is_empty() || !redo.is_empty() {
            result.insert(actor.clone(), Value::Object(object! {"undo" => Value::Object(undo), "redo" => Value::Object(redo)}));
        }
    }
    Ok(Some(result))
}

/// An actor's `undo`/`redo` stack, empty when absent.
fn stack<'a>(m: &'a Map, key: &str) -> Result<&'a [Value]> {
    match m.get(key) {
        None => Ok(&[]),
        Some(v) => Ok(json::list(v)?),
    }
}

fn history_splice(before: &[Value], after: &[Value]) -> Map {
    let mut shared = 0;
    while shared < before.len() && shared < after.len() && before[shared] == after[shared] {
        shared += 1;
    }
    if shared == before.len() && shared == after.len() {
        Map::new()
    } else {
        object! {"pop" => (before.len() - shared) as i64, "add" => Value::Array(after[shared..].to_vec())}
    }
}

fn apply_splice(values: &mut Vec<Value>, change: &Map) -> Result<()> {
    let pop = json::as_int(change.get("pop"))?;
    require(pop >= 0 && pop as usize <= values.len(), "Invalid history splice")?;
    let keep = values.len() - pop as usize;
    values.truncate(keep);
    values.extend(json::list_at(change, "add")?.iter().cloned());
    Ok(())
}

/// `Files.createTempFile(dir, prefix, suffix)`.
fn create_temp_file(dir: &Path, prefix: &str, suffix: &str) -> Result<(PathBuf, File)> {
    for _ in 0..100 {
        let name = format!("{}{}{}", prefix, rand_digits(), suffix);
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_error(&path, e)),
        }
    }
    Err(Error::io("Unable to create temporary checkpoint file"))
}

fn rand_digits() -> String {
    let id = uuid::Uuid::new_v4();
    let bytes = id.as_bytes();
    let mut v: u64 = 0;
    for b in &bytes[..8] {
        v = (v << 8) | *b as u64;
    }
    (v & 0x7fff_ffff_ffff_ffff).to_string()
}
