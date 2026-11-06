//! Port of `Main.java`: deterministic local actions and the CLI entry point.
//! No implicit network calls or AI provider dependency.

use crate::item_coverage::FACTORY;
use crate::jutil;
use crate::local_proxy::{CommandReply, LeaseUpdate, LocalProxy};
use notemaker_core::endpoint::EndpointDescriptor;
use notemaker_core::store::EditorPreferences;
use crate::png::{Png, RenderOutputPath};
use crate::raster::PixelSize;
use notemaker_core::clock::EpochMillis;
use notemaker_core::collaboration;
use notemaker_core::drawing_actions;
use notemaker_core::error::require;
use notemaker_core::geometry::{DocPoint, DocRect, Polygon};
use notemaker_core::ids::{
    ActorId, ConflictChoice, ConflictChoiceName, ConflictId, DocumentName, EntityId, EntityKind,
    LayerId, LayerPosition, PageNumber, SessionId,
};
use notemaker_core::layer_editor::{self, HitMode, ItemFilter, LayerEditor};
use notemaker_core::model::Model;
use notemaker_core::net::Port;
use notemaker_core::paths::ReplicaDir;
use notemaker_core::render::{self, HiddenLayers};
use notemaker_core::shape_catalog;
use notemaker_core::store::{self, HistoryDirection, State, Store, WithState};
use notemaker_core::wire::OperationList;
use notemaker_core::{
    actions, gif_asset, ids, image_asset, jfmt, json, object, Error, ErrorKind, Map, Result, Value,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Command-line arguments after the program name (Java `String[] args`),
/// options first, then the command and its arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandArgs(pub Vec<String>);

impl std::ops::Deref for CommandArgs {
    type Target = [String];
    fn deref(&self) -> &[String] {
        &self.0
    }
}

/// A process exit status (`System.exit` code).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitStatus(pub i32);

impl ExitStatus {
    pub const SUCCESS: ExitStatus = ExitStatus(0);
    pub const FAILURE: ExitStatus = ExitStatus(1);
}

thread_local! {
    /// `Main.proxyInput` (package-visible `ThreadLocal<Object>`): the
    /// `/command` body's `input` while `LocalProxy.execute` runs `Main.run`.
    pub static PROXY_INPUT: RefCell<Option<Value>> = const { RefCell::new(None) };
    /// `Main.inProxy` (package-visible `ThreadLocal<Boolean>`).
    pub static IN_PROXY: Cell<bool> = const { Cell::new(false) };
}

/// `Main.HELP`.
pub const HELP: &str = concat!(
    "NoteMaker CLI\nUsage: notemaker [OPTIONS] COMMAND [ARGS]\n",
    "Options precede the command:\n",
    "--state DIR                Local replica; default NOTEMAKER_STATE, otherwise\n",
    "                           $XDG_STATE_HOME/notemaker or ~/.local/state/notemaker\n",
    "--document NAME            Case-sensitive document name (default: notes)\n",
    "--actor ID                 Stable editing identity; default NOTEMAKER_ACTOR, otherwise cli\n",
    "--proxy-file FILE          Selected proxy; default NOTEMAKER_PROXY_FILE\n",
    "--help                     Show this help without contacting a server\n\n",
    "ui sessions | ui state SESSION | ui actions SESSION\n",
    "ui action SESSION NAME FILE|-   Control one running app session with JSON arguments\n",
    "serve [PORT [UPSTREAM]]     Start replica proxy; upstream URL or descriptor\n",
    "document create [fixed|pages|infinite] | document delete | document list\ninspect | status | log\n",
    "diagram add FILE|-         Atomic new layer and editable diagram objects\n",
    "svg add FILE.svg|- [PLACEMENT.json]   Place SVG; optional {x,y,w,h,layer,page,object}\n",
    "shape list                   List supported editable shapes\n",
    "layer put FILE|-           {layer,name,force} (create or update)\nlayer hide ID | layer show ID\n",
    "layer duplicate ID | layer merge SOURCE TARGET | layer reorder ID INDEX\n",
    "object put [snapshot] FILE|-  Create/replace an editable object\n",
    "object update ID FILE|-    Patch geometry, label or styling\n",
    "object hide ID | object show ID\n",
    "selection query FILE|- | selection select FILE|- | selection heartbeat SESSION | selection deselect SESSION\n",
    "selection copy SESSION | selection cut SESSION | selection paste FILE|-\n",
    "selection duplicate SESSION | selection delete SESSION | selection disconnect\n",
    "transform preview FILE|- | transform commit FILE|- | transform cancel SESSION\n",
    "conflicts | conflict resolve ID local|remote|both\n",
    "stroke put FILE|- | stroke hide ID | stroke show ID\n",
    "erase FILE|- | fill FILE|- | template add FILE|- | image add FILE [PLACEMENT.json] | gif add FILE [PLACEMENT.json]\n",
    "apply FILE|-               Atomic JSON array of protocol actions\n",
    "append [snapshot] FILE|-   Append one raw action; optionally return post-edit snapshot\n",
    "settings FILE|-            Update local editor preferences\n",
    "snapshot                   Materialized model and local editor metadata\n",
    "checkpoint                 Fold the local journal into its durable base\n",
    "undo | redo [snapshot]     History for the selected actor; optional post-edit snapshot\n",
    "export FILE.svg|FILE.png [PAGE [X Y WIDTH HEIGHT]]   Page is zero-based\n",
    "sync [UPSTREAM]            Request proxy synchronization (URL or proxy descriptor)\n",
    "configure UPSTREAM         Set sole upstream; empty string disconnects\n",
    "FILE|- accepts JSON from a file or stdin (svg add reads raw SVG).\n",
    "Edits require a running selected proxy and work offline; document list lists local documents only.\n",
    "JSON on stdout (except help); JSON errors on stderr and exit 1.\n",
    "Mutations are not idempotent: inspect log before retrying a lost result.\n",
    "sync and document delete retries are idempotent. Deletion is permanent; names cannot be reused.\n",
    "See apps/cli/README.md for input schemas.\n\n",
    "Examples:\n",
    "  notemaker --document notes status\n",
    "  notemaker --proxy-file /path/to/replica/proxy.json --document notes sync http://127.0.0.1:8765\n",
);

/// `Main.main(args)`: prints the JSON result (if any) to stdout; on failure
/// prints `{"error": rootCause.toString()}` to stderr. Returns the process
/// exit code (0, or 1 where Java called `System.exit(1)`). `args` excludes
/// the program name.
pub fn main(args: CommandArgs) -> ExitStatus {
    match run(&args) {
        Ok(result) => {
            if let Some(result) = result {
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "{}", json::write(&result));
                let _ = out.flush();
            }
            ExitStatus::SUCCESS
        }
        Err(e) => {
            let _ = std::io::stdout().flush();
            eprintln!(
                "{}",
                json::write(&Value::Object(object! {"error" => e.to_string()}))
            );
            ExitStatus::FAILURE
        }
    }
}

