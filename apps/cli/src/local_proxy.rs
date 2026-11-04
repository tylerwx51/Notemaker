//! Port of `LocalProxy.java`: sole local writer, durable replica peer, and
//! event-driven upstream synchronizer.
//!
//! `LocalProxy::new` returns `Arc<LocalProxy>`; the four Java threads
//! (`local-proxy`, `proxy-sync`, `proxy-notifications`, `proxy-ui-sessions`)
//! and the per-connection client threads hold clones. The proxy stays alive
//! until [`LocalProxy::close`], which joins every thread before releasing the
//! `proxy.lock` fcntl lock, exactly like Java.

use crate::jutil::{self, FileLock, Monitor};
use crate::main::{self as cli, CommandArgs};
use crate::ui_hub::UiHub;
use notemaker_core::clock::EpochMillis;
use notemaker_core::collaboration::{self, LEASE_MILLIS};
use notemaker_core::endpoint::{Disconnector, Endpoint, EndpointDescriptor};
use notemaker_core::error::require;
use notemaker_core::ids::{
    ActorId, DocumentName, EntityId, EntityKind, LayerId, PageNumber, SessionId,
};
use notemaker_core::layer_editor::{self, ItemFilter, LayerEditor};
use notemaker_core::net::{
    ApiPath, ChatId, ChatPath, EndpointPath, HeaderName, HeaderValue, HttpMethod, HttpStatus, Port,
    UiPath,
};
use notemaker_core::store::{self, HistoryDirection, ReplicaCursor, State, Store, SyncMode, WithState};
use notemaker_core::store::EditorPreferences;
use notemaker_core::wire::{EntityRefList, EntityRefs, Operation, ReplicaState};
use notemaker_core::timing::TimingPhase;
use notemaker_core::{actions, json, object, timing, Error, ErrorKind, Map, Result, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

notemaker_core::string_newtype! {
    /// The ID of a live app screen registered with
    /// [`LocalProxy::register_ui_session`].
    pub struct UiSessionId;
}

notemaker_core::string_newtype! {
    /// A UI session request kind: `state`, `actions` or `action`.
    pub struct UiVerb;
}

notemaker_core::string_newtype! {
    /// The name of a semantic UI action (`UiActions`), `""` unless the verb
    /// is `action`.
    pub struct UiActionName;
}

/// What [`LocalProxy::release`] does with a session's selection leases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseUpdate {
    /// Extend the leases (`selection heartbeat`).
    Renew,
    /// Drop the leases and the actor's active session.
    Release,
}

/// `System.currentTimeMillis()`.
fn now_millis() -> i64 {
    EpochMillis::now().0
}

/// Error type returned by [`UiSession::request`]. Its `Display` is used where
/// Java called `error.toString()` (core [`notemaker_core::Error`] already
/// displays as `java.lang.IllegalArgumentException: …`).
pub type UiError = Box<dyn std::error::Error + Send + Sync>;

notemaker_core::json_object_newtype! {
    /// The input of a live UI request: a `UiActions` action's structured input
    /// (empty for `state` and `actions`).
    pub struct UiActionInput;
}

notemaker_core::json_value_newtype! {
    /// A live UI session's reply: the screen state, the action registry or an
    /// action's structured result.
    pub struct UiReply;
}

notemaker_core::json_object_newtype! {
    /// One `/command` request body: `{document, actor, args, input}`.
    pub struct CommandRequest;
}

notemaker_core::json_value_newtype! {
    /// A command's structured `--input` JSON (`null` when absent).
    pub struct CommandInput;
}

notemaker_core::json_value_newtype! {
    /// A command's structured result, as printed by the CLI and returned
    /// under `result` by `/command` (`null` for no result).
    pub struct CommandReply;
}

/// [`LocalProxy::execute_with_snapshot`]'s reply with the snapshot it made.
#[derive(Clone, Debug)]
pub struct ExecutedCommand {
    pub reply: CommandReply,
    /// The reply's snapshot as a [`State`], or `None` when the command made
    /// no snapshot.
    pub snapshot: Option<State>,
}

notemaker_core::json_object_newtype! {
    /// `select(…)`'s request: `{session, layer?, page?, region?, entities?, topmost?, …}`.
    pub struct SelectionRequest;
}

notemaker_core::json_object_newtype! {
    /// `select(…)`'s reply: `{session, selected, skipped, expires, …}`.
    pub struct SelectionReply;
}

notemaker_core::json_object_newtype! {
    /// `release(…)`/`releaseActor(…)`'s reply.
    pub struct ReleaseReply;
}

/// `LocalProxy.UiSession`: a live tablet screen. Process-local; never enters
/// the replica journal. Called from proxy worker threads.
pub trait UiSession: Send + Sync {
    /// `Object request(String verb, String action, Map<String,Object> input)`.
    /// `verb` is `state`, `actions` or `action`; `action` is `""` unless
    /// `verb == "action"`.
    fn request(&self, verb: &UiVerb, action: &UiActionName, input: UiActionInput) -> std::result::Result<UiReply, UiError>;
}

/// A `UiError` as a core error: core errors pass through; other errors keep
/// their text under `IllegalStateException`.
fn ui_error(e: UiError) -> Error {
    match e.downcast::<Error>() {
        Ok(core) => *core,
        Err(other) => Error::illegal_state(other.to_string()),
    }
}

/// State guarded by the Java `changes` monitor.
struct Changes {
    generation: i64,
    upload_due: i64,
    retry_at: i64,
    fetch: bool,
    configuring: bool,
    requests: Vec<mpsc::Sender<Result<Map>>>,
}

type Waiter = mpsc::Sender<Result<Map>>;

/// `LocalProxy`. Thread-safe; share as `Arc<LocalProxy>`.
/// A `snapshot` reply and what it was computed from: the document, the
/// journal state map (by identity; the weak reference keeps its address
/// reserved) and the previews.
struct SnapshotCache {
    document: String,
    base: std::sync::Weak<ReplicaState>,
    previews: Map,
    reply: State,
}

thread_local! {
    /// `Some` while [`LocalProxy::execute_with_snapshot`] runs on this
    /// thread; holds the snapshot `finish_snapshot` made.
    static CAPTURED_SNAPSHOT: std::cell::RefCell<Option<Option<State>>> = const { std::cell::RefCell::new(None) };
}

pub struct LocalProxy {
    directory: PathBuf,
    listener: TcpListener,
    local_port: Port,
    ownership: Mutex<Option<FileLock>>,
    sockets: Mutex<HashMap<u64, TcpStream>>,
    next_socket: AtomicU64,
    documents: Mutex<indexmap::IndexMap<String, Arc<Store>>>,
    ephemeral_previews: Mutex<indexmap::IndexMap<String, Map>>,
    /// The last in-process [`LocalProxy::snapshot_state`] reply.
    snapshot_cache: Mutex<Option<SnapshotCache>>,
    /// Bumped (under the `ephemeral_previews` lock) by every local preview
    /// mutation; see `sync_once`.
    preview_epoch: AtomicU64,
    ui_sessions: Mutex<indexmap::IndexMap<String, Arc<dyn UiSession>>>,
    ui_hub: UiHub,
    ui_results: Mutex<indexmap::IndexMap<String, Map>>,
    ui_in_flight: Mutex<HashSet<String>>,
    ui_upstream: Mutex<Option<Arc<Endpoint>>>,
    changes: Mutex<Changes>,
    changes_cv: Condvar,
    settings: Mutex<Map>,
    id: String,
    token: String,
    closed: AtomicBool,
    preempt_fetch: AtomicBool,
    upstream: Mutex<Option<Arc<Endpoint>>>,
    stream: Mutex<Option<Disconnector>>,
    running: AtomicBool,
    this: Monitor,
    initialized: AtomicBool,
    sync_uploading: AtomicBool,
    active_document: Mutex<String>,
    last_upload: AtomicI64,
    synchronizer: OnceLock<ThreadId>,
    sync_alive: AtomicBool,
    workers: Mutex<Vec<std::thread::JoinHandle<()>>>,
    acceptor_done: Mutex<bool>,
    acceptor_cv: Condvar,
    clients: Mutex<usize>,
    clients_cv: Condvar,
    /// `public final Map<String,Boolean> connected`: per-document upstream
    /// reachability from the last sync attempt.
    pub connected: Mutex<HashMap<String, bool>>,
    /// `public final Map<String,String> errors`: per-document last sync error
    /// (`Throwable.toString()` text).
    pub errors: Mutex<HashMap<String, String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `System.nanoTime()/1000000`.
fn now() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as i64 + 1_000_000
}

fn empty_map() -> Value {
    Value::Object(Map::new())
}

fn leases_of(state: &Map) -> Result<Map> {
    Ok(json::map(json::get_or_default(state, "leases", &empty_map()))?.clone())
}

fn revision(state: &Map) -> Result<i64> {
    json::as_long(state.get("revision"))
}

fn map_value(value: Option<&Value>) -> Result<Map> {
    match value {
        None | Some(Value::Null) => Err(Error::null_pointer()),
        Some(v) => Ok(json::map(v)?.clone()),
    }
}

fn str_list(values: &[String]) -> Value {
    Value::Array(values.iter().cloned().map(Value::from).collect())
}

fn int_of(value: Option<&Value>) -> Result<i32> {
    json::as_int(value)
}

/// Java `Socket` errors that `configure` treats as "upstream offline":
/// `ConnectException`, `SocketTimeoutException`, `UnknownHostException`.
fn offline(e: &Error, endpoint: &Endpoint) -> bool {
    if e.kind != ErrorKind::Io {
        return false;
    }
    let m = e.message();
    let host = json::java_string(endpoint.descriptor.get("url"));
    // Java caught ConnectException. Android's libcore throws it for every
    // connect errno, so an unreachable network (no route yet right after
    // boot, or Wi-Fi off) also counts as an offline main server.
    m.contains("Connection refused")
        || m.contains("Network is unreachable")
        || m.contains("No route to host")
        || m.contains("timed out")
        || host.contains(&format!("//{}", m))
        || m.starts_with("Invalid host")
}

struct SelectionScope {
    layer: String,
    page: PageNumber,
}

impl LocalProxy {
    /// `new LocalProxy(directory, port)`: takes the `proxy.lock` ownership
    /// lock (`IOException("A proxy already owns this replica")` when held),
    /// loads/creates `proxy-config.json`, opens every known document, binds
    /// `127.0.0.1:port` (0 = ephemeral), writes `proxy.json`, and starts the
    /// worker threads. A shutdown hook (SIGINT/SIGTERM in the CLI process)
    /// closes the proxy like Java's `notemaker-proxy-shutdown` hook.
    ///
    /// Java wrapped setup failures as `new IOException(cause)`; the CLI prints
    /// the root cause, so the Rust error is the root cause itself.
    pub fn new(directory: &Path, port: Port) -> Result<Arc<LocalProxy>> {
        std::fs::create_dir_all(directory).map_err(|e| {
            Error::io(format!(
                "{}: {}",
                directory.display(),
                notemaker_core::error::io_message(&e)
            ))
        })?;
        let held = FileLock::try_lock(&directory.join("proxy.lock"))?
            .ok_or_else(|| Error::io("A proxy already owns this replica"))?;
        timing::mark(TimingPhase("proxy_lock_done"));
        let mut ownership = Some(held);
        let result = LocalProxy::open(directory, port, &mut ownership);
        if result.is_err() {
            if let Some(held) = ownership.take() {
                held.release();
            }
        }
        result
    }

