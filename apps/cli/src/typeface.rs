//! Text for `Png` and `ItemCoverage`: replaces AWT `Font` and Android
//! `Typeface`/`Paint.getTextPath` with ab_glyph outlines from files.
//!
//! Font lookup: `NOTEMAKER_FONT` (kept from `Png.ensureFonts`, regular face
//! only), then the `<dir>` entries of `FONTCONFIG_FILE` (DejaVu from
//! `shell.nix`) or the default fontconfig file on the host, then
//! `/system/fonts` on Android.

use crate::canvas::{CanvasPoint, Path};
use ab_glyph::{Font, OutlineCurve, VariableFont};
use notemaker_core::style::{FontFamily, TextContent};
use notemaker_core::{Error, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// `Typeface.create(family, style)`: one loaded font face.
pub struct Typeface {
    font: ab_glyph::FontArc,
    /// File the face was loaded from (diagnostics).
    pub source: PathBuf,
}

/// The weight and slant of a face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FaceStyle {
    pub bold: bool,
    pub italic: bool,
}

impl FaceStyle {
    pub const REGULAR: FaceStyle = FaceStyle { bold: false, italic: false };
}

/// A font size in canvas units (`Paint.setTextSize`).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct TextSize(pub f32);

/// The advance width of a run of text in canvas units (`Paint.measureText`).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct TextAdvance(pub f32);

/// The shared resvg font database for SVG `<text>` (see [`svg_fontdb`]).
#[derive(Clone)]
pub struct SvgFonts(pub Arc<resvg::usvg::fontdb::Database>);

type FaceKey = (u8, bool, bool);

fn faces() -> &'static Mutex<HashMap<FaceKey, Arc<Typeface>>> {
    static FACES: OnceLock<Mutex<HashMap<FaceKey, Arc<Typeface>>>> = OnceLock::new();
    FACES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn family_index(family: &str) -> u8 {
    match family {
        "serif" => 1,
        "monospace" => 2,
        _ => 0,
    }
}

/// Candidate file names: host DejaVu first, then Android system fonts.
fn candidates(family: u8, bold: bool, italic: bool) -> Vec<&'static str> {
    let style = (bold as usize) * 2 + italic as usize;
    let pick = |names: [&'static str; 4]| names[style];
    match family {
        1 => vec![
            pick([
                "DejaVuSerif.ttf",
                "DejaVuSerif-Italic.ttf",
                "DejaVuSerif-Bold.ttf",
                "DejaVuSerif-BoldItalic.ttf",
            ]),
            pick([
                "NotoSerif-Regular.ttf",
                "NotoSerif-Italic.ttf",
                "NotoSerif-Bold.ttf",
                "NotoSerif-BoldItalic.ttf",
            ]),
            "DejaVuSerif.ttf",
            "NotoSerif-Regular.ttf",
            "NotoSerif.ttf",
        ],
        2 => vec![
            pick([
                "DejaVuSansMono.ttf",
                "DejaVuSansMono-Oblique.ttf",
                "DejaVuSansMono-Bold.ttf",
                "DejaVuSansMono-BoldOblique.ttf",
            ]),
            "DejaVuSansMono.ttf",
            "DroidSansMono.ttf",
            "CutiveMono.ttf",
        ],
        _ => vec![
            pick([
                "DejaVuSans.ttf",
                "DejaVuSans-Oblique.ttf",
                "DejaVuSans-Bold.ttf",
                "DejaVuSans-BoldOblique.ttf",
            ]),
            pick([
                "Roboto-Regular.ttf",
                "Roboto-Italic.ttf",
                "Roboto-Bold.ttf",
                "Roboto-BoldItalic.ttf",
            ]),
            "DejaVuSans.ttf",
            "Roboto-Regular.ttf",
            "Roboto.ttf",
        ],
    }
}

fn find_file(name: &str) -> Option<PathBuf> {
    fn walk(dir: &std::path::Path, name: &str, depth: usize) -> Option<PathBuf> {
        let direct = dir.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        if depth == 0 {
            return None;
        }
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .collect();
        entries.sort();
        entries.iter().find_map(|d| walk(d, name, depth - 1))
    }
    font_dirs().iter().find_map(|d| walk(d, name, 4))
}

