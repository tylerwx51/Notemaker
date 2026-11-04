//! Port of `ErasePerformance.java` (instrumentation benchmark): real
//! input-to-updated-draw measurements, restricted to the isolated test
//! package. Erase latency phases are recorded through
//! `MainActivity.erase_timings`.

use crate::concurrent::CountDownLatch;
use crate::coverage_check;
use crate::error::{self, AppError, AppResult};
use crate::field::Field;
use crate::integration::{GestureDrag, GestureEnd, Integration};
use crate::jx;
use crate::main_activity::MainActivity;
use crate::performance::{drain_writes, sleep, window, write_file_output};
use indexmap::IndexMap;
use notemaker_core::json::{self, Map, Value};
use notemaker_core::layer_editor::LayerEditor;
use notemaker_core::{geometry, object, store};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Iteration order of a `ConcurrentHashMap<String,Long>` (default table of 16,
/// resized when the size reaches 3/4 of the table) after inserting `keys`:
/// bins by spread hash, insertion order within a bin.
fn concurrent_hash_map_order(keys: Vec<String>) -> Vec<String> {
    let mut capacity: usize = 16;
    while keys.len() >= capacity - (capacity >> 2) {
        capacity *= 2;
    }
    let bin = |k: &String| {
        let h = json::java_string_hash(k);
        let spread = (h ^ ((h as u32) >> 16) as i32) & 0x7fffffff;
        (spread as usize) & (capacity - 1)
    };
    let mut indexed: Vec<(usize, usize, String)> = keys.into_iter().enumerate().map(|(i, k)| (bin(&k), i, k)).collect();
    indexed.sort_by_key(|(b, i, _)| (*b, *i));
    indexed.into_iter().map(|(_, _, k)| k).collect()
}

/// `static Map<String,Object> run(Integration test, MainActivity app) throws Exception`.
pub fn run(test: &Arc<Integration>, app: &Arc<MainActivity>) -> AppResult<crate::performance::PerformanceReport> {
    let package = jcall!(app.this, "android/content/Context", "getPackageName", "()Ljava/lang/String;")?.string().unwrap_or_default();
    error::require(package == "com.notemaker.latency", "Erase benchmark requires isolated latency package")?;
    coverage_check::cache_pixels()?;
    drain_writes()?;
    test.wait_for_idle_sync()?;
    let original = store::model(&app.replica().read()?)?;
    let mut samples: Vec<Value> = Vec::new();
    let arguments = test.arguments.get().expect("arguments");
    let mode = jcall!(arguments, "android/os/Bundle", "getString", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;", "mode", "partial")?.string().unwrap_or_default();
    test.run_on_main_sync(|| -> AppResult<()> {
        app.set_drawing_tool(None)?;
        app.erase.set(true);
        app.eraser_mode.set(mode.clone());
        app.eraser_radius.set(12.0);
        Ok(())
    })??;
    let editor = LayerEditor::new(&original, &notemaker_core::ids::LayerId::from(app.selected.get()), notemaker_core::ids::PageNumber(app.page.get()))?;
    for item in editor.items() {
        if item.kind != "stroke" {
            continue;
        }
        let pts = json::list(item.content.get("points").unwrap_or(&Value::Null))?;
        let point = json::list(&pts[pts.len() / 2])?;
        let x = json::as_double(point.first())? as f32;
        let y = json::as_double(point.get(1))? as f32;
        if !item.intersects(&geometry::Polygon::capsule(geometry::Segment { start: geometry::DocPoint { x: x as f64, y: y as f64 }, end: geometry::DocPoint { x: x as f64, y: y as f64 } }, geometry::Radius(12.0)), &notemaker_cli::item_coverage::FACTORY)? {
            continue;
        }
        let drawn = Arc::new(CountDownLatch::new(1));
        let start = Arc::new(AtomicI64::new(0));
        let elapsed = Arc::new(AtomicI64::new(0));
        let feedback = Arc::new(AtomicI64::new(0));
        let old_masks = item.content.get("erasures").cloned();
        let trace: Arc<Field<IndexMap<String, i64>>> = Arc::new(Field::new(IndexMap::new()));
        let item_id = item.id.clone();
        test.run_on_main_sync(|| -> AppResult<()> {
            app.erase_timings.set(Some(trace.clone()));
            let canvas = app.canvas();
            {
                let weak = Arc::downgrade(app);
                let (drawn, start, elapsed, feedback, trace) = (drawn.clone(), start.clone(), elapsed.clone(), feedback.clone(), trace.clone());
                let (id, old_masks) = (item_id.clone(), old_masks.clone());
                canvas.after_draw.set(Some(Arc::new(move || {
                    let Some(app) = weak.upgrade() else { return };
                    let canvas = app.canvas();
                    if feedback.load(Ordering::SeqCst) == 0 && !canvas.erase_previews.with(|p| p.is_empty()) {
                        feedback.store(jx::nano_time().0 - start.load(Ordering::SeqCst), Ordering::SeqCst);
                        // ConcurrentHashMap.put(key, null) threw NullPointerException.
                        let draw_start = trace.with(|t| t.get("draw_start").copied()).unwrap_or_else(|| panic!("java.lang.NullPointerException"));
                        trace.with_mut(|t| t.insert("feedback_draw_start".into(), draw_start));
                        trace.with_mut(|t| t.insert("feedback_draw_done".into(), jx::nano_time().0));
                    }
                    let model = app.model();
                    if let Some(current) = model.strokes.get(&id) {
                        if !json::objects_equal(old_masks.as_ref(), current.get("erasures")) || model.active.get(&id) == Some(&false) {
                            elapsed.store(jx::nano_time().0 - start.load(Ordering::SeqCst), Ordering::SeqCst);
                            if feedback.load(Ordering::SeqCst) == 0 {
                                feedback.store(elapsed.load(Ordering::SeqCst), Ordering::SeqCst);
                            }
                            canvas.after_draw.set(None);
                            drawn.count_down();
                        }
                    }
                })));
            }
            start.store(jx::nano_time().0, Ordering::SeqCst);
            Integration::gesture(app, GestureDrag { x, y, width: 0.0, height: 0.0 }, GestureEnd::Up)
        })??;
        error::require(drawn.await_timeout(Duration::from_secs(15)), "Updated content was not drawn")?;
        let mut phases = Map::new();
        let entries = trace.get();
        for key in concurrent_hash_map_order(entries.keys().cloned().collect()) {
            phases.insert(key.clone(), json::double((entries[&key] - start.load(Ordering::SeqCst)) as f64 / 1e6));
        }
        samples.push(Value::Object(object! {
            "points" => pts.len() as i64,
            "input_to_feedback_ms" => json::double(feedback.load(Ordering::SeqCst) as f64 / 1e6),
            "input_to_updated_draw_ms" => json::double(elapsed.load(Ordering::SeqCst) as f64 / 1e6),
            "phases_ms" => phases,
        }));
        test.run_on_main_sync(|| app.erase_timings.set(None))?;
        drain_writes()?;
        test.wait_for_idle_sync()?;
        app.replica().command(crate::app_replica::CliArgs(&["undo"]), None)?;
        let restored = app.replica().read()?;
        test.run_on_main_sync(|| -> AppResult<()> {
            (|| -> AppResult<()> {
                app.refresh_shared(&restored)?;
                app.update()
            })()
            .map_err(|e| AppError::runtime_cause(&e))
        })??;
        test.wait_for_idle_sync()?;
        sleep(jx::DelayMillis(100));
    }
    let decor = jcall!(window(app)?, "android/view/Window", "getDecorView", "()Landroid/view/View;")?.some();
    let display = jcall!(decor, "android/view/View", "getDisplay", "()Landroid/view/Display;")?.some();
    let refresh_rate = jcall!(display, "android/view/Display", "getRefreshRate", "()F")?.float();
    let mut report = object! {"samples" => samples, "mode" => mode.as_str(), "display_refresh_hz" => json::from_float(refresh_rate)};
    // Undo advances version metadata, so compare visible entity geometry separately.
    let after = store::model(&app.replica().read()?)?;
    let mut restored = original.strokes == after.strokes && original.objects == after.objects;
    for id in original.strokes.keys() {
        restored &= (original.active.get(id) == Some(&false)) == (after.active.get(id) == Some(&false));
    }
    for id in original.objects.keys() {
        restored &= (original.object_active.get(id) == Some(&false)) == (after.object_active.get(id) == Some(&false));
    }
    error::require(restored, "Undo did not restore source geometry and visibility")?;
    report.insert("geometry_restored".into(), Value::from(restored));
    write_file_output(app, "erase-performance.json", json::write_map(&report).as_bytes())?;
    Ok(report.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_hash_map_iteration_order() {
        // Bins are hash & 15: "a" (97) and "q" (113) share bin 1 in
        // insertion order, "b" (98) is bin 2.
        let order = concurrent_hash_map_order(vec!["b".into(), "a".into(), "q".into()]);
        assert_eq!(order, vec!["a".to_string(), "q".to_string(), "b".to_string()]);
    }
}
