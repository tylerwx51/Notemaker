//! `notemaker_cli::canvas::Canvas` over a JNI `android.graphics.Canvas`.
//!
//! Wherever Java wrote `new com.notemaker.cli.Png(canvas)` on an Android
//! `Canvas`, the port writes
//! `Png::new(&mut AndroidCanvas::new(canvas.clone()))`, so the CLI's
//! rendering logic issues the same `save`/`clipOutPath`/`drawPath`/… calls on
//! the real Canvas. Text arrives as glyph paths, images as premultiplied
//! pixmaps (`drawBitmap` of a temporary `Bitmap`), and SVG through the trait's
//! default `draw_svg` (resvg raster at device scale, then `draw_bitmap`).
//! The Java Android `Png` replayed cached `Picture`s for SVG; that cache does
//! not exist here (see docs/design/android-port-plan.md, "Open points").
//!
//! Also: conversions for `Path`, `RectF`, `Matrix` and `Paint` enums. The
//! `android.graphics.RectF` methods the app uses ([`RectFExt`]) live in
//! `java_port::rect_f` and are re-exported here.

use crate::android_consts::PAINT_FILTER_BITMAP_FLAG;
use crate::jx::{self, JResult, Obj};
use notemaker_cli::canvas::{Bitmap, Canvas, CanvasMatrix, CanvasSegment, CanvasSize, Cap, FillType, Join, Paint, Path, RectF, Rotation, SaveCount, ScaleFactors, Style, SvgTree, Translation, Verb};
use notemaker_core::style::{ArgbColor, ArgbPixels};
use resvg::tiny_skia;
use std::sync::OnceLock;

pub use crate::java_port::rect_f::RectFExt;

/// A `Canvas` trait object drawing into a Java `android.graphics.Canvas`.
pub struct AndroidCanvas {
    canvas: Obj,
    /// Reused Java `Paint` and the values last applied to it.
    paint: Option<(Obj, Paint)>,
    bitmap_paint: Option<Obj>,
}

fn ok<T>(r: JResult<T>) -> T {
    r.unwrap_or_else(|e| panic!("{e}"))
}

impl AndroidCanvas {
    pub fn new(canvas: Obj) -> AndroidCanvas {
        AndroidCanvas { canvas, paint: None, bitmap_paint: None }
    }
    /// The wrapped Java canvas.
    pub fn canvas(&self) -> &Obj {
        &self.canvas
    }
    /// The Java `Paint` configured like `p`.
    pub fn java_paint(&mut self, p: &Paint) -> JResult<Obj> {
        if let Some((obj, applied)) = &mut self.paint {
            if applied != p {
                apply_paint(obj, p, Some(applied))?;
                *applied = *p;
            }
            return Ok(obj.clone());
        }
        let obj = jnew!("android/graphics/Paint", "()V")?;
        apply_paint(&obj, p, None)?;
        self.paint = Some((obj.clone(), *p));
        Ok(obj)
    }
}

/// Sets `p` on a Java `Paint`; `previous` skips unchanged attributes.
pub fn apply_paint(paint: &Obj, p: &Paint, previous: Option<&Paint>) -> JResult<()> {
    let changed = |f: fn(&Paint) -> bool| previous.map_or(true, |q| f(q) != f(p));
    if previous.map_or(true, |q| q.color != p.color) {
        jcall!(paint, "android/graphics/Paint", "setColor", "(I)V", p.color)?;
    }
    if previous.map_or(true, |q| q.style != p.style) {
        jcall!(paint, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &style_obj(p.style)?)?;
    }
    if previous.map_or(true, |q| q.stroke_width != p.stroke_width) {
        jcall!(paint, "android/graphics/Paint", "setStrokeWidth", "(F)V", p.stroke_width)?;
    }
    if previous.map_or(true, |q| q.cap != p.cap) {
        jcall!(paint, "android/graphics/Paint", "setStrokeCap", "(Landroid/graphics/Paint$Cap;)V", &cap_obj(p.cap)?)?;
    }
    if previous.map_or(true, |q| q.join != p.join) {
        jcall!(paint, "android/graphics/Paint", "setStrokeJoin", "(Landroid/graphics/Paint$Join;)V", &join_obj(p.join)?)?;
    }
    if changed(|q| q.anti_alias) {
        jcall!(paint, "android/graphics/Paint", "setAntiAlias", "(Z)V", p.anti_alias)?;
    }
    Ok(())
}

macro_rules! cached_static {
    ($cell:ident, $get:expr) => {{
        static $cell: OnceLock<Obj> = OnceLock::new();
        if let Some(v) = $cell.get() {
            return Ok(v.clone());
        }
        let v: Obj = $get?.some();
        Ok($cell.get_or_init(|| v).clone())
    }};
}

