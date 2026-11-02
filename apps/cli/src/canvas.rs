//! Backend-neutral drawing target: the subset of `android.graphics.Canvas`,
//! `Paint`, `Path` and `RectF` that the Java `Png`/`ItemCoverage` used.
//!
//! [`Png`](crate::png::Png) renders through `&mut dyn Canvas`, so the same
//! scene, stroke-command and text logic serves the tiny-skia CLI backend
//! ([`RasterCanvas`](crate::raster::RasterCanvas)) and the Android app's JNI
//! `android.graphics.Canvas` backend. Geometry is `f32`, as on Android.

use resvg::{tiny_skia, usvg};

/// `Path.FillType` (only the two types the Java code used).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FillType {
    /// `WINDING` / AWT `WIND_NON_ZERO`.
    #[default]
    Winding,
    /// `EVEN_ODD` / AWT `WIND_EVEN_ODD`.
    EvenOdd,
}

/// One path verb with its points, in `android.graphics.Path` order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verb {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

/// `android.graphics.Path` / AWT `Path2D` as plain data. A JNI backend
/// rebuilds an Android `Path` from [`Path::verbs`]; tiny-skia uses
/// [`Path::to_skia`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    fill_type: FillType,
    verbs: Vec<Verb>,
}

impl Path {
    /// `new Path()` (winding fill).
    pub fn new() -> Path {
        Path::default()
    }
    pub fn fill_type(&self) -> FillType {
        self.fill_type
    }
    pub fn set_fill_type(&mut self, fill_type: FillType) {
        self.fill_type = fill_type;
    }
    pub fn verbs(&self) -> &[Verb] {
        &self.verbs
    }
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.verbs.push(Verb::Move(x, y));
    }
    pub fn line_to(&mut self, x: f32, y: f32) {
        self.inject_move();
        self.verbs.push(Verb::Line(x, y));
    }
    pub fn quad_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.inject_move();
        self.verbs.push(Verb::Quad(x1, y1, x2, y2));
    }
    pub fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32) {
        self.inject_move();
        self.verbs.push(Verb::Cubic(x1, y1, x2, y2, x3, y3));
    }
    pub fn close(&mut self) {
        if !self.verbs.is_empty() && !matches!(self.verbs.last(), Some(Verb::Close)) {
            self.verbs.push(Verb::Close);
        }
    }
    /// Skia starts a contour at (0,0) or the last move when a segment
    /// follows nothing or a close.
    fn inject_move(&mut self) {
        match self.verbs.last() {
            None => self.verbs.push(Verb::Move(0.0, 0.0)),
            Some(Verb::Close) => {
                let start = self
                    .verbs
                    .iter()
                    .rev()
                    .find_map(|v| {
                        if let Verb::Move(x, y) = v {
                            Some((*x, *y))
                        } else {
                            None
                        }
                    })
                    .unwrap_or((0.0, 0.0));
                self.verbs.push(Verb::Move(start.0, start.1));
            }
            _ => {}
        }
    }
    /// `addPath(other)`: appends `other`'s contours, keeping this fill type.
    pub fn add_path(&mut self, other: &Path) {
        self.verbs.extend_from_slice(&other.verbs);
    }
    /// `addRect(l,t,r,b,Direction.CW)`.
    pub fn add_rect(&mut self, rect: RectF) {
        self.verbs.push(Verb::Move(rect.left, rect.top));
        self.verbs.push(Verb::Line(rect.right, rect.top));
        self.verbs.push(Verb::Line(rect.right, rect.bottom));
        self.verbs.push(Verb::Line(rect.left, rect.bottom));
        self.verbs.push(Verb::Close);
    }
    /// `addOval(rect,Direction.CW)` (four cubic quadrants).
    pub fn add_oval(&mut self, rect: RectF) {
        let cx = (rect.left + rect.right) / 2.0;
        let cy = (rect.top + rect.bottom) / 2.0;
        let kx = (rect.right - rect.left) / 2.0 * 0.552_284_8;
        let ky = (rect.bottom - rect.top) / 2.0 * 0.552_284_8;
        let (l, t, r, b) = (rect.left, rect.top, rect.right, rect.bottom);
        self.verbs.push(Verb::Move(r, cy));
        self.verbs.push(Verb::Cubic(r, cy + ky, cx + kx, b, cx, b));
        self.verbs.push(Verb::Cubic(cx - kx, b, l, cy + ky, l, cy));
        self.verbs.push(Verb::Cubic(l, cy - ky, cx - kx, t, cx, t));
        self.verbs.push(Verb::Cubic(cx + kx, t, r, cy - ky, r, cy));
        self.verbs.push(Verb::Close);
    }
    /// `isEmpty()`: no verbs at all (not "zero area").
    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }
    /// `computeBounds(bounds, true)`: control-point bounds.
    pub fn compute_bounds(&self) -> RectF {
        let mut b: Option<RectF> = None;
        let mut add = |x: f32, y: f32| match &mut b {
            None => b = Some(RectF::new(x, y, x, y)),
            Some(r) => {
                r.left = r.left.min(x);
                r.top = r.top.min(y);
                r.right = r.right.max(x);
                r.bottom = r.bottom.max(y);
            }
        };
        for v in &self.verbs {
            match *v {
                Verb::Move(x, y) | Verb::Line(x, y) => add(x, y),
                Verb::Quad(a, b2, c, d) => {
                    add(a, b2);
                    add(c, d)
                }
                Verb::Cubic(a, b2, c, d, e, f) => {
                    add(a, b2);
                    add(c, d);
                    add(e, f)
                }
                Verb::Close => {}
            }
        }
        b.unwrap_or_default()
    }
    /// The same path transformed by `t`.
    pub fn transform(&self, t: tiny_skia::Transform) -> Path {
        let p = |x: f32, y: f32| {
            let mut pt = [tiny_skia::Point::from_xy(x, y)];
            t.map_points(&mut pt);
            (pt[0].x, pt[0].y)
        };
        let verbs = self
            .verbs
            .iter()
            .map(|v| match *v {
                Verb::Move(x, y) => {
                    let (x, y) = p(x, y);
                    Verb::Move(x, y)
                }
                Verb::Line(x, y) => {
                    let (x, y) = p(x, y);
                    Verb::Line(x, y)
                }
                Verb::Quad(a, b, c, d) => {
                    let (a, b) = p(a, b);
                    let (c, d) = p(c, d);
                    Verb::Quad(a, b, c, d)
                }
                Verb::Cubic(a, b, c, d, e, f) => {
                    let (a, b) = p(a, b);
                    let (c, d) = p(c, d);
                    let (e, f) = p(e, f);
                    Verb::Cubic(a, b, c, d, e, f)
                }
                Verb::Close => Verb::Close,
            })
            .collect();
        Path {
            fill_type: self.fill_type,
            verbs,
        }
    }
    /// tiny-skia path and fill rule; `None` for an empty path.
    pub fn to_skia(&self) -> Option<(tiny_skia::Path, tiny_skia::FillRule)> {
        let mut b = tiny_skia::PathBuilder::new();
        for v in &self.verbs {
            match *v {
                Verb::Move(x, y) => b.move_to(x, y),
                Verb::Line(x, y) => b.line_to(x, y),
                Verb::Quad(a, c, d, e) => b.quad_to(a, c, d, e),
                Verb::Cubic(a, c, d, e, f, g) => b.cubic_to(a, c, d, e, f, g),
                Verb::Close => b.close(),
            }
        }
        let rule = match self.fill_type {
            FillType::Winding => tiny_skia::FillRule::Winding,
            FillType::EvenOdd => tiny_skia::FillRule::EvenOdd,
        };
        b.finish().map(|p| (p, rule))
    }
    /// Path from a tiny-skia path (e.g. a stroker result).
    pub fn from_skia(path: &tiny_skia::Path, fill_type: FillType) -> Path {
        let mut out = Path {
            fill_type,
            verbs: Vec::new(),
        };
        for s in path.segments() {
            match s {
                tiny_skia::PathSegment::MoveTo(p) => out.verbs.push(Verb::Move(p.x, p.y)),
                tiny_skia::PathSegment::LineTo(p) => out.verbs.push(Verb::Line(p.x, p.y)),
                tiny_skia::PathSegment::QuadTo(a, b) => {
                    out.verbs.push(Verb::Quad(a.x, a.y, b.x, b.y))
                }
                tiny_skia::PathSegment::CubicTo(a, b, c) => {
                    out.verbs.push(Verb::Cubic(a.x, a.y, b.x, b.y, c.x, c.y))
                }
                tiny_skia::PathSegment::Close => out.verbs.push(Verb::Close),
            }
        }
        out
    }
    /// Contours flattened to line segments (`tolerance` in path units), for
    /// boolean geometry. Each contour is implicitly closed.
    pub fn flatten(&self, tolerance: f32) -> Vec<Vec<[f32; 2]>> {
        let mut out: Vec<Vec<[f32; 2]>> = Vec::new();
        let mut current: Vec<[f32; 2]> = Vec::new();
        let mut last = [0f32, 0f32];
        let segments = |len: f32| -> usize {
            ((len / tolerance.max(1e-4)).sqrt().ceil() as usize).clamp(1, 256)
        };
        for v in &self.verbs {
            match *v {
                Verb::Move(x, y) => {
                    if current.len() > 1 {
                        out.push(std::mem::take(&mut current));
                    }
                    current.clear();
                    current.push([x, y]);
                    last = [x, y];
                }
                Verb::Line(x, y) => {
                    current.push([x, y]);
                    last = [x, y];
                }
                Verb::Quad(x1, y1, x2, y2) => {
                    let dd = ((last[0] - 2.0 * x1 + x2).abs()).max((last[1] - 2.0 * y1 + y2).abs());
                    let n = segments(dd * 2.0);
                    for i in 1..=n {
                        let t = i as f32 / n as f32;
                        let u = 1.0 - t;
                        current.push([
                            u * u * last[0] + 2.0 * u * t * x1 + t * t * x2,
                            u * u * last[1] + 2.0 * u * t * y1 + t * t * y2,
                        ]);
                    }
                    last = [x2, y2];
                }
                Verb::Cubic(x1, y1, x2, y2, x3, y3) => {
                    let dd = (last[0] - 2.0 * x1 + x2)
                        .abs()
                        .max((last[1] - 2.0 * y1 + y2).abs())
                        .max((x1 - 2.0 * x2 + x3).abs())
                        .max((y1 - 2.0 * y2 + y3).abs());
                    let n = segments(dd * 6.0);
                    for i in 1..=n {
                        let t = i as f32 / n as f32;
                        let u = 1.0 - t;
                        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                        current.push([
                            a * last[0] + b * x1 + c * x2 + d * x3,
                            a * last[1] + b * y1 + c * y2 + d * y3,
                        ]);
                    }
                    last = [x3, y3];
                }
                Verb::Close => {
                    if current.len() > 1 {
                        let start = current[0];
                        out.push(std::mem::take(&mut current));
                        current.push(start);
                        last = start;
                    } else {
                        current.clear();
                    }
                }
            }
        }
        if current.len() > 1 {
            out.push(current);
        }
        out
    }
}

