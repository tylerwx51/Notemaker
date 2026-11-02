//! Port of `Png.java` (JVM and Android variants collapsed): the raster
//! `Render.Surface`, PNG export, coverage tiles and stroke command recording.
//!
//! `Png` draws through `&mut dyn Canvas`, exactly like the Android
//! `Png(Canvas)`. The CLI uses [`RasterCanvas`](crate::raster::RasterCanvas);
//! the Android app wraps its JNI `android.graphics.Canvas` in its own
//! [`Canvas`] implementation and uses `Png::new(&mut that)` wherever Java
//! wrote `new com.notemaker.cli.Png(canvas)`.

use crate::area::Area;
use crate::canvas::{Canvas, Cap, FillType, Join, Paint, Path, RectF, Style};
use crate::item_coverage;
use crate::raster::RasterCanvas;
use crate::typeface::Typeface;
use indexmap::IndexMap;
use notemaker_core::geometry::Polygon;
use notemaker_core::model::Model;
use notemaker_core::render::{self, Surface};
use notemaker_core::svg_asset::{self, DotGrid};
use notemaker_core::{Error, Map, Result};
use resvg::{tiny_skia, usvg};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

/// `Png.useSvgPictures`: cache rendered SVG objects per (source, w, h)
/// (Android `Picture` cache; on tiny-skia, a cached raster at device scale).
pub static USE_SVG_PICTURES: AtomicBool = AtomicBool::new(true);
/// `Png.useDirectDots`: draw `SvgAsset.dotGrid` templates as circles instead
/// of through resvg.
pub static USE_DIRECT_DOTS: AtomicBool = AtomicBool::new(true);
/// `Png.traceSvg`: log SVG timing lines (`NoteMakerStartup` tag on Android,
/// stderr elsewhere).
pub static TRACE_SVG: AtomicBool = AtomicBool::new(false);

/// One recorded canvas call of a compiled stroke (`StrokeCommands.Command`).
/// Replaying applies each to a `Canvas` with a caller-supplied `Paint`, as
/// `Png.replayStroke` did; a JNI backend can also translate them directly.
#[derive(Clone, Debug, PartialEq)]
pub enum StrokeCommand {
    /// `canvas.save(); for(path) canvas.clipOutPath(path);` (`beginErase`).
    SaveClipOut(Vec<Path>),
    /// `canvas.restore()` (`endErase`).
    Restore,
    /// `paint.setColor(color); setStyle(FILL); canvas.drawPath(path)`.
    FillPath { path: Path, color: i32 },
    /// `paint.setColor(color); setStyle(STROKE); setStrokeWidth(width);
    /// setStrokeJoin(ROUND); canvas.drawPath(path)`.
    StrokePath { path: Path, color: i32, width: f32 },
    /// `paint.setColor(color); setStyle(STROKE); setStrokeWidth(width);
    /// canvas.drawLine(x0, y0, x1, y1)`.
    Line {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        color: i32,
        width: f32,
    },
}

/// `Png.StrokeCommands`: an immutable, cheaply cloned stroke recording.
#[derive(Clone, Debug)]
pub struct StrokeCommands {
    commands: Arc<[StrokeCommand]>,
}

impl StrokeCommands {
    /// The recorded calls, in order (for non-`Canvas` replays).
    pub fn commands(&self) -> &[StrokeCommand] {
        &self.commands
    }
}

/// `Png`: `Render.Surface` over a [`Canvas`], or a stroke recorder.
pub struct Png<'a> {
    target: Target<'a>,
    paint: Paint,
    clips: Vec<i32>,
    transforms: Vec<i32>,
}

enum Target<'a> {
    Canvas(&'a mut dyn Canvas),
    Recorder(Vec<StrokeCommand>),
}

/// `SVGS` (parsed trees, LRU 16). Keyed by (source, w, h) because the
/// normalized `100%` viewport resolves against the object box.
type SvgKey = (String, u32, u32);
fn svgs() -> &'static Mutex<IndexMap<SvgKey, Arc<usvg::Tree>>> {
    static SVGS: OnceLock<Mutex<IndexMap<SvgKey, Arc<usvg::Tree>>>> = OnceLock::new();
    SVGS.get_or_init(|| Mutex::new(IndexMap::new()))
}

const IMAGE_CACHE_LIMIT: usize = 24 * 1024 * 1024;
const IMAGE_CACHE_KEY_LIMIT: usize = 4 * 1024 * 1024;

