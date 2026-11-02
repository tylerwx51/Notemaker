//! Port of `ItemCoverage.java` (JVM `Area` and Android `Path.op` variants
//! collapsed): exact vector coverage for hit testing, with lazy raster tiles
//! for images and SVG objects.

use crate::area::Area;
use crate::canvas::{FillType, Path, RectF};
use crate::png::Png;
use crate::typeface::Typeface;
use indexmap::IndexMap;
use notemaker_core::coverage::{Coverage, CoverageFactory, Painter, Tiles};
use notemaker_core::geometry::Polygon;
use notemaker_core::ink_geometry::InkGeometry;
use notemaker_core::model::Model;
use notemaker_core::render::{self, Surface};
use notemaker_core::{json, Map, Result};
use resvg::tiny_skia;
use std::sync::{Arc, Mutex, OnceLock};

/// `ItemCoverage`: painted area of one item in item-local coordinates.
#[derive(Default)]
pub struct ItemCoverage {
    parts: Vec<Area>,
    cuts: Vec<Area>,
    part_bounds: Vec<Option<RectF>>,
    cut_bounds: Vec<Option<RectF>>,
    box_: Option<RectF>,
    ink: Option<InkGeometry>,
    raster: bool,
    masks: Vec<Polygon>,
}

/// The `ItemCoverage::create` method reference as a `Coverage.Factory`.
/// Use the [`FACTORY`] static so `DrawItem`'s per-factory cache sees one
/// stable identity.
pub struct Factory;

/// `ItemCoverage::create` as a factory value with a stable address.
pub static FACTORY: Factory = Factory;

/// A cache entry handed out as an owned `Box<dyn Coverage>`.
struct Shared(Arc<dyn Coverage>);

impl Coverage for Shared {
    fn bounds(&self) -> [f64; 4] {
        self.0.bounds()
    }
    fn intersects(&self, polygon: &[[f64; 2]]) -> Result<bool> {
        self.0.intersects(polygon)
    }
}

impl CoverageFactory for Factory {
    fn create(&self, local_content: &Map) -> Result<Box<dyn Coverage>> {
        Ok(Box::new(Shared(create(local_content)?)))
    }
}

/// `(x,y,size)->Png.coverageTile(new Model(),item,x,y,size)`.
struct TilePainter {
    item: Map,
}

impl Painter for TilePainter {
    fn paint(&self, x: i32, y: i32, size: i32) -> Result<Vec<i32>> {
        Png::coverage_tile(&Model::new(), &self.item, x, y, size)
    }
}

const CACHE_LIMIT: usize = 10000;
const IGNORED: [&str; 12] = [
    "id",
    "actor",
    "seq",
    "at",
    "action",
    "session",
    "bases",
    "visibility",
    "object",
    "stroke",
    "layer",
    "page",
];

fn cache() -> &'static Mutex<IndexMap<String, Arc<dyn Coverage>>> {
    static CACHE: OnceLock<Mutex<IndexMap<String, Arc<dyn Coverage>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(IndexMap::new()))
}

/// `ItemCoverage.create(item)`: cached (LRU, 10000 entries, keyed by the item
/// without identity/placement fields) coverage, shared between callers.
pub fn create(item: &Map) -> Result<Arc<dyn Coverage>> {
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    let mut key = item.clone();
    for k in IGNORED {
        key.shift_remove(k);
    }
    let key = json::write_map(&key);
    if let Some(cached) = cache.shift_remove(&key) {
        cache.insert(key, cached.clone());
        return Ok(cached);
    }
    let mut c = ItemCoverage::default();
    if item.get("kind").and_then(|k| k.as_str()) == Some("stroke") {
        c.ink = Some(InkGeometry::new());
    }
    render::draw_item(&Model::new(), item, &mut c)?;
    if let Some(ink) = &mut c.ink {
        ink.finish();
        let b = ink.bounds();
        let ink_bounds = RectF::new(b[0] as f32, b[1] as f32, b[2] as f32, b[3] as f32);
        c.box_ = Some(match c.box_ {
            None => ink_bounds,
            Some(r) => RectF::new(
                r.left.min(ink_bounds.left),
                r.top.min(ink_bounds.top),
                r.right.max(ink_bounds.right),
                r.bottom.max(ink_bounds.bottom),
            ),
        });
    }
    for p in std::mem::take(&mut c.masks) {
        let cut = Area::of(&path(&p, true));
        c.cut_bounds.push(cut.bounds());
        c.cuts.push(cut);
    }
    let result: Arc<dyn Coverage> = if c.raster {
        Arc::new(Tiles::new(
            c.bounds(),
            Box::new(TilePainter { item: item.clone() }),
        ))
    } else {
        Arc::new(c)
    };
    cache.insert(key, result.clone());
    if cache.len() > CACHE_LIMIT {
        cache.shift_remove_index(0);
    }
    Ok(result)
}

