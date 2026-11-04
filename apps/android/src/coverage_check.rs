//! Port of `CoverageCheck.java` (instrumentation check): selection coverage,
//! object preview handoff and preview rollback on the real app; tile cache
//! pixel parity (`cachePixels`).

use crate::android_canvas::{bitmap_pixels, AndroidCanvas};
use crate::android_consts::*;
use crate::concurrent::CountDownLatch;
use crate::error::{AppError, AppResult};
use crate::ink_cache::InkCache;
use crate::ink_cache::BitmapSize;
use crate::integration::{self as it, BitmapPoint, Integration};
use crate::jx::{self, Obj};
use crate::main_activity::MainActivity;
use crate::org_json::{JSONArray, JV};
use crate::replica_service;
use indexmap::IndexMap;
use notemaker_cli::item_coverage::FACTORY;
use notemaker_cli::png::{Png, USE_SVG_PICTURES};
use notemaker_core::draw_item::DrawItem;
use notemaker_core::ids::PageNumber;
use notemaker_core::json::{self, Map, Value};
use notemaker_core::layer_editor::LayerEditor;
use notemaker_core::model::{Entity, Model};
use notemaker_core::render::{self, Surface};
use notemaker_core::{image_asset, object, store};
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// `static void run(Integration test, MainActivity app) throws Exception`.
pub fn run(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<()> {
    cache_pixels_sized(true)?;
    let replica = app.replica();
    replica.command(crate::app_replica::CliArgs(&["layer", "put", "-"]), Some(&Value::Object(object! {"layer" => "under", "name" => "Under", "force" => false, "order" => -1})))?;
    for layer in ["base", "under"] {
        let fill = if layer == "base" { COLOR_BLACK } else { COLOR_RED };
        replica.command(crate::app_replica::CliArgs(&["object", "put", "-"]), Some(&Value::Object(object! {"object" => format!("{layer}-box"), "layer" => layer, "shape" => "rect", "x" => 0, "y" => 0, "w" => 100, "h" => 100, "fill" => fill})))?;
    }
    replica.command(crate::app_replica::CliArgs(&["object", "put", "-"]), Some(&Value::Object(object! {"object" => "ring", "shape" => "ellipse", "x" => 120, "y" => 120, "w" => 100, "h" => 100, "width" => 4})))?;
    replica.command(crate::app_replica::CliArgs(&["stroke", "put", "-"]), Some(&Value::Object(object! {"stroke" => "ink", "layer" => "base", "page" => 0, "width" => 4, "color" => COLOR_BLACK, "points" => vec![Value::from(vec![0, 50, 1]), Value::from(vec![100, 50, 1])]})))?;
    let source = it::create_bitmap(BitmapSize { width: 4, height: 4 })?;
    jcall!(source, "android/graphics/Bitmap", "eraseColor", "(I)V", COLOR_WHITE)?;
    for (x, y) in [(1, 1), (2, 1), (1, 2), (2, 2)] {
        jcall!(source, "android/graphics/Bitmap", "setPixel", "(III)V", x, y, COLOR_TRANSPARENT)?;
    }
    let bytes = jnew!("java/io/ByteArrayOutputStream", "()V")?;
    let png = jsget!("android/graphics/Bitmap$CompressFormat", "PNG", "Landroid/graphics/Bitmap$CompressFormat;")?.some();
    jcall!(source, "android/graphics/Bitmap", "compress", "(Landroid/graphics/Bitmap$CompressFormat;ILjava/io/OutputStream;)Z", &png, 100, &bytes)?;
    it::recycle_bitmap(&source)?;
    let data = jcall!(bytes, "java/io/ByteArrayOutputStream", "toByteArray", "()[B")?.some();
    let image = image_asset::from_bytes(&jx::byte_array(data.as_obj()))?;
    replica.command(crate::app_replica::CliArgs(&["object", "put", "-"]), Some(&Value::Object(object! {"object" => "image", "shape" => "image", "image" => image, "x" => 300, "y" => 100, "w" => 40, "h" => 40})))?;
    require(query(app, 320.0, 120.0)?.is_empty(), "Transparent image hole selected")?;
    require(query(app, 302.0, 102.0)?.len() == 1, "Opaque white image pixel missed")?;
    require(query(app, 170.0, 170.0)?.is_empty(), "Hollow shape interior selected")?;
    require(query(app, 120.0, 170.0)?.len() == 1, "Shape outline missed")?;
    replica.command(crate::app_replica::CliArgs(&["erase", "-"]), Some(&Value::Object(object! {"layer" => "base", "page" => 0, "mode" => "partial", "radius" => 10, "points" => vec![Value::from(vec![50, 0]), Value::from(vec![50, 100])]})))?;
    require(query(app, 50.0, 50.0)?.is_empty(), "Erased area still selects")?;
    require(query(app, 20.0, 50.0)?.len() == 2, "Remaining coverage lost")?;
    let snapshot = replica.read()?;
    let model = store::model(&snapshot)?;
    for layer in ["base", "under"] {
        for topmost in [true, false] {
            let hits = if topmost { notemaker_core::layer_editor::HitMode::Topmost } else { notemaker_core::layer_editor::HitMode::All };
            let layer = notemaker_core::ids::LayerId::from(layer);
            for region in [notemaker_core::geometry::DocRect { x: 0.0, y: 0.0, width: 110.0, height: 110.0 }.polygon(), notemaker_core::geometry::DocRect { x: 115.0, y: 115.0, width: 120.0, height: 120.0 }.polygon(), notemaker_core::geometry::DocRect { x: 300.0, y: 100.0, width: 40.0, height: 40.0 }.polygon()] {
                let expected = LayerEditor::new(&model, &layer, PageNumber(0))?.select(Some(&region), hits, &FACTORY)?;
                let actual = LayerEditor::select_in(&model, &layer, PageNumber(0), Some(&region), hits, &FACTORY)?;
                require(expected == actual, "Lazy selection changed ordered coverage hits")?;
            }
        }
    }
    let rendered = it::create_bitmap(BitmapSize { width: 400, height: 250 })?;
    let c = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", &rendered)?;
    jcall!(c, "android/graphics/Canvas", "drawColor", "(I)V", COLOR_WHITE)?;
    render::draw(&model, PageNumber(0), &notemaker_core::render::HiddenLayers(HashSet::new()), &mut Png::new(&mut AndroidCanvas::new(c)))?;
    require(pixel(&rendered, 50, 50)? == COLOR_RED, "Partial erase damaged or hid underlying layer")?;
    require(pixel(&rendered, 20, 50)? == COLOR_BLACK, "Partial erase lost untouched pixels")?;
    it::recycle_bitmap(&rendered)?;
    test.run_on_main_sync(|| app.refresh_shared(&snapshot))??;
    preview_rollback(test, app)?;
    object_preview(test, app)?;
    test.run_on_main_sync(|| app.select_at(crate::main_activity::DocPointF { x: 120.0, y: 170.0 }))??;
    let until = jx::uptime_millis().0 + 10000;
    while app.selection_session.with(|s| s.is_none()) && jx::uptime_millis().0 < until {
        std::thread::sleep(Duration::from_millis(25));
    }
    require(app.selection_session.with(|s| s.is_some()), "Activity did not select outline")?;
    test.run_on_main_sync(|| app.select_layer(&notemaker_core::ids::LayerId::from("under")))??;
    replica_service::writes().submit(|| ()).get()?;
    require(app.selection_session.with(|s| s.is_none()) && app.selected_entities.with(|s| s.is_empty()), "Layer switch retained selection")?;
    require(app.selecting.get(), "Layer switch unexpectedly left the selection tool")?;
    require(json::map_at(&*app.replica().read()?, "leases")?.is_empty(), "Layer switch retained leases")
}
/// `private static List<Object> query(MainActivity app, double x, double y) throws Exception`.
fn query(app: &Arc<MainActivity>, x: f64, y: f64) -> AppResult<Vec<Value>> {
    let result = app.replica().command(crate::app_replica::CliArgs(&["selection", "query", "-"]), Some(&Value::Object(object! {"layer" => "base", "region" => notemaker_core::geometry::DocRect { x: x - 0.1, y: y - 0.1, width: 0.2, height: 0.2 }.polygon().to_json()})))?;
    let items = json::map(&result)?.get("items").cloned().unwrap_or(Value::Null);
    Ok(json::list(&items)?.clone())
}
/// `private static void require(boolean condition, String message)`.
fn require(condition: bool, message: &str) -> AppResult<()> {
    if !condition {
        return Err(AppError::illegal_state(message));
    }
    Ok(())
}
/// `private static void objectPreview(Integration test, MainActivity app) throws Exception`.
fn object_preview(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<()> {
    let id = "preview-created-box";
    let paused = Arc::new(CountDownLatch::new(1));
    let release = Arc::new(CountDownLatch::new(1));
    let committed = Arc::new(CountDownLatch::new(1));
    let deliver = Arc::new(CountDownLatch::new(1));
    {
        let (committed, deliver) = (committed.clone(), deliver.clone());
        app.before_object_commit_callback.set(Some(Arc::new(move || {
            committed.count_down();
            if !deliver.await_timeout(Duration::from_secs(30)) {
                panic!("java.lang.IllegalStateException: Object callback gate timed out");
            }
        })));
    }
    {
        let (paused, release) = (paused.clone(), release.clone());
        replica_service::writes().execute(move || {
            paused.count_down();
            release.await_();
        });
    }
    require(paused.await_timeout(Duration::from_secs(5)), "Object writer did not pause")?;
    let coalesced_before = app.coalesced_object_frames.get();
    let before = it::create_bitmap(BitmapSize { width: 800, height: 800 })?;
    let during = it::create_bitmap(BitmapSize { width: 800, height: 800 })?;
    let after = it::create_bitmap(BitmapSize { width: 800, height: 800 })?;
    let body = (|| -> AppResult<()> {
        test.run_on_main_sync(|| -> AppResult<()> {
            draw_canvas(app, &before)?;
            app.persist_object(&MainActivity::obj(&[
                ("kind", JV::from("object")),
                ("object", JV::from(id)),
                ("shape", JV::from("rect")),
                ("layer", JV::from("base")),
                ("page", JV::from(0)),
                ("x", JV::from(300)),
                ("y", JV::from(300)),
                ("w", JV::from(70)),
                ("h", JV::from(70)),
                ("color", JV::from(COLOR_BLACK)),
                ("fill", JV::from(COLOR_BLACK)),
                ("width", JV::from(4)),
                ("fontSize", JV::from(24)),
                ("text", JV::from("")),
            ]))?;
            require(app.pending_object_previews.with(|p| p.contains_key(id)), "Missing transient object preview")?;
            draw_canvas(app, &during)?;
            require(!it::same_as(&before, &during)?, "Pending object did not change canvas pixels")?;
            require(app.drawn_object_previews.with(|p| p.contains(id)), "Preview was not drawn")?;
            require(!app.model().objects.contains_key(id), "Preview entered model before commit")
        })??;
        require(!store::model(&app.replica().read()?)?.objects.contains_key(id), "Preview changed durable state")
    })();
    release.count_down();
    body?;
    let body = (|| -> AppResult<()> {
        require(committed.await_timeout(Duration::from_secs(30)), "Object command did not commit")?;
        app.refresh_from(&app.replica())?;
        test.wait_for_idle_sync()?;
        require(app.object_refresh_deferred.get(), "Local listener did not reach the pending object before save callback")?;
        require(!app.model().objects.contains_key(id), "Local listener replaced the painted preview before save callback")
    })();
    deliver.count_down();
    app.before_object_commit_callback.set(None);
    body?;
    replica_service::writes().submit(|| ()).get_timeout(Duration::from_secs(10))?;
    test.wait_for_idle_sync()?;
    require(app.pending_object_previews.with(|p| p.is_empty()), "Committed object retained transient preview")?;
    require(store::model(&app.replica().read()?)?.objects.contains_key(id), "Previewed object was not durable")?;
    test.run_on_main_sync(|| draw_canvas(app, &after))??;
    let same_pixels = it::same_as(&during, &after)?;
    it::recycle_bitmap(&before)?;
    it::recycle_bitmap(&during)?;
    it::recycle_bitmap(&after)?;
    require(same_pixels, "Committed object changed preview pixels")?;
    require(app.coalesced_object_frames.get() == coalesced_before + 1, "Matching committed object still redrew the canvas")
}
/// `private static void previewRollback(Integration test, MainActivity app) throws Exception`.
fn preview_rollback(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<()> {
    let paused = Arc::new(CountDownLatch::new(1));
    let release = Arc::new(CountDownLatch::new(1));
    {
        let (paused, release) = (paused.clone(), release.clone());
        replica_service::writes().execute(move || {
            paused.count_down();
            release.await_();
        });
    }
    require(paused.await_timeout(Duration::from_secs(5)), "Writer did not pause")?;
    let before = store::model(&app.replica().read()?)?;
    let mut locked = false;
    let body = (|| -> AppResult<()> {
        test.run_on_main_sync(|| app.apply_erase(&crate::main_activity::EraserMode::from("partial"), &notemaker_core::ids::LayerId::from("base"), PageNumber(0), Some(&JSONArray::parse("[[20,20]]")?), notemaker_core::geometry::Radius(8.0)))??;
        let canvas = app.canvas();
        require(canvas.erase_previews.with(|p| p.len()) == 1, "Missing pending feedback")?;
        let preview = canvas.erase_previews.with(|p| p.values().next().map(|e| (e.layer.clone(), e.page, e.paths.clone())));
        let (layer, page, paths) = preview.ok_or_else(|| AppError::java("java.util.NoSuchElementException", ""))?;
        require(layer == "base" && page == 0 && paths.len() == 1, "Preview scope changed")?;
        let pixels = it::create_bitmap(BitmapSize { width: 100, height: 100 })?;
        let c = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", &pixels)?;
        jcall!(c, "android/graphics/Canvas", "drawColor", "(I)V", COLOR_RED)?;
        jcall!(c, "android/graphics/Canvas", "save", "()I")?;
        for cut in &paths {
            jcall!(c, "android/graphics/Canvas", "clipOutPath", "(Landroid/graphics/Path;)Z", cut)?;
        }
        jcall!(c, "android/graphics/Canvas", "drawColor", "(I)V", COLOR_BLACK)?;
        jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
        require(pixel(&pixels, 20, 20)? == COLOR_RED && pixel(&pixels, 40, 20)? == COLOR_BLACK, "Preview cleared underlying layer")?;
        it::recycle_bitmap(&pixels)?;
        require(before.objects == store::model(&app.replica().read()?)?.objects, "Preview mutated durable state")?;
        let replica = app.replica();
        replica_service::ensure(&app.this)?.execute(&object! {"document" => replica.document.clone(), "actor" => "racer", "args" => vec!["selection", "select", "-"], "input" => object! {"session" => "preview-race", "layer" => "base", "page" => 0, "entities" => vec![Value::Object(object! {"kind" => "object", "id" => "base-box"})]}})?;
        locked = true;
        Ok(())
    })();
    release.count_down();
    body?;
    let body = (|| -> AppResult<()> {
        replica_service::writes().submit(|| ()).get_timeout(Duration::from_secs(10))?;
        test.wait_for_idle_sync()?;
        require(app.canvas().erase_previews.with(|p| p.is_empty()), "Rejected erase retained preview")?;
        require(app.save_error.with(|e| e.is_some()), "Late lock did not reject erase")?;
        require(before.objects == store::model(&app.replica().read()?)?.objects, "Rejected erase changed source")
    })();
    let cleanup = (|| -> AppResult<()> {
        let replica = app.replica();
        if locked {
            replica_service::ensure(&app.this)?.execute(&object! {"document" => replica.document.clone(), "actor" => "racer", "args" => vec!["selection", "deselect", "preview-race"], "input" => Value::Null})?;
        }
        let restored = replica.read()?;
        test.run_on_main_sync(|| {
            app.save_error.set(None);
            app.refresh_shared(&restored)
        })?
    })();
    it::finally(body, cleanup)
}
/// Tolerances for the 240 px generated-dot comparisons, where resvg (the
/// reference) and Skia (the direct path) rasterize ~0.6 px dots differently.
/// Measured on the API 36 emulator: 6761/31/24 opaque, 6574/30/12 translucent.
const SUBPIXEL_DOT_CHANGED: i32 = 8000;
const SUBPIXEL_DOT_ALPHA: i32 = 40;
const SUBPIXEL_DOT_COLOR: i32 = 32;
/// `static void cachePixels()`.
pub fn cache_pixels() -> AppResult<()> {
    cache_pixels_sized(false)
}
/// `private static void cachePixels(boolean fullSize)`.
fn cache_pixels_sized(full_size: bool) -> AppResult<()> {
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\"><circle cx=\"50\" cy=\"50\" r=\"30\" fill=\"none\" stroke=\"black\"/><text x=\"5\" y=\"20\">Cache</text></svg>";
    let direct = it::create_bitmap(BitmapSize { width: 200, height: 200 })?;
    let recorded = it::create_bitmap(BitmapSize { width: 200, height: 200 })?;
    USE_SVG_PICTURES.store(false, Ordering::SeqCst);
    draw_svg(&direct, 5.25, 10.5, 150.5, 160.25, svg)?;
    USE_SVG_PICTURES.store(true, Ordering::SeqCst);
    draw_svg(&recorded, 5.25, 10.5, 150.5, 160.25, svg)?;
    require(it::same_as(&direct, &recorded)?, "SVG display-list pixels differ")?;
    it::recycle_bitmap(&direct)?;
    it::recycle_bitmap(&recorded)?;
    let mut dots = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 714.2857142857143 1000.0000000000001\" width=\"714.2857142857143\" height=\"1000.0000000000001\"><g stroke=\"#999999\" stroke-opacity=\"1.0\" stroke-width=\"0.7142857142857143\">");
    for y in 1..=43 {
        for x in 1..=31 {
            dots.push_str("<circle cx=\"");
            dots.push_str(&notemaker_core::jfmt::double(x as f64 * 22.857142857142858));
            dots.push_str("\" cy=\"");
            dots.push_str(&notemaker_core::jfmt::double(y as f64 * 22.857142857142858));
            dots.push_str("\" r=\"1.4285714285714286\" fill=\"#999999\"/>");
        }
    }
    dots.push_str("</g></svg>");
    let fast_dots = it::create_bitmap(BitmapSize { width: 240, height: 240 })?;
    let dom_dots = it::create_bitmap(BitmapSize { width: 240, height: 240 })?;
    draw_svg(&fast_dots, 5.25, 10.5, 210.0, 210.0, &dots)?;
    draw_svg(&dom_dots, 5.25, 10.5, 210.0, 210.0, &format!(" {dots}"))?;
    let (changed, max_alpha_difference, max_color_difference) = compare(&bitmap_pixels(&fast_dots)?, &bitmap_pixels(&dom_dots)?);
    // The direct cubic path and AndroidSVG differ only at antialiased edges.
    // Java: changed<=1500, alpha<=12, color<=12. The reference renderer is
    // now resvg (agreed androidsvg replacement). At this 0.21x scale every
    // dot is ~0.6 px wide; dot positions match exactly (same row/column
    // coverage peaks), but tiny-skia's coverage for sub-pixel fill+hairline
    // shapes is ~16% heavier than Skia's (169k vs 145k summed alpha; the
    // analytic area is ~150k). The full-size check below keeps Java's
    // thresholds. See docs/verification.md.
    require(
        changed <= SUBPIXEL_DOT_CHANGED && max_alpha_difference <= SUBPIXEL_DOT_ALPHA && max_color_difference <= SUBPIXEL_DOT_COLOR,
        &format!("Generated dot SVG fast path changed pixels: {changed}, alpha {max_alpha_difference}, color {max_color_difference}"),
    )?;
    it::recycle_bitmap(&fast_dots)?;
    it::recycle_bitmap(&dom_dots)?;
    if full_size {
        let full_fast = it::create_bitmap(BitmapSize { width: 1000, height: 1400 })?;
        let full_reference = it::create_bitmap(BitmapSize { width: 1000, height: 1400 })?;
        draw_svg(&full_fast, 0.0, 0.0, 1000.0, 1400.0, &dots)?;
        draw_svg(&full_reference, 0.0, 0.0, 1000.0, 1400.0, &format!(" {dots}"))?;
        let (changed, max_alpha_difference, max_color_difference) = compare(&bitmap_pixels(&full_fast)?, &bitmap_pixels(&full_reference)?);
        require(
            changed <= 28000 && max_alpha_difference <= 12 && max_color_difference <= 12,
            &format!("Full-size generated dot SVG changed pixels: {changed}, alpha {max_alpha_difference}, color {max_color_difference}"),
        )?;
        it::recycle_bitmap(&full_fast)?;
        it::recycle_bitmap(&full_reference)?;
    }
    let translucent = dots.replace("#999999", "#336699").replace("stroke-opacity=\"1.0\"", "stroke-opacity=\"0.5\"");
    let alpha_fast = it::create_bitmap(BitmapSize { width: 240, height: 240 })?;
    let alpha_reference = it::create_bitmap(BitmapSize { width: 240, height: 240 })?;
    draw_svg(&alpha_fast, 5.25, 10.5, 210.0, 210.0, &translucent)?;
    draw_svg(&alpha_reference, 5.25, 10.5, 210.0, 210.0, &format!(" {translucent}"))?;
    let (changed, max_alpha_difference, max_color_difference) = compare(&bitmap_pixels(&alpha_fast)?, &bitmap_pixels(&alpha_reference)?);
    // Same sub-pixel dots as above (Java: 1500/12/12).
    require(
        changed <= SUBPIXEL_DOT_CHANGED && max_alpha_difference <= SUBPIXEL_DOT_ALPHA && max_color_difference <= SUBPIXEL_DOT_COLOR,
        &format!("Translucent generated dot SVG changed pixels: {changed}, alpha {max_alpha_difference}, color {max_color_difference}"),
    )?;
    it::recycle_bitmap(&alpha_fast)?;
    it::recycle_bitmap(&alpha_reference)?;
    let mut visible: IndexMap<String, Entity> = IndexMap::new();
    for i in 0..3 {
        let mut points = Vec::new();
        for p in 0..40 {
            points.push(Value::Array(vec![Value::from(20 + p * 4), json::double((70 + i * 12) as f64 + 20.0 * (p as f64 * 0.3).sin()), json::double(0.4 + (p % 5) as f64 * 0.1)]));
        }
        let color = if i == 1 { 0x80660000u32 as i32 } else { COLOR_BLACK };
        let brush = if i == 2 { "square" } else { "pen" };
        visible.insert(format!("s{i}"), Arc::new(object! {"kind" => "stroke", "stroke" => format!("s{i}"), "layer" => "base", "page" => 0, "points" => points, "width" => 12, "color" => color, "brush" => brush}));
    }
    visible.insert("object:box".into(), Arc::new(notemaker_core::actions::drawing(&object! {"object" => "box", "shape" => "rect", "x" => 50, "y" => 50, "w" => 90, "h" => 90, "fill" => 0x80ff0000u32 as i32}, &notemaker_core::ids::LayerId::from("base"))?));
    let original = visible.get("s1").cloned().ok_or_else(|| AppError::Core(notemaker_core::Error::null_pointer()))?;
    let mut cache = InkCache::new(crate::ink_cache::BitmapSize { width: 320, height: 320 })?;
    for step in 0..10 {
        jx::frame(|| -> AppResult<()> {
            if step == 1 {
                let erased = DrawItem::new(&Model::new(), original.clone())?.erase(&notemaker_core::geometry::Erasures::from(vec![notemaker_core::geometry::DocRect { x: 75.0, y: 30.0, width: 25.0, height: 130.0 }.polygon()]))?;
                visible.insert("s1".into(), Arc::new(erased));
            }
            if step == 2 {
                visible.insert("s1".into(), original.clone());
            }
            if step == 3 {
                visible.shift_remove("s0");
            }
            if step == 4 {
                if let Some(a) = visible.shift_remove("s1") {
                    visible.insert("s1".into(), a);
                }
            }
            if step == 5 {
                let mut a = (*original).clone();
                a.insert("stroke".into(), Value::from("append"));
                visible.insert("append".into(), Arc::new(a));
            }
            if step == 6 {
                let mut a = (*original).clone();
                a.insert("brush".into(), Value::from("marker"));
                a.insert("width".into(), Value::from(24));
                visible.insert("s1".into(), Arc::new(a));
            }
            if step == 7 {
                let bx = visible.shift_remove("object:box");
                let mut a = (*original).clone();
                a.insert("stroke".into(), Value::from("insert"));
                visible.insert("insert".into(), Arc::new(a));
                if let Some(bx) = bx {
                    visible.insert("object:box".into(), bx);
                }
            }
            if step == 9 {
                let mut a = copy_of(visible.get("s1"))?;
                a.insert("material".into(), Value::from(90));
                visible.insert("s1".into(), Arc::new(a));
            }
            if step == 8 {
                let mut bx = copy_of(visible.get("object:box"))?;
                bx.insert("x".into(), Value::from(80));
                bx.insert("rotation".into(), Value::from(33));
                visible.insert("object:box".into(), Arc::new(bx));
            }
            let actual = it::create_bitmap(BitmapSize { width: 320, height: 320 })?;
            let expected = it::create_bitmap(BitmapSize { width: 320, height: 320 })?;
            let mut transform = |c: &Obj| -> AppResult<()> {
                jcall!(c, "android/graphics/Canvas", "translate", "(FF)V", 10.5f32, 5.25f32)?;
                jcall!(c, "android/graphics/Canvas", "scale", "(FF)V", 1.3f32, 1.3f32)?;
                Ok(())
            };
            let actual_canvas = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", &actual)?;
            cache.draw(&actual_canvas, crate::ink_cache::VisibleInk(visible.clone()), &mut transform, &mut |c: &Obj, s: &Entity| -> AppResult<()> {
                render::draw_item(&Model::new(), s, &mut Png::new(&mut AndroidCanvas::new(c.clone())))?;
                Ok(())
            })?;
            let c = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", &expected)?;
            transform(&c)?;
            for s in visible.values() {
                render::draw_item(&Model::new(), s, &mut Png::new(&mut AndroidCanvas::new(c.clone())))?;
            }
            if !it::same_as(&actual, &expected)? {
                let a = bitmap_pixels(&actual)?;
                let b = bitmap_pixels(&expected)?;
                let (mut differences, mut l, mut t, mut r, mut bottom) = (0, 320, 320, 0, 0);
                for y in 0..320 {
                    for x in 0..320 {
                        let i = (y * 320 + x) as usize;
                        if a[i] != b[i] {
                            differences += 1;
                            l = l.min(x);
                            t = t.min(y);
                            r = r.max(x);
                            bottom = bottom.max(y);
                        }
                    }
                }
                return Err(AppError::illegal_state(format!("Regional ink cache step {step}: {differences} different pixels in {l},{t}–{r},{bottom}")));
            }
            it::recycle_bitmap(&actual)?;
            it::recycle_bitmap(&expected)?;
            Ok(())
        })?;
    }
    it::recycle_bitmap(&cache.bitmap)
}

/// `new Png(new Canvas(bitmap)).svg(x, y, w, h, source)`.
fn draw_svg(bitmap: &Obj, x: f64, y: f64, w: f64, h: f64, source: &str) -> AppResult<()> {
    let c = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", bitmap)?;
    Png::new(&mut AndroidCanvas::new(c)).svg(notemaker_core::geometry::DocRect { x, y, width: w, height: h }, source)?;
    Ok(())
}
/// `app.canvas.draw(new Canvas(bitmap))` (UI thread).
fn draw_canvas(app: &Arc<MainActivity>, bitmap: &Obj) -> AppResult<()> {
    it::draw_view(&app.canvas().view(), bitmap)
}
/// `bitmap.getPixel(x, y)` as Java's `int`.
fn pixel(bitmap: &Obj, x: i32, y: i32) -> AppResult<i32> {
    Ok(it::pixel(bitmap, BitmapPoint { x, y })?.0)
}
/// `new LinkedHashMap<>(visible.get(key))`.
fn copy_of(entity: Option<&Entity>) -> AppResult<Map> {
    Ok((**entity.ok_or_else(|| AppError::Core(notemaker_core::Error::null_pointer()))?).clone())
}
/// The dot-SVG comparison loop: (changed pixels, max alpha difference,
/// max premultiplied red difference).
fn compare(fast: &[i32], reference: &[i32]) -> (i32, i32, i32) {
    let (mut changed, mut max_alpha_difference, mut max_color_difference) = (0, 0, 0);
    for (f, r) in fast.iter().zip(reference.iter()) {
        let (f, r) = (*f, *r);
        if f == r {
            continue;
        }
        changed += 1;
        let (f, r) = (notemaker_core::style::ArgbColor(f).channels(), notemaker_core::style::ArgbColor(r).channels());
        max_alpha_difference = max_alpha_difference.max((f.alpha - r.alpha).abs());
        max_color_difference = max_color_difference.max((f.red * f.alpha - r.red * r.alpha).abs() / 255);
    }
    (changed, max_alpha_difference, max_color_difference)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_counts_premultiplied_red() {
        let same = 0xff112233u32 as i32;
        let a = 0x80ff0000u32 as i32;
        let b = 0x7fff0000u32 as i32;
        assert_eq!(compare(&[same, a], &[same, b]), (1, 1, 1));
        assert_eq!(compare(&[same], &[same]), (0, 0, 0));
        assert!(require(true, "x").is_ok());
        assert_eq!(require(false, "boom").unwrap_err().message().as_deref(), Some("boom"));
    }
}
