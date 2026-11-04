//! Port of `StrokeCheck.java` (instrumentation check): live strokes and
//! their committed pixels must match. Instrumentation thread; UI work via
//! `Integration::run_on_main_sync`.

use crate::android_consts::*;
use crate::concurrent::CountDownLatch;
use crate::error::{self, AppError, AppResult};
use crate::ink_cache::BitmapSize;
use crate::integration::{self as it, InputSource, Integration, PointerId, PointerTool, Pointers, TouchSize};
use crate::jx::{self, Obj};
use crate::main_activity::{MainActivity, ScreenPoint, StylusPressure};
use crate::org_json::JSONObject;
use crate::replica_service;
use notemaker_core::json::Value;
use notemaker_core::{ids, object, store};
use std::sync::Arc;
use std::time::Duration;

/// `static void onMain(Integration test, Runnable task)`: runs `task` on the
/// main thread and rethrows its failure on the calling thread.
pub fn on_main(test: &Arc<Integration>, task: impl FnOnce() -> AppResult<()> + Send) -> AppResult<()> {
    test.run_on_main_sync(task)?
}
/// `static Bitmap snapshot(MainActivity app)` (UI thread).
pub fn snapshot(app: &Arc<MainActivity>) -> AppResult<Obj> {
    let canvas = app.canvas();
    let image = it::create_bitmap(BitmapSize { width: canvas.width().0, height: canvas.height().0 })?;
    it::draw_view(&canvas.view(), &image)?;
    Ok(image)
}
/// `static void equal(MainActivity app, Bitmap before, Bitmap after, String stage)`.
fn equal(app: &Arc<MainActivity>, before: &Obj, after: &Obj, stage: &str) -> AppResult<()> {
    if it::same_as(before, after)? {
        return Ok(());
    }
    let a = crate::android_canvas::bitmap_pixels(before)?;
    let mut b = a.0.clone();
    let after_pixels = crate::android_canvas::bitmap_pixels(after)?;
    // `after.getPixels(b, ...)` into an array sized like `before`.
    let n = b.len().min(after_pixels.len());
    b[..n].copy_from_slice(&after_pixels[..n]);
    let differences = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
    let saved = (|| -> AppResult<()> {
        for i in 0..2 {
            it::write_png(app, &format!("stroke-{}.png", if i == 0 { "before" } else { "after" }), if i == 0 { before } else { after })?;
        }
        Ok(())
    })();
    saved.map_err(|e| AppError::runtime_cause(&e))?;
    Err(AppError::illegal_state(format!("{stage}: {differences} canvas pixels changed")))
}
/// `static void input(MainActivity app, long down, int action, float x, float y, float pressure)` (UI thread).
fn input(app: &Arc<MainActivity>, down: i64, action: i32, x: f32, y: f32, pressure: f32) -> AppResult<()> {
    let property = it::pointer_properties(PointerId(0), PointerTool::Stylus)?;
    let canvas = app.canvas();
    let scale = canvas.scale().0 * canvas.zoom.get();
    let point = ScreenPoint { x: x * scale + canvas.offset_x.get(), y: y * scale + canvas.offset_y.get() };
    let coords = it::pointer_coords(point, StylusPressure(pressure), TouchSize(1.0))?;
    let event = it::obtain(jx::UptimeMillis(down), jx::uptime_millis(), MotionAction(action), &Pointers::single(property, coords), InputSource::Stylus)?;
    let dispatched = jcall!(canvas.view(), "android/view/View", "dispatchTouchEvent", "(Landroid/view/MotionEvent;)Z", &event);
    let recycled = it::recycle(&event);
    dispatched?;
    recycled
}
/// `static void run(Integration test, MainActivity app) throws Exception`.
pub fn run(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<()> {
    let brushes = ["pen", "marker", "pencil", "brush", "oily", "fountain", "charcoal", "neoBrush", "latinCalligraphy", "asiaCalligraphy", "square"];
    for brush in brushes {
        for transformed in [false, true] {
            jx::frame(|| run_case(test, app, brush, transformed))?;
        }
    }
    crate::stroke_crossings::run()
}

/// One `brush` × `transformed` iteration of `run`.
fn run_case(test: &Arc<Integration>, app: &Arc<MainActivity>, brush: &str, transformed: bool) -> AppResult<()> {
    drain()?;
    test.wait_for_idle_sync()?;
    on_main(test, || {
        app.document.set(format!("stroke-pixels-{}", String::from(ids::Uuid::random())));
        app.host.set(crate::network::ServerAddr::Unset);
        app.proxy_url.set(crate::network::ProxyAddr::Unset);
        app.load(&crate::main_activity::DocumentType::from("fixed"))?;
        app.configure_proxy()?;
        app.set_drawing_tool(None)?;
        app.erase.set(false);
        app.pan.set(false);
        app.selecting.set(false);
        app.scribble_to_erase.set(false);
        app.brush.set(brush.to_string());
        app.width.set(12.0);
        app.color.set(0xff202020u32 as i32);
        app.material.set(50);
        let canvas = app.canvas();
        canvas.zoom.set(if transformed { 1.17 } else { 1.0 });
        canvas.offset_x.set(if transformed { 13.25 } else { 0.0 });
        canvas.offset_y.set(if transformed { -7.5 } else { 0.0 });
        // Existing content exercises compositing and stable stacking, not just blank paper.
        let background = object! {"kind" => "stroke", "stroke" => "background", "layer" => "base", "page" => 0, "width" => 25, "color" => 0x80770000u32 as i32, "brush" => "pen", "points" => vec![Value::from(vec![20, 20, 1]), Value::from(vec![900, 1100, 1])]};
        app.enqueue(&json_object(background))?;
        let bx = notemaker_core::actions::drawing(&notemaker_core::wire::ActionInput(object! {"object" => "box", "shape" => "rect", "x" => 170, "y" => 20, "w" => 35, "h" => 1100, "fill" => 0x800000ffu32 as i32}), &notemaker_core::ids::LayerId::from("base"))?;
        app.enqueue(&json_object(bx.into()))?;
        app.update()
    })?;
    drain()?;
    test.wait_for_idle_sync()?;
    let blocked = Arc::new(CountDownLatch::new(1));
    let release = Arc::new(CountDownLatch::new(1));
    {
        let (blocked, release) = (blocked.clone(), release.clone());
        replica_service::writes().execute(move || {
            blocked.count_down();
            release.await_();
        });
    }
    error::require(blocked.await_timeout(Duration::from_secs(10)), "Could not pause persistence")?;
    let mut before: Option<Obj> = None;
    let mut id: Option<String> = None;
    let body = on_main(test, || {
        let down = jx::uptime_millis().0;
        input(app, down, MOTIONEVENT_ACTION_DOWN, 80.0, 80.0, 0.7)?;
        let points: [[f32; 2]; 4] = [[280.0, 230.0], [80.0, 230.0], [280.0, 80.0], [300.0, 100.0]];
        for p in points {
            input(app, down, MOTIONEVENT_ACTION_MOVE, p[0], p[1], 0.7)?;
            let prefix = snapshot(app)?;
            app.canvas().emit()?;
            let emitted = snapshot(app)?;
            equal(app, &prefix, &emitted, &format!("{brush} live emission"))?;
            it::recycle_bitmap(&prefix)?;
            it::recycle_bitmap(&emitted)?;
        }
        id = app.canvas().current.get();
        let image = snapshot(app)?;
        before = Some(image.clone());
        input(app, down, MOTIONEVENT_ACTION_UP, 300.0, 100.0, 0.7)?;
        let up = snapshot(app)?;
        equal(app, &image, &up, &format!("{brush} pen-up transformed={transformed}"))?;
        it::recycle_bitmap(&up)
    });
    release.count_down();
    body?;
    drain()?;
    test.wait_for_idle_sync()?;
    on_main(test, || {
        let save_error = app.save_error.get();
        error::require(save_error.is_none(), &format!("Stroke save failed: {}", save_error.as_deref().unwrap_or("null")))?;
        let before = before.as_ref().ok_or_else(|| AppError::Core(notemaker_core::Error::null_pointer()))?;
        let saved = snapshot(app)?;
        equal(app, before, &saved, &format!("{brush} persisted"))?;
        it::recycle_bitmap(&saved)?;
        // Rebuilding the raster cache must preserve the complete canvas too.
        app.canvas().ink_document.set(String::new());
        let rebuilt = snapshot(app)?;
        equal(app, before, &rebuilt, &format!("{brush} cache rebuild"))?;
        it::recycle_bitmap(&rebuilt)?;
        it::recycle_bitmap(before)
    })?;
    let model = store::model(&app.replica().read()?)?;
    error::require(id.as_ref().is_some_and(|id| model.strokes.contains_key(id)), "Finished stroke missing from local replica")
}

/// `ReplicaService.writes.submit(() -> {}).get(30, SECONDS)`.
fn drain() -> AppResult<()> {
    replica_service::writes().submit(|| ()).get_timeout(Duration::from_secs(30))?;
    Ok(())
}
/// `new org.json.JSONObject(map)`.
fn json_object(map: notemaker_core::json::Map) -> JSONObject {
    JSONObject::from_value(&Value::Object(map)).unwrap_or_default()
}