/// `IMAGES` with the Android byte and key-length budgets.
#[derive(Default)]
struct ImageCache {
    images: IndexMap<String, Arc<tiny_skia::Pixmap>>,
    bytes: usize,
    key_chars: usize,
}

fn images() -> &'static Mutex<ImageCache> {
    static IMAGES: OnceLock<Mutex<ImageCache>> = OnceLock::new();
    IMAGES.get_or_init(|| Mutex::new(ImageCache::default()))
}

fn trace(line: String) {
    if TRACE_SVG.load(Ordering::Relaxed) {
        eprintln!("NoteMakerStartup: {}", line);
    }
}

fn ms(since: Instant) -> String {
    notemaker_core::jfmt::double(since.elapsed().as_nanos() as f64 / 1e6)
}

/// Premultiplied pixmap from straight RGBA bytes.
fn pixmap_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<tiny_skia::Pixmap> {
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    for (dst, src) in pixmap.pixels_mut().iter_mut().zip(rgba.chunks_exact(4)) {
        *dst = tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3]).premultiply();
    }
    Some(pixmap)
}

fn decode_png(bytes: &[u8]) -> Option<tiny_skia::Pixmap> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let data = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => return None,
    };
    pixmap_from_rgba(info.width, info.height, &rgba)
}

fn decode_jpeg(bytes: &[u8]) -> Option<tiny_skia::Pixmap> {
    let mut decoder = jpeg_decoder::Decoder::new(bytes);
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let rgba: Vec<u8> = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        jpeg_decoder::PixelFormat::L16 => pixels
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], 255])
            .collect(),
        jpeg_decoder::PixelFormat::CMYK32 => pixels
            .chunks_exact(4)
            .flat_map(|p| {
                let k = 255 - p[3] as u32;
                let f = |c: u8| ((255 - c as u32) * k / 255) as u8;
                [f(p[0]), f(p[1]), f(p[2]), 255]
            })
            .collect(),
    };
    pixmap_from_rgba(info.width as u32, info.height as u32, &rgba)
}

/// First GIF frame composited on its logical screen (`Movie` at time 0).
fn decode_gif(bytes: &[u8]) -> Option<tiny_skia::Pixmap> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(bytes).ok()?;
    let (sw, sh) = (decoder.width() as u32, decoder.height() as u32);
    let frame = decoder.read_next_frame().ok()??;
    let mut rgba = vec![0u8; (sw * sh * 4) as usize];
    for row in 0..frame.height as u32 {
        for col in 0..frame.width as u32 {
            let (x, y) = (frame.left as u32 + col, frame.top as u32 + row);
            if x >= sw || y >= sh {
                continue;
            }
            let s = ((row * frame.width as u32 + col) * 4) as usize;
            let d = ((y * sw + x) * 4) as usize;
            rgba[d..d + 4].copy_from_slice(&frame.buffer[s..s + 4]);
        }
    }
    pixmap_from_rgba(sw, sh, &rgba)
}

fn decode_image(bytes: &[u8]) -> Option<tiny_skia::Pixmap> {
    if bytes.starts_with(b"\x89PNG") {
        decode_png(bytes)
    } else if bytes.starts_with(b"GIF8") {
        decode_gif(bytes)
    } else {
        decode_jpeg(bytes)
    }
}

fn base64_payload(data: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let comma = data.find(',').map(|i| i + 1).unwrap_or(0);
    let cleaned: String = data[comma..]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .map_err(|_| Error::illegal_argument("Could not render image"))
}

fn null_canvas() -> Error {
    Error::null_pointer()
}

/// `SvgAsset.viewport` writes `width="100%" height="100%"` plus a viewBox on
/// the root. androidsvg (`renderToCanvas(canvas, RectF(0,0,w,h))`) and jsvg
/// resolve those percentages against the target box and then fit the viewBox
/// with the root's `preserveAspectRatio` (default `xMidYMid meet`). usvg
/// resolves them against the viewBox instead, so the tree would be the viewBox
/// size and the later `w/size`, `h/size` scale would stretch it. Writing the
/// target size into the root tag lets usvg apply `preserveAspectRatio`
/// itself, and the tree size is exactly `w`x`h`.
fn fit_root_size(normalized: &str, w: f64, h: f64) -> String {
    if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
        return normalized.to_string();
    }
    let Some(start) = normalized.find("<svg") else {
        return normalized.to_string();
    };
    let Some(end) = normalized[start..].find('>').map(|e| start + e) else {
        return normalized.to_string();
    };
    let tag = normalized[start..end]
        .replacen(" width=\"100%\"", &format!(" width=\"{}\"", notemaker_core::jfmt::double(w)), 1)
        .replacen(" height=\"100%\"", &format!(" height=\"{}\"", notemaker_core::jfmt::double(h)), 1);
    format!("{}{}{}", &normalized[..start], tag, &normalized[end..])
}