fn load(path: &std::path::Path, bold: bool) -> Option<Typeface> {
    let bytes = std::fs::read(path).ok()?;
    let mut font = ab_glyph::FontVec::try_from_vec(bytes).ok()?;
    if bold && font.variations().iter().any(|a| &a.tag == b"wght") {
        font.set_variation(b"wght", 700.0);
    }
    Some(Typeface {
        font: ab_glyph::FontArc::new(font),
        source: path.to_path_buf(),
    })
}

impl Typeface {
    /// `family` is `"serif"`, `"monospace"` or anything else (sans-serif),
    /// matching the Java mapping. Faces are cached per (family, bold, italic).
    pub fn create(family: &FontFamily, style: FaceStyle) -> Result<Arc<Typeface>> {
        let FaceStyle { bold, italic } = style;
        let key = (family_index(family), bold, italic);
        let mut cache = faces().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(face) = cache.get(&key) {
            return Ok(face.clone());
        }
        let mut face = None;
        if let Ok(path) = std::env::var("NOTEMAKER_FONT") {
            face = load(std::path::Path::new(&path), bold);
        }
        if face.is_none() {
            for name in candidates(key.0, bold, italic)
                .into_iter()
                .chain(candidates(0, false, false))
            {
                if let Some(found) = find_file(name).and_then(|p| load(&p, bold)) {
                    face = Some(found);
                    break;
                }
            }
        }
        let face =
            Arc::new(face.ok_or_else(|| Error::illegal_state("Could not initialize fonts"))?);
        cache.insert(key, face.clone());
        Ok(face)
    }

    fn scale(&self, size: f32) -> f32 {
        size / self.font.units_per_em().unwrap_or(2048.0)
    }

    /// `Paint.measureText` / `FontMetrics.stringWidth`: advance width at `size`.
    pub fn measure_text(&self, text: &TextContent, size: TextSize) -> TextAdvance {
        let s = self.scale(size.0);
        TextAdvance(
            text.chars()
                .map(|c| self.font.h_advance_unscaled(self.font.glyph_id(c)) * s)
                .sum(),
        )
    }

    /// `Paint.getTextPath(text, 0, len, x, y, path)` / `GlyphVector.getOutline`:
    /// glyph outlines with the baseline at `y` (non-zero fill).
    pub fn text_path(&self, text: &TextContent, baseline: CanvasPoint, size: TextSize) -> Path {
        let CanvasPoint { x, y } = baseline;
        let s = self.scale(size.0);
        let mut path = Path::new();
        let mut pen = x;
        for c in text.chars() {
            let id = self.font.glyph_id(c);
            if let Some(outline) = self.font.outline(id) {
                let map = |p: ab_glyph::Point| (pen + p.x * s, y - p.y * s);
                let mut end: Option<ab_glyph::Point> = None;
                for curve in &outline.curves {
                    let start = match curve {
                        OutlineCurve::Line(a, _)
                        | OutlineCurve::Quad(a, _, _)
                        | OutlineCurve::Cubic(a, _, _, _) => *a,
                    };
                    if end != Some(start) {
                        if end.is_some() {
                            path.close();
                        }
                        let (sx, sy) = map(start);
                        path.move_to(CanvasPoint { x: sx, y: sy });
                    }
                    match curve {
                        OutlineCurve::Line(_, b) => {
                            let (bx, by) = map(*b);
                            path.line_to(CanvasPoint { x: bx, y: by });
                            end = Some(*b);
                        }
                        OutlineCurve::Quad(_, b, c2) => {
                            let (bx, by) = map(*b);
                            let (cx, cy) = map(*c2);
                            path.quad_to(CanvasPoint { x: bx, y: by }, CanvasPoint { x: cx, y: cy });
                            end = Some(*c2);
                        }
                        OutlineCurve::Cubic(_, b, c2, d) => {
                            let (bx, by) = map(*b);
                            let (cx, cy) = map(*c2);
                            let (dx, dy) = map(*d);
                            path.cubic_to(
                                CanvasPoint { x: bx, y: by },
                                CanvasPoint { x: cx, y: cy },
                                CanvasPoint { x: dx, y: dy },
                            );
                            end = Some(*d);
                        }
                    }
                }
                if end.is_some() {
                    path.close();
                }
            }
            pen += self.font.h_advance_unscaled(id) * s;
        }
        path
    }
}