fn in_proxy() -> bool {
    IN_PROXY.with(|p| p.get())
}

fn io_error(path: &str, e: std::io::Error) -> Error {
    match e.kind() {
        // java.nio.file.NoSuchFileException carries just the path.
        std::io::ErrorKind::NotFound => Error::io(path.to_string()),
        _ => Error::io(format!(
            "{}: {}",
            path,
            notemaker_core::error::io_message(&e)
        )),
    }
}

/// `File.getAbsolutePath()`.
fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn arity(a: &[String], n: usize) -> Result<()> {
    if a.len() == n {
        Ok(())
    } else {
        Err(Error::illegal_argument(format!(
            "Expected {} arguments; see --help",
            n
        )))
    }
}

fn require_msg(condition: bool, message: String) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::illegal_argument(message))
    }
}

fn read_limited(path: &str, limit: u64, message: &str) -> Result<Vec<u8>> {
    if path == "-" {
        let mut out = Vec::new();
        let mut buffer = [0u8; 8192];
        let mut stdin = std::io::stdin().lock();
        loop {
            let n = stdin
                .read(&mut buffer)
                .map_err(|e| Error::io(notemaker_core::error::io_message(&e)))?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buffer[..n]);
            require(out.len() as u64 <= limit, message)?;
        }
        return Ok(out);
    }
    let size = std::fs::metadata(path)
        .map_err(|e| io_error(path, e))?
        .len();
    require(size <= limit, message)?;
    std::fs::read(path).map_err(|e| io_error(path, e))
}

fn read_input(path: &str) -> Result<String> {
    Ok(String::from_utf8_lossy(&read_limited(path, 3_000_000, "Input exceeds 3 MB")?).into_owned())
}

fn input(path: &str) -> Result<Value> {
    if in_proxy() {
        let value = PROXY_INPUT
            .with(|p| p.borrow().clone())
            .filter(|v| !v.is_null());
        require(
            path == "-" && value.is_some(),
            "Proxy input must be supplied in the input field with FILE=-",
        )?;
        return Ok(json::copy(&value.unwrap_or(Value::Null)));
    }
    json::parse(&read_input(path)?)
}

/// A command's JSON object result.
fn obj(m: impl Into<Map>) -> Result<Option<Value>> {
    Ok(Some(Value::Object(m.into())))
}

fn map_of(value: Value) -> Result<Map> {
    match value {
        Value::Object(m) => Ok(m),
        Value::Null => Err(Error::null_pointer()),
        other => Ok(json::map(&other)?.clone()),
    }
}

fn list_of(value: Value) -> Result<Vec<Value>> {
    match value {
        Value::Array(l) => Ok(l),
        Value::Null => Err(Error::null_pointer()),
        other => Ok(json::list(&other)?.clone()),
    }
}

const PLACEMENT_KEYS: [&str; 7] = ["x", "y", "w", "h", "layer", "page", "object"];

fn svg_input(a: &[String]) -> Result<Map> {
    require(
        (a.len() == 2 || a.len() == 3) && a[0] == "add",
        "svg add FILE.svg|- [PLACEMENT.json]",
    )?;
    if in_proxy() {
        arity(a, 2)?;
        return map_of(input(&a[1])?);
    }
    require(
        a.len() != 3 || a[1] != "-" || a[2] != "-",
        "SVG and placement cannot both read stdin",
    )?;
    let mut placement = if a.len() == 3 {
        map_of(input(&a[2])?)?
    } else {
        Map::new()
    };
    for key in placement.keys() {
        require_msg(
            PLACEMENT_KEYS.contains(&key.as_str()),
            format!("Unknown SVG placement field: {}", key),
        )?;
    }
    placement.insert("svg".into(), Value::from(read_input(&a[1])?));
    Ok(placement)
}

fn image_placement(args: &[String]) -> Result<Map> {
    let offset = if args[0] == "image" || args[0] == "gif" {
        2
    } else {
        1
    };
    require(
        args.len() == offset + 1 || args.len() == offset + 2,
        "image|gif add FILE [PLACEMENT.json]",
    )?;
    require(
        args[offset - 1] == "add",
        "image|gif add FILE [PLACEMENT.json]",
    )?;
    let placement = if args.len() == offset + 2 {
        map_of(json::parse(&read_input(&args[offset + 1])?)?)?
    } else {
        Map::new()
    };
    for key in placement.keys() {
        require_msg(
            PLACEMENT_KEYS.contains(&key.as_str()),
            format!("Unknown image placement field: {}", key),
        )?;
    }
    Ok(placement)
}

fn image_bytes(path: &str) -> Result<Vec<u8>> {
    let bytes = read_limited(path, image_asset::MAX_BYTES as u64, "Image exceeds 2 MiB")?;
    image_asset::from_bytes(&bytes)?;
    Ok(bytes)
}

fn asset_bytes(path: &str, limit: usize) -> Result<Vec<u8>> {
    read_limited(path, limit as u64, "Asset exceeds 2 MiB")
}

/// Package-visible `Main.snapshot(state)`: materialized model plus local
/// editor metadata.
fn snapshot(s: &State) -> Result<Map> {
    Ok((snapshot_state(s)?.into_map()).into())
}