/// `Paint.Style.FILL` / `STROKE`.
pub fn style_obj(s: Style) -> JResult<Obj> {
    match s {
        Style::Fill => cached_static!(FILL, jsget!("android/graphics/Paint$Style", "FILL", "Landroid/graphics/Paint$Style;")),
        Style::Stroke => cached_static!(STROKE, jsget!("android/graphics/Paint$Style", "STROKE", "Landroid/graphics/Paint$Style;")),
    }
}
/// `Paint.Cap`.
pub fn cap_obj(c: Cap) -> JResult<Obj> {
    match c {
        Cap::Butt => cached_static!(BUTT, jsget!("android/graphics/Paint$Cap", "BUTT", "Landroid/graphics/Paint$Cap;")),
        Cap::Round => cached_static!(ROUND, jsget!("android/graphics/Paint$Cap", "ROUND", "Landroid/graphics/Paint$Cap;")),
        Cap::Square => cached_static!(SQUARE, jsget!("android/graphics/Paint$Cap", "SQUARE", "Landroid/graphics/Paint$Cap;")),
    }
}
/// `Paint.Join`.
pub fn join_obj(j: Join) -> JResult<Obj> {
    match j {
        Join::Miter => cached_static!(MITER, jsget!("android/graphics/Paint$Join", "MITER", "Landroid/graphics/Paint$Join;")),
        Join::Round => cached_static!(ROUND, jsget!("android/graphics/Paint$Join", "ROUND", "Landroid/graphics/Paint$Join;")),
        Join::Bevel => cached_static!(BEVEL, jsget!("android/graphics/Paint$Join", "BEVEL", "Landroid/graphics/Paint$Join;")),
    }
}
/// `Path.FillType.WINDING` / `EVEN_ODD`.
pub fn fill_type_obj(f: FillType) -> JResult<Obj> {
    match f {
        FillType::Winding => cached_static!(WINDING, jsget!("android/graphics/Path$FillType", "WINDING", "Landroid/graphics/Path$FillType;")),
        FillType::EvenOdd => cached_static!(EVEN_ODD, jsget!("android/graphics/Path$FillType", "EVEN_ODD", "Landroid/graphics/Path$FillType;")),
    }
}
/// `Bitmap.Config.ARGB_8888`.
pub fn argb_8888() -> JResult<Obj> {
    cached_static!(ARGB, jsget!("android/graphics/Bitmap$Config", "ARGB_8888", "Landroid/graphics/Bitmap$Config;"))
}

/// A new Java `android.graphics.Path` with `path`'s fill type and verbs.
pub fn path_to_java(path: &Path) -> JResult<Obj> {
    let p = jnew!("android/graphics/Path", "()V")?;
    if path.fill_type() != FillType::Winding {
        jcall!(p, "android/graphics/Path", "setFillType", "(Landroid/graphics/Path$FillType;)V", &fill_type_obj(path.fill_type())?)?;
    }
    for v in path.verbs() {
        match *v {
            Verb::Move(x, y) => jcall!(p, "android/graphics/Path", "moveTo", "(FF)V", x, y)?,
            Verb::Line(x, y) => jcall!(p, "android/graphics/Path", "lineTo", "(FF)V", x, y)?,
            Verb::Quad(x1, y1, x2, y2) => jcall!(p, "android/graphics/Path", "quadTo", "(FFFF)V", x1, y1, x2, y2)?,
            Verb::Cubic(x1, y1, x2, y2, x3, y3) => jcall!(p, "android/graphics/Path", "cubicTo", "(FFFFFF)V", x1, y1, x2, y2, x3, y3)?,
            Verb::Close => jcall!(p, "android/graphics/Path", "close", "()V")?,
        };
    }
    Ok(p)
}

/// `new RectF(l, t, r, b)`.
pub fn rect_to_java(r: &RectF) -> JResult<Obj> {
    jnew!("android/graphics/RectF", "(FFFF)V", r.left, r.top, r.right, r.bottom)
}
/// Reads a Java `RectF`.
pub fn rect_from_java(o: &Obj) -> JResult<RectF> {
    Ok(RectF {
        left: jget!(o, "android/graphics/RectF", "left", "F")?.float(),
        top: jget!(o, "android/graphics/RectF", "top", "F")?.float(),
        right: jget!(o, "android/graphics/RectF", "right", "F")?.float(),
        bottom: jget!(o, "android/graphics/RectF", "bottom", "F")?.float(),
    })
}
/// `Matrix.getValues(float[9])`.
fn matrix_values(matrix: &Obj) -> JResult<[f32; 9]> {
    let a = jx::new_float_array(&[0.0; 9])?;
    jcall!(matrix, "android/graphics/Matrix", "getValues", "([F)V", &a)?;
    let v = jx::float_array(a.as_obj());
    let mut out = [0.0; 9];
    out.copy_from_slice(&v[..9]);
    Ok(out)
}
/// Affine part of an Android matrix as a tiny-skia transform.
fn transform_from_values(v: &[f32; 9]) -> tiny_skia::Transform {
    // MSCALE_X, MSKEW_X, MTRANS_X, MSKEW_Y, MSCALE_Y, MTRANS_Y
    tiny_skia::Transform::from_row(v[0], v[3], v[1], v[4], v[2], v[5])
}
/// `canvas.getMatrix()` as a tiny-skia transform.
fn canvas_transform(canvas: &Obj) -> JResult<tiny_skia::Transform> {
    let m = jnew!("android/graphics/Matrix", "()V")?;
    jcall!(canvas, "android/graphics/Canvas", "getMatrix", "(Landroid/graphics/Matrix;)V", &m)?;
    Ok(transform_from_values(&matrix_values(&m)?))
}