    fn open(
        directory: &Path,
        port: Port,
        ownership: &mut Option<FileLock>,
    ) -> Result<Arc<LocalProxy>> {
        let config = directory.join("proxy-config.json");
        let settings = if config.exists() {
            let bytes = std::fs::read(&config).map_err(|e| {
                Error::io(format!(
                    "{} ({})",
                    config.display(),
                    notemaker_core::error::io_message(&e)
                ))
            })?;
            json::map(&json::parse(&String::from_utf8_lossy(&bytes))?)?.clone()
        } else {
            object! {"id" => String::from(notemaker_core::ids::Uuid::random()), "token" => format!("{}{}", String::from(notemaker_core::ids::Uuid::random()), String::from(notemaker_core::ids::Uuid::random()))}
        };
        let id = json::string(&settings, "id")?.to_string();
        let token = json::string(&settings, "token")?.to_string();
        timing::mark(TimingPhase("proxy_config_read"));
        let upstream = match settings.get("upstream") {
            Some(v) => Some(Arc::new(Endpoint::new((map_value(Some(v))?).into()))),
            None => None,
        };
        let listener = {
            // Bind late (after validation) like Java, but the socket is needed
            // in the struct; validation failures release it on drop.
            let address = format!("127.0.0.1:{}", port);
            TcpListener::bind(&address)
                .map_err(|e| Error::io(notemaker_core::error::io_message(&e)))?
        };
        let local_port = listener.local_addr().map(|a| Port::from(a.port())).unwrap_or(port);
        let proxy = Arc::new(LocalProxy {
            directory: directory.to_path_buf(),
            listener,
            local_port,
            ownership: Mutex::new(None),
            sockets: Mutex::new(HashMap::new()),
            next_socket: AtomicU64::new(0),
            documents: Mutex::new(indexmap::IndexMap::new()),
            ephemeral_previews: Mutex::new(indexmap::IndexMap::new()),
            snapshot_cache: Mutex::new(None),
            preview_epoch: AtomicU64::new(0),
            ui_sessions: Mutex::new(indexmap::IndexMap::new()),
            ui_hub: UiHub::new(),
            ui_results: Mutex::new(indexmap::IndexMap::new()),
            ui_in_flight: Mutex::new(HashSet::new()),
            ui_upstream: Mutex::new(None),
            changes: Mutex::new(Changes {
                generation: 0,
                upload_due: i64::MAX,
                retry_at: 0,
                fetch: true,
                configuring: false,
                requests: Vec::new(),
            }),
            changes_cv: Condvar::new(),
            settings: Mutex::new(settings.clone()),
            id,
            token,
            closed: AtomicBool::new(false),
            preempt_fetch: AtomicBool::new(false),
            upstream: Mutex::new(upstream),
            stream: Mutex::new(None),
            running: AtomicBool::new(true),
            this: Monitor::default(),
            initialized: AtomicBool::new(false),
            sync_uploading: AtomicBool::new(false),
            active_document: Mutex::new(String::new()),
            last_upload: AtomicI64::new(i64::MIN / 2),
            synchronizer: OnceLock::new(),
            sync_alive: AtomicBool::new(false),
            workers: Mutex::new(Vec::new()),
            acceptor_done: Mutex::new(false),
            acceptor_cv: Condvar::new(),
            clients: Mutex::new(0),
            clients_cv: Condvar::new(),
            connected: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashMap::new()),
        });
        let empty = Value::Array(Vec::new());
        for document in json::list(json::get_or_default(&settings, "documents", &empty))? {
            proxy.store(
                document
                    .as_str()
                    .ok_or_else(|| Error::class_cast("documents entry is not a String"))?,
            )?;
        }
        timing::mark(TimingPhase("proxy_known_documents"));
        let known: HashSet<PathBuf> = lock(&proxy.documents)
            .values()
            .map(|s| s.file.clone())
            .collect();
        let mut files: Vec<PathBuf> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(directory) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("document-") && name.ends_with(".json") {
                    files.push(entry.path());
                }
            }
        }
        for file in files {
            // Known documents are validated by Store.read below. Parsing a
            // checkpointed base here would deserialize it twice on startup.
            if known.iter().any(|k| k.file_name() == file.file_name()) {
                continue;
            }
            let bytes = std::fs::read(&file).map_err(|e| {
                Error::io(format!(
                    "{} ({})",
                    file.display(),
                    notemaker_core::error::io_message(&e)
                ))
            })?;
            let state = json::parse(&String::from_utf8_lossy(&bytes))?;
            let state = json::map(&state)?;
            if state.contains_key("document") {
                proxy.store(json::string(state, "document")?)?;
            }
        }
        timing::mark(TimingPhase("proxy_document_scan"));
        for local in proxy.document_stores() {
            if let Some(root) = local
                .read()?
                .get("serverId")
                .cloned()
                .filter(|v| !v.is_null())
            {
                let mut settings = lock(&proxy.settings);
                require(
                    !settings.contains_key("rootId") || settings.get("rootId") == Some(&root),
                    "Replica contains different server databases",
                )?;
                settings.insert("rootId".into(), root);
            }
        }
        timing::mark(TimingPhase("proxy_root_validation"));
        {
            let keys = proxy.document_names();
            lock(&proxy.settings).insert("documents".into(), str_list(&keys));
        }
        proxy.save_settings()?;
        proxy.initialized.store(true, Ordering::SeqCst);
        timing::mark(TimingPhase("proxy_config_synced"));
        timing::mark(TimingPhase("proxy_bound"));
        proxy.durable(&directory.join("proxy.json"), &Value::Object(object! {"url" => format!("http://127.0.0.1:{}", proxy.port()), "token" => proxy.token.clone(), "id" => proxy.id.clone()}))?;
        timing::mark(TimingPhase("proxy_descriptor_synced"));
        *lock(&proxy.ownership) = ownership.take();
        {
            let pending = proxy.has_pending();
            lock(&proxy.changes).upload_due = if pending { now() + 500 } else { i64::MAX };
        }
        timing::mark(TimingPhase("proxy_pending_checked"));
        LocalProxy::start(&proxy);
        Ok(proxy)
    }

    fn start(proxy: &Arc<LocalProxy>) {
        let spawn = |name: &str, body: Box<dyn FnOnce() + Send>| {
            std::thread::Builder::new()
                .name(name.to_string())
                .spawn(body)
                .expect("spawn proxy thread")
        };
        let p = proxy.clone();
        let acceptor = spawn(
            "local-proxy",
            Box::new(move || {
                p.accept_loop();
                *lock(&p.acceptor_done) = true;
                p.acceptor_cv.notify_all();
            }),
        );
        proxy.sync_alive.store(true, Ordering::SeqCst);
        let p = proxy.clone();
        let synchronizer = spawn(
            "proxy-sync",
            Box::new(move || {
                let _ = p.synchronizer.set(std::thread::current().id());
                p.sync_loop();
                p.sync_alive.store(false, Ordering::SeqCst);
            }),
        );
        let p = proxy.clone();
        let subscriber = spawn("proxy-notifications", Box::new(move || p.subscribe()));
        let p = proxy.clone();
        let ui_worker = spawn("proxy-ui-sessions", Box::new(move || p.ui_loop()));
        drop(acceptor);
        lock(&proxy.workers).extend([synchronizer, subscriber, ui_worker]);
        // Wait until the sync thread has published its id (cancel target).
        while proxy.synchronizer.get().is_none() && proxy.sync_alive.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
    }

    fn accept_loop(self: &Arc<Self>) {
        while self.running.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((socket, _)) => {
                    if !self.running.load(Ordering::SeqCst) {
                        break;
                    }
                    let key = self.next_socket.fetch_add(1, Ordering::SeqCst);
                    if let Ok(clone) = socket.try_clone() {
                        lock(&self.sockets).insert(key, clone);
                    }
                    let p = self.clone();
                    self.client(move || p.handle(socket, key));
                }
                Err(e) => {
                    if self.running.load(Ordering::SeqCst) {
                        eprintln!(
                            "Proxy accept: java.net.SocketException: {}",
                            notemaker_core::error::io_message(&e)
                        );
                    }
                }
            }
        }
    }

    /// `clients.execute(task)`: a tracked worker thread.
    fn client(self: &Arc<Self>, task: impl FnOnce() + Send + 'static) {
        *lock(&self.clients) += 1;
        let p = self.clone();
        let spawned = std::thread::Builder::new()
            .name("local-proxy-client".into())
            .spawn(move || {
                task();
                let mut count = lock(&p.clients);
                *count -= 1;
                p.clients_cv.notify_all();
            });
        if spawned.is_err() {
            *lock(&self.clients) -= 1;
            self.clients_cv.notify_all();
        }
    }

    fn document_names(&self) -> Vec<String> {
        hash_order(lock(&self.documents).keys())
    }

    fn document_stores(&self) -> Vec<Arc<Store>> {
        let docs = lock(&self.documents);
        hash_order(docs.keys())
            .into_iter()
            .filter_map(|k| docs.get(&k).cloned())
            .collect()
    }

    fn durable(&self, destination: &Path, value: &Value) -> Result<()> {
        let io = |e: std::io::Error| Error::io(notemaker_core::error::io_message(&e));
        let mut temporary;
        let mut file;
        loop {
            temporary = self
                .directory
                .join(format!("proxy-{}.tmp", (rand_u64() >> 1)));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(f) => {
                    file = f;
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io(e)),
            }
        }
        let written = file
            .write_all(json::write(value).as_bytes())
            .and_then(|_| file.sync_all());
        drop(file);
        if let Err(e) = written {
            let _ = std::fs::remove_file(&temporary);
            return Err(io(e));
        }
        std::fs::rename(&temporary, destination).map_err(io)?;
        if let Ok(dir) = std::fs::File::open(&self.directory) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    fn save_settings(&self) -> Result<()> {
        let ordered = {
            let settings = lock(&self.settings);
            let mut out = Map::new();
            for key in hash_order(settings.keys()) {
                out.insert(key.clone(), settings[&key].clone());
            }
            out
        };
        self.durable(
            &self.directory.join("proxy-config.json"),
            &Value::Object(ordered),
        )
    }

    fn store(&self, document: &str) -> Result<Arc<Store>> {
        if let Some(found) = lock(&self.documents).get(document) {
            return Ok(found.clone());
        }
        let created = Arc::new(Store::writer(&self.directory, &DocumentName::from(document))?);
        let (store, fresh) = {
            let mut docs = lock(&self.documents);
            match docs.get(document) {
                Some(previous) => (previous.clone(), false),
                None => {
                    docs.insert(document.to_string(), created.clone());
                    (created, true)
                }
            }
        };
        if fresh {
            let _sync = self.this.enter();
            if self.initialized.load(Ordering::SeqCst) {
                let keys = self.document_names();
                lock(&self.settings).insert("documents".into(), str_list(&keys));
                self.save_settings()?;
                self.wake_fetch();
            }
        }
        Ok(store)
    }

    fn changed(&self, local: bool) {
        {
            let mut c = lock(&self.changes);
            c.generation += 1;
            if local {
                c.upload_due = c.upload_due.min(now() + 500);
            }
            self.changes_cv.notify_all();
        }
        // A fetch-only pass over an idle document must not hold up a newly edited
        // active document until the peer's read timeout. Abort only the sync
        // worker's request; the next cycle will retry under the normal upload gate.
        if local {
            self.preempt_fetch_sweep();
        }
    }

    /// `ephemeralPreviews` for a local mutation: bumps `preview_epoch` while
    /// the lock is held.
    fn previews_mut(&self) -> MutexGuard<'_, indexmap::IndexMap<String, Map>> {
        let guard = lock(&self.ephemeral_previews);
        self.preview_epoch.fetch_add(1, Ordering::SeqCst);
        guard
    }

    fn wake_ui(&self) {
        let _c = lock(&self.changes);
        self.changes_cv.notify_all();
    }

    fn upstream(&self) -> Option<Arc<Endpoint>> {
        lock(&self.upstream).clone()
    }

    fn ui_peer(&self) -> Option<Arc<Endpoint>> {
        lock(&self.ui_upstream).clone().or_else(|| self.upstream())
    }

    /// `registerUiSession(session, handler)`; `"Session ID is required"`.
    pub fn register_ui_session(&self, session: &UiSessionId, handler: Arc<dyn UiSession>) -> Result<()> {
        let session: &str = session;
        require(!session.is_empty(), "Session ID is required")?;
        lock(&self.ui_sessions).insert(session.to_string(), handler);
        self.wake_ui();
        Ok(())
    }

    /// `unregisterUiSession(session)`.
    pub fn unregister_ui_session(&self, session: &UiSessionId) {
        let session: &str = session;
        lock(&self.ui_sessions).shift_remove(session);
        self.wake_ui();
    }

    /// `setUiUpstream(descriptor)`; `None` is Java `null` (use the replica
    /// upstream for UI relaying).
    pub fn set_ui_upstream(&self, descriptor: Option<&EndpointDescriptor>) {
        *lock(&self.ui_upstream) = descriptor.map(|d| Arc::new(Endpoint::new(d.clone())));
        self.wake_ui();
    }

    /// `version()`: change generation.
    pub fn version(&self) -> i64 {
        lock(&self.changes).generation
    }

    /// `awaitChange(previous)`: blocks until the generation differs from
    /// `previous` or the proxy closes; returns the current generation.
    pub fn await_change(&self, previous: i64) -> i64 {
        let mut c = lock(&self.changes);
        while self.running.load(Ordering::SeqCst) && c.generation == previous {
            c = self.changes_cv.wait(c).unwrap_or_else(|e| e.into_inner());
        }
        c.generation
    }

    /// `awaitChange` with a bound, for callers that must observe a
    /// cooperative interrupt (Java interrupted the waiting thread instead).
    pub fn await_change_timeout(&self, previous: i64, timeout: Duration) -> i64 {
        let deadline = Instant::now() + timeout;
        let mut c = lock(&self.changes);
        while self.running.load(Ordering::SeqCst) && c.generation == previous {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            c = self
                .changes_cv
                .wait_timeout(c, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        c.generation
    }

    fn has_pending(&self) -> bool {
        for local in self.document_stores() {
            match local
                .read()
                .and_then(|s| Ok(!json::list_at(&s, "pending")?.is_empty()))
            {
                Ok(true) | Err(_) => return true,
                Ok(false) => {}
            }
        }
        false
    }

    fn wake_fetch(&self) {
        {
            let mut c = lock(&self.changes);
            c.fetch = true;
            self.changes_cv.notify_all();
        }
        self.preempt_fetch_sweep();
    }

    fn preempt_fetch_sweep(&self) {
        if self.sync_uploading.load(Ordering::SeqCst)
            || !self.sync_alive.load(Ordering::SeqCst)
            || self
                .preempt_fetch
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return;
        }
        let cancelled = match (self.upstream(), self.synchronizer.get()) {
            (Some(peer), Some(thread)) => peer.cancel(*thread),
            _ => false,
        };
        if !cancelled {
            self.preempt_fetch.store(false, Ordering::SeqCst);
        }
    }

    /// `synchronized configure(document, target)`: `target` is an upstream
    /// URL, a descriptor file, or `None`/`""` to disconnect.
    pub fn configure(&self, document: &DocumentName, target: Option<&str>) -> Result<()> {
        let document: &str = document;
        let _sync = self.this.enter();
        self.store(document)?;
        let candidate = match target {
            None | Some("") => None,
            Some(t) => Some(Arc::new(Endpoint::resolve(t)?)),
        };
        let old = self.upstream();
        if old.as_ref().map(|e| &e.descriptor) == candidate.as_ref().map(|e| &e.descriptor) {
            self.wake_fetch();
            return Ok(());
        }
        // Publish tentatively before validation so simultaneous cross-links see each other.
        {
            let mut c = lock(&self.changes);
            c.configuring = true;
            *lock(&self.upstream) = candidate.clone();
        }
        let attempt = (|| -> Result<()> {
            if let Some(candidate) = &candidate {
                let validated = (|| -> Result<()> {
                    let parent = candidate
                        .request(ApiPath::Info { trail: self.id.clone(), validate: true }, None)?;
                    require(
                        int_of(Some(json::get_or_default(
                            &parent,
                            "protocol",
                            &Value::from(0),
                        )))? >= 3,
                        "Upstream requires replica protocol 3",
                    )?;
                    require(
                        candidate.descriptor.contains_key("token")
                            || parent.get("kind").and_then(Value::as_str) == Some("server"),
                        "Use a private descriptor for a proxy upstream",
                    )?;
                    let root = parent.get("rootId").filter(|v| !v.is_null());
                    let settings = lock(&self.settings);
                    require(
                        root.is_none()
                            || !settings.contains_key("rootId")
                            || settings.get("rootId") == root,
                        "Server database differs from this replica",
                    )
                })();
                if let Err(e) = validated {
                    if !offline(&e, candidate) || candidate.descriptor.contains_key("token") {
                        return Err(e);
                    }
                    // A known-address main server may be offline.
                }
            }
            {
                let mut settings = lock(&self.settings);
                match &candidate {
                    None => {
                        settings.shift_remove("upstream");
                    }
                    Some(c) => {
                        settings.insert("upstream".into(), Value::Object(c.descriptor.clone().into()));
                    }
                }
            }
            self.save_settings()
        })();
        if attempt.is_err() {
            *lock(&self.upstream) = old;
        }
        {
            let mut c = lock(&self.changes);
            c.configuring = false;
            self.changes_cv.notify_all();
        }
        attempt?;
        if let Some(active) = lock(&self.stream).as_ref() {
            active.disconnect();
        }
        let pending = self.has_pending();
        {
            let mut c = lock(&self.changes);
            c.fetch = true;
            c.upload_due = if pending { now() + 500 } else { i64::MAX };
            c.retry_at = 0;
            self.changes_cv.notify_all();
        }
        self.changed(false);
        Ok(())
    }

    fn info(&self, trail: &str, validate: bool) -> Result<Map> {
        let mut lineage: Vec<String> = if trail.is_empty() {
            Vec::new()
        } else {
            split(trail, ",")
        };
        require(!lineage.contains(&self.id), "Proxy cycle rejected")?;
        require(lineage.len() < 64, "Proxy chain exceeds 64 hops")?;
        lineage.push(self.id.clone());
        let mut root = lock(&self.settings)
            .get("rootId")
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(peer) = self
            .upstream()
            .filter(|p| validate && p.descriptor.contains_key("token"))
        {
            let parent = peer.request(
                ApiPath::Info {
                    trail: lineage.join(","),
                    validate: true,
                },
                None,
            )?;
            root = parent.get("rootId").cloned().unwrap_or(Value::Null);
            lineage = json::list(parent.get("lineage").unwrap_or(&Value::Null))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| Error::class_cast("lineage entry is not a String"))
                })
                .collect::<Result<_>>()?;
        }
        Ok(
            object! {"kind" => "proxy", "protocol" => 3, "id" => self.id.clone(), "rootId" => root, "lineage" => str_list(&lineage), "documents" => str_list(&self.document_names())},
        )
    }

    fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// `Thread.sleep(ms)` that ends early when the proxy closes.
    fn sleep(&self, ms: i64) -> bool {
        let deadline = Instant::now() + Duration::from_millis(ms.max(0) as u64);
        let mut c = lock(&self.changes);
        while self.running() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            c = self
                .changes_cv
                .wait_timeout(c, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        false
    }

    fn sync_loop(self: &Arc<Self>) {
        let mut backoff: i64 = 500;
        while self.running() {
            let (peer, upload, waiters) = {
                let mut c = lock(&self.changes);
                loop {
                    let t = now();
                    if !(self.running()
                        && (c.configuring
                            || lock(&self.upstream).is_none()
                            || t < c.retry_at
                            || (!c.fetch && t < c.upload_due)))
                    {
                        break;
                    }
                    let deadline = if c.retry_at > t {
                        c.retry_at
                    } else {
                        c.upload_due
                    };
                    if c.configuring || lock(&self.upstream).is_none() || deadline == i64::MAX {
                        c = self.changes_cv.wait(c).unwrap_or_else(|e| e.into_inner());
                    } else {
                        c = self
                            .changes_cv
                            .wait_timeout(c, Duration::from_millis((deadline - t).max(1) as u64))
                            .unwrap_or_else(|e| e.into_inner())
                            .0;
                    }
                }
                if !self.running() {
                    return;
                }
                let peer = self.upstream().expect("upstream checked under lock");
                let upload = now() >= c.upload_due;
                self.sync_uploading.store(upload, Ordering::SeqCst);
                c.fetch = false;
                if upload {
                    c.upload_due = i64::MAX;
                }
                let waiters: Vec<Waiter> = if upload {
                    std::mem::take(&mut c.requests)
                } else {
                    Vec::new()
                };
                (peer, upload, waiters)
            };
            match self.sync_once(&peer, upload) {
                Ok(result) => {
                    self.preempt_fetch.store(false, Ordering::SeqCst);
                    backoff = 500;
                    for waiter in waiters {
                        let _ = waiter.send(Ok(result.clone()));
                    }
                }
                Err(e) => {
                    if self.preempt_fetch.swap(false, Ordering::SeqCst) {
                        let mut c = lock(&self.changes);
                        c.retry_at = 0;
                        c.fetch = true;
                        self.changes_cv.notify_all();
                        backoff = 500;
                        // Java re-queued nothing here: the waiters were dropped.
                        for waiter in waiters {
                            let _ = waiter.send(Err(e.clone()));
                        }
                        continue;
                    }
                    for doc in self.document_names() {
                        lock(&self.connected).insert(doc.clone(), false);
                        lock(&self.errors).insert(doc, e.to_string());
                    }
                    self.changed(false);
                    for waiter in waiters {
                        let _ = waiter.send(Err(e.clone()));
                    }
                    let mut c = lock(&self.changes);
                    c.retry_at = now() + backoff;
                    c.fetch = true;
                    c.upload_due = c.upload_due.min(c.retry_at);
                    drop(c);
                    backoff = (backoff * 2).min(30000);
                }
            }
        }
    }

    fn sync_once(self: &Arc<Self>, peer: &Endpoint, upload: bool) -> Result<Map> {
        let remote = peer.request(
            ApiPath::Info {
                trail: self.id.clone(),
                validate: false,
            },
            None,
        )?;
        require(
            int_of(Some(json::get_or_default(
                &remote,
                "protocol",
                &Value::from(0),
            )))? >= 3,
            "Upstream requires replica protocol 3",
        )?;
        require(
            remote.get("id").and_then(Value::as_str) != Some(self.id.as_str()),
            "Proxy cycle rejected",
        )?;
        if let Some(root) = remote.get("rootId").filter(|v| !v.is_null()) {
            let _sync = self.this.enter();
            let changed = {
                let mut settings = lock(&self.settings);
                require(
                    !settings.contains_key("rootId") || settings.get("rootId") == Some(root),
                    "Server database differs from this replica",
                )?;
                if settings.get("rootId") != Some(root) {
                    settings.insert("rootId".into(), root.clone());
                    true
                } else {
                    false
                }
            };
            if changed {
                self.save_settings()?;
            }
        }
        let remote_id = json::string(&remote, "id")?.to_string();
        let mut result = Map::new();
        let mut more = false;
        let priority = lock(&self.active_document).clone();
        let mut sync_documents: Vec<(String, Arc<Store>)> = {
            let docs = lock(&self.documents);
            hash_order(docs.keys())
                .into_iter()
                .map(|k| (k.clone(), docs[&k].clone()))
                .collect()
        };
        sync_documents.sort_by(|a, b| {
            let (aa, bb) = (a.0 == priority, b.0 == priority);
            if aa != bb {
                return if aa {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
            }
            if let (Ok(ap), Ok(bp)) = (a.1.needs_upload(&remote_id), b.1.needs_upload(&remote_id)) {
                if ap != bp {
                    return if ap {
                        std::cmp::Ordering::Less
                    } else {
                        std::cmp::Ordering::Greater
                    };
                }
            }
            jcompare(&a.0, &b.0)
        });
        for (name, local) in sync_documents {
            let before_state = local.read()?;
            let leases_before = leases_of(&before_state)?;
            let before = revision(&before_state)?;
            let mut gate = || -> Result<()> {
                let delay = self.last_upload.load(Ordering::SeqCst) + 500 - now();
                if delay > 0 {
                    self.sleep(delay);
                }
                self.last_upload.store(now(), Ordering::SeqCst);
                Ok(())
            };
            let epoch = self.preview_epoch.load(Ordering::SeqCst);
            let mode = if upload { SyncMode::Upload } else { SyncMode::DownloadOnly };
            let synced = local.sync(peer, (&remote.0).into(), mode, &mut gate)?;
            let raced = {
                let mut previews = lock(&self.ephemeral_previews);
                // Deviation from Java: a preview set/cleared locally while this
                // fetch was in flight is newer than the fetched list, which would
                // otherwise resurrect a cleared preview until the next sync.
                let raced = self.preview_epoch.load(Ordering::SeqCst) != epoch;
                if !raced {
                    previews.retain(|k, _| !k.starts_with(&format!("{}:", name)));
                    for value in
                        json::map(json::get_or_default(&synced, "previews", &empty_map()))?.values()
                    {
                        let preview = json::map(value)?;
                        let key = format!("{}:{}", name, json::java_string(preview.get("session")));
                        if json::as_long(Some(json::get_or_default(
                            preview,
                            "expires",
                            &Value::from(0),
                        )))? > now_millis()
                        {
                            previews.insert(key, preview.clone());
                        }
                    }
                }
                raced
            };
            if raced {
                self.wake_fetch();
            }
            result.insert(name.clone(), Value::Object(synced.into()));
            let connection_changed = lock(&self.connected).insert(name.clone(), true) != Some(true);
            lock(&self.errors).remove(&name);
            let after = local.read()?;
            if revision(&after)? != before
                || connection_changed
                || leases_before != leases_of(&after)?
            {
                self.changed(false);
            }
            more |= local.needs_upload(&remote_id)?;
        }
        let mut c = lock(&self.changes);
        if more {
            c.upload_due = c.upload_due.min(now() + 500);
        }
        c.retry_at = 0;
        Ok(result)
    }

    fn subscribe(self: &Arc<Self>) {
        let mut backoff: i64 = 500;
        while self.running() {
            let Some(peer) = self.upstream() else {
                let mut c = lock(&self.changes);
                while lock(&self.upstream).is_none() && self.running() {
                    c = self.changes_cv.wait(c).unwrap_or_else(|e| e.into_inner());
                }
                continue;
            };
            let outcome = (|| -> Result<()> {
                let mut c = peer.open(ApiPath::Stream)?;
                c.set_read_timeout(45000);
                *lock(&self.stream) = Some(c.disconnector());
                let read = (|| -> Result<()> {
                    let code = c.response_code()?;
                    if code != HttpStatus::OK {
                        return Err(Error::io(format!("Notification stream HTTP {}", code)));
                    }
                    let mut reader = BufReader::new(c.input_stream()?);
                    let mut line = String::new();
                    while self.running() && self.upstream().is_some_and(|u| Arc::ptr_eq(&u, &peer))
                    {
                        line.clear();
                        let n = reader
                            .read_line(&mut line)
                            .map_err(|e| Error::io(notemaker_core::error::io_message(&e)))?;
                        if n == 0 {
                            break;
                        }
                        if line.starts_with("data:") {
                            backoff = 500;
                            self.wake_fetch();
                        }
                    }
                    Ok(())
                })();
                c.disconnect();
                *lock(&self.stream) = None;
                read?;
                if self.upstream().is_some_and(|u| Arc::ptr_eq(&u, &peer)) {
                    return Err(Error::io("Notification stream ended"));
                }
                Ok(())
            })();
            if outcome.is_err() {
                if !self.sleep(backoff) {
                    return;
                }
                backoff = (backoff * 2).min(30000);
            }
        }
    }

    fn ui_relay(&self, path: UiPath, body: Option<&Map>) -> Result<Map> {
        let peer = self.ui_peer();
        if let Some(peer) = &peer {
            match peer.request(path, body.map(|b| Value::Object(b.clone())).as_ref().map(Into::into)) {
                Ok(r) => return Ok(r.into()),
                Err(e) if e.kind == ErrorKind::Http => return Err(e),
                Err(e) if e.kind.is_io() => {
                    if path == UiPath::Command {
                        return Err(e);
                    }
                    // Keep reachable child sessions usable while the ancestor is offline.
                }
                Err(e) => return Err(e),
            }
        }
        match path {
            UiPath::Sessions => {
                let mut listing = self.ui_hub.list();
                if peer.is_some() {
                    listing.insert("scope".into(), Value::from("reachable"));
                    listing.insert("upstreamConnected".into(), Value::from(false));
                }
                Ok(listing.into())
            }
            UiPath::Heartbeat => self.ui_hub.heartbeat((body.ok_or_else(Error::null_pointer)?).into()).map(Into::into),
            UiPath::Command => self.ui_hub.command((body.ok_or_else(Error::null_pointer)?).into()).map(Into::into),
        }
    }

    fn ui_sessions_ordered(&self) -> Vec<(String, Arc<dyn UiSession>)> {
        let sessions = lock(&self.ui_sessions);
        hash_order(sessions.keys())
            .into_iter()
            .map(|k| (k.clone(), sessions[&k].clone()))
            .collect()
    }

    fn ui_loop(self: &Arc<Self>) {
        let mut sent_presence = false;
        while self.running() {
            let attempt = (|| -> Result<()> {
                let mut states = Vec::new();
                for (_, handler) in self.ui_sessions_ordered() {
                    if let Ok(state) = handler.request(&UiVerb::from("state"), &UiActionName::from(""), Map::new().into()) {
                        states.push(state);
                    }
                    // The Activity will unregister or recover.
                }
                let results: Vec<Value> = {
                    let r = lock(&self.ui_results);
                    hash_order(r.keys())
                        .into_iter()
                        .map(|k| Value::Object(r[&k].clone()))
                        .collect()
                };
                if !states.is_empty() || sent_presence || !results.is_empty() {
                    let present = !states.is_empty();
                    let heartbeat = object! {"proxyId" => self.id.clone(), "sessions" => states, "results" => results};
                    let reply = self.ui_relay(UiPath::Heartbeat, Some(&heartbeat))?;
                    sent_presence = present;
                    let empty = Value::Array(Vec::new());
                    for value in json::list(json::get_or_default(&reply, "acknowledged", &empty))? {
                        let key = json::java_string(Some(value));
                        lock(&self.ui_results).shift_remove(&key);
                        lock(&self.ui_in_flight).remove(&key);
                    }
                    for value in json::list(json::get_or_default(&reply, "commands", &empty))? {
                        let command = json::map(value)?.clone();
                        let key = json::string(&command, "requestId")?.to_string();
                        if !lock(&self.ui_in_flight).insert(key.clone()) {
                            continue;
                        }
                        let p = self.clone();
                        self.client(move || {
                            let mut result = object! {"requestId" => key.clone()};
                            let outcome = (|| -> Result<Value> {
                                let handler = lock(&p.ui_sessions)
                                    .get(json::string(&command, "session")?)
                                    .cloned();
                                let handler = handler.ok_or_else(|| {
                                    Error::illegal_argument("App session is closed")
                                })?;
                                let input = map_value(command.get("input"))?;
                                handler
                                    .request(
                                        &UiVerb::from(json::string(&command, "verb")?),
                                        &UiActionName::from(json::string(&command, "action")?),
                                        input.into(),
                                    )
                                    .map(|reply| reply.0)
                                    .map_err(ui_error)
                            })();
                            match outcome {
                                Ok(v) => result.insert("value".into(), v),
                                Err(e) => result.insert("error".into(), Value::from(e.to_string())),
                            };
                            lock(&p.ui_results).insert(key, result);
                            p.wake_ui();
                        });
                    }
                }
                Ok(())
            })();
            let _ = attempt;
            let c = lock(&self.changes);
            if self.running() {
                let _ = self.changes_cv.wait_timeout(c, Duration::from_millis(1000));
            }
        }
    }

    fn sync_command(&self, document: &str, target: Option<&str>) -> Result<Value> {
        if let Some(target) = target {
            self.configure(&DocumentName::from(document), Some(target))?;
        }
        require(self.upstream().is_some(), "No upstream configured")?;
        let (sender, receiver) = mpsc::channel();
        {
            let mut c = lock(&self.changes);
            c.requests.push(sender);
            c.upload_due = c.upload_due.min(now() + 500);
            self.changes_cv.notify_all();
        }
        let all = match receiver.recv_timeout(Duration::from_secs(35)) {
            Ok(r) => r?,
            Err(_) => return Err(Error::without_message(ErrorKind::IllegalState)),
        };
        Ok(all.get(document).cloned().unwrap_or(Value::Null))
    }

    /// `port()`: bound local port.
    pub fn port(&self) -> Port {
        self.local_port
    }

    /// `await()`: joins the `local-proxy` acceptor thread.
    pub fn await_(&self) {
        let mut done = lock(&self.acceptor_done);
        while !*done {
            done = self
                .acceptor_cv
                .wait(done)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// `close()`: idempotent; releases owned leases, stops and joins all
    /// threads, then releases `proxy.lock`.
    pub fn close(&self) -> Result<()> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        lock(&self.ui_sessions).clear();
        let docs: Vec<(String, Arc<Store>)> = {
            let d = lock(&self.documents);
            hash_order(d.keys())
                .into_iter()
                .map(|k| (k.clone(), d[&k].clone()))
                .collect()
        };
        for (doc, store) in docs {
            let _ = (|| -> Result<()> {
                self.release_owned(&store, &doc, None)?;
                let keys: Vec<String> = hash_order(lock(&self.ephemeral_previews).keys());
                for key in keys {
                    if key.starts_with(&format!("{}:", doc)) {
                        let preview = lock(&self.ephemeral_previews).get(&key).cloned();
                        if let Some(preview) = preview {
                            if preview.get("owner").and_then(Value::as_str)
                                == Some(self.id.as_str())
                            {
                                let removed = {
                                    let mut previews = self.previews_mut();
                                    if previews.get(&key) == Some(&preview) {
                                        previews.shift_remove(&key);
                                        true
                                    } else {
                                        false
                                    }
                                };
                                if removed {
                                    self.forward_preview_clear(
                                        &doc,
                                        &json::java_string(preview.get("session")),
                                        &json::java_string(preview.get("actor")),
                                    );
                                }
                            }
                        }
                    }
                }
                Ok(())
            })();
        }
        self.running.store(false, Ordering::SeqCst);
        // SAFETY: shutdown(2) on our own listening socket wakes a blocked accept.
        unsafe {
            use std::os::unix::io::AsRawFd;
            libc::shutdown(self.listener.as_raw_fd(), libc::SHUT_RDWR);
        }
        if let Some(active) = lock(&self.stream).as_ref() {
            active.disconnect();
        }
        if let (Some(peer), Some(thread)) = (self.upstream(), self.synchronizer.get()) {
            peer.cancel(*thread);
        }
        {
            let mut c = lock(&self.changes);
            self.changes_cv.notify_all();
            for request in c.requests.drain(..) {
                let _ = request.send(Err(Error::io("Proxy closed")));
            }
        }
        for socket in lock(&self.sockets).values() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        // Never release ownership while an old writer can still commit.
        let workers: Vec<_> = lock(&self.workers).drain(..).collect();
        for worker in workers {
            if worker.thread().id() != std::thread::current().id() {
                let _ = worker.join();
            }
        }
        {
            let mut count = lock(&self.clients);
            while *count > 0 {
                count = self
                    .clients_cv
                    .wait_timeout(count, Duration::from_secs(1))
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
                for socket in lock(&self.sockets).values() {
                    let _ = socket.shutdown(Shutdown::Both);
                }
            }
        }
        if let Some(held) = lock(&self.ownership).take() {
            held.release();
        }
        Ok(())
    }

    fn handle(self: &Arc<Self>, socket: TcpStream, key: u64) {
        if let Err(e) = self.handle_inner(&socket) {
            if self.running() {
                eprintln!("Proxy request: {}", e);
            }
        }
        let _ = socket.shutdown(Shutdown::Both);
        lock(&self.sockets).remove(&key);
    }

    fn handle_inner(self: &Arc<Self>, connection: &TcpStream) -> Result<()> {
        let io = |e: std::io::Error| Error::io(notemaker_core::error::io_message(&e));
        connection
            .set_read_timeout(Some(Duration::from_millis(45000)))
            .map_err(io)?;
        let mut input = BufReader::new(connection.try_clone().map_err(io)?);
        let request = line(&mut input)?;
        let mut authorization = String::new();
        let mut length: i32 = 0;
        let mut total = 0usize;
        loop {
            let header = line(&mut input)?;
            if header.is_empty() {
                break;
            }
            total += json::jlen(&header);
            require(total <= 16384, "Headers too large")?;
            let colon = header.find(':');
            require(colon.is_some_and(|c| c > 0), "Invalid header")?;
            let colon = colon.unwrap_or(0);
            let (key, value) = (
                &header[..colon],
                notemaker_core::jfmt::java_trim(&header[colon + 1..]),
            );
            if key.eq_ignore_ascii_case("Content-Length") {
                length = jutil::parse_int(value)?;
            }
            if key.eq_ignore_ascii_case("Authorization") {
                authorization = value.to_string();
            }
        }
        let mut status = 200;
        let response: Result<Option<Value>> = (|| -> Result<Option<Value>> {
            require(
                authorization == format!("Bearer {}", self.token),
                "Unauthorized proxy request",
            )?;
            let parts = split(&request, " ");
            let method = parts.first().cloned().unwrap_or_default();
            let target = parts.get(1).ok_or_else(|| {
                Error::new(
                    ErrorKind::IndexOutOfBounds,
                    format!("Index 1 out of bounds for length {}", parts.len()),
                )
            })?;
            let uri = Uri::parse(target)?;
            let q = uri.query()?;
            let path = uri.path()?;
            if method == "GET" && path == "/api/stream" {
                self.events(connection)?;
                return Ok(None);
            }
            let mut request_body: Option<Vec<u8>> = None;
            let mut body = Map::new();
            if method == "POST" {
                require(length > 0 && length <= 4_000_000, "Request exceeds 4 MB")?;
                let mut bytes = vec![0u8; length as usize];
                input.read_exact(&mut bytes).map_err(|e| {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        Error::io("java.io.EOFException")
                    } else {
                        io(e)
                    }
                })?;
                body = json::map(&json::parse(&String::from_utf8_lossy(&bytes))?)?.clone();
                request_body = Some(bytes);
            }
            if path == "/api/chats" || path.starts_with("/api/chats/") {
                self.forward_chat(connection, &method, &uri, request_body)?;
                return Ok(None);
            }
            let get = |k: &str, d: &str| q.get(k).cloned().unwrap_or_else(|| d.to_string());
            let response: Value = if method == "GET" && path == "/api/ui/sessions" {
                Value::Object(self.ui_relay(UiPath::Sessions, None)?)
            } else if method == "POST" && (path == "/api/ui/heartbeat" || path == "/api/ui/command")
            {
                let ui = if path == "/api/ui/heartbeat" {
                    UiPath::Heartbeat
                } else {
                    UiPath::Command
                };
                Value::Object(self.ui_relay(ui, Some(&body))?)
            } else if method == "GET" && path == "/api/info" {
                Value::Object(self.info(&get("trail", ""), q.contains_key("validate"))?)
            } else if method == "GET" && path == "/api/replica" {
                let doc = get("document", "notes");
                *lock(&self.active_document) = doc.clone();
                let fresh = !lock(&self.documents).contains_key(&doc);
                let r = self.store(&doc)?.replica(
                    ReplicaCursor(jutil::parse_int(&get("after", "0"))?),
                    ReplicaCursor(jutil::parse_int(&get("pendingAfter", "0"))?),
                )?;
                if fresh {
                    self.wake_fetch();
                }
                Value::Object(r.into())
            } else if method == "GET" && path == "/api/locks" {
                let mut state = self.store(&get("document", "notes"))?.read()?;
                Value::Object(self.lock_state(&mut state)?)
            } else if method == "POST" && path == "/api/locks" {
                self.post_locks(&body)?
            } else if method == "POST" && path == "/api/events" {
                let operations =
                    json::list(body.get("operations").unwrap_or(&Value::Null))?.clone();
                require(operations.len() <= 100, "Upload exceeds 100 actions")?;
                let document = json::string(&body, "document")?.to_string();
                *lock(&self.active_document) = document.clone();
                let local = self.store(&document)?;
                let before = revision(&local.read()?.map)?;
                let root = body.get("rootId").filter(|v| !v.is_null()).cloned();
                let response = {
                    let _sync = self.this.enter();
                    {
                        let settings = lock(&self.settings);
                        require(
                            root.is_none()
                                || !settings.contains_key("rootId")
                                || settings.get("rootId") == root.as_ref(),
                            "Server database differs from this replica",
                        )?;
                    }
                    let response = local.accept((&operations).into(), root.as_ref().map(Into::into))?;
                    if let Some(root) = &root {
                        let changed = {
                            let mut settings = lock(&self.settings);
                            if settings.get("rootId") != Some(root) {
                                settings.insert("rootId".into(), root.clone());
                                true
                            } else {
                                false
                            }
                        };
                        if changed {
                            self.save_settings()?;
                        }
                    }
                    response
                };
                if revision(&local.read()?.map)? != before {
                    self.changed(true);
                }
                Value::Object(response.into())
            } else if method == "POST" && path == "/command" {
                Value::Object(object! {"result" => self.execute((&body).into())?})
            } else {
                return Err(Error::illegal_argument("Unsupported endpoint"));
            };
            Ok(Some(response))
        })();
        let response = match response {
            Ok(None) => return Ok(()),
            Ok(Some(v)) => v,
            Err(e) => {
                status = 400;
                Value::Object(object! {"error" => e.to_string()})
            }
        };
        let data = json::write(&response).into_bytes();
        let mut out = connection;
        let head = format!("HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", status, if status == 200 { "OK" } else { "Bad Request" }, data.len());
        out.write_all(head.as_bytes())
            .and_then(|_| out.write_all(&data))
            .and_then(|_| out.flush())
            .map_err(io)?;
        Ok(())
    }

    fn post_locks(&self, body: &Map) -> Result<Value> {
        let peer = self.upstream();
        let mut decision: Option<Map> = None;
        if let Some(peer) = &peer {
            // Keep arbitration at the nearest reachable proxy.
            decision = peer
                .request(ApiPath::LockUpdate, Some((&Value::Object(body.clone())).into()))
                .ok()
                .map(|reply| reply.0);
        }
        let mut accepted = body.clone();
        if let Some(decision) = decision.as_ref().filter(|d| d.contains_key("selected")) {
            accepted = json::map(&json::copy(&Value::Object(body.clone())))?.clone();
            let mut filtered = Map::new();
            let leases = json::map(json::get_or_default(body, "leases", &empty_map()))?.clone();
            for key in json::list(decision.get("selected").unwrap_or(&Value::Null))? {
                let k = json::java_string(Some(key));
                if let Some(lease) = key.as_str().and_then(|k| leases.get(k)) {
                    filtered.insert(k, lease.clone());
                }
            }
            accepted.insert("leases".into(), Value::Object(filtered));
            accepted.insert(
                "remove".into(),
                json::get_or_default(body, "remove", &Value::Array(Vec::new())).clone(),
            );
        }
        let doc = json::string(body, "document")?.to_string();
        let lock_store = self.store(&doc)?;
        let before_state = lock_store.read()?;
        let leases_before = leases_of(&before_state)?;
        let previews_before = self.preview_snapshot(&doc, &before_state)?;
        let local = apply_remote_leases(&lock_store, &accepted)?;
        let empty = Value::Array(Vec::new());
        for session in json::list(json::get_or_default(body, "clearPreviews", &empty))? {
            self.previews_mut().shift_remove(&format!(
                "{}:{}",
                doc,
                json::java_string(Some(session))
            ));
        }
        let live_leases = leases_of(&lock_store.read()?.map)?;
        for value in json::map(json::get_or_default(body, "previews", &empty_map()))?.values() {
            if let Some(normalized) = normalize_preview(json::map(value)?, &live_leases)? {
                let key = format!("{}:{}", doc, json::java_string(normalized.get("session")));
                self.previews_mut().insert(key, normalized);
            }
        }
        let response = match decision {
            None => Value::Object(local),
            Some(d) => Value::Object(d),
        };
        let after_state = lock_store.read()?;
        let previews_after = self.preview_snapshot(&doc, &after_state)?;
        if leases_before != leases_of(&after_state)? || previews_before != previews_after {
            self.changed(false);
        }
        Ok(response)
    }

    /// Chat requests belong to the primary server, but follow the same configured proxy chain as notes.
    fn forward_chat(
        &self,
        socket: &TcpStream,
        method: &str,
        uri: &Uri,
        body: Option<Vec<u8>>,
    ) -> Result<()> {
        let peer = self.upstream().ok_or_else(|| {
            Error::illegal_argument(
                "No upstream proxy configured for chat. All app traffic must go through the proxy.",
            )
        })?;
        let io = |e: std::io::Error| Error::io(notemaker_core::error::io_message(&e));
        let Some(path) = chat_path(method, uri)? else {
            // The primary server has no such chat route.
            let data = json::write(&Value::Object(object! {"error" => "Not found"}));
            let head = format!(
                "HTTP/1.1 {} Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                HttpStatus::NOT_FOUND,
                data.len()
            );
            let mut out = socket;
            return out
                .write_all(head.as_bytes())
                .and_then(|_| out.write_all(data.as_bytes()))
                .and_then(|_| out.flush())
                .map_err(io);
        };
        let streaming = matches!(path, ChatPath::Stream { .. });
        let path = EndpointPath::from(path);
        let mut c = peer.open(&path)?;
        c.set_request_method(path.method());
        c.set_read_timeout(if streaming { 45000 } else { 10000 });
        let outcome = (|| -> Result<()> {
            if let Some(body) = body {
                c.set_request_property(HeaderName::ContentType, HeaderValue::json());
                c.write_body(body);
            }
            let status = c.response_code()?;
            let source = if status.is_error() {
                c.error_stream()?
            } else {
                Some(c.input_stream()?)
            };
            let mut out = socket;
            let content_type = c
                .header_field(HeaderName::ContentType)
                .cloned()
                .unwrap_or_else(HeaderValue::json);
            let head = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                status,
                if status.is_error() { "Upstream Error" } else { "OK" },
                content_type
            );
            out.write_all(head.as_bytes())
                .and_then(|_| out.flush())
                .map_err(io)?;
            if let Some(mut source) = source {
                let mut buffer = [0u8; 8192];
                loop {
                    let n = source.read(&mut buffer).map_err(io)?;
                    if n == 0 {
                        break;
                    }
                    out.write_all(&buffer[..n])
                        .and_then(|_| out.flush())
                        .map_err(io)?;
                }
            }
            Ok(())
        })();
        c.disconnect();
        outcome
    }

    fn events(&self, socket: &TcpStream) -> Result<()> {
        let io = |e: std::io::Error| Error::io(notemaker_core::error::io_message(&e));
        let mut out = socket;
        out.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n").map_err(io)?;
        let mut seen: i64 = -1;
        while self.running() {
            let current = {
                let mut c = lock(&self.changes);
                if c.generation == seen {
                    c = self
                        .changes_cv
                        .wait_timeout(c, Duration::from_millis(15000))
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
                c.generation
            };
            let text = if current == seen {
                ": heartbeat\n\n".to_string()
            } else {
                format!("data: {}\n\n", current)
            };
            out.write_all(text.as_bytes())
                .and_then(|_| out.flush())
                .map_err(io)?;
            seen = current;
        }
        Ok(())
    }

    /// `execute(body)`: one `/command` request (`{document, actor, args, input}`).
    pub fn execute(&self, body: &CommandRequest) -> Result<CommandReply> {
        self.execute_value(body).map(CommandReply)
    }

    fn execute_value(&self, body: &Map) -> Result<Value> {
        require(self.running(), "Proxy is closed")?;
        let mut args: Vec<String> = Vec::new();
        for value in json::list(body.get("args").unwrap_or(&Value::Null))? {
            match value {
                Value::String(s) if !s.starts_with("--") => args.push(s.clone()),
                _ => {
                    return Err(Error::illegal_argument(
                        "Proxy options belong in document/actor fields",
                    ))
                }
            }
        }
        require(!args.is_empty(), "Missing command")?;
        let string_field = |key: &str, default: &str| -> Result<String> {
            match body.get(key) {
                None => Ok(default.to_string()),
                Some(Value::String(s)) => Ok(s.clone()),
                Some(Value::Null) => Err(Error::null_pointer()),
                Some(_) => Err(Error::class_cast(format!("{} is not a String", key))),
            }
        };
        let doc = string_field("document", "notes")?;
        let actor = string_field("actor", "cli")?;
        let actor_id = ActorId::from(&actor);
        let cmd = args[0].clone();
        let input = || map_value(body.get("input"));
        if cmd == "ui" {
            require(
                args.len() >= 2,
                "ui sessions|state SESSION|actions SESSION|action SESSION NAME -",
            )?;
            let verb = args[1].as_str();
            if verb == "sessions" {
                require(args.len() == 2, "ui sessions")?;
                let mut result = self.ui_relay(UiPath::Sessions, None)?;
                let mut sessions =
                    json::list(result.get("sessions").unwrap_or(&Value::Null))?.clone();
                for (session, handler) in self.ui_sessions_ordered() {
                    if !sessions.iter().any(|item| {
                        item.get("session").and_then(Value::as_str) == Some(session.as_str())
                    }) {
                        match handler.request(&UiVerb::from("state"), &UiActionName::from(""), Map::new().into()) {
                            Ok(state) => sessions.push(state.into()),
                            Err(unavailable) => sessions.push(Value::Object(object! {"session" => session.clone(), "error" => ui_error(unavailable).to_string()})),
                        }
                    }
                }
                result.insert("sessions".into(), Value::Array(sessions));
                return Ok((Value::Object(result)).into());
            }
            require(args.len() >= 3, "Session ID is required")?;
            let session = args[2].clone();
            let handler = lock(&self.ui_sessions).get(&session).cloned();
            if verb == "state" || verb == "actions" {
                require_msg(args.len() == 3, format!("ui {} SESSION", verb))?;
                return match handler {
                    Some(h) => h.request(&UiVerb::from(verb), &UiActionName::from(""), Map::new().into()).map(|reply| reply.0).map_err(ui_error),
                    None => Ok((self.ui_relay(UiPath::Command, Some(&object! {"session" => session, "verb" => verb, "action" => "", "input" => empty_map()}))?.get("result").cloned().unwrap_or(Value::Null)).into()),
                };
            }
            require(
                verb == "action" && args.len() == 5 && args[4] == "-",
                "ui action SESSION NAME FILE|-",
            )?;
            return match handler {
                Some(h) => h.request(&UiVerb::from("action"), &UiActionName::from(&args[3]), (input()?).into()).map(|reply| reply.0).map_err(ui_error),
                None => Ok(self
                    .ui_relay(UiPath::Command, Some(&object! {"session" => session, "verb" => "action", "action" => args[3].clone(), "input" => Value::Object(input()?)}))?
                    .get("result")
                    .cloned()
                    .unwrap_or(Value::Null)),
            };
        }
        *lock(&self.active_document) = doc.clone();
        let local = self.store(&doc)?;
        let finish_snapshot = |state: &mut State| -> Result<Map> {
            Ok((self.finish_snapshot(&doc, state)?.into_map()).into())
        };
        if cmd == "snapshot" {
            require(args.len() == 1, "snapshot takes no arguments")?;
            let mut state = local.read()?;
            return Ok((Value::Object(finish_snapshot(&mut state)?)).into());
        }
        if cmd == "append" && args.len() == 3 && args[1] == "snapshot" && args[2] == "-" {
            let mut state = local.append(&(input()?).into(), &actor_id)?;
            let operation = last_pending_id(&state)?;
            let snapshot = finish_snapshot(&mut state)?;
            self.changed(true);
            return Ok(Value::Object(
                object! {"operation" => operation, "snapshot" => snapshot},
            ));
        }
        if cmd == "append" && args == ["append", "settings", "-"] {
            let request = input()?;
            let operation = Operation(map_value(request.get("operation"))?);
            // Java validated the operation before the preferences.
            notemaker_core::Model::validate(&operation)?;
            let settings = EditorPreferences::try_from(&map_value(request.get("settings"))?)?;
            let state = local.append_with_settings(&operation, &actor_id, Some(&settings))?;
            let operation = last_pending_id(&state)?;
            self.changed(true);
            return Ok(Value::Object(
                object! {"operation" => operation, "saved" => true},
            ));
        }
        if (cmd == "undo" || cmd == "redo") && args.len() == 2 && args[1] == "snapshot" {
            let direction = if cmd == "redo" { HistoryDirection::Redo } else { HistoryDirection::Undo };
            let WithState { mut result, mut state } = local.history_with_state(&actor_id, direction)?;
            result.insert(
                "snapshot".into(),
                Value::Object(finish_snapshot(&mut state)?),
            );
            self.changed(true);
            return Ok((Value::Object(result.into())).into());
        }
        if cmd == "object"
            && args.len() == 4
            && args[1] == "put"
            && args[2] == "snapshot"
            && args[3] == "-"
        {
            let mut drawing = actions::drawing(&(input()?).into(), &LayerId::base())?;
            drawing.insert("kind".into(), Value::from("object"));
            drawing.shift_remove("id");
            let WithState { mut result, mut state } = local.commit_with_state(&actor_id, &mut |model| {
                if let Some(id) = json::key_of(drawing.get("object")) {
                    if model.objects.contains_key(id) {
                        LayerEditor::with_filter(
                            model,
                            &LayerId::from(json::string(&drawing, "layer")?),
                            PageNumber(int_of(drawing.get("page"))?),
                            ItemFilter::IncludeInactive,
                        )?
                        .resolve(&EntityKind::object(), &EntityId::from(json::string(&drawing, "object")?))?;
                    }
                }
                Ok((vec![Value::Object(drawing.clone().into())]).into())
            })?;
            result.insert(
                "snapshot".into(),
                Value::Object(finish_snapshot(&mut state)?),
            );
            self.changed(true);
            return Ok((Value::Object(result.into())).into());
        }
        if cmd == "configure" {
            require(
                args.len() == 2,
                "configure UPSTREAM_URL_OR_DESCRIPTOR (empty to disconnect)",
            )?;
            self.configure(&DocumentName::from(&doc), Some(&args[1]))?;
            return Ok((Value::Object(object! {"configured" => true})).into());
        }
        if cmd == "sync" {
            require(args.len() <= 2, "sync [UPSTREAM_URL_OR_DESCRIPTOR]")?;
            return self.sync_command(&doc, args.get(1).map(String::as_str));
        }
        if cmd == "selection" && args.len() == 3 && args[1] == "select" {
            return Ok(Value::Object(self.acquire_selection(
                &local,
                &doc,
                &actor,
                &input()?,
            )?));
        }
        if cmd == "selection" && args.len() == 3 && args[1] == "heartbeat" {
            return Ok(Value::Object(
                self.heartbeat_selection(&local, &doc, &actor, &args[2])?,
            ));
        }
        if cmd == "selection" && args.len() == 2 && args[1] == "disconnect" {
            return Ok(Value::Object(self.release_owned(
                &local,
                &doc,
                Some(&actor),
            )?));
        }
        if cmd == "transform" && args.len() == 3 && args[1] == "cancel" {
            let session = args[2].clone();
            self.clear_preview(&doc, &session, &actor);
            return Ok(Value::Object(
                object! {"cancelled" => true, "session" => session},
            ));
        }
        if cmd == "transform" && args.len() == 3 && args[1] == "preview" && args[2] == "-" {
            let request = input()?;
            let transform = map_value(request.get("transform"))?;
            let entities = json::list(request.get("entities").unwrap_or(&Value::Null))?.clone();
            return Ok(Value::Object(self.preview(
                &doc,
                &actor,
                json::string(&request, "session")?,
                &entities,
                &transform,
            )?));
        }
        const ALLOWED: [&str; 26] = [
            "document",
            "inspect",
            "snapshot",
            "append",
            "settings",
            "status",
            "log",
            "checkpoint",
            "diagram",
            "svg",
            "image",
            "gif",
            "erase",
            "fill",
            "template",
            "shape",
            "apply",
            "layer",
            "object",
            "stroke",
            "undo",
            "redo",
            "selection",
            "transform",
            "conflicts",
            "conflict",
        ];
        require(ALLOWED.contains(&cmd.as_str()), "Unsupported proxy command")?;
        let mut local_args: Vec<String> = vec![
            "--state".into(),
            absolute(&self.directory),
            "--document".into(),
            doc.clone(),
            "--actor".into(),
            actor.clone(),
        ];
        local_args.extend(args.iter().cloned());
        cli::PROXY_INPUT.with(|p| *p.borrow_mut() = body.get("input").cloned());
        cli::IN_PROXY.with(|p| p.set(true));
        let setup = (|| -> Result<(i64, Map)> {
            let before_state = local.read()?;
            Ok((revision(&before_state)?, leases_of(&before_state)?))
        })();
        let (before, previous_leases) = match setup {
            Ok(v) => v,
            Err(e) => {
                cli::PROXY_INPUT.with(|p| *p.borrow_mut() = None);
                cli::IN_PROXY.with(|p| p.set(false));
                return Err(e);
            }
        };
        let result = (|| -> Result<Value> {
            let mut result = cli::run(&CommandArgs(local_args.clone()))?.unwrap_or(Value::Null.into());
            if cmd == "transform" && args.len() > 1 && args[1] == "commit" {
                let request = input()?;
                self.clear_preview(&doc, json::string(&request, "session")?, &actor);
            }
            if cmd == "status" {
                let connected = lock(&self.connected).get(&doc) == Some(&true);
                let error = lock(&self.errors).get(&doc).cloned();
                let m = json::map_mut(&mut result)?;
                m.insert("connected".into(), Value::from(connected));
                m.insert(
                    "upstreamError".into(),
                    error.map(Value::from).unwrap_or(Value::Null),
                );
                m.insert("proxyId".into(), Value::from(self.id.clone()));
            }
            Ok(result.into())
        })();
        cli::PROXY_INPUT.with(|p| *p.borrow_mut() = None);
        cli::IN_PROXY.with(|p| p.set(false));
        if cmd == "selection" {
            let _ = self.forward_selection_change(&local, &doc, &actor, &args, &previous_leases, result.is_ok());
        }
        let action = uploads_changes(&cmd, &args);
        let after_state = local.read()?;
        if revision(&after_state)? != before {
            self.changed(action);
        }
        result
    }

    fn forward_selection_change(
        &self,
        local: &Store,
        doc: &str,
        actor: &str,
        args: &[String],
        previous_leases: &Map,
        succeeded: bool,
    ) -> Result<()> {
        let mut session = if args.len() > 1 {
            args[args.len() - 1].clone()
        } else {
            String::new()
        };
        let mut cleared: Vec<String> = Vec::new();
        let add = |s: String, cleared: &mut Vec<String>| {
            if !cleared.contains(&s) {
                cleared.push(s);
            }
        };
        if args.len() > 1 && args[1] == "disconnect" {
            session = String::new();
            for raw in previous_leases.values() {
                let lease = json::map(raw)?;
                if lease.get("actor").and_then(Value::as_str) == Some(actor) {
                    add(json::java_string(lease.get("session")), &mut cleared);
                }
            }
            let previews: Vec<(String, Map)> = lock(&self.ephemeral_previews)
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (key, preview) in previews {
                if key.starts_with(&format!("{}:", doc))
                    && preview.get("actor").and_then(Value::as_str) == Some(actor)
                {
                    add(json::java_string(preview.get("session")), &mut cleared);
                }
            }
        } else if args.len() > 1 && args[1] == "deselect" {
            add(session.clone(), &mut cleared);
        }
        for old in hash_order(cleared.iter()) {
            self.clear_preview(doc, &old, actor);
        }
        // A successful deselect/disconnect released these leases even if the
        // sync thread has since restored them from the upstream's stale copy
        // (it fetched before this removal was forwarded). Drop restored copies
        // again and forward the removal; otherwise the upstream keeps the old
        // session's lease and this actor's next selection is skipped.
        let released = succeeded && args.len() > 1 && (args[1] == "deselect" || args[1] == "disconnect");
        let same_holder = |a: &Map, b: &Value| json::map(b).is_ok_and(|b| a.get("session") == b.get("session") && a.get("actor") == b.get("actor"));
        let mut owned = Vec::new();
        for (key, value) in previous_leases {
            let lease = json::map(value)?;
            if (session.is_empty() || lease.get("session").and_then(Value::as_str) == Some(session.as_str()))
                && lease.get("actor").and_then(Value::as_str) == Some(actor)
            {
                owned.push((key.clone(), lease.clone()));
            }
        }
        if released {
            local.edit_transient(&mut |s| {
                let mut leases = leases_of(s)?;
                for (key, lease) in &owned {
                    if leases.get(key).is_some_and(|now| same_holder(lease, now)) {
                        leases.shift_remove(key);
                    }
                }
                s.insert("leases".into(), Value::Object(leases));
                Ok(())
            })?;
        }
        let current = leases_of(&local.read()?.map)?;
        let removed: Vec<Value> = owned.iter().filter(|(key, _)| !current.contains_key(key)).map(|(key, _)| Value::from(key.clone())).collect();
        let mut changed_leases = Map::new();
        for (key, value) in &current {
            if previous_leases.get(key) != Some(value) {
                changed_leases.insert(key.clone(), value.clone());
            }
        }
        self.forward_locks(doc, &changed_leases, &removed, &session, actor);
        if previous_leases != &current {
            self.changed(false);
        }
        Ok(())
    }

    /// Static `LocalProxy.request(descriptor, document, actor, args, input)`:
    /// POST `/command` to a proxy and return its `result` (`Value::Null` for
    /// Java `null`). `input` `None` sends JSON `null`.
    pub fn request(
        descriptor: &EndpointDescriptor,
        document: &DocumentName,
        actor: &ActorId,
        args: &CommandArgs,
        input: Option<&CommandInput>,
    ) -> Result<CommandReply> {
        let (document, actor): (&str, &str) = (document, actor);
        let body = object! {"document" => document, "actor" => actor, "args" => str_list(args), "input" => input.map(|i| i.0.clone()).unwrap_or(Value::Null)};
        Ok(CommandReply(
            Endpoint::new(descriptor.clone())
                .request(EndpointPath::Command, Some((&Value::Object(body)).into()))?
                .get("result")
                .cloned()
                .unwrap_or(Value::Null),
        ))
    }

    fn lock_state(&self, state: &mut State) -> Result<Map> {
        collaboration::expire(state, EpochMillis(now_millis()))?;
        let document = json::string(state, "document")?.to_string();
        Ok(
            object! {"leases" => Value::Object(leases_of(state)?), "previews" => Value::Object(self.preview_snapshot(&document, state)?)},
        )
    }

    /// The `snapshot` reply for `state` (Java's `Store.Snapshot`, carrying
    /// the materialized model).
    /// Inside [`LocalProxy::execute_with_snapshot`] the reply's snapshot is
    /// also kept as a [`State`] for the caller and leaves out
    /// `effectiveModel` (see `cli::snapshot_state_in_process`).
    fn finish_snapshot(&self, doc: &str, state: &mut State) -> Result<State> {
        let now = now_millis();
        if collaboration::expire_changes(state, EpochMillis(now))? {
            collaboration::expire(state, EpochMillis(now))?;
        }
        let captured = CAPTURED_SNAPSHOT.with(|c| c.borrow().is_some());
        let mut snapshot = if captured { cli::snapshot_state_in_process(state)? } else { cli::snapshot_state(state)? };
        snapshot.insert("leases".into(), Value::Object(leases_of(state)?));
        snapshot.insert(
            "previews".into(),
            Value::Object(self.preview_snapshot(doc, state)?),
        );
        if captured {
            CAPTURED_SNAPSHOT.with(|c| *c.borrow_mut() = Some(Some(snapshot.clone())));
        }
        Ok(snapshot)
    }

    /// In-process `execute(body)` that also returns the reply's snapshot as
    /// a [`State`] carrying its model, like the `Store.Snapshot` Java's
    /// embedded proxy put in the reply. The reply's `snapshot` map then has
    /// no `effectiveModel`: callers use the returned state instead, which
    /// spares the deep copy into JSON and the re-parse. `None` when the
    /// command made no snapshot.
    pub fn execute_with_snapshot(&self, body: &CommandRequest) -> Result<ExecutedCommand> {
        let previous = CAPTURED_SNAPSHOT.with(|c| c.borrow_mut().replace(None));
        let result = self.execute(body);
        let snapshot = CAPTURED_SNAPSHOT.with(|c| std::mem::replace(&mut *c.borrow_mut(), previous)).flatten();
        Ok(ExecutedCommand { reply: result?, snapshot })
    }

    /// In-process `execute({document, actor, args: ["snapshot"]})`. Java's
    /// embedded proxy returned the `Store.Snapshot` object itself, so the app
    /// kept its model without parsing `effectiveModel`; this returns the same
    /// reply as a [`State`] that carries the model. Unlike the JSON reply it
    /// has no `effectiveModel` key (see `cli::snapshot_state_in_process`).
    pub fn snapshot_state(&self, document: &DocumentName) -> Result<State> {
        let document: &str = document;
        require(self.running(), "Proxy is closed")?;
        *lock(&self.active_document) = document.to_string();
        let local = self.store(document)?;
        let mut state = local.read()?;
        let now = now_millis();
        if collaboration::expire_changes(&state, EpochMillis(now))? {
            return self.finish_snapshot(document, &mut state);
        }
        // Same reply as `finish_snapshot` when nothing expires. The editor
        // reads the same revision twice per edit (after the write and on the
        // change notification); reuse the shared reply instead of rebuilding
        // `effectiveModel`.
        let previews = self.preview_snapshot(document, &state)?;
        let mut cache = lock(&self.snapshot_cache);
        if let Some(c) = cache.as_ref() {
            if c.document == document && std::ptr::eq(c.base.as_ptr(), Arc::as_ptr(&state.map)) && c.previews == previews {
                return Ok(c.reply.clone());
            }
        }
        let mut reply = cli::snapshot_state_in_process(&state)?;
        reply.insert("leases".into(), Value::Object(leases_of(&state)?));
        reply.insert("previews".into(), Value::Object(previews.clone()));
        *cache = Some(SnapshotCache { document: document.to_string(), base: Arc::downgrade(&state.map), previews, reply: reply.clone() });
        Ok(reply)
    }

    fn preview_snapshot(&self, document: &str, state: &Map) -> Result<Map> {
        let mut out = Map::new();
        let leases = leases_of(state)?;
        let prefix = format!("{}:", document);
        let mut previews = lock(&self.ephemeral_previews);
        for key in hash_order(previews.keys()) {
            let Some(preview) = previews.get(&key).cloned() else {
                continue;
            };
            let normalized = if key.starts_with(&prefix) {
                normalize_preview(&preview, &leases)?
            } else {
                None
            };
            match normalized {
                None => {
                    if key.starts_with(&prefix) && previews.get(&key) == Some(&preview) {
                        previews.shift_remove(&key);
                    }
                }
                Some(normalized) => {
                    previews.insert(key.clone(), normalized.clone());
                    out.insert(
                        json::java_string(normalized.get("session")),
                        Value::Object(normalized),
                    );
                }
            }
        }
        Ok(out)
    }

    fn clear_preview(&self, document: &str, session: &str, actor: &str) -> bool {
        let key = format!("{}:{}", document, session);
        let removed = {
            let mut previews = self.previews_mut();
            match previews.get(&key) {
                None => return false,
                Some(p) if p.get("actor").and_then(Value::as_str) != Some(actor) => return false,
                Some(_) => previews.shift_remove(&key).is_some(),
            }
        };
        if removed {
            self.forward_preview_clear(document, session, actor);
            self.changed(false);
        }
        removed
    }

    /// Static `select(store, actor, request)`.
    pub fn select(store: &Store, actor: &ActorId, request: &SelectionRequest) -> Result<SelectionReply> {
        let actor: &str = actor;
        let scope = selection_scope(store, request)?;
        select_validated(store, actor, request, &scope).map(Into::into)
    }

    fn acquire_selection(
        &self,
        store: &Store,
        document: &str,
        actor: &str,
        request: &Map,
    ) -> Result<Map> {
        let scope = if self.upstream().is_none() {
            requested_selection_scope(request)?
        } else {
            selection_scope(store, request)?
        };
        let session = json::java_string(Some(
            &request
                .get("session")
                .cloned()
                .unwrap_or_else(|| Value::from(String::from(notemaker_core::ids::Uuid::random()))),
        ));
        let page = request.get("page").cloned().unwrap_or(Value::from(0));
        let entities = json::list(request.get("entities").unwrap_or(&Value::Null))?.clone();
        let mut candidates = Map::new();
        let expires = now_millis() + LEASE_MILLIS;
        for value in &entities {
            let e = json::map(value)?;
            let key = collaboration::key(
                &EntityKind::from(json::string(e, "kind")?),
                &EntityId::from(json::string(e, "id")?),
                Some((&page).into()),
            );
            candidates.insert(
                String::from(key),
                Value::Object(object! {"layer" => scope.layer.clone(), "session" => session.clone(), "actor" => actor, "owner" => self.id.clone(), "page" => page.clone(), "expires" => expires, "created" => now_millis()}),
            );
        }
        let peer = self.upstream();
        let mut decision: Option<Map> = None;
        if let Some(peer) = &peer {
            // Offline selection remains local until the next sync attempt.
            decision = peer
                .request(ApiPath::LockUpdate, Some((&Value::Object(object! {"document" => document, "session" => session.clone(), "actor" => actor, "leases" => Value::Object(candidates.clone()), "remove" => Value::Array(Vec::new())})).into()))
                .ok()
                .map(|reply| reply.0);
        }
        let mut accepted: Vec<Value> = Vec::new();
        let mut skipped: Vec<Value> = Vec::new();
        let empty = Value::Array(Vec::new());
        let keys: HashSet<String> = match &decision {
            None => candidates.keys().cloned().collect(),
            Some(d) => json::list(json::get_or_default(d, "selected", &empty))?
                .iter()
                .map(|k| json::java_string(Some(k)))
                .collect(),
        };
        let mut filtered = Vec::new();
        for value in &entities {
            let e = json::map(value)?;
            let key = collaboration::key(
                &EntityKind::from(json::string(e, "kind")?),
                &EntityId::from(json::string(e, "id")?),
                Some((&page).into()),
            );
            if keys.contains(&*key) {
                filtered.push(value.clone());
            } else {
                let mut remote: Option<Map> = None;
                if let Some(d) = &decision {
                    for item in json::list(json::get_or_default(d, "skipped", &empty))? {
                        if item.get("key").and_then(Value::as_str) == Some(&*key) {
                            remote = Some(json::map(item)?.clone());
                        }
                    }
                }
                let by = match &remote {
                    None => Value::from("another session"),
                    Some(r) => r.get("actor").cloned().unwrap_or(Value::Null),
                };
                skipped.push(Value::Object(object! {"kind" => e.get("kind").cloned().unwrap_or(Value::Null), "id" => e.get("id").cloned().unwrap_or(Value::Null), "page" => page.clone(), "actor" => by}));
            }
        }
        if decision.is_some() {
            let peer = peer.as_ref().expect("decision implies a peer");
            let authoritative = peer
                .request(
                    ApiPath::Locks {
                        document: DocumentName::from(document),
                    },
                    None,
                )
                .ok()
                .and_then(|r| {
                    json::map(json::get_or_default(&r, "leases", &empty_map()))
                        .ok()
                        .cloned()
                })
                .unwrap_or_default();
            let mut selected_root = Map::new();
            for key in candidates.keys() {
                if let Some(v) = authoritative.get(key) {
                    selected_root.insert(key.clone(), v.clone());
                }
            }
            store.edit_transient(&mut |s| {
                let mut local = leases_of(s)?;
                for key in candidates.keys() {
                    local.shift_remove(key);
                }
                s.insert("leases".into(), Value::Object(local));
                Ok(())
            })?;
            if !selected_root.is_empty() {
                apply_remote_leases(
                    store,
                    &object! {"document" => document, "actor" => actor, "session" => session.clone(), "leases" => Value::Object(selected_root), "remove" => Value::Array(Vec::new())},
                )?;
            }
        }
        if !filtered.is_empty() {
            let local = select_validated(
                store,
                actor,
                &object! {"layer" => scope.layer.clone(), "session" => session.clone(), "page" => page.clone(), "entities" => filtered, "owner" => self.id.clone()},
                &scope,
            )?;
            accepted.extend(json::list_at(&local, "selected")?.iter().cloned());
            skipped.extend(json::list_at(&local, "skipped")?.iter().cloned());
        }
        if !accepted.is_empty() {
            self.changed(false);
        }
        Ok(
            object! {"session" => session, "selected" => accepted, "skipped" => skipped, "expires" => expires},
        )
    }

    /// Static `assertSession(store, actor, session, entities)`.
    pub fn assert_session(
        store: &Store,
        actor: &ActorId,
        session: &SessionId,
        entities: &EntityRefs,
    ) -> Result<()> {
        let mut state = store.read()?;
        let model = store::model(&state)?;
        collaboration::require_selection(&mut state, &model, actor, session, entities)
    }

    /// Static `selected(store, actor, session)`: `[{page, kind, id}]`.
    pub fn selected(store: &Store, actor: &ActorId, session: &SessionId) -> Result<EntityRefList> {
        let (actor, session): (&str, &str) = (actor, session);
        let mut state = store.read()?;
        collaboration::expire(&mut state, EpochMillis(now_millis()))?;
        let mut entities = Vec::new();
        for (key, value) in &leases_of(&state)? {
            let lease = json::map(value)?;
            if lease.get("session").and_then(Value::as_str) == Some(session) {
                require(
                    lease.get("actor").and_then(Value::as_str) == Some(actor),
                    "Session belongs to another actor",
                )?;
                let bits: Vec<&str> = key.splitn(3, ':').collect();
                let page = jutil::parse_int(bits[0])?;
                let kind = bits.get(1).ok_or_else(|| {
                    Error::new(
                        ErrorKind::IndexOutOfBounds,
                        "Index 1 out of bounds for length 1",
                    )
                })?;
                let id = bits.get(2).ok_or_else(|| {
                    Error::new(
                        ErrorKind::IndexOutOfBounds,
                        "Index 2 out of bounds for length 2",
                    )
                })?;
                entities.push(Value::Object(
                    object! {"page" => page, "kind" => *kind, "id" => *id},
                ));
            }
        }
        require(
            !entities.is_empty(),
            "Selection session has no active entities",
        )?;
        Ok(entities.into())
    }

    /// Static `release(store, actor, session, heartbeat)`.
    pub fn release(store: &Store, actor: &ActorId, session: &SessionId, update: LeaseUpdate) -> Result<ReleaseReply> {
        let (actor, session): (&str, &str) = (actor, session);
        let heartbeat = update == LeaseUpdate::Renew;
        let now = now_millis();
        let mut count = 0;
        store.edit_transient(&mut |s| {
            collaboration::expire(s, EpochMillis(now))?;
            let mut leases = leases_of(s)?;
            for key in leases.keys().cloned().collect::<Vec<_>>() {
                let mut lease = json::map(&leases[&key])?.clone();
                if lease.get("session").and_then(Value::as_str) == Some(session) {
                    require(
                        lease.get("actor").and_then(Value::as_str) == Some(actor),
                        "Session belongs to another actor",
                    )?;
                    if heartbeat {
                        lease.insert("expires".into(), Value::from(now + LEASE_MILLIS));
                        leases.insert(key, Value::Object(lease));
                    } else {
                        leases.shift_remove(&key);
                    }
                    count += 1;
                }
            }
            let active =
                json::map(json::get_or_default(s, "activeSessions", &empty_map()))?.clone();
            if !heartbeat && active.get(actor).and_then(Value::as_str) == Some(session) {
                if let Some(Value::Object(a)) = s.get_mut("activeSessions") {
                    a.shift_remove(actor);
                }
            }
            s.insert("leases".into(), Value::Object(leases));
            Ok(())
        })?;
        Ok(
            object! {if heartbeat { "renewed" } else { "released" } => count, "session" => session, "expires" => if heartbeat { now + LEASE_MILLIS } else { 0 }}
                .into(),
        )
    }

    /// Static `releaseActor(store, actor)`.
    pub fn release_actor(store: &Store, actor: &ActorId) -> Result<ReleaseReply> {
        let actor: &str = actor;
        let mut released = 0;
        store.edit_transient(&mut |s| {
            let mut leases = leases_of(s)?;
            for key in leases.keys().cloned().collect::<Vec<_>>() {
                if json::map(&leases[&key])?
                    .get("actor")
                    .and_then(Value::as_str)
                    == Some(actor)
                {
                    released += 1;
                    leases.shift_remove(&key);
                }
            }
            if let Some(Value::Object(a)) = s.get_mut("activeSessions") {
                a.shift_remove(actor);
            }
            s.insert("leases".into(), Value::Object(leases));
            Ok(())
        })?;
        Ok(object! {"released" => released}.into())
    }

    fn release_owned(&self, store: &Store, document: &str, actor: Option<&str>) -> Result<Map> {
        let mut released = 0;
        let mut sessions: Vec<String> = Vec::new();
        let mut by_actor: indexmap::IndexMap<String, Vec<Value>> = indexmap::IndexMap::new();
        store.edit_transient(&mut |s| {
            let mut leases = leases_of(s)?;
            for key in leases.keys().cloned().collect::<Vec<_>>() {
                let lease = json::map(&leases[&key])?.clone();
                if lease.get("owner").and_then(Value::as_str) == Some(self.id.as_str())
                    && actor.is_none_or(|a| lease.get("actor").and_then(Value::as_str) == Some(a))
                {
                    released += 1;
                    by_actor
                        .entry(json::java_string(lease.get("actor")))
                        .or_default()
                        .push(Value::from(key.clone()));
                    let session = json::java_string(lease.get("session"));
                    if !sessions.contains(&session) {
                        sessions.push(session);
                    }
                    leases.shift_remove(&key);
                }
            }
            let mut active =
                json::map(json::get_or_default(s, "activeSessions", &empty_map()))?.clone();
            for (key, value) in active.clone() {
                if actor.is_none_or(|a| a == key)
                    && sessions.contains(&json::java_string(Some(&value)))
                {
                    active.shift_remove(&key);
                }
            }
            s.insert("leases".into(), Value::Object(leases));
            s.insert("activeSessions".into(), Value::Object(active));
            Ok(())
        })?;
        for session in &sessions {
            // Java passed the (possibly null) actor; a null actor never matches.
            if let Some(actor) = actor {
                self.clear_preview(document, session, actor);
            }
        }
        for key in hash_order(by_actor.keys()) {
            self.forward_locks(document, &Map::new(), &by_actor[&key], "", &key);
        }
        Ok(object! {"released" => released})
    }

    fn heartbeat_selection(
        &self,
        store: &Store,
        document: &str,
        actor: &str,
        session: &str,
    ) -> Result<Map> {
        let now = now_millis();
        let expires = now + LEASE_MILLIS;
        let local = leases_of(&store.read()?.map)?;
        let mut candidates = Map::new();
        for (key, value) in &local {
            let lease = json::map(value)?;
            if lease.get("session").and_then(Value::as_str) == Some(session)
                && lease.get("actor").and_then(Value::as_str) == Some(actor)
            {
                let mut renewed = json::map(&json::copy(value))?.clone();
                renewed.insert("expires".into(), Value::from(expires));
                candidates.insert(key.clone(), Value::Object(renewed));
            }
        }
        require(
            !candidates.is_empty(),
            "Selection session has no active entities",
        )?;
        let peer = self.upstream();
        let mut decision: Option<Map> = None;
        if let Some(peer) = &peer {
            decision = peer
                .request(ApiPath::LockUpdate, Some((&Value::Object(object! {"document" => document, "session" => session, "actor" => actor, "leases" => Value::Object(candidates.clone()), "remove" => Value::Array(Vec::new())})).into()))
                .ok()
                .map(|reply| reply.0);
        }
        let empty = Value::Array(Vec::new());
        let accepted: HashSet<String> = match &decision {
            None => candidates.keys().cloned().collect(),
            Some(d) => json::list(json::get_or_default(d, "selected", &empty))?
                .iter()
                .map(|k| json::java_string(Some(k)))
                .collect(),
        };
        let mut skipped = Vec::new();
        for key in candidates.keys() {
            if !accepted.contains(key) {
                skipped.push(Value::Object(object! {"key" => key.clone()}));
            }
        }
        store.edit_transient(&mut |state| {
            let mut leases = leases_of(state)?;
            for (key, value) in &candidates {
                if accepted.contains(key) {
                    leases.insert(key.clone(), value.clone());
                } else {
                    leases.shift_remove(key);
                }
            }
            let active =
                json::map(json::get_or_default(state, "activeSessions", &empty_map()))?.clone();
            if accepted.is_empty() && active.get(actor).and_then(Value::as_str) == Some(session) {
                if let Some(Value::Object(a)) = state.get_mut("activeSessions") {
                    a.shift_remove(actor);
                }
            }
            state.insert("leases".into(), Value::Object(leases));
            Ok(())
        })?;
        if !accepted.is_empty() || !skipped.is_empty() {
            self.changed(false);
        }
        if peer.is_some() {
            let removed: Vec<Value> = skipped
                .iter()
                .map(|v| Value::from(json::java_string(v.get("key"))))
                .collect();
            let mut subset = Map::new();
            for key in hash_order(accepted.iter()) {
                if let Some(v) = candidates.get(&key) {
                    subset.insert(key, v.clone());
                }
            }
            self.forward_locks(document, &subset, &removed, session, actor);
        }
        Ok(
            object! {"renewed" => accepted.len(), "session" => session, "expires" => if accepted.is_empty() { 0 } else { expires }, "skipped" => skipped},
        )
    }

    /// `preview(document, actor, session, entities, transform)`.
    fn preview(
        &self,
        document: &str,
        actor: &str,
        session: &str,
        entities: &[Value],
        transform: &Map,
    ) -> Result<Map> {
        let store = self.store(document)?;
        let mut state = store.read()?;
        let now = now_millis();
        collaboration::expire(&mut state, EpochMillis(now))?;
        let blocked = collaboration::conflicts(&state, &ActorId::from(actor), entities.into())?;
        require_msg(
            blocked.is_empty(),
            format!(
                "Preview includes unlocked entities: {}",
                json::write_list(&blocked)
            ),
        )?;
        let leases = leases_of(&state)?;
        for value in entities {
            let e = json::map(value)?;
            let key = collaboration::key(
                &EntityKind::from(json::string(e, "kind")?),
                &EntityId::from(json::string(e, "id")?),
                Some((json::get_or_default(e, "page", &Value::from(0))).into()),
            );
            let lease = leases
                .get(&*key)
                .map(json::map)
                .transpose()?
                .cloned()
                .unwrap_or_default();
            require(
                lease.get("session").and_then(Value::as_str) == Some(session)
                    && lease.get("actor").and_then(Value::as_str) == Some(actor),
                "Preview requires this session's selection lease",
            )?;
        }
        let mut lease_limit = i64::MAX;
        for value in entities {
            let e = json::map(value)?;
            let key = collaboration::key(
                &EntityKind::from(json::string(e, "kind")?),
                &EntityId::from(json::string(e, "id")?),
                Some((json::get_or_default(e, "page", &Value::from(0))).into()),
            );
            let lease = map_value(leases.get(&*key))?;
            lease_limit = lease_limit.min(json::as_long(lease.get("expires"))?);
        }
        let expiry = (now + LEASE_MILLIS).min(lease_limit);
        let preview = object! {"session" => session, "actor" => actor, "owner" => self.id.clone(), "entities" => json::copy(&Value::Array(entities.to_vec())), "transform" => Value::Object(transform.clone()), "expires" => expiry};
        self.previews_mut()
            .insert(format!("{}:{}", document, session), preview);
        self.forward_locks(document, &Map::new(), &[], session, actor);
        self.changed(false);
        Ok(object! {"previewing" => true, "session" => session, "expires" => expiry})
    }

    fn forward_locks(
        &self,
        document: &str,
        leases: &Map,
        remove: &[Value],
        session: &str,
        actor: &str,
    ) {
        let Some(peer) = self.upstream() else { return };
        let mut previews = Map::new();
        {
            let all = lock(&self.ephemeral_previews);
            for key in hash_order(all.keys()) {
                if key.starts_with(&format!("{}:", document)) {
                    let value = all[&key].clone();
                    previews.insert(
                        json::java_string(value.get("session")),
                        Value::Object(value),
                    );
                }
            }
        }
        let _ = peer.request(
            ApiPath::LockUpdate,
            Some((&Value::Object(object! {"document" => document, "session" => session, "actor" => actor, "leases" => Value::Object(leases.clone()), "remove" => Value::Array(remove.to_vec()), "previews" => Value::Object(previews)})).into()),
        );
    }

    fn forward_preview_clear(&self, document: &str, session: &str, actor: &str) {
        let Some(peer) = self.upstream() else { return };
        let _ = peer.request(
            ApiPath::LockUpdate,
            Some((&Value::Object(object! {"document" => document, "session" => session, "actor" => actor, "leases" => empty_map(), "remove" => Value::Array(Vec::new()), "clearPreviews" => vec![Value::from(session)]})).into()),
        );
    }
}

fn require_msg(condition: bool, message: String) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::illegal_argument(message))
    }
}