/// `Main.snapshot` as Java built it: a `Store.Snapshot` that also carries the
/// materialized model, so in-process readers (the app's embedded proxy) get
/// the model without re-parsing `effectiveModel`.
pub fn snapshot_state(s: &State) -> Result<State> {
    snapshot_state_with(s, true)
}

/// [`snapshot_state`] for in-process readers only: the reply carries the
/// model and leaves out the `effectiveModel` JSON. Java's `model.json()`
/// shared every entity map, so building it cost O(entities); here it
/// deep-copies every stroke. `Store.Snapshot` readers never looked at it
/// ("In-process snapshots retain the immutable materialization; JSON
/// clients use effectiveModel").
pub fn snapshot_state_in_process(s: &State) -> Result<State> {
    snapshot_state_with(s, false)
}

fn snapshot_state_with(s: &State, model_json: bool) -> Result<State> {
    let model = store::model(s)?;
    let mut snapshot = State::snapshot(&model);
    let out: &mut Map = &mut snapshot;
    if model_json {
        out.insert("effectiveModel".into(), Value::Object(model.json().into()));
    }
    out.insert("conflicts".into(), Value::Array(model.visible_conflicts().into()));
    out.insert(
        "revision".into(),
        s.get("revision").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "cursor".into(),
        s.get("cursor").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "eventCount".into(),
        Value::from(json::list_at(s, "events")?.len()),
    );
    out.insert(
        "pendingCount".into(),
        Value::from(json::list_at(s, "pending")?.len()),
    );
    for key in ["hidden", "undo", "redo", "selected", "page", "clipboard"] {
        if let Some(v) = s.get(key) {
            out.insert(key.into(), v.clone());
        }
    }
    Ok(snapshot)
}

fn int(v: Option<&Value>) -> Result<i32> {
    json::as_int(v)
}

fn page_of(entity: &Map) -> Result<i32> {
    json::as_int(Some(json::get_or_default(entity, "page", &Value::from(0))))
}

/// `Main.run(args)`: `Ok(None)` is Java's `null` result (help, `serve`).
pub fn run(args: &CommandArgs) -> Result<Option<CommandReply>> {
    run_command(args).map(|result| result.map(CommandReply))
}