/// A new `ARGB_8888` Java `Bitmap` holding the pixmap's premultiplied pixels.
fn pixmap_to_bitmap(pixmap: &tiny_skia::Pixmap) -> JResult<Obj> {
    let bitmap = jstatic!("android/graphics/Bitmap", "createBitmap", "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;", pixmap.width() as i32, pixmap.height() as i32, &argb_8888()?)?.some();
    let bytes = jx::new_byte_array(pixmap.data())?;
    let buffer = jstatic!("java/nio/ByteBuffer", "wrap", "([B)Ljava/nio/ByteBuffer;", &bytes)?.some();
    jcall!(bitmap, "android/graphics/Bitmap", "copyPixelsFromBuffer", "(Ljava/nio/Buffer;)V", &buffer)?;
    Ok(bitmap)
}
/// `bitmap.getPixels(...)` of the whole bitmap (unpremultiplied ARGB ints).
pub fn bitmap_pixels(bitmap: &Obj) -> JResult<ArgbPixels> {
    let w = jcall!(bitmap, "android/graphics/Bitmap", "getWidth", "()I")?.int();
    let h = jcall!(bitmap, "android/graphics/Bitmap", "getHeight", "()I")?.int();
    let a = jx::new_int_array(&vec![0; (w.max(0) * h.max(0)) as usize])?;
    jcall!(bitmap, "android/graphics/Bitmap", "getPixels", "([IIIIIII)V", &a, 0, w, 0, 0, w, h)?;
    Ok(ArgbPixels(jx::int_array(a.as_obj())))
}