/// Java `String.compareTo` (UTF-16 code unit order).
fn jcompare(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn absolute(path: &Path) -> String {
    if path.is_absolute() {
        path.display().to_string()
    } else {
        std::env::current_dir()
            .map(|d| d.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string()
    }
}

fn rand_u64() -> u64 {
    let id = uuid::Uuid::new_v4();
    let b = id.as_bytes();
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

fn last_pending_id(state: &Map) -> Result<Value> {
    let pending = json::list_at(state, "pending")?;
    let last = pending.last().ok_or_else(|| {
        Error::new(
            ErrorKind::IndexOutOfBounds,
            "Index -1 out of bounds for length 0",
        )
    })?;
    Ok(json::map(last)?.get("id").cloned().unwrap_or(Value::Null))
}

fn uploads_changes(cmd: &str, args: &[String]) -> bool {
    match cmd {
        "selection" => {
            args.len() > 1 && ["cut", "delete", "duplicate", "paste"].contains(&args[1].as_str())
        }
        "transform" => args.len() > 1 && args[1] == "commit",
        "conflict" => args.len() > 1 && args[1] == "resolve",
        _ => {
            ![
                "settings",
                "inspect",
                "snapshot",
                "status",
                "log",
                "shape",
                "conflicts",
            ]
            .contains(&cmd)
                && !(cmd == "document" && args.len() > 1 && args[1] == "list")
        }
    }
}

/// `LocalProxy.line(in)`: one header line, CR dropped, at most ~8 KiB.
fn line(input: &mut impl BufRead) -> Result<String> {
    let mut out: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = input
            .read(&mut byte)
            .map_err(|e| Error::io(notemaker_core::error::io_message(&e)))?;
        if n == 0 || byte[0] == b'\n' {
            break;
        }
        if out.len() > 8192 {
            return Err(Error::io("Header too long"));
        }
        if byte[0] != b'\r' {
            out.push(byte[0]);
        }
    }
    Ok(out
        .iter()
        .map(|&b| if b < 128 { b as char } else { '\u{fffd}' })
        .collect())
}

/// The chat request a client sent, routed as the primary server routes it
/// (`server.py` `chat_request`); `None` for a route the server does not have.
fn chat_path(method: &str, uri: &Uri) -> Result<Option<ChatPath>> {
    let Ok(method) = method.parse::<HttpMethod>() else {
        return Ok(None);
    };
    let query = uri.query()?;
    let number = |key: &str| -> Result<Option<i64>> {
        query
            .get(key)
            .map(|v| {
                v.parse::<i64>().map_err(|_| {
                    Error::illegal_argument(format!("Invalid chat {key}: {v}"))
                })
            })
            .transpose()
    };
    let segments: Vec<&str> = uri.raw_path.trim_matches('/').split('/').collect();
    let chat = |id: &str| ChatId::from(id);
    Ok(Some(match (method, segments.as_slice()) {
        (HttpMethod::Get, ["api", "chats", "options"]) => ChatPath::Options,
        (HttpMethod::Get, ["api", "chats"]) => ChatPath::List {
            limit: number("limit")?,
            offset: number("offset")?,
        },
        (HttpMethod::Get, ["api", "chats", id]) => ChatPath::Get { chat: chat(id) },
        (HttpMethod::Get, ["api", "chats", id, "events"]) => ChatPath::Events {
            chat: chat(id),
            after: number("after")?,
        },
        (HttpMethod::Get, ["api", "chats", id, "stream"]) => ChatPath::Stream {
            chat: chat(id),
            after: number("after")?,
        },
        (HttpMethod::Post, ["api", "chats"]) => ChatPath::Create,
        (HttpMethod::Post, ["api", "chats", id, "messages"]) => {
            ChatPath::Messages { chat: chat(id) }
        }
        (HttpMethod::Post, ["api", "chats", id, "stop"]) => ChatPath::Stop { chat: chat(id) },
        _ => return Ok(None),
    }))
}

/// The parts of `java.net.URI` the proxy used.
struct Uri {
    raw_path: String,
    raw_query: Option<String>,
}

impl Uri {
    fn parse(target: &str) -> Result<Uri> {
        let without_fragment = target.split('#').next().unwrap_or("");
        if target.chars().any(|c| {
            c == ' '
                || c == '"'
                || c == '<'
                || c == '>'
                || c == '\\'
                || c == '^'
                || c == '`'
                || c == '{'
                || c == '|'
                || c == '}'
                || c.is_control()
        }) {
            return Err(Error::new(
                ErrorKind::IllegalArgument,
                format!("Illegal character in path at index 0: {}", target),
            ));
        }
        let (path, query) = match without_fragment.split_once('?') {
            Some((p, q)) => (p.to_string(), Some(q.to_string())),
            None => (without_fragment.to_string(), None),
        };
        // Absolute URIs (`http://host/path`) keep only their path.
        let path = match path.find("://") {
            Some(i) => path[i + 3..]
                .find('/')
                .map(|j| path[i + 3 + j..].to_string())
                .unwrap_or_default(),
            None => path,
        };
        Ok(Uri {
            raw_path: path,
            raw_query: query,
        })
    }

    /// `getPath()`: percent-decoded (no `+` handling).
    fn path(&self) -> Result<String> {
        let bytes = self.raw_path.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    out.push(b);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// `LocalProxy.query(uri)`.
    fn query(&self) -> Result<HashMap<String, String>> {
        let mut out = HashMap::new();
        if let Some(raw) = &self.raw_query {
            for part in split(raw, "&") {
                let mut pair = part.splitn(2, '=');
                let key = url_decode(pair.next().unwrap_or(""))?;
                let value = match pair.next() {
                    Some(v) => url_decode(v)?,
                    None => String::new(),
                };
                out.insert(key, value);
            }
        }
        Ok(out)
    }
}

fn normalize_preview(preview: &Map, leases: &Map) -> Result<Option<Map>> {
    let now = now_millis();
    let expires = json::as_long(Some(json::get_or_default(
        preview,
        "expires",
        &Value::from(0),
    )))?;
    let mut lease_limit = i64::MAX;
    let actor = json::java_string(preview.get("actor"));
    let session = json::java_string(preview.get("session"));
    let empty = Value::Array(Vec::new());
    let entities = json::list(json::get_or_default(preview, "entities", &empty))?;
    if entities.is_empty() {
        return Ok(None);
    }
    for value in entities {
        let entity = json::map(value)?;
        let key = collaboration::key(
            &EntityKind::from(json::java_string(entity.get("kind"))),
            &EntityId::from(json::java_string(entity.get("id"))),
            Some((json::get_or_default(entity, "page", &Value::from(0))).into()),
        );
        let lease = leases
            .get(&*key)
            .map(json::map)
            .transpose()?
            .cloned()
            .unwrap_or_default();
        if lease.get("actor").and_then(Value::as_str) != Some(actor.as_str())
            || lease.get("session").and_then(Value::as_str) != Some(session.as_str())
        {
            return Ok(None);
        }
        let live_until = json::as_long(Some(json::get_or_default(
            &lease,
            "expires",
            &Value::from(0),
        )))?;
        if live_until <= now {
            return Ok(None);
        }
        lease_limit = lease_limit.min(live_until);
    }
    let capped = expires.min((now + LEASE_MILLIS).min(lease_limit));
    if capped <= now {
        return Ok(None);
    }
    let mut normalized = json::map(&json::copy(&Value::Object(preview.clone())))?.clone();
    normalized.insert("expires".into(), Value::from(capped));
    Ok(Some(normalized))
}

fn apply_remote_leases(store: &Store, body: &Map) -> Result<Map> {
    let incoming = json::map(json::get_or_default(body, "leases", &empty_map()))?.clone();
    let mut accepted: Vec<Value> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    store.edit_transient(&mut |s| {
        let mut current = leases_of(s)?;
        collaboration::expire(s, EpochMillis(now_millis()))?;
        let empty = Value::Array(Vec::new());
        for key in json::list(json::get_or_default(body, "remove", &empty))? {
            let k = json::key_of(Some(key));
            let lease = k.and_then(|k| current.get(k)).map(json::map).transpose()?.cloned().unwrap_or_default();
            if lease.is_empty() || ((body.get("session").and_then(Value::as_str) == Some("") || json::objects_equal(body.get("session"), lease.get("session"))) && json::objects_equal(body.get("actor"), lease.get("actor"))) {
                if let Some(k) = k {
                    current.shift_remove(k);
                }
            }
        }
        let now = now_millis();
        for (key, value) in &incoming {
            let mut lease = json::map(&json::copy(value))?.clone();
            let expiry = json::as_long(Some(json::get_or_default(&lease, "expires", &Value::from(0))))?.min(now + LEASE_MILLIS);
            if expiry <= now {
                continue;
            }
            lease.insert("expires".into(), Value::from(expiry));
            let old = current.get(key).map(json::map).transpose()?.cloned().unwrap_or_default();
            if !old.is_empty() && json::as_long(Some(json::get_or_default(&old, "expires", &Value::from(0))))? > now && (!json::objects_equal(old.get("actor"), lease.get("actor")) || !json::objects_equal(old.get("session"), lease.get("session"))) {
                skipped.push(Value::Object(object! {"key" => key.clone(), "actor" => old.get("actor").cloned().unwrap_or(Value::Null), "session" => old.get("session").cloned().unwrap_or(Value::Null)}));
                continue;
            }
            current.insert(key.clone(), Value::Object(lease));
            accepted.push(Value::from(key.clone()));
        }
        s.insert("leases".into(), Value::Object(current));
        Ok(())
    })?;
    Ok(object! {"accepted" => accepted.clone(), "selected" => accepted, "skipped" => skipped})
}

fn require_session_scope(state: &Map, session: Option<&str>, scope: &SelectionScope) -> Result<()> {
    let Some(session) = session else {
        return Ok(());
    };
    for raw in leases_of(state)?.values() {
        let lease = json::map(raw)?;
        if lease.get("session").and_then(Value::as_str) == Some(session) {
            let layer = json::java_string(Some(json::get_or_default(
                lease,
                "layer",
                &Value::from("base"),
            )));
            require(
                scope.layer == layer
                    && scope.page.0
                        == int_of(Some(json::get_or_default(lease, "page", &Value::from(0))))?,
                "Selection session belongs to another layer/page",
            )?;
        }
    }
    Ok(())
}

fn requested_selection_scope(request: &Map) -> Result<SelectionScope> {
    let layer = match request.get("layer") {
        None => "base".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => return Err(Error::null_pointer()),
        Some(_) => return Err(Error::class_cast("layer is not a String")),
    };
    let page = layer_editor::page(request.into())?;
    let refs = json::list(request.get("entities").unwrap_or(&Value::Null))?;
    require(
        !refs.is_empty() && refs.len() <= 200,
        "Selection needs 1–200 entities",
    )?;
    Ok(SelectionScope { layer, page })
}

fn selection_scope(store: &Store, request: &Map) -> Result<SelectionScope> {
    let scope = requested_selection_scope(request)?;
    LayerEditor::require_references(
        &store.model()?,
        &LayerId::from(&scope.layer),
        scope.page,
        (json::list(request.get("entities").unwrap_or(&Value::Null))?).into(),
    )?;
    if request.contains_key("session") {
        let state = store.read()?;
        require_session_scope(
            &state,
            request.get("session").and_then(Value::as_str),
            &scope,
        )?;
    }
    Ok(scope)
}

fn select_validated(
    store: &Store,
    actor: &str,
    request: &Map,
    scope: &SelectionScope,
) -> Result<Map> {
    let mut selected: Vec<Value> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    let session = json::java_string(Some(
        &request
            .get("session")
            .cloned()
            .unwrap_or_else(|| Value::from(String::from(notemaker_core::ids::Uuid::random()))),
    ));
    let now = now_millis();
    let page = request.get("page").cloned().unwrap_or(Value::from(0));
    require(
        page.is_number() && (0..=10000).contains(&json::as_int(Some(&page))?),
        "Invalid page",
    )?;
    let entities = json::list(request.get("entities").unwrap_or(&Value::Null))?.clone();
    require(
        !entities.is_empty() && entities.len() <= 200,
        "Selection needs 1–200 entities",
    )?;
    store.edit_transient(&mut |s| {
        LayerEditor::require_references(&store::model(s)?, &LayerId::from(&scope.layer), scope.page, (&entities).into())?;
        collaboration::expire(s, EpochMillis(now))?;
        require_session_scope(s, Some(&session), scope)?;
        let mut leases = leases_of(s)?;
        let mut sessions = json::map(json::get_or_default(s, "activeSessions", &empty_map()))?.clone();
        for value in &entities {
            let e = json::map(value)?;
            let kind = json::string(e, "kind")?;
            let id = json::string(e, "id")?;
            let key = String::from(collaboration::key(&EntityKind::from(kind), &EntityId::from(id), Some((&page).into())));
            let prior = leases.get(&key).map(json::map).transpose()?.cloned().unwrap_or_default();
            if !prior.is_empty() && (prior.get("actor").and_then(Value::as_str) != Some(actor) || prior.get("session").and_then(Value::as_str) != Some(session.as_str())) {
                skipped.push(Value::Object(object! {"kind" => kind, "id" => id, "page" => page.clone(), "actor" => prior.get("actor").cloned().unwrap_or(Value::Null)}));
                continue;
            }
            let mut lease = object! {"layer" => scope.layer.clone(), "session" => session.clone(), "actor" => actor, "page" => page.clone(), "expires" => now + LEASE_MILLIS, "created" => now};
            if let Some(owner) = request.get("owner") {
                lease.insert("owner".into(), owner.clone());
            }
            leases.insert(key, Value::Object(lease));
            selected.push(Value::Object(object! {"kind" => kind, "id" => id, "page" => page.clone()}));
        }
        if !selected.is_empty() {
            sessions.insert(actor.to_string(), Value::from(session.clone()));
        }
        s.insert("activeSessions".into(), Value::Object(sessions));
        s.insert("leases".into(), Value::Object(leases));
        Ok(())
    })?;
    Ok(
        object! {"session" => session, "selected" => selected, "skipped" => skipped, "expires" => now + LEASE_MILLIS},
    )
}

// Java library semantics used only by this module.

/// `s.split(regex_literal)` for a literal separator: trailing empty strings
/// are removed; `""` gives `[""]`.
fn split(s: &str, sep: &str) -> Vec<String> {
    let mut parts: Vec<String> = s.split(sep).map(str::to_string).collect();
    if s.is_empty() {
        return vec![String::new()];
    }
    while parts.len() > 1 && parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    if parts.len() == 1 && parts[0].is_empty() && !s.is_empty() {
        parts.clear();
    }
    parts
}

/// `URLDecoder.decode(s, "UTF-8")`.
fn url_decode(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                while i < bytes.len() && bytes[i] == b'%' {
                    if i + 2 >= bytes.len() {
                        return Err(Error::illegal_argument(
                            "URLDecoder: Incomplete trailing escape (%) pattern",
                        ));
                    }
                    let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                        .ok()
                        .and_then(|h| u8::from_str_radix(h, 16).ok());
                    match hex {
                        Some(b) => out.push(b),
                        None => {
                            return Err(Error::illegal_argument(
                                "URLDecoder: Illegal hex characters in escape (%) pattern",
                            ))
                        }
                    }
                    i += 3;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Iteration order of a `HashMap`/`ConcurrentHashMap` with these keys.
fn hash_order<'a>(keys: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    json::java_hash_set_order(keys.into_iter().cloned())
}