fn run_command(args: &CommandArgs) -> Result<Option<Value>> {
    let mut a: Vec<String> = args.to_vec();
    if a.is_empty() || a.iter().any(|x| x == "--help") || (a.len() == 1 && a[0] == "help") {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(HELP.as_bytes());
        let _ = out.flush();
        return Ok(None);
    }
    let env = |k: &str| std::env::var(k).ok();
    let mut state = env("NOTEMAKER_STATE").unwrap_or_else(|| {
        let base = env("XDG_STATE_HOME").unwrap_or_else(|| {
            format!("{}/.local/state", env("HOME").unwrap_or_else(|| "?".into()))
        });
        format!("{}/notemaker", base)
    });
    let mut doc = "notes".to_string();
    let mut actor = env("NOTEMAKER_ACTOR")
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| "cli".into());
    let mut proxy = env("NOTEMAKER_PROXY_FILE");
    while !a.is_empty() && a[0].starts_with("--") {
        let flag = a.remove(0);
        require_msg(!a.is_empty(), format!("Missing {} value", flag))?;
        let value = a.remove(0);
        match flag.as_str() {
            "--state" => state = value,
            "--document" => doc = value,
            "--actor" => actor = value,
            "--proxy-file" => proxy = Some(value),
            _ => return Err(Error::illegal_argument(format!("Unknown option {}", flag))),
        }
    }
    require(!a.is_empty(), "Missing command")?;
    let document_name = DocumentName::from(&doc);
    let actor_id = ActorId::from(&actor);
    if a[0] == "serve" || in_proxy() {
        proxy = None;
    }
    let mut remote_model: Option<Model> = None;
    if let Some(proxy) = proxy {
        let bytes = std::fs::read(&proxy).map_err(|e| io_error(&proxy, e))?;
        let descriptor = EndpointDescriptor(map_of(json::parse(&String::from_utf8_lossy(&bytes))?)?);
        let mut value: Option<Value> = None;
        let cmd = a[0].clone();
        if cmd == "svg" {
            value = Some(Value::Object(svg_input(&a[1..])?));
            a = vec!["svg".into(), "add".into(), "-".into()];
        } else if cmd == "image" || cmd == "gif" {
            let mut placement = image_placement(&a)?;
            let gif = cmd == "gif";
            placement.insert(
                "shape".into(),
                Value::from(if gif { "gif" } else { "image" }),
            );
            let data = if gif {
                gif_asset::from_bytes(&asset_bytes(&a[2], gif_asset::MAX_BYTES)?)?
            } else {
                image_asset::from_bytes(&image_bytes(&a[2])?)?
            };
            placement.insert(
                (if gif { "gif" } else { "image" }).into(),
                Value::from(data),
            );
            value = Some(Value::Object(placement));
            a = vec![cmd.clone(), "add".into(), "-".into()];
        } else if (cmd == "selection"
            && a.len() == 3
            && ["select", "paste", "query"].contains(&a[1].as_str()))
            || (cmd == "transform"
                && a.len() == 3
                && ["preview", "commit"].contains(&a[1].as_str()))
        {
            let last = a.len() - 1;
            value = Some(input(&a[last])?);
            a[last] = "-".into();
        } else if cmd == "ui" && a.len() == 5 && a[1] == "action" {
            value = Some(input(&a[4])?);
            a[4] = "-".into();
        } else if [
            "append", "settings", "apply", "diagram", "erase", "fill", "template",
        ]
        .contains(&cmd.as_str())
            || (["object", "layer", "stroke", "image"].contains(&cmd.as_str())
                && a.iter().any(|x| x == "put" || x == "update" || x == "add"))
        {
            let last = a.len() - 1;
            value = Some(input(&a[last])?);
            a[last] = "-".into();
        }
        if cmd == "export" {
            remote_model = Some(Model::from_json((&map_of(LocalProxy::request(
                &descriptor,
                &document_name,
                &actor_id,
                &CommandArgs(vec!["inspect".to_string()]),
                None,
            )?.0)?).into())?);
        } else {
            return Ok(Some(LocalProxy::request(
                &descriptor,
                &document_name,
                &actor_id,
                &CommandArgs(a),
                value.as_ref().map(Into::into),
            )?.0));
        }
    }
    let command = a.remove(0);
    if command == "ui" {
        return Err(Error::illegal_argument(
            "UI commands require the running tablet's local --proxy-file",
        ));
    }
    let directory = ReplicaDir::new(&state);
    let store = if remote_model.is_none() {
        Some(if in_proxy() {
            Store::writer(&directory, &document_name)?
        } else {
            Store::new(&directory, &document_name)?
        })
    } else {
        None
    };
    let read_only = ["inspect", "snapshot", "status", "log", "export", "serve"]
        .contains(&command.as_str())
        || (command == "document" && a == ["list"])
        || (command == "shape" && a == ["list"]);
    require(read_only || in_proxy(), "Writes and synchronization require --proxy-file (or NOTEMAKER_PROXY_FILE); start the selected proxy with serve")?;
    let store = || store.as_ref().ok_or_else(Error::null_pointer);
    match command.as_str() {
        "serve" => {
            require(a.len() <= 2, "serve [PORT [SERVER_URL]]")?;
            let port = if a.is_empty() {
                Port::DEFAULT_PROXY
            } else {
                // Java parsed an int and failed at bind with this message.
                u16::try_from(jutil::parse_int(&a[0])?)
                    .map(Port::from)
                    .map_err(|_| Error::io("invalid port value"))?
            };
            let signals = block_shutdown_signals();
            let service = LocalProxy::new(&directory, port)?;
            spawn_shutdown_hook(service.clone(), signals);
            let served = (|| -> Result<()> {
                if a.len() == 2 {
                    service.configure(&document_name, Some(&a[1]))?;
                }
                let descriptor = absolute(&directory.join("proxy.json"));
                let mut out = std::io::stdout().lock();
                let _ = writeln!(
                    out,
                    "{}",
                    json::write(&Value::Object(
                        object! {"port" => service.port(), "descriptor" => descriptor.display().to_string()}
                    ))
                );
                let _ = out.flush();
                drop(out);
                service.await_();
                if SHUTDOWN_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0 {
                    // The shutdown hook is closing the proxy; it exits the
                    // process with 128+signal, like the JVM.
                    loop {
                        std::thread::park();
                    }
                }
                Ok(())
            })();
            let closed = service.close();
            served?;
            closed?;
            Ok(None)
        }
        "document" => {
            require(!a.is_empty(), "document create|delete|list")?;
            let sub = a.remove(0);
            if sub == "list" {
                arity(&a, 0)?;
                let mut documents = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&directory) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if !(name.starts_with("document-") && name.ends_with(".json")) {
                            continue;
                        }
                        let path = entry.path();
                        let bytes = std::fs::read(&path)
                            .map_err(|e| io_error(&path.display().to_string(), e))?;
                        let s = map_of(json::parse(&String::from_utf8_lossy(&bytes))?)?;
                        let doc_name = json::java_string(Some(
                            s.get("document")
                                .cloned()
                                .as_ref()
                                .unwrap_or(&Value::from(name.clone())),
                        ));
                        if !Store::new(&directory, &DocumentName::from(&doc_name))?.model()?.deleted {
                            documents.push(Value::from(doc_name));
                        }
                    }
                }
                return obj(object! {"documents" => documents});
            }
            if sub == "delete" {
                arity(&a, 0)?;
                return obj(store()?.delete_document(&actor_id)?);
            }
            require(
                sub == "create" && a.len() <= 1,
                "document create [fixed|pages|infinite]",
            )?;
            let type_ = if a.is_empty() {
                "fixed".to_string()
            } else {
                a[0].clone()
            };
            let mut op = object! {"kind" => "document", "type" => type_.clone()};
            Model::validate((&op).into())?;
            store()?.edit(&mut |s| {
                require(
                    json::list_at(s, "events")?.is_empty()
                        && json::list_at(s, "pending")?.is_empty(),
                    "Document already exists",
                )?;
                op.insert("id".into(), Value::from(String::from(ids::Uuid::random())));
                op.insert("actor".into(), Value::from(actor.clone()));
                json::list_at_mut(s, "pending")?.push(Value::Object(op.clone()));
                Ok(())
            })?;
            obj(object! {"document" => doc.clone(), "type" => type_})
        }
        "inspect" => {
            arity(&a, 0)?;
            obj(store()?.model()?.json())
        }
        "snapshot" => {
            arity(&a, 0)?;
            obj(snapshot(&store()?.read()?)?)
        }
        "append" => {
            let with_snapshot = a.len() == 2 && a[0] == "snapshot";
            if !with_snapshot {
                arity(&a, 1)?;
            }
            let s = store()?.append(&(map_of(input(&a[a.len() - 1])?)?).into(), &actor_id)?;
            let pending = json::list_at(&s, "pending")?;
            let last = pending.last().ok_or_else(|| {
                Error::new(
                    ErrorKind::IndexOutOfBounds,
                    "Index -1 out of bounds for length 0",
                )
            })?;
            let mut result =
                object! {"operation" => json::map(last)?.get("id").cloned().unwrap_or(Value::Null)};
            if with_snapshot {
                result.insert("snapshot".into(), Value::Object(snapshot(&s)?));
            }
            obj(result)
        }
        "settings" => {
            arity(&a, 1)?;
            let preferences = Map::from(&EditorPreferences::try_from(&map_of(input(&a[0])?)?)?);
            store()?.edit(&mut |s| {
                for (k, v) in &preferences {
                    s.insert(k.clone(), v.clone());
                }
                Ok(())
            })?;
            obj(object! {"saved" => true})
        }
        "status" => {
            arity(&a, 0)?;
            let s = store()?.read()?;
            obj(object! {
                "document" => doc.clone(), "deleted" => store::model(&s)?.deleted, "cursor" => s.get("cursor").cloned().unwrap_or(Value::Null),
                "pending" => json::list_at(&s, "pending")?.len(), "revision" => s.get("revision").cloned().unwrap_or(Value::Null), "serverId" => s.get("serverId").cloned().unwrap_or(Value::Null)
            })
        }
        "log" => {
            arity(&a, 0)?;
            obj(store()?.read()?.into_map())
        }
        "checkpoint" => {
            arity(&a, 0)?;
            obj(store()?.checkpoint()?)
        }
        "undo" | "redo" => {
            let with_snapshot = a.len() == 1 && a[0] == "snapshot";
            if !with_snapshot {
                arity(&a, 0)?;
            }
            let direction = if command == "redo" { HistoryDirection::Redo } else { HistoryDirection::Undo };
            if with_snapshot {
                let WithState { mut result, state } = store()?.history_with_state(&actor_id, direction)?;
                result.insert("snapshot".into(), Value::Object(snapshot(&state)?));
                obj(result)
            } else {
                obj(store()?.history(&actor_id, direction)?)
            }
        }
        "diagram" => {
            arity(&a, 2)?;
            require(a[0] == "add", "diagram add FILE|-")?;
            let diagram = map_of(input(&a[1])?)?;
            obj(store()?.commit(&actor_id, &mut |m| actions::diagram(m, (&diagram).into()))?)
        }
        "svg" => {
            let mut raw = svg_input(&a)?;
            raw.insert("shape".into(), Value::from("svg"));
            let drawing = actions::drawing((&raw).into(), &LayerId::base())?;
            obj(store()?.commit(&actor_id, &mut |m| {
                require(
                    !json::key_of(drawing.get("object"))
                        .is_some_and(|id| m.objects.contains_key(id)),
                    "Object ID already exists; use object update",
                )?;
                Ok((vec![Value::Object(drawing.clone().into())]).into())
            })?)
        }
        "shape" => {
            arity(&a, 1)?;
            require(a[0] == "list", "shape list")?;
            obj(object! {"shapes" => shape_catalog::json()})
        }
        "transform" => {
            if a.len() == 2 && a[0] == "cancel" {
                return obj(object! {"cancelled" => true, "session" => a[1].clone()});
            }
            require(
                a.len() == 2 && (a[0] == "preview" || a[0] == "commit"),
                "transform preview|commit FILE|-",
            )?;
            let request = map_of(input(&a[1])?)?;
            let session = SessionId::from(json::string(&request, "session")?);
            let entities = list_of(request.get("entities").cloned().unwrap_or(Value::Null))?;
            let transform = map_of(request.get("transform").cloned().unwrap_or(Value::Null))?;
            for key in transform.keys() {
                require_msg(
                    [
                        "scale", "dx", "dy", "rotation", "flipX", "flipY", "pivotX", "pivotY",
                    ]
                    .contains(&key.as_str()),
                    format!("Unknown transform field: {}", key),
                )?;
            }
            if a[0] == "preview" {
                return obj(object! {"preview" => true, "session" => session});
            }
            LocalProxy::assert_session(store()?, &actor_id, &session, (&entities).into())?;
            obj(store()?.commit_selected(
                &actor_id,
                &session,
                (&entities).into(),
                &mut |m| collaboration::transform(m, (&entities).into(), (&transform).into()),
                (&entities).into(),
            )?)
        }
        "conflicts" => {
            arity(&a, 0)?;
            obj(object! {"conflicts" => store()?.conflicts()?})
        }
        "conflict" => {
            require(
                a.len() == 3 && a[0] == "resolve",
                "conflict resolve ID local|remote|both",
            )?;
            let store = store()?;
            let choice = ConflictChoice::parse(&ConflictChoiceName::from(&a[2]))?;
            obj(store.conflict(&ConflictId::from(&a[1]), choice, &actor_id)?)
        }
        "erase" => {
            arity(&a, 1)?;
            let request = map_of(input(&a[0])?)?;
            obj(store()?.commit(&actor_id, &mut |m| {
                drawing_actions::erase(m, (&request).into(), &FACTORY)
            })?)
        }
        "fill" => {
            arity(&a, 1)?;
            let request = map_of(input(&a[0])?)?;
            let model = store()?.model()?;
            let mut dependencies = Vec::new();
            let page_value = request.get("page").cloned().unwrap_or(Value::from(0));
            if request.contains_key("object") {
                dependencies.push(Value::Object(object! {"kind" => "object", "id" => request.get("object").cloned().unwrap_or(Value::Null), "page" => page_value}));
            } else if request.contains_key("stroke") {
                dependencies.push(Value::Object(object! {"kind" => "stroke", "id" => request.get("stroke").cloned().unwrap_or(Value::Null), "page" => page_value}));
            } else {
                let page = layer_editor::page((&request).into())?;
                let layer = json::java_string(Some(json::get_or_default(
                    &request,
                    "layer",
                    &Value::from("base"),
                )));
                let layer_on = model.layer_active.get(&layer) != Some(&false);
                for entity in model.strokes.values() {
                    let id = json::java_string(entity.get("stroke"));
                    if page.0 == page_of(entity)?
                        && entity.get("layer").and_then(Value::as_str) == Some(layer.as_str())
                        && layer_on
                        && model.active.get(&id) != Some(&false)
                    {
                        dependencies.push(Value::Object(object! {"kind" => "stroke", "id" => entity.get("stroke").cloned().unwrap_or(Value::Null), "page" => page}));
                    }
                }
                for entity in model.objects.values() {
                    let id = json::java_string(entity.get("object"));
                    if page.0 == page_of(entity)?
                        && entity.get("layer").and_then(Value::as_str) == Some(layer.as_str())
                        && layer_on
                        && model.object_active.get(&id) != Some(&false)
                    {
                        dependencies.push(Value::Object(object! {"kind" => "object", "id" => entity.get("object").cloned().unwrap_or(Value::Null), "page" => page}));
                    }
                }
            }
            obj(store()?.commit_with_dependencies(
                &actor_id,
                &mut |m| drawing_actions::fill(m, (&request).into()),
                (&dependencies).into(),
            )?)
        }
        "template" => {
            arity(&a, 2)?;
            require(a[0] == "add", "template add FILE|-")?;
            let request = map_of(input(&a[1])?)?;
            obj(store()?.commit(&actor_id, &mut |m| drawing_actions::template(m, (&request).into()))?)
        }
        "image" | "gif" => {
            let gif = command == "gif";
            if in_proxy() {
                arity(&a, 2)?;
                let placement = map_of(input(&a[1])?)?;
                return obj(store()?.commit(&actor_id, &mut |_| {
                    Ok(vec![Value::from(actions::drawing((&placement).into(), &LayerId::base())?)].into())
                })?);
            }
            require(
                a.len() >= 2 && a[0] == "add",
                if gif {
                    "gif add FILE [PLACEMENT.json]"
                } else {
                    "image add FILE [PLACEMENT.json]"
                },
            )?;
            let mut placement = image_placement(&a)?;
            if gif {
                placement.insert("shape".into(), Value::from("gif"));
                placement.insert(
                    "gif".into(),
                    Value::from(gif_asset::from_bytes(&asset_bytes(
                        &a[1],
                        gif_asset::MAX_BYTES,
                    )?)?),
                );
                return obj(store()?.commit(&actor_id, &mut |_| {
                    Ok(vec![Value::from(actions::drawing((&placement).into(), &LayerId::base())?)].into())
                })?);
            }
            let bytes = image_bytes(&a[1])?;
            obj(store()?.commit(&actor_id, &mut |m| {
                drawing_actions::image(m, &bytes, (&placement).into())
            })?)
        }
        "apply" => {
            arity(&a, 1)?;
            let ops = list_of(input(&a[0])?)?;
            obj(store()?.commit(&actor_id, &mut |_| Ok(ops.clone().into()))?)
        }
        "selection" => selection(store()?, &actor_id, &mut a),
        "layer" | "object" | "stroke" => entity_command(store()?, &actor_id, &command, &mut a),
        "export" => {
            require(
                a.len() == 1 || a.len() == 2 || a.len() == 6,
                "export FILE [PAGE [X Y WIDTH HEIGHT]]",
            )?;
            let out = RenderOutputPath::new(&a[0]);
            let page = if a.len() > 1 {
                jutil::parse_int(&a[1])?
            } else {
                0
            };
            require((0..=10000).contains(&page), "Invalid page")?;
            let (mut x, mut y, mut w, mut h) = (0.0, 0.0, 1000.0, 1400.0);
            if a.len() == 6 {
                x = jfmt::parse_double(&a[2])?;
                y = jfmt::parse_double(&a[3])?;
                w = jfmt::parse_double(&a[4])?;
                h = jfmt::parse_double(&a[5])?;
            }
            require(
                x.is_finite()
                    && y.is_finite()
                    && w.is_finite()
                    && h.is_finite()
                    && w >= 1.0
                    && h >= 1.0
                    && w <= 8192.0
                    && h <= 8192.0
                    && w * h <= 16_000_000.0,
                "Invalid export viewport (max 16 million pixels)",
            )?;
            let model = match remote_model {
                Some(m) => m,
                None => store()?.model()?,
            };
            let name = out
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.ends_with(".svg") {
                let mut svg = render::Svg::new(DocRect { x, y, width: w, height: h });
                render::draw(&model, PageNumber(page), &HiddenLayers::default(), &mut svg)?;
                std::fs::write(&out, svg.to_string()).map_err(|e| io_error(&a[0], e))?;
            } else if name.ends_with(".png") {
                Png::export(
                    &model,
                    PageNumber(page),
                    DocPoint { x, y },
                    PixelSize { width: w as i32 as u32, height: h as i32 as u32 },
                    &out,
                )?;
            } else {
                return Err(Error::illegal_argument(
                    "Export filename must end in .svg or .png",
                ));
            }
            obj(object! {"file" => absolute(&out).display().to_string(), "page" => page})
        }
        _ => Err(Error::illegal_argument(format!(
            "Unknown command: {}",
            command
        ))),
    }
}

