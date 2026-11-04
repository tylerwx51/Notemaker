//! Port of `ItemCoverage.java` (JVM `Area` and Android `Path.op` variants
//! collapsed): exact vector coverage for hit testing, with lazy raster tiles
//! for images and SVG objects.

use crate::area::Area;
use crate::canvas::{CanvasPoint, FillType, Path, RectF, SkiaPath};
use crate::png::Png;
use crate::typeface::{FaceStyle, TextSize, Typeface};
use indexmap::IndexMap;
use notemaker_core::coverage::{Coverage, CoverageFactory, CoverageTile, Painter, Tiles};
use notemaker_core::geometry::{Bounds, DocPoint, DocRect, Erasures, Polygon, Segment};
use notemaker_core::ink_geometry::{AreaOverlap, InkGeometry, InkPart};
use notemaker_core::model::Model;
use notemaker_core::render::{self, Surface};
use notemaker_core::style::{
    ArgbColor, ArgbPixels, FontFamily, FontSize, PathEnd, RectShape, ShapePaint, StrokeWidth,
    TextAlignment, TextContent, TextStyle,
};
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
    fn bounds(&self) -> Bounds {
        self.0.bounds()
    }
    fn intersects(&self, polygon: &Polygon) -> Result<bool> {
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
    fn paint(&self, tile: CoverageTile) -> Result<ArgbPixels> {
        Png::coverage_tile(&Model::new(), &self.item, tile)
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
        let ink_bounds = RectF {
            left: b.left as f32,
            top: b.top as f32,
            right: b.right as f32,
            bottom: b.bottom as f32,
        };
        c.box_ = Some(match c.box_ {
            None => ink_bounds,
            Some(r) => RectF {
                left: r.left.min(ink_bounds.left),
                top: r.top.min(ink_bounds.top),
                right: r.right.max(ink_bounds.right),
                bottom: r.bottom.max(ink_bounds.bottom),
            },
        });
    }
    for p in std::mem::take(&mut c.masks) {
        let cut = Area::of(&path(&p, PathEnd::Closed));
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
pub fn path(p: &Polygon, end: PathEnd) -> Path {
    let mut path = Path::new();
    path.set_fill_type(FillType::EvenOdd);
    if let Some(first) = p.first() {
        path.move_to(CanvasPoint { x: first.x as f32, y: first.y as f32 });
        for q in &p[1..] {
            path.line_to(CanvasPoint { x: q.x as f32, y: q.y as f32 });
        }
        if end == PathEnd::Closed {
            path.close();
        }
    }
    path
}

/// How many coverages the shared cache holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CachedCoverageCount(pub usize);

/// Number of cached coverages (probes read the private `CACHE` by reflection).
pub fn cache_len() -> CachedCoverageCount {
    CachedCoverageCount(cache().lock().unwrap_or_else(|e| e.into_inner()).len())
}

/// Empty the coverage cache (`CACHE.clear()` by reflection in the probes).
pub fn cache_clear() {
    cache().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// AWT `BasicStroke(width, cap, join).createStrokedShape(shape)`.
fn stroked(shape: &Path, width: f64, round: bool) -> Area {
    let Some(SkiaPath { path: p, .. }) = shape.to_skia() else {
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
        Some(outline) => Area::of(&Path::from_skia(&SkiaPath {
            path: outline,
            rule: tiny_skia::FillRule::Winding,
        })),
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

fn alpha(color: ArgbColor) -> u32 {
    (color.0 as u32) >> 24
}

fn rect_path(x: f64, y: f64, w: f64, h: f64, ellipse: bool) -> Path {
    let mut p = Path::new();
    let r = RectF {
        left: x as f32,
        top: y as f32,
        right: (x + w) as f32,
        bottom: (y + h) as f32,
    };
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
                    Some(r) => RectF {
                        left: r.left.min(b.left),
                        top: r.top.min(b.top),
                        right: r.right.max(b.right),
                        bottom: r.bottom.max(b.bottom),
                    },
                });
            }
        }
        self.parts.push(part);
        self.part_bounds.push(b);
    }

    fn paint(&mut self, shape: &Path, color: ArgbColor, width: f64, fill: Option<ArgbColor>) {
        if fill.is_some_and(|f| alpha(f) != 0) {
            self.add(Area::of(shape));
        }
        if alpha(color) != 0 && width > 0.0 {
            self.add(stroked(shape, width, true));
        }
    }
}