/// `Png.ensureFonts()`: fails with `IllegalState("Could not initialize fonts")`
/// when no sans-serif face can be found.
pub fn ensure_fonts() -> Result<()> {
    Typeface::create(&FontFamily::from("sans-serif"), FaceStyle::REGULAR).map(|_| ())
}

fn fontconfig_dirs(file: &std::path::Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    let Ok(doc) = roxmltree::Document::parse_with_options(
        &text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    ) else {
        return;
    };
    let base = file.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    for node in doc.descendants() {
        let Some(value) = node.text().map(str::trim).filter(|t| !t.is_empty()) else {
            continue;
        };
        let resolve = |v: &str| {
            if let Some(rest) = v.strip_prefix("~/") {
                std::env::var("HOME")
                    .map(|h| PathBuf::from(h).join(rest))
                    .unwrap_or_else(|_| PathBuf::from(v))
            } else if v.starts_with('/') {
                PathBuf::from(v)
            } else {
                base.join(v)
            }
        };
        match node.tag_name().name() {
            "dir" => out.push(resolve(value)),
            "include" if depth > 0 => {
                let target = resolve(value);
                if target.is_dir() {
                    let mut files: Vec<PathBuf> = std::fs::read_dir(&target)
                        .into_iter()
                        .flatten()
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| p.extension().is_some_and(|e| e == "conf"))
                        .collect();
                    files.sort();
                    for f in files {
                        fontconfig_dirs(&f, out, depth - 1);
                    }
                } else {
                    fontconfig_dirs(&target, out, depth - 1);
                }
            }
            _ => {}
        }
    }
}

/// Directories searched for font files, in order.
fn font_dirs() -> Vec<PathBuf> {
    static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    DIRS.get_or_init(|| {
        let mut dirs = Vec::new();
        let config = std::env::var("FONTCONFIG_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/etc/fonts/fonts.conf"));
        fontconfig_dirs(&config, &mut dirs, 3);
        for extra in [
            "/system/fonts",
            "/usr/share/fonts",
            "/run/current-system/sw/share/X11/fonts",
        ] {
            dirs.push(PathBuf::from(extra));
        }
        let mut seen = std::collections::HashSet::new();
        dirs.retain(|d| d.is_dir() && seen.insert(d.clone()));
        dirs
    })
    .clone()
}

/// Shared resvg font database built from [`font_dirs`] (SVG `<text>`).
pub fn svg_fontdb() -> SvgFonts {
    static DB: OnceLock<SvgFonts> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        // Only the faces Png itself uses: /system/fonts is large on Android.
        let mut files: Vec<PathBuf> = std::env::var("NOTEMAKER_FONT")
            .map(PathBuf::from)
            .into_iter()
            .collect();
        let mut seen = std::collections::HashSet::new();
        for family in 0..3u8 {
            for style in 0..4u8 {
                for name in candidates(family, style >= 2, style % 2 == 1) {
                    if seen.insert(name) {
                        files.extend(find_file(name));
                    }
                }
            }
        }
        for file in files {
            if let Ok(bytes) = std::fs::read(&file) {
                db.load_font_data(bytes);
            }
        }
        let names: std::collections::HashSet<String> = db
            .faces()
            .flat_map(|f| f.families.iter().map(|(n, _)| n.clone()))
            .collect();
        let choose = |options: &[&str]| {
            options
                .iter()
                .find(|n| names.contains(**n))
                .map(|n| n.to_string())
        };
        let (sans, serif, mono) = (
            choose(&["DejaVu Sans", "Roboto"]),
            choose(&["DejaVu Serif", "Noto Serif"]),
            choose(&["DejaVu Sans Mono", "Droid Sans Mono", "Cutive Mono"]),
        );
        if let Some(n) = sans {
            db.set_sans_serif_family(n);
        }
        if let Some(n) = serif {
            db.set_serif_family(n);
        }
        if let Some(n) = mono {
            db.set_monospace_family(n);
        }
        SvgFonts(Arc::new(db))
    })
    .clone()
}