impl<'a> Png<'a> {
    /// `new Png(canvas)`: anti-aliased paint with round caps.
    pub fn new(canvas: &'a mut dyn Canvas) -> Png<'a> {
        Png {
            target: Target::Canvas(canvas),
            paint: Png::base_paint(),
            clips: Vec::new(),
            transforms: Vec::new(),
        }
    }

    fn base_paint() -> Paint {
        Paint {
            cap: Cap::Round,
            ..Paint::anti_alias()
        }
    }

    fn canvas(&mut self) -> Result<&mut dyn Canvas> {
        match &mut self.target {
            Target::Canvas(c) => Ok(&mut **c),
            Target::Recorder(_) => Err(null_canvas()),
        }
    }

    /// `Png.coverageTile(m, item, x, y, size)`: ARGB pixels of `item`
    /// painted into a transparent `size`×`size` tile at (x, y).
    pub fn coverage_tile(m: &Model, item: &Map, x: i32, y: i32, size: i32) -> Result<Vec<i32>> {
        let mut image = RasterCanvas::new(size as u32, size as u32)?;
        image.translate(-x as f32, -y as f32);
        render::draw_item(m, item, &mut Png::new(&mut image))?;
        Ok(image.argb_pixels())
    }

    /// `Png.export(m, page, x, y, w, h, out)`: white background, viewport at
    /// (x, y), PNG written to `out`.
    pub fn export(
        m: &Model,
        page: i32,
        x: f64,
        y: f64,
        w: i32,
        h: i32,
        out: &std::path::Path,
    ) -> Result<()> {
        let mut image = RasterCanvas::new(w as u32, h as u32)?;
        image.draw_color(0xffff_ffffu32 as i32);
        image.translate(-x as f32, -y as f32);
        render::draw(m, page, &HashSet::new(), &mut Png::new(&mut image))?;
        let bytes = image.encode_png()?;
        std::fs::write(out, bytes).map_err(|e| {
            Error::io(format!(
                "{} ({})",
                out.display(),
                notemaker_core::error::io_message(&e)
            ))
        })
    }

    /// `Png.compileStroke(stroke)`: record `Render.drawStroke` once.
    pub fn compile_stroke(stroke: &Map) -> Result<StrokeCommands> {
        let mut recorder = Png {
            target: Target::Recorder(Vec::new()),
            paint: Png::base_paint(),
            clips: Vec::new(),
            transforms: Vec::new(),
        };
        render::draw_stroke(stroke, &mut recorder)?;
        match recorder.target {
            Target::Recorder(commands) => Ok(StrokeCommands {
                commands: commands.into(),
            }),
            Target::Canvas(_) => unreachable!(),
        }
    }

    /// `Png.replayStroke(canvas, commands, paint)`: `paint` supplies
    /// anti-aliasing and cap; commands set color, style, width and join.
    pub fn replay_stroke(canvas: &mut dyn Canvas, commands: &StrokeCommands, paint: &mut Paint) {
        for command in commands.commands() {
            match command {
                StrokeCommand::SaveClipOut(paths) => {
                    canvas.save();
                    for p in paths {
                        canvas.clip_out_path(p);
                    }
                }
                StrokeCommand::Restore => canvas.restore(),
                StrokeCommand::FillPath { path, color } => {
                    paint.color = *color;
                    paint.style = Style::Fill;
                    canvas.draw_path(path, paint);
                }
                StrokeCommand::StrokePath { path, color, width } => {
                    paint.color = *color;
                    paint.style = Style::Stroke;
                    paint.stroke_width = *width;
                    paint.join = Join::Round;
                    canvas.draw_path(path, paint);
                }
                StrokeCommand::Line {
                    x0,
                    y0,
                    x1,
                    y1,
                    color,
                    width,
                } => {
                    paint.color = *color;
                    paint.style = Style::Stroke;
                    paint.stroke_width = *width;
                    canvas.draw_line(*x0, *y0, *x1, *y1, paint);
                }
            }
        }
    }

    /// `Png.ensureFonts()`; see [`crate::typeface::ensure_fonts`].
    pub fn ensure_fonts() -> Result<()> {
        crate::typeface::ensure_fonts()
    }

    /// Record or draw a fill.
    fn fill_path(&mut self, path: Path, color: i32) -> Result<()> {
        match &mut self.target {
            Target::Recorder(commands) => commands.push(StrokeCommand::FillPath { path, color }),
            Target::Canvas(canvas) => {
                self.paint.color = color;
                self.paint.style = Style::Fill;
                canvas.draw_path(&path, &self.paint);
            }
        }
        Ok(())
    }

    fn draw_dots(canvas: &mut dyn Canvas, w: f64, h: f64, dots: &DotGrid) {
        canvas.save();
        let scale = (w / dots.width).min(h / dots.height);
        canvas.translate(
            ((w - dots.width * scale) / 2.0) as f32,
            ((h - dots.height * scale) / 2.0) as f32,
        );
        canvas.scale(scale as f32, scale as f32);
        let fill = Paint {
            color: dots.color,
            style: Style::Fill,
            ..Paint::anti_alias()
        };
        let alpha = notemaker_core::jmath::round(dots.stroke_opacity * 255.0) as u32 & 255;
        let stroke = Paint {
            color: ((dots.color as u32 & 0x00ff_ffff) | (alpha << 24)) as i32,
            style: Style::Stroke,
            stroke_width: dots.stroke_width as f32,
            ..Paint::anti_alias()
        };
        let circles = dots.circles();
        for c in circles.chunks_exact(3) {
            let (cx, cy, r) = (c[0] as f32, c[1] as f32, c[2] as f32);
            let (left, top, right, bottom, k) =
                (cx - r, cy - r, cx + r, cy + r, r * 0.552_284_8f32);
            let mut circle = Path::new();
            circle.move_to(cx, top);
            circle.cubic_to(cx + k, top, right, cy - k, right, cy);
            circle.cubic_to(right, cy + k, cx + k, bottom, cx, bottom);
            circle.cubic_to(cx - k, bottom, left, cy + k, left, cy);
            circle.cubic_to(left, cy - k, cx - k, top, cx, top);
            circle.close();
            canvas.draw_path(&circle, &fill);
            canvas.draw_path(&circle, &stroke);
        }
        canvas.restore();
    }

    fn svg_tree(source: &str, w: f64, h: f64) -> Result<Arc<usvg::Tree>> {
        let key = (
            source.to_string(),
            (w as f32).to_bits(),
            (h as f32).to_bits(),
        );
        let mut cache = svgs().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tree) = cache.shift_remove(&key) {
            cache.insert(key, tree.clone());
            return Ok(tree);
        }
        let started = Instant::now();
        let normalized = fit_root_size(&svg_asset::viewport(source)?, w, h);
        let validated = Instant::now();
        let options = usvg::Options {
            default_size: usvg::Size::from_wh(w as f32, h as f32)
                .unwrap_or(usvg::Size::from_wh(100.0, 100.0).expect("valid size")),
            fontdb: crate::typeface::svg_fontdb(),
            ..usvg::Options::default()
        };
        let tree = Arc::new(
            usvg::Tree::from_str(&normalized, &options)
                .map_err(|_| Error::illegal_argument("Could not render SVG"))?,
        );
        trace(format!(
            "svg_source_bytes={} svg_validate_ms={} svg_parse_ms={}",
            source.len(),
            notemaker_core::jfmt::double((validated - started).as_nanos() as f64 / 1e6),
            ms(validated)
        ));
        cache.insert(key, tree.clone());
        if cache.len() > 16 {
            cache.shift_remove_index(0);
        }
        Ok(tree)
    }