impl Coverage for ItemCoverage {
    fn bounds(&self) -> Bounds {
        match self.box_ {
            None => Bounds { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 },
            Some(b) => Bounds {
                left: b.left as f64,
                top: b.top as f64,
                right: b.right as f64,
                bottom: b.bottom as f64,
            },
        }
    }
    fn intersects(&self, polygon: &Polygon) -> Result<bool> {
        let mut query = Area::of(&path(polygon, PathEnd::Closed));
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
            let area = Bounds {
                left: qb.left as f64,
                top: qb.top as f64,
                right: qb.right as f64,
                bottom: qb.bottom as f64,
            };
            for (part, overlap) in ink.parts(area) {
                if overlap == AreaOverlap::Overlapping {
                    let shape = match part {
                        InkPart::Line { segment, width } => {
                            let mut line = Path::new();
                            line.move_to(CanvasPoint { x: segment.start.x as f32, y: segment.start.y as f32 });
                            line.line_to(CanvasPoint { x: segment.end.x as f32, y: segment.end.y as f32 });
                            stroked(&line, width.0, true)
                        }
                        InkPart::Dot { rect, shape } => Area::of(&rect_path(
                            rect.x,
                            rect.y,
                            rect.width,
                            rect.height,
                            shape == RectShape::Ellipse,
                        )),
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
    fn begin_erase(&mut self, erasures: &Erasures) -> Result<()> {
        self.masks.extend(erasures.iter().cloned());
        Ok(())
    }
    fn end_erase(&mut self) -> Result<()> {
        Ok(())
    }
    fn polygon(&mut self, p: &Polygon, paint: ShapePaint, end: PathEnd) -> Result<()> {
        let ShapePaint { color, width, fill } = paint;
        let shape = path(p, end);
        if self.ink.is_some() {
            if fill.is_some_and(|f| alpha(f) != 0) {
                self.add(Area::of(&shape));
            }
            return Ok(());
        }
        self.paint(&shape, color, width.0, fill);
        Ok(())
    }
    fn line(&mut self, segment: Segment, color: ArgbColor, width: StrokeWidth) -> Result<()> {
        if let Some(ink) = &mut self.ink {
            if alpha(color) != 0 && width.0 > 0.0 {
                ink.line(segment, width);
            }
            return Ok(());
        }
        let Segment { start, end } = segment;
        let mut line = Path::new();
        line.move_to(CanvasPoint { x: start.x as f32, y: start.y as f32 });
        line.line_to(CanvasPoint { x: end.x as f32, y: end.y as f32 });
        self.paint(&line, color, width.0, None);
        Ok(())
    }
    fn rect(&mut self, rect: DocRect, paint: ShapePaint, shape: RectShape) -> Result<()> {
        let DocRect { x, y, width: w, height: h } = rect;
        let ShapePaint { color, width, fill } = paint;
        let width = width.0;
        if let Some(ink) = &mut self.ink {
            if width == 0.0 {
                if fill.is_some_and(|f| alpha(f) != 0) && w > 0.0 && h > 0.0 {
                    ink.dot(rect, shape);
                }
                return Ok(());
            }
        }
        let shape_path = rect_path(x, y, w, h, shape == RectShape::Ellipse);
        if fill.is_some_and(|f| alpha(f) != 0) {
            self.add(Area::of(&shape_path));
        }
        if alpha(color) != 0 && width > 0.0 {
            self.add(stroked(&shape_path, width, false));
        }
        Ok(())
    }
    fn text(&mut self, origin: DocPoint, text: &TextContent, color: ArgbColor, size: FontSize) -> Result<()> {
        self.styled_text(
            origin,
            text,
            color,
            size,
            &FontFamily::from("sans-serif"),
            TextStyle { bold: false, italic: false, underline: false },
            &TextAlignment::from("left"),
        )
    }
    fn styled_text(
        &mut self,
        origin: DocPoint,
        text: &TextContent,
        color: ArgbColor,
        size: FontSize,
        family: &FontFamily,
        style: TextStyle,
        alignment: &TextAlignment,
    ) -> Result<()> {
        let DocPoint { x, y } = origin;
        let size = size.0;
        let alignment: &str = alignment;
        if alpha(color) == 0 || text.is_empty() {
            return Ok(());
        }
        let face = Typeface::create(family, FaceStyle { bold: style.bold, italic: style.italic })?;
        let width = face.measure_text(text, TextSize(size as f32)).0 as f64;
        let left = x - if alignment == "center" {
            width / 2.0
        } else if alignment == "right" {
            width
        } else {
            0.0
        };
        self.add(Area::of(&face.text_path(
            text,
            CanvasPoint { x: left as f32, y: y as f32 },
            TextSize(size as f32),
        )));
        if style.underline {
            self.line(
                Segment {
                    start: DocPoint { x: left, y: y + 2.0 },
                    end: DocPoint { x: left + width, y: y + 2.0 },
                },
                color,
                StrokeWidth(1.0),
            )?;
        }
        Ok(())
    }
    fn image(&mut self, rect: DocRect, _data: &str) -> Result<()> {
        self.raster = true;
        self.add(Area::of(&rect_path(rect.x, rect.y, rect.width, rect.height, false)));
        Ok(())
    }
    fn svg(&mut self, rect: DocRect, _source: &str) -> Result<()> {
        self.raster = true;
        self.add(Area::of(&rect_path(rect.x, rect.y, rect.width, rect.height, false)));
        Ok(())
    }
}
