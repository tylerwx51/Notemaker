//! Port of `Png.java` (JVM and Android variants collapsed): the raster
//! `Render.Surface`, PNG export, coverage tiles and stroke command recording.
//!
//! `Png` draws through `&mut dyn Canvas`, exactly like the Android
//! `Png(Canvas)`. The CLI uses [`RasterCanvas`](crate::raster::RasterCanvas);
//! the Android app wraps its JNI `android.graphics.Canvas` in its own
//! [`Canvas`] implementation and uses `Png::new(&mut that)` wherever Java
//! wrote `new com.notemaker.cli.Png(canvas)`.

use crate::area::Area;
use crate::canvas::{
    Bitmap, Canvas, CanvasPoint, CanvasSegment, CanvasSize, Cap, FillType, Join, Paint, Path,
    RectF, Rotation, SaveCount, ScaleFactors, Style, SvgTree, Translation,
};
use crate::item_coverage;
use crate::raster::{PixelSize, RasterCanvas};
use crate::typeface::{FaceStyle, TextSize, Typeface};
use indexmap::IndexMap;
use notemaker_core::coverage::CoverageTile;
use notemaker_core::geometry::{Degrees, DocPoint, DocRect, Erasures, InkPasses, Polygon, Segment};
use notemaker_core::ids::PageNumber;
use notemaker_core::model::Model;
use notemaker_core::render::{self, HiddenLayers, Surface};
use notemaker_core::style::{
    ArgbColor, ArgbPixels, Flip, FontFamily, FontSize, PathEnd, RectShape, ShapePaint, StrokeWidth,
    TextAlignment, TextContent, TextStyle,
};
use notemaker_core::svg_asset::{self, DotGrid};
use notemaker_core::{Error, Map, Result};
use resvg::{tiny_skia, usvg};
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
    FillPath { path: Path, color: ArgbColor },
    /// `paint.setColor(color); setStyle(STROKE); setStrokeWidth(width);
    /// setStrokeJoin(ROUND); canvas.drawPath(path)`.
    StrokePath { path: Path, color: ArgbColor, width: f32 },
    /// `paint.setColor(color); setStyle(STROKE); setStrokeWidth(width);
    /// canvas.drawLine(x0, y0, x1, y1)`.
    Line {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        color: ArgbColor,
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
    pub fn commands(&self) -> impl ExactSizeIterator<Item = &StrokeCommand> + '_ {
        self.commands.iter()
    }
}

/// `Png`: `Render.Surface` over a [`Canvas`], or a stroke recorder.
pub struct Png<'a> {
    target: Target<'a>,
    paint: Paint,
    clips: Vec<SaveCount>,
    transforms: Vec<SaveCount>,
}

enum Target<'a> {
    Canvas(&'a mut dyn Canvas),
    Recorder(Vec<StrokeCommand>),
}

/// `SVGS` (parsed trees, LRU 16). Keyed by (source, w, h) because the
/// normalized `100%` viewport resolves against the object box.
type SvgKey = (String, u32, u32);
fn svgs() -> &'static Mutex<IndexMap<SvgKey, Arc<SvgTree>>> {
    static SVGS: OnceLock<Mutex<IndexMap<SvgKey, Arc<SvgTree>>>> = OnceLock::new();
    SVGS.get_or_init(|| Mutex::new(IndexMap::new()))
}

const IMAGE_CACHE_LIMIT: usize = 24 * 1024 * 1024;
const IMAGE_CACHE_KEY_LIMIT: usize = 4 * 1024 * 1024;

/// `IMAGES` with the Android byte and key-length budgets.
#[derive(Default)]
struct ImageCache {
    images: IndexMap<String, Arc<Bitmap>>,
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
    pub fn coverage_tile(m: &Model, item: &Map, tile: CoverageTile) -> Result<ArgbPixels> {
        let CoverageTile { x, y, size } = tile;
        let mut image = RasterCanvas::new(PixelSize { width: size as u32, height: size as u32 })?;
        image.translate(Translation { dx: -x as f32, dy: -y as f32 });
        render::draw_item(m, item, &mut Png::new(&mut image))?;
        Ok(image.argb_pixels())
    }