/// `android.graphics.RectF` (AWT `Rectangle2D` in the JVM variant).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl RectF {
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> RectF {
        RectF {
            left,
            top,
            right,
            bottom,
        }
    }
    /// `isEmpty()`: `left >= right || top >= bottom`.
    pub fn is_empty(&self) -> bool {
        !(self.left < self.right && self.top < self.bottom)
    }
    /// Static `RectF.intersects(a, b)`.
    pub fn intersects(a: &RectF, b: &RectF) -> bool {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }
    /// `union(RectF)`: ignores an empty argument; replaces an empty receiver.
    pub fn union(&mut self, other: &RectF) {
        if other.is_empty() {
            return;
        }
        if self.is_empty() {
            *self = *other;
        } else {
            self.left = self.left.min(other.left);
            self.top = self.top.min(other.top);
            self.right = self.right.max(other.right);
            self.bottom = self.bottom.max(other.bottom);
        }
    }
}

/// `Paint.Style`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    Fill,
    Stroke,
}

/// `Paint.Cap`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cap {
    #[default]
    Butt,
    Round,
    Square,
}

/// `Paint.Join`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Join {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// `android.graphics.Paint` fields that `Png` sets. Text attributes are not
/// here: text is converted to glyph outlines by [`crate::typeface`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    /// ARGB, as `setColor(int)`.
    pub color: i32,
    pub style: Style,
    /// `0` is a hairline, as on Android.
    pub stroke_width: f32,
    pub cap: Cap,
    pub join: Join,
    pub anti_alias: bool,
}

