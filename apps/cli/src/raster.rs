//! tiny-skia [`Canvas`] backend: the CLI's replacement for the JVM
//! `BufferedImage`/`Graphics2D` pair and the Android CLI's `Bitmap`/`Canvas`.

use crate::canvas::{Canvas, Cap, Join, Paint, Path, RectF, Style};
use notemaker_core::{Error, Result};
use resvg::{tiny_skia, usvg};
use std::sync::Arc;

/// An ARGB raster with an Android-style save stack (matrix + clip mask).
pub struct RasterCanvas {
    pixmap: tiny_skia::Pixmap,
    matrix: tiny_skia::Transform,
    clip: Option<Arc<tiny_skia::Mask>>,
    saved: Vec<(tiny_skia::Transform, Option<Arc<tiny_skia::Mask>>)>,
}

fn color_of(argb: i32) -> tiny_skia::Color {
    let c = argb as u32;
    tiny_skia::Color::from_rgba8((c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8)
}

fn skia_paint(paint: &Paint) -> tiny_skia::Paint<'static> {
    let mut p = tiny_skia::Paint::default();
    p.set_color(color_of(paint.color));
    p.anti_alias = paint.anti_alias;
    p
}

fn skia_stroke(paint: &Paint) -> tiny_skia::Stroke {
    tiny_skia::Stroke {
        width: paint.stroke_width,
        miter_limit: 4.0,
        line_cap: match paint.cap {
            Cap::Butt => tiny_skia::LineCap::Butt,
            Cap::Round => tiny_skia::LineCap::Round,
            Cap::Square => tiny_skia::LineCap::Square,
        },
        line_join: match paint.join {
            Join::Miter => tiny_skia::LineJoin::Miter,
            Join::Round => tiny_skia::LineJoin::Round,
            Join::Bevel => tiny_skia::LineJoin::Bevel,
        },
        dash: None,
    }
}

impl RasterCanvas {
    /// `Bitmap.createBitmap(w, h, ARGB_8888)`: transparent pixels.
    pub fn new(width: u32, height: u32) -> Result<RasterCanvas> {
        let pixmap = tiny_skia::Pixmap::new(width, height)
            .ok_or_else(|| Error::illegal_argument("width and height must be > 0"))?;
        Ok(RasterCanvas {
            pixmap,
            matrix: tiny_skia::Transform::identity(),
            clip: None,
            saved: Vec::new(),
        })
    }
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }
    pub fn pixmap(&self) -> &tiny_skia::Pixmap {
        &self.pixmap
    }
    pub fn into_pixmap(self) -> tiny_skia::Pixmap {
        self.pixmap
    }
    /// `getRGB`/`getPixels`: unpremultiplied ARGB ints, row-major.
    pub fn argb_pixels(&self) -> Vec<i32> {
        self.pixmap
            .pixels()
            .iter()
            .map(|p| {
                let c = p.demultiply();
                (((c.alpha() as u32) << 24)
                    | ((c.red() as u32) << 16)
                    | ((c.green() as u32) << 8)
                    | c.blue() as u32) as i32
            })
            .collect()
    }
    /// PNG bytes (RGBA, unpremultiplied), as `ImageIO.write(image, "png", …)`.
    pub fn encode_png(&self) -> Result<Vec<u8>> {
        self.pixmap
            .encode_png()
            .map_err(|e| Error::io(format!("PNG encoding failed: {}", e)))
    }

    fn mask(&self) -> Option<&tiny_skia::Mask> {
        self.clip.as_deref()
    }

    fn full_mask(&self) -> tiny_skia::Mask {
        let mut m = tiny_skia::Mask::new(self.pixmap.width(), self.pixmap.height())
            .expect("non-empty canvas");
        m.data_mut().fill(255);
        m
    }

    fn fill(&mut self, path: &Path, paint: &Paint) {
        if let Some((p, rule)) = path.to_skia() {
            let sp = skia_paint(paint);
            let clip = self.clip.clone();
            self.pixmap
                .fill_path(&p, &sp, rule, self.matrix, clip.as_deref());
        }
    }

    fn stroke(&mut self, path: &Path, paint: &Paint) {
        if let Some((p, _)) = path.to_skia() {
            let sp = skia_paint(paint);
            let clip = self.clip.clone();
            self.pixmap
                .stroke_path(&p, &sp, &skia_stroke(paint), self.matrix, clip.as_deref());
        }
    }

    fn paint_path(&mut self, path: &Path, paint: &Paint) {
        match paint.style {
            Style::Fill => self.fill(path, paint),
            Style::Stroke => self.stroke(path, paint),
        }
    }
}

