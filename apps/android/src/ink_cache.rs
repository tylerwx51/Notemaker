//! Port of `InkCache.java`: settled layer pixels; edits repaint only the
//! damaged region in original stroke order (layer thumbnails in `EditorUi`).
//! UI thread only; instance methods take `&mut self`.

use crate::android_canvas::{argb_8888, rect_from_java, rect_to_java, RectFExt};
use crate::android_consts::COLOR_TRANSPARENT;
use crate::error::AppResult;
use crate::java_port::math::{float_value, fmax, fmin};
use crate::jx::{self, Obj};
use indexmap::IndexMap;
use notemaker_cli::canvas::RectF;
use notemaker_core::draw_item::DrawItem;
use notemaker_core::json::{self, Map, Value};
use notemaker_core::model::{Entity, Model};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

/// `static long rebuilds`.
pub static REBUILDS: AtomicI64 = AtomicI64::new(0);

/// `Consumer<Canvas> transform` (applied to the cache's Java `Canvas`).
pub type Transform<'a> = dyn FnMut(&Obj) -> AppResult<()> + 'a;
/// `BiConsumer<Canvas, Map<String,Object>> stroke`.
pub type StrokeRenderer<'a> = dyn FnMut(&Obj, &Entity) -> AppResult<()> + 'a;

/// `final class InkCache`.
pub struct InkCache {
    /// `final Bitmap bitmap` (ARGB_8888).
    pub bitmap: Obj,
    pub drawn: IndexMap<String, Entity>,
    /// `IdentityHashMap<Map,RectF> paintBounds`: keyed by `Arc::as_ptr`,
    /// holding the entity like Java's strong key.
    pub paint_bounds: HashMap<usize, (Entity, RectF)>,
}

/// The size of the cache bitmap in device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BitmapSize {
    pub width: i32,
    pub height: i32,
}

/// The settled strokes and objects to show, by id, in drawing order
/// (`LinkedHashMap<String, Map<String,Object>> visible`).
#[derive(Clone, Default)]
pub struct VisibleInk(pub IndexMap<String, Entity>);

fn ptr(e: &Entity) -> usize {
    Arc::as_ptr(e) as *const () as usize
}

