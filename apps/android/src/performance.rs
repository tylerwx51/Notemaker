//! Port of `Performance.java` (instrumentation benchmark): synthetic stylus
//! strokes with frame metrics, latency percentiles and resource use.
//!
//! Real view input and window frame timing; run only in an isolated test
//! package or emulator. Runs on the instrumentation thread; input events are
//! posted to an asynchronous main-looper `Handler` and frame metrics arrive on
//! the `performance-frames` `HandlerThread`.

use crate::android_consts::*;
use crate::concurrent::CountDownLatch;
use crate::error::{self, AppError, AppResult};
use crate::integration::Integration;
use crate::jx::{self, Local, Obj, Reply};
use crate::main_activity::MainActivity;
use crate::replica_service;
use crate::tile_ink_cache::TileInkCache;
use notemaker_core::json::{self, Map, Value};
use notemaker_core::model::Model;
use notemaker_core::{object, store};
use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// `private static final int[] FRAME_PHASE_IDS`.
pub const FRAME_PHASE_IDS: [i32; 8] = [
    FRAMEMETRICS_UNKNOWN_DELAY_DURATION,
    FRAMEMETRICS_INPUT_HANDLING_DURATION,
    FRAMEMETRICS_ANIMATION_DURATION,
    FRAMEMETRICS_LAYOUT_MEASURE_DURATION,
    FRAMEMETRICS_DRAW_DURATION,
    FRAMEMETRICS_SYNC_DURATION,
    FRAMEMETRICS_COMMAND_ISSUE_DURATION,
    FRAMEMETRICS_SWAP_BUFFERS_DURATION,
];
/// `private static final String[] FRAME_PHASE_NAMES`.
pub const FRAME_PHASE_NAMES: [&str; 8] = ["unknown_delay", "input", "animation", "layout", "draw", "sync", "command_issue", "swap_buffers"];

// ---------------------------------------------------------------------------
// Shared helpers for the benchmark/check modules (Java inline idioms).

