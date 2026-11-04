//! Port of `ReplicaService.java`: the local replica owner.
//!
//! Android may stop background execution; the journal survives. The single
//! `replica-writer` executor serializes every local command the app issues
//! (Java `ReplicaService.writes`), and `proxy` is the process-wide embedded
//! `LocalProxy` (Java `static volatile LocalProxy proxy`).

use crate::concurrent::ExecutorService;
use crate::error::AppResult;
use crate::field::Field;
use crate::components::ComponentEvent;
use crate::jx::{self, JResult, Obj, Reply};
use crate::monitor::Monitor;
use notemaker_cli::local_proxy::LocalProxy;
use notemaker_core::net::Port;
use std::sync::{Arc, LazyLock};

/// `static final ExecutorService writes` (thread `replica-writer`).
pub static WRITES: LazyLock<ExecutorService> = LazyLock::new(|| ExecutorService::named("replica-writer"));
/// `static volatile LocalProxy proxy`.
pub static PROXY: Field<Option<Arc<LocalProxy>>> = Field::new(None);
/// The `ReplicaService.class` monitor (`static synchronized` / `synchronized(ReplicaService.class)`).
static CLASS_LOCK: Monitor = Monitor::new();

/// `ReplicaService.writes`.
pub fn writes() -> &'static ExecutorService {
    &WRITES
}
/// `ReplicaService.proxy` (may be `None`).
pub fn proxy() -> Option<Arc<LocalProxy>> {
    PROXY.get()
}

/// `static synchronized LocalProxy ensure(Context context) throws IOException`.
pub fn ensure(context: &Obj) -> AppResult<Arc<LocalProxy>> {
    let _class = CLASS_LOCK.enter();
    if let Some(p) = PROXY.get() {
        return Ok(p);
    }
    let dir = crate::ui::files_dir(context)?;
    // 8766 as in Java. Isolated latency builds (tests/tablet_erase_latency.py)
    // compile with NOTEMAKER_REPLICA_PORT=0, replacing the Java source patch.
    let port = option_env!("NOTEMAKER_REPLICA_PORT").and_then(|v| v.parse::<u16>().ok()).map(Port::from).unwrap_or(Port::DEFAULT_PROXY);
    let p = open_proxy(&dir, port)?;
    PROXY.set(Some(p.clone()));
    Ok(p)
}

/// How long a relaunched process waits for its killed predecessor to release
/// `proxy.lock` and the proxy port.
const TAKEOVER_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// `new LocalProxy(dir, port)`, retried while another process holds the
/// replica or its port. When Android kills the app (force-stop, instrumentation
/// restart, low memory) and immediately starts a replacement, the old process
/// can still be exiting, so its `fcntl` lock and listening socket have not been
/// released yet. Java tried once and crashed with "Cannot read saved document"
/// (ADR 0031). A genuine second owner still fails, after `TAKEOVER_WAIT`.
fn open_proxy(dir: &std::path::Path, port: Port) -> AppResult<Arc<LocalProxy>> {
    let deadline = std::time::Instant::now() + TAKEOVER_WAIT;
    loop {
        match LocalProxy::new(dir, port) {
            Ok(p) => return Ok(p),
            Err(e) if held_by_another_process(&e) && std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

fn held_by_another_process(e: &notemaker_core::Error) -> bool {
    e.kind.is_io() && matches!(e.message(), "A proxy already owns this replica" | "Address already in use")
}

/// Routes the shim's `onStartCommand` / `onDestroy`.
pub fn component_event(event: &ComponentEvent) -> JResult<Reply> {
    let ComponentEvent { component, name, args } = *event;
    match &**name {
        "onStartCommand" => {
            let this = jx::global(component).expect("service");
            let intent = args.first().and_then(jx::global);
            let flags = args.get(1).map(|a| jx::unbox_int(a)).unwrap_or(0);
            let id = args.get(2).map(|a| jx::unbox_int(a)).unwrap_or(0);
            Ok(Reply::Int(on_start_command(&this, intent, flags, id)?))
        }
        "onDestroy" => {
            on_destroy();
            Ok(Reply::Void)
        }
        other => Err(jx::JavaError::new("java.lang.IllegalStateException", format!("Unknown ReplicaService event {other}"))),
    }
}

/// `public int onStartCommand(Intent intent, int flags, int id)`.
fn on_start_command(this: &Obj, intent: Option<Obj>, _flags: i32, _id: i32) -> JResult<i32> {
    let (document, host) = match &intent {
        None => (Some("notes".to_string()), Some(String::new())),
        Some(i) => (
            jcall!(i, "android/content/Intent", "getStringExtra", "(Ljava/lang/String;)Ljava/lang/String;", "document")?.string(),
            jcall!(i, "android/content/Intent", "getStringExtra", "(Ljava/lang/String;)Ljava/lang/String;", "host")?.string(),
        ),
    };
    let service = this.clone();
    WRITES.execute(move || {
        let result: AppResult<()> = (|| {
            let proxy = ensure(&service)?;
            // Java passed a null document straight to configure, which threw.
            let document = document.ok_or_else(|| crate::error::AppError::Core(notemaker_core::Error::null_pointer()))?;
            proxy.configure(&notemaker_core::ids::DocumentName::from(document), host.as_deref())?;
            Ok(())
        })();
        if let Err(e) = result {
            jx::log::e_err(crate::jx::log::NOTEMAKER, "Cannot start local proxy", &e);
        }
    });
    Ok(crate::android_consts::SERVICE_START_NOT_STICKY)
}

/// `public void onDestroy()` (the shim calls `super.onDestroy()` afterwards).
///
/// Finish shutdown before Android can restart the app process. Returning
/// while the old process still holds proxy.lock lets its replacement fail to
/// open the same replica.
pub fn on_destroy() {
    let done = WRITES.submit(|| {
        let _class = CLASS_LOCK.enter();
        let old = PROXY.get();
        if let Some(old) = old {
            if let Err(e) = old.close() {
                jx::log::e_err(crate::jx::log::NOTEMAKER, "Proxy shutdown", &e);
            }
        }
        PROXY.set(None);
    });
    if let Err(e) = done.get() {
        jx::log::e_err(crate::jx::log::NOTEMAKER, "Proxy shutdown task failed", &e);
    }
}