/// `ItemCoverage.path(points, close)`: even-odd polyline path.
pub fn path(p: &[[f64; 2]], close: bool) -> Path {
    let mut path = Path::new();
    path.set_fill_type(FillType::EvenOdd);
    if let Some(first) = p.first() {
        path.move_to(first[0] as f32, first[1] as f32);
        for q in &p[1..] {
            path.line_to(q[0] as f32, q[1] as f32);
        }
        if close {
            path.close();
        }
    }
    path
}

/// Number of cached coverages (probes read the private `CACHE` by reflection).
pub fn cache_len() -> usize {
    cache().lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// Empty the coverage cache (`CACHE.clear()` by reflection in the probes).
pub fn cache_clear() {
    cache().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// AWT `BasicStroke(width, cap, join).createStrokedShape(shape)`.
fn stroked(shape: &Path, width: f64, round: bool) -> Area {
    let Some((p, _)) = shape.to_skia() else {
        return Area::default();
    };
    let stroke = tiny_skia::Stroke {
        width: width as f32,
        miter_limit: 10.0,
        line_cap: if round {
            tiny_skia::LineCap::Round
        } else {
            tiny_skia::LineCap::Square
        },
        line_join: if round {
            tiny_skia::LineJoin::Round
        } else {
            tiny_skia::LineJoin::Miter
        },
        dash: None,
    };
    match p.stroke(&stroke, 8.0) {
        Some(outline) => Area::of(&Path::from_skia(&outline, FillType::Winding)),
        None => Area::default(),
    }
}

/// `Rectangle2D.intersects`: both non-empty and strictly overlapping.
fn rect_hits(a: &Option<RectF>, b: &Option<RectF>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => !a.is_empty() && !b.is_empty() && RectF::intersects(a, b),
        _ => false,
    }
}

fn alpha(color: i32) -> u32 {
    (color as u32) >> 24
}

fn rect_path(x: f64, y: f64, w: f64, h: f64, ellipse: bool) -> Path {
    let mut p = Path::new();
    let r = RectF::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32);
    if ellipse {
        p.add_oval(r);
    } else {
        p.add_rect(r);
    }
    p
}

impl ItemCoverage {
    fn add(&mut self, part: Area) {
        let b = part.bounds();
        if !part.is_empty() {
            if let Some(b) = b {
                self.box_ = Some(match self.box_ {
                    None => b,
                    Some(r) => RectF::new(
                        r.left.min(b.left),
                        r.top.min(b.top),
                        r.right.max(b.right),
                        r.bottom.max(b.bottom),
                    ),
                });
            }
        }
        self.parts.push(part);
        self.part_bounds.push(b);
    }

    fn paint(&mut self, shape: &Path, color: i32, width: f64, fill: Option<i32>) {
        if fill.is_some_and(|f| alpha(f) != 0) {
            self.add(Area::of(shape));
        }
        if alpha(color) != 0 && width > 0.0 {
            self.add(stroked(shape, width, true));
        }
    }
}

