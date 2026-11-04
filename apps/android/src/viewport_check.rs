//! Port of `ViewportCheck.java` (instrumentation check): viewport culling and
//! tile-cache checks against the editor's uncached draw path. Tiled ink must
//! match cold renders across zoom, pan and tile seams.
//!
//! Runs on the instrumentation thread; every canvas access goes through
//! `test.run_on_main_sync`.

#![allow(clippy::too_many_arguments)]

use crate::android_canvas;
use crate::android_consts::{COLOR_BLACK, COLOR_WHITE};
use crate::error::{AppError, AppResult};
use crate::ink_cache::BitmapSize;
use crate::integration::{bitmap_size, create_bitmap, pixel, BitmapPoint, Integration};
use crate::jx::{self, Obj};
use crate::main_activity::MainActivity;
use crate::performance::{drain_writes, open_file_output, write_file_output};
use crate::stroke_crossings::{bitmap_canvas, recycle};
use crate::tile_ink_cache::{self, TileInkCache};
use crate::ui::round_f32;
use notemaker_cli::canvas::RectF;
use notemaker_core::json::{self, Map, Value};
use notemaker_core::style::ArgbColor;
use notemaker_core::{ids, jfmt, object};
use std::sync::Arc;

/// `private static final class Frame`.
pub struct Frame {
    /// `final Bitmap image`.
    pub image: Obj,
    pub metrics: Map,
}
impl Frame {
    fn new(image: Obj, metrics: Map) -> Frame {
        Frame { image, metrics }
    }
}

/// Java `String.valueOf(float)`.
fn f(v: f32) -> String {
    jfmt::float(v)
}
/// `Arrays.toString(int[])`.
fn int_array_string(values: &[i32]) -> String {
    format!("[{}]", values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", "))
}
/// `Arrays.deepToString(float[][])`.
fn float_matrix_string(values: &[[f32; 2]]) -> String {
    format!("[{}]", values.iter().map(|p| format!("[{}, {}]", f(p[0]), f(p[1]))).collect::<Vec<_>>().join(", "))
}
/// `AbstractCollection.toString()` of the JSON lists built here (numbers
/// print as their boxed Java values).
fn list_string(values: &[Value]) -> String {
    let item = |v: &Value| match v {
        Value::Array(a) => list_string(a),
        Value::Number(n) if n.is_f64() => jfmt::double(n.as_f64().unwrap_or(f64::NAN)),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => json::write(other),
    };
    format!("[{}]", values.iter().map(item).collect::<Vec<_>>().join(", "))
}
/// `RectF.toString()`.
fn rect_string(r: &RectF) -> String {
    format!("RectF({}, {}, {}, {})", f(r.left), f(r.top), f(r.right), f(r.bottom))
}
/// `test.runOnMainSync(()->{try{app.refreshShared(state);}catch(Exception e){throw new RuntimeException(e);}})`.
fn refresh<S: crate::main_activity::SharedState + Sync + ?Sized>(test: &Arc<Integration>, app: &Arc<MainActivity>, state: &S) -> AppResult<()> {
    test.run_on_main_sync(|| app.refresh_shared(state).map_err(|e| AppError::runtime_cause(&e)))?
}
fn put_object(app: &MainActivity, input: Map) -> AppResult<()> {
    app.replica().command(crate::app_replica::CliArgs(&["object", "put", "-"]), Some(&Value::Object(input)))?;
    Ok(())
}

