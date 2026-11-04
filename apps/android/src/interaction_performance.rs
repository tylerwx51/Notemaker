//! Port of `InteractionPerformance.java` (instrumentation benchmark):
//! draw timing for common viewport and toolbar changes on a disposable note
//! (object commits, previews, history, selection, pages, fill).
//!
//! Runs on the instrumentation thread. Actions are posted to an asynchronous
//! main-looper `Handler`; frame metrics arrive on `interaction-frames`; the
//! decor view's `OnDrawListener` acknowledges the first durable window draw.

use crate::android_consts::*;
use crate::concurrent::CountDownLatch;
use crate::error::{self, AppError, AppResult};
use crate::integration::Integration;
use crate::jx::{self, Reply};
use crate::main_activity::MainActivity;
use crate::org_json::JV;
use crate::performance::{self, async_main_handler, FrameMetricId, FrameMetrics, LatencySamples, StrokeCount, dispatch_touch, drain_writes, frame_metric, handler_thread, quit_safely, recycle_event, sleep, thread_handler, window};
use indexmap::IndexMap;
use notemaker_core::json::{self, Map, Value};
use notemaker_core::{ids, object, store};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The benchmark modes, in measurement order.
const NAMES: [&str; 11] = ["pan", "zoom_redraw", "tool_switch", "layer_switch", "selection_tap", "document_reopen", "history_toggle", "stroke_history_toggle", "object_create", "page_switch", "fill_region"];

/// `BooleanSupplier`.
type Supplier = Arc<dyn Fn() -> bool + Send + Sync>;
/// `Runnable` held in an `AtomicReference`.
type Ack = Arc<dyn Fn() + Send + Sync>;

/// `Map<String,List<Double>>` (lists keyed by mode).
type Lists = IndexMap<String, Vec<f64>>;
/// The same with `Collections.synchronizedList` values (written by the frame listener).
type SharedLists = Arc<Mutex<Lists>>;

fn push(lists: &mut Lists, name: &str, value: f64) {
    lists.get_mut(name).expect("mode list").push(value);
}
fn shared_push(lists: &SharedLists, name: &str, value: f64) {
    lists.lock().unwrap().get_mut(name).expect("mode list").push(value);
}
fn shared_values(lists: &SharedLists, name: &str) -> Vec<f64> {
    lists.lock().unwrap().get(name).cloned().unwrap_or_default()
}
fn command(app: &MainActivity, args: &[&str], input: Map) -> AppResult<Value> {
    app.replica().command(crate::app_replica::CliArgs(args), Some(&Value::Object(input)))
}
/// `test.runOnMainSync(()->{try{app.refreshShared(state);...}catch(Exception error){throw new RuntimeException(error);}})`.
fn refresh<S: crate::main_activity::SharedState + ?Sized>(app: &Arc<MainActivity>, state: &S, then: impl FnOnce() -> AppResult<()>) -> AppResult<()> {
    (|| -> AppResult<()> {
        app.refresh_shared(state)?;
        then()
    })()
    .map_err(|e| AppError::runtime_cause(&e))
}
/// `!Boolean.FALSE.equals(map.get(id))`.
fn not_false(map: &IndexMap<String, bool>, id: &str) -> bool {
    map.get(id) != Some(&false)
}