impl Coverage for ItemCoverage {
    fn bounds(&self) -> [f64; 4] {
        match self.box_ {
            None => [0.0; 4],
            Some(b) => [b.left as f64, b.top as f64, b.right as f64, b.bottom as f64],
        }
    }
    fn intersects(&self, polygon: &[[f64; 2]]) -> Result<bool> {
        let mut query = Area::of(&path(polygon, true));
        let mut q = query.bounds();
        for i in 0..self.cuts.len() {
            if rect_hits(&q, &self.cut_bounds[i]) {
                query = query.subtract(&self.cuts[i]);
                if query.is_empty() {
                    return Ok(false);
                }
            }
        }
        q = query.bounds();
        let Some(qb) = q else { return Ok(false) };
        if let Some(ink) = &self.ink {
            for i in 0..ink.size() {
                if ink.overlaps(
                    i,
                    qb.left as f64,
                    qb.top as f64,
                    qb.right as f64,
                    qb.bottom as f64,
                ) {
                    let (x, y, w, h) = (
                        ink.value(i, 1),
                        ink.value(i, 2),
                        ink.value(i, 3),
                        ink.value(i, 4),
                    );
                    let shape = if ink.value(i, 0) == 0.0 {
                        let mut line = Path::new();
                        line.move_to(x as f32, y as f32);
                        line.line_to(w as f32, h as f32);
                        stroked(&line, ink.value(i, 5), true)
                    } else {
                        Area::of(&rect_path(x, y, w, h, ink.value(i, 0) == 1.0))
                    };
                    if query.overlaps(&shape) {
                        return Ok(true);
                    }
                }
            }
        }
        for i in 0..self.parts.len() {
            if rect_hits(&q, &self.part_bounds[i]) && query.overlaps(&self.parts[i]) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl Surface for ItemCoverage {
    fn begin_erase(&mut self, polygons: &[Polygon]) -> Result<()> {
        self.masks.extend(polygons.iter().cloned());
        Ok(())
    }
    fn end_erase(&mut self) -> Result<()> {
        Ok(())
    }
    fn polygon(
        &mut self,
        p: &[[f64; 2]],
        color: i32,
        width: f64,
        fill: Option<i32>,
        closed: bool,
    ) -> Result<()> {
        let shape = path(p, closed);
        if self.ink.is_some() {
            if fill.is_some_and(|f| alpha(f) != 0) {
                self.add(Area::of(&shape));
            }
            return Ok(());
        }
        self.paint(&shape, color, width, fill);
        Ok(())
    }
    fn line(&mut self, x: f64, y: f64, ex: f64, ey: f64, color: i32, width: f64) -> Result<()> {
        if let Some(ink) = &mut self.ink {
            if alpha(color) != 0 && width > 0.0 {
                ink.line(x, y, ex, ey, width);
            }
            return Ok(());
        }
        let mut line = Path::new();
        line.move_to(x as f32, y as f32);
        line.line_to(ex as f32, ey as f32);
        self.paint(&line, color, width, None);
        Ok(())
    }
    fn rect(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: i32,
        width: f64,
        fill: Option<i32>,
        ellipse: bool,
    ) -> Result<()> {
        if let Some(ink) = &mut self.ink {
            if width == 0.0 {
                if fill.is_some_and(|f| alpha(f) != 0) && w > 0.0 && h > 0.0 {
                    ink.dot(x, y, w, h, ellipse);
                }
                return Ok(());
            }
        }
        let shape = rect_path(x, y, w, h, ellipse);
        if fill.is_some_and(|f| alpha(f) != 0) {
            self.add(Area::of(&shape));
        }
        if alpha(color) != 0 && width > 0.0 {
            self.add(stroked(&shape, width, false));
        }
        Ok(())
    }
    fn text(&mut self, x: f64, y: f64, text: &str, color: i32, size: f64) -> Result<()> {
        self.styled_text(
            x,
            y,
            text,
            color,
            size,
            "sans-serif",
            false,
            false,
            false,
            "left",
        )
    }
    fn styled_text(
        &mut self,
        x: f64,
        y: f64,
        text: &str,
        color: i32,
        size: f64,
        family: &str,
        bold: bool,
        italic: bool,
        underline: bool,
        alignment: &str,
    ) -> Result<()> {
        if alpha(color) == 0 || text.is_empty() {
            return Ok(());
        }
        let face = Typeface::create(family, bold, italic)?;
        let width = face.measure_text(text, size as f32) as f64;
        let left = x - if alignment == "center" {
            width / 2.0
        } else if alignment == "right" {
            width
        } else {
            0.0
        };
        self.add(Area::of(&face.text_path(
            text,
            left as f32,
            y as f32,
            size as f32,
        )));
        if underline {
            self.line(left, y + 2.0, left + width, y + 2.0, color, 1.0)?;
        }
        Ok(())
    }
    fn image(&mut self, x: f64, y: f64, w: f64, h: f64, _data: &str) -> Result<()> {
        self.raster = true;
        self.add(Area::of(&rect_path(x, y, w, h, false)));
        Ok(())
    }
    fn svg(&mut self, x: f64, y: f64, w: f64, h: f64, _source: &str) -> Result<()> {
        self.raster = true;
        self.add(Area::of(&rect_path(x, y, w, h, false)));
        Ok(())
    }
}