fn deactivate(entities: &[Value]) -> Result<OperationList> {
    let mut generated = Vec::new();
    for value in entities {
        let e = json::map(value)?;
        let kind = json::string(e, "kind")?;
        generated.push(Value::Object(object! {"kind" => if kind == "object" { "object-active" } else { "active" }, kind => json::string(e, "id")?, "active" => false}));
    }
    Ok(OperationList(generated))
}

fn selection(store: &Store, actor: &ActorId, a: &mut Vec<String>) -> Result<Option<Value>> {
    require(
        !a.is_empty(),
        "selection select|heartbeat|deselect|disconnect|duplicate|delete|copy|cut|paste",
    )?;
    let sub = a.remove(0);
    match sub.as_str() {
        "query" => {
            arity(a, 1)?;
            let request = map_of(input(&a[0])?)?;
            let m = store.model()?;
            let layer = match request.get("layer") {
                None => "base".to_string(),
                Some(v) => v
                    .as_str()
                    .ok_or_else(|| Error::class_cast("layer is not a String"))?
                    .to_string(),
            };
            let editor = LayerEditor::new(&m, &LayerId::from(&layer), layer_editor::page((&request).into())?)?;
            let region = match request.get("region") {
                Some(r) => Some(Polygon::try_from(r)?),
                None => None,
            };
            let hits = if request.get("topmost") == Some(&Value::Bool(true)) {
                HitMode::Topmost
            } else {
                HitMode::All
            };
            obj(object! {"items" => editor.select(region.as_ref(), hits, &FACTORY)?})
        }
        "select" => {
            arity(a, 1)?;
            let selection = map_of(input(&a[0])?)?;
            obj(LocalProxy::select(store, actor, (&selection).into())?)
        }
        "paste" => {
            arity(a, 1)?;
            let mut request = map_of(input(&a[0])?)?;
            if !request.contains_key("entities") {
                let state = store.read()?;
                let clipboard = json::map(json::get_or_default(
                    &state,
                    "clipboard",
                    &Value::Object(Map::new()),
                ))?
                .clone();
                let entities = json::list(json::get_or_default(
                    &clipboard,
                    "entities",
                    &Value::Array(Vec::new()),
                ))?
                .clone();
                request.insert("entities".into(), Value::Array(entities));
            }
            obj(store.commit(actor, &mut |m| drawing_actions::paste(m, (&request).into()))?)
        }
        "disconnect" => {
            arity(a, 0)?;
            obj(LocalProxy::release_actor(store, actor)?)
        }
        "copy" | "cut" => {
            arity(a, 1)?;
            let session = SessionId::from(&a[0]);
            let entities = LocalProxy::selected(store, actor, &session)?;
            LocalProxy::assert_session(store, actor, &session, &entities)?;
            let model = store.model()?;
            let snapshots = drawing_actions::copy_selection(&model, &entities)?;
            let copied_at = EpochMillis::now();
            store.edit(&mut |s| {
                s.insert("clipboard".into(), Value::Object(object! {"copiedAt" => copied_at, "entities" => json::copy(&Value::Array(snapshots.clone().into()))}));
                Ok(())
            })?;
            let mut cut_result = None;
            if sub == "cut" {
                cut_result = Some(store.commit_selected(
                    actor,
                    &session,
                    &entities,
                    &mut |_| deactivate(&entities),
                    &entities,
                )?);
                LocalProxy::release(store, actor, &session, LeaseUpdate::Release)?;
            }
            let mut response = object! {"copiedAt" => copied_at, "entities" => snapshots};
            if let Some(cut) = cut_result {
                response.insert(
                    "operation".into(),
                    cut.get("operation").cloned().unwrap_or(Value::Null),
                );
            }
            obj(response)
        }
        "duplicate" | "delete" => {
            arity(a, 1)?;
            let session = SessionId::from(&a[0]);
            let entities = LocalProxy::selected(store, actor, &session)?;
            LocalProxy::assert_session(store, actor, &session, &entities)?;
            if sub == "delete" {
                let result = store.commit_selected(
                    actor,
                    &session,
                    &entities,
                    &mut |_| deactivate(&entities),
                    &entities,
                )?;
                LocalProxy::release(store, actor, &session, LeaseUpdate::Release)?;
                return obj(result);
            }
            let result = store.commit_selected(
                actor,
                &session,
                &entities,
                &mut |m| {
                    let mut generated = Vec::new();
                    let mut ids: HashMap<String, String> = HashMap::new();
                    for value in &entities {
                        let e = json::map(value)?;
                        ids.insert(
                            format!("{}:{}", json::string(e, "kind")?, json::string(e, "id")?),
                            String::from(ids::Uuid::random()),
                        );
                    }
                    for value in &entities {
                        let e = json::map(value)?;
                        let (kind, id) = (json::string(e, "kind")?, json::string(e, "id")?);
                        let old = if kind == "object" {
                            m.objects.get(id)
                        } else {
                            m.strokes.get(id)
                        };
                        let old = old.ok_or_else(Error::null_pointer)?;
                        let mut n = (**old).clone();
                        n.insert(
                            kind.to_string(),
                            Value::from(ids[&format!("{}:{}", kind, id)].clone()),
                        );
                        if kind == "object" {
                            let (x, y) = (json::number(&n, "x")?, json::number(&n, "y")?);
                            n.insert("x".into(), json::double(x + 24.0));
                            n.insert("y".into(), json::double(y + 24.0));
                            for key in ["from", "to"] {
                                if let Some(target) = n
                                    .get(key)
                                    .map(|v| format!("object:{}", json::java_string(Some(v))))
                                {
                                    if let Some(new_id) = ids.get(&target) {
                                        n.insert(key.into(), Value::from(new_id.clone()));
                                    }
                                }
                            }
                        } else {
                            let mut points = Vec::new();
                            for p in json::list(n.get("points").unwrap_or(&Value::Null))? {
                                let xyz = json::list(p)?;
                                points.push(Value::Array(vec![
                                    json::double(json::as_double(xyz.first())? + 24.0),
                                    json::double(json::as_double(xyz.get(1))? + 24.0),
                                    xyz.get(2).cloned().ok_or_else(|| {
                                        Error::new(
                                            ErrorKind::IndexOutOfBounds,
                                            format!(
                                                "Index 2 out of bounds for length {}",
                                                xyz.len()
                                            ),
                                        )
                                    })?,
                                ]));
                            }
                            n.insert("points".into(), Value::Array(points));
                        }
                        generated.push(Value::Object(n.into()));
                    }
                    Ok(generated.into())
                },
                &entities,
            )?;
            obj(result)
        }
        _ => {
            arity(a, 1)?;
            obj(LocalProxy::release(
                store,
                actor,
                &SessionId::from(&a[0]),
                if sub == "heartbeat" { LeaseUpdate::Renew } else { LeaseUpdate::Release },
            )?)
        }
    }
}

