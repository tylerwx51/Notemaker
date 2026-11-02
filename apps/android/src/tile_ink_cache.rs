//! Port of `TileInkCache.java`: bounded document-space cache for settled
//! layer tiles (UI thread only; owned by `CanvasView.tile_ink` in a `Field`,
//! so instance methods take `&mut self`).
//!
//! Java `static long` counters are atomics; the `static synchronized` bound
//! cache (an `IdentityHashMap` keyed by item map identity) is a `Mutex` keyed
//! by `Arc::as_ptr` that keeps the `Entity` alive like Java's strong key.
//! Access-ordered `LinkedHashMap`s (`tiles`, `strokePaths`) are `IndexMap`s:
//! a hit moves the entry to the end (`move_index`).
//!
//! Java's Android `Png.StrokeCommands` held `android.graphics.Path` objects
//! built once at compile time and replayed them with the cache's single
//! `strokePaint`. The port keeps that: each [`PathEntry`] carries the
//! compiled `StrokeCommands` plus their Java `Path`s ([`JavaCommand`]), and
//! [`TileInkCache::play`] replays them with one cached Java `Paint`, so a
//! cached stroke costs no Java object creation per frame.

use crate::android_canvas::{apply_paint, argb_8888, path_to_java};
use crate::android_consts::{COLOR_TRANSPARENT, PAINT_ANTI_ALIAS_FLAG, PAINT_DITHER_FLAG, PAINT_FILTER_BITMAP_FLAG};
use crate::error::AppResult;
use crate::ink_cache::{float_value, fmax, fmin};
use crate::jx::{self, JResult, Obj, Ret};
use indexmap::{IndexMap, IndexSet};
use notemaker_cli::canvas::{Cap, Join, Paint, RectF, Style};
use notemaker_cli::png::{Png, StrokeCommand, StrokeCommands};
use notemaker_core::draw_item::DrawItem;
use notemaker_core::json::{self, Map, Value};
use notemaker_core::jmath;
use notemaker_core::model::{Entity, Model};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub const GUTTER: i32 = 2;
pub const MAX_BYTES: i64 = 96 * 1024 * 1024;
pub const MAX_PICTURE_BYTES: i64 = 24 * 1024 * 1024;
pub const MAX_PICTURE_POINTS: i32 = 100000;
const MAX_PICTURES: i32 = 512;

pub static TILE_RENDERS: AtomicI64 = AtomicI64::new(0);
pub static REUSED_TILE_REPAINTS: AtomicI64 = AtomicI64::new(0);
pub static ITEM_RENDERS: AtomicI64 = AtomicI64::new(0);
pub static GEOMETRY_RENDERS: AtomicI64 = AtomicI64::new(0);
pub static PICTURE_REUSES: AtomicI64 = AtomicI64::new(0);
pub static TRANSIENT_PICTURE_REUSES: AtomicI64 = AtomicI64::new(0);
pub static PICTURE_MISSES: AtomicI64 = AtomicI64::new(0);
pub static PICTURE_EVICTIONS: AtomicI64 = AtomicI64::new(0);
pub static CULLED_ITEMS: AtomicI64 = AtomicI64::new(0);
pub static ALLOCATED_TILES: AtomicI64 = AtomicI64::new(0);
pub static REUSED_TILES: AtomicI64 = AtomicI64::new(0);
pub static EVICTED_TILES: AtomicI64 = AtomicI64::new(0);
pub static TRACE_FIRST_DRAW: AtomicBool = AtomicBool::new(false);
pub static TRACED_COMPILE_NS: AtomicI64 = AtomicI64::new(0);
pub static TRACED_PLAY_NS: AtomicI64 = AtomicI64::new(0);
pub static TRACED_OTHER_NS: AtomicI64 = AtomicI64::new(0);
pub static TRACED_OTHER_MAX_NS: AtomicI64 = AtomicI64::new(0);
pub static TRACED_OTHER_MAX_SHAPE: Mutex<Option<String>> = Mutex::new(None);
pub static TRACED_COMPILES: AtomicI32 = AtomicI32::new(0);
pub static TRACED_PLAYS: AtomicI32 = AtomicI32::new(0);
pub static TRACED_OTHER_RENDERS: AtomicI32 = AtomicI32::new(0);

fn inc(counter: &AtomicI64) {
    counter.fetch_add(1, Ordering::Relaxed);
}
fn get(counter: &AtomicI64) -> i64 {
    counter.load(Ordering::Relaxed)
}
fn tracing() -> bool {
    TRACE_FIRST_DRAW.load(Ordering::Relaxed)
}
/// Releases the local reference of an object result Java code ignored.
fn discard(ret: Ret) {
    if let Some(o) = ret.obj() {
        let _ = jx::env().delete_local_ref(o);
    }
}
fn ptr(e: &Entity) -> usize {
    Arc::as_ptr(e) as *const () as usize
}
fn is_kind(m: &Map, kind: &str) -> bool {
    m.get("kind").and_then(Value::as_str) == Some(kind)
}
/// `Shader.TileMode.CLAMP`.
fn clamp() -> JResult<Obj> {
    static CLAMP: OnceLock<Obj> = OnceLock::new();
    if let Some(v) = CLAMP.get() {
        return Ok(v.clone());
    }
    let v = jsget!("android/graphics/Shader$TileMode", "CLAMP", "Landroid/graphics/Shader$TileMode;")?.some();
    Ok(CLAMP.get_or_init(|| v).clone())
}
/// `(int) Math.floor(value)` of a float quotient.
fn floor_int(value: f32) -> i32 {
    (value as f64).floor() as i32
}
fn set_values(matrix: &Obj, values: &[f32; 9]) -> AppResult<()> {
    let array = jx::new_float_array(values)?;
    jcall!(matrix, "android/graphics/Matrix", "setValues", "([F)V", &array)?;
    Ok(())
}
fn create_bitmap(width: i32, height: i32) -> AppResult<Obj> {
    Ok(jstatic!("android/graphics/Bitmap", "createBitmap", "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;", width, height, &argb_8888()?)?.some())
}
fn recycle(bitmap: &Obj) -> AppResult<()> {
    jcall!(bitmap, "android/graphics/Bitmap", "recycle", "()V")?;
    Ok(())
}

