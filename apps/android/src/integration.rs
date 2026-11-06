//! Port of `Integration.java`: the instrumentation that drives the real
//! Activity and Canvas with pressure-bearing input events, plus the
//! `bridge` server used by the host test harness.
//!
//! The shim `com.notemaker.Integration extends Instrumentation` stores
//! `arguments` and forwards `onCreate`/`onStart`. There is one
//! instrumentation per process; the Rust instance lives in [`INSTANCE`].
//! `onStart` runs on the instrumentation thread (started by `start()`); UI
//! work goes through [`Integration::run_on_main_sync`].
//!
//! Java's `runOnMainSync` lambdas that threw crashed the main thread. Here the
//! main-thread closures return their `AppResult` to the instrumentation thread,
//! which reports it as `FAIL: ...` like every other check failure.

#![allow(clippy::too_many_arguments)]

use crate::android_consts::*;
use crate::components::ComponentEvent;
use crate::concurrent::CountDownLatch;
use crate::error::{AppError, AppResult};
use crate::field::Field;
use crate::editor_ui::ToolKind;
use crate::ink_cache::BitmapSize;
use crate::jx::{self, Arg, JResult, Obj, Reply, UptimeMillis};
use crate::main_activity::{MainActivity, ScreenPoint, StylusPressure};
use crate::network::{ProxyAddr, ServerAddr};
use notemaker_core::net::{ApiPath, BearerToken, HeaderName, HeaderValue, HttpMethod, HttpStatus, HttpUrl};
use crate::org_json::{JSONArray, JSONObject, JV};
use crate::{replica_service, ui};
use notemaker_core::json::{self, Map, Value};
use notemaker_core::object;
use notemaker_core::style::ArgbColor;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The process's instrumentation instance (created by `onCreate`).
pub static INSTANCE: Mutex<Option<Arc<Integration>>> = Mutex::new(None);

/// `public class Integration extends Instrumentation`.
pub struct Integration {
    /// The Java instrumentation (`this`).
    pub this: Obj,
    /// `Bundle arguments`.
    pub arguments: Field<Option<Obj>>,
}

/// Routes the shim's `onCreate(Bundle)` / `onStart()`.
pub fn component_event(event: &ComponentEvent) -> JResult<Reply> {
    let ComponentEvent { component, name, args } = *event;
    match &**name {
        "onCreate" => {
            let this = jx::global(component).expect("instrumentation");
            let test = Arc::new(Integration { this, arguments: Field::new(None) });
            *INSTANCE.lock().unwrap() = Some(test.clone());
            test.on_create(args.first().and_then(jx::global)).map_err(jx::JavaError::from)?;
        }
        "onStart" => {
            let test = Integration::instance();
            test.on_start().map_err(jx::JavaError::from)?;
        }
        other => return Err(jx::JavaError::new("java.lang.IllegalStateException", format!("Unknown Integration event {other}"))),
    }
    Ok(Reply::Void)
}

/// `MainActivity.obj(k1, v1, ...)` with `JV` conversions.
macro_rules! jo {
    ($($k:expr => $v:expr),* $(,)?) => {
        MainActivity::obj(&[$(($k, JV::from($v))),*])
    };
}

impl Integration {
    /// The running instrumentation.
    pub fn instance() -> Arc<Integration> {
        INSTANCE.lock().unwrap().clone().expect("Integration not created")
    }

    // -----------------------------------------------------------------------
    // Inherited `Instrumentation` methods used by the checks (JNI wrappers).