impl Canvas for RasterCanvas {
    fn save(&mut self) -> i32 {
        let count = self.saved.len() as i32 + 1;
        self.saved.push((self.matrix, self.clip.clone()));
        count
    }
    fn restore(&mut self) {
        if let Some((m, c)) = self.saved.pop() {
            self.matrix = m;
            self.clip = c;
        }
    }
    fn restore_to_count(&mut self, count: i32) {
        while self.saved.len() as i32 + 1 > count.max(1) {
            self.restore();
        }
    }
    fn translate(&mut self, dx: f32, dy: f32) {
        self.matrix = self.matrix.pre_translate(dx, dy);
    }
    fn rotate(&mut self, degrees: f32) {
        self.matrix = self
            .matrix
            .pre_concat(tiny_skia::Transform::from_rotate(degrees));
    }
    fn scale(&mut self, sx: f32, sy: f32) {
        self.matrix = self.matrix.pre_scale(sx, sy);
    }
    fn matrix(&self) -> tiny_skia::Transform {
        self.matrix
    }
    fn clip_rect(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        let mut path = Path::new();
        path.add_rect(RectF::new(left, top, right, bottom));
        let mut mask = match self.clip.take() {
            Some(m) => Arc::try_unwrap(m).unwrap_or_else(|m| (*m).clone()),
            None => self.full_mask(),
        };
        match path.to_skia() {
            Some((p, rule)) => mask.intersect_path(&p, rule, true, self.matrix),
            None => mask.clear(),
        }
        self.clip = Some(Arc::new(mask));
    }
    fn clip_out_path(&mut self, path: &Path) {
        let Some((p, rule)) = path.to_skia() else {
            return;
        };
        let mut out = tiny_skia::Mask::new(self.pixmap.width(), self.pixmap.height())
            .expect("non-empty canvas");
        out.fill_path(&p, rule, true, self.matrix);
        out.invert();
        let mask = match self.clip.take() {
            Some(m) => {
                let mut m = Arc::try_unwrap(m).unwrap_or_else(|m| (*m).clone());
                for (d, o) in m.data_mut().iter_mut().zip(out.data()) {
                    *d = ((*d as u32 * *o as u32 + 127) / 255) as u8;
                }
                m
            }
            None => out,
        };
        self.clip = Some(Arc::new(mask));
    }
    fn draw_color(&mut self, color: i32) {
        let (w, h) = (self.pixmap.width() as f32, self.pixmap.height() as f32);
        let mut sp = tiny_skia::Paint::default();
        sp.set_color(color_of(color));
        sp.anti_alias = false;
        if let Some(rect) = tiny_skia::Rect::from_xywh(0.0, 0.0, w, h) {
            let clip = self.clip.clone();
            self.pixmap
                .fill_rect(rect, &sp, tiny_skia::Transform::identity(), clip.as_deref());
        }
    }
    fn draw_path(&mut self, path: &Path, paint: &Paint) {
        self.paint_path(path, paint);
    }
    fn draw_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, paint: &Paint) {
        let mut path = Path::new();
        path.move_to(x0, y0);
        path.line_to(x1, y1);
        // A line is always stroked (Canvas.drawLine ignores FILL style).
        self.stroke(&path, paint);
    }
    fn draw_rect(&mut self, rect: RectF, paint: &Paint) {
        let mut path = Path::new();
        path.add_rect(rect);
        self.paint_path(&path, paint);
    }
    fn draw_oval(&mut self, rect: RectF, paint: &Paint) {
        let mut path = Path::new();
        path.add_oval(rect);
        self.paint_path(&path, paint);
    }
    fn draw_bitmap(&mut self, bitmap: &tiny_skia::Pixmap, dst: RectF, paint: &Paint) {
        let Some(rect) = tiny_skia::Rect::from_ltrb(dst.left, dst.top, dst.right, dst.bottom)
        else {
            return;
        };
        let sx = (dst.right - dst.left) / bitmap.width() as f32;
        let sy = (dst.bottom - dst.top) / bitmap.height() as f32;
        let mut sp = tiny_skia::Paint::default();
        sp.anti_alias = paint.anti_alias;
        sp.shader = tiny_skia::Pattern::new(
            bitmap.as_ref(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Nearest,
            1.0,
            tiny_skia::Transform::from_row(sx, 0.0, 0.0, sy, dst.left, dst.top),
        );
        let clip = self.clip.clone();
        self.pixmap
            .fill_rect(rect, &sp, self.matrix, clip.as_deref());
    }
    fn draw_svg(&mut self, tree: &usvg::Tree, w: f32, h: f32) {
        let size = tree.size();
        let transform = self.matrix.pre_scale(w / size.width(), h / size.height());
        match self.mask() {
            None => resvg::render(tree, transform, &mut self.pixmap.as_mut()),
            Some(_) => {
                // resvg has no clip-mask parameter: render to a layer, then
                // composite through the mask.
                let Some(mut layer) =
                    tiny_skia::Pixmap::new(self.pixmap.width(), self.pixmap.height())
                else {
                    return;
                };
                resvg::render(tree, transform, &mut layer.as_mut());
                let clip = self.clip.clone();
                self.pixmap.draw_pixmap(
                    0,
                    0,
                    layer.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    tiny_skia::Transform::identity(),
                    clip.as_deref(),
                );
            }
        }
    }
}