impl InkCache {
    /// `InkCache(int width, int height)`.
    pub fn new(size: BitmapSize) -> AppResult<InkCache> {
        let BitmapSize { width, height } = size;
        let bitmap = jstatic!("android/graphics/Bitmap", "createBitmap", "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;", width, height, &argb_8888()?)?.some();
        Ok(InkCache { bitmap, drawn: IndexMap::new(), paint_bounds: HashMap::new() })
    }
    /// `void draw(Canvas destination, LinkedHashMap visible, Consumer<Canvas> transform, BiConsumer<Canvas,Map> stroke)`.
    pub fn draw(&mut self, destination: &Obj, visible: VisibleInk, transform: &mut Transform<'_>, stroke: &mut StrokeRenderer<'_>) -> AppResult<()> {
        let visible = visible.0;
        let mut changed = false;
        let mut reordered = false;
        let mut dirty = RectF::default();
        let mut old_order: Vec<&String> = Vec::new();
        let mut new_order: Vec<&String> = Vec::new();
        for id in self.drawn.keys() {
            if visible.contains_key(id) {
                old_order.push(id);
            }
        }
        let mut added = false;
        for id in visible.keys() {
            if self.drawn.contains_key(id) {
                new_order.push(id);
                if added {
                    changed = true;
                }
            } else {
                added = true;
            }
        }
        reordered |= old_order != new_order;
        drop(old_order);
        drop(new_order);
        let drawn: Vec<(String, Entity)> = self.drawn.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (key, old) in &drawn {
            let next = visible.get(key);
            if next.is_none_or(|next| !InkCache::same_ink(old, next)) {
                // A new native clipping path can change antialias rounding along
                // the entire edited stroke, so repaint its full paint bounds.
                changed = true;
                let b = self.bounds(old)?;
                dirty.union(&b);
                if let Some(next) = next {
                    let b = self.bounds(next)?;
                    dirty.union(&b);
                }
            }
        }
        let mut next_bounds: HashMap<usize, (Entity, RectF)> = HashMap::new();
        for value in visible.values() {
            let b = self.bounds(value)?;
            next_bounds.insert(ptr(value), (value.clone(), b));
        }
        let image = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", &self.bitmap)?;
        transform(&image)?;
        if reordered {
            REBUILDS.fetch_add(1, Ordering::Relaxed);
            jcall!(self.bitmap, "android/graphics/Bitmap", "eraseColor", "(I)V", COLOR_TRANSPARENT)?;
            for value in visible.values() {
                stroke(&image, value)?;
            }
        } else if changed {
            for (key, value) in &visible {
                if !self.drawn.contains_key(key) {
                    let b = self.bounds(value)?;
                    dirty.union(&b);
                }
            }
            // Native clipping can change edge coverage: repaint intersecting items
            // completely. Geometry bounds are cached, and reverse traversal follows
            // the usual newest-to-oldest chain without repeatedly scanning points.
            let mut remaining: Vec<RectF> = visible.values().map(|value| next_bounds[&ptr(value)].1).collect();
            loop {
                let mut grew = false;
                let mut i = remaining.len();
                while i > 0 {
                    i -= 1;
                    let b = remaining[i];
                    if RectF::intersects(&dirty, &b) {
                        if !dirty.contains_rect(&b) {
                            dirty.union(&b);
                            grew = true;
                        }
                        remaining.remove(i);
                    }
                }
                if !grew {
                    break;
                }
            }
            let matrix = jcall!(image, "android/graphics/Canvas", "getMatrix", "()Landroid/graphics/Matrix;")?.some();
            let inverse = jnew!("android/graphics/Matrix", "()V")?;
            let pixels_obj = rect_to_java(&dirty)?;
            jcall!(matrix, "android/graphics/Matrix", "mapRect", "(Landroid/graphics/RectF;)Z", &pixels_obj)?;
            let mut pixels = rect_from_java(&pixels_obj)?;
            pixels.set(
                pixels.left.floor() - 1.0,
                pixels.top.floor() - 1.0,
                pixels.right.ceil() + 1.0,
                pixels.bottom.ceil() + 1.0,
            );
            let pixels_obj = rect_to_java(&pixels)?;
            if jcall!(matrix, "android/graphics/Matrix", "invert", "(Landroid/graphics/Matrix;)Z", &inverse)?.bool() {
                let dirty_obj = rect_to_java(&dirty)?;
                jcall!(inverse, "android/graphics/Matrix", "mapRect", "(Landroid/graphics/RectF;Landroid/graphics/RectF;)Z", &dirty_obj, &pixels_obj)?;
                dirty = rect_from_java(&dirty_obj)?;
            }
            jcall!(image, "android/graphics/Canvas", "save", "()I")?;
            let identity = jnew!("android/graphics/Matrix", "()V")?;
            jcall!(image, "android/graphics/Canvas", "setMatrix", "(Landroid/graphics/Matrix;)V", &identity)?;
            jcall!(image, "android/graphics/Canvas", "clipRect", "(Landroid/graphics/RectF;)Z", &pixels_obj)?;
            let clear = jsget!("android/graphics/PorterDuff$Mode", "CLEAR", "Landroid/graphics/PorterDuff$Mode;")?.some();
            jcall!(image, "android/graphics/Canvas", "drawColor", "(ILandroid/graphics/PorterDuff$Mode;)V", COLOR_TRANSPARENT, &clear)?;
            jcall!(image, "android/graphics/Canvas", "setMatrix", "(Landroid/graphics/Matrix;)V", &matrix)?;
            for value in visible.values() {
                let b = self.bounds(value)?;
                if RectF::intersects(&dirty, &b) {
                    stroke(&image, value)?;
                }
            }
            jcall!(image, "android/graphics/Canvas", "restore", "()V")?;
        } else {
            for (key, value) in &visible {
                if !self.drawn.contains_key(key) {
                    stroke(&image, value)?;
                }
            }
        }
        self.drawn = visible;
        self.paint_bounds = next_bounds;
        jcall!(destination, "android/graphics/Canvas", "drawBitmap", "(Landroid/graphics/Bitmap;FFLandroid/graphics/Paint;)V", &self.bitmap, 0.0f32, 0.0f32, jx::NULL)?;
        Ok(())
    }
    /// `private static boolean sameBase(Map a, Map b)`.
    fn same_base(a: &Map, b: &Map) -> bool {
        if a.get("kind").and_then(Value::as_str) == Some("object") || b.get("kind").and_then(Value::as_str) == Some("object") {
            return a == b;
        }
        for key in ["color", "width", "page", "layer", "brush", "material", "points"] {
            if !json::objects_equal(a.get(key), b.get(key)) {
                return false;
            }
        }
        true
    }
    /// `private static boolean sameInk(Map a, Map b)` (`a == b` is `Arc::ptr_eq`).
    fn same_ink(a: &Entity, b: &Entity) -> bool {
        Arc::ptr_eq(a, b) || (InkCache::same_base(a, b) && json::objects_equal(a.get("erasures"), b.get("erasures")))
    }
    /// `private RectF bounds(Map stroke)`.
    fn bounds(&mut self, stroke: &Entity) -> AppResult<RectF> {
        if let Some((_, cached)) = self.paint_bounds.get(&ptr(stroke)) {
            return Ok(*cached);
        }
        let result = InkCache::compute_bounds(stroke)?;
        self.paint_bounds.insert(ptr(stroke), (stroke.clone(), result));
        Ok(result)
    }
    /// `private static RectF computeBounds(Map stroke)`.
    fn compute_bounds(stroke: &Map) -> AppResult<RectF> {
        if stroke.get("kind").and_then(Value::as_str) == Some("object") {
            let b = DrawItem::from_map(&Model::new(), stroke.clone())?.paint_bounds(&notemaker_cli::item_coverage::FACTORY)?;
            return Ok(RectF { left: b.left as f32 - 1.0, top: b.top as f32 - 1.0, right: b.right as f32 + 1.0, bottom: b.bottom as f32 + 1.0 });
        }
        let width = float_value(stroke.get("width"))?;
        // Pen ribbons and round caps stay within half the nominal width. Using
        // the broad textured-brush margin falsely links nearby rows of handwriting.
        let pen = match stroke.get("brush") {
            None => true,
            Some(v) => v.as_str() == Some("pen"),
        };
        let pad = if pen { fmax(0.3, width / 2.0) + 1.0 } else { width * 2.0 + 1.0 };
        let (mut l, mut t, mut r, mut b) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for raw in json::list(stroke.get("points").unwrap_or(&Value::Null))? {
            let p = json::list(raw)?;
            let x = float_value(p.first())?;
            let y = float_value(p.get(1))?;
            l = fmin(l, x);
            t = fmin(t, y);
            r = fmax(r, x);
            b = fmax(b, y);
        }
        Ok(RectF { left: l - pad, top: t - pad, right: r + pad, bottom: b + pad })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notemaker_core::json::parse;

    fn map(text: &str) -> Map {
        parse(text).unwrap().as_object().unwrap().clone()
    }

    #[test]
    fn stroke_bounds() {
        let b = InkCache::compute_bounds(&map(r#"{"kind":"stroke","width":4,"points":[[10,20],[30,5.5]]}"#)).unwrap();
        assert_eq!(b, RectF { left: 7.0, top: 2.5, right: 33.0, bottom: 23.0 });
        let b = InkCache::compute_bounds(&map(r#"{"kind":"stroke","width":4,"brush":"marker","points":[[10,20]]}"#)).unwrap();
        assert_eq!(b, RectF { left: 1.0, top: 11.0, right: 19.0, bottom: 29.0 });
    }

    #[test]
    fn same_ink() {
        let a: Entity = Arc::new(map(r#"{"kind":"stroke","color":1,"width":2,"points":[[1,2]],"seq":4}"#));
        let b: Entity = Arc::new(map(r#"{"kind":"stroke","color":1,"width":2,"points":[[1,2]],"seq":5}"#));
        let c: Entity = Arc::new(map(r#"{"kind":"stroke","color":1,"width":2,"points":[[1,2]],"erasures":[]}"#));
        assert!(InkCache::same_ink(&a, &a));
        assert!(InkCache::same_ink(&a, &b));
        assert!(!InkCache::same_ink(&a, &c));
    }
}