/// `BiConsumer<Canvas, Map<String,Object>> renderer`: draws one item into a
/// Java `android.graphics.Canvas`.
pub type Renderer<'a> = dyn FnMut(&Obj, &Entity) -> AppResult<()> + 'a;

/// `private static final class BoundRecord`.
pub struct BoundRecord {
    pub bounds: RectF,
    pub point_count: i32,
}
impl BoundRecord {
    pub fn new(bounds: RectF, point_count: i32) -> BoundRecord {
        BoundRecord { bounds, point_count }
    }
}

/// `static final IdentityHashMap BOUNDS` + `static int cachedBoundPoints`
/// (guarded by the class monitor in Java).
#[derive(Default)]
pub struct BoundsCache {
    /// `Arc::as_ptr(item) as usize` → (the item, its record).
    pub entries: HashMap<usize, (Entity, BoundRecord)>,
    pub cached_bound_points: i32,
}
pub static BOUNDS: Mutex<Option<BoundsCache>> = Mutex::new(None);

/// `static final class Item`.
pub struct Item {
    pub id: String,
    pub value: Entity,
    pub bounds: RectF,
    pub cache_picture: bool,
}
impl Item {
    /// `Item(String id, Map value)` (`cachePicture = true`).
    pub fn new(id: &str, value: Entity) -> AppResult<Item> {
        Item::with_picture(id, value, true)
    }
    /// `Item(String id, Map value, boolean cachePicture)`.
    pub fn with_picture(id: &str, value: Entity, cache_picture: bool) -> AppResult<Item> {
        let bounds = if cache_picture { TileInkCache::bounds(&value)? } else { TileInkCache::compute_bounds(&value)? };
        Ok(Item { id: id.to_string(), value, bounds, cache_picture })
    }
}

/// `private static final class Key`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub layer: String,
    pub page: i32,
    pub x: i32,
    pub y: i32,
    pub tier: i32,
    pub culling: bool,
}
impl Key {
    /// `Key(int page, String layer, int x, int y, int tier, boolean culling)`.
    pub fn new(page: i32, layer: &str, x: i32, y: i32, tier: i32, culling: bool) -> Key {
        Key { layer: layer.to_string(), page, x, y, tier, culling }
    }
}

/// `private static final class Tile`.
pub struct Tile {
    pub bitmap: Obj,
    pub shader: Obj,
    pub bitmap_matrix: Obj,
    pub items: Vec<Arc<Item>>,
    pub bytes: i64,
}
impl Tile {
    /// `Tile(Bitmap bitmap, List<Item> items, int tx, int ty, int tier, int tileSize)`.
    pub fn new(bitmap: Obj, items: Vec<Arc<Item>>, tx: i32, ty: i32, tier: i32, tile_size: i32) -> AppResult<Tile> {
        let row_bytes = jcall!(bitmap, "android/graphics/Bitmap", "getRowBytes", "()I")?.int();
        let height = jcall!(bitmap, "android/graphics/Bitmap", "getHeight", "()I")?.int();
        let bytes = row_bytes as i64 * height as i64;
        let shader = TileInkCache::shader(&bitmap, (tx * tile_size) as f32, (ty * tile_size) as f32, tier)?;
        let bitmap_matrix = jnew!("android/graphics/Matrix", "()V")?;
        let inv = 1.0f32 / tier as f32;
        set_values(&bitmap_matrix, &[inv, 0.0, (tx * tile_size) as f32 - GUTTER as f32 * inv, 0.0, inv, (ty * tile_size) as f32 - GUTTER as f32 * inv, 0.0, 0.0, 1.0])?;
        Ok(Tile { bitmap, shader, bitmap_matrix, items, bytes })
    }
}

/// One recorded stroke call with its Java `Path`s, built once per
/// [`PathEntry`] (the Java `Png.StrokeCommands` lambdas captured their
/// `android.graphics.Path` objects the same way).
#[derive(Clone)]
pub enum JavaCommand {
    SaveClipOut(Vec<Obj>),
    Restore,
    FillPath { path: Obj, color: i32 },
    StrokePath { path: Obj, color: i32, width: f32 },
    Line { x0: f32, y0: f32, x1: f32, y1: f32, color: i32, width: f32 },
}

/// `com.notemaker.cli.Png.compileStroke(item)` including the Java paths.
fn compile_stroke(item: &Map) -> AppResult<(StrokeCommands, Arc<[JavaCommand]>)> {
    let commands = Png::compile_stroke(item)?;
    let mut java = Vec::with_capacity(commands.commands().len());
    for command in commands.commands() {
        java.push(match command {
            StrokeCommand::SaveClipOut(paths) => JavaCommand::SaveClipOut(paths.iter().map(path_to_java).collect::<JResult<Vec<Obj>>>()?),
            StrokeCommand::Restore => JavaCommand::Restore,
            StrokeCommand::FillPath { path, color } => JavaCommand::FillPath { path: path_to_java(path)?, color: *color },
            StrokeCommand::StrokePath { path, color, width } => JavaCommand::StrokePath { path: path_to_java(path)?, color: *color, width: *width },
            StrokeCommand::Line { x0, y0, x1, y1, color, width } => JavaCommand::Line { x0: *x0, y0: *y0, x1: *x1, y1: *y1, color: *color, width: *width },
        });
    }
    Ok((commands, java.into()))
}