    /// `runOnMainSync(Runnable)`: runs `f` on the main thread and waits.
    /// A panic or error escaping `f` is thrown on the main thread, as an
    /// exception from a Java runnable was.
    pub fn run_on_main_sync<R: Send + 'static>(&self, f: impl FnOnce() -> R + Send) -> JResult<R> {
        let result: Arc<Mutex<Option<R>>> = Arc::new(Mutex::new(None));
        let out = result.clone();
        let job: Box<dyn FnOnce() + Send + '_> = Box::new(move || {
            let value = f();
            *out.lock().unwrap() = Some(value);
        });
        // Safety: runOnMainSync blocks until the runnable has run; the
        // closure is taken out (and dropped) below before this frame ends,
        // so it never outlives the borrows it captures.
        let job: Box<dyn FnOnce() + Send + 'static> = unsafe { std::mem::transmute(job) };
        let slot = Arc::new(Mutex::new(Some(job)));
        let runner = slot.clone();
        let runnable = jx::callback(move |_, _| {
            let job = runner.lock().unwrap().take();
            if let Some(job) = job {
                job();
            }
            Ok(Reply::Void)
        });
        let call = jcall!(self.this, "android/app/Instrumentation", "runOnMainSync", "(Ljava/lang/Runnable;)V", &runnable);
        drop(slot.lock().unwrap().take());
        call?;
        let value = result.lock().unwrap().take();
        value.ok_or_else(|| jx::JavaError::new("java.lang.IllegalStateException", "runOnMainSync task did not complete"))
    }
    /// `waitForIdleSync()`.
    pub fn wait_for_idle_sync(&self) -> JResult<()> {
        jcall!(self.this, "android/app/Instrumentation", "waitForIdleSync", "()V")?;
        Ok(())
    }
    /// `getTargetContext()`.
    pub fn target_context(&self) -> JResult<Obj> {
        Ok(jcall!(self.this, "android/app/Instrumentation", "getTargetContext", "()Landroid/content/Context;")?.some())
    }
    /// `startActivitySync(intent)`.
    pub fn start_activity_sync(&self, intent: &Obj) -> JResult<Obj> {
        Ok(jcall!(self.this, "android/app/Instrumentation", "startActivitySync", "(Landroid/content/Intent;)Landroid/app/Activity;", intent)?.some())
    }
    /// `finish(resultCode, results)`.
    fn finish(&self, result_code: i32, results: &Obj) -> JResult<()> {
        jcall!(self.this, "android/app/Instrumentation", "finish", "(ILandroid/os/Bundle;)V", result_code, results)?;
        Ok(())
    }
    /// `getUiAutomation()`.
    pub fn ui_automation(&self) -> JResult<Obj> {
        Ok(jcall!(self.this, "android/app/Instrumentation", "getUiAutomation", "()Landroid/app/UiAutomation;")?.some())
    }

    // -----------------------------------------------------------------------
    // Integration.java

    /// `static void liveGesture(MainActivity app, float shift, CountDownLatch done)`.
    fn live_gesture(app: &Arc<MainActivity>, shift: f32, done: Arc<CountDownLatch>) -> AppResult<()> {
        let handler = ui::main_handler()?;
        let count = 24;
        let index = Arc::new(AtomicI32::new(0));
        let start = jx::uptime_millis();
        // `Runnable[] step`: the runnable reposts itself; the slot is cleared
        // after the last step so the Java runnable no longer references itself.
        let step: Arc<Mutex<Option<Obj>>> = Arc::new(Mutex::new(None));
        let slot = step.clone();
        let app = app.clone();
        let posting = handler.clone();
        let runnable = jx::callback(move |_, _| {
            let i = index.fetch_add(1, Ordering::SeqCst);
            {
                let _lock = app.lock.enter();
                let canvas = app.canvas();
                let property = pointer_properties(PointerId(0), PointerTool::Stylus)?;
                let scale = canvas.scale().0;
                let point = ScreenPoint { x: ((100 + i * 18) as f32 + shift) * scale, y: ((200 + i * 12) as f32 + shift) * scale };
                let coords = pointer_coords(point, StylusPressure(0.2f32 + (i % 10) as f32 * 0.06f32), TouchSize(1.0))?;
                let action = if i == 0 {
                    MOTIONEVENT_ACTION_DOWN
                } else if i == count - 1 {
                    MOTIONEVENT_ACTION_UP
                } else {
                    MOTIONEVENT_ACTION_MOVE
                };
                let event = obtain(start, jx::uptime_millis(), MotionAction(action), &Pointers::single(property, coords), InputSource::Stylus)?;
                canvas.on_touch_event(&event)?;
                recycle(&event)?;
            }
            if index.load(Ordering::SeqCst) < count {
                let next = slot.lock().unwrap().clone();
                if let Some(next) = next {
                    ui::post_delayed(&posting, &next, crate::jx::DelayMillis(500))?;
                }
            } else {
                slot.lock().unwrap().take();
                done.count_down();
            }
            Ok(Reply::Void)
        });
        *step.lock().unwrap() = Some(runnable.clone());
        ui::post(&handler, &runnable)?;
        Ok(())
    }
    /// `static void gesture(MainActivity app, float x, float y, float w, float h, boolean cancel)` (UI thread):
    /// stylus down at `(x, y)`, one move to `(x + w, y + h)`, then up or cancel there.
    pub fn gesture(app: &Arc<MainActivity>, drag: GestureDrag, end: GestureEnd) -> AppResult<()> {
        let GestureDrag { x, y, width: w, height: h } = drag;
        let cancel = end == GestureEnd::Cancel;
        let start = jx::uptime_millis();
        for i in 0..3 {
            let action = if i == 0 {
                MOTIONEVENT_ACTION_DOWN
            } else if i == 1 {
                MOTIONEVENT_ACTION_MOVE
            } else if cancel {
                MOTIONEVENT_ACTION_CANCEL
            } else {
                MOTIONEVENT_ACTION_UP
            };
            let property = pointer_properties(PointerId(0), PointerTool::Stylus)?;
            let canvas = app.canvas();
            let point = ScreenPoint { x: (x + if i == 0 { 0.0 } else { w }) * canvas.scale().0, y: (y + if i == 0 { 0.0 } else { h }) * canvas.scale().0 };
            let coords = pointer_coords(point, StylusPressure(0.7), TouchSize(1.0))?;
            let event = obtain(start, UptimeMillis(start.0 + i * 20), MotionAction(action), &Pointers::single(property, coords), InputSource::Stylus)?;
            canvas.on_touch_event(&event)?;
            recycle(&event)?;
        }
        Ok(())
    }
    /// `static MotionEvent touch(long start, long time, int action, int count, float x0, float y0, float x1, float y1)`.
    fn touch(start: i64, time: i64, action: i32, count: i32, x0: f32, y0: f32, x1: f32, y1: f32) -> AppResult<Obj> {
        let mut pointers = Pointers { properties: Vec::new(), coords: Vec::new() };
        for i in 0..count {
            pointers.properties.push(pointer_properties(PointerId(i), PointerTool::Finger)?);
            pointers.coords.push(pointer_coords(ScreenPoint { x: if i == 0 { x0 } else { x1 }, y: if i == 0 { y0 } else { y1 } }, StylusPressure(1.0), TouchSize(1.0))?);
        }
        obtain(UptimeMillis(start), UptimeMillis(time), MotionAction(action), &pointers, InputSource::Touchscreen)
    }
    /// `static void copy(InputStream in, OutputStream out)`: an `IOException`
    /// becomes `RuntimeException(e)`.
    pub fn copy(input: &mut dyn std::io::Read, out: &mut dyn std::io::Write) -> AppResult<()> {
        let mut b = [0u8; 8192];
        loop {
            let n = match input.read(&mut b) {
                Ok(0) => return Ok(()),
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(AppError::runtime_cause(&AppError::from(e))),
            };
            out.write_all(&b[..n]).map_err(|e| AppError::runtime_cause(&AppError::from(e)))?;
        }
    }
    /// `public void onCreate(Bundle args)`: `arguments = args; start();`.
    pub fn on_create(&self, args: Option<Obj>) -> AppResult<()> {
        self.arguments.set(args);
        jcall!(self.this, "android/app/Instrumentation", "start", "()V")?;
        Ok(())
    }
    /// `public void onStart()` (instrumentation thread).
    pub fn on_start(self: &Arc<Self>) -> AppResult<()> {
        jx::log::i(crate::jx::log::PERFORMANCE, "Instrumentation starting");
        let mut result = new_bundle()?;
        // `catch (Throwable error)`: Rust panics are caught as well.
        let outcome = catch(|| -> AppResult<()> {
            let context = self.target_context()?;
            let class = jx::find_class("com/notemaker/MainActivity")?;
            let intent = jnew!("android/content/Intent", "(Landroid/content/Context;Ljava/lang/Class;)V", &context, &class)?;
            jcall!(intent, "android/content/Intent", "addFlags", "(I)Landroid/content/Intent;", INTENT_FLAG_ACTIVITY_NEW_TASK)?;
            let activity = self.start_activity_sync(&intent)?;
            let app = MainActivity::from_java(activity.as_obj());
            jx::log::i(crate::jx::log::PERFORMANCE, "Activity started");
            self.wait_for_idle_sync()?;
            let arguments = self.arguments.get().ok_or_else(null_pointer)?;
            if bundle_string(&arguments, "action")?.as_deref() == Some("bridge") {
                self.serve(&app)?;
            } else {
                result = self.run_action(&app, &arguments)?;
            }
            self.finish(ACTIVITY_RESULT_OK, &result)?;
            Ok(())
        });
        if let Err(error) = outcome {
            put_string(&result, "stream", &format!("FAIL: {}\n", error.stack_trace()))?;
            self.finish(ACTIVITY_RESULT_CANCELED, &result)?;
        }
        Ok(())
    }
    /// `private void serve(MainActivity app) throws Exception`: the bridge loop.
    pub fn serve(self: &Arc<Self>, app: &Arc<MainActivity>) -> AppResult<()> {
        let server = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let port = server.local_addr()?.port();
        write_file(app, "integration-bridge-port", port.to_string().as_bytes())?;
        loop {
            // `server.setSoTimeout(120000)`: accept() gives up after 120 s.
            accept_ready(&server, 120000)?;
            let (socket, _) = server.accept()?;
            let mut input = std::io::BufReader::new(socket.try_clone()?);
            let mut output = std::io::BufWriter::new(socket);
            let mut raw = Vec::new();
            loop {
                raw.clear();
                if input.read_until(b'\n', &mut raw)? == 0 {
                    break;
                }
                let line = read_line_text(&raw);
                let done = jx::frame(|| -> AppResult<bool> {
                    let request = JSONObject::parse(&line)?;
                    if request.opt_string("action") == "shutdown" {
                        output.write_all(b"{\"stream\":\"PASS shutdown\"}\n")?;
                        output.flush()?;
                        return Ok(true);
                    }
                    let command = new_bundle()?;
                    for key in request.keys() {
                        put_string(&command, &key, &request.get_string(&key)?)?;
                    }
                    let result = match catch(|| self.run_action(app, &command)) {
                        Ok(result) => result,
                        Err(error) => {
                            let result = new_bundle()?;
                            put_string(&result, "stream", &format!("FAIL: {}\n", error.stack_trace()))?;
                            result
                        }
                    };
                    let response = JSONObject::new();
                    for key in bundle_keys(&result)? {
                        response.put_opt(&key, bundle_string(&result, &key)?.map(JV::Str))?;
                    }
                    output.write_all(response.to_string().as_bytes())?;
                    output.write_all(b"\n")?;
                    output.flush()?;
                    Ok(false)
                })?;
                if done {
                    return Ok(());
                }
            }
        }
    }
    /// `private Bundle runAction(MainActivity app, Bundle arguments) throws Exception`.
    pub fn run_action(self: &Arc<Self>, app: &Arc<MainActivity>, arguments: &Obj) -> AppResult<Obj> {
        let result = new_bundle()?;
        let action = bundle_string_or(arguments, "action", "state")?;
        if action == "ui-tree" {
            let nodes = JSONArray::new();
            let root = jcall!(self.ui_automation()?, "android/app/UiAutomation", "getRootInActiveWindow", "()Landroid/view/accessibility/AccessibilityNodeInfo;")?.global();
            if let Some(root) = root {
                self.collect_nodes(&root, &nodes)?;
            }
            put_string(&result, "nodes", &nodes.to_string())?;
            put_string(&result, "stream", "PASS ui-tree\n")?;
            return Ok(result);
        }
        if action == "erase-performance" {
            let metrics = crate::erase_performance::run(self, app)?;
            put_string(&result, "stream", &format!("PASS erase-performance\n{}\n", json::write_map(&metrics)))?;
            return Ok(result);
        }
        if action == "interaction-performance" {
            let metrics = json::write_map(&(crate::interaction_performance::run(self, app)?).into());
            write_file(app, "interaction-performance.json", metrics.as_bytes())?;
            put_string(&result, "stream", "PASS interaction-performance\n")?;
            return Ok(result);
        }
        if action == "checkpoint-performance" {
            let metrics = json::write_map(&(crate::checkpoint_performance::run(app)?).into());
            write_file(app, "checkpoint-performance.json", metrics.as_bytes())?;
            put_string(&result, "stream", "PASS checkpoint-performance\n")?;
            return Ok(result);
        }
        if action == "stroke-pixels" {
            crate::stroke_check::run(self, app)?;
            put_string(&result, "stream", "PASS stroke-pixels\n")?;
            return Ok(result);
        }
        if action == "viewport" {
            crate::viewport_check::run(self, app)?;
            put_string(&result, "stream", "PASS viewport\n")?;
            return Ok(result);
        }
        if action == "coverage" {
            crate::coverage_check::run(self, app)?;
            put_string(&result, "stream", "PASS coverage\n")?;
            return Ok(result);
        }
        if action == "file-preview" {
            let snapshot = app.replica().read()?;
            let preview = crate::document_browser::DocumentBrowser::thumbnail(&snapshot)?;
            let ink = crate::android_canvas::bitmap_pixels(&preview)?.0.into_iter().filter(|p| *p != COLOR_WHITE).count();
            recycle_bitmap(&preview)?;
            if ink == 0 {
                return Err(AppError::illegal_state("Document preview contains no drawing"));
            }
            let revision = snapshot.get("revision").ok_or_else(null_pointer)?;
            if Some(revision) != app.replica().read()?.get("revision") {
                return Err(AppError::illegal_state("Preview changed document state"));
            }
            put_string(&result, "stream", "PASS file-preview\n")?;
            return Ok(result);
        }
        if action == "performance" {
            let metrics = json::write_map(&(crate::performance::run(self, app)?).into());
            write_file(app, "performance.json", metrics.as_bytes())?;
            put_string(&result, "stream", "PASS performance; metrics saved to performance.json\n")?;
            return Ok(result);
        }
        self.drain_writes()?;
        let mut child: Option<crate::app_replica::CliProcess> = None;
        if action == "cli" || action == "cli-concurrent" {
            let arguments_json = JSONArray::parse(&String::from_utf8_lossy(&base64_decode(bundle_string(arguments, "cli")?.as_deref())?))?;
            let mut cli_args = Vec::new();
            for i in 0..arguments_json.length() {
                cli_args.push(arguments_json.get_string(i)?);
            }
            let cli_args: Vec<&str> = cli_args.iter().map(String::as_str).collect();
            let mut started = app.start_cli(crate::app_replica::CliArgs(&cli_args))?;
            let input = base64_decode(Some(&bundle_string_or(arguments, "input", "")?))?;
            if let Some(mut stdin) = started.stdin.take() {
                stdin.write_all(&input)?;
            }
            child = Some(started);
        }
        let mut drawn_stroke: Option<String> = None;
        let mut max_gesture_points: i32 = 0;
        let live_done = Arc::new(CountDownLatch::new(1));
        self.run_on_main_sync(|| self.main_actions(app, arguments, &action, &mut drawn_stroke, &mut max_gesture_points, &live_done))??;
        if action == "live-draw" {
            if !live_done.await_timeout(Duration::from_secs(20)) {
                return Err(AppError::illegal_state("Timed out delivering live stylus gesture"));
            }
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
        }
        if action == "draw" {
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            let completed = drawn_stroke.as_ref().and_then(|id| app.strokes.with(|s| s.get(id).cloned()));
            let stored = completed.and_then(|c| c.opt_json_array("points")).map(|p| p.length()).unwrap_or(0);
            if bundle_string_or(arguments, "tool", "stylus")? == "finger" {
                if drawn_stroke.is_some() || max_gesture_points != 0 || stored != 0 {
                    return Err(AppError::illegal_state("Finger input created a pen stroke"));
                }
            } else {
                // ACTION_UP appends the final sample and clears the in-flight list, so the
                // last observable in-gesture length is 11 while the persisted stroke has 12.
                if max_gesture_points < 11 || stored < 12 {
                    return Err(AppError::illegal_state(format!(
                        "Canvas stroke lost move samples: observed={} stored={} stroke={}",
                        max_gesture_points,
                        stored,
                        drawn_stroke.as_deref().unwrap_or("null")
                    )));
                }
            }
        }
        if action == "remote-preview" {
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            let revision_before = app.revision.get();
            let undo_before = app.undo.get();
            let before = self.capture_canvas(app)?;
            let session = "55555555-5555-4555-8555-555555555555";
            let entity = object! {"kind" => "object", "id" => "remote-preview-object", "page" => app.page.get()};
            let proxy = replica_service::ensure(&app.this)?;
            proxy.execute(&notemaker_cli::local_proxy::CommandRequest(object! {"document" => app.document.get(), "actor" => "remote-editor", "args" => vec!["selection", "select", "-"], "input" => object! {"session" => session, "page" => app.page.get(), "entities" => vec![Value::Object(entity.clone())]}}))?;
            let transform = object! {"session" => session, "entities" => vec![Value::Object(entity.clone())], "transform" => object! {"scale" => 1, "dx" => 180, "dy" => 40}};
            proxy.execute(&notemaker_cli::local_proxy::CommandRequest(object! {"document" => app.document.get(), "actor" => "remote-editor", "args" => vec!["transform", "preview", "-"], "input" => transform}))?;
            self.wait_until(&|| Ok(app.transform_previews.with(|p| p.contains_key(session))), 10000, "foreign transform preview notification")?;
            self.wait_for_idle_sync()?;
            let previewed = self.capture_canvas(app)?;
            if !self.images_differ(&before, &previewed)? {
                return Err(AppError::illegal_state("Foreign preview updated metadata but did not render a visual transform"));
            }
            if app.revision.get() != revision_before || app.undo.get() != undo_before {
                return Err(AppError::illegal_state("Ephemeral foreign preview changed revision or undo history"));
            }
            proxy.execute(&notemaker_cli::local_proxy::CommandRequest(object! {"document" => app.document.get(), "actor" => "remote-editor", "args" => vec!["transform", "cancel", session]}))?;
            self.wait_until(&|| Ok(!app.transform_previews.with(|p| p.contains_key(session))), 10000, "foreign preview cancellation notification")?;
            self.wait_for_idle_sync()?;
            let cancelled = self.capture_canvas(app)?;
            if self.images_differ(&before, &cancelled)? {
                return Err(AppError::illegal_state("Cancelled foreign preview did not restore committed canvas pixels"));
            }
            if app.revision.get() != revision_before || app.undo.get() != undo_before {
                return Err(AppError::illegal_state("Foreign preview cancel changed revision or undo history"));
            }
            proxy.execute(&notemaker_cli::local_proxy::CommandRequest(object! {"document" => app.document.get(), "actor" => "remote-editor", "args" => vec!["selection", "deselect", session]}))?;
            recycle_bitmap(&before)?;
            recycle_bitmap(&previewed)?;
            recycle_bitmap(&cancelled)?;
        }
        if action == "locked-undo" {
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            self.wait_until(&|| Ok(app.pending_object_history.with(|p| p.is_empty())), 10000, "object history save")?;
            let undo_before = app.undo.get();
            let redo_before = app.redo.get();
            let model_before = json::write_map(&app.model().json());
            let session = "66666666-6666-4666-8666-666666666666";
            let entity = object! {"kind" => "object", "id" => "locked-undo-object", "page" => app.page.get()};
            let proxy = replica_service::ensure(&app.this)?;
            proxy.execute(&notemaker_cli::local_proxy::CommandRequest(object! {"document" => app.document.get(), "actor" => "remote-editor", "args" => vec!["selection", "select", "-"], "input" => object! {"session" => session, "page" => app.page.get(), "entities" => vec![Value::Object(entity)]}}))?;
            self.run_on_main_sync(|| app.history(notemaker_core::store::HistoryDirection::Undo))??;
            self.wait_until(&|| Ok(!app.history_pending.get()), 10000, "rejected locked undo")?;
            if app.undo.get() != undo_before || app.redo.get() != redo_before || model_before != json::write_map(&app.model().json()) {
                return Err(AppError::illegal_state("Rejected locked undo changed model or local history"));
            }
            proxy.execute(&notemaker_cli::local_proxy::CommandRequest(object! {"document" => app.document.get(), "actor" => "remote-editor", "args" => vec!["selection", "deselect", session]}))?;
            self.run_on_main_sync(|| app.history(notemaker_core::store::HistoryDirection::Undo))??;
            self.wait_until(&|| Ok(!app.history_pending.get() && app.undo.with(|u| u.len()) as i64 == undo_before.len() as i64 - 1), 10000, "unlocked undo")?;
            if app.model().object_active.get("locked-undo-object") != Some(&false) || app.redo.with(|r| r.len()) != redo_before.len() + 1 {
                return Err(AppError::illegal_state("Undo did not succeed after foreign lease release"));
            }
        }
        if action == "conflict-preview" {
            self.wait_for_idle_sync()?;
        }
        if action == "locked-selection" {
            if bundle_contains(arguments, "cli")? {
                let arguments_json = JSONArray::parse(&String::from_utf8_lossy(&base64_decode(bundle_string(arguments, "cli")?.as_deref())?))?;
                let mut cli_args = Vec::new();
                for i in 0..arguments_json.length() {
                    cli_args.push(Value::String(arguments_json.get_string(i)?));
                }
                let input = json::parse(&String::from_utf8_lossy(&base64_decode(Some(&bundle_string_or(arguments, "input", "")?))?))?;
                replica_service::ensure(&app.this)?.execute(&notemaker_cli::local_proxy::CommandRequest(object! {"document" => app.document.get(), "actor" => "remote-editor", "args" => cli_args, "input" => input}))?;
            }
            let object = bundle_string_or(arguments, "object", "locked-object")?;
            self.run_on_main_sync(|| app.start_selection(vec![Value::Object(object! {"kind" => "object", "id" => object.as_str()})]))??;
            self.wait_for_selection_condition(app, &|| Ok(app.selection_skipped.get() > 0), "locked selection result")?;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            if !app.selected_entities.with(|s| s.is_empty()) || app.selection_skipped.get() != 1 {
                return Err(AppError::illegal_state("Locked selection was acquired instead of skipped"));
            }
            self.run_on_main_sync(|| app.deselect())??;
            self.drain_writes()?;
        }
        if action == "clipboard" {
            self.run_on_main_sync(|| app.enqueue(&jo! {"kind" => "object", "object" => "clipboard-object", "shape" => "rect", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 120, "y" => 120, "w" => 80, "h" => 60, "color" => COLOR_BLACK, "width" => 4, "fontSize" => 24, "text" => ""}))??;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            self.run_on_main_sync(|| app.start_selection(vec![Value::Object(object! {"kind" => "object", "id" => "clipboard-object"})]))??;
            self.wait_for_selection(app, 1, "clipboard selection")?;
            self.run_on_main_sync(|| app.copy_selection())??;
            self.wait_until(
                &|| {
                    app.selection_clipboard.with(|c| match c.get("entities") {
                        None => Ok(false),
                        Some(v) => Ok(!json::list(v)?.is_empty()),
                    })
                },
                10000,
                "selection copied to proxy clipboard",
            )?;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            self.run_on_main_sync(|| app.deselect())??;
            self.drain_writes()?;
        }
        if action == "lasso" {
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            self.run_on_main_sync(|| {
                app.selecting.set(true);
                Integration::gesture(app, GestureDrag { x: 50.0, y: 50.0, width: 500.0, height: 500.0 }, GestureEnd::Up)
            })??;
            self.wait_for_selection_condition(app, &|| Ok(app.selected_entities.with(|s| s.len()) == 2), "lasso selection")?;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            let selected = app.selected_entities.get();
            let mut object_selected = false;
            let mut stroke_selected = false;
            for entity in &selected {
                object_selected |= id_is(entity, "lasso-object");
                stroke_selected |= id_is(entity, "lasso-stroke");
            }
            if !object_selected || !stroke_selected {
                return Err(AppError::illegal_state(format!("Lasso missed object or stroke: {}", maps_string(&selected))));
            }
            self.run_on_main_sync(|| app.deselect())??;
            self.drain_writes()?;
            self.run_on_main_sync(|| app.select_at(crate::main_activity::DocPointF { x: 400.0, y: 250.0 }))??;
            self.wait_for_selection_condition(app, &|| Ok(app.selected_entities.with(|s| s.len()) == 1), "stroke midpoint selection")?;
            let selected = app.selected_entities.get();
            if !id_is(first(&selected)?, "lasso-stroke") {
                return Err(AppError::illegal_state(format!("Stroke midpoint hit test failed: {}", maps_string(&selected))));
            }
            self.run_on_main_sync(|| app.deselect())??;
            self.drain_writes()?;
        }
        if action == "freeform-lasso" {
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            let start = jx::uptime_millis().0;
            let path: [[f32; 2]; 4] = [[100.0, 100.0], [300.0, 100.0], [100.0, 300.0], [100.0, 100.0]];
            self.run_on_main_sync(|| -> AppResult<()> {
                app.selecting.set(true);
                for (i, point) in path.iter().enumerate() {
                    let mode = if i == 0 {
                        MOTIONEVENT_ACTION_DOWN
                    } else if i == path.len() - 1 {
                        MOTIONEVENT_ACTION_UP
                    } else {
                        MOTIONEVENT_ACTION_MOVE
                    };
                    let canvas = app.canvas();
                    let event = jstatic!("android/view/MotionEvent", "obtain", "(JJIFFI)Landroid/view/MotionEvent;", start, start + i as i64 * 40, mode, point[0] * canvas.scale().0, point[1] * canvas.scale().0, 0)?.some();
                    canvas.on_touch_event(&event)?;
                    recycle(&event)?;
                }
                Ok(())
            })??;
            self.wait_for_selection_condition(app, &|| Ok(app.selected_entities.with(|s| s.len()) == 1), "freeform lasso selection")?;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            let selected = app.selected_entities.get();
            if !id_is(first(&selected)?, "lasso-inside") {
                return Err(AppError::illegal_state(format!("Freeform lasso selected outside its polygon: {}", maps_string(&selected))));
            }
            self.run_on_main_sync(|| app.deselect())??;
            self.drain_writes()?;
        }
        if action == "layer-order" {
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            self.run_on_main_sync(|| -> AppResult<()> {
                app.refresh_shared(&app.replica().read()?)?;
                app.update()
            })??;
            self.wait_for_idle_sync()?;
            let first_image = self.capture_canvas(app)?;
            let before = [self.canvas_pixel(app, &first_image, 160.0, 150.0)?, self.canvas_pixel(app, &first_image, 550.0, 550.0)?];
            let (top, bottom) = (ArgbColor(before[0]).channels(), ArgbColor(before[1]).channels());
            if top.red < 220 || top.green > 80 || top.blue > 80 || bottom.red > 60 || bottom.green > 60 || bottom.blue > 60 {
                return Err(AppError::illegal_state(format!("Template/layer paint order is incorrect: [{}, {}]", before[0], before[1])));
            }
            let proxy = replica_service::ensure(&app.this)?;
            proxy.execute(&notemaker_cli::local_proxy::CommandRequest(object! {"document" => app.document.get(), "actor" => "human", "args" => vec!["layer", "reorder", "order-high", "0"]}))?;
            let state = app.replica().read()?;
            self.run_on_main_sync(|| -> AppResult<()> {
                app.refresh_shared(&state)?;
                app.update()
            })??;
            self.wait_for_idle_sync()?;
            let second = self.capture_canvas(app)?;
            let after = self.canvas_pixel(app, &second, 160.0, 150.0)?;
            let after = ArgbColor(after).channels();
            if after.blue < 220 || after.red > 80 {
                return Err(AppError::illegal_state(format!("Layer reorder did not update Android compositing: {},{},{}", after.red, after.green, after.blue)));
            }
            recycle_bitmap(&first_image)?;
            recycle_bitmap(&second)?;
        }
        if action == "tool-switch-selection" {
            self.tool_switch_selection(app)?;
        }
        if action == "select-panel" {
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            self.run_on_main_sync(|| app.start_selection(vec![Value::Object(object! {"kind" => "object", "id" => "lasso-object"})]))??;
            self.wait_for_selection(app, 1, "selection panel lease")?;
            self.run_on_main_sync(|| app.editor().show_selection_panel())??;
            self.wait_for_idle_sync()?;
        }
        if action == "collaboration" {
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            self.run_on_main_sync(|| app.start_selection(vec![Value::Object(object! {"kind" => "object", "id" => "collaboration-object"})]))??;
            self.wait_for_selection(app, 1, "selection lease")?;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            let _session = app.selection_session.get();
            let specification = object! {"scale" => 1.5, "dx" => 20, "dy" => 30, "rotation" => 30, "flipX" => true};
            self.run_on_main_sync(|| app.preview_transform((&specification).into()))??;
            self.wait_until(&|| Ok(!app.transform_previews.with(|p| p.is_empty())), 10000, "transform preview")?;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            if !app.replica().read()?.contains_key("previews") {
                return Err(AppError::illegal_state("Snapshot omitted transform previews"));
            }
            self.run_on_main_sync(|| app.cancel_transform_preview())??;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            let x = {
                let model = app.model();
                let object = model.objects.get("collaboration-object").cloned().ok_or_else(null_pointer)?;
                json::as_double(object.get("x"))?
            };
            if (x - 120.0).abs() > 0.01 || !app.transform_previews.with(|p| p.is_empty()) {
                return Err(AppError::illegal_state("Canceled preview changed durable state"));
            }
            self.run_on_main_sync(|| app.preview_transform((&specification).into()))??;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            self.run_on_main_sync(|| app.commit_transform_preview())??;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            let transformed = app.model().objects.get("collaboration-object").cloned();
            let committed = match &transformed {
                None => false,
                Some(t) => (json::as_double(t.get("rotation"))? - 30.0).abs() <= 0.01 && t.get("flipX") == Some(&Value::Bool(true)),
            };
            if !committed {
                return Err(AppError::illegal_state(format!(
                    "Transform did not commit rotation/flip: {}",
                    json::java_string(transformed.map(|t| Value::Object(((*t).clone()).into())).as_ref())
                )));
            }
            self.run_on_main_sync(|| app.duplicate_selection())??;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            if app.model().objects.len() < 2 {
                return Err(AppError::illegal_state("Selection duplicate did not create an object"));
            }
            self.run_on_main_sync(|| app.delete_selection())??;
            self.drain_writes()?;
            self.wait_for_idle_sync()?;
            if app.model().object_active.get("collaboration-object") != Some(&false) {
                return Err(AppError::illegal_state("Selection delete did not deactivate original"));
            }
            self.run_on_main_sync(|| app.deselect())??;
            self.drain_writes()?;
        }
        if let Some(mut process) = child {
            let mut stdout = process.stdout.take();
            let mut stderr = process.stderr.take();
            let output_thread = std::thread::spawn(move || {
                let mut output = Vec::new();
                if let Some(s) = stdout.as_mut() {
                    if let Err(e) = Integration::copy(s, &mut output) {
                        jx::log::e_err(crate::jx::log::NOTEMAKER, "CLI stdout copy failed", &e);
                    }
                }
                output
            });
            let errors_thread = std::thread::spawn(move || {
                let mut errors = Vec::new();
                if let Some(s) = stderr.as_mut() {
                    if let Err(e) = Integration::copy(s, &mut errors) {
                        jx::log::e_err(crate::jx::log::NOTEMAKER, "CLI stderr copy failed", &e);
                    }
                }
                errors
            });
            // `child.waitFor(30, SECONDS)`.
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            let status = loop {
                if let Some(status) = process.try_wait()? {
                    break status;
                }
                if std::time::Instant::now() >= deadline {
                    let _ = process.kill();
                    return Err(AppError::runtime("CLI timed out"));
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            let output = output_thread.join().unwrap_or_default();
            let errors = errors_thread.join().unwrap_or_default();
            if status.code() != Some(0) {
                return Err(AppError::runtime(format!("CLI failed: {}", String::from_utf8_lossy(&errors))));
            }
            write_file(app, "cli-result.json", &output)?;
            let text = if output.len() < 32000 { String::from_utf8_lossy(&output).into_owned() } else { "See files/cli-result.json".to_string() };
            put_string(&result, "cli", &text)?;
        }
        if action == "conflict-preview" {
            self.make_conflict_fixture(app)?;
            let snapshot = app.replica().read()?;
            let conflicts = match snapshot.get("conflicts") {
                None => 0,
                Some(v) => json::list(v)?.len(),
            };
            if conflicts != 1 {
                return Err(AppError::illegal_state(format!("Proxy ingress did not materialize the expected real conflict: {}", json::java_string(snapshot.get("conflicts")))));
            }
            self.run_on_main_sync(|| -> AppResult<()> {
                app.refresh_shared(&snapshot)?;
                app.update()?;
                app.editor().show_conflict_panel()
            })??;
            self.wait_for_idle_sync()?;
        }
        self.drain_writes()?;
        std::thread::sleep(Duration::from_millis(50));
        self.run_on_main_sync(|| {
            let _lock = app.lock.enter();
            self.report_state(app, arguments, &action, &result)
        })??;
        put_string(&result, "stream", &format!("PASS {action}\n"))?;
        Ok(result)
    }

    /// The `runOnMainSync(() -> { synchronized(app.lock) {...} ... })` block
    /// of `runAction` (UI thread).
    fn main_actions(&self, app: &Arc<MainActivity>, arguments: &Obj, action: &str, drawn_stroke: &mut Option<String>, max_gesture_points: &mut i32, live_done: &Arc<CountDownLatch>) -> AppResult<()> {
        {
            let _lock = app.lock.enter();
            if action == "configure" {
                app.save();
                app.document.set(bundle_string_or(arguments, "document", "integration")?);
                app.host.set(ServerAddr::from(bundle_string_or(arguments, "host", "http://10.0.2.2:8765")?.as_str()));
                app.proxy_url.set(ProxyAddr::from(bundle_string_or(arguments, "proxyUrl", "")?.as_str()));
                app.proxy_token.set(BearerToken::from(bundle_string_or(arguments, "proxyToken", "")?));
                app.load(&crate::main_activity::DocumentType::from(bundle_string_or(arguments, "type", "fixed")?))?;
                app.configure_proxy()?;
                let editor = ui::edit(&app.this)?;
                ui::put_string(&editor, &ui::PrefKey::from("host"), &ui::PrefText::from(app.host.get().to_string()))?;
                ui::put_string(&editor, &ui::PrefKey::from("document"), &ui::PrefText::from(&app.document.get()))?;
                ui::put_string(&editor, &ui::PrefKey::from("proxyUrl"), &ui::PrefText::from(app.proxy_url.get().to_string()))?;
                ui::put_string(&editor, &ui::PrefKey::from("proxyToken"), &ui::PrefText::from(&*app.proxy_token.get()))?;
                jcall!(editor, "android/content/SharedPreferences$Editor", "commit", "()Z")?;
            }
            if action == "live-draw" {
                Integration::live_gesture(app, parse_float(&bundle_string_or(arguments, "shift", "0")?)?, live_done.clone())?;
            }
            if action == "draw" || action == "cli-concurrent" {
                app.set_drawing_tool(None)?;
                app.color.set(parse_int(&bundle_string_or(arguments, "color", "-16777216")?)?);
                app.width.set(10.0);
                app.erase.set(false);
                app.pan.set(false);
                let shift = parse_float(&bundle_string_or(arguments, "shift", "0")?)?;
                let canvas = app.canvas();
                let scale = canvas.scale().0;
                let start = jx::uptime_millis().0;
                let count = 12;
                let finger = bundle_string_or(arguments, "tool", "stylus")? == "finger";
                for i in 0..count {
                    let tool_type = if finger { PointerTool::Finger } else { PointerTool::Stylus };
                    let property = pointer_properties(PointerId(0), tool_type)?;
                    let point = ScreenPoint { x: ((100 + i * 30) as f32 + shift) * scale, y: ((200 + i * 20) as f32 + shift) * scale };
                    let coords = pointer_coords(point, StylusPressure(0.2f32 + i as f32 * 0.06f32), TouchSize(1.0))?;
                    let event_action = if i == 0 {
                        MOTIONEVENT_ACTION_DOWN
                    } else if i == count - 1 {
                        MOTIONEVENT_ACTION_UP
                    } else {
                        MOTIONEVENT_ACTION_MOVE
                    };
                    let source = if tool_type == PointerTool::Finger { InputSource::Touchscreen } else { InputSource::Stylus };
                    let event = obtain(UptimeMillis(start), UptimeMillis(start + i as i64 * 20), MotionAction(event_action), &Pointers::single(property, coords), source)?;
                    canvas.on_touch_event(&event)?;
                    if i == 0 {
                        *drawn_stroke = canvas.current.get();
                    }
                    if let Some(points) = canvas.points.get() {
                        *max_gesture_points = (*max_gesture_points).max(points.length() as i32);
                    }
                    recycle(&event)?;
                }
            }
            if action == "shape" {
                app.set_drawing_tool(Some(&notemaker_core::ids::ShapeId::from(bundle_string_or(arguments, "shape", "triangle")?)))?;
                app.object_fill_color.set(COLOR_LTGRAY);
                let x = parse_float(&bundle_string_or(arguments, "x", "100")?)?;
                let y = parse_float(&bundle_string_or(arguments, "y", "100")?)?;
                let w = parse_float(&bundle_string_or(arguments, "w", "160")?)?;
                let h = parse_float(&bundle_string_or(arguments, "h", "120")?)?;
                let end = if parse_boolean(&bundle_string_or(arguments, "cancel", "false")?)? { GestureEnd::Cancel } else { GestureEnd::Up };
                Integration::gesture(app, GestureDrag { x, y, width: w, height: h }, end)?;
                app.set_drawing_tool(None)?;
            }
            if action == "fill-tap" {
                app.set_drawing_tool(None)?;
                app.object_fill_color.set(COLOR_RED);
                app.enqueue(&jo! {"kind" => "object", "object" => String::from(MainActivity::id()), "shape" => "rect", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 100, "y" => 100, "w" => 240, "h" => 180, "color" => COLOR_BLACK, "width" => 5, "fontSize" => 24, "text" => ""})?;
                app.fill_tap_mode.set(true);
                Integration::gesture(app, GestureDrag { x: 180.0, y: 160.0, width: 0.0, height: 0.0 }, GestureEnd::Up)?;
            }
            if action == "collaboration" {
                app.set_drawing_tool(None)?;
                app.enqueue(&jo! {"kind" => "object", "object" => "collaboration-object", "shape" => "rect", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 120, "y" => 140, "w" => 160, "h" => 120, "color" => COLOR_BLACK, "width" => 5, "fontSize" => 24, "text" => ""})?;
            }
            if action == "remote-preview" {
                app.set_drawing_tool(None)?;
                app.enqueue(&jo! {"kind" => "object", "object" => "remote-preview-object", "shape" => "rect", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 100, "y" => 120, "w" => 120, "h" => 90, "color" => COLOR_BLACK, "width" => 5, "fontSize" => 24, "text" => ""})?;
            }
            if action == "locked-undo" {
                app.persist_object(&jo! {"kind" => "object", "object" => "locked-undo-object", "shape" => "rect", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 100, "y" => 120, "w" => 120, "h" => 90, "color" => COLOR_BLACK, "width" => 5, "fontSize" => 24, "text" => ""})?;
            }
            if action == "lasso" {
                app.set_drawing_tool(None)?;
                app.enqueue(&jo! {"kind" => "object", "object" => "lasso-object", "shape" => "rect", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 100, "y" => 100, "w" => 150, "h" => 150, "color" => COLOR_BLACK, "width" => 5, "fontSize" => 24, "text" => ""})?;
                app.enqueue(&jo! {"kind" => "stroke", "stroke" => "lasso-stroke", "layer" => app.selected.get(), "page" => app.page.get(), "color" => COLOR_BLACK, "width" => 5, "brush" => "pen", "points" => points(&[[400, 100, 1], [400, 400, 1]])?})?;
            }
            if action == "freeform-lasso" {
                app.set_drawing_tool(None)?;
                app.freeform_lasso.set(true);
                app.enqueue(&jo! {"kind" => "object", "object" => "lasso-inside", "shape" => "rect", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 130, "y" => 130, "w" => 20, "h" => 20, "color" => COLOR_BLACK, "width" => 1, "fontSize" => 24, "text" => ""})?;
                app.enqueue(&jo! {"kind" => "object", "object" => "lasso-outside-bounds", "shape" => "rect", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 270, "y" => 270, "w" => 20, "h" => 20, "color" => COLOR_BLACK, "width" => 1, "fontSize" => 24, "text" => ""})?;
            }
            if action == "layer-order" {
                app.set_drawing_tool(None)?;
                app.enqueue(&jo! {"kind" => "layer", "layer" => "template-0", "name" => "Page template", "force" => false, "order" => -10})?;
                app.enqueue(&jo! {"kind" => "layer", "layer" => "order-low", "name" => "Blue below red", "force" => false, "order" => -2})?;
                app.enqueue(&jo! {"kind" => "layer", "layer" => "order-high", "name" => "Red above blue", "force" => false, "order" => 2})?;
                app.enqueue(&jo! {"kind" => "object", "object" => "order-template-bg", "shape" => "rect", "layer" => "template-0", "page" => app.page.get(), "x" => 500, "y" => 500, "w" => 120, "h" => 100, "color" => COLOR_BLACK, "width" => 1, "fill" => 0xffddddddu32 as i32, "fontSize" => 24, "text" => ""})?;
                app.enqueue(&jo! {"kind" => "object", "object" => "order-blue", "shape" => "rect", "layer" => "order-low", "page" => app.page.get(), "x" => 100, "y" => 100, "w" => 120, "h" => 100, "color" => COLOR_BLACK, "width" => 1, "fill" => COLOR_BLUE, "fontSize" => 24, "text" => ""})?;
                app.enqueue(&jo! {"kind" => "object", "object" => "order-red", "shape" => "rect", "layer" => "order-high", "page" => app.page.get(), "x" => 100, "y" => 100, "w" => 120, "h" => 100, "color" => COLOR_BLACK, "width" => 1, "fill" => COLOR_RED, "fontSize" => 24, "text" => ""})?;
                app.enqueue(&jo! {"kind" => "stroke", "stroke" => "order-ink", "layer" => "base", "page" => app.page.get(), "color" => COLOR_BLACK, "width" => 8, "brush" => "pen", "points" => points(&[[510, 550, 1], [600, 550, 1]])?})?;
            }
            if action == "tool-switch-selection" {
                app.set_drawing_tool(None)?;
                app.enqueue(&jo! {"kind" => "object", "object" => "tool-switch-item", "shape" => "rect", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 120, "y" => 140, "w" => 150, "h" => 100, "color" => COLOR_BLACK, "width" => 5, "fontSize" => 24, "text" => ""})?;
                app.enqueue(&jo! {"kind" => "object", "object" => "tool-switch-item-2", "shape" => "ellipse", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 340, "y" => 220, "w" => 90, "h" => 70, "color" => COLOR_BLACK, "width" => 5, "fontSize" => 24, "text" => ""})?;
            }
            if action == "select-panel" && !app.model().objects.contains_key("lasso-object") {
                app.enqueue(&jo! {"kind" => "object", "object" => "lasso-object", "shape" => "rect", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 100, "y" => 100, "w" => 150, "h" => 150, "color" => COLOR_BLACK, "width" => 5, "fontSize" => 24, "text" => ""})?;
            }
            if action == "mixed-drawing" {
                app.set_drawing_tool(Some(&notemaker_core::ids::ShapeId::from("triangle")))?;
                Integration::gesture(app, GestureDrag { x: 100.0, y: 100.0, width: 100.0, height: 100.0 }, GestureEnd::Up)?;
                app.set_drawing_tool(None)?;
                Integration::gesture(app, GestureDrag { x: 300.0, y: 100.0, width: 100.0, height: 100.0 }, GestureEnd::Up)?;
                app.set_drawing_tool(Some(&notemaker_core::ids::ShapeId::from("star")))?;
                Integration::gesture(app, GestureDrag { x: 100.0, y: 300.0, width: 100.0, height: 100.0 }, GestureEnd::Up)?;
                app.set_drawing_tool(None)?;
                Integration::gesture(app, GestureDrag { x: 300.0, y: 300.0, width: 100.0, height: 100.0 }, GestureEnd::Up)?;
                app.history(notemaker_core::store::HistoryDirection::Undo)?; // Pending object saves must block this attempt.
            }
            if action == "text" {
                app.place_text(Some(&notemaker_core::style::TextContent::from(bundle_string_or(arguments, "text", "Editable text")?)), notemaker_core::style::FontSize(32.0), crate::main_activity::DocPointF { x: 300.0, y: 400.0 })?;
            }
            if action == "layer" {
                app.selected.set(bundle_string_or(arguments, "layer", "test-layer")?);
                let force = parse_boolean(&bundle_string_or(arguments, "force", "false")?)?;
                app.enqueue(&jo! {"kind" => "layer", "layer" => app.selected.get(), "name" => "Test layer", "force" => force})?;
                app.save();
            }
            if action == "hide" {
                let layer = bundle_string_or(arguments, "layer", "test-layer")?;
                app.hidden.with_mut(|h| h.insert(layer));
                app.save();
            }
            if action == "undo" {
                app.history(notemaker_core::store::HistoryDirection::Undo)?;
            }
            if action == "redo" {
                app.history(notemaker_core::store::HistoryDirection::Redo)?;
            }
            if action == "page" {
                app.page.set(parse_int(&bundle_string_or(arguments, "page", "1")?)?);
                app.save();
            }
            if action == "erase" {
                app.erase.set(true);
                let shift = parse_float(&bundle_string_or(arguments, "shift", "0")?)?;
                let canvas = app.canvas();
                canvas.erase_at(crate::main_activity::ScreenPoint { x: (200.0 + shift) * canvas.scale().0, y: (266.0 + shift) * canvas.scale().0 })?;
                app.save();
            }
            app.update()?;
        }
        if action == "gesture-routing" {
            app.set_drawing_tool(None)?;
            let strokes_before = app.strokes.with(|s| s.len());
            let start = jx::uptime_millis().0;
            let canvas = app.canvas();
            let scale = canvas.scale().0;
            let send = |event: Obj| -> AppResult<()> {
                canvas.on_touch_event(&event)?;
                recycle(&event)?;
                Ok(())
            };
            send(Integration::touch(start, start, MOTIONEVENT_ACTION_DOWN, 1, 100.0 * scale, 200.0 * scale, 0.0, 0.0)?)?;
            send(Integration::touch(start, start + 20, MOTIONEVENT_ACTION_MOVE, 1, 180.0 * scale, 260.0 * scale, 0.0, 0.0)?)?;
            send(Integration::touch(start, start + 40, MOTIONEVENT_ACTION_UP, 1, 180.0 * scale, 260.0 * scale, 0.0, 0.0)?)?;
            if canvas.points.with(|p| p.is_some()) || canvas.current.with(|c| c.is_some()) || app.strokes.with(|s| s.len()) != strokes_before {
                return Err(AppError::illegal_state("Finger drag created a pen stroke"));
            }
            let old_x = canvas.offset_x.get();
            let old_y = canvas.offset_y.get();
            let old_zoom = canvas.zoom.get();
            let pointer = 1 << MOTIONEVENT_ACTION_POINTER_INDEX_SHIFT;
            send(Integration::touch(start + 100, start + 100, MOTIONEVENT_ACTION_DOWN, 1, 100.0 * scale, 100.0 * scale, 0.0, 0.0)?)?;
            send(Integration::touch(start + 100, start + 110, MOTIONEVENT_ACTION_POINTER_DOWN | pointer, 2, 100.0 * scale, 100.0 * scale, 200.0 * scale, 100.0 * scale)?)?;
            send(Integration::touch(start + 100, start + 130, MOTIONEVENT_ACTION_MOVE, 2, 140.0 * scale, 130.0 * scale, 240.0 * scale, 130.0 * scale)?)?;
            send(Integration::touch(start + 100, start + 150, MOTIONEVENT_ACTION_POINTER_UP | pointer, 2, 140.0 * scale, 130.0 * scale, 240.0 * scale, 130.0 * scale)?)?;
            send(Integration::touch(start + 100, start + 170, MOTIONEVENT_ACTION_UP, 1, 140.0 * scale, 130.0 * scale, 0.0, 0.0)?)?;
            if (canvas.offset_x.get() - old_x).abs() < 20.0 || (canvas.offset_y.get() - old_y).abs() < 15.0 || (canvas.zoom.get() - old_zoom).abs() > 0.05f32 {
                return Err(AppError::illegal_state("Two-finger move did not pan the page cleanly"));
            }
        }
        Ok(())
    }

    /// `tool-switch-selection` (instrumentation thread part of `runAction`).
    fn tool_switch_selection(&self, app: &Arc<MainActivity>) -> AppResult<()> {
        self.drain_writes()?;
        self.wait_for_idle_sync()?;
        let entered = Arc::new(CountDownLatch::new(1));
        let release = Arc::new(CountDownLatch::new(1));
        {
            let (entered, release) = (entered.clone(), release.clone());
            replica_service::writes().execute(move || {
                entered.count_down();
                if !release.await_timeout(Duration::from_secs(15)) {
                    panic!("java.lang.IllegalStateException: selection race gate timed out");
                }
            });
        }
        if !entered.await_timeout(Duration::from_secs(5)) {
            return Err(AppError::illegal_state("selection race gate did not start"));
        }
        let item = || vec![Value::Object(object! {"kind" => "object", "id" => "tool-switch-item"})];
        self.run_on_main_sync(|| -> AppResult<()> {
            app.start_selection(item())?;
            app.editor().open_tool(&ToolKind::from("pen"), Some(&app.canvas().view()))
        })??;
        release.count_down();
        self.drain_writes()?;
        self.wait_for_idle_sync()?;
        self.drain_writes()?;
        let mut state = app.replica().read()?;
        let mut leases = map_or_empty(&state, "leases")?;
        let key = format!("{}:object:tool-switch-item", app.page.get());
        if app.selection_session.get().is_some() || !app.selected_entities.with(|s| s.is_empty()) || leases.contains_key(&key) {
            return Err(AppError::illegal_state(format!(
                "A late selection acquisition survived switching to Pen: session={} leases={}",
                app.selection_session.get().as_deref().unwrap_or("null"),
                map_string(&leases)
            )));
        }
        self.run_on_main_sync(|| app.start_selection(item()))??;
        self.wait_for_selection(app, 1, "active selection for Select mode")?;
        let session = app.selection_session.get();
        self.run_on_main_sync(|| app.editor().open_tool(&ToolKind::from("select"), Some(&app.canvas().view())))??;
        self.drain_writes()?;
        state = app.replica().read()?;
        leases = map_or_empty(&state, "leases")?;
        // `session.equals(app.selectionSession)`: a null session threw in Java.
        let session = session.ok_or_else(null_pointer)?;
        if app.selection_session.get().as_deref() != Some(session.as_str()) || !leases.contains_key(&key) {
            return Err(AppError::illegal_state("Opening Select mode discarded the current selection"));
        }
        let model_before = json::write_map(&app.model().json());
        let undo_before = app.undo.get();
        let revision_before = app.revision.get();
        self.run_on_main_sync(|| app.preview_selection(notemaker_core::collaboration::SelectionTransform { scale: 1.0, dx: 120.0, dy: 20.0 }))??;
        self.wait_until(&|| Ok(app.transform_previews.with(|p| p.contains_key(&session))), 10000, "active transform preview before tool switch")?;
        self.run_on_main_sync(|| app.editor().open_tool(&ToolKind::from("pen"), Some(&app.canvas().view())))??;
        self.drain_writes()?;
        self.wait_for_idle_sync()?;
        self.drain_writes()?;
        state = app.replica().read()?;
        leases = map_or_empty(&state, "leases")?;
        let previews = map_or_empty(&state, "previews")?;
        if app.selection_session.get().is_some() || !app.selected_entities.with(|s| s.is_empty()) || leases.contains_key(&key) || previews.contains_key(&session) {
            return Err(AppError::illegal_state(format!("Switching to Pen retained a lease or transform preview: leases={} previews={}", map_string(&leases), map_string(&previews))));
        }
        if model_before != json::write_map(&app.model().json()) || undo_before != app.undo.get() || revision_before != app.revision.get() {
            return Err(AppError::illegal_state("Cancelling the preview on tool switch changed drawing state or history"));
        }
        for kind in ["eraser", "move", "shapes", "text", "fill"] {
            jx::frame(|| -> AppResult<()> {
                self.run_on_main_sync(|| app.start_selection(item()))??;
                self.wait_for_selection(app, 1, &format!("selection before {kind} switch"))?;
                let selected_tool = kind;
                self.run_on_main_sync(|| app.editor().open_tool(&ToolKind::from(selected_tool), Some(&app.canvas().view())))??;
                self.drain_writes()?;
                self.wait_for_idle_sync()?;
                self.drain_writes()?;
                let state = app.replica().read()?;
                let leases = map_or_empty(&state, "leases")?;
                if app.selection_session.get().is_some() || !app.selected_entities.with(|s| s.is_empty()) || leases.contains_key(&key) {
                    return Err(AppError::illegal_state(format!("Switching to {} retained a selection lease: {}", kind, map_string(&leases))));
                }
                Ok(())
            })?;
        }
        self.run_on_main_sync(|| app.start_selection(item()))??;
        self.wait_for_selection(app, 1, "selection before shape setter")?;
        self.run_on_main_sync(|| app.set_drawing_tool(Some(&notemaker_core::ids::ShapeId::from("rect"))))??;
        self.drain_writes()?;
        self.wait_for_idle_sync()?;
        self.drain_writes()?;
        state = app.replica().read()?;
        leases = map_or_empty(&state, "leases")?;
        if app.selection_session.get().is_some() || leases.contains_key(&key) {
            return Err(AppError::illegal_state(format!("Direct shape selection retained a lease: {}", map_string(&leases))));
        }
        self.run_on_main_sync(|| app.start_selection(vec![Value::Object(object! {"kind" => "object", "id" => "tool-switch-item"}), Value::Object(object! {"kind" => "object", "id" => "tool-switch-item-2"})]))??;
        self.wait_for_selection(app, 2, "multi-item selection before deselect race")?;
        let deselect_entered = Arc::new(CountDownLatch::new(1));
        let deselect_release = Arc::new(CountDownLatch::new(1));
        {
            let (entered, release) = (deselect_entered.clone(), deselect_release.clone());
            replica_service::writes().execute(move || {
                entered.count_down();
                if !release.await_timeout(Duration::from_secs(15)) {
                    panic!("java.lang.IllegalStateException: deselect race gate timed out");
                }
            });
        }
        if !deselect_entered.await_timeout(Duration::from_secs(5)) {
            return Err(AppError::illegal_state("deselect race gate did not start"));
        }
        self.run_on_main_sync(|| -> AppResult<()> {
            app.deselect_entity(&notemaker_core::ids::EntityKind::object(), &notemaker_core::ids::EntityId::from("tool-switch-item"))?;
            app.editor().open_tool(&ToolKind::from("move"), Some(&app.canvas().view()))
        })??;
        deselect_release.count_down();
        self.drain_writes()?;
        self.wait_for_idle_sync()?;
        self.drain_writes()?;
        state = app.replica().read()?;
        leases = map_or_empty(&state, "leases")?;
        if app.selection_session.get().is_some() || app.selecting.get() || leases.contains_key(&key) || leases.contains_key(&format!("{}:object:tool-switch-item-2", app.page.get())) {
            return Err(AppError::illegal_state(format!("A delayed per-item deselect reacquired locks after switching tools: {}", map_string(&leases))));
        }
        Ok(())
    }

    /// The final `runOnMainSync(() -> { synchronized(app.lock) {...} })` of
    /// `runAction`: cache verification, `integration.json`, optional render.
    fn report_state(&self, app: &Arc<MainActivity>, arguments: &Obj, action: &str, result: &Obj) -> AppResult<()> {
        if action == "state" && parse_boolean(&bundle_string_or(arguments, "verifyCache", "false")?)? {
            let canvas = app.canvas();
            let width = canvas.width().0;
            let height = canvas.height().0;
            let cached = create_bitmap(BitmapSize { width, height })?;
            let reference = create_bitmap(BitmapSize { width, height })?;
            draw_view(&canvas.view(), &cached)?;
            canvas.cull_tile_items.set(false);
            let drawn = draw_view(&canvas.view(), &reference);
            canvas.cull_tile_items.set(true);
            drawn?;
            let a = crate::android_canvas::bitmap_pixels(&cached)?;
            let b = crate::android_canvas::bitmap_pixels(&reference)?;
            let (mut mismatched, mut min_x, mut min_y, mut max_x, mut max_y, mut max_channel_diff) = (0, width, height, -1, -1, 0);
            for i in 0..a.len() {
                if a[i] != b[i] {
                    let (ca, cb) = (ArgbColor(a[i]).channels(), ArgbColor(b[i]).channels());
                    let dr = (ca.red - cb.red).abs();
                    let dg = (ca.green - cb.green).abs();
                    let db = (ca.blue - cb.blue).abs();
                    mismatched += 1;
                    let x = i as i32 % width;
                    let y = i as i32 / width;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                    max_channel_diff = max_channel_diff.max(dr.max(dg.max(db)));
                }
            }
            if mismatched > 0 {
                write_png(app, "cache-diff-cached.png", &cached)?;
                write_png(app, "cache-diff-reference.png", &reference)?;
                let diagnostic = jo! {"width" => width, "height" => height, "mismatchedPixels" => mismatched, "thresholdPixels" => 0, "maxChannelDiff" => max_channel_diff, "bounds" => ints(&[min_x, min_y, max_x, max_y])?};
                write_file(app, "cache-diff.json", diagnostic.to_string().as_bytes())?;
            }
            recycle_bitmap(&cached)?;
            recycle_bitmap(&reference)?;
            if mismatched > 0 {
                return Err(AppError::illegal_state(format!(
                    "Ink tile cache differs from uncullled same-tier reference: {mismatched} pixels bounds={min_x},{min_y}..{max_x},{max_y} maxChannelDiff={max_channel_diff}"
                )));
            }
        }
        let stored = app.store.get().ok_or_else(null_pointer)?.read()?;
        let stored_undo = match stored.get("undo") {
            None => 0,
            Some(v) => json::list(v)?.len() as i32,
        };
        let events = match stored.get("events") {
            None | Some(Value::Null) => JSONArray::new(),
            Some(v) => JSONArray::from_value(&Value::Array(json::list(v)?.clone())).unwrap_or_default(),
        };
        let state = jo! {
            "document" => app.document.get(),
            "type" => app.type_.get(),
            "cursor" => app.cursor.get(),
            "pending" => app.pending_count.get(),
            "strokes" => app.strokes.with(|s| s.len()) as i32,
            "editorUndoCount" => app.undo.with(|u| u.len()) as i32,
            "storedUndoCount" => stored_undo,
            "events" => events,
            "visible" => app.canvas().visible(&notemaker_core::ids::LayerId::from("test-layer")),
            "page" => app.page.get(),
            "model" => JSONObject::parse(&json::write_map(&app.model().json()))?,
        };
        let text = state.to_string();
        write_file(app, "integration.json", text.as_bytes())?;
        if parse_boolean(&bundle_string_or(arguments, "image", "false")?)? {
            let image = create_bitmap(BitmapSize { width: 1000, height: 1400 })?;
            let c = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", &image)?;
            jcall!(c, "android/graphics/Canvas", "drawColor", "(I)V", COLOR_WHITE)?;
            let hidden = app.hidden.get();
            let mut surface = crate::android_canvas::AndroidCanvas::new(c);
            notemaker_core::render::draw(&app.model(), notemaker_core::ids::PageNumber(app.page.get()), &notemaker_core::render::HiddenLayers(hidden), &mut notemaker_cli::png::Png::new(&mut surface))?;
            write_png(app, "integration.png", &image)?;
            let pixels = crate::android_canvas::bitmap_pixels(&image)?;
            let (mut count, mut min_x, mut min_y, mut max_x, mut max_y) = (0, 1000, 1400, -1, -1);
            for (i, pixel) in pixels.iter().enumerate() {
                let pixel = ArgbColor(*pixel).channels();
                if pixel.red < 245 || pixel.green < 245 || pixel.blue < 245 {
                    count += 1;
                    let x = i as i32 % 1000;
                    let y = i as i32 / 1000;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
            let stats = jo! {"pixels" => count, "bounds" => ints(&[min_x, min_y, max_x, max_y])?};
            write_file(app, "render-stats.json", stats.to_string().as_bytes())?;
            recycle_bitmap(&image)?;
        }
        let summary = if json::jlen(&text) < 32000 { text } else { "See files/integration.json".to_string() };
        put_string(result, "state", &summary)?;
        Ok(())
    }

    /// `private void collectNodes(AccessibilityNodeInfo node, JSONArray nodes) throws Exception`.
    pub fn collect_nodes(&self, node: &Obj, nodes: &JSONArray) -> AppResult<()> {
        let bounds = jnew!("android/graphics/Rect", "()V")?;
        jcall!(node, "android/view/accessibility/AccessibilityNodeInfo", "getBoundsInScreen", "(Landroid/graphics/Rect;)V", &bounds)?;
        let char_sequence = |o: Option<Obj>| o.and_then(|o| jx::to_string(o.as_obj())).unwrap_or_default();
        let text = char_sequence(jcall!(node, "android/view/accessibility/AccessibilityNodeInfo", "getText", "()Ljava/lang/CharSequence;")?.global());
        let description = char_sequence(jcall!(node, "android/view/accessibility/AccessibilityNodeInfo", "getContentDescription", "()Ljava/lang/CharSequence;")?.global());
        let package = char_sequence(jcall!(node, "android/view/accessibility/AccessibilityNodeInfo", "getPackageName", "()Ljava/lang/CharSequence;")?.global());
        let left = jget!(bounds, "android/graphics/Rect", "left", "I")?.int();
        let top = jget!(bounds, "android/graphics/Rect", "top", "I")?.int();
        let right = jget!(bounds, "android/graphics/Rect", "right", "I")?.int();
        let bottom = jget!(bounds, "android/graphics/Rect", "bottom", "I")?.int();
        nodes.put(jo! {"text" => text, "content-desc" => description, "bounds" => format!("[{left},{top}][{right},{bottom}]"), "package" => package})?;
        let count = jcall!(node, "android/view/accessibility/AccessibilityNodeInfo", "getChildCount", "()I")?.int();
        for i in 0..count {
            let child = jcall!(node, "android/view/accessibility/AccessibilityNodeInfo", "getChild", "(I)Landroid/view/accessibility/AccessibilityNodeInfo;", i)?.global();
            if let Some(child) = child {
                self.collect_nodes(&child, nodes)?;
            }
        }
        Ok(())
    }
    /// `private void drainWrites() throws Exception`: waits for `replica-writer` (30 s).
    pub fn drain_writes(&self) -> AppResult<()> {
        replica_service::writes().submit(|| ()).get_timeout(Duration::from_secs(30))?;
        Ok(())
    }
    /// `private void waitForSelection(MainActivity app, int count, String label) throws Exception`.
    fn wait_for_selection(&self, app: &Arc<MainActivity>, count: i32, label: &str) -> AppResult<()> {
        self.wait_for_selection_condition(app, &|| Ok(app.selection_session.with(|s| s.is_some()) && app.selected_entities.with(|s| s.len()) as i32 >= count), label)
    }
    /// `private void waitForSelectionCondition(MainActivity app, Callable<Boolean> condition, String label) throws Exception`.
    fn wait_for_selection_condition(&self, app: &Arc<MainActivity>, condition: &dyn Fn() -> AppResult<bool>, label: &str) -> AppResult<()> {
        self.wait_until(&|| Ok(app.selection_error.with(|e| e.is_some()) || condition()?), 30000, label)?;
        if let Some(error) = app.selection_error.get() {
            return Err(AppError::illegal_state(format!("Could not acquire {label}: {error}")));
        }
        Ok(())
    }
    /// `private void waitUntil(Callable<Boolean> condition, long timeout, String label) throws Exception`.
    fn wait_until(&self, condition: &dyn Fn() -> AppResult<bool>, timeout: i64, label: &str) -> AppResult<()> {
        let deadline = jx::current_time_millis().0 + timeout;
        while jx::current_time_millis().0 < deadline {
            self.wait_for_idle_sync()?;
            if condition()? {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(AppError::illegal_state(format!("Timed out waiting for {label}")))
    }
    /// `private void makeConflictFixture(MainActivity app) throws Exception`.
    pub fn make_conflict_fixture(&self, app: &Arc<MainActivity>) -> AppResult<()> {
        use notemaker_core::store::Store;
        let id = "android-conflict-item";
        let root = ui::cache_dir(&app.this)?.join("drawing-conflict-fixture");
        let document = notemaker_core::ids::DocumentName::from(app.document.get());
        let a = Store::writer(&notemaker_core::paths::ReplicaDir::new(root.join("a")), &document)?;
        let b = Store::writer(&notemaker_core::paths::ReplicaDir::new(root.join("b")), &document)?;
        let seed = notemaker_core::actions::drawing(&notemaker_core::wire::ActionInput(object! {"object" => id, "shape" => "rect", "x" => 80, "y" => 90, "w" => 180, "h" => 120, "color" => COLOR_BLACK, "width" => 5}), &notemaker_core::ids::LayerId::from("base"))?;
        a.append(&seed, &notemaker_core::ids::ActorId::from("fixture-seed"))?;
        let common = {
            let state = a.read()?;
            let pending = json::list(state.get("pending").unwrap_or(&Value::Null))?;
            let first = pending.first().ok_or_else(|| AppError::java("java.lang.IndexOutOfBoundsException", "Index: 0, Size: 0"))?;
            json::map(first)?.clone()
        };
        b.accept((&[Value::Object(common)]).into(), None)?;
        a.commit(&notemaker_core::ids::ActorId::from("writer-a"), &mut |model| {
            let mut changed = (**model.objects.get(id).ok_or_else(notemaker_core::Error::null_pointer)?).clone();
            changed.insert("x".into(), Value::from(100));
            Ok((vec![Value::Object(changed.into())]).into())
        })?;
        b.commit(&notemaker_core::ids::ActorId::from("writer-b"), &mut |model| {
            let mut changed = (**model.objects.get(id).ok_or_else(notemaker_core::Error::null_pointer)?).clone();
            changed.insert("shape".into(), Value::from("ellipse"));
            changed.insert("x".into(), Value::from(250));
            Ok((vec![Value::Object(changed.into())]).into())
        })?;
        let mut incoming: Vec<Value> = json::list(a.read()?.get("pending").unwrap_or(&Value::Null))?.clone();
        incoming.extend(json::list(b.read()?.get("pending").unwrap_or(&Value::Null))?.iter().cloned());
        let proxy = replica_service::ensure(&app.this)?;
        let descriptor_bytes = std::fs::read(app.files_dir()?.join("proxy.json"))?;
        let descriptor = json::map(&json::parse(&String::from_utf8_lossy(&descriptor_bytes))?)?.clone();
        let mut request = notemaker_core::endpoint::HttpConnection::new(&HttpUrl::from(format!("http://127.0.0.1:{}", proxy.port())).join(ApiPath::Events))?;
        request.set_request_method(HttpMethod::Post);
        request.set_connect_timeout(5000);
        request.set_read_timeout(10000);
        request.set_request_property(HeaderName::Authorization, BearerToken::from(json::java_string(descriptor.get("token"))).authorization());
        request.set_request_property(HeaderName::ContentType, HeaderValue::from("application/json; charset=utf-8"));
        let body = json::write_map(&object! {"document" => document, "operations" => incoming}).into_bytes();
        request.write_body(body);
        if request.response_code()? != HttpStatus::OK {
            let mut error = Vec::new();
            let mut stream = request.error_stream()?.ok_or_else(null_pointer)?;
            Integration::copy(&mut stream, &mut error)?;
            return Err(AppError::illegal_state(format!("Proxy conflict fixture ingress failed: {}", String::from_utf8_lossy(&error))));
        }
        let received = (|| -> AppResult<()> {
            let mut response = request.input_stream()?;
            let mut received = Vec::new();
            Integration::copy(&mut response, &mut received)?;
            json::parse(&String::from_utf8_lossy(&received))?;
            Ok(())
        })();
        request.disconnect();
        received
    }
    /// `private Bitmap captureCanvas(MainActivity app)`.
    pub fn capture_canvas(&self, app: &Arc<MainActivity>) -> AppResult<Obj> {
        self.run_on_main_sync(|| -> AppResult<Obj> {
            let canvas = app.canvas();
            let bitmap = create_bitmap(BitmapSize { width: canvas.width().0, height: canvas.height().0 })?;
            draw_view(&canvas.view(), &bitmap)?;
            Ok(bitmap)
        })?
    }
    /// `private int canvasPixel(MainActivity app, Bitmap bitmap, float x, float y)`.
    fn canvas_pixel(&self, app: &Arc<MainActivity>, bitmap: &Obj, x: f32, y: f32) -> AppResult<i32> {
        let canvas = app.canvas();
        let BitmapSize { width, height } = bitmap_size(bitmap)?;
        let px = 0.max((width - 1).min(ui::round_f32(canvas.offset_x.get() + x * canvas.scale().0 * canvas.zoom.get())));
        let py = 0.max((height - 1).min(ui::round_f32(canvas.offset_y.get() + y * canvas.scale().0 * canvas.zoom.get())));
        Ok(pixel(bitmap, BitmapPoint { x: px, y: py })?.0)
    }
    /// `private boolean imagesDiffer(Bitmap a, Bitmap b)`: compares every
    /// fourth pixel in both directions (read in bulk).
    pub fn images_differ(&self, a: &Obj, b: &Obj) -> AppResult<bool> {
        let BitmapSize { width: aw, height: ah } = bitmap_size(a)?;
        let BitmapSize { width: bw, height: bh } = bitmap_size(b)?;
        if aw != bw || ah != bh {
            return Ok(true);
        }
        let pa = crate::android_canvas::bitmap_pixels(a)?;
        let pb = crate::android_canvas::bitmap_pixels(b)?;
        for y in (0..ah).step_by(4) {
            for x in (0..aw).step_by(4) {
                let i = (y * aw + x) as usize;
                if pa[i] != pb[i] {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// Java idioms shared by the instrumentation checks (Integration, StrokeCheck,
// CoverageCheck).

/// Runs `f`, turning a panic into an error (Java `catch (Throwable)`).
pub(crate) fn catch<T>(f: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into());
            Err(AppError::runtime(format!("Rust panic: {msg}")))
        }
    }
}
/// Java `try { body } finally { cleanup }`: a failing `finally` replaces the body's outcome.
pub(crate) fn finally<T>(body: AppResult<T>, cleanup: AppResult<()>) -> AppResult<T> {
    cleanup?;
    body
}
fn null_pointer() -> AppError {
    AppError::Core(notemaker_core::Error::null_pointer())
}
/// `list.get(0)` of a selected-entity list.
fn first(list: &[Map]) -> AppResult<&Map> {
    list.first().ok_or_else(|| AppError::java("java.lang.IndexOutOfBoundsException", format!("Index 0 out of bounds for length {}", list.len())))
}
/// `"id".equals(entity.get("id"))`.
fn id_is(entity: &Map, id: &str) -> bool {
    entity.get("id").and_then(Value::as_str) == Some(id)
}
/// `String.valueOf(List<Map>)`.
fn maps_string(list: &[Map]) -> String {
    json::java_string(Some(&Value::Array(list.iter().cloned().map(Value::Object).collect())))
}
/// `String.valueOf(Map)`.
fn map_string(map: &Map) -> String {
    json::java_string(Some(&Value::Object(map.clone())))
}
/// `Json.map(state.getOrDefault(key, Collections.emptyMap()))`.
fn map_or_empty(state: &Map, key: &str) -> AppResult<Map> {
    Ok(match state.get(key) {
        None => Map::new(),
        Some(v) => json::map(v)?.clone(),
    })
}
/// `new JSONArray(Arrays.asList(values))` of `Integer`s.
fn ints(values: &[i32]) -> AppResult<JSONArray> {
    let a = JSONArray::new();
    for v in values {
        a.put(*v)?;
    }
    Ok(a)
}
/// `new JSONArray().put(new JSONArray(Arrays.asList(x, y, p)))...`.
fn points(values: &[[i32; 3]]) -> AppResult<JSONArray> {
    let a = JSONArray::new();
    for p in values {
        a.put(ints(p)?)?;
    }
    Ok(a)
}
/// `BufferedReader.readLine()` text of one raw line (without `\n` / `\r\n`).
fn read_line_text(raw: &[u8]) -> String {
    let mut end = raw.len();
    if end > 0 && raw[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && raw[end - 1] == b'\r' {
        end -= 1;
    }
    String::from_utf8_lossy(&raw[..end]).into_owned()
}
/// Waits until `server` has a pending connection (`ServerSocket.setSoTimeout`).
fn accept_ready(server: &std::net::TcpListener, timeout_ms: i32) -> AppResult<()> {
    use std::os::fd::AsRawFd;
    loop {
        let mut fd = libc::pollfd { fd: server.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let n = unsafe { libc::poll(&mut fd, 1, timeout_ms) };
        if n > 0 {
            return Ok(());
        }
        if n == 0 {
            return Err(AppError::java("java.net.SocketTimeoutException", "Accept timed out"));
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error.into());
        }
    }
}

/// `Float.parseFloat(s)` (Java parsing rules).
fn parse_float(s: &str) -> AppResult<f32> {
    Ok(jstatic!("java/lang/Float", "parseFloat", "(Ljava/lang/String;)F", s)?.float())
}
/// `Integer.parseInt(s)`.
fn parse_int(s: &str) -> AppResult<i32> {
    Ok(jstatic!("java/lang/Integer", "parseInt", "(Ljava/lang/String;)I", s)?.int())
}
/// `Boolean.parseBoolean(s)`.
fn parse_boolean(s: &str) -> AppResult<bool> {
    Ok(jstatic!("java/lang/Boolean", "parseBoolean", "(Ljava/lang/String;)Z", s)?.bool())
}
/// `android.util.Base64.decode(s, Base64.DEFAULT)`.
fn base64_decode(s: Option<&str>) -> AppResult<Vec<u8>> {
    let bytes = jstatic!("android/util/Base64", "decode", "(Ljava/lang/String;I)[B", s, BASE64_DEFAULT)?.global();
    Ok(bytes.map(|b| jx::byte_array(b.as_obj())).unwrap_or_default())
}

/// `new Bundle()`.
fn new_bundle() -> AppResult<Obj> {
    Ok(jnew!("android/os/Bundle", "()V")?)
}
/// `bundle.putString(key, value)`.
fn put_string(bundle: &Obj, key: &str, value: &str) -> AppResult<()> {
    jcall!(bundle, "android/os/Bundle", "putString", "(Ljava/lang/String;Ljava/lang/String;)V", key, value)?;
    Ok(())
}
/// `bundle.getString(key)`.
fn bundle_string(bundle: &Obj, key: &str) -> AppResult<Option<String>> {
    Ok(jcall!(bundle, "android/os/Bundle", "getString", "(Ljava/lang/String;)Ljava/lang/String;", key)?.string())
}
/// `bundle.getString(key, fallback)`.
fn bundle_string_or(bundle: &Obj, key: &str, fallback: &str) -> AppResult<String> {
    Ok(jcall!(bundle, "android/os/Bundle", "getString", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;", key, fallback)?.string().unwrap_or_else(|| fallback.to_string()))
}
/// `bundle.containsKey(key)`.
fn bundle_contains(bundle: &Obj, key: &str) -> AppResult<bool> {
    Ok(jcall!(bundle, "android/os/Bundle", "containsKey", "(Ljava/lang/String;)Z", key)?.bool())
}
/// `bundle.keySet()` in iteration order.
fn bundle_keys(bundle: &Obj) -> AppResult<Vec<String>> {
    let keys = jcall!(bundle, "android/os/Bundle", "keySet", "()Ljava/util/Set;")?.some();
    let array = jcall!(keys, "java/util/Set", "toArray", "()[Ljava/lang/Object;")?.some();
    let mut out = Vec::new();
    for i in 0..jx::array_length(array.as_obj()) {
        if let Some(key) = jx::array_element(array.as_obj(), i) {
            out.push(jx::rstr(key.as_obj()));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Synthesized MotionEvents

/// A synthesized stylus drag in page units at the canvas view's base scale
/// (multiplied by [`crate::main_activity::CanvasView::scale`], before zoom
/// and pan): from `(x, y)` to `(x + width, y + height)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GestureDrag {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// How a synthesized gesture ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureEnd {
    /// `MotionEvent.ACTION_UP`
    Up,
    /// `MotionEvent.ACTION_CANCEL`
    Cancel,
}

/// `MotionEvent.PointerProperties.id` of a synthesized pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerId(pub i32);

/// `MotionEvent.PointerProperties.toolType` of a synthesized pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerTool {
    /// `MotionEvent.TOOL_TYPE_FINGER`
    Finger,
    /// `MotionEvent.TOOL_TYPE_STYLUS`
    Stylus,
}
impl<'a> From<PointerTool> for Arg<'a> {
    fn from(tool: PointerTool) -> Self {
        Arg::Int(match tool {
            PointerTool::Finger => MOTIONEVENT_TOOL_TYPE_FINGER,
            PointerTool::Stylus => MOTIONEVENT_TOOL_TYPE_STYLUS,
        })
    }
}

/// `MotionEvent.PointerCoords.size`: the normalized contact size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchSize(pub f32);

/// The `InputDevice.SOURCE_*` a synthesized MotionEvent claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSource {
    /// `InputDevice.SOURCE_TOUCHSCREEN`
    Touchscreen,
    /// `InputDevice.SOURCE_STYLUS`
    Stylus,
}
impl<'a> From<InputSource> for Arg<'a> {
    fn from(source: InputSource) -> Self {
        Arg::Int(match source {
            InputSource::Touchscreen => INPUTDEVICE_SOURCE_TOUCHSCREEN,
            InputSource::Stylus => INPUTDEVICE_SOURCE_STYLUS,
        })
    }
}

/// The `PointerProperties[]` and `PointerCoords[]` of one synthesized
/// MotionEvent, in pointer-index order (both lists have one entry per pointer).
pub struct Pointers {
    pub properties: Vec<Obj>,
    pub coords: Vec<Obj>,
}
impl Pointers {
    /// A single pointer.
    pub fn single(properties: Obj, coords: Obj) -> Pointers {
        Pointers { properties: vec![properties], coords: vec![coords] }
    }
}

/// `new MotionEvent.PointerProperties()` with `id` and `toolType`.
pub(crate) fn pointer_properties(id: PointerId, tool: PointerTool) -> AppResult<Obj> {
    let property = jnew!("android/view/MotionEvent$PointerProperties", "()V")?;
    jset!(property, "android/view/MotionEvent$PointerProperties", "id", "I", id.0)?;
    jset!(property, "android/view/MotionEvent$PointerProperties", "toolType", "I", tool)?;
    Ok(property)
}
/// `new MotionEvent.PointerCoords()` with `x`, `y`, `pressure`, `size`.
pub(crate) fn pointer_coords(point: ScreenPoint, pressure: StylusPressure, size: TouchSize) -> AppResult<Obj> {
    let coords = jnew!("android/view/MotionEvent$PointerCoords", "()V")?;
    jset!(coords, "android/view/MotionEvent$PointerCoords", "x", "F", point.x)?;
    jset!(coords, "android/view/MotionEvent$PointerCoords", "y", "F", point.y)?;
    jset!(coords, "android/view/MotionEvent$PointerCoords", "pressure", "F", pressure.0)?;
    jset!(coords, "android/view/MotionEvent$PointerCoords", "size", "F", size.0)?;
    Ok(coords)
}
/// `MotionEvent.obtain(down, time, action, count, properties, coords, 0, 0, 1, 1, 0, 0, source, 0)`.
pub(crate) fn obtain(down: UptimeMillis, time: UptimeMillis, action: MotionAction, pointers: &Pointers, source: InputSource) -> AppResult<Obj> {
    let p: Vec<Option<&Obj>> = pointers.properties.iter().map(Some).collect();
    let c: Vec<Option<&Obj>> = pointers.coords.iter().map(Some).collect();
    let p = jx::new_object_array("android/view/MotionEvent$PointerProperties", &p)?;
    let c = jx::new_object_array("android/view/MotionEvent$PointerCoords", &c)?;
    Ok(jstatic!(
        "android/view/MotionEvent",
        "obtain",
        "(JJII[Landroid/view/MotionEvent$PointerProperties;[Landroid/view/MotionEvent$PointerCoords;IIFFIIII)Landroid/view/MotionEvent;",
        down.0,
        time.0,
        action.0,
        pointers.properties.len() as i32,
        &p,
        &c,
        0,
        0,
        1.0f32,
        1.0f32,
        0,
        0,
        source,
        0
    )?
    .some())
}
/// `event.recycle()`.
pub(crate) fn recycle(event: &Obj) -> AppResult<()> {
    jcall!(event, "android/view/MotionEvent", "recycle", "()V")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Bitmaps

/// A pixel position in an Android `Bitmap` (`bitmap.getPixel(x, y)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BitmapPoint {
    pub x: i32,
    pub y: i32,
}

/// `Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)`.
pub(crate) fn create_bitmap(size: BitmapSize) -> AppResult<Obj> {
    let config = crate::android_canvas::argb_8888()?;
    Ok(jstatic!("android/graphics/Bitmap", "createBitmap", "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;", size.width, size.height, &config)?.some())
}
/// `view.draw(new Canvas(bitmap))`.
pub(crate) fn draw_view(view: &Obj, bitmap: &Obj) -> AppResult<()> {
    let c = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", bitmap)?;
    jcall!(view, "android/view/View", "draw", "(Landroid/graphics/Canvas;)V", &c)?;
    Ok(())
}
/// `bitmap.getWidth()`, `bitmap.getHeight()`.
pub(crate) fn bitmap_size(bitmap: &Obj) -> AppResult<BitmapSize> {
    Ok(BitmapSize { width: jcall!(bitmap, "android/graphics/Bitmap", "getWidth", "()I")?.int(), height: jcall!(bitmap, "android/graphics/Bitmap", "getHeight", "()I")?.int() })
}
/// `bitmap.getPixel(x, y)`.
pub(crate) fn pixel(bitmap: &Obj, at: BitmapPoint) -> AppResult<ArgbColor> {
    Ok(ArgbColor(jcall!(bitmap, "android/graphics/Bitmap", "getPixel", "(II)I", at.x, at.y)?.int()))
}
/// `a.sameAs(b)`.
pub(crate) fn same_as(a: &Obj, b: &Obj) -> AppResult<bool> {
    Ok(jcall!(a, "android/graphics/Bitmap", "sameAs", "(Landroid/graphics/Bitmap;)Z", b)?.bool())
}
/// `bitmap.recycle()`.
pub(crate) fn recycle_bitmap(bitmap: &Obj) -> AppResult<()> {
    jcall!(bitmap, "android/graphics/Bitmap", "recycle", "()V")?;
    Ok(())
}
/// `try (FileOutputStream out = app.openFileOutput(name, 0)) { out.write(bytes); }`.
fn write_file(app: &MainActivity, name: &str, bytes: &[u8]) -> AppResult<()> {
    let out = jcall!(app.this, "android/content/Context", "openFileOutput", "(Ljava/lang/String;I)Ljava/io/FileOutputStream;", name, 0)?.some();
    let array = jx::new_byte_array(bytes)?;
    let written = jcall!(out, "java/io/FileOutputStream", "write", "([B)V", &array);
    let closed = jcall!(out, "java/io/FileOutputStream", "close", "()V");
    written?;
    closed?;
    Ok(())
}
/// `try (FileOutputStream out = app.openFileOutput(name, 0)) { bitmap.compress(PNG, 100, out); }`.
pub(crate) fn write_png(app: &MainActivity, name: &str, bitmap: &Obj) -> AppResult<()> {
    let out = jcall!(app.this, "android/content/Context", "openFileOutput", "(Ljava/lang/String;I)Ljava/io/FileOutputStream;", name, 0)?.some();
    let png = jsget!("android/graphics/Bitmap$CompressFormat", "PNG", "Landroid/graphics/Bitmap$CompressFormat;")?.some();
    let written = jcall!(bitmap, "android/graphics/Bitmap", "compress", "(Landroid/graphics/Bitmap$CompressFormat;ILjava/io/OutputStream;)Z", &png, 100, &out);
    let closed = jcall!(out, "java/io/FileOutputStream", "close", "()V");
    written?;
    closed?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_lines() {
        assert_eq!(read_line_text(b"{\"a\":1}\r\n"), "{\"a\":1}");
        assert_eq!(read_line_text(b"x"), "x");
    }

    #[test]
    fn finally_prefers_cleanup_error() {
        let body: AppResult<i32> = Err(AppError::illegal_state("body"));
        assert_eq!(finally(body, Err(AppError::illegal_state("cleanup"))).unwrap_err().message().as_deref(), Some("cleanup"));
        assert_eq!(finally(Ok(1), Ok(())).unwrap(), 1);
        let caught = catch(|| -> AppResult<()> { panic!("boom") }).unwrap_err();
        assert!(caught.to_string().contains("boom"));
    }

    #[test]
    fn json_helpers() {
        let entity = object! {"kind" => "object", "id" => "a"};
        assert!(id_is(&entity, "a"));
        assert_eq!(maps_string(&[entity]), "[{kind=object, id=a}]");
        let state = object! {"leases" => object! {"0:object:x" => 1}};
        assert!(map_or_empty(&state, "leases").unwrap().contains_key("0:object:x"));
        assert!(map_or_empty(&state, "previews").unwrap().is_empty());
    }
}