/// `static Map<String,Object> run(Instrumentation test, MainActivity app) throws Exception`.
pub fn run(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<Map> {
    let input = async_main_handler()?;
    let frames = handler_thread(&jx::ThreadName::from("interaction-frames"))?;
    let mut draw: Lists = IndexMap::new();
    let mut handler: Lists = IndexMap::new();
    let mut queue: Lists = IndexMap::new();
    let mut action_lists: Lists = IndexMap::new();
    let mut draw_wait: Lists = IndexMap::new();
    let mut ui_draw: Lists = IndexMap::new();
    let frame: SharedLists = Arc::new(Mutex::new(IndexMap::new()));
    let frame_draw: SharedLists = Arc::new(Mutex::new(IndexMap::new()));
    let frame_command: SharedLists = Arc::new(Mutex::new(IndexMap::new()));
    let mut undo_ui_draw: Vec<f64> = Vec::new();
    let mut redo_ui_draw: Vec<f64> = Vec::new();
    let mut undo_canvas_draw: Vec<f64> = Vec::new();
    let mut redo_canvas_draw: Vec<f64> = Vec::new();
    let mut selected_hit_test: Vec<f64> = Vec::new();
    let mut object_commit_ack_latency: Vec<f64> = Vec::new();
    for name in NAMES {
        draw.insert(name.into(), Vec::new());
        handler.insert(name.into(), Vec::new());
        frame.lock().unwrap().insert(name.into(), Vec::new());
        queue.insert(name.into(), Vec::new());
        action_lists.insert(name.into(), Vec::new());
        draw_wait.insert(name.into(), Vec::new());
        ui_draw.insert(name.into(), Vec::new());
        frame_draw.lock().unwrap().insert(name.into(), Vec::new());
        frame_command.lock().unwrap().insert(name.into(), Vec::new());
    }
    let current: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let ui_draw_ack: Arc<Mutex<Option<Ack>>> = Arc::new(Mutex::new(None));
    let ui_draw_ready: Arc<Mutex<Option<Supplier>>> = Arc::new(Mutex::new(None));
    let ui_listener = {
        let ui_draw_ack = ui_draw_ack.clone();
        let ui_draw_ready = ui_draw_ready.clone();
        jx::callback(move |method, _| {
            if method == "onDraw()" {
                let ack = ui_draw_ack.lock().unwrap().clone();
                let ready = ui_draw_ready.lock().unwrap().clone();
                if let Some(ack) = ack {
                    if ready.map(|r| r()).unwrap_or(true) {
                        // uiDrawAck.compareAndSet(ack, null)
                        let swapped = {
                            let mut slot = ui_draw_ack.lock().unwrap();
                            match &*slot {
                                Some(a) if Arc::ptr_eq(a, &ack) => {
                                    *slot = None;
                                    true
                                }
                                _ => false,
                            }
                        };
                        if swapped {
                            ack();
                        }
                    }
                }
            }
            Ok(Reply::Void)
        })
    };
    let listener = {
        let current = current.clone();
        let frame = frame.clone();
        let frame_draw = frame_draw.clone();
        let frame_command = frame_command.clone();
        jx::callback(move |method, args| {
            if method == "onFrameMetricsAvailable(Window,FrameMetrics,int)" {
                let name = current.lock().unwrap().clone();
                if let Some(name) = name {
                    let metrics = &args[1];
                    shared_push(&frame, &name, frame_metric(FrameMetrics(metrics), FrameMetricId(FRAMEMETRICS_TOTAL_DURATION))?.0 as f64 / 1e6);
                    shared_push(&frame_draw, &name, frame_metric(FrameMetrics(metrics), FrameMetricId(FRAMEMETRICS_DRAW_DURATION))?.0 as f64 / 1e6);
                    shared_push(&frame_command, &name, frame_metric(FrameMetrics(metrics), FrameMetricId(FRAMEMETRICS_COMMAND_ISSUE_DURATION))?.0 as f64 / 1e6);
                }
            }
            Ok(Reply::Void)
        })
    };
    test.run_on_main_sync(|| -> AppResult<()> {
        app.pan.set(true);
        app.erase.set(false);
        let canvas = app.canvas();
        canvas.zoom.set(1.0);
        canvas.offset_y.set(0.0);
        canvas.offset_x.set(0.0);
        let window = window(app)?;
        jcall!(window, "android/view/Window", "addOnFrameMetricsAvailableListener", "(Landroid/view/Window$OnFrameMetricsAvailableListener;Landroid/os/Handler;)V", &listener, &thread_handler(&frames)?)?;
        let decor = jcall!(window, "android/view/Window", "getDecorView", "()Landroid/view/View;")?.some();
        let observer = jcall!(decor, "android/view/View", "getViewTreeObserver", "()Landroid/view/ViewTreeObserver;")?.some();
        jcall!(observer, "android/view/ViewTreeObserver", "addOnDrawListener", "(Landroid/view/ViewTreeObserver$OnDrawListener;)V", &ui_listener)?;
        canvas.invalidate()?;
        Ok(())
    })??;
    test.wait_for_idle_sync()?;
    let start = performance::resources(StrokeCount(0))?;
    let coalesced_before = app.coalesced_object_frames.get();
    let mut mode_resources = Map::new();
    let mut page_tiles = Map::new();
    let original_layer = app.selected.get();
    let extra_layer = format!("performance-{}", String::from(ids::Uuid::random()));
    let history_id = format!("performance-history-{}", String::from(ids::Uuid::random()));
    let stroke_history_id = format!("performance-stroke-history-{}", String::from(ids::Uuid::random()));
    let create_prefix = format!("performance-create-{}-", String::from(ids::Uuid::random()));
    let selection_id = format!("performance-selection-{}", String::from(ids::Uuid::random()));
    let page_document = format!("performance-pages-{}", String::from(ids::Uuid::random()));
    let fill_document = format!("performance-fill-{}", String::from(ids::Uuid::random()));
    let body = (|| -> AppResult<()> {
        for name in NAMES {
            if name == "layer_switch" {
                command(app, &["layer", "put", "-"], object! {"layer" => extra_layer.as_str(), "name" => "Benchmark layer", "force" => false})?;
                let state = app.replica().read()?;
                test.run_on_main_sync(|| refresh(app, &state, || app.update()))??;
                test.wait_for_idle_sync()?;
            }
            if name == "selection_tap" {
                drain_writes()?;
                command(
                    app,
                    &["object", "put", "-"],
                    object! {"object" => selection_id.as_str(), "shape" => "rect",
                    "layer" => original_layer.as_str(), "page" => app.page.get(), "x" => 440, "y" => 260, "w" => 80, "h" => 80,
                    "color" => COLOR_BLACK, "fill" => COLOR_BLACK,
                    "width" => 4, "fontSize" => 24, "text" => ""},
                )?;
                let state = app.replica().read()?;
                test.run_on_main_sync(|| {
                    refresh(app, &state, || {
                        app.selecting.set(true);
                        app.update()
                    })
                })??;
                test.wait_for_idle_sync()?;
            }
            if name == "history_toggle" {
                command(
                    app,
                    &["object", "put", "-"],
                    object! {"kind" => "object", "object" => history_id.as_str(),
                    "shape" => "rect", "layer" => app.selected.get(), "page" => app.page.get(), "x" => 120, "y" => 140, "w" => 180, "h" => 110,
                    "color" => COLOR_BLACK, "width" => 5, "fontSize" => 24, "text" => ""},
                )?;
                let state = app.replica().read()?;
                test.run_on_main_sync(|| {
                    refresh(app, &state, || {
                        app.undo.with_mut(|u| u.clear());
                        app.redo.with_mut(|r| r.clear());
                        app.undo.with_mut(|u| u.push("object-action".into()));
                        app.save();
                        app.update()
                    })
                })??;
                test.wait_for_idle_sync()?;
            }
            if name == "stroke_history_toggle" {
                command(
                    app,
                    &["append", "-"],
                    object! {"kind" => "stroke", "stroke" => stroke_history_id.as_str(),
                    "layer" => app.selected.get(), "page" => app.page.get(), "width" => 3, "color" => COLOR_BLACK,
                    "points" => vec![Value::from(vec![160, 180, 1]), Value::from(vec![280, 220, 1])]},
                )?;
                let state = app.replica().read()?;
                test.run_on_main_sync(|| {
                    refresh(app, &state, || {
                        app.undo.with_mut(|u| u.clear());
                        app.redo.with_mut(|r| r.clear());
                        app.undo.with_mut(|u| u.push(stroke_history_id.clone()));
                        app.save();
                        app.update()
                    })
                })??;
                test.wait_for_idle_sync()?;
            }
            if name == "object_create" {
                drain_writes()?;
                test.run_on_main_sync(|| -> AppResult<()> {
                    app.undo.with_mut(|u| u.clear());
                    app.redo.with_mut(|r| r.clear());
                    app.save();
                    app.update()
                })??;
                drain_writes()?;
                test.wait_for_idle_sync()?;
            }
            if name == "page_switch" {
                drain_writes()?;
                test.run_on_main_sync(|| app.open_document(&notemaker_core::ids::DocumentName::from(&page_document), &crate::main_activity::DocumentType::from("pages")))??;
                drain_writes()?;
                for page_number in 0..2 {
                    command(
                        app,
                        &["object", "put", "-"],
                        object! {"kind" => "object",
                        "object" => format!("performance-page-{page_number}"), "shape" => "rect", "layer" => "base",
                        "page" => page_number, "x" => 120 + page_number * 90, "y" => 140, "w" => 180, "h" => 110,
                        "color" => COLOR_BLACK, "width" => 5, "fontSize" => 24, "text" => ""},
                    )?;
                }
                let state = app.replica().read()?;
                test.run_on_main_sync(|| {
                    refresh(app, &state, || {
                        app.update()?;
                        error::require(app.type_.get() == "pages" && app.page.get() == 0, "Page benchmark did not open its two-page document")
                    })
                })??;
                test.wait_for_idle_sync()?;
            }
            if name == "fill_region" {
                drain_writes()?;
                test.run_on_main_sync(|| app.open_document(&notemaker_core::ids::DocumentName::from(&fill_document), &crate::main_activity::DocumentType::from("fixed")))??;
                drain_writes()?;
                command(
                    app,
                    &["object", "put", "-"],
                    object! {"object" => "performance-fill-boundary", "shape" => "rect",
                    "layer" => "base", "page" => 0, "x" => 400, "y" => 400, "w" => 200, "h" => 160,
                    "color" => COLOR_BLACK, "width" => 4, "fontSize" => 24, "text" => ""},
                )?;
                let state = app.replica().read()?;
                test.run_on_main_sync(|| refresh(app, &state, || app.update()))??;
                test.wait_for_idle_sync()?;
            }
            let mode_start = performance::resources(StrokeCount(0))?;
            let tile_start = if name == "page_switch" { Some(tile_metrics(test, app)?) } else { None };
            *current.lock().unwrap() = Some(name.to_string());
            let history_mode = name == "history_toggle" || name == "stroke_history_toggle";
            let count = if name == "pan" || history_mode {
                60
            } else if name == "document_reopen" {
                12
            } else if name == "fill_region" {
                20
            } else {
                40
            };
            let warmup = if name == "document_reopen" {
                2
            } else if history_mode {
                6
            } else if name == "fill_region" {
                4
            } else {
                5
            };
            for i in 0..count {
                let index: i32 = i;
                let painted = Arc::new(CountDownLatch::new(1));
                let handled = Arc::new(CountDownLatch::new(1));
                let ui_painted = Arc::new(CountDownLatch::new(1));
                let committed = Arc::new(CountDownLatch::new(1));
                let drawn = Arc::new(AtomicI64::new(0));
                let ui_drawn = Arc::new(AtomicI64::new(0));
                let started = Arc::new(AtomicI64::new(0));
                let completed = Arc::new(AtomicI64::new(0));
                let committed_at = Arc::new(AtomicI64::new(0));
                let queued = jx::nano_time().0;
                let runnable = {
                    let app = app.clone();
                    let (painted, handled, ui_painted, committed) = (painted.clone(), handled.clone(), ui_painted.clone(), committed.clone());
                    let (drawn, ui_drawn, started, completed, committed_at) = (drawn.clone(), ui_drawn.clone(), started.clone(), completed.clone(), committed_at.clone());
                    let (ui_draw_ready, ui_draw_ack) = (ui_draw_ready.clone(), ui_draw_ack.clone());
                    let (selection_id, history_id, stroke_history_id) = (selection_id.clone(), history_id.clone(), stroke_history_id.clone());
                    let (extra_layer, original_layer, create_prefix) = (extra_layer.clone(), original_layer.clone(), create_prefix.clone());
                    jx::try_once_runnable(move || -> jx::JResult<()> {
                        (|| -> AppResult<()> {
                            started.store(jx::nano_time().0, Ordering::SeqCst);
                            let history = history_mode;
                            let stroke_history = name == "stroke_history_toggle";
                            let object_create = name == "object_create";
                            let fill_region = name == "fill_region";
                            let forward = index % 2 == 1;
                            let create_id = format!("{create_prefix}{index}");
                            let expected_fill_objects = app.model().objects.len() + 1;
                            let mut ready: Supplier = {
                                let app = app.clone();
                                let (selection_id, history_id, stroke_history_id, create_id) = (selection_id.clone(), history_id.clone(), stroke_history_id.clone(), create_id.clone());
                                Arc::new(move || {
                                    if name == "selection_tap" {
                                        app.selection_session.get().is_some() && app.selected_entities.with(|entities| entities.iter().any(|entity| entity.get("id").and_then(|v| v.as_str()) == Some(selection_id.as_str())))
                                    } else if object_create {
                                        app.model().objects.contains_key(&create_id) && !app.object_history_pending()
                                    } else {
                                        !history || (!app.history_pending.get() && {
                                            let model = app.model();
                                            let active = if stroke_history { &model.active } else { &model.object_active };
                                            let id = if stroke_history { &stroke_history_id } else { &history_id };
                                            if forward {
                                                not_false(active, id)
                                            } else {
                                                !not_false(active, id)
                                            }
                                        })
                                    }
                                })
                            };
                            if fill_region {
                                let app = app.clone();
                                ready = Arc::new(move || app.model().objects.len() >= expected_fill_objects && !app.object_history_pending());
                            }
                            let draw_ready = ready;
                            let ui_ready: Supplier = if object_create {
                                let app = app.clone();
                                let (create_id, draw_ready) = (create_id.clone(), draw_ready.clone());
                                Arc::new(move || app.pending_object_previews.with(|p| p.contains_key(&create_id)) || draw_ready())
                            } else {
                                draw_ready.clone()
                            };
                            *ui_draw_ready.lock().unwrap() = Some(ui_ready);
                            {
                                let (ui_drawn, ui_painted) = (ui_drawn.clone(), ui_painted.clone());
                                *ui_draw_ack.lock().unwrap() = Some(Arc::new(move || {
                                    ui_drawn.store(jx::nano_time().0, Ordering::SeqCst);
                                    ui_painted.count_down();
                                }));
                            }
                            let canvas = app.canvas();
                            {
                                let weak = Arc::downgrade(&app);
                                let (drawn, painted, create_id) = (drawn.clone(), painted.clone(), create_id.clone());
                                canvas.after_draw.set(Some(Arc::new(move || {
                                    let Some(app) = weak.upgrade() else { return };
                                    if drawn.load(Ordering::SeqCst) == 0
                                        && (if object_create { app.drawn_object_previews.with(|d| d.contains(&create_id)) || draw_ready() } else { draw_ready() })
                                    {
                                        drawn.store(jx::nano_time().0, Ordering::SeqCst);
                                        painted.count_down();
                                    }
                                })));
                            }
                            if name == "pan" {
                                app.pan.set(true);
                                let x = canvas.width().0 as f32 / 2.0;
                                let y = canvas.height().0 as f32 / 2.0;
                                let when = jx::uptime_millis().0;
                                let down = jstatic!("android/view/MotionEvent", "obtain", "(JJIFFI)Landroid/view/MotionEvent;", when, when, MOTIONEVENT_ACTION_DOWN, x, y, 0)?.some();
                                let move_ = jstatic!("android/view/MotionEvent", "obtain", "(JJIFFI)Landroid/view/MotionEvent;", when, when + 8, MOTIONEVENT_ACTION_MOVE, x + if index % 2 == 0 { 8.0 } else { -8.0 }, y, 0)?.some();
                                let view = canvas.view();
                                dispatch_touch(&view, &down)?;
                                dispatch_touch(&view, &move_)?;
                                recycle_event(&down)?;
                                recycle_event(&move_)?;
                            } else if name == "zoom_redraw" {
                                canvas.zoom.set(if index % 2 == 0 { 1.2 } else { 1.0 });
                                canvas.invalidate()?;
                            } else if name == "tool_switch" {
                                let rectangle = notemaker_core::ids::ShapeId::from("rectangle");
                                app.set_drawing_tool(if index % 2 == 0 { Some(&rectangle) } else { None })?;
                            } else if name == "layer_switch" {
                                app.select_layer(&notemaker_core::ids::LayerId::from(if index % 2 == 0 { &extra_layer } else { &original_layer }))?;
                            } else if name == "page_switch" {
                                app.deselect()?;
                                app.reset_drawing_gesture()?;
                                app.page.set(if index % 2 == 0 { 1 } else { 0 });
                                app.save();
                                app.update()?;
                            } else if name == "selection_tap" {
                                app.selecting.set(true);
                                app.select_at(crate::main_activity::DocPointF { x: 480.0, y: 300.0 })?;
                            } else if history {
                                app.history(if forward { notemaker_core::store::HistoryDirection::Redo } else { notemaker_core::store::HistoryDirection::Undo })?;
                            } else if object_create {
                                {
                                    let (create_id, committed_at, committed) = (create_id.clone(), committed_at.clone(), committed.clone());
                                    app.object_commit_ack.set(Some(Arc::new(move |id: &str| {
                                        if create_id == id {
                                            committed_at.store(jx::nano_time().0, Ordering::SeqCst);
                                            committed.count_down();
                                        }
                                    })));
                                }
                                let object = MainActivity::obj(&[
                                    ("kind", JV::from("object")),
                                    ("object", JV::from(create_id.as_str())),
                                    ("shape", JV::from("rect")),
                                    ("layer", JV::from(app.selected.get())),
                                    ("page", JV::from(app.page.get())),
                                    ("x", JV::from(100 + (index % 10) * 55)),
                                    ("y", JV::from(300 + (index / 10) * 55)),
                                    ("w", JV::from(42)),
                                    ("h", JV::from(36)),
                                    ("color", JV::from(COLOR_BLACK)),
                                    ("width", JV::from(3)),
                                    ("fontSize", JV::from(24)),
                                    ("text", JV::from("")),
                                ]);
                                app.persist_object(&object)?;
                            } else if fill_region {
                                app.fill_at(crate::main_activity::DocPointF { x: 500.0, y: 480.0 })?;
                            } else {
                                let type_ = app.type_.get();
                                app.load(&crate::main_activity::DocumentType::from(&type_))?;
                                app.configure_proxy()?;
                                app.update()?;
                            }
                            completed.store(jx::nano_time().0, Ordering::SeqCst);
                            handled.count_down();
                            Ok(())
                        })()
                        .map_err(Into::into)
                    })
                };
                crate::ui::post(&input, &runnable)?;
                let five = Duration::from_secs(5);
                if !handled.await_timeout(five)
                    || !ui_painted.await_timeout(five)
                    || (name != "layer_switch" && name != "tool_switch" && !painted.await_timeout(five))
                    || (name == "object_create" && !committed.await_timeout(five))
                {
                    return Err(AppError::illegal_state(format!("Interaction draw timed out: {name} #{index}")));
                }
                if name == "object_create" {
                    test.run_on_main_sync(|| app.object_commit_ack.set(None))?;
                }
                let ms = |a: &AtomicI64, b: i64| (a.load(Ordering::SeqCst) - b) as f64 / 1e6;
                if index >= warmup {
                    if name == "object_create" {
                        object_commit_ack_latency.push(ms(&committed_at, queued));
                    }
                    push(&mut handler, name, ms(&completed, queued));
                    push(&mut ui_draw, name, ms(&ui_drawn, queued));
                    push(&mut queue, name, ms(&started, queued));
                    push(&mut action_lists, name, ms(&completed, started.load(Ordering::SeqCst)));
                    if name != "layer_switch" && name != "tool_switch" {
                        push(&mut draw, name, ms(&drawn, queued));
                        push(&mut draw_wait, name, ms(&drawn, completed.load(Ordering::SeqCst)));
                    }
                    if name == "history_toggle" {
                        (if index % 2 == 0 { &mut undo_ui_draw } else { &mut redo_ui_draw }).push(ms(&ui_drawn, queued));
                        (if index % 2 == 0 { &mut undo_canvas_draw } else { &mut redo_canvas_draw }).push(ms(&drawn, queued));
                    }
                }
                if name == "selection_tap" {
                    let hit_ns = test.run_on_main_sync(|| -> AppResult<i64> {
                        let began = jx::nano_time().0;
                        let hit = app.selected_entity_at(crate::main_activity::DocPointF { x: 480.0, y: 300.0 })?;
                        let hit_ns = jx::nano_time().0 - began;
                        error::require(
                            hit.as_ref().and_then(|h| h.get("id")).and_then(|v| v.as_str()) == Some(selection_id.as_str()),
                            "Selected-object hit test missed the fixture",
                        )?;
                        Ok(hit_ns)
                    })??;
                    if index >= warmup {
                        selected_hit_test.push(hit_ns as f64 / 1e6);
                    }
                    test.run_on_main_sync(|| app.deselect())??;
                    drain_writes()?;
                    test.wait_for_idle_sync()?;
                    error::require(app.selection_session.get().is_none() && app.selected_entities.with(|e| e.is_empty()), "Selection benchmark left a lease active")?;
                }
                sleep(jx::DelayMillis(25));
            }
            if name == "history_toggle" {
                drain_writes()?;
                let durable = app.replica().read()?;
                let saved = store::model(&durable)?;
                error::require(saved.objects.contains_key(&history_id) && not_false(&saved.object_active, &history_id), "History toggle did not persist final object")?;
                test.run_on_main_sync(|| error::require(app.undo.with(|u| u.len()) == 1 && app.redo.with(|r| r.is_empty()), "History toggle did not restore editor history"))??;
            }
            if name == "stroke_history_toggle" {
                drain_writes()?;
                let durable = app.replica().read()?;
                let saved = store::model(&durable)?;
                error::require(saved.strokes.contains_key(&stroke_history_id) && not_false(&saved.active, &stroke_history_id), "Stroke history toggle did not persist final stroke")?;
                test.run_on_main_sync(|| {
                    error::require(
                        app.undo.with(|u| u.len() == 1 && u[0] == stroke_history_id) && app.redo.with(|r| r.is_empty()),
                        "Stroke history toggle did not restore editor history",
                    )
                })??;
            }
            if name == "object_create" {
                drain_writes()?;
                let durable = app.replica().read()?;
                let saved = store::model(&durable)?;
                for index in 0..40 {
                    error::require(saved.objects.contains_key(&format!("{create_prefix}{index}")), &format!("Object creation did not persist #{index}"))?;
                }
                test.run_on_main_sync(|| error::require(app.undo.with(|u| u.len()) == 40 && app.redo.with(|r| r.is_empty()), "Object creation did not persist editor history"))??;
            }
            if name == "page_switch" {
                drain_writes()?;
                let durable = app.replica().read()?;
                error::require(json::as_int(durable.get("page"))? == 0, "Page switch did not persist final page")?;
            }
            if name == "fill_region" {
                drain_writes()?;
                let saved = store::model(&app.replica().read()?)?;
                error::require(saved.objects.len() == 21, "Fill benchmark lost a region object")?;
                test.run_on_main_sync(|| error::require(app.undo.with(|u| u.len()) == 20 && app.redo.with(|r| r.is_empty()), "Fill benchmark did not persist editor history"))??;
            }
            test.wait_for_idle_sync()?;
            mode_resources.insert(name.into(), Value::Object(object! {"before" => mode_start, "after" => performance::resources(StrokeCount(0))?}));
            if let Some(tile_start) = tile_start {
                let tile_end = tile_metrics(test, app)?;
                let get = |key: &str| tile_end.get(key).cloned().unwrap_or(Value::Null);
                page_tiles = object! {
                    "tile_renders" => delta(&tile_start, &tile_end, "tileRenders"),
                    "item_renders" => delta(&tile_start, &tile_end, "itemRenders"),
                    "allocated_tiles" => delta(&tile_start, &tile_end, "allocatedTiles"),
                    "reused_tiles" => delta(&tile_start, &tile_end, "reusedTiles"),
                    "evicted_tiles" => delta(&tile_start, &tile_end, "evictedTiles"),
                    "cache_clears" => delta(&tile_start, &tile_end, "cacheClears"),
                    "resident_pages" => get("residentPages"),
                    "current_bytes" => get("currentBytes"),
                    "peak_bytes" => get("peakBytes"),
                };
            }
        }
        test.run_on_main_sync(|| -> AppResult<()> {
            app.canvas().after_draw.set(None);
            app.pan.set(false);
            app.set_drawing_tool(None)
        })??;
        test.wait_for_idle_sync()?;
        sleep(jx::DelayMillis(250));
        Ok(())
    })();
    // finally
    *current.lock().unwrap() = None;
    test.run_on_main_sync(|| -> AppResult<()> {
        app.canvas().after_draw.set(None);
        app.object_commit_ack.set(None);
        *ui_draw_ack.lock().unwrap() = None;
        *ui_draw_ready.lock().unwrap() = None;
        let window = window(app)?;
        jcall!(window, "android/view/Window", "removeOnFrameMetricsAvailableListener", "(Landroid/view/Window$OnFrameMetricsAvailableListener;)V", &listener)?;
        let decor = jcall!(window, "android/view/Window", "getDecorView", "()Landroid/view/View;")?.some();
        let observer = jcall!(decor, "android/view/View", "getViewTreeObserver", "()Landroid/view/ViewTreeObserver;")?.some();
        jcall!(observer, "android/view/ViewTreeObserver", "removeOnDrawListener", "(Landroid/view/ViewTreeObserver$OnDrawListener;)V", &ui_listener)?;
        Ok(())
    })??;
    quit_safely(&frames)?;
    body?;
    let end = performance::resources(StrokeCount(0))?;
    let mut modes = Map::new();
    performance::gc()?;
    sleep(jx::DelayMillis(500));
    let post_gc = performance::resources(StrokeCount(0))?;
    for name in NAMES {
        modes.insert(
            name.into(),
            Value::Object(object! {
                "handler" => performance::stats(LatencySamples(&handler[name])),
                "input_to_draw" => performance::stats(LatencySamples(&draw[name])),
                "input_to_ui_draw" => performance::stats(LatencySamples(&ui_draw[name])),
                "window_frame" => performance::stats(LatencySamples(&shared_values(&frame, name))),
                "queue" => performance::stats(LatencySamples(&queue[name])),
                "action" => performance::stats(LatencySamples(&action_lists[name])),
                "handler_to_draw" => performance::stats(LatencySamples(&draw_wait[name])),
                "frame_draw" => performance::stats(LatencySamples(&shared_values(&frame_draw, name))),
                "frame_command_issue" => performance::stats(LatencySamples(&shared_values(&frame_command, name))),
            }),
        );
    }
    let history_directions = object! {
        "undo" => object! {"input_to_ui_draw" => performance::stats(LatencySamples(&undo_ui_draw)), "input_to_draw" => performance::stats(LatencySamples(&undo_canvas_draw))},
        "redo" => object! {"input_to_ui_draw" => performance::stats(LatencySamples(&redo_ui_draw)), "input_to_draw" => performance::stats(LatencySamples(&redo_canvas_draw))},
    };
    let state_revision = app.replica().read()?.get("revision").cloned().unwrap_or(Value::Null);
    Ok(object! {
        "modes" => modes,
        "mode_resources" => mode_resources,
        "page_tile_cache" => page_tiles,
        "coalesced_object_frames" => app.coalesced_object_frames.get() - coalesced_before,
        "object_create_commit_ack" => performance::stats(LatencySamples(&object_commit_ack_latency)),
        "selected_hit_test" => performance::stats(LatencySamples(&selected_hit_test)),
        "start_resources" => start,
        "end_resources" => end,
        "post_gc_resources" => post_gc,
        "history_directions" => history_directions,
        "latency_definition" => "enqueue to first visible Canvas draw (preview or committed for object creation) and first durable updated window draw; 5 warmup actions per mode except 2 for document reopen and 6 for history; synthetic pan, viewport zoom redraw, direct page switching, and selection tap with lease acquisition; history modes alternate object or stroke undo and redo",
        "document" => app.document.get(),
        "state_revision" => state_revision,
    })
}
/// `private static Map<String,Object> tileMetrics(Instrumentation test, MainActivity app)`.
fn tile_metrics(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<Map> {
    Ok(test.run_on_main_sync(|| app.canvas().tile_cache_metrics())?)
}
/// `private static long delta(Map before, Map after, String key)`.
fn delta(before: &Map, after: &Map, key: &str) -> i64 {
    performance::long_metric(after, key) - performance::long_metric(before, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_false_matches_boolean_false_equals() {
        let mut m = IndexMap::new();
        m.insert("a".to_string(), false);
        m.insert("b".to_string(), true);
        assert!(!not_false(&m, "a"));
        assert!(not_false(&m, "b"));
        assert!(not_false(&m, "missing"));
    }

    #[test]
    fn delta_of_metrics() {
        let before = object! {"tileRenders" => 3};
        let after = object! {"tileRenders" => 10};
        assert_eq!(delta(&before, &after, "tileRenders"), 7);
    }
}
