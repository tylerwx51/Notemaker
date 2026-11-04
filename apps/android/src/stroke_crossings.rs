//! Port of `StrokeCrossings.java` (instrumentation check): crossing ink must
//! be independent of whether the pen was lifted between passes. Renders into
//! real Android `Bitmap`s through `AndroidCanvas`. Runs on the
//! instrumentation thread.

use crate::android_canvas::AndroidCanvas;
use crate::android_consts::COLOR_WHITE;
use crate::error::{self, AppResult};
use crate::ink_cache::BitmapSize;
use crate::integration::{create_bitmap, pixel, BitmapPoint};
use crate::jx::{self, Obj};
use notemaker_cli::Png;
use notemaker_core::json::Value;
use notemaker_core::{jfmt, jmath, object, render};

// ---------------------------------------------------------------------------
// Bitmap helpers shared with `viewport_check`.

/// `new Canvas(bitmap)`.
pub(crate) fn bitmap_canvas(bitmap: &Obj) -> AppResult<Obj> {
    Ok(jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", bitmap)?)
}
/// `bitmap.recycle()`.
pub(crate) fn recycle(bitmap: &Obj) -> AppResult<()> {
    jcall!(bitmap, "android/graphics/Bitmap", "recycle", "()V")?;
    Ok(())
}
/// Runs `f`, then recycles `bitmaps` (Java `try{...}finally{a.recycle();...}`).
fn finally_recycle<T>(bitmaps: &[&Obj], f: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
    let result = f();
    for b in bitmaps {
        recycle(b)?;
    }
    result
}

/// `private static Bitmap render(double[][][] paths, String brush, int color)`.
fn render(paths: &[Vec<[f64; 2]>], brush: &str, color: i32) -> AppResult<Obj> {
    let image = create_bitmap(BitmapSize { width: 200, height: 200 })?;
    let canvas = bitmap_canvas(&image)?;
    jcall!(canvas, "android/graphics/Canvas", "drawColor", "(I)V", COLOR_WHITE)?;
    for path in paths {
        let points: Vec<Value> = path.iter().map(|p| Value::from(vec![Value::from(p[0]), Value::from(p[1]), Value::from(1)])).collect();
        let stroke = object! {"width" => 12, "brush" => brush, "material" => 50, "color" => color, "points" => points};
        let mut surface = AndroidCanvas::new(canvas.clone());
        render::draw_stroke((&stroke).into(), &mut Png::new(&mut surface))?;
    }
    Ok(image)
}
/// `private static void seams()`.
fn seams() -> AppResult<()> {
    let circle: Vec<[f64; 2]> = (0..181)
        .map(|i| {
            let angle = jmath::to_radians((i * 2) as f64);
            [100.0 + 60.0 * jmath::cos(angle), 100.0 + 60.0 * jmath::sin(angle)]
        })
        .collect();
    for brush in ["pen", "marker"] {
        let color = if brush == "pen" { 0x80000000u32 as i32 } else { 0xff000000u32 as i32 };
        let curve = render(std::slice::from_ref(&circle), brush, color)?;
        let straight = render(&[vec![[30.0, 100.0], [170.0, 100.0]]], brush, color)?;
        finally_recycle(&[&curve, &straight], || {
            let ink = pixel(&straight, BitmapPoint { x: 100, y: 100 })?;
            for join in [90, 180, 270] {
                for degrees in (join - 15)..=(join + 15) {
                    let angle = jmath::to_radians(degrees as f64);
                    let x = jmath::round(100.0 + 60.0 * jmath::cos(angle)) as i32;
                    let y = jmath::round(100.0 + 60.0 * jmath::sin(angle)) as i32;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            error::require(pixel(&curve, BitmapPoint { x: x + dx, y: y + dy })? == ink, &format!("{brush} ribbon seam at {degrees} degrees"))?;
                        }
                    }
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}
/// `static void run()`.
pub fn run() -> AppResult<()> {
    seams()?;
    let loops: [Vec<[f64; 2]>; 3] = [
        vec![[30.0, 30.0], [170.0, 170.0], [170.0, 30.0], [30.0, 170.0]],
        vec![[30.0, 100.0], [170.0, 100.0], [170.0, 30.0], [100.0, 30.0], [100.0, 170.0]],
        vec![[30.0, 100.0], [170.0, 100.0], [30.0, 100.0]],
    ];
    let lines: [Vec<Vec<[f64; 2]>>; 3] = [
        vec![vec![[30.0, 30.0], [170.0, 170.0]], vec![[170.0, 30.0], [30.0, 170.0]]],
        vec![vec![[30.0, 100.0], [170.0, 100.0]], vec![[100.0, 30.0], [100.0, 170.0]]],
        vec![vec![[30.0, 100.0], [170.0, 100.0]], vec![[170.0, 100.0], [30.0, 100.0]]],
    ];
    let brushes = ["pen", "pen", "marker"];
    let colors = [0xff000000u32 as i32, 0x80000000u32 as i32, 0xff000000u32 as i32];
    for style in 0..brushes.len() {
        for fixture in 0..loops.len() {
            for reverse in [false, true] {
                jx::frame(|| -> AppResult<()> {
                    let mut path = loops[fixture].clone();
                    if reverse {
                        path.reverse();
                    }
                    let actual = render(&[path], brushes[style], colors[style])?;
                    let expected = render(&lines[fixture], brushes[style], colors[style])?;
                    finally_recycle(&[&actual, &expected], || {
                        let mut max = 0;
                        let mut sum: i64 = 0;
                        let mut interior_changes = 0;
                        for y in 70..130 {
                            for x in 70..130 {
                                let difference = (pixel(&actual, BitmapPoint { x, y })?.channels().red - pixel(&expected, BitmapPoint { x, y })?.channels().red).abs();
                                max = max.max(difference);
                                sum += difference as i64;
                                let mut flat = true;
                                let p = pixel(&expected, BitmapPoint { x, y })?;
                                for dy in -1..=1 {
                                    for dx in -1..=1 {
                                        if pixel(&expected, BitmapPoint { x: x + dx, y: y + dy })? != p {
                                            flat = false;
                                        }
                                    }
                                }
                                if flat && pixel(&actual, BitmapPoint { x, y })? != p {
                                    interior_changes += 1;
                                }
                            }
                        }
                        let label = format!("{} color={} fixture={} reversed={}", brushes[style], colors[style], fixture, reverse);
                        // A union and two source-over passes differ at antialiased crossing
                        // corners; fully covered ink and paper must match exactly.
                        error::require(interior_changes == 0, &format!("Crossing interior changed: {label} pixels={interior_changes}"))?;
                        let mean = sum as f64 / (60.0 * 60.0);
                        error::require(max <= 64 && mean < 0.34, &format!("Self crossing changed coverage: {label} max={max} mean={}", jfmt::double(mean)))?;
                        error::require(pixel(&actual, BitmapPoint { x: 100, y: 100 })? == pixel(&expected, BitmapPoint { x: 100, y: 100 })?, &format!("Crossing ink density changed: {label}"))?;
                        Ok(())
                    })
                })?;
            }
        }
    }
    Ok(())
}