/// `Thread.sleep(ms)` on the instrumentation thread.
pub(crate) fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}
/// `ReplicaService.writes.submit(()->{}).get(30,TimeUnit.SECONDS)`.
pub(crate) fn drain_writes() -> AppResult<()> {
    replica_service::writes().submit(|| ()).get_timeout(Duration::from_secs(30))?;
    Ok(())
}
/// `app.getWindow()`.
pub(crate) fn window(app: &MainActivity) -> AppResult<Obj> {
    Ok(jcall!(app.this, "android/app/Activity", "getWindow", "()Landroid/view/Window;")?.some())
}
/// `app.openFileOutput(name, 0)`.
pub(crate) fn open_file_output(app: &MainActivity, name: &str) -> AppResult<Obj> {
    Ok(jcall!(app.this, "android/content/Context", "openFileOutput", "(Ljava/lang/String;I)Ljava/io/FileOutputStream;", name, 0)?.some())
}
/// `try(FileOutputStream out=app.openFileOutput(name,0)){out.write(bytes);}`.
pub(crate) fn write_file_output(app: &MainActivity, name: &str, bytes: &[u8]) -> AppResult<()> {
    let out = open_file_output(app, name)?;
    let array = jx::new_byte_array(bytes)?;
    let written = jcall!(out, "java/io/FileOutputStream", "write", "([B)V", &array);
    let closed = jcall!(out, "java/io/FileOutputStream", "close", "()V");
    written?;
    closed?;
    Ok(())
}
/// `HandlerThread frames=new HandlerThread(name); frames.start();`.
pub(crate) fn handler_thread(name: &str) -> AppResult<Obj> {
    let frames = jnew!("android/os/HandlerThread", "(Ljava/lang/String;)V", name)?;
    jcall!(frames, "java/lang/Thread", "start", "()V")?;
    Ok(frames)
}
/// `new Handler(frames.getLooper())`.
pub(crate) fn thread_handler(frames: &Obj) -> AppResult<Obj> {
    let looper = jcall!(frames, "android/os/HandlerThread", "getLooper", "()Landroid/os/Looper;")?.some();
    Ok(jnew!("android/os/Handler", "(Landroid/os/Looper;)V", &looper)?)
}
/// `frames.quitSafely()`.
pub(crate) fn quit_safely(frames: &Obj) -> AppResult<()> {
    jcall!(frames, "android/os/HandlerThread", "quitSafely", "()Z")?;
    Ok(())
}
/// `Handler.createAsync(Looper.getMainLooper())`.
pub(crate) fn async_main_handler() -> AppResult<Obj> {
    let looper = jstatic!("android/os/Looper", "getMainLooper", "()Landroid/os/Looper;")?.some();
    Ok(jstatic!("android/os/Handler", "createAsync", "(Landroid/os/Looper;)Landroid/os/Handler;", &looper)?.some())
}
/// `metrics.getMetric(id)` of a `FrameMetrics` callback argument.
pub(crate) fn frame_metric(metrics: &Local, id: i32) -> jx::JResult<i64> {
    Ok(jcall!(metrics, "android/view/FrameMetrics", "getMetric", "(I)J", id)?.long())
}
/// `System.gc()`.
pub(crate) fn gc() -> AppResult<()> {
    jstatic!("java/lang/System", "gc", "()V")?;
    Ok(())
}
/// `view.dispatchTouchEvent(e)`.
pub(crate) fn dispatch_touch(view: &Obj, event: &Obj) -> AppResult<bool> {
    Ok(jcall!(view, "android/view/View", "dispatchTouchEvent", "(Landroid/view/MotionEvent;)Z", event)?.bool())
}
/// `e.recycle()`.
pub(crate) fn recycle_event(event: &Obj) -> AppResult<()> {
    jcall!(event, "android/view/MotionEvent", "recycle", "()V")?;
    Ok(())
}
/// `MotionEvent.PointerProperties pp=new ...; pp.id=0; pp.toolType=MotionEvent.TOOL_TYPE_STYLUS;`.
fn stylus_properties() -> AppResult<Obj> {
    let pp = jnew!("android/view/MotionEvent$PointerProperties", "()V")?;
    jset!(pp, "android/view/MotionEvent$PointerProperties", "id", "I", 0)?;
    jset!(pp, "android/view/MotionEvent$PointerProperties", "toolType", "I", MOTIONEVENT_TOOL_TYPE_STYLUS)?;
    Ok(pp)
}
/// `pc.pressure=.5f; pc.size=1;`.
fn press(pc: &Obj) -> AppResult<()> {
    jset!(pc, "android/view/MotionEvent$PointerCoords", "pressure", "F", 0.5f32)?;
    jset!(pc, "android/view/MotionEvent$PointerCoords", "size", "F", 1f32)?;
    Ok(())
}
/// `MotionEvent.obtain(down,SystemClock.uptimeMillis(),action,1,new PointerProperties[]{pp},new PointerCoords[]{pc},0,0,1,1,0,0,InputDevice.SOURCE_STYLUS,0)`.
fn obtain_stylus(down: i64, action: i32, pp: &Obj, pc: &Obj) -> AppResult<Obj> {
    let properties = jx::new_object_array("android/view/MotionEvent$PointerProperties", &[Some(pp)])?;
    let coords = jx::new_object_array("android/view/MotionEvent$PointerCoords", &[Some(pc)])?;
    let now = jx::uptime_millis();
    Ok(jstatic!(
        "android/view/MotionEvent",
        "obtain",
        "(JJII[Landroid/view/MotionEvent$PointerProperties;[Landroid/view/MotionEvent$PointerCoords;IIFFIIII)Landroid/view/MotionEvent;",
        down,
        now,
        action,
        1,
        &properties,
        &coords,
        0,
        0,
        1f32,
        1f32,
        0,
        0,
        INPUTDEVICE_SOURCE_STYLUS,
        0
    )?
    .some())
}

/// The coordinates `position` writes into `pc.x`/`pc.y`.
pub fn position_xy(stroke: i32, point: i32, scale: f32, visible_width: f32, visible_height: f32) -> (f32, f32) {
    let columns = 1.max(20.min(((visible_width - 100.0) / 45.0) as i32 + 1));
    let rows = 1.max(50.min(((visible_height - 100.0) / 24.0) as i32 + 1));
    let cell = stroke % (columns * rows);
    let x = (30 + (cell % columns) * 45) as f32;
    let y = (40 + (cell / columns) * 24) as f32;
    if stroke % 100 == 99 {
        let angle = (point * 2) as f64 * std::f64::consts::PI / 499.0;
        ((x + 20.0 * notemaker_core::jmath::sin(2.0 * angle) as f32) * scale, (y + 20.0 * notemaker_core::jmath::sin(angle) as f32) * scale)
    } else {
        ((x + point as f32 * 0.45f32) * scale, (y + point as f32 * 0.15f32) * scale)
    }
}