/// `private static void phase(String name)`.
fn phase(name: &str) {
    jx::log::i(crate::jx::log::VIEWPORT_CHECK, name);
}
/// `private static void require(boolean condition, String message)`.
fn require(condition: bool, message: &str) -> AppResult<()> {
    if !condition {
        return Err(AppError::illegal_state(message));
    }
    Ok(())
}
/// `private static Bitmap snapshot(Integration test, MainActivity app)`.
fn snapshot(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<Obj> {
    test.run_on_main_sync(|| -> AppResult<Obj> {
        let canvas = app.canvas();
        let image = create_bitmap(BitmapSize { width: canvas.width().0, height: canvas.height().0 })?;
        jcall!(canvas.view(), "android/view/View", "draw", "(Landroid/graphics/Canvas;)V", &bitmap_canvas(&image)?)?;
        Ok(image)
    })?
}
/// `private static Frame captureCold(Integration test, MainActivity app)`.
fn capture_cold(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<Frame> {
    test.run_on_main_sync(|| -> AppResult<Frame> {
        let canvas = app.canvas();
        canvas.ink_document.set(String::new());
        let image = create_bitmap(BitmapSize { width: canvas.width().0, height: canvas.height().0 })?;
        jcall!(canvas.view(), "android/view/View", "draw", "(Landroid/graphics/Canvas;)V", &bitmap_canvas(&image)?)?;
        Ok(Frame::new(image, canvas.tile_cache_metrics()))
    })?
}
/// `private static int[] differences(Bitmap a, Bitmap b)`.
fn differences(a: &Obj, b: &Obj) -> AppResult<Vec<i32>> {
    require(bitmap_size(a)? == bitmap_size(b)?, "Reference dimensions changed")?;
    let x = android_canvas::bitmap_pixels(a)?;
    let y = android_canvas::bitmap_pixels(b)?;
    Ok(pixel_differences(&x, &y))
}
/// The per-pixel loop of `differences`.
fn pixel_differences(x: &[i32], y: &[i32]) -> Vec<i32> {
    let mut out = vec![0, 0, 0, 0];
    for i in 0..x.len() {
        if x[i] != y[i] {
            out[0] += 1;
            let (a, b) = (ArgbColor(x[i]).channels(), ArgbColor(y[i]).channels());
            out[1] = out[1].max((a.alpha - b.alpha).abs());
            out[2] = out[2].max((a.red - b.red).abs());
            out[3] = out[3].max((a.green - b.green).abs());
        }
    }
    out
}
/// `private static Map<String,Object> metrics(Integration test, MainActivity app)`.
fn metrics(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<Map> {
    Ok(test.run_on_main_sync(|| app.canvas().tile_cache_metrics())?)
}
/// `private static boolean hasItem(Map<String,Object> metrics, String id)`.
fn has_item(metrics: &Map, id: &str) -> bool {
    let ids = json::list(metrics.get("lastRenderedItemIds").unwrap_or(&Value::Null)).unwrap_or_else(|e| panic!("{}", AppError::from(e)));
    ids.iter().any(|value| value.as_str() == Some(id))
}
/// `private static long n(Map<String,Object> m, String key)`.
fn n(m: &Map, key: &str) -> i64 {
    crate::performance::long_metric(m, key)
}
/// `private static void saveDebug(MainActivity app, String name, Bitmap bitmap)`:
/// failures are wrapped in a `RuntimeException`.
fn save_debug(app: &Arc<MainActivity>, name: &str, bitmap: &Obj) -> AppResult<()> {
    (|| -> AppResult<()> {
        let out = open_file_output(app, name)?;
        let format = jsget!("android/graphics/Bitmap$CompressFormat", "PNG", "Landroid/graphics/Bitmap$CompressFormat;")?.some();
        let compressed = jcall!(bitmap, "android/graphics/Bitmap", "compress", "(Landroid/graphics/Bitmap$CompressFormat;ILjava/io/OutputStream;)Z", &format, 100, &out);
        let closed = jcall!(out, "java/io/FileOutputStream", "close", "()V");
        compressed?;
        closed?;
        Ok(())
    })()
    .map_err(|e| AppError::runtime_cause(&e))
}
/// `try(FileOutputStream out=app.openFileOutput(name,0)){out.write(text.getBytes("UTF-8"));}catch(Exception e){throw new RuntimeException(e);}`.
fn save_text(app: &Arc<MainActivity>, name: &str, text: &str) -> AppResult<()> {
    write_file_output(app, name, text.as_bytes()).map_err(|e| AppError::runtime_cause(&e))
}
/// `private static void resetDocument(Integration test, MainActivity app, String type, float zoom, float ox, float oy) throws Exception`.
fn reset_document(test: &Arc<Integration>, app: &Arc<MainActivity>, type_: &str, zoom: f32, ox: f32, oy: f32) -> AppResult<()> {
    test.run_on_main_sync(|| -> AppResult<()> {
        app.document.set(format!("viewport-{}", String::from(ids::Uuid::random())));
        app.host.set(crate::network::ServerAddr::Unset);
        app.proxy_url.set(crate::network::ProxyAddr::Unset);
        app.load(&crate::main_activity::DocumentType::from(type_))?;
        app.configure_proxy()?;
        let canvas = app.canvas();
        canvas.zoom.set(zoom);
        canvas.offset_x.set(ox);
        canvas.offset_y.set(oy);
        canvas.use_ink_cache.set(true);
        canvas.cull_tile_items.set(true);
        canvas.monolithic_tile_probe.set(None);
        app.set_drawing_tool(None)?;
        app.erase.set(false);
        app.pan.set(false);
        app.selecting.set(false);
        app.scribble_to_erase.set(false);
        app.page.set(0);
        app.selected.set("base".into());
        app.update()
    })??;
    drain_writes()?;
    test.wait_for_idle_sync()?;
    Ok(())
}
/// `private static List<Object> points(float scale, float zoom, float ox, float oy, float[][] screen)`.
fn points(scale: f32, zoom: f32, ox: f32, oy: f32, screen: &[[f32; 2]]) -> Vec<Value> {
    screen.iter().map(|p| Value::from(vec![json::from_float((p[0] - ox) / (scale * zoom)), json::from_float((p[1] - oy) / (scale * zoom)), json::double(0.9)])).collect()
}
/// `private static void addStroke(MainActivity app, String id, String brush, int color, double width, List<Object> pts) throws Exception`.
fn add_stroke(app: &Arc<MainActivity>, id: &str, brush: &str, color: i32, width: f64, pts: Vec<Value>) -> AppResult<()> {
    let input = object! {"stroke" => id, "layer" => "base", "page" => 0, "color" => color, "width" => json::double(width), "brush" => brush, "material" => 65, "points" => pts};
    app.replica().command(crate::app_replica::CliArgs(&["stroke", "put", "-"]), Some(&Value::Object(input)))?;
    Ok(())
}
/// `private static void compare(Integration test, MainActivity app, String where)`.
fn compare(test: &Arc<Integration>, app: &Arc<MainActivity>, where_: &str) -> AppResult<()> {
    let cached = snapshot(test, app)?;
    test.run_on_main_sync(|| app.canvas().cull_tile_items.set(false))?;
    let direct = snapshot(test, app)?;
    test.run_on_main_sync(|| app.canvas().cull_tile_items.set(true))?;
    let diff = differences(&cached, &direct)?;
    let size = bitmap_size(&cached)?;
    let pixels = size.width * size.height;
    if diff[0] != 0 {
        save_debug(app, "viewport-compare-cached.png", &cached)?;
        save_debug(app, "viewport-compare-reference.png", &direct)?;
        let cold = capture_cold(test, app)?;
        let cold_diff = differences(&cold.image, &direct)?;
        save_debug(app, "viewport-compare-cold.png", &cold.image)?;
        let warm = metrics(test, app)?;
        save_text(
            app,
            "viewport-compare-debug.txt",
            &format!("where={} warmReferenceDiff={} coldReferenceDiff={} coldMetrics={} warmMetrics={}", where_, int_array_string(&diff), int_array_string(&cold_diff), json::write_map(&cold.metrics), json::write_map(&warm)),
        )?;
        recycle(&cold.image)?;
    }
    recycle(&cached)?;
    recycle(&direct)?;
    require(diff[0] == 0, &format!("{} culled/unculled same-tier raster differs at {} pixels (viewport {}); per-channel maxima alpha/red/green={}/{}/{}", where_, diff[0], pixels, diff[1], diff[2], diff[3]))
}
/// `private static float nearbyTileBoundary(float center, float offset, float unit, float limit)`.
fn nearby_tile_boundary(center: f32, offset: f32, unit: f32, limit: f32) -> f32 {
    let doc = (center - offset) / unit;
    let mut boundary = ((doc / 256f32) as f64).floor() as f32 * 256f32;
    let screen = offset + boundary * unit;
    if screen < 24.0 || screen > limit - 24.0 {
        let next = boundary + 256.0;
        let projected = offset + next * unit;
        if projected >= 24.0 && projected <= limit - 24.0 {
            boundary = next;
        }
    }
    boundary
}
/// `private static void seamCompare(Integration test, MainActivity app, float zoom, float ox, float oy, float scale, float seamX, float seamY)`.
fn seam_compare(test: &Arc<Integration>, app: &Arc<MainActivity>, zoom: f32, ox: f32, oy: f32, scale: f32, seam_x: f32, seam_y: f32) -> AppResult<()> {
    let cached = snapshot(test, app)?;
    let unit = scale * zoom;
    let probe_x = 48.0 / unit;
    let probe_y = 24.0 / unit;
    test.run_on_main_sync(|| app.canvas().monolithic_tile_probe.set(Some(RectF { left: seam_x - probe_x, top: seam_y - probe_y, right: seam_x + probe_x, bottom: seam_y + probe_y })))?;
    let monolithic = snapshot(test, app)?;
    test.run_on_main_sync(|| app.canvas().monolithic_tile_probe.set(None))?;
    test.run_on_main_sync(|| app.canvas().use_ink_cache.set(false))?;
    let direct = snapshot(test, app)?;
    test.run_on_main_sync(|| app.canvas().use_ink_cache.set(true))?;
    let BitmapSize { width: w, height: h } = bitmap_size(&cached)?;
    let sx = round_f32(ox + seam_x * scale * zoom);
    let sy = round_f32(oy + seam_y * scale * zoom);
    let left = 0.max(sx - 30);
    let right = w.min(sx + 30);
    let top = 0.max(sy - 8);
    let bottom = h.min(sy + 9);
    let (mut visible, mut worst, mut mono_diff) = (0, 0, 0);
    let (mut diff_left, mut diff_top, mut diff_right, mut diff_bottom) = (w, h, 0, 0);
    for y in top..bottom {
        for x in left..right {
            let a = pixel(&cached, BitmapPoint { x, y })?.0;
            let b = pixel(&direct, BitmapPoint { x, y })?.0;
            let m = pixel(&monolithic, BitmapPoint { x, y })?.0;
            if m != COLOR_WHITE {
                visible += 1;
            }
            if a != m {
                mono_diff += 1;
                diff_left = diff_left.min(x);
                diff_top = diff_top.min(y);
                diff_right = diff_right.max(x + 1);
                diff_bottom = diff_bottom.max(y + 1);
            }
            let (a, b) = (ArgbColor(a).channels(), ArgbColor(b).channels());
            worst = worst.max((a.red - b.red).abs().max((a.green - b.green).abs().max((a.blue - b.blue).abs())));
        }
    }
    if mono_diff > 0 {
        save_debug(app, "viewport-seam-tiled.png", &cached)?;
        save_debug(app, "viewport-seam-monolithic.png", &monolithic)?;
        save_debug(app, "viewport-seam-direct.png", &direct)?;
        let tier = app.canvas().raster_tier.get();
        let warm = metrics(test, app)?;
        save_text(
            app,
            "viewport-seam-debug.txt",
            &format!(
                "zoom={} seam={},{} screen={},{} roi={},{}..{},{} diffBounds={},{}..{},{} monoDiff={} worst={} unit={} tier={} metrics={}",
                f(zoom),
                f(seam_x),
                f(seam_y),
                sx,
                sy,
                left,
                top,
                right,
                bottom,
                diff_left,
                diff_top,
                diff_right,
                diff_bottom,
                mono_diff,
                worst,
                f(scale * zoom),
                tier.0,
                json::write_map(&warm)
            ),
        )?;
    }
    recycle(&cached)?;
    recycle(&monolithic)?;
    recycle(&direct)?;
    require(visible > 4, "Translucent seam fixture is not visible at the selected zoom")?;
    require(mono_diff == 0, &format!("Tile seam differs from the monolithic same-tier reference at {mono_diff} ROI pixels"))?;
    require(worst <= 24, &format!("Tile seam changed translucent pigment by {worst} channel levels"))
}
/// `private static int edgeHits(Bitmap image, String edge)`.
fn edge_hits(image: &Obj, edge: &str) -> AppResult<i32> {
    let BitmapSize { width: w, height: h } = bitmap_size(image)?;
    let mut hits = 0;
    if edge == "top" {
        for y in 0..12 {
            for x in 12..w - 12 {
                if pixel(image, BitmapPoint { x, y })?.0 != COLOR_WHITE {
                    hits += 1;
                }
            }
        }
    }
    if edge == "bottom" {
        for y in h - 12..h {
            for x in 12..w - 12 {
                if pixel(image, BitmapPoint { x, y })?.0 != COLOR_WHITE {
                    hits += 1;
                }
            }
        }
    }
    if edge == "left" {
        for x in 0..12 {
            for y in 12..h - 12 {
                if pixel(image, BitmapPoint { x, y })?.0 != COLOR_WHITE {
                    hits += 1;
                }
            }
        }
    }
    if edge == "right" {
        for x in w - 12..w {
            for y in 12..h - 12 {
                if pixel(image, BitmapPoint { x, y })?.0 != COLOR_WHITE {
                    hits += 1;
                }
            }
        }
    }
    Ok(hits)
}
/// `static void run(Integration test, MainActivity app) throws Exception`.
pub fn run(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<()> {
    drain_writes()?;
    let canvas = app.canvas();
    let width = canvas.width().0 as f32;
    let height = canvas.height().0 as f32;
    let scale = canvas.scale().0;
    require(width > 200.0 && height > 200.0, "Canvas has no usable viewport")?;
    let brushes = ["pen", "marker", "charcoal", "oily", "square", "pencil"];
    for type_ in ["fixed", "infinite"] {
        for zoom in [0.2f32, 1.0, 5.0] {
            phase(&format!("begin {} zoom={}", type_, f(zoom)));
            let ox = 13.25f32;
            let oy = -7.5f32;
            reset_document(test, app, type_, zoom, ox, oy)?;
            // Each centerline is parallel to an edge and remains just beyond the
            // viewport. Its wide footprint, cap or textured brush must still show.
            let pad = 0.5f32.max(70.0 * scale * zoom * 0.12f32);
            let lines: [[[f32; 2]; 2]; 4] = [
                [[-300.0, -pad], [width + 300.0, -pad]],
                [[-300.0, height + pad], [width + 300.0, height + pad]],
                [[-pad, -300.0], [-pad, height + 300.0]],
                [[width + pad, -300.0], [width + pad, height + 300.0]],
            ];
            let sides = ["top", "bottom", "left", "right"];
            for i in 0..4 {
                add_stroke(app, &format!("edge-{i}"), brushes[(i + (zoom * 10.0) as i32 as usize) % brushes.len()], 0xff202020u32 as i32, 70.0, points(scale, zoom, ox, oy, &lines[i]))?;
            }
            // Tile boundaries are at 256 document units. A translucent long
            // stroke crosses a visible tile corner, making a duplicated gutter visible.
            let unit = scale * zoom;
            let seam_x = if type_ == "fixed" { 0.0 } else { nearby_tile_boundary(width / 2.0, ox, unit, width) };
            let seam_y = if type_ == "fixed" { 256.0 } else { nearby_tile_boundary(height / 2.0, oy, unit, height) };
            add_stroke(
                app,
                "tile-seam",
                "charcoal",
                0x80660000u32 as i32,
                60.0,
                vec![
                    Value::from(vec![json::double(seam_x as f64 - 180.0), json::double(seam_y as f64), json::double(0.8)]),
                    Value::from(vec![json::double(seam_x as f64 + 180.0), json::double(seam_y as f64), json::double(0.8)]),
                ],
            )?;
            // Include a large fully offscreen textured stroke to prove culling
            // happens before the expensive geometry renderer is entered.
            let far = [[width + 2000.0, height + 1700.0], [width + 2600.0, height + 1800.0]];
            add_stroke(app, "offscreen-heavy", "charcoal", COLOR_BLACK, 100.0, points(scale, zoom, ox, oy, &far))?;
            if type_ == "fixed" {
                put_object(app, object! {"object" => "outside-page", "layer" => "base", "page" => 0, "shape" => "rect", "x" => 1010, "y" => 300, "w" => 100, "h" => 100, "fill" => 0x800000ffu32 as i32, "color" => COLOR_BLACK, "width" => 3})?;
            }
            let initial = app.replica().read()?;
            refresh(test, app, &initial)?;
            test.wait_for_idle_sync()?;
            let first_frame = capture_cold(test, app)?;
            let rendered = first_frame.image.clone();
            let first_draw = first_frame.metrics;
            phase(&format!("initial draw {} zoom={}", type_, f(zoom)));
            require(n(&first_draw, "itemRenders") > 0, "Viewport draw did not execute any geometry rendering; the cull check would be vacuous")?;
            if type_ == "infinite" {
                test.run_on_main_sync(|| app.canvas().use_ink_cache.set(false))?;
                let uncullled = snapshot(test, app)?;
                test.run_on_main_sync(|| app.canvas().use_ink_cache.set(true))?;
                for side in sides {
                    let reference_hits = edge_hits(&uncullled, side)?;
                    let cached_hits = edge_hits(&rendered, side)?;
                    if reference_hits <= 3 || cached_hits <= 3 {
                        let si = sides.iter().position(|s| *s == side).unwrap();
                        let edge = app.model().strokes.get(&format!("edge-{si}")).cloned();
                        let bounds = match edge {
                            None => RectF::default(),
                            Some(edge) => tile_ink_cache::Item::new(tile_ink_cache::TileItemId::from(format!("edge-{si}")), edge)?.bounds,
                        };
                        let viewport = app.canvas().document_viewport();
                        save_debug(app, "viewport-edge-cached.png", &rendered)?;
                        save_debug(app, "viewport-edge-reference.png", &uncullled)?;
                        let tier = app.canvas().raster_tier.get();
                        let warm = metrics(test, app)?;
                        save_text(
                            app,
                            "viewport-edge-debug.txt",
                            &format!(
                                "side={} zoom={} width={} height={} scale={} unit={} pad={} screenLine={} documentPoints={} itemBounds={} documentViewport={} tier={} tileSize={} referenceHits={} cachedHits={} metrics={}",
                                side,
                                f(zoom),
                                f(width),
                                f(height),
                                f(scale),
                                f(unit),
                                f(pad),
                                float_matrix_string(&lines[si]),
                                list_string(&points(scale, zoom, ox, oy, &lines[si])),
                                rect_string(&bounds),
                                rect_string(&viewport),
                                tier.0,
                                TileInkCache::tile_size(tier).0,
                                reference_hits,
                                cached_hits,
                                json::write_map(&warm)
                            ),
                        )?;
                    }
                    require(
                        reference_hits > 3,
                        &format!(
                            "Direct uncached reference fixture did not paint across {} edge at zoom={} hits={} viewport={}x{} scale={} unit={} pad={}",
                            side,
                            f(zoom),
                            reference_hits,
                            bitmap_size(&uncullled)?.width,
                            bitmap_size(&uncullled)?.height,
                            f(scale),
                            f(unit),
                            f(pad)
                        ),
                    )?;
                    require(cached_hits > 3, &format!("Cached centerline outside {side} edge was culled despite reference paint"))?;
                }
                recycle(&uncullled)?;
                require(has_item(&first_draw, "edge-0") || has_item(&first_draw, "edge-1") || has_item(&first_draw, "edge-2") || has_item(&first_draw, "edge-3"), "No visible edge stroke reached the geometry renderer")?;
                require(!has_item(&first_draw, "offscreen-heavy"), "Fully offscreen item reached the geometry renderer")?;
                seam_compare(test, app, zoom, ox, oy, scale, seam_x, seam_y)?;
                phase(&format!("seam passed {} zoom={}", type_, f(zoom)));
            } else {
                let x0 = 0.max(round_f32(ox + scale * zoom * 1008.0));
                let x1 = bitmap_size(&rendered)?.width.min(round_f32(ox + scale * zoom * 1120.0));
                let y0 = 0.max(round_f32(oy + scale * zoom * 290.0));
                let y1 = bitmap_size(&rendered)?.height.min(round_f32(oy + scale * zoom * 410.0));
                let mut blue_pixels = 0;
                if zoom == 0.2f32 {
                    require(x1 > x0 && y1 > y0, "Fixed-page clipping probe did not intersect the viewport")?;
                }
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = pixel(&rendered, BitmapPoint { x, y })?.channels();
                        if p.blue > p.red + 25 && p.blue > p.green + 25 {
                            blue_pixels += 1;
                        }
                    }
                }
                require(blue_pixels == 0, "Fixed-page rendering showed an object outside the paper clip")?;
            }
            recycle(&rendered)?;
            compare(test, app, &format!("{} zoom={}", type_, f(zoom)))?;
            phase(&format!("uncull reference passed {} zoom={}", type_, f(zoom)));
            let first = metrics(test, app)?;
            require(n(&first, "culledItems") > 0, "No fully offscreen items were culled")?;
            let items_before = n(&first, "itemRenders");
            compare(test, app, &format!("repeat {} zoom={}", type_, f(zoom)))?;
            let second = metrics(test, app)?;
            require(n(&second, "itemRenders") == items_before, "Repeated identical draw rerendered tile geometry")?;
            require(n(&second, "currentBytes") <= n(&second, "maxBytes"), "Tile cache exceeded its configured memory bound")?;
            require(n(&second, "peakBytes") <= n(&second, "maxBytes"), "Tile cache peak exceeded its configured memory bound")?;
            phase(&format!("reuse passed {} zoom={}", type_, f(zoom)));
            // A document-space object at the top edge exercises geometry clipping
            // through the same viewport cache; then edits must invalidate its tile.
            let before_edit = n(&metrics(test, app)?, "tileRenders");
            put_object(app, object! {"object" => "edge-object", "layer" => "base", "page" => 0, "shape" => "rect", "x" => 100, "y" => 100, "w" => 160, "h" => 90, "fill" => 0x804000ffu32 as i32, "color" => COLOR_BLACK, "width" => 3})?;
            let with_object = app.replica().read()?;
            refresh(test, app, &with_object)?;
            test.wait_for_idle_sync()?;
            compare(test, app, &format!("object insertion {} zoom={}", type_, f(zoom)))?;
            phase(&format!("object insertion passed {} zoom={}", type_, f(zoom)));
            require(n(&metrics(test, app)?, "tileRenders") > before_edit, "Object edit did not invalidate and rerender affected viewport tiles")?;
            let stable_items = n(&metrics(test, app)?, "itemRenders");
            compare(test, app, &format!("stable object cache {} zoom={}", type_, f(zoom)))?;
            require(n(&metrics(test, app)?, "itemRenders") == stable_items, "Stable object was rendered repeatedly")?;
            phase(&format!("stable object passed {} zoom={}", type_, f(zoom)));
            let before_erase = snapshot(test, app)?;
            app.replica().command(
                crate::app_replica::CliArgs(&["erase", "-"]),
                Some(&Value::Object(object! {"layer" => "base", "page" => 0, "mode" => "partial", "radius" => 28, "points" => vec![Value::from(vec![json::double(seam_x as f64), json::double(seam_y as f64)])]})),
            )?;
            let masked = app.replica().read()?;
            refresh(test, app, &masked)?;
            test.wait_for_idle_sync()?;
            let after_erase = snapshot(test, app)?;
            let erased_pixels = differences(&before_erase, &after_erase)?[0];
            recycle(&before_erase)?;
            recycle(&after_erase)?;
            require(erased_pixels > 0, "Partial erase fixture did not change visible seam pixels")?;
            compare(test, app, &format!("masked stroke at tile seam {} zoom={}", type_, f(zoom)))?;
            phase(&format!("partial erase passed {} zoom={}", type_, f(zoom)));
            // Pan changes which document tiles are visible. Returning to the
            // original viewport should reuse resident tiles without a rebuild.
            if type_ == "infinite" && zoom == 1f32 {
                test.run_on_main_sync(|| {
                    let canvas = app.canvas();
                    canvas.offset_x.set(canvas.offset_x.get() + width * 0.18f32);
                    canvas.offset_y.set(canvas.offset_y.get() + height * 0.16f32);
                })?;
                recycle(&snapshot(test, app)?)?;
                let away = metrics(test, app)?;
                test.run_on_main_sync(|| {
                    let canvas = app.canvas();
                    canvas.offset_x.set(ox);
                    canvas.offset_y.set(oy);
                })?;
                recycle(&snapshot(test, app)?)?;
                let returned = metrics(test, app)?;
                require(n(&returned, "reusedTiles") > n(&away, "reusedTiles"), "Returning to the original pan did not reuse resident tiles")?;
                require(n(&returned, "tileRenders") == n(&away, "tileRenders"), "Returning to the original pan rerendered resident tiles")?;
            }
        }
    }
    // A connector's cached geometry depends on its attached nodes. Moving a
    // node changes the same connector map, so verify the editor re-resolves it.
    reset_document(test, app, "infinite", 1.0, 0.0, 0.0)?;
    phase("begin attached arrow");
    put_object(app, object! {"object" => "node-a", "layer" => "base", "page" => 0, "shape" => "rect", "x" => 300, "y" => 300, "w" => 100, "h" => 100, "fill" => 0x804000ffu32 as i32, "color" => COLOR_BLACK, "width" => 3})?;
    put_object(app, object! {"object" => "node-b", "layer" => "base", "page" => 0, "shape" => "rect", "x" => 700, "y" => 300, "w" => 100, "h" => 100, "fill" => 0x804000ffu32 as i32, "color" => COLOR_BLACK, "width" => 3})?;
    put_object(app, object! {"object" => "connector", "layer" => "base", "page" => 0, "shape" => "arrowLine", "x" => 350, "y" => 350, "w" => 400, "h" => 0, "from" => "node-a", "to" => "node-b", "color" => COLOR_BLACK, "width" => 8})?;
    let connected = app.replica().read()?;
    refresh(test, app, &connected)?;
    test.wait_for_idle_sync()?;
    compare(test, app, "attached arrow initial")?;
    let before_move = n(&metrics(test, app)?, "tileRenders");
    put_object(app, object! {"object" => "node-b", "layer" => "base", "page" => 0, "shape" => "rect", "x" => 900, "y" => 450, "w" => 100, "h" => 100, "fill" => 0x804000ffu32 as i32, "color" => COLOR_BLACK, "width" => 3})?;
    let moved = app.replica().read()?;
    refresh(test, app, &moved)?;
    test.wait_for_idle_sync()?;
    compare(test, app, "attached arrow endpoint moved")?;
    require(n(&metrics(test, app)?, "tileRenders") > before_move, "Moving an attached node did not rerender tiles containing its arrow")?;
    phase("attached arrow passed");

    reset_document(test, app, "infinite", 1.0, 0.0, 0.0)?;
    phase("begin zoom tier");
    add_stroke(
        app,
        "zoom-stroke",
        "pen",
        0xff202020u32 as i32,
        20.0,
        vec![Value::from(vec![json::double(20.0), json::double(120.0), json::double(0.8)]), Value::from(vec![json::double(900.0), json::double(120.0), json::double(0.8)])],
    )?;
    let zoom_content = app.replica().read()?;
    refresh(test, app, &zoom_content)?;
    test.wait_for_idle_sync()?;
    // Capture pixels and render counters in one UI turn. A scheduled window
    // draw between separate calls can clear beginFrame's item IDs.
    let populated = capture_cold(test, app)?;
    recycle(&populated.image)?;
    let tier_one = populated.metrics;
    let original_scale_renders = n(&tier_one, "tileRenders");
    require(has_item(&tier_one, "zoom-stroke"), "Zoom fixture did not populate a tile with visible geometry")?;
    let next_zoom = 5f32;
    test.run_on_main_sync(|| app.canvas().zoom.set(next_zoom))?;
    recycle(&snapshot(test, app)?)?;
    let tier_two = metrics(test, app)?;
    require(n(&tier_two, "densityTier") > n(&tier_one, "densityTier"), "Zoom fixture did not cross a populated raster-density tier")?;
    require(n(&tier_two, "tileRenders") > original_scale_renders, "Zoom density change did not rerasterize populated tiles")?;
    compare(test, app, "intermediate zoom tier")?;
    phase("zoom tier passed");
    // Exercise many viewport shifts to force LRU eviction while remaining
    // within the advertised bounded bitmap budget.
    reset_document(test, app, "infinite", 5.0, 0.0, 0.0)?;
    phase("begin LRU");
    put_object(app, object! {"object" => "wide-cache-fill", "layer" => "base", "page" => 0, "shape" => "rect", "x" => -50000, "y" => -50000, "w" => 100000, "h" => 100000, "fill" => 0x20660000, "color" => COLOR_BLACK, "width" => 2})?;
    let cache_fill = app.replica().read()?;
    refresh(test, app, &cache_fill)?;
    test.wait_for_idle_sync()?;
    recycle(&snapshot(test, app)?)?;
    require(n(&metrics(test, app)?, "densityTier") == 8, "High-zoom eviction fixture did not reach density tier 8")?;
    let evictions_before = n(&metrics(test, app)?, "evictedTiles");
    for i in 0..40 {
        let shift = i;
        test.run_on_main_sync(|| {
            let canvas = app.canvas();
            canvas.offset_x.set((-shift * canvas.width().0) as f32 * 0.8f32);
            canvas.offset_y.set((-shift * canvas.height().0) as f32 * 0.7f32);
        })?;
        recycle(&snapshot(test, app)?)?;
    }
    let bounded = metrics(test, app)?;
    require(n(&bounded, "currentBytes") <= n(&bounded, "maxBytes") && n(&bounded, "peakBytes") <= n(&bounded, "maxBytes"), "Panning grew tile cache beyond the memory budget")?;
    require(n(&bounded, "evictedTiles") > evictions_before, "Populated high-density panning did not exercise bounded-cache eviction")?;
    phase("LRU passed");

    reset_document(test, app, "pages", 1.0, 0.0, 0.0)?;
    phase("begin page cache");
    for page in 0..2 {
        put_object(
            app,
            object! {"object" => format!("page-box-{page}"), "layer" => "base", "page" => page, "shape" => "rect",
            "x" => 120 + page * 280, "y" => 180, "w" => 180, "h" => 120,
            "fill" => if page == 0 { 0xff000000u32 as i32 } else { 0xff0055aau32 as i32 }, "color" => COLOR_BLACK, "width" => 3},
        )?;
    }
    let pages = app.replica().read()?;
    refresh(test, app, &pages)?;
    let page_zero = snapshot(test, app)?;
    compare(test, app, "page 0 before flip")?;
    test.run_on_main_sync(|| app.page.set(1))?;
    let page_one = snapshot(test, app)?;
    compare(test, app, "page 1 first draw")?;
    require(differences(&page_zero, &page_one)?[0] > 0, "Page fixture rendered identical pixels on both pages")?;
    recycle(&page_zero)?;
    recycle(&page_one)?;
    let page_one_renders = n(&metrics(test, app)?, "tileRenders");
    test.run_on_main_sync(|| app.page.set(0))?;
    recycle(&snapshot(test, app)?)?;
    require(n(&metrics(test, app)?, "tileRenders") == page_one_renders, "Returning to page 0 repainted resident tiles")?;
    compare(test, app, "page 0 retained tile")?;
    put_object(app, object! {"object" => "page-box-1", "layer" => "base", "page" => 1, "shape" => "rect", "x" => 500, "y" => 330, "w" => 180, "h" => 120, "fill" => 0xff0055aau32 as i32, "color" => COLOR_BLACK, "width" => 3})?;
    let changed_page = app.replica().read()?;
    test.run_on_main_sync(|| -> AppResult<()> {
        (|| -> AppResult<()> {
            app.refresh_shared(&changed_page)?;
            app.page.set(1);
            Ok(())
        })()
        .map_err(|e| AppError::runtime_cause(&e))
    })??;
    recycle(&snapshot(test, app)?)?;
    require(n(&metrics(test, app)?, "tileRenders") > page_one_renders, "Edited page reused stale tiles")?;
    compare(test, app, "page 1 after edit")?;
    phase("page cache passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_boundaries() {
        // Centered at 500 screen px, unit 1: floor(486.75/256)*256 = 256, on screen.
        assert_eq!(nearby_tile_boundary(500.0, 13.25, 1.0, 1000.0), 256.0);
        // A boundary too close to the left edge moves to the next tile.
        assert_eq!(nearby_tile_boundary(20.0, 0.0, 1.0, 1000.0), 256.0);
        // Negative document coordinates floor downwards; the next boundary
        // (0 at screen 0) is still too close to the edge, so -256 stays.
        assert_eq!(nearby_tile_boundary(-10.0, 0.0, 1.0, 1000.0), -256.0);
    }

    #[test]
    fn formatting() {
        assert_eq!(int_array_string(&[1, 0, -2, 3]), "[1, 0, -2, 3]");
        assert_eq!(float_matrix_string(&[[-300.0, -0.5], [1.5, 2.0]]), "[[-300.0, -0.5], [1.5, 2.0]]");
        let pts = points(1.0, 1.0, 0.0, 0.0, &[[1.5, -2.0]]);
        assert_eq!(list_string(&pts), "[[1.5, -2.0, 0.9]]");
        assert_eq!(rect_string(&RectF { left: 0.0, top: 1.5, right: 2.0, bottom: 3.0 }), "RectF(0.0, 1.5, 2.0, 3.0)");
    }

    #[test]
    fn pixel_diffs() {
        let a = [0xff000000u32 as i32, 0xffffffffu32 as i32, 0x80102030u32 as i32];
        let b = [0xff000000u32 as i32, 0xff00ff00u32 as i32, 0x70102030u32 as i32];
        assert_eq!(pixel_differences(&a, &b), vec![2, 0x10, 0xff, 0]);
    }
}
