//! Port of `AppReplica.java`: the editor's selected proxy. Local reads retain
//! the incremental `Store` cache.

use crate::error::{AppError, AppResult};
use crate::field::Field;
use crate::jx::{self, JThread, ThreadName};
use crate::main_activity::MainActivity;
use notemaker_cli::local_proxy::LocalProxy;
use notemaker_cli::main::CommandArgs;
use notemaker_core::ids::{ActorId, DocumentName};
use notemaker_core::endpoint::{Disconnector, Endpoint};
use notemaker_core::json::{self, Map, Value};
use notemaker_core::store::{State, Store};
use notemaker_core::{ids, object, timing};
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

/// The arguments of one CLI command run against the replica
/// (`List<String> args`), e.g. `["selection", "select", "-"]`.
#[derive(Clone, Copy, Debug)]
pub struct CliArgs<'a>(pub &'a [&'a str]);

/// A `… snapshot` command's reply and its snapshot (see
/// [`AppReplica::command_snapshot`]).
pub struct SnapshotReply {
    pub result: Map,
    pub snapshot: Option<State>,
}

/// `final class AppReplica`.
pub struct AppReplica {
    pub app: Weak<MainActivity>,
    /// `final Store local` (shared with `MainActivity.store`).
    pub local: Arc<Store>,
    pub document: DocumentName,
    /// `final Map<String,Object> descriptor` (`null` for the embedded proxy).
    pub descriptor: Option<Map>,
    pub descriptor_file: PathBuf,
    /// `volatile HttpURLConnection stream`: the open notification stream.
    pub stream: Field<Option<Disconnector>>,
}

impl AppReplica {
    /// `AppReplica(MainActivity app, String document, String url, String token) throws Exception`.
    pub fn new(app: &Arc<MainActivity>, document: &DocumentName, url: &str, token: &str) -> AppResult<Arc<AppReplica>> {
        let files = app.files_dir()?;
        let local = Arc::new(Store::new(&files, document)?);
        let descriptor = if url.is_empty() { None } else { Some(object! {"url" => url, "token" => token}) };
        let descriptor_file = match &descriptor {
            None => files.join("proxy.json"),
            Some(_) => files.join(format!("selected-proxy-{}.json", String::from(ids::Uuid::from_name(format!("{url}\n{token}"))))),
        };
        if let Some(d) = &descriptor {
            std::fs::write(&descriptor_file, json::write_map(d).as_bytes())?;
        }
        Ok(Arc::new(AppReplica { app: Arc::downgrade(app), local, document: document.clone(), descriptor, descriptor_file, stream: Field::new(None) }))
    }

    fn app(&self) -> Arc<MainActivity> {
        self.app.upgrade().expect("MainActivity dropped")
    }

    /// `boolean remote()`.
    pub fn remote(&self) -> bool {
        self.descriptor.is_some()
    }

    /// `Map<String,Object> read() throws Exception`: `snapshot` command.
    /// For the embedded proxy this is Java's `Store.Snapshot` object: the
    /// returned [`State`] carries the materialized model, which
    /// `MainActivity.refreshShared` takes instead of parsing `effectiveModel`.
    pub fn read(&self) -> AppResult<State> {
        if self.descriptor.is_some() {
            let v = self.command(CliArgs(&["snapshot"]), None)?;
            return Ok(State::plain(json::map(&v)?.clone()));
        }
        let app = self.app();
        let trace = app.trace_cold_load.get();
        let mut started = if trace { jx::nano_time().0 } else { 0 };
        let ensure_started = started;
        let previous = if trace {
            Some(timing::set_observer(Some(Box::new(move |phase: &str| {
                jx::log::i(crate::jx::log::STARTUP, &format!("{}={}", phase, notemaker_core::jfmt::double((jx::nano_time().0 - ensure_started) as f64 / 1e6)));
            }))))
        } else {
            None
        };
        let proxy = crate::replica_service::ensure(&app.this);
        if let Some(previous) = previous {
            timing::set_observer(previous);
        }
        let proxy = proxy?;
        if trace {
            jx::log::i(crate::jx::log::STARTUP, &format!("proxy_ensure_duration={}", notemaker_core::jfmt::double((jx::nano_time().0 - started) as f64 / 1e6)));
        }
        started = if trace { jx::nano_time().0 } else { 0 };
        let result = proxy.snapshot_state(&self.document)?;
        if trace {
            jx::log::i(crate::jx::log::STARTUP, &format!("snapshot_execute_duration={}", notemaker_core::jfmt::double((jx::nano_time().0 - started) as f64 / 1e6)));
        }
        Ok(result)
    }

    /// `Object command(List<String> args, Object input) throws Exception`.
    pub fn command(&self, args: CliArgs, input: Option<&Value>) -> AppResult<Value> {
        self.command_named(&self.document, args, input)
    }

    /// `Object command(String name, List<String> args, Object input) throws Exception`.
    pub fn command_named(&self, name: &DocumentName, args: CliArgs, input: Option<&Value>) -> AppResult<Value> {
        Ok(self.command_with(name, args, input, false)?.0)
    }

    /// `command(args, input)` for the `… snapshot` commands: the reply and its
    /// `snapshot`. From the embedded proxy the snapshot is the
    /// `Store.Snapshot`-like [`State`] carrying its model (Java put that
    /// object in the reply), so `refreshShared` does not re-parse
    /// `effectiveModel`. `None` when the reply has no snapshot.
    pub fn command_snapshot(&self, args: CliArgs, input: Option<&Value>) -> AppResult<SnapshotReply> {
        let (result, captured) = self.command_with(&self.document, args, input, true)?;
        let result = json::map(&result)?.clone();
        let snapshot = match captured {
            Some(state) => Some(state),
            None => match result.get("snapshot") {
                Some(Value::Object(s)) => Some(State::plain(s.clone())),
                _ => None,
            },
        };
        Ok(SnapshotReply { result, snapshot })
    }

    fn command_with(&self, name: &DocumentName, args: CliArgs, input: Option<&Value>, capture: bool) -> AppResult<(Value, Option<State>)> {
        let args = args.0;
        let owned = CommandArgs(args.iter().map(|s| s.to_string()).collect());
        if let Some(descriptor) = &self.descriptor {
            return Ok((LocalProxy::request(descriptor, name, &ActorId::from("human"), &owned, input)?, None));
        }
        let app = self.app();
        let trace = app.trace_cold_load.get() && args.len() == 1 && args[0] == "snapshot";
        let mut started = if trace { jx::nano_time().0 } else { 0 };
        let ensure_started = started;
        let previous = if trace {
            Some(timing::set_observer(Some(Box::new(move |phase: &str| {
                jx::log::i(crate::jx::log::STARTUP, &format!("{}={}", phase, notemaker_core::jfmt::double((jx::nano_time().0 - ensure_started) as f64 / 1e6)));
            }))))
        } else {
            None
        };
        let proxy = crate::replica_service::ensure(&app.this);
        if let Some(previous) = previous {
            timing::set_observer(previous);
        }
        let proxy = proxy?;
        if trace {
            jx::log::i(crate::jx::log::STARTUP, &format!("proxy_ensure_duration={}", notemaker_core::jfmt::double((jx::nano_time().0 - started) as f64 / 1e6)));
        }
        started = if trace { jx::nano_time().0 } else { 0 };
        let args_value = Value::Array(owned.0.into_iter().map(Value::String).collect());
        let body = object! {"document" => name.clone(), "actor" => "human", "args" => args_value, "input" => input.cloned().unwrap_or(Value::Null)};
        let result = if capture { proxy.execute_with_snapshot(&body)? } else { (proxy.execute(&body)?, None) };
        if trace {
            jx::log::i(crate::jx::log::STARTUP, &format!("snapshot_execute_duration={}", notemaker_core::jfmt::double((jx::nano_time().0 - started) as f64 / 1e6)));
        }
        Ok(result)
    }

    /// `void listen(Runnable changed) throws Exception`: follows the selected
    /// proxy's `/api/stream` while this replica is current.
    pub fn listen(&self, changed: &dyn Fn()) -> AppResult<()> {
        let descriptor = self.descriptor.clone().ok_or_else(|| AppError::Core(notemaker_core::Error::null_pointer()))?;
        let mut connection = Endpoint::new(descriptor).open("/api/stream")?;
        self.stream.set(Some(connection.disconnector()));
        connection.set_read_timeout(45000);
        let result = (|| -> AppResult<()> {
            if connection.response_code()? != 200 {
                return Err(AppError::io("Cannot subscribe to selected proxy"));
            }
            let mut input = std::io::BufReader::new(connection.input_stream()?);
            let app = self.app();
            let current = |app: &MainActivity| app.running.get() && app.replica.with(|r| r.as_ref().is_some_and(|r| std::ptr::eq(r.as_ref(), self)));
            let mut line = String::new();
            while current(&app) {
                line.clear();
                if input.read_line(&mut line)? == 0 {
                    break;
                }
                if line.starts_with("data:") {
                    changed();
                }
            }
            if current(&app) {
                return Err(AppError::io("Selected proxy notification stream ended"));
            }
            Ok(())
        })();
        connection.disconnect();
        self.stream.set(None);
        result
    }

    /// `void close()`: disconnects the stream on thread `close-proxy-stream`.
    pub fn close(&self) {
        if let Some(connection) = self.stream.get() {
            JThread::spawn(&ThreadName::from("close-proxy-stream"), move || connection.disconnect());
        }
    }
}