/// `static void position(MotionEvent.PointerCoords pc, int stroke, int point, float scale, float visibleWidth, float visibleHeight)`.
pub fn position(pc: &Obj, stroke: i32, point: i32, scale: f32, visible_width: f32, visible_height: f32) -> AppResult<()> {
    let (x, y) = position_xy(stroke, point, scale, visible_width, visible_height);
    jset!(pc, "android/view/MotionEvent$PointerCoords", "x", "F", x)?;
    jset!(pc, "android/view/MotionEvent$PointerCoords", "y", "F", y)?;
    Ok(())
}
/// `static int bucket(int stroke)`.
pub fn bucket(stroke: i32) -> i32 {
    if stroke < 100 {
        0
    } else if (400..500).contains(&stroke) {
        1
    } else if stroke >= 900 {
        2
    } else {
        -1
    }
}
/// `static double percentile(List<Double> values, double fraction)`.
pub fn percentile(values: &[f64], fraction: f64) -> f64 {
    let mut sorted = values.to_vec();
    // Collections.sort on Double: Double.compareTo order.
    sorted.sort_by(|a, b| a.total_cmp(b));
    if sorted.is_empty() {
        0.0
    } else {
        sorted[(sorted.len() - 1).min((sorted.len() as f64 * fraction) as i32 as usize)]
    }
}
/// `static Map<String,Object> stats(List<Double> values)`.
pub fn stats(values: &[f64]) -> Map {
    object! {
        "over_30_ms" => values.iter().filter(|v| **v >= 30.0).count() as i64,
        "samples" => values.len() as i64,
        "p50_ms" => json::double(percentile(values, 0.50)),
        "p95_ms" => json::double(percentile(values, 0.95)),
        "p99_ms" => json::double(percentile(values, 0.99)),
        "max_ms" => json::double(percentile(values, 1.0)),
    }
}
/// `private static void prewarmGraphics(Instrumentation test, MainActivity app) throws Exception`.
pub fn prewarm_graphics(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<()> {
    test.run_on_main_sync(|| -> AppResult<()> {
        app.document.set(format!("performance-warmup-{}", jx::current_time_millis()));
        app.host.set("http://127.0.0.1:1".into());
        app.load("fixed")?;
        app.configure_proxy()?;
        app.update()?;
        app.set_drawing_tool(None)?;
        app.width.set(10.0);
        Ok(())
    })??;
    // Exercise the complete pen-to-window path before measuring a fresh
    // document. GPU command setup can stall one of the first rendered frames.
    for stroke in 0..40 {
        let number = stroke;
        test.run_on_main_sync(|| -> AppResult<()> {
            let down = jx::uptime_millis();
            let canvas = app.canvas();
            let scale = canvas.scale();
            for step in 0..3 {
                let pp = stylus_properties()?;
                let pc = jnew!("android/view/MotionEvent$PointerCoords", "()V")?;
                position(&pc, number, step * 24, scale, canvas.width() as f32 / scale, canvas.height() as f32 / scale)?;
                press(&pc)?;
                let action = if step == 0 {
                    MOTIONEVENT_ACTION_DOWN
                } else if step == 2 {
                    MOTIONEVENT_ACTION_UP
                } else {
                    MOTIONEVENT_ACTION_MOVE
                };
                let e = obtain_stylus(down, action, &pp, &pc)?;
                dispatch_touch(&canvas.view(), &e)?;
                recycle_event(&e)?;
            }
            Ok(())
        })??;
        sleep(18);
    }
    drain_writes()
}

/// A `List<Double>` wrapped by `Collections.synchronizedList`.
type Samples = Arc<Mutex<Vec<f64>>>;
fn new_samples() -> Samples {
    Arc::new(Mutex::new(Vec::new()))
}
fn add(list: &Samples, value: f64) {
    list.lock().unwrap().push(value);
}
fn values(list: &Samples) -> Vec<f64> {
    list.lock().unwrap().clone()
}
/// `long[] eventTiming={queued,phase,stroke,step,before,after,postCall}`.
type EventTiming = Arc<[AtomicI64; 7]>;

/// `Map<Thread,StackTraceElement[]>` of the main and `replica-writer` threads
/// (`Thread.getAllStackTraces()`), as `name → Arrays.toString(trace)`.
fn stall_stacks() -> AppResult<Map> {
    let mut stacks = Map::new();
    let main_looper = jstatic!("android/os/Looper", "getMainLooper", "()Landroid/os/Looper;")?.some();
    let main_thread = jcall!(main_looper, "android/os/Looper", "getThread", "()Ljava/lang/Thread;")?.some();
    let all = jstatic!("java/lang/Thread", "getAllStackTraces", "()Ljava/util/Map;")?.some();
    let entries = jcall!(all, "java/util/Map", "entrySet", "()Ljava/util/Set;")?.some();
    let iterator = jcall!(entries, "java/util/Set", "iterator", "()Ljava/util/Iterator;")?.some();
    while jcall!(iterator, "java/util/Iterator", "hasNext", "()Z")?.bool() {
        let entry = jcall!(iterator, "java/util/Iterator", "next", "()Ljava/lang/Object;")?.some();
        let thread = jcall!(entry, "java/util/Map$Entry", "getKey", "()Ljava/lang/Object;")?.some();
        let name = jcall!(thread, "java/lang/Thread", "getName", "()Ljava/lang/String;")?.string().unwrap_or_default();
        if thread.same(&main_thread) || name == "replica-writer" {
            let trace = jcall!(entry, "java/util/Map$Entry", "getValue", "()Ljava/lang/Object;")?.global();
            let text = jstatic!("java/util/Arrays", "toString", "([Ljava/lang/Object;)Ljava/lang/String;", &trace)?.string().unwrap_or_else(|| "null".into());
            stacks.insert(name, Value::from(text));
        }
    }
    Ok(stacks)
}

/// `static Map<String,Object> run(Instrumentation test, MainActivity app) throws Exception`.
pub fn run(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<Map> {
    prewarm_graphics(test, app)?;
    let samples: Arc<Vec<Vec<Samples>>> = Arc::new((0..3).map(|_| (0..4).map(|_| new_samples()).collect()).collect());
    let frame_phases: Arc<Vec<Vec<Samples>>> = Arc::new((0..3).map(|_| (0..FRAME_PHASE_IDS.len()).map(|_| new_samples()).collect()).collect());
    let stroke_latency = new_samples();
    let figure_eight_latency = new_samples();
    let phase_latency: Arc<Vec<Samples>> = Arc::new((0..3).map(|_| new_samples()).collect());
    // Main-thread pending events: a draw acknowledges every coalesced event, so
    // missing/slow MOVE and UP events cannot disappear from the latency budget.
    let pending_draw: Arc<Mutex<Vec<EventTiming>>> = Arc::new(Mutex::new(Vec::new()));
    let slow_events: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let mut dispatch_stalls: Vec<Value> = Vec::new();
    let current = Arc::new(AtomicI32::new(-1));
    // Android input messages bypass traversal synchronization barriers. A regular
    // runOnMainSync message does not, adding an artificial extra vsync of delay.
    let input = async_main_handler()?;
    let frames = handler_thread("performance-frames")?;
    let listener = {
        let current = current.clone();
        let samples = samples.clone();
        let frame_phases = frame_phases.clone();
        jx::callback(move |method, args| {
            if method == "onFrameMetricsAvailable(Window,FrameMetrics,int)" {
                let b = bucket(current.load(Ordering::SeqCst));
                if current.load(Ordering::SeqCst) >= 10 && b >= 0 {
                    let metrics = &args[1];
                    add(&samples[b as usize][3], frame_metric(metrics, FRAMEMETRICS_TOTAL_DURATION)? as f64 / 1e6);
                    for (m, id) in FRAME_PHASE_IDS.iter().enumerate() {
                        add(&frame_phases[b as usize][m], frame_metric(metrics, *id)? as f64 / 1e6);
                    }
                }
            }
            Ok(Reply::Void)
        })
    };
    test.run_on_main_sync(|| -> AppResult<()> {
        app.document.set(format!("performance-{}", jx::current_time_millis()));
        app.host.set("http://127.0.0.1:1".into());
        app.load("fixed")?;
        app.configure_proxy()?;
        app.update()?;
        let editor = crate::ui::edit(&app.this)?;
        crate::ui::put_string(&editor, "document", &app.document.get())?;
        crate::ui::put_string(&editor, "host", &app.host.get())?;
        jcall!(editor, "android/content/SharedPreferences$Editor", "commit", "()Z")?;
        let canvas = app.canvas();
        {
            let pending_draw = pending_draw.clone();
            let stroke_latency = stroke_latency.clone();
            let phase_latency = phase_latency.clone();
            let figure_eight_latency = figure_eight_latency.clone();
            let slow_events = slow_events.clone();
            canvas.after_draw.set(Some(Arc::new(move || {
                let drawn = jx::nano_time();
                let mut pending = pending_draw.lock().unwrap();
                for event in pending.iter() {
                    let e: [i64; 7] = std::array::from_fn(|i| event[i].load(Ordering::SeqCst));
                    let ms = (drawn - e[0]) as f64 / 1e6;
                    add(&stroke_latency, ms);
                    add(&phase_latency[e[1] as usize], ms);
                    if e[2] % 100 == 99 {
                        add(&figure_eight_latency, ms);
                    }
                    let mut slow = slow_events.lock().unwrap();
                    if ms >= 30.0 && slow.len() < 20 {
                        slow.push(Value::Object(object! {
                            "recorded_wall_ms" => jx::current_time_millis(),
                            "post_call_ms" => if e[6] == 0 { Value::Null } else { json::double((e[6] - e[0]) as f64 / 1e6) },
                            "stroke" => e[2] + 1,
                            "step" => e[3],
                            "total_ms" => json::double(ms),
                            "queue_ms" => json::double((e[4] - e[0]) as f64 / 1e6),
                            "handler_ms" => json::double((e[5] - e[4]) as f64 / 1e6),
                            "draw_wait_ms" => json::double((drawn - e[5]) as f64 / 1e6),
                        }));
                    }
                }
                pending.clear();
            })));
        }
        {
            let current = current.clone();
            let samples = samples.clone();
            canvas.draw_latency.set(Some(Arc::new(move |n: i64| {
                let b = bucket(current.load(Ordering::SeqCst));
                if current.load(Ordering::SeqCst) >= 10 && b >= 0 {
                    add(&samples[b as usize][2], n as f64 / 1e6);
                }
            })));
        }
        let handler = thread_handler(&frames)?;
        let window = window(app)?;
        jcall!(window, "android/view/Window", "addOnFrameMetricsAvailableListener", "(Landroid/view/Window$OnFrameMetricsAvailableListener;Landroid/os/Handler;)V", &listener, &handler)?;
        Ok(())
    })??;
    sleep(1000);
    let mut max_saving = 0;
    let start = jx::nano_time();
    let initial_tile_metrics = tile_metrics(test, app)?;
    let mut resources_list: Vec<Value> = vec![Value::Object(resources(0)?)];
    for stroke in 0..1000 {
        jx::frame(|| -> AppResult<()> {
            current.store(stroke, Ordering::SeqCst);
            let number = stroke;
            let b = bucket(stroke);
            let down = jx::uptime_millis();
            for step in 0..8 {
                let index = step;
                let handled = Arc::new(CountDownLatch::new(1));
                let queued = jx::nano_time();
                let event_timing: EventTiming = Arc::new([
                    AtomicI64::new(queued),
                    AtomicI64::new(if index == 0 { 0 } else if index == 7 { 2 } else { 1 }),
                    AtomicI64::new(number as i64),
                    AtomicI64::new(index as i64),
                    AtomicI64::new(0),
                    AtomicI64::new(0),
                    AtomicI64::new(0),
                ]);
                let runnable = {
                    let app = app.clone();
                    let handled = handled.clone();
                    let event_timing = event_timing.clone();
                    let pending_draw = pending_draw.clone();
                    let samples = samples.clone();
                    jx::try_once_runnable(move || -> jx::JResult<()> {
                        (|| -> AppResult<()> {
                            let pp = stylus_properties()?;
                            let pc = jnew!("android/view/MotionEvent$PointerCoords", "()V")?;
                            let canvas = app.canvas();
                            let scale = canvas.scale();
                            let batch = if number % 100 == 99 { 83 } else { 8 };
                            let point = if index == 0 {
                                0
                            } else if index == 7 {
                                6 * batch + 1
                            } else {
                                1 + (index - 1) * batch
                            };
                            let visible_width = canvas.width() as f32 / scale;
                            let visible_height = canvas.height() as f32 / scale;
                            position(&pc, number, point, scale, visible_width, visible_height)?;
                            press(&pc)?;
                            let action = if index == 0 {
                                MOTIONEVENT_ACTION_DOWN
                            } else if index == 7 {
                                MOTIONEVENT_ACTION_UP
                            } else {
                                MOTIONEVENT_ACTION_MOVE
                            };
                            let e = obtain_stylus(down, action, &pp, &pc)?;
                            if action == MOTIONEVENT_ACTION_MOVE {
                                for history in 1..batch {
                                    position(&pc, number, point + history, scale, visible_width, visible_height)?;
                                    let coords = jx::new_object_array("android/view/MotionEvent$PointerCoords", &[Some(&pc)])?;
                                    jcall!(e, "android/view/MotionEvent", "addBatch", "(J[Landroid/view/MotionEvent$PointerCoords;I)V", jx::uptime_millis(), &coords, 0)?;
                                }
                            }
                            let view = canvas.view();
                            let before = jx::nano_time();
                            dispatch_touch(&view, &e)?;
                            let after = jx::nano_time();
                            recycle_event(&e)?;
                            event_timing[4].store(before, Ordering::SeqCst);
                            event_timing[5].store(after, Ordering::SeqCst);
                            if number >= 10 {
                                pending_draw.lock().unwrap().push(event_timing.clone());
                            }
                            if number >= 10 && b >= 0 {
                                add(&samples[b as usize][0], (after - before) as f64 / 1e6);
                                add(&samples[b as usize][1], (after - queued) as f64 / 1e6);
                            }
                            handled.count_down();
                            Ok(())
                        })()
                        .map_err(Into::into)
                    })
                };
                crate::ui::post(&input, &runnable)?;
                event_timing[6].store(jx::nano_time(), Ordering::SeqCst);
                if !handled.await_timeout(Duration::from_millis(30)) {
                    let stacks = stall_stacks()?;
                    dispatch_stalls.push(Value::Object(object! {
                        "stroke" => number + 1,
                        "step" => index,
                        "wall_ms" => jx::current_time_millis(),
                        "wait_ms" => json::double((jx::nano_time() - queued) as f64 / 1e6),
                        "stacks" => stacks,
                    }));
                    error::require(handled.await_timeout(Duration::from_secs(5)), "Input dispatch timed out")?;
                }
                // Sample at 250 Hz; real window frames still determine draw latency.
                sleep(6);
            }
            let pending = test.run_on_main_sync(|| app.saving.get())?;
            max_saving = max_saving.max(pending);
            if stroke % 100 == 99 {
                resources_list.push(Value::Object(resources(stroke + 1)?));
                jx::log::i("NoteMakerPerformance", &format!("strokes={} saveQueue={}", stroke + 1, pending));
            }
            Ok(())
        })?;
    }
    let drain_start = jx::nano_time();
    drain_writes()?;
    test.wait_for_idle_sync()?;
    sleep(500);
    // The physical E Ink window may schedule the last traversal after the
    // fixed grace period. Wait for a natural draw before declaring loss.
    let draw_deadline = jx::uptime_millis() + 3000;
    loop {
        let undrawn = test.run_on_main_sync(|| pending_draw.lock().unwrap().len())?;
        if undrawn == 0 {
            break;
        }
        sleep(50);
        if jx::uptime_millis() >= draw_deadline {
            break;
        }
    }
    let state = app.store.get().expect("store").read()?;
    let effective = store::model(&state)?;
    let replay = Model::from_state(&state)?;
    error::require(json::write_map(&effective.json()) == json::write_map(&replay.json()), "Incremental model differs from full replay")?;
    error::require(effective.strokes.len() == 1000, &format!("Lost strokes: {}", effective.strokes.len()))?;
    let final_tile_metrics = test.run_on_main_sync(|| -> AppResult<Map> {
        {
            let pending = pending_draw.lock().unwrap();
            let last = match pending.last() {
                None => "none".to_string(),
                Some(e) => (e[2].load(Ordering::SeqCst) + 1).to_string(),
            };
            error::require(pending.is_empty(), &format!("Input never reached a draw: {} queued, last stroke {}", pending.len(), last))?;
        }
        let canvas = app.canvas();
        let metrics = canvas.tile_cache_metrics();
        canvas.after_draw.set(None);
        canvas.draw_latency.set(None);
        let window = window(app)?;
        jcall!(window, "android/view/Window", "removeOnFrameMetricsAvailableListener", "(Landroid/view/Window$OnFrameMetricsAvailableListener;)V", &listener)?;
        Ok(metrics)
    })??;
    quit_safely(&frames)?;
    let f = &final_tile_metrics;
    let i = &initial_tile_metrics;
    let get = |key: &str| f.get(key).cloned().unwrap_or(Value::Null);
    let mut tile_cache = object! {
        "tile_renders" => metric_delta(f, i, "tileRenders"),
        "item_renders" => metric_delta(f, i, "itemRenders"),
        "geometry_renders" => metric_delta(f, i, "geometryRenders"),
        "path_command_reuses" => metric_delta(f, i, "pathCommandReuses"),
        "transient_path_command_reuses" => metric_delta(f, i, "transientPathCommandReuses"),
        "path_command_misses" => metric_delta(f, i, "pathCommandMisses"),
        "path_command_evictions" => metric_delta(f, i, "pathCommandEvictions"),
        "reused_tiles" => metric_delta(f, i, "reusedTiles"),
        "current_bytes" => get("currentBytes"),
        "peak_bytes" => get("peakBytes"),
        "max_bytes" => get("maxBytes"),
        "path_command_bytes_estimate" => get("pathCommandBytesEstimate"),
        "path_command_points_estimate" => get("pathCommandPointsEstimate"),
        "max_path_command_bytes_estimate" => get("maxPathCommandBytesEstimate"),
        "max_path_command_points_estimate" => get("maxPathCommandPointsEstimate"),
    };
    tile_cache.insert("reused_tile_repaints".into(), Value::from(metric_delta(f, i, "reusedTileRepaints")));
    let timed_end = jx::nano_time();
    let bounds_cache = TileInkCache::bounds_cache_metrics();
    gc()?;
    sleep(1000);
    let after_gc = resources(1000)?;
    let report = object! {
        "samples" => resources_list,
        "after_gc" => after_gc,
        "bounds_cache" => bounds_cache,
        "sampling" => "Process counters sampled between every 100 strokes; cost is outside input latency but included in elapsed time. After-GC resources and bounds-cache occupancy are sampled after the timed workload. I/O counters are cumulative, not storage latency.",
    };
    write_file_output(app, "performance-resources.json", json::write_map(&report).as_bytes())?;
    let mut windows: Vec<Value> = Vec::new();
    let names = ["1-100 (10 warmup)", "401-500", "901-1000"];
    for b in 0..3 {
        let mut phases = Map::new();
        for m in 0..FRAME_PHASE_IDS.len() {
            phases.insert(FRAME_PHASE_NAMES[m].into(), Value::Object(stats(&values(&frame_phases[b][m]))));
        }
        windows.push(Value::Object(object! {
            "strokes" => names[b],
            "handler" => stats(&values(&samples[b][0])),
            "dispatch" => stats(&values(&samples[b][1])),
            "input_to_draw" => stats(&values(&samples[b][2])),
            "frame" => stats(&values(&samples[b][3])),
            "frame_phases" => phases,
        }));
    }
    let dispatch_stalls_value = dispatch_stalls;
    let slow_events_value = slow_events.lock().unwrap().clone();
    Ok(object! {
        "dispatch_stalls" => dispatch_stalls_value,
        "slow_events" => slow_events_value,
        "latency_definition" => "UI enqueue to end of canvas draw; every event after 10 warmup strokes, including coalesced events",
        "stroke_latency" => stats(&values(&stroke_latency)),
        "figure_eight_latency" => stats(&values(&figure_eight_latency)),
        "down_latency" => stats(&values(&phase_latency[0])),
        "move_latency" => stats(&values(&phase_latency[1])),
        "up_latency" => stats(&values(&phase_latency[2])),
        "document" => app.document.get(),
        "strokes" => 1000,
        "short_strokes" => 990,
        "short_stroke_points" => 50,
        "figure_eight_strokes" => 10,
        "figure_eight_points" => 500,
        "elapsed_seconds" => json::double((timed_end - start) as f64 / 1e9),
        "drain_ms" => json::double((timed_end - drain_start) as f64 / 1e6),
        "max_save_queue" => max_saving,
        "tile_cache" => tile_cache,
        "incremental_matches_replay" => true,
        "windows" => windows,
    })
}

/// Java `String.split(":")` (trailing empty strings removed).
fn split_colon(line: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = line.split(':').collect();
    while !parts.is_empty() && parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}
/// `Long.parseLong(s)` (decimal, optional sign).
fn parse_long(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok()
}
/// `/proc/self/io` counters, or `unavailable` with the failure's simple class name.
fn proc_io() -> Map {
    let mut io = Map::new();
    let text = match std::fs::read_to_string("/proc/self/io") {
        Ok(t) => t,
        Err(e) => {
            let name = if e.kind() == std::io::ErrorKind::NotFound || e.kind() == std::io::ErrorKind::PermissionDenied { "FileNotFoundException" } else { "IOException" };
            io.insert("unavailable".into(), Value::from(name));
            return io;
        }
    };
    for line in text.lines() {
        let parts = split_colon(line);
        if parts.len() == 2 {
            match parse_long(notemaker_core::jfmt::java_trim(parts[1])) {
                Some(v) => {
                    io.insert(parts[0].into(), Value::from(v));
                }
                None => {
                    io.insert("unavailable".into(), Value::from("NumberFormatException"));
                    break;
                }
            }
        }
    }
    io
}

/// `static Map<String,Object> resources(int strokes)`.
pub fn resources(strokes: i32) -> AppResult<Map> {
    let start = jx::nano_time();
    let memory = jnew!("android/os/Debug$MemoryInfo", "()V")?;
    jstatic!("android/os/Debug", "getMemoryInfo", "(Landroid/os/Debug$MemoryInfo;)V", &memory)?;
    let mut memory_parts = Map::new();
    for category in ["java-heap", "native-heap", "code", "stack", "graphics", "private-other", "system", "total-swap"] {
        let value = jcall!(memory, "android/os/Debug$MemoryInfo", "getMemoryStat", "(Ljava/lang/String;)Ljava/lang/String;", format!("summary.{category}"))?.string();
        if let Some(value) = value {
            if let Some(v) = parse_long(&value) {
                memory_parts.insert(format!("{category}_kib"), Value::from(v));
            }
        }
    }
    let io = proc_io();
    let runtime = jstatic!("java/lang/Runtime", "getRuntime", "()Ljava/lang/Runtime;")?.some();
    let total = jcall!(runtime, "java/lang/Runtime", "totalMemory", "()J")?.long();
    let free = jcall!(runtime, "java/lang/Runtime", "freeMemory", "()J")?.long();
    let pss = jcall!(memory, "android/os/Debug$MemoryInfo", "getTotalPss", "()I")?.int();
    let private_dirty = jcall!(memory, "android/os/Debug$MemoryInfo", "getTotalPrivateDirty", "()I")?.int();
    let native_allocated = jstatic!("android/os/Debug", "getNativeHeapAllocatedSize", "()J")?.long();
    let cpu = jstatic!("android/os/Process", "getElapsedCpuTime", "()J")?.long();
    Ok(object! {
        "strokes" => strokes,
        "pss_kib" => pss,
        "private_dirty_kib" => private_dirty,
        "memory_parts" => memory_parts,
        "java_used_bytes" => total - free,
        "native_allocated_bytes" => native_allocated,
        "process_cpu_ms" => cpu,
        "io" => io,
        "sampling_ms" => json::double((jx::nano_time() - start) as f64 / 1e6),
    })
}
/// `private static Map<String,Object> tileMetrics(Instrumentation test, MainActivity app)`.
pub fn tile_metrics(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<Map> {
    Ok(test.run_on_main_sync(|| app.canvas().tile_cache_metrics())?)
}
/// `((Number)m.get(key)).longValue()`: Java threw (NPE/ClassCast) for a
/// missing or non-numeric metric; this panics with the same exception text.
pub(crate) fn long_metric(m: &Map, key: &str) -> i64 {
    json::as_long(m.get(key)).unwrap_or_else(|e| panic!("{}", AppError::from(e)))
}
/// `private static long metricDelta(Map after, Map before, String key)`.
pub fn metric_delta(after: &Map, before: &Map, key: &str) -> i64 {
    long_metric(after, key) - long_metric(before, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets() {
        assert_eq!(bucket(0), 0);
        assert_eq!(bucket(99), 0);
        assert_eq!(bucket(100), -1);
        assert_eq!(bucket(400), 1);
        assert_eq!(bucket(499), 1);
        assert_eq!(bucket(500), -1);
        assert_eq!(bucket(900), 2);
        assert_eq!(bucket(999), 2);
    }

    #[test]
    fn percentiles_and_stats() {
        assert_eq!(percentile(&[], 0.5), 0.0);
        let v = [5.0, 1.0, 40.0, 3.0];
        assert_eq!(percentile(&v, 0.5), 5.0);
        assert_eq!(percentile(&v, 1.0), 40.0);
        assert_eq!(percentile(&v, 0.0), 1.0);
        let s = stats(&v);
        assert_eq!(json::write_map(&s), r#"{"over_30_ms":1,"samples":4,"p50_ms":5.0,"p95_ms":40.0,"p99_ms":40.0,"max_ms":40.0}"#);
        assert_eq!(json::write_map(&stats(&[])), r#"{"over_30_ms":0,"samples":0,"p50_ms":0.0,"p95_ms":0.0,"p99_ms":0.0,"max_ms":0.0}"#);
    }

    #[test]
    fn positions() {
        // 1080x1800 view at scale 1: 20 columns, 50 rows.
        assert_eq!(position_xy(0, 0, 1.0, 1080.0, 1800.0), (30.0, 40.0));
        assert_eq!(position_xy(21, 10, 2.0, 1080.0, 1800.0), ((75.0 + 4.5) * 2.0, (64.0 + 1.5) * 2.0));
        let (x, y) = position_xy(99, 0, 1.0, 1080.0, 1800.0);
        assert_eq!((x, y), (30.0 + 19.0 * 45.0, 40.0 + 4.0 * 24.0));
        // Narrow views clamp to one column/row.
        assert_eq!(position_xy(7, 0, 1.0, 10.0, 10.0), (30.0, 40.0));
    }

    #[test]
    fn colon_split() {
        assert_eq!(split_colon("rchar: 12"), vec!["rchar", " 12"]);
        assert_eq!(split_colon("a:"), vec!["a"]);
        assert_eq!(split_colon("a:b:c").len(), 3);
        assert_eq!(parse_long("12"), Some(12));
        assert_eq!(parse_long("+12"), Some(12));
        assert_eq!(parse_long("1 2"), None);
        assert_eq!(parse_long(""), None);
    }
}