    fn cached_image(data: &str) -> Result<Arc<tiny_skia::Pixmap>> {
        let mut cache = images().lock().unwrap_or_else(|e| e.into_inner());
        let cacheable = data.len() <= 1_000_000;
        if cacheable {
            if let Some(image) = cache.images.shift_remove(data) {
                cache.images.insert(data.to_string(), image.clone());
                return Ok(image);
            }
        }
        let bytes = base64_payload(data)?;
        let image = Arc::new(
            decode_image(&bytes)
                .ok_or_else(|| Error::illegal_argument("Could not decode image pixels"))?,
        );
        let used = image.data().len();
        if cacheable && used <= IMAGE_CACHE_LIMIT && data.len() <= IMAGE_CACHE_KEY_LIMIT {
            while (cache.bytes + used > IMAGE_CACHE_LIMIT
                || cache.key_chars + data.len() > IMAGE_CACHE_KEY_LIMIT)
                && !cache.images.is_empty()
            {
                if let Some((key, eldest)) = cache.images.shift_remove_index(0) {
                    cache.bytes -= eldest.data().len();
                    cache.key_chars -= key.len();
                }
            }
            cache.images.insert(data.to_string(), image.clone());
            cache.bytes += used;
            cache.key_chars += data.len();
        }
        Ok(image)
    }
}