/// `private static final class PathEntry`.
#[derive(Clone)]
pub struct PathEntry {
    pub commands: StrokeCommands,
    /// The Java `Path`s of `commands`, replayed by [`TileInkCache::play`].
    pub java: Arc<[JavaCommand]>,
    pub source: Entity,
    pub bytes: i64,
    pub points: i64,
}

/// `final class TileInkCache`.
pub struct TileInkCache {
    pub current_bytes: i64,
    pub peak_bytes: i64,
    pub cache_clears: i64,
    pub picture_bytes: i64,
    pub picture_points: i64,
    pub rendered_item_ids: IndexSet<String>,
    pub geometry_item_ids: IndexSet<String>,
    /// Access-ordered `LinkedHashMap<Key,Tile>(32,.75f,true)`.
    pub tiles: IndexMap<Key, Tile>,
    /// Access-ordered `LinkedHashMap<String,PathEntry>(64,.75f,true)`.
    pub stroke_paths: IndexMap<String, PathEntry>,
    pub transient_stroke_paths: IndexMap<String, PathEntry>,
    /// `new Paint(Paint.FILTER_BITMAP_FLAG | Paint.DITHER_FLAG)` (Java object).
    pub bitmap_paint: Obj,
    /// `strokePaint()`: anti-aliased, round cap; the attributes currently set
    /// on [`Self::stroke_paint_java`] (replay mutates both, as Java mutated
    /// its one `Paint`).
    pub stroke_paint: Paint,
    /// The Java `Paint` of `strokePaint()`.
    pub stroke_paint_java: Obj,
}