impl Canvas for AndroidCanvas {
    fn save(&mut self) -> SaveCount {
        SaveCount(ok(jcall!(self.canvas, "android/graphics/Canvas", "save", "()I")).int())
    }
    fn restore(&mut self) {
        ok(jcall!(self.canvas, "android/graphics/Canvas", "restore", "()V"));
    }
    fn restore_to_count(&mut self, count: SaveCount) {
        ok(jcall!(self.canvas, "android/graphics/Canvas", "restoreToCount", "(I)V", count.0));
    }
    fn translate(&mut self, offset: Translation) {
        ok(jcall!(self.canvas, "android/graphics/Canvas", "translate", "(FF)V", offset.dx, offset.dy));
    }
    fn rotate(&mut self, degrees: Rotation) {
        ok(jcall!(self.canvas, "android/graphics/Canvas", "rotate", "(F)V", degrees.0));
    }
    fn scale(&mut self, factors: ScaleFactors) {
        ok(jcall!(self.canvas, "android/graphics/Canvas", "scale", "(FF)V", factors.sx, factors.sy));
    }
    fn matrix(&self) -> CanvasMatrix {
        CanvasMatrix(ok(canvas_transform(&self.canvas)))
    }
    fn clip_rect(&mut self, rect: RectF) {
        ok(jcall!(self.canvas, "android/graphics/Canvas", "clipRect", "(FFFF)Z", rect.left, rect.top, rect.right, rect.bottom));
    }
    fn clip_out_path(&mut self, path: &Path) {
        let p = ok(path_to_java(path));
        ok(jcall!(self.canvas, "android/graphics/Canvas", "clipOutPath", "(Landroid/graphics/Path;)Z", &p));
    }
    fn draw_color(&mut self, color: ArgbColor) {
        ok(jcall!(self.canvas, "android/graphics/Canvas", "drawColor", "(I)V", color));
    }
    fn draw_path(&mut self, path: &Path, paint: &Paint) {
        let p = ok(path_to_java(path));
        let jp = ok(self.java_paint(paint));
        ok(jcall!(self.canvas, "android/graphics/Canvas", "drawPath", "(Landroid/graphics/Path;Landroid/graphics/Paint;)V", &p, &jp));
    }
    fn draw_line(&mut self, line: CanvasSegment, paint: &Paint) {
        let jp = ok(self.java_paint(paint));
        ok(jcall!(self.canvas, "android/graphics/Canvas", "drawLine", "(FFFFLandroid/graphics/Paint;)V", line.start.x, line.start.y, line.end.x, line.end.y, &jp));
    }
    fn draw_rect(&mut self, rect: RectF, paint: &Paint) {
        let jp = ok(self.java_paint(paint));
        ok(jcall!(self.canvas, "android/graphics/Canvas", "drawRect", "(FFFFLandroid/graphics/Paint;)V", rect.left, rect.top, rect.right, rect.bottom, &jp));
    }
    fn draw_oval(&mut self, rect: RectF, paint: &Paint) {
        let jp = ok(self.java_paint(paint));
        ok(jcall!(self.canvas, "android/graphics/Canvas", "drawOval", "(FFFFLandroid/graphics/Paint;)V", rect.left, rect.top, rect.right, rect.bottom, &jp));
    }
    fn draw_bitmap(&mut self, bitmap: &Bitmap, dst: RectF, paint: &Paint) {
        let b = ok(pixmap_to_bitmap(&bitmap.0));
        if self.bitmap_paint.is_none() {
            self.bitmap_paint = Some(ok(jnew!("android/graphics/Paint", "(I)V", PAINT_FILTER_BITMAP_FLAG)));
        }
        let bp = self.bitmap_paint.clone().unwrap();
        ok(jcall!(bp, "android/graphics/Paint", "setAlpha", "(I)V", ((paint.color.0 as u32) >> 24) as i32));
        let d = ok(rect_to_java(&dst));
        ok(jcall!(self.canvas, "android/graphics/Canvas", "drawBitmap", "(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/RectF;Landroid/graphics/Paint;)V", &b, jx::NULL, &d, &bp));
        ok(jcall!(b, "android/graphics/Bitmap", "recycle", "()V"));
    }
    /// androidsvg drew SVG vectors directly on the Canvas, at their true
    /// device positions. A raster made in its own pixel grid and drawn at a
    /// fractional device offset (e.g. an object at x=5.25) is resampled and
    /// visibly blurs thin or tiny shapes. For axis-aligned matrices (every
    /// editor, tile and thumbnail canvas) render resvg on the device pixel
    /// grid instead, limited to the canvas surface, and copy it 1:1.
    fn draw_svg(&mut self, svg: &SvgTree, size: CanvasSize) {
        let (tree, w, h) = (&svg.0, size.width, size.height);
        let m = self.matrix().0;
        if m.kx != 0.0 || m.ky != 0.0 || !(m.sx > 0.0 && m.sy > 0.0) || !(w > 0.0 && h > 0.0) {
            notemaker_cli::canvas::draw_svg_raster(self, svg, size);
            return;
        }
        let cw = ok(jcall!(self.canvas, "android/graphics/Canvas", "getWidth", "()I")).int() as f32;
        let ch = ok(jcall!(self.canvas, "android/graphics/Canvas", "getHeight", "()I")).int() as f32;
        let (x0, y0) = (m.tx, m.ty);
        let (x1, y1) = (m.tx + w * m.sx, m.ty + h * m.sy);
        let left = x0.floor().max(0.0);
        let top = y0.floor().max(0.0);
        let right = if cw > 0.0 { x1.ceil().min(cw) } else { x1.ceil() };
        let bottom = if ch > 0.0 { y1.ceil().min(ch) } else { y1.ceil() };
        if right <= left || bottom <= top {
            return;
        }
        if right - left > 8192.0 || bottom - top > 8192.0 {
            notemaker_cli::canvas::draw_svg_raster(self, svg, size);
            return;
        }
        let Some(mut pixmap) = tiny_skia::Pixmap::new((right - left) as u32, (bottom - top) as u32) else {
            return;
        };
        let size = tree.size();
        let transform = tiny_skia::Transform::from_row(m.sx, 0.0, 0.0, m.sy, m.tx - left, m.ty - top)
            .pre_scale(w / size.width(), h / size.height());
        resvg::render(tree, transform, &mut pixmap.as_mut());
        let b = ok(pixmap_to_bitmap(&pixmap));
        static UNFILTERED: OnceLock<Obj> = OnceLock::new();
        let paint = UNFILTERED.get_or_init(|| ok(jnew!("android/graphics/Paint", "()V")));
        // Local coordinates of the device-pixel box, so the bitmap maps 1:1.
        let dst = RectF { left: (left - m.tx) / m.sx, top: (top - m.ty) / m.sy, right: (right - m.tx) / m.sx, bottom: (bottom - m.ty) / m.sy };
        let d = ok(rect_to_java(&dst));
        ok(jcall!(self.canvas, "android/graphics/Canvas", "drawBitmap", "(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/RectF;Landroid/graphics/Paint;)V", &b, jx::NULL, &d, paint));
        ok(jcall!(b, "android/graphics/Bitmap", "recycle", "()V"));
    }
}