fn entity_command(
    store: &Store,
    actor: &ActorId,
    kind: &str,
    a: &mut Vec<String>,
) -> Result<Option<Value>> {
    require(!a.is_empty(), "Missing subcommand")?;
    let sub = a.remove(0);
    if kind == "layer" && sub == "duplicate" {
        arity(a, 1)?;
        let id = a[0].clone();
        let model = store.model()?;
        let mut dependencies = Vec::new();
        for entity in model.objects.values() {
            if entity.get("layer").and_then(Value::as_str) == Some(id.as_str()) {
                dependencies.push(Value::Object(object! {"kind" => "object", "id" => entity.get("object").cloned().unwrap_or(Value::Null), "page" => json::get_or_default(entity, "page", &Value::from(0)).clone()}));
            }
        }
        for entity in model.strokes.values() {
            if entity.get("layer").and_then(Value::as_str) == Some(id.as_str()) {
                dependencies.push(Value::Object(object! {"kind" => "stroke", "id" => entity.get("stroke").cloned().unwrap_or(Value::Null), "page" => json::get_or_default(entity, "page", &Value::from(0)).clone()}));
            }
        }
        return obj(store.commit_with_dependencies(
            actor,
            &mut |m| drawing_actions::duplicate_layer(m, &LayerId::from(&id)),
            (&dependencies).into(),
        )?);
    }
    if kind == "layer" && sub == "merge" {
        arity(a, 2)?;
        let (source, target) = (a[0].clone(), a[1].clone());
        return obj(store.commit(actor, &mut |m| {
            drawing_actions::merge_layer(m, &LayerId::from(&source), &LayerId::from(&target))
        })?);
    }
    if kind == "layer" && sub == "reorder" {
        arity(a, 2)?;
        let id = a[0].clone();
        let index = jutil::parse_int(&a[1])?;
        return obj(store.commit(actor, &mut |m| {
            drawing_actions::reorder_layer(m, &LayerId::from(&id), LayerPosition(index))
        })?);
    }
    if sub == "hide" || sub == "show" {
        require(!a.is_empty() && a.len() <= 3, "hide/show ID [LAYER [PAGE]]")?;
        let id = a[0].clone();
        let scope_layer = if a.len() > 1 {
            a[1].clone()
        } else {
            "base".into()
        };
        let scope_page = if a.len() > 2 {
            jutil::parse_int(&a[2])?
        } else {
            0
        };
        return obj(store.commit(actor, &mut |m| {
            if kind != "layer" {
                LayerEditor::with_filter(
                    m,
                    &LayerId::from(&scope_layer),
                    PageNumber(scope_page),
                    ItemFilter::IncludeInactive,
                )?
                .resolve(&EntityKind::from(kind), &EntityId::from(&id))?;
            }
            let known = match kind {
                "object" => m.objects.contains_key(&id),
                "layer" => m.layers.contains_key(&id),
                _ => m.strokes.contains_key(&id),
            };
            require_msg(known, format!("Unknown {} ID", kind))?;
            let action_kind = if kind == "stroke" {
                "active".to_string()
            } else {
                format!("{}-active", kind)
            };
            Ok(OperationList(vec![Value::Object(
                object! {"kind" => action_kind, kind => id.clone(), "active" => sub == "show"},
            )]))
        })?);
    }
    if sub == "update" {
        require(
            kind == "object",
            "Only object update is supported; use put for other kinds",
        )?;
        arity(a, 2)?;
        let id = a[0].clone();
        let mut patch = map_of(input(&a[1])?)?;
        let scope = map_of(
            patch
                .get("scope")
                .cloned()
                .unwrap_or_else(|| Value::Object(object! {"layer" => "base", "page" => 0})),
        )?;
        patch.shift_remove("scope");
        require(
            !patch.contains_key("kind")
                && !patch.contains_key("object")
                && !patch.contains_key("id"),
            "Cannot patch identity",
        )?;
        return obj(store.commit(actor, &mut |m| {
            let layer = match scope.get("layer") {
                None => "base".to_string(),
                Some(v) => v
                    .as_str()
                    .ok_or_else(|| Error::class_cast("layer is not a String"))?
                    .to_string(),
            };
            let editor = LayerEditor::with_filter(
                m,
                &LayerId::from(&layer),
                layer_editor::page((&scope).into())?,
                ItemFilter::IncludeInactive,
            )?;
            let mut o = json::map(&json::copy(&Value::Object(
                ((*editor.resolve(&EntityKind::object(), &EntityId::from(&id))?.content).clone()).into(),
            )))?
            .clone();
            require(
                !patch.contains_key("layer")
                    || patch.get("layer").and_then(Value::as_str) == Some(&*editor.layer),
                "Use layer management or cut/paste to move layers",
            )?;
            for (k, v) in &patch {
                o.insert(k.clone(), v.clone());
            }
            Ok((vec![Value::Object(o)]).into())
        })?);
    }
    require(sub == "put", "Unknown subcommand")?;
    let with_snapshot = kind == "object" && a.len() == 2 && a[0] == "snapshot";
    if with_snapshot {
        a.remove(0);
    }
    arity(a, 1)?;
    let raw = map_of(input(&a[0])?)?;
    let mut o = if kind == "object" {
        actions::drawing((&raw).into(), &LayerId::base())?
    } else {
        (json::map(&json::copy(&Value::Object(raw)))?.clone()).into()
    };
    o.insert("kind".into(), Value::from(kind));
    o.shift_remove("id");
    let mut build = |m: &mut Model| -> Result<OperationList> {
        if kind != "layer" {
            let exists = json::key_of(o.get(kind)).is_some_and(|id| {
                if kind == "object" {
                    m.objects.contains_key(id)
                } else {
                    m.strokes.contains_key(id)
                }
            });
            if exists {
                LayerEditor::with_filter(
                    m,
                    &LayerId::from(json::string(&o, "layer")?),
                    PageNumber(int(o.get("page"))?),
                    ItemFilter::IncludeInactive,
                )?
                .resolve(&EntityKind::from(kind), &EntityId::from(json::string(&o, kind)?))?;
            }
        }
        Ok(OperationList(vec![Value::from(o.clone())]))
    };
    if with_snapshot {
        let WithState { mut result, state } = store.commit_with_state(actor, &mut build)?;
        result.insert("snapshot".into(), Value::Object(snapshot(&state)?));
        obj(result)
    } else {
        obj(store.commit(actor, &mut build)?)
    }
}