impl Surface for Png<'_> {
    fn begin_erase(&mut self, polygons: &[Polygon]) -> Result<()> {
        let paths: Vec<Path> = polygons
            .iter()
            .map(|p| item_coverage::path(p, true))
            .collect();
        match &mut self.target {
            Target::Recorder(commands) => commands.push(StrokeCommand::SaveClipOut(paths)),
            Target::Canvas(canvas) => {
                self.clips.push(canvas.save());
                for p in &paths {
                    canvas.clip_out_path(p);
                }
            }
        }
        Ok(())
    }
    fn end_erase(&mut self) -> Result<()> {
        match &mut self.target {
            Target::Recorder(commands) => commands.push(StrokeCommand::Restore),
            Target::Canvas(canvas) => {
                let count = self.clips.pop().ok_or_else(|| {
                    Error::new(notemaker_core::ErrorKind::IllegalState, "Deque empty")
                })?;
                canvas.restore_to_count(count);
            }
        }
        Ok(())
    }
    fn polygon(
        &mut self,
        points: &[[f64; 2]],
        color: i32,
        width: f64,
        fill: Option<i32>,
        closed: bool,
    ) -> Result<()> {
        let p = item_coverage::path(points, closed);
        if let Some(fill) = fill {
            self.fill_path(p.clone(), fill)?;
        }
        if width > 0.0 {
            let width = width as f32;
            match &mut self.target {
                Target::Recorder(commands) => commands.push(StrokeCommand::StrokePath {
                    path: p,
                    color,
                    width,
                }),
                Target::Canvas(canvas) => {
                    self.paint.color = color;
                    self.paint.style = Style::Stroke;
                    self.paint.stroke_width = width;
                    self.paint.join = Join::Round;
                    canvas.draw_path(&p, &self.paint);
                }
            }
        }
        Ok(())
    }
    fn ink(&mut self, passes: &[Polygon], color: i32) -> Result<()> {
        if passes.len() == 1 {
            return self.polygon(&passes[0], color, 0.0, Some(color), true);
        }
        if (color as u32 >> 24) == 255 {
            let mut union = Path::new();
            union.set_fill_type(FillType::Winding);
            for points in passes {
                union.add_path(&item_coverage::path(points, true));
            }
            return self.fill_path(union, color);
        }
        // Layer k contains pixels visited at least k+1 times. Unioning before
        // rasterization prevents antialias cracks at artificial pass boundaries.
        let mut layers: Vec<Area> = Vec::new();
        for points in passes {
            let mut carry = Area::of(&item_coverage::path(points, true));
            for layer in layers.iter_mut() {
                let overlap = layer.intersect(&carry);
                *layer = layer.union(&carry);
                carry = overlap;
                if carry.is_empty() {
                    break;
                }
            }
            if !carry.is_empty() {
                layers.push(carry);
            }
        }
        for layer in &layers {
            self.fill_path(layer.to_path(), color)?;
        }
        Ok(())
    }
    fn line(&mut self, x: f64, y: f64, ex: f64, ey: f64, color: i32, width: f64) -> Result<()> {
        let (x0, y0, x1, y1, width) = (x as f32, y as f32, ex as f32, ey as f32, width as f32);
        match &mut self.target {
            Target::Recorder(commands) => commands.push(StrokeCommand::Line {
                x0,
                y0,
                x1,
                y1,
                color,
                width,
            }),
            Target::Canvas(canvas) => {
                self.paint.color = color;
                self.paint.style = Style::Stroke;
                self.paint.stroke_width = width;
                canvas.draw_line(x0, y0, x1, y1, &self.paint);
            }
        }
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
        let mut paint = self.paint;
        paint.join = Join::Miter;
        let canvas = self.canvas()?;
        let r = RectF::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32);
        let draw = |canvas: &mut dyn Canvas, paint: &Paint| {
            if ellipse {
                canvas.draw_oval(r, paint)
            } else {
                canvas.draw_rect(r, paint)
            }
        };
        if let Some(fill) = fill {
            paint.color = fill;
            paint.style = Style::Fill;
            draw(canvas, &paint);
        }
        if width > 0.0 {
            paint.color = color;
            paint.style = Style::Stroke;
            paint.stroke_width = width as f32;
            draw(canvas, &paint);
        }
        self.paint = paint;
        Ok(())
    }
    fn text(&mut self, x: f64, y: f64, text: &str, color: i32, size: f64) -> Result<()> {
        let face = Typeface::create("sans-serif", false, false)?;
        let path = face.text_path(text, x as f32, y as f32, size as f32);
        let mut paint = self.paint;
        paint.color = color;
        paint.style = Style::Fill;
        self.canvas()?.draw_path(&path, &paint);
        self.paint = paint;
        Ok(())
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
        let face = Typeface::create(family, bold, italic)?;
        let width = face.measure_text(text, size as f32);
        let draw_x = x as f32
            - if alignment == "center" {
                width / 2.0
            } else if alignment == "right" {
                width
            } else {
                0.0
            };
        let path = face.text_path(text, draw_x, y as f32, size as f32);
        let mut paint = self.paint;
        paint.color = color;
        paint.style = Style::Fill;
        let canvas = self.canvas()?;
        canvas.draw_path(&path, &paint);
        if underline {
            let line = Paint {
                color,
                style: Style::Stroke,
                stroke_width: 1.0,
                cap: Cap::Square,
                ..paint
            };
            canvas.draw_line(
                draw_x,
                (y + 2.0) as f32,
                draw_x + width,
                (y + 2.0) as f32,
                &line,
            );
        }
        self.paint = paint;
        Ok(())
    }
    fn image(&mut self, x: f64, y: f64, w: f64, h: f64, data: &str) -> Result<()> {
        let image = Png::cached_image(data)?;
        let paint = Paint {
            color: 0xffff_ffffu32 as i32,
            ..self.paint
        };
        self.canvas()?.draw_bitmap(
            &image,
            RectF::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32),
            &paint,
        );
        Ok(())
    }
    fn svg(&mut self, x: f64, y: f64, w: f64, h: f64, source: &str) -> Result<()> {
        let canvas = self.canvas()?;
        if USE_DIRECT_DOTS.load(Ordering::Relaxed) {
            let begun = Instant::now();
            if let Some(dots) = svg_asset::dot_grid(source)? {
                canvas.save();
                canvas.translate(x as f32, y as f32);
                canvas.clip_rect(0.0, 0.0, w as f32, h as f32);
                Png::draw_dots(canvas, w, h, &dots);
                canvas.restore();
                trace(format!(
                    "svg_direct_dots_ms={} svg_source_bytes={}",
                    ms(begun),
                    source.len()
                ));
                return Ok(());
            }
        }
        let tree = Png::svg_tree(source, w, h)?;
        if !(w > 0.0 && h > 0.0) {
            return Ok(());
        }
        canvas.save();
        canvas.translate(x as f32, y as f32);
        canvas.clip_rect(0.0, 0.0, w as f32, h as f32);
        canvas.draw_svg(&tree, w as f32, h as f32);
        canvas.restore();
        Ok(())
    }
    fn save_transform(
        &mut self,
        cx: f64,
        cy: f64,
        rotation: f64,
        flip_x: bool,
        flip_y: bool,
    ) -> Result<()> {
        let canvas = self.canvas()?;
        let count = canvas.save();
        canvas.translate(cx as f32, cy as f32);
        canvas.rotate(rotation as f32);
        canvas.scale(
            if flip_x { -1.0 } else { 1.0 },
            if flip_y { -1.0 } else { 1.0 },
        );
        canvas.translate(-cx as f32, -cy as f32);
        self.transforms.push(count);
        Ok(())
    }
    fn restore_transform(&mut self) -> Result<()> {
        let count = self
            .transforms
            .pop()
            .ok_or_else(|| Error::new(notemaker_core::ErrorKind::IllegalState, "Deque empty"))?;
        self.canvas()?.restore_to_count(count);
        Ok(())
    }
}