impl Default for Paint {
    /// `new Paint()`: black fill, hairline, butt cap, miter join, no AA.
    fn default() -> Paint {
        Paint {
            color: 0xff00_0000u32 as i32,
            style: Style::Fill,
            stroke_width: 0.0,
            cap: Cap::Butt,
            join: Join::Miter,
            anti_alias: false,
        }
    }
}

impl Paint {
    /// `new Paint(Paint.ANTI_ALIAS_FLAG)`.
    pub fn anti_alias() -> Paint {
        Paint {
            anti_alias: true,
            ..Paint::default()
        }
    }
}

/// A drawing target with Android `Canvas` semantics: a save stack of matrix
/// and clip, `restoreToCount`, and `clipOutPath`. All coordinates are in
/// local (pre-matrix) units.
pub trait Canvas {
    /// `save()`: returns the save count before the push.
    fn save(&mut self) -> i32;
    fn restore(&mut self);
    fn restore_to_count(&mut self, count: i32);
    fn translate(&mut self, dx: f32, dy: f32);
    /// Degrees, clockwise in a y-down space.
    fn rotate(&mut self, degrees: f32);
    fn scale(&mut self, sx: f32, sy: f32);
    /// Current local → device matrix (`getMatrix()`).
    fn matrix(&self) -> tiny_skia::Transform;
    /// `clipRect(l,t,r,b)` (intersect).
    fn clip_rect(&mut self, left: f32, top: f32, right: f32, bottom: f32);
    /// `clipOutPath(path)` (difference).
    fn clip_out_path(&mut self, path: &Path);
    /// `drawColor(color)`: fill the clip with `color` (source-over).
    fn draw_color(&mut self, color: i32);
    fn draw_path(&mut self, path: &Path, paint: &Paint);
    fn draw_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, paint: &Paint);
    fn draw_rect(&mut self, rect: RectF, paint: &Paint);
    fn draw_oval(&mut self, rect: RectF, paint: &Paint);
    /// `drawBitmap(bitmap, null, dst, paint)`: premultiplied RGBA scaled
    /// into `dst` with bilinear filtering.
    fn draw_bitmap(&mut self, bitmap: &tiny_skia::Pixmap, dst: RectF, paint: &Paint);
    /// Render a parsed SVG into the local box `(0,0,w,h)` (the caller has
    /// already translated and clipped). The default rasterizes with resvg at
    /// the current device scale and calls [`Canvas::draw_bitmap`]; a raster
    /// backend overrides it to render directly with its matrix and clip.
    fn draw_svg(&mut self, tree: &usvg::Tree, w: f32, h: f32) {
        draw_svg_raster(self, tree, w, h);
    }
}

/// The default [`Canvas::draw_svg`]: rasterize with resvg at the current
/// device scale and draw the result into `(0,0,w,h)` with
/// [`Canvas::draw_bitmap`]. Backends that override `draw_svg` for the common
/// case can fall back to this.
pub fn draw_svg_raster<C: Canvas + ?Sized>(canvas: &mut C, tree: &usvg::Tree, w: f32, h: f32) {
    let m = canvas.matrix();
    let sx = (m.sx * m.sx + m.ky * m.ky).sqrt().max(1e-3);
    let sy = (m.kx * m.kx + m.sy * m.sy).sqrt().max(1e-3);
    let pw = (w * sx).ceil().clamp(1.0, 8192.0) as u32;
    let ph = (h * sy).ceil().clamp(1.0, 8192.0) as u32;
    let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
        return;
    };
    let size = tree.size();
    resvg::render(
        tree,
        tiny_skia::Transform::from_scale(pw as f32 / size.width(), ph as f32 / size.height()),
        &mut pixmap.as_mut(),
    );
    canvas.draw_bitmap(&pixmap, RectF::new(0.0, 0.0, w, h), &Paint::anti_alias());
}