/// Signal received by the shutdown hook (0 = none).
static SHUTDOWN_SIGNAL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Signals that end `serve` (JVM shutdown-hook triggers).
const SHUTDOWN_SIGNALS: [i32; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

/// Block the shutdown signals in this thread before the proxy threads start,
/// so only the hook thread receives them.
fn block_shutdown_signals() -> libc::sigset_t {
    // SAFETY: sigset manipulation on a local set; pthread_sigmask on this thread.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in SHUTDOWN_SIGNALS {
            libc::sigaddset(&mut set, s);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    }
}

/// `Runtime.addShutdownHook(new Thread(close, "notemaker-proxy-shutdown"))`:
/// on SIGINT/SIGTERM/SIGHUP close the proxy, then exit with 128+signal like
/// the JVM.
fn spawn_shutdown_hook(proxy: std::sync::Arc<LocalProxy>, set: libc::sigset_t) {
    let _ = std::thread::Builder::new()
        .name("notemaker-proxy-shutdown".into())
        .spawn(move || {
            let mut signal: libc::c_int = 0;
            // SAFETY: waits on the signals blocked in every proxy thread.
            let rc = unsafe { libc::sigwait(&set, &mut signal) };
            if rc != 0 {
                return;
            }
            SHUTDOWN_SIGNAL.store(signal, std::sync::atomic::Ordering::SeqCst);
            let _ = proxy.close();
            let _ = std::io::stdout().flush();
            std::process::exit(128 + signal);
        });
}