    /// `Png.export(m, page, x, y, w, h, out)`: white background, viewport
    /// origin at `origin`, `size` pixels, PNG written to `out`.
    pub fn export(
        m: &Model,
        page: PageNumber,
        origin: DocPoint,
        size: PixelSize,
        out: &std::path::Path,
    ) -> Result<()> {
        let DocPoint { x, y } = origin;
        let mut image = RasterCanvas::new(size)?;
        image.draw_color(ArgbColor::WHITE);
        image.translate(Translation { dx: -x as f32, dy: -y as f32 });
        render::draw(m, page, &HiddenLayers::default(), &mut Png::new(&mut image))?;
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
                    canvas.draw_line(
                        CanvasSegment {
                            start: CanvasPoint { x: *x0, y: *y0 },
                            end: CanvasPoint { x: *x1, y: *y1 },
                        },
                        paint,
                    );
                }
            }
        }
    }

    /// `Png.ensureFonts()`; see [`crate::typeface::ensure_fonts`].
    pub fn ensure_fonts() -> Result<()> {
        crate::typeface::ensure_fonts()
    }

    /// Record or draw a fill.
    fn fill_path(&mut self, path: Path, color: ArgbColor) -> Result<()> {
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
        canvas.translate(Translation {
            dx: ((w - dots.width * scale) / 2.0) as f32,
            dy: ((h - dots.height * scale) / 2.0) as f32,
        });
        canvas.scale(ScaleFactors { sx: scale as f32, sy: scale as f32 });
        let fill = Paint {
            color: ArgbColor(dots.color),
            style: Style::Fill,
            ..Paint::anti_alias()
        };
        let alpha = notemaker_core::jmath::round(dots.stroke_opacity * 255.0) as u32 & 255;
        let stroke = Paint {
            color: ArgbColor(((dots.color as u32 & 0x00ff_ffff) | (alpha << 24)) as i32),
            style: Style::Stroke,
            stroke_width: dots.stroke_width as f32,
            ..Paint::anti_alias()
        };
        let p = |x: f32, y: f32| CanvasPoint { x, y };
        for c in dots.circles() {
            let (cx, cy, r) = (c.center.x as f32, c.center.y as f32, c.radius.0 as f32);
            let (left, top, right, bottom, k) =
                (cx - r, cy - r, cx + r, cy + r, r * 0.552_284_8f32);
            let mut circle = Path::new();
            circle.move_to(p(cx, top));
            circle.cubic_to(p(cx + k, top), p(right, cy - k), p(right, cy));
            circle.cubic_to(p(right, cy + k), p(cx + k, bottom), p(cx, bottom));
            circle.cubic_to(p(cx - k, bottom), p(left, cy + k), p(left, cy));
            circle.cubic_to(p(left, cy - k), p(cx - k, top), p(cx, top));
            circle.close();
            canvas.draw_path(&circle, &fill);
            canvas.draw_path(&circle, &stroke);
        }
        canvas.restore();
    }

    fn svg_tree(source: &str, w: f64, h: f64) -> Result<Arc<SvgTree>> {
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
            fontdb: crate::typeface::svg_fontdb().0,
            ..usvg::Options::default()
        };
        let tree = Arc::new(SvgTree(
            usvg::Tree::from_str(&normalized, &options)
                .map_err(|_| Error::illegal_argument("Could not render SVG"))?,
        ));
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

    fn cached_image(data: &str) -> Result<Arc<Bitmap>> {
        let mut cache = images().lock().unwrap_or_else(|e| e.into_inner());
        let cacheable = data.len() <= 1_000_000;
        if cacheable {
            if let Some(image) = cache.images.shift_remove(data) {
                cache.images.insert(data.to_string(), image.clone());
                return Ok(image);
            }
        }
        let bytes = base64_payload(data)?;
        let image = Arc::new(Bitmap(
            decode_image(&bytes)
                .ok_or_else(|| Error::illegal_argument("Could not decode image pixels"))?,
        ));
        let used = image.0.data().len();
        if cacheable && used <= IMAGE_CACHE_LIMIT && data.len() <= IMAGE_CACHE_KEY_LIMIT {
            while (cache.bytes + used > IMAGE_CACHE_LIMIT
                || cache.key_chars + data.len() > IMAGE_CACHE_KEY_LIMIT)
                && !cache.images.is_empty()
            {
                if let Some((key, eldest)) = cache.images.shift_remove_index(0) {
                    cache.bytes -= eldest.0.data().len();
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
    fn begin_erase(&mut self, erasures: &Erasures) -> Result<()> {
        let paths: Vec<Path> = erasures
            .iter()
            .map(|p| item_coverage::path(p, PathEnd::Closed))
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
    fn polygon(&mut self, points: &Polygon, paint: ShapePaint, end: PathEnd) -> Result<()> {
        let ShapePaint { color, width, fill } = paint;
        let width = width.0;
        let p = item_coverage::path(points, end);
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
    fn ink(&mut self, passes: &InkPasses, color: ArgbColor) -> Result<()> {
        if passes.len() == 1 {
            return self.polygon(
                &passes[0],
                ShapePaint { color, width: StrokeWidth(0.0), fill: Some(color) },
                PathEnd::Closed,
            );
        }
        if (color.0 as u32 >> 24) == 255 {
            let mut union = Path::new();
            union.set_fill_type(FillType::Winding);
            for points in passes {
                union.add_path(&item_coverage::path(points, PathEnd::Closed));
            }
            return self.fill_path(union, color);
        }
        // Layer k contains pixels visited at least k+1 times. Unioning before
        // rasterization prevents antialias cracks at artificial pass boundaries.
        let mut layers: Vec<Area> = Vec::new();
        for points in passes {
            let mut carry = Area::of(&item_coverage::path(points, PathEnd::Closed));
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
    fn line(&mut self, segment: Segment, color: ArgbColor, width: StrokeWidth) -> Result<()> {
        let Segment { start, end } = segment;
        let (x0, y0, x1, y1, width) = (
            start.x as f32,
            start.y as f32,
            end.x as f32,
            end.y as f32,
            width.0 as f32,
        );
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
                canvas.draw_line(
                    CanvasSegment {
                        start: CanvasPoint { x: x0, y: y0 },
                        end: CanvasPoint { x: x1, y: y1 },
                    },
                    &self.paint,
                );
            }
        }
        Ok(())
    }
    fn rect(&mut self, rect: DocRect, paint: ShapePaint, shape: RectShape) -> Result<()> {
        let DocRect { x, y, width: w, height: h } = rect;
        let ShapePaint { color, width, fill } = paint;
        let width = width.0;
        let ellipse = shape == RectShape::Ellipse;
        let mut paint = self.paint;
        paint.join = Join::Miter;
        let canvas = self.canvas()?;
        let r = RectF {
            left: x as f32,
            top: y as f32,
            right: (x + w) as f32,
            bottom: (y + h) as f32,
        };
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
    fn text(&mut self, origin: DocPoint, text: &TextContent, color: ArgbColor, size: FontSize) -> Result<()> {
        let face = Typeface::create(&FontFamily::from("sans-serif"), FaceStyle::REGULAR)?;
        let path = face.text_path(
            text,
            CanvasPoint { x: origin.x as f32, y: origin.y as f32 },
            TextSize(size.0 as f32),
        );
        let mut paint = self.paint;
        paint.color = color;
        paint.style = Style::Fill;
        self.canvas()?.draw_path(&path, &paint);
        self.paint = paint;
        Ok(())
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
        let face = Typeface::create(family, FaceStyle { bold: style.bold, italic: style.italic })?;
        let width = face.measure_text(text, TextSize(size as f32)).0;
        let draw_x = x as f32
            - if alignment == "center" {
                width / 2.0
            } else if alignment == "right" {
                width
            } else {
                0.0
            };
        let path = face.text_path(text, CanvasPoint { x: draw_x, y: y as f32 }, TextSize(size as f32));
        let mut paint = self.paint;
        paint.color = color;
        paint.style = Style::Fill;
        let canvas = self.canvas()?;
        canvas.draw_path(&path, &paint);
        if style.underline {
            let line = Paint {
                color,
                style: Style::Stroke,
                stroke_width: 1.0,
                cap: Cap::Square,
                ..paint
            };
            canvas.draw_line(
                CanvasSegment {
                    start: CanvasPoint { x: draw_x, y: (y + 2.0) as f32 },
                    end: CanvasPoint { x: draw_x + width, y: (y + 2.0) as f32 },
                },
                &line,
            );
        }
        self.paint = paint;
        Ok(())
    }
    fn image(&mut self, rect: DocRect, data: &str) -> Result<()> {
        let DocRect { x, y, width: w, height: h } = rect;
        let image = Png::cached_image(data)?;
        let paint = Paint {
            color: ArgbColor::WHITE,
            ..self.paint
        };
        self.canvas()?.draw_bitmap(
            &image,
            RectF {
                left: x as f32,
                top: y as f32,
                right: (x + w) as f32,
                bottom: (y + h) as f32,
            },
            &paint,
        );
        Ok(())
    }
    fn svg(&mut self, rect: DocRect, source: &str) -> Result<()> {
        let DocRect { x, y, width: w, height: h } = rect;
        let canvas = self.canvas()?;
        let clip = RectF { left: 0.0, top: 0.0, right: w as f32, bottom: h as f32 };
        if USE_DIRECT_DOTS.load(Ordering::Relaxed) {
            let begun = Instant::now();
            if let Some(dots) = svg_asset::dot_grid(source)? {
                canvas.save();
                canvas.translate(Translation { dx: x as f32, dy: y as f32 });
                canvas.clip_rect(clip);
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
        canvas.translate(Translation { dx: x as f32, dy: y as f32 });
        canvas.clip_rect(clip);
        canvas.draw_svg(&tree, CanvasSize { width: w as f32, height: h as f32 });
        canvas.restore();
        Ok(())
    }
    fn save_transform(&mut self, center: DocPoint, rotation: Degrees, flip: Flip) -> Result<()> {
        let DocPoint { x: cx, y: cy } = center;
        let canvas = self.canvas()?;
        let count = canvas.save();
        canvas.translate(Translation { dx: cx as f32, dy: cy as f32 });
        canvas.rotate(Rotation(rotation.0 as f32));
        canvas.scale(ScaleFactors {
            sx: if flip.horizontal { -1.0 } else { 1.0 },
            sy: if flip.vertical { -1.0 } else { 1.0 },
        });
        canvas.translate(Translation { dx: -cx as f32, dy: -cy as f32 });
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