impl TileInkCache {
    /// `TileInkCache()`.
    pub fn new() -> AppResult<TileInkCache> {
        let bitmap_paint = jnew!("android/graphics/Paint", "(I)V", PAINT_FILTER_BITMAP_FLAG | PAINT_DITHER_FLAG)?;
        let stroke_paint_java = jnew!("android/graphics/Paint", "(I)V", PAINT_ANTI_ALIAS_FLAG)?;
        jcall!(stroke_paint_java, "android/graphics/Paint", "setStrokeCap", "(Landroid/graphics/Paint$Cap;)V", &crate::android_canvas::cap_obj(Cap::Round)?)?;
        Ok(TileInkCache {
            current_bytes: 0,
            peak_bytes: 0,
            cache_clears: 0,
            picture_bytes: 0,
            picture_points: 0,
            rendered_item_ids: IndexSet::new(),
            geometry_item_ids: IndexSet::new(),
            tiles: IndexMap::new(),
            stroke_paths: IndexMap::new(),
            transient_stroke_paths: IndexMap::new(),
            bitmap_paint,
            stroke_paint: TileInkCache::new_stroke_paint(),
            stroke_paint_java,
        })
    }
    /// `private static Paint strokePaint()`.
    pub fn new_stroke_paint() -> Paint {
        Paint { cap: Cap::Round, ..Paint::anti_alias() }
    }
    /// `static int tileSize(int tier)`.
    pub fn tile_size(tier: i32) -> i32 {
        if tier >= 8 {
            64
        } else if tier >= 4 {
            128
        } else {
            256
        }
    }
    /// `void beginFrame()`.
    pub fn begin_frame(&mut self) {
        self.rendered_item_ids.clear();
        self.geometry_item_ids.clear();
        self.transient_stroke_paths.clear();
    }
    /// `private void recordRendered(String id)`.
    pub fn record_rendered(&mut self, id: &str) {
        if !self.rendered_item_ids.contains(id) {
            self.rendered_item_ids.insert(id.to_string());
        }
    }
    /// `private void recordGeometry(String id)`.
    pub fn record_geometry(&mut self, id: &str) {
        if !self.geometry_item_ids.contains(id) {
            self.geometry_item_ids.insert(id.to_string());
        }
    }
    /// `static int densityTier(float effectiveScale)`.
    pub fn density_tier(effective_scale: f32) -> i32 {
        if !(effective_scale > 0.0) {
            return 1;
        }
        let mut tier = 1;
        while tier < 8 && effective_scale > tier as f32 * 1.414_213_6f32 {
            tier *= 2;
        }
        while tier > 1 && effective_scale < tier as f32 / 1.414_213_6f32 {
            tier /= 2;
        }
        tier.max(1)
    }
    /// LinkedHashMap access order: `tiles.get(key)` moves the entry last.
    fn touch_tile(&mut self, key: &Key) -> bool {
        match self.tiles.get_index_of(key) {
            Some(i) => {
                let last = self.tiles.len() - 1;
                self.tiles.move_index(i, last);
                true
            }
            None => false,
        }
    }
    /// `void draw(Canvas destination, int page, String layer, List<Item> items, RectF viewport, int tier, boolean cullItems, BiConsumer renderer)`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, destination: &Obj, page: i32, layer: &str, items: &[Arc<Item>], viewport: &RectF, tier: i32, cull_items: bool, renderer: &mut Renderer<'_>) -> AppResult<()> {
        let mut reference: Option<(Obj, Obj)> = None;
        let result = self.draw_tiles(destination, page, layer, items, viewport, tier, cull_items, renderer, &mut reference);
        // finally { if (referenceBitmap != null) referenceBitmap.recycle(); }
        if let Some((bitmap, _)) = &reference {
            let recycled = recycle(bitmap);
            result?;
            return recycled;
        }
        result
    }
    #[allow(clippy::too_many_arguments)]
    fn draw_tiles(&mut self, destination: &Obj, page: i32, layer: &str, items: &[Arc<Item>], viewport: &RectF, tier: i32, cull_items: bool, renderer: &mut Renderer<'_>, reference: &mut Option<(Obj, Obj)>) -> AppResult<()> {
        let tile_size = TileInkCache::tile_size(tier);
        let ts = tile_size as f32;
        let first_x = floor_int(viewport.left / ts);
        let last_x = floor_int(viewport.right.next_down() / ts);
        let first_y = floor_int(viewport.top / ts);
        let last_y = floor_int(viewport.bottom.next_down() / ts);
        let mut ty = first_y;
        while ty <= last_y {
            let mut tx = first_x;
            while tx <= last_x {
                self.draw_one_tile(destination, page, layer, items, tier, cull_items, renderer, reference, tile_size, tx, ty)?;
                if tx == i32::MAX {
                    break;
                }
                tx += 1;
            }
            if ty == i32::MAX {
                break;
            }
            ty += 1;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn draw_one_tile(&mut self, destination: &Obj, page: i32, layer: &str, items: &[Arc<Item>], tier: i32, cull_items: bool, renderer: &mut Renderer<'_>, reference: &mut Option<(Obj, Obj)>, tile_size: i32, tx: i32, ty: i32) -> AppResult<()> {
        let left = (tx * tile_size) as f32;
        let top = (ty * tile_size) as f32;
        let gutter = GUTTER as f32 / tier as f32;
        let core = RectF::new(left - gutter, top - gutter, left + tile_size as f32 + gutter, top + tile_size as f32 + gutter);
        let contributors: Vec<Arc<Item>> = if cull_items { items.iter().filter(|item| RectF::intersects(&core, &item.bounds)).cloned().collect() } else { items.to_vec() };
        let key = Key::new(page, layer, tx, ty, tier, true);
        let has_tile = cull_items && self.touch_tile(&key);
        if contributors.is_empty() {
            if has_tile {
                self.remove(&key)?;
            }
            return Ok(());
        }
        if has_tile && TileInkCache::same(&self.tiles[&key].items, &contributors) {
            inc(&REUSED_TILES);
            self.draw_tile_bitmap(destination, &self.tiles[&key], tx, ty, tile_size)?;
            return Ok(());
        }
        // Full tile repaint preserves Skia's exact anti-aliasing at changed
        // item edges. Reuse a resident bitmap to avoid graphics allocation.
        if cull_items {
            for item in items {
                if !RectF::intersects(&core, &item.bounds) {
                    inc(&CULLED_ITEMS);
                }
            }
        }
        let size = tile_size * tier + 2 * GUTTER;
        let canvas;
        let mut bitmap = None;
        if cull_items {
            let b = if has_tile {
                let b = self.tiles[&key].bitmap.clone();
                jcall!(b, "android/graphics/Bitmap", "eraseColor", "(I)V", COLOR_TRANSPARENT)?;
                inc(&REUSED_TILE_REPAINTS);
                b
            } else {
                self.trim_for(size as i64 * size as i64 * 4)?;
                create_bitmap(size, size)?
            };
            canvas = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", &b)?;
            bitmap = Some(b);
        } else {
            match reference {
                None => {
                    let b = create_bitmap(size, size)?;
                    let c = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", &b)?;
                    *reference = Some((b, c));
                }
                Some((b, _)) => {
                    jcall!(*b, "android/graphics/Bitmap", "eraseColor", "(I)V", COLOR_TRANSPARENT)?;
                }
            }
            let (_, c) = reference.as_ref().unwrap();
            canvas = c.clone();
            let identity = jnew!("android/graphics/Matrix", "()V")?;
            jcall!(canvas, "android/graphics/Canvas", "setMatrix", "(Landroid/graphics/Matrix;)V", &identity)?;
        }
        jcall!(canvas, "android/graphics/Canvas", "scale", "(FF)V", tier as f32, tier as f32)?;
        jcall!(canvas, "android/graphics/Canvas", "translate", "(FF)V", -left + gutter, -top + gutter)?;
        if cull_items {
            jcall!(canvas, "android/graphics/Canvas", "clipRect", "(FFFF)Z", left - gutter, top - gutter, left + tile_size as f32 + gutter, top + tile_size as f32 + gutter)?;
        }
        for item in &contributors {
            if cull_items {
                inc(&ITEM_RENDERS);
                self.record_rendered(&item.id);
                self.draw_item(&canvas, item, renderer)?;
            } else {
                renderer(&canvas, &item.value)?;
            }
        }
        if cull_items {
            if !has_tile {
                let tile = Tile::new(bitmap.unwrap(), contributors, tx, ty, tier, tile_size)?;
                let bytes = tile.bytes;
                self.tiles.insert(key.clone(), tile);
                self.current_bytes += bytes;
                inc(&ALLOCATED_TILES);
                self.peak_bytes = self.peak_bytes.max(self.current_bytes);
            } else {
                self.tiles.get_mut(&key).unwrap().items = contributors;
            }
            self.draw_tile_bitmap(destination, &self.tiles[&key], tx, ty, tile_size)?;
            inc(&TILE_RENDERS);
        } else {
            let (b, _) = reference.as_ref().unwrap();
            let shader = TileInkCache::shader(b, left, top, tier)?;
            self.draw_tile(destination, &shader, tx, ty, tile_size)?;
        }
        Ok(())
    }
    /// `void drawMonolithic(Canvas destination, List<Item> items, RectF probe, int tier, BiConsumer renderer)`.
    pub fn draw_monolithic(&mut self, destination: &Obj, items: &[Arc<Item>], probe: &RectF, tier: i32, renderer: &mut Renderer<'_>) -> AppResult<()> {
        let tile_size = TileInkCache::tile_size(tier);
        let ts = tile_size as f32;
        let first_x = floor_int(probe.left / ts);
        let last_x = floor_int(probe.right.next_down() / ts);
        let first_y = floor_int(probe.top / ts);
        let last_y = floor_int(probe.bottom.next_down() / ts);
        let left = (first_x * tile_size) as f32;
        let top = (first_y * tile_size) as f32;
        let right = ((last_x + 1) * tile_size) as f32;
        let bottom = ((last_y + 1) * tile_size) as f32;
        let width = (((right - left) * tier as f32) as f64).ceil() as i32 + 2 * GUTTER;
        let height = (((bottom - top) * tier as f32) as f64).ceil() as i32 + 2 * GUTTER;
        let bitmap = create_bitmap(width, height)?;
        let canvas = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", &bitmap)?;
        let g = GUTTER as f32 / tier as f32;
        jcall!(canvas, "android/graphics/Canvas", "scale", "(FF)V", tier as f32, tier as f32)?;
        jcall!(canvas, "android/graphics/Canvas", "translate", "(FF)V", -left + g, -top + g)?;
        jcall!(canvas, "android/graphics/Canvas", "clipRect", "(FFFF)Z", left - g, top - g, right + g, bottom + g)?;
        for item in items {
            renderer(&canvas, &item.value)?;
        }
        let shader = TileInkCache::shader(&bitmap, left, top, tier)?;
        self.draw_core(destination, &shader, left, top, right, bottom)?;
        recycle(&bitmap)
    }
    /// `private static BitmapShader shader(Bitmap bitmap, float originX, float originY, int tier)`.
    pub fn shader(bitmap: &Obj, origin_x: f32, origin_y: f32, tier: i32) -> AppResult<Obj> {
        let mode = clamp()?;
        let shader = jnew!("android/graphics/BitmapShader", "(Landroid/graphics/Bitmap;Landroid/graphics/Shader$TileMode;Landroid/graphics/Shader$TileMode;)V", bitmap, &mode, &mode)?;
        let local = jnew!("android/graphics/Matrix", "()V")?;
        let inv = 1.0f32 / tier as f32;
        let left = origin_x - GUTTER as f32 * inv;
        let top = origin_y - GUTTER as f32 * inv;
        set_values(&local, &[inv, 0.0, left, 0.0, inv, top, 0.0, 0.0, 1.0])?;
        jcall!(shader, "android/graphics/Shader", "setLocalMatrix", "(Landroid/graphics/Matrix;)V", &local)?;
        Ok(shader)
    }
    /// `private void drawTile(Canvas destination, BitmapShader shader, int tx, int ty, int tileSize)`.
    pub fn draw_tile(&mut self, destination: &Obj, shader: &Obj, tx: i32, ty: i32, tile_size: i32) -> AppResult<()> {
        let x = (tx * tile_size) as f32;
        let y = (ty * tile_size) as f32;
        self.draw_core(destination, shader, x, y, x + tile_size as f32, y + tile_size as f32)
    }
    /// `private void drawTileBitmap(Canvas destination, Tile tile, int tx, int ty, int tileSize)`.
    pub fn draw_tile_bitmap(&self, destination: &Obj, tile: &Tile, tx: i32, ty: i32, tile_size: i32) -> AppResult<()> {
        let x = (tx * tile_size) as f32;
        let y = (ty * tile_size) as f32;
        jcall!(destination, "android/graphics/Canvas", "save", "()I")?;
        jcall!(destination, "android/graphics/Canvas", "clipRect", "(FFFF)Z", x, y, x + tile_size as f32, y + tile_size as f32)?;
        discard(jcall!(self.bitmap_paint, "android/graphics/Paint", "setShader", "(Landroid/graphics/Shader;)Landroid/graphics/Shader;", jx::NULL)?);
        jcall!(self.bitmap_paint, "android/graphics/Paint", "setAntiAlias", "(Z)V", false)?;
        jcall!(destination, "android/graphics/Canvas", "drawBitmap", "(Landroid/graphics/Bitmap;Landroid/graphics/Matrix;Landroid/graphics/Paint;)V", &tile.bitmap, &tile.bitmap_matrix, &self.bitmap_paint)?;
        jcall!(destination, "android/graphics/Canvas", "restore", "()V")?;
        Ok(())
    }
    /// `private void drawCore(Canvas destination, BitmapShader shader, float left, float top, float right, float bottom)`.
    pub fn draw_core(&self, destination: &Obj, shader: &Obj, left: f32, top: f32, right: f32, bottom: f32) -> AppResult<()> {
        discard(jcall!(self.bitmap_paint, "android/graphics/Paint", "setShader", "(Landroid/graphics/Shader;)Landroid/graphics/Shader;", shader)?);
        jcall!(self.bitmap_paint, "android/graphics/Paint", "setAntiAlias", "(Z)V", false)?;
        jcall!(destination, "android/graphics/Canvas", "drawRect", "(FFFFLandroid/graphics/Paint;)V", left, top, right, bottom, &self.bitmap_paint)?;
        discard(jcall!(self.bitmap_paint, "android/graphics/Paint", "setShader", "(Landroid/graphics/Shader;)Landroid/graphics/Shader;", jx::NULL)?);
        Ok(())
    }
    /// `private static boolean same(List<Item> a, List<Item> b)`.
    pub fn same(a: &[Arc<Item>], b: &[Arc<Item>]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        for (x, y) in a.iter().zip(b) {
            if !Arc::ptr_eq(&x.value, &y.value) && *x.value != *y.value {
                return false;
            }
        }
        true
    }
    /// `private void trimFor(long incoming)`.
    pub fn trim_for(&mut self, incoming: i64) -> AppResult<()> {
        while self.current_bytes + incoming > MAX_BYTES && !self.tiles.is_empty() {
            let (_, tile) = self.tiles.shift_remove_index(0).unwrap();
            self.current_bytes -= tile.bytes;
            recycle(&tile.bitmap)?;
            inc(&EVICTED_TILES);
        }
        Ok(())
    }
    /// `private void remove(Key key, Tile tile)`.
    pub fn remove(&mut self, key: &Key) -> AppResult<()> {
        if let Some(tile) = self.tiles.shift_remove(key) {
            self.current_bytes -= tile.bytes;
            recycle(&tile.bitmap)?;
        }
        Ok(())
    }
    /// `private void drawItem(Canvas canvas, Item item, BiConsumer renderer)`.
    pub fn draw_item(&mut self, canvas: &Obj, item: &Item, renderer: &mut Renderer<'_>) -> AppResult<()> {
        if !is_kind(&item.value, "stroke") {
            inc(&GEOMETRY_RENDERS);
            self.record_geometry(&item.id);
            let started = if tracing() { jx::nano_time() } else { 0 };
            renderer(canvas, &item.value)?;
            if tracing() {
                let elapsed = jx::nano_time() - started;
                TRACED_OTHER_NS.fetch_add(elapsed, Ordering::Relaxed);
                TRACED_OTHER_RENDERS.fetch_add(1, Ordering::Relaxed);
                if elapsed > TRACED_OTHER_MAX_NS.load(Ordering::Relaxed) {
                    TRACED_OTHER_MAX_NS.store(elapsed, Ordering::Relaxed);
                    *TRACED_OTHER_MAX_SHAPE.lock().unwrap_or_else(|e| e.into_inner()) = Some(json::java_string(item.value.get("shape")));
                }
            }
            return Ok(());
        }
        let entry = if item.cache_picture {
            match self.stroke_paths.get_index_of(&item.id) {
                Some(i) => {
                    let last = self.stroke_paths.len() - 1;
                    self.stroke_paths.move_index(i, last);
                    Some(self.stroke_paths[last].clone())
                }
                None => None,
            }
        } else {
            self.transient_stroke_paths.get(&item.id).cloned()
        };
        if let Some(entry) = &entry {
            if Arc::ptr_eq(&entry.source, &item.value) || *entry.source == *item.value {
                if item.cache_picture {
                    inc(&PICTURE_REUSES);
                } else {
                    inc(&TRANSIENT_PICTURE_REUSES);
                }
                return self.play(canvas, entry);
            }
        }
        if item.cache_picture {
            if entry.is_some() {
                self.remove_path(&item.id);
            }
            inc(&PICTURE_MISSES);
        } else {
            self.transient_stroke_paths.shift_remove(&item.id);
        }
        let points = TileInkCache::path_point_estimate(&item.value)?;
        let cost = 4096 + points * 256;
        let retain = item.cache_picture && points > 0 && points <= 20000 && cost <= MAX_PICTURE_BYTES;
        if retain {
            self.trim_paths(cost, points);
        }
        inc(&GEOMETRY_RENDERS);
        self.record_geometry(&item.id);
        let started = if tracing() { jx::nano_time() } else { 0 };
        let (commands, java) = compile_stroke(&item.value)?;
        let recorded = PathEntry { commands, java, source: item.value.clone(), bytes: cost, points };
        if tracing() {
            TRACED_COMPILE_NS.fetch_add(jx::nano_time() - started, Ordering::Relaxed);
            TRACED_COMPILES.fetch_add(1, Ordering::Relaxed);
        }
        if retain && self.picture_bytes + cost <= MAX_PICTURE_BYTES && self.picture_points + points <= MAX_PICTURE_POINTS as i64 && (self.stroke_paths.len() as i32) < MAX_PICTURES {
            self.stroke_paths.insert(item.id.clone(), recorded.clone());
            self.picture_bytes += cost;
            self.picture_points += points;
        } else if !item.cache_picture {
            self.transient_stroke_paths.insert(item.id.clone(), recorded.clone());
        }
        self.play(canvas, &recorded)
    }
    /// The `Paint` mutation of one replayed command (see `Png::replay_stroke`),
    /// applied to the cached Java paint.
    fn stroke_paint_for(&mut self, style: Style, color: i32, width: Option<f32>, round_join: bool) -> AppResult<Obj> {
        let mut next = self.stroke_paint;
        next.color = color;
        next.style = style;
        if let Some(width) = width {
            next.stroke_width = width;
        }
        if round_join {
            next.join = Join::Round;
        }
        if next != self.stroke_paint {
            apply_paint(&self.stroke_paint_java, &next, Some(&self.stroke_paint))?;
            self.stroke_paint = next;
        }
        Ok(self.stroke_paint_java.clone())
    }
    /// `private void play(Canvas canvas, PathEntry entry)`:
    /// `Png.replayStroke(canvas, entry.commands, strokePaint)` over the
    /// entry's Java paths.
    pub fn play(&mut self, canvas: &Obj, entry: &PathEntry) -> AppResult<()> {
        let started = if tracing() { jx::nano_time() } else { 0 };
        for command in entry.java.iter() {
            match command {
                JavaCommand::SaveClipOut(paths) => {
                    jcall!(canvas, "android/graphics/Canvas", "save", "()I")?;
                    for path in paths {
                        jcall!(canvas, "android/graphics/Canvas", "clipOutPath", "(Landroid/graphics/Path;)Z", path)?;
                    }
                }
                JavaCommand::Restore => {
                    jcall!(canvas, "android/graphics/Canvas", "restore", "()V")?;
                }
                JavaCommand::FillPath { path, color } => {
                    let paint = self.stroke_paint_for(Style::Fill, *color, None, false)?;
                    jcall!(canvas, "android/graphics/Canvas", "drawPath", "(Landroid/graphics/Path;Landroid/graphics/Paint;)V", path, &paint)?;
                }
                JavaCommand::StrokePath { path, color, width } => {
                    let paint = self.stroke_paint_for(Style::Stroke, *color, Some(*width), true)?;
                    jcall!(canvas, "android/graphics/Canvas", "drawPath", "(Landroid/graphics/Path;Landroid/graphics/Paint;)V", path, &paint)?;
                }
                JavaCommand::Line { x0, y0, x1, y1, color, width } => {
                    let paint = self.stroke_paint_for(Style::Stroke, *color, Some(*width), false)?;
                    jcall!(canvas, "android/graphics/Canvas", "drawLine", "(FFFFLandroid/graphics/Paint;)V", *x0, *y0, *x1, *y1, &paint)?;
                }
            }
        }
        if tracing() {
            TRACED_PLAY_NS.fetch_add(jx::nano_time() - started, Ordering::Relaxed);
            TRACED_PLAYS.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
    /// `private void trimPaths(long incoming, long points)`.
    pub fn trim_paths(&mut self, incoming: i64, points: i64) {
        while (self.picture_bytes + incoming > MAX_PICTURE_BYTES || self.picture_points + points > MAX_PICTURE_POINTS as i64 || self.stroke_paths.len() as i32 >= MAX_PICTURES) && !self.stroke_paths.is_empty() {
            let (_, e) = self.stroke_paths.shift_remove_index(0).unwrap();
            self.picture_bytes -= e.bytes;
            self.picture_points -= e.points;
            inc(&PICTURE_EVICTIONS);
        }
    }
    /// `private void removePath(String id, PathEntry entry)`.
    pub fn remove_path(&mut self, id: &str) {
        if let Some(entry) = self.stroke_paths.shift_remove(id) {
            self.picture_bytes -= entry.bytes;
            self.picture_points -= entry.points;
        }
    }
    /// `private static long pathPointEstimate(Map<String,Object> item)`.
    pub fn path_point_estimate(item: &Map) -> AppResult<i64> {
        let empty = Value::Array(Vec::new());
        let raw = json::list(item.get("points").unwrap_or(&empty))?;
        if raw.is_empty() {
            return Ok(0);
        }
        let mut length = 0.0f64;
        let (mut px, mut py) = (0.0f64, 0.0f64);
        for (i, point) in raw.iter().enumerate() {
            let p = json::list(point)?;
            let x = json::as_double(p.first())?;
            let y = json::as_double(p.get(1))?;
            if i > 0 {
                length += jmath::hypot(x - px, y - py);
            }
            px = x;
            py = y;
        }
        let width = json::as_double(item.get("width"))?;
        let brush = match item.get("brush") {
            None => "pen".to_string(),
            v => json::java_string(v),
        };
        let factor = match brush.as_str() {
            "marker" => 1.8,
            "neoBrush" => 1.4,
            "brush" | "charcoal" => 1.35,
            "pencil" | "oily" => 0.82,
            "latinCalligraphy" | "asiaCalligraphy" => 1.5,
            _ => 1.0,
        };
        let step = jmath::max(0.75, jmath::min(2.0, width * factor * 0.2));
        let mut estimate = 1i64.max(8192i64.min((length / step).ceil() as i64 + 1));
        for erase in json::list(item.get("erasures").unwrap_or(&empty))? {
            estimate += json::list(erase)?.len() as i64;
        }
        Ok(estimate)
    }
    /// `void clear()`.
    pub fn clear(&mut self) -> AppResult<()> {
        self.cache_clears += 1;
        for tile in self.tiles.values() {
            recycle(&tile.bitmap)?;
        }
        self.current_bytes = 0;
        self.tiles.clear();
        self.stroke_paths.clear();
        self.transient_stroke_paths.clear();
        self.picture_bytes = 0;
        self.picture_points = 0;
        self.rendered_item_ids.clear();
        self.geometry_item_ids.clear();
        Ok(())
    }
    /// `long cacheClears()`.
    pub fn cache_clears(&self) -> i64 {
        self.cache_clears
    }
    /// `static synchronized Map<String,Object> boundsCacheMetrics()`.
    pub fn bounds_cache_metrics() -> Map {
        let guard = BOUNDS.lock().unwrap_or_else(|e| e.into_inner());
        let (entries, points) = guard.as_ref().map(|b| (b.entries.len() as i64, b.cached_bound_points as i64)).unwrap_or((0, 0));
        notemaker_core::object! { "entries" => entries, "points" => points }
    }
    /// `int residentPageCount()`.
    pub fn resident_page_count(&self) -> i32 {
        self.tiles.keys().map(|k| k.page).collect::<HashSet<i32>>().len() as i32
    }
    /// `Map<String,Object> metrics()`.
    pub fn metrics(&self) -> Map {
        let rendered: Vec<Value> = self.rendered_item_ids.iter().map(|s| Value::from(s.as_str())).collect();
        let geometry: Vec<Value> = self.geometry_item_ids.iter().map(|s| Value::from(s.as_str())).collect();
        notemaker_core::object! {
            "tileRenders" => get(&TILE_RENDERS),
            "itemRenders" => get(&ITEM_RENDERS),
            "geometryRenders" => get(&GEOMETRY_RENDERS),
            "pathCommandReuses" => get(&PICTURE_REUSES),
            "transientPathCommandReuses" => get(&TRANSIENT_PICTURE_REUSES),
            "pathCommandMisses" => get(&PICTURE_MISSES),
            "pathCommandEvictions" => get(&PICTURE_EVICTIONS),
            "pathCommandBytesEstimate" => self.picture_bytes,
            "pathCommandPointsEstimate" => self.picture_points,
            "maxPathCommandBytesEstimate" => MAX_PICTURE_BYTES,
            "maxPathCommandPointsEstimate" => MAX_PICTURE_POINTS as i64,
            "pictureReuses" => get(&PICTURE_REUSES),
            "transientPictureReuses" => get(&TRANSIENT_PICTURE_REUSES),
            "pictureMisses" => get(&PICTURE_MISSES),
            "pictureEvictions" => get(&PICTURE_EVICTIONS),
            "pictureBytes" => self.picture_bytes,
            "picturePoints" => self.picture_points,
            "maxPictureBytes" => MAX_PICTURE_BYTES,
            "maxPicturePoints" => MAX_PICTURE_POINTS as i64,
            "culledItems" => get(&CULLED_ITEMS),
            "allocatedTiles" => get(&ALLOCATED_TILES),
            "reusedTiles" => get(&REUSED_TILES),
            "evictedTiles" => get(&EVICTED_TILES),
            "currentBytes" => self.current_bytes,
            "peakBytes" => self.peak_bytes,
            "maxBytes" => MAX_BYTES,
            "densityTier" => 0i64,
            "lastRenderedItemIds" => rendered.clone(),
            "renderedItemIds" => rendered,
            "lastGeometryItemIds" => geometry,
        }
    }
    /// `private static synchronized RectF bounds(Map<String,Object> item)`.
    pub fn bounds(item: &Entity) -> AppResult<RectF> {
        let mut guard = BOUNDS.lock().unwrap_or_else(|e| e.into_inner());
        if is_kind(item, "object") {
            return TileInkCache::compute_bounds(item);
        }
        let cache = guard.get_or_insert_with(BoundsCache::default);
        if let Some((_, cached)) = cache.entries.get(&ptr(item)) {
            return Ok(cached.bounds);
        }
        let result = TileInkCache::compute_bounds(item)?;
        let point_count = match item.get("points") {
            Some(Value::Array(points)) => points.len() as i32,
            _ => 0,
        };
        if point_count <= 10000 {
            while (cache.cached_bound_points + point_count > 100000 || cache.entries.len() >= 4096) && !cache.entries.is_empty() {
                let first = *cache.entries.keys().next().unwrap();
                if let Some((_, record)) = cache.entries.remove(&first) {
                    cache.cached_bound_points -= record.point_count;
                }
            }
            cache.entries.insert(ptr(item), (item.clone(), BoundRecord::new(result, point_count)));
            cache.cached_bound_points += point_count;
        }
        Ok(result)
    }
    /// `private static RectF computeBounds(Map<String,Object> item)`.
    pub fn compute_bounds(item: &Map) -> AppResult<RectF> {
        if is_kind(item, "object") {
            let b = DrawItem::from_map(&Model::new(), item.clone())?.paint_bounds(&notemaker_cli::item_coverage::FACTORY)?;
            return Ok(RectF::new(b[0] as f32 - 2.0, b[1] as f32 - 2.0, b[2] as f32 + 2.0, b[3] as f32 + 2.0));
        }
        let width = float_value(item.get("width"))?;
        let brush = match item.get("brush") {
            None => "pen".to_string(),
            v => json::java_string(v),
        };
        let pad = (if brush == "pen" { fmax(0.3, width / 2.0) } else { fmax(width * 2.0, width / 2.0) }) + 2.0;
        let (mut l, mut t, mut r, mut b) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for raw in json::list(item.get("points").unwrap_or(&Value::Null))? {
            let p = json::list(raw)?;
            let x = float_value(p.first())?;
            let y = float_value(p.get(1))?;
            l = fmin(l, x);
            t = fmin(t, y);
            r = fmax(r, x);
            b = fmax(b, y);
        }
        if l == f32::INFINITY {
            return Ok(RectF::default());
        }
        Ok(RectF::new(l - pad, t - pad, r + pad, b + pad))
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
    fn tiers_and_sizes() {
        assert_eq!(TileInkCache::density_tier(0.0), 1);
        assert_eq!(TileInkCache::density_tier(f32::NAN), 1);
        assert_eq!(TileInkCache::density_tier(1.0), 1);
        assert_eq!(TileInkCache::density_tier(1.5), 2);
        assert_eq!(TileInkCache::density_tier(2.5), 2);
        assert_eq!(TileInkCache::density_tier(3.0), 4);
        assert_eq!(TileInkCache::density_tier(6.0), 8);
        assert_eq!(TileInkCache::density_tier(100.0), 8);
        assert_eq!(TileInkCache::tile_size(1), 256);
        assert_eq!(TileInkCache::tile_size(4), 128);
        assert_eq!(TileInkCache::tile_size(8), 64);
    }

    #[test]
    fn bounds_and_estimates() {
        let stroke = map(r#"{"kind":"stroke","width":4,"points":[[0,0],[30,40]]}"#);
        assert_eq!(TileInkCache::compute_bounds(&stroke).unwrap(), RectF::new(-4.0, -4.0, 34.0, 44.0));
        let marker = map(r#"{"kind":"stroke","width":4,"brush":"marker","points":[[0,0]]}"#);
        assert_eq!(TileInkCache::compute_bounds(&marker).unwrap(), RectF::new(-10.0, -10.0, 10.0, 10.0));
        let empty = map(r#"{"kind":"stroke","width":4,"points":[]}"#);
        assert_eq!(TileInkCache::compute_bounds(&empty).unwrap(), RectF::default());
        // length 50, step min(2, 4*.2)=.8 → ceil(62.5)+1 = 64, plus 3 erasure points.
        let erased = map(r#"{"kind":"stroke","width":4,"points":[[0,0],[30,40]],"erasures":[[[0,0],[1,1],[2,2]]]}"#);
        assert_eq!(TileInkCache::path_point_estimate(&erased).unwrap(), 67);
        assert_eq!(TileInkCache::path_point_estimate(&empty).unwrap(), 0);
    }

    #[test]
    fn bound_cache_identity() {
        let a: Entity = Arc::new(map(r#"{"kind":"stroke","width":2,"points":[[1,1]]}"#));
        let first = TileInkCache::bounds(&a).unwrap();
        assert_eq!(first, TileInkCache::bounds(&a).unwrap());
        let metrics = TileInkCache::bounds_cache_metrics();
        assert!(metrics["entries"].as_i64().unwrap() >= 1);
    }
}
