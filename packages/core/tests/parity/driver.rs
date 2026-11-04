//! Rust half of the Java/Rust parity harness; mirrors `Parity.java` line by
//! line so both print identical canonical sections. Shared by the `parity`
//! example (used by `parity.sh`) and the `parity_snapshot` integration test.
#![allow(dead_code)]

use notemaker_core::clock::EpochMillis;
use notemaker_core::collaboration::{self, SelectionTransform};
use notemaker_core::coverage::{Coverage, CoverageFactory, CoverageTile, Painter, Tiles};
use notemaker_core::draw_item::DrawItem;
use notemaker_core::error::{Error, ErrorKind, Result};
use notemaker_core::geometry::{Bounds, DocPoint, DocRect, Polygon, Radius, Segment, Size};
use notemaker_core::gif_asset::FrameIndex;
use notemaker_core::ids::{ActorId, ConflictChoice, ConflictChoiceName, ConflictId, DocumentName, EntityId, EntityKind, LayerId, LayerPosition, PageNumber, SessionId, ShapeId};
use notemaker_core::ink_geometry::{AreaOverlap, InkGeometry, InkPart};
use notemaker_core::json::{self, Map, Value};
use notemaker_core::layer_editor::{HitMode, ItemFilter, LayerEditor};
use notemaker_core::model::{Entity, Model};
use notemaker_core::render::{self, HiddenLayers, Svg};
use notemaker_core::store::{EditorPreferences, HistoryDirection, ReplicaCursor, State, Store};
use notemaker_core::style::{ArgbPixels, RectShape, StrokeWidth};
use notemaker_core::{actions, drawing_actions, gif_asset, image_asset, jfmt, object, shape_catalog, svg_asset};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Driver {
    out: String,
    section: String,
    header: Option<String>,
    fixtures: PathBuf,
    seed: u64,
    scenes: HashMap<String, Model>,
    started: Option<std::time::Instant>,
}

fn d(v: f64) -> String {
    jfmt::double(v)
}

/// `Arrays.toString(double[])`.
fn arr(v: &[f64]) -> String {
    let parts: Vec<String> = v.iter().map(|x| d(*x)).collect();
    format!("[{}]", parts.join(", "))
}

fn arr2(v: &[[f64; 2]]) -> String {
    let parts: Vec<String> = v.iter().map(|p| arr(p)).collect();
    format!("[{}]", parts.join(", "))
}

/// `double[4]` form of bounds: `{left, top, right, bottom}`.
fn bounds4(b: Bounds) -> [f64; 4] {
    [b.left, b.top, b.right, b.bottom]
}

/// `double[][]` form of a polygon.
fn points(p: &Polygon) -> Vec<[f64; 2]> {
    p.iter().map(|q| [q.x, q.y]).collect()
}

fn poly(p: &Polygon) -> String {
    arr2(&points(p))
}

fn pt(x: f64, y: f64) -> DocPoint {
    DocPoint { x, y }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Polygon {
    DocRect { x, y, width, height }.polygon()
}

fn bounds(b: [f64; 4]) -> Bounds {
    Bounds { left: b[0], top: b[1], right: b[2], bottom: b[3] }
}

fn actor(s: &str) -> ActorId {
    ActorId::from(s)
}

fn err(e: &Error) -> String {
    let kind = match e.kind {
        ErrorKind::IllegalArgument | ErrorKind::NumberFormat => "IAE",
        ErrorKind::Io | ErrorKind::Http | ErrorKind::SslHandshake | ErrorKind::SslPeerUnverified => "IO",
        ErrorKind::UnsupportedOperation => "UNSUPPORTED",
        ErrorKind::IllegalState => "IllegalStateException",
        ErrorKind::ClassCast => "ClassCastException",
        ErrorKind::NullPointer => "NullPointerException",
        ErrorKind::IndexOutOfBounds => "IndexOutOfBoundsException",
        ErrorKind::OverlappingFileLock => "OverlappingFileLockException",
        ErrorKind::Arithmetic => "ArithmeticException",
    };
    format!("ERR {} {}", kind, e.message())
}

fn w(v: &Value) -> String {
    json::write(v)
}
fn wm(v: &Map) -> String {
    json::write_map(v)
}
fn wl(v: &[Value]) -> String {
    json::write_list(v)
}

fn canonical(text: &str) -> String {
    fn is_uuid(b: &[u8]) -> bool {
        if b.len() < 36 {
            return false;
        }
        for (i, c) in b[..36].iter().enumerate() {
            let ok = if matches!(i, 8 | 13 | 18 | 23) { *c == b'-' } else { c.is_ascii_digit() || (b'a'..=b'f').contains(c) };
            if !ok {
                return false;
            }
        }
        true
    }
    let bytes = text.as_bytes();
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut start = 0;
    while i < bytes.len() {
        if is_uuid(&bytes[i..]) {
            out.push_str(&text[start..i]);
            let id = &text[i..i + 36];
            let n = ids.len();
            let r = ids.entry(id.to_string()).or_insert_with(|| format!("<id-{}>", n)).clone();
            out.push_str(&r);
            i += 36;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&text[start..]);
    out
}

fn read_text(p: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(p).unwrap()).into_owned()
}

fn read_lines(p: &Path) -> Vec<String> {
    let text = read_text(p);
    let mut lines: Vec<String> = text.split('\n').map(|s| s.strip_suffix('\r').unwrap_or(s).to_string()).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

fn sorted_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    files
}

fn file_name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

/// Display value used by `Parity.show`.
pub enum Shown {
    Str(String),
    Json(Value),
}

impl From<String> for Shown {
    fn from(s: String) -> Shown {
        Shown::Str(s)
    }
}
impl From<&str> for Shown {
    fn from(s: &str) -> Shown {
        Shown::Str(s.to_string())
    }
}
impl From<Value> for Shown {
    fn from(v: Value) -> Shown {
        Shown::Json(v)
    }
}
impl From<Map> for Shown {
    fn from(v: Map) -> Shown {
        Shown::Json(Value::Object(v))
    }
}
impl From<Vec<Value>> for Shown {
    fn from(v: Vec<Value>) -> Shown {
        Shown::Json(Value::Array(v))
    }
}
macro_rules! shown_json {
    ($($t:ty),*) => {$(
        impl From<$t> for Shown {
            fn from(v: $t) -> Shown {
                Shown::Json(Value::from(v))
            }
        }
    )*};
}
shown_json!(
    notemaker_core::wire::Operation,
    notemaker_core::wire::OperationList,
    notemaker_core::wire::EntityRefList,
    notemaker_core::wire::ConflictList,
    notemaker_core::wire::ModelJson,
    notemaker_core::store::CommitResult,
    notemaker_core::store::HistoryResult,
    notemaker_core::store::AcceptResult,
    notemaker_core::store::ReplicaPage,
    notemaker_core::store::DeleteResult,
    notemaker_core::store::SyncResult,
    notemaker_core::collaboration::LockedEntityList,
    notemaker_core::journal::CheckpointResult
);
impl From<Vec<notemaker_core::wire::Operation>> for Shown {
    fn from(v: Vec<notemaker_core::wire::Operation>) -> Shown {
        Shown::Json(Value::Array(v.into_iter().map(Value::from).collect()))
    }
}
impl From<bool> for Shown {
    fn from(v: bool) -> Shown {
        Shown::Json(Value::Bool(v))
    }
}
impl From<[f64; 2]> for Shown {
    fn from(v: [f64; 2]) -> Shown {
        Shown::Str(arr(&v))
    }
}
impl From<[f64; 4]> for Shown {
    fn from(v: [f64; 4]) -> Shown {
        Shown::Str(arr(&v))
    }
}
impl From<Polygon> for Shown {
    fn from(v: Polygon) -> Shown {
        Shown::Str(poly(&v))
    }
}
impl From<Bounds> for Shown {
    fn from(v: Bounds) -> Shown {
        Shown::Str(arr(&bounds4(v)))
    }
}
impl From<Size> for Shown {
    fn from(v: Size) -> Shown {
        Shown::Str(arr(&[v.width, v.height]))
    }
}
impl From<Segment> for Shown {
    fn from(v: Segment) -> Shown {
        Shown::Str(arr(&[v.start.x, v.start.y, v.end.x, v.end.y]))
    }
}
impl From<State> for Shown {
    fn from(v: State) -> Shown {
        Shown::Json(Value::from(v.into_map()))
    }
}

fn show(v: Shown) -> String {
    match v {
        Shown::Str(s) => s,
        Shown::Json(v) => match v {
            Value::String(s) => s,
            other => w(&other),
        },
    }
}

/// The deterministic test coverage factory shared with `Parity.java`.
pub struct TestFactory;

struct FnPainter<F: Fn(i32, i32, i32) -> Vec<i32> + Send + Sync>(F);
impl<F: Fn(i32, i32, i32) -> Vec<i32> + Send + Sync> Painter for FnPainter<F> {
    fn paint(&self, tile: CoverageTile) -> notemaker_core::Result<ArgbPixels> {
        Ok(ArgbPixels((self.0)(tile.x, tile.y, tile.size)))
    }
}

impl CoverageFactory for TestFactory {
    fn create(&self, local: &notemaker_core::wire::EntityContent) -> Result<Box<dyn Coverage>> {
        let width = json::number(local, "width")?;
        if local.get("kind").and_then(Value::as_str) == Some("stroke") {
            let mut pts: Vec<[f64; 2]> = Vec::new();
            for v in json::list_at(local, "points")? {
                let p = json::list(v)?;
                pts.push([json::as_double(p.first())?, json::as_double(p.get(1))?]);
            }
            let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            for p in &pts {
                b[0] = b[0].min(p[0]);
                b[1] = b[1].min(p[1]);
                b[2] = b[2].max(p[0]);
                b[3] = b[3].max(p[1]);
            }
            b[0] -= width;
            b[1] -= width;
            b[2] += width;
            b[3] += width;
            let rr = width / 2.0 + 0.5;
            return Ok(Box::new(Tiles::new(
                bounds(b),
                Box::new(FnPainter(move |x, y, size| {
                    let mut px = vec![0i32; (size * size) as usize];
                    for j in 0..size {
                        for i in 0..size {
                            let cx = (x + i) as f64 + 0.5;
                            let cy = (y + j) as f64 + 0.5;
                            let mut hit = false;
                            let mut k = 0;
                            while k < pts.len() && !hit {
                                let a = pts[k];
                                let c = pts[(k + 1).min(pts.len() - 1)];
                                let dx = c[0] - a[0];
                                let dy = c[1] - a[1];
                                let q = dx * dx + dy * dy;
                                let t = if q == 0.0 { 0.0 } else { 0f64.max(1f64.min(((cx - a[0]) * dx + (cy - a[1]) * dy) / q)) };
                                let ex = cx - a[0] - t * dx;
                                let ey = cy - a[1] - t * dy;
                                hit = ex * ex + ey * ey <= rr * rr;
                                k += 1;
                            }
                            px[(j * size + i) as usize] = if hit { 0xff000000u32 as i32 } else { 0 };
                        }
                    }
                    px
                })),
            )));
        }
        let wd = json::number(local, "w")?;
        let h = json::number(local, "h")?;
        let b = [wd.min(0.0) - width, h.min(0.0) - width, wd.max(0.0) + width, h.max(0.0) + width];
        Ok(Box::new(Tiles::new(
            bounds(b),
            Box::new(FnPainter(move |x, y, size| {
                let mut px = vec![0i32; (size * size) as usize];
                for j in 0..size {
                    for i in 0..size {
                        let cx = (x + i) as f64 + 0.5;
                        let cy = (y + j) as f64 + 0.5;
                        let inside = cx >= wd.min(0.0) && cx <= wd.max(0.0) && cy >= h.min(0.0) && cy <= h.max(0.0);
                        px[(j * size + i) as usize] = if inside { 0xff000000u32 as i32 } else { 0 };
                    }
                }
                px
            })),
        )))
    }
}

static FACTORY: TestFactory = TestFactory;

fn gif_info(info: &gif_asset::Info) -> String {
    let ints = |v: &[i32]| format!("[{}]", v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "));
    format!(
        "{}x{} delays={} offsets={} duration={} t1={}",
        info.width,
        info.height,
        ints(&info.delays),
        ints(&info.offsets),
        info.duration,
        if info.offsets.len() > 1 { info.time_for_frame(FrameIndex(1)).0 } else { -1 }
    )
}

fn list_of(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn map_of(v: Option<&Value>) -> Map {
    v.and_then(Value::as_object).cloned().unwrap_or_default()
}

fn ints(v: &[i64]) -> Value {
    Value::Array(v.iter().map(|x| Value::from(*x)).collect())
}

fn point(x: f64, y: f64, p: f64) -> Value {
    Value::Array(vec![json::double(x), json::double(y), json::double(p)])
}

impl Driver {
    pub fn new(fixtures: &Path) -> Driver {
        Driver { out: String::new(), section: String::new(), header: None, fixtures: fixtures.to_path_buf(), seed: 0x9E3779B97F4A7C15, scenes: HashMap::new(), started: None }
    }

    pub fn finish(mut self) -> String {
        self.flush();
        self.out
    }

    fn begin(&mut self, name: &str) {
        self.flush();
        self.header = Some(name.to_string());
        self.started = Some(std::time::Instant::now());
    }
    fn timing(&self, started: std::time::Instant) {
        if std::env::var_os("PARITY_TIMING").is_some() {
            eprintln!("{:>8.1} ms  {}", started.elapsed().as_secs_f64() * 1000.0, self.header.as_deref().unwrap_or("?"));
        }
    }
    fn line(&mut self, text: &str) {
        self.section.push_str(text);
        self.section.push('\n');
    }
    fn flush(&mut self) {
        if let Some(started) = self.started.take() {
            self.timing(started);
        }
        if let Some(h) = self.header.take() {
            let text = canonical(&self.section);
            self.out.push_str("== ");
            self.out.push_str(&h);
            self.out.push('\n');
            self.out.push_str(&text);
            self.section.clear();
        }
    }
    fn attempt<T: Into<Shown>>(&mut self, label: &str, body: impl FnOnce(&mut Driver) -> Result<T>) {
        let line = match body(self) {
            Ok(v) => format!("{} {}", label, show(v.into())),
            Err(e) => format!("{} {}", label, err(&e)),
        };
        self.line(&line);
    }
    fn next(&mut self) -> i64 {
        let mut s = self.seed;
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        self.seed = s;
        s as i64
    }
    fn coord(&mut self) -> f64 {
        ((((self.next() as u64) >> 11) as i64 % 200000) - 100000) as f64 / 100.0
    }

    pub fn run(&mut self, command: &str, args: &[String]) {
        match command {
            "all" => {
                self.json_sections();
                self.validate();
                self.assets();
                self.svg();
                self.geometry();
                self.scenes_section();
                self.actions();
            }
            "read" => {
                for doc in &args[1..] {
                    self.read_store(Path::new(&args[0]), doc);
                }
            }
            "write" => self.write_sequence(Path::new(&args[0])),
            "mutate" => self.mutate(Path::new(&args[0]), &args[1]),
            _ => panic!("Unknown command {}", command),
        }
        self.flush();
    }

    // ------------------------------------------------------------------ JSON
    fn json_sections(&mut self) {
        self.begin("json/cases");
        for raw in read_lines(&self.fixtures.join("json/cases.txt")) {
            let text = raw.replace("\\n", "\n");
            self.attempt(&raw, |_| json::parse(&text).map(|v| w(&v)));
        }
        self.begin("json/equivalent");
        for raw in read_lines(&self.fixtures.join("json/equivalent.txt")) {
            let pair: Vec<&str> = raw.split('\t').collect();
            let (a, b) = (pair[0].to_string(), pair[1].to_string());
            self.attempt(&raw, |_| Ok(json::equivalent(&json::parse(&a)?, &json::parse(&b)?)));
        }
        self.begin("json/doubles");
        for i in 0..60000 {
            let bits = self.next();
            let v: f64 = match i % 6 {
                0 => f64::from_bits(bits as u64),
                1 => (bits % 2000001) as f64 / 1000.0,
                2 => (bits % 100000) as f64 * 0.1,
                3 => (((bits % 1000000) as f64).abs()).sqrt() / 7.0,
                4 => {
                    let exp = (((bits as u64) >> 3) % 40) as i32 - 20;
                    (((bits as u64) >> 11) as i64) as f64 * f64::from_bits(0x3ca0000000000000) * format!("1e{}", exp).parse::<f64>().unwrap()
                }
                _ => ((bits % 100000) as i32) as f64 / (1 + ((bits as u64) >> 40) as i64 % 999) as f64,
            };
            let head = if v.is_nan() { "nan".to_string() } else { format!("{:x}", v.to_bits()) };
            self.line(&format!("{} {}", head, d(v)));
        }
        self.begin("json/floats");
        for i in 0..20000 {
            let bits = self.next();
            let f: f32 = if i % 2 == 0 { f32::from_bits(bits as u32) } else { ((bits % 2000001) as f64 / 1000.0) as f32 };
            let head = if f.is_nan() { "nan".to_string() } else { format!("{:x}", f.to_bits()) };
            self.line(&format!("{} {}", head, jfmt::float(f)));
        }
        self.begin("json/objects");
        let o = object! {
            "a" => 1, "b" => 1.0, "c" => -0.0, "d" => 1e-5, "e" => i64::MAX,
            "f" => Value::Array(vec![Value::from(1), Value::from(2.5), Value::from("x"), Value::Null, Value::Bool(true)]),
            "g" => object!{"h" => Map::new()},
        };
        self.line(&wm(&o));
        self.attempt("nonfinite", |_| Ok(wl(&[json::double(f64::NAN)])));
    }

    fn validate(&mut self) {
        self.begin("validate");
        for raw in read_lines(&self.fixtures.join("validate.jsonl")) {
            let label: String = if json::jlen(&raw) > 80 { raw.encode_utf16().take(80).collect::<Vec<u16>>().pipe(|u| char::decode_utf16(u).map(|c| c.unwrap_or('?')).collect()) } else { raw.clone() };
            self.attempt(&label, |_| {
                let v = json::parse(&raw)?;
                Model::validate(&v.try_into()?)?;
                Ok("OK")
            });
        }
    }

    // ------------------------------------------------------------------ assets
    fn assets(&mut self) {
        for p in sorted_files(&self.fixtures.join("assets")) {
            let name = file_name(&p);
            if name == "uris.txt" {
                continue;
            }
            let bytes = std::fs::read(&p).unwrap();
            self.begin(&format!("assets/{}", name));
            let b64 = jfmt::base64_encode(&bytes);
            if name.ends_with(".gif") {
                self.attempt("bytes", |_| gif_asset::validate_bytes(Some(&bytes)).map(|i| gif_info(&i)));
                self.attempt("uri", |_| gif_asset::validate(Some(&format!("data:image/gif;base64,{}", b64))).map(|i| gif_info(&i)));
                self.attempt("fromBytes", |_| gif_asset::from_bytes(&bytes).map(|s| Value::from(json::jlen(&s) as i64)));
            } else {
                let mime = if name.ends_with(".png") { "image/png" } else { "image/jpeg" };
                let other = if mime == "image/png" { "image/jpeg" } else { "image/png" };
                self.attempt("uri", |_| image_asset::validate(Some(&format!("data:{};base64,{}", mime, b64))));
                self.attempt("wrongMime", |_| image_asset::validate(Some(&format!("data:{};base64,{}", other, b64))));
                self.attempt("fromBytes", |_| image_asset::from_bytes(&bytes).map(|s| s[..30].to_string()));
            }
        }
        self.begin("assets/uris");
        for uri in read_lines(&self.fixtures.join("assets/uris.txt")) {
            self.attempt(&format!("image {}", uri), |_| image_asset::validate(Some(&uri)));
            self.attempt(&format!("gif {}", uri), |_| gif_asset::validate(Some(&uri)).map(|i| gif_info(&i)));
        }
        self.attempt("image null", |_| image_asset::validate(None));
        self.attempt("gif null", |_| gif_asset::validate(None).map(|i| gif_info(&i)));
    }

    fn svg(&mut self) {
        for p in sorted_files(&self.fixtures.join("svg")) {
            let source = read_text(&p);
            self.begin(&format!("svg/{}", file_name(&p)));
            self.attempt("size", |_| svg_asset::validate(&source));
            self.attempt("viewport", |_| svg_asset::viewport(&source));
            self.attempt("dots", |_| {
                Ok(match svg_asset::dot_grid(&source)? {
                    None => "none".to_string(),
                    Some(g) => format!("{} {} {} {} {} {}", d(g.width), d(g.height), d(g.stroke_width), d(g.stroke_opacity), g.color, arr(&g.circles().flat_map(|c| [c.center.x, c.center.y, c.radius.0]).collect::<Vec<f64>>())),
                })
            });
        }
    }

    // ------------------------------------------------------------------ geometry
    fn geometry(&mut self) {
        self.begin("geometry/capsule");
        for radius in [0.05, 0.5, 1.0, 3.3, 12.0, 100.0, 10000.0] {
            for i in 0..4 {
                let a = pt(self.coord(), self.coord());
                let b = if i == 0 { a } else { pt(self.coord(), self.coord()) };
                let text = format!("{} {}", d(radius), poly(&Polygon::capsule(Segment { start: a, end: b }, Radius(radius))));
                self.line(&text);
            }
        }
        self.begin("geometry/predicates");
        for i in 0..400 {
            let n = 3 + (((self.next() as u64) >> 1) % 5) as usize;
            let m = 3 + (((self.next() as u64) >> 1) % 5) as usize;
            let ox = self.coord();
            let oy = self.coord();
            let mut a = Polygon::default();
            for _ in 0..n {
                let p = pt(ox + self.coord() / 50.0, oy + self.coord() / 50.0);
                a.push(p);
            }
            let mut b = Polygon::default();
            for _ in 0..m {
                let p = pt(ox + self.coord() / 50.0, oy + self.coord() / 50.0);
                b.push(p);
            }
            let px = ox + self.coord() / 40.0;
            let py = oy + self.coord() / 40.0;
            let text = format!(
                "{} {} {} {} {}",
                i,
                a.intersects(&b),
                a.contains(pt(px, py)),
                arr(&bounds4(a.bounds())),
                poly(&rect(px, py, ox, oy))
            );
            self.line(&text);
        }
        self.begin("geometry/polygon");
        for raw in ["[[0,0],[1,0],[0,1]]", "[[0,0],[1,0]]", "[[0,0],[1,0],[0,\"1\"]]", "[[0,0],[1,0],[0,1,2]]", "[[0,0],[1,0],[0,1e10]]", "{}", "[[0.5,1.25],[2,3],[4,5],[6,7]]"] {
            self.attempt(raw, |_| Ok(Value::from(&Polygon::try_from(&json::parse(raw)?)?)));
        }
        self.begin("shapes");
        self.line(&w(&shape_catalog::json()));
        for id in shape_catalog::ids() {
            self.attempt(&format!("{} polygon={}", id, shape_catalog::polygon(&id)), |_| shape_catalog::vertices(&id));
            self.attempt(&format!("{} svg", id), |_| shape_catalog::drawing_svg(&id).map(str::to_string));
        }
        self.attempt("name nope", |_| shape_catalog::name(&ShapeId::from("nope")).map(|n| n.to_string()));
        self.attempt("known svg", |_| Ok(shape_catalog::known(&ShapeId::from("svg"))));
        self.attempt("known template", |_| Ok(shape_catalog::known(&ShapeId::from("template"))));
        self.begin("ink");
        let mut ink = InkGeometry::new();
        self.line(&format!("{} {}", arr(&bounds4(ink.bounds())), ink.parts(Bounds::EMPTY).count()));
        for i in 0..40 {
            if i % 3 == 0 {
                let (x, y, wv, h) = (self.coord(), self.coord(), self.coord().abs(), self.coord().abs());
                ink.dot(DocRect { x, y, width: wv, height: h }, if i % 2 == 0 { RectShape::Ellipse } else { RectShape::Rectangle });
            } else {
                let (x, y, ex, ey, wd) = (self.coord(), self.coord(), self.coord(), self.coord(), self.coord().abs() / 10.0);
                ink.line(Segment { start: pt(x, y), end: pt(ex, ey) }, StrokeWidth(wd));
            }
        }
        ink.finish();
        self.line(&format!("{} {}", arr(&bounds4(ink.bounds())), ink.parts(Bounds::EMPTY).count()));
        for i in 0..ink.parts(Bounds::EMPTY).count() {
            let l = self.coord();
            let t = self.coord();
            let (part, overlap) = ink.parts(Bounds { left: l, top: t, right: l + 300.0, bottom: t + 300.0 }).nth(i).unwrap();
            let (kind, width) = match part {
                InkPart::Line { width, .. } => (0.0, width.0),
                InkPart::Dot { shape: RectShape::Ellipse, .. } => (1.0, 0.0),
                InkPart::Dot { shape: RectShape::Rectangle, .. } => (2.0, 0.0),
            };
            let text = format!("{} {} {} {}", i, d(kind), d(width), overlap == AreaOverlap::Overlapping);
            self.line(&text);
        }
        self.begin("coverage/tiles");
        let tiles = Tiles::new(
            bounds([-70.0, -10.0, 130.0, 90.0]),
            Box::new(FnPainter(|x, y, size| {
                let mut px = vec![0i32; (size * size) as usize];
                for j in 0..size {
                    for i in 0..size {
                        let cx = (x + i) as f64 + 0.5;
                        let cy = (y + j) as f64 + 0.5;
                        let dd = (cx - 30.0) * (cx - 30.0) / 4900.0 + (cy - 40.0) * (cy - 40.0) / 1600.0;
                        px[(j * size + i) as usize] = if dd <= 1.0 { 0xff000000u32 as i32 } else if dd <= 1.2 { 0x01000000 } else { 0 };
                    }
                }
                px
            })),
        );
        for i in 0..200 {
            let x = self.coord() / 5.0;
            let y = self.coord() / 5.0;
            let ex = x + self.coord() / 20.0;
            let ey = y + self.coord() / 20.0;
            let r = 1.0 + self.coord().abs() / 100.0;
            let capsule = Polygon::capsule(Segment { start: pt(x, y), end: pt(ex, ey) }, Radius(r));
            self.attempt(&i.to_string(), |_| tiles.intersects(&capsule));
        }
        self.attempt("huge", |_| tiles.intersects(&rect(-1e6, -1e6, 2e6, 2e6)));
        let big = Tiles::new(bounds([-1e6, -1e6, 1e6, 1e6]), Box::new(FnPainter(|_, _, size| vec![0; (size * size) as usize])));
        self.attempt("too large", |_| big.intersects(&rect(-1e5, -1e5, 2e5, 2e5)));
    }

    // ------------------------------------------------------------------ scenes
    fn scene(&mut self, name: &str) -> Model {
        if let Some(m) = self.scenes.get(name) {
            return m.clone();
        }
        let s = json::parse(&read_text(&self.fixtures.join(format!("scenes/{}.json", name)))).unwrap();
        let s = s.as_object().unwrap().clone();
        let mut m = Model::new();
        if s.get("type").and_then(Value::as_str) != Some("fixed") {
            m.apply(&object! {"kind" => "document", "type" => s.get("type").cloned().unwrap_or(Value::Null)}.into()).unwrap();
        }
        let raw = json::is_true(s.get("raw"));
        for op in list_of(s.get("ops")) {
            let result = (|| -> Result<()> {
                let op = json::map(&op)?;
                if !raw {
                    Model::validate(op.into())?;
                }
                m.apply(op.into())
            })();
            if let Err(e) = result {
                if self.header.is_some() {
                    self.line(&format!("apply {}", err(&e)));
                }
            }
        }
        self.scenes.insert(name.to_string(), m.clone());
        m
    }

    fn scenes_section(&mut self) {
        for p in sorted_files(&self.fixtures.join("scenes")) {
            let name = file_name(&p).replace(".json", "");
            self.begin(&format!("scene/{}", name));
            self.scenes.remove(&name);
            let m = self.scene(&name);
            let s = json::parse(&read_text(&p)).unwrap().as_object().unwrap().clone();
            self.line(&format!("model {}", wm(&m.json())));
            self.line(&format!("fromJson {}", wm(&Model::from_json(&m.json()).unwrap().json())));
            let layers: Vec<String> = m.layers.keys().cloned().collect();
            for layer in &layers {
                for page in 0..3 {
                    let items = m.layer_items(&LayerId::from(layer), PageNumber(page));
                    if items.is_empty() {
                        continue;
                    }
                    let mut ids = String::new();
                    for o in items.iter() {
                        let kind = json::java_string(o.get("kind"));
                        ids.push_str(&format!("{}:{} ", kind, json::java_string(o.get(kind.as_str()))));
                    }
                    self.line(&format!("items {} {} {}", layer, page, ids));
                }
            }
            for rv in list_of(s.get("renders")) {
                let render = rv.as_object().unwrap().clone();
                let vp = list_of(render.get("viewport"));
                let f = |i: usize| json::as_double(vp.get(i)).unwrap();
                let (x, y, wv, h) = (f(0), f(1), f(2), f(3));
                let hidden = HiddenLayers( list_of(render.get("hidden")).iter().map(|v| v.as_str().unwrap().to_string()).collect::<HashSet<String>>());
                let page = PageNumber(json::as_int(render.get("page")).unwrap());
                let model = &m;
                self.attempt(&format!("render {}", wm(&render)), |_| {
                    let mut svg = Svg::new(DocRect { x, y, width: wv, height: h });
                    render::draw(model, page, &hidden, &mut svg)?;
                    Ok(svg.to_string())
                });
            }
            let mut all: Vec<Entity> = m.strokes.values().cloned().collect();
            all.extend(m.objects.values().cloned());
            for o in all {
                let kind = json::java_string(o.get("kind"));
                let id = format!("{}:{}", kind, json::java_string(o.get(kind.as_str())));
                let model = &m;
                self.attempt(&format!("item {}", id), |_| {
                    let item = DrawItem::new(model, o.clone())?;
                    let mut b = String::new();
                    b.push_str(&format!(
                        "{} {} {} erasures={}",
                        item.active(),
                        { let p = item.local(pt(10.0, 20.0))?; arr(&[p.x, p.y]) },
                        wm(&item.ref_()),
                        DrawItem::erasures(&o)?.len()
                    ));
                    b.push_str(&format!(" geometry={} local={}", wm(item.geometry()), wm(&*item.local_content()?)));
                    match item.paint_bounds(&FACTORY) {
                        Ok(p) => match item.has_coverage(&FACTORY) {
                            Ok(has) => b.push_str(&format!(" paint={} has={}", arr(&bounds4(p)), has)),
                            Err(e) => b.push_str(&format!(" paint={}", err(&e))),
                        },
                        Err(e) => b.push_str(&format!(" paint={}", err(&e))),
                    }
                    Ok(b)
                });
                if kind == "object" {
                    self.attempt(&format!("endpoints {}", id), |_| render::endpoints(model, &o));
                }
            }
        }
    }

    // ------------------------------------------------------------------ actions
    fn actions(&mut self) {
        self.begin("actions");
        let mut clipboard: Option<Vec<Value>> = None;
        for (n, raw) in read_lines(&self.fixtures.join("actions.jsonl")).into_iter().enumerate() {
            let call = json::parse(&raw).unwrap().as_object().unwrap().clone();
            let mut m = self.scene(call.get("scene").and_then(Value::as_str).unwrap());
            let fn_name = call.get("fn").and_then(Value::as_str).unwrap().to_string();
            let label = format!("{} {}", n, fn_name);
            let copied = clipboard.clone();
            let result: Result<Shown> = (|| -> Result<Shown> {
                let s = |k: &str| call.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
                Ok(match fn_name.as_str() {
                    "copySelection" => drawing_actions::copy_selection(&m, (&list_of(call.get("entities"))).into())?.into(),
                    "paste" => {
                        let mut input = map_of(call.get("input"));
                        if input.get("entities").and_then(Value::as_str) == Some("COPY") {
                            input.insert("entities".into(), copied.clone().map(Value::Array).unwrap_or(Value::Null));
                        }
                        drawing_actions::paste(&m, (&input).into())?.into()
                    }
                    "duplicateLayer" => drawing_actions::duplicate_layer(&m, &LayerId::from(s("id")))?.into(),
                    "mergeLayer" => drawing_actions::merge_layer(&m, &LayerId::from(s("source")), &LayerId::from(s("target")))?.into(),
                    "reorderLayer" => drawing_actions::reorder_layer(&m, &LayerId::from(s("id")), LayerPosition(json::as_int(call.get("index"))?))?.into(),
                    "template" => drawing_actions::template(&m, (&map_of(call.get("input"))).into())?.into(),
                    "fill" => drawing_actions::fill(&m, (&map_of(call.get("input"))).into())?.into(),
                    "erase" => drawing_actions::erase(&m, (&map_of(call.get("input"))).into(), &FACTORY)?.into(),
                    "eraseRegions" => {
                        let r = drawing_actions::erase_regions((&map_of(call.get("input"))).into())?;
                        let again = drawing_actions::erase_regions((&map_of(call.get("input"))).into())?;
                        format!("{} {}", r.len(), poly(&again[0])).into()
                    }
                    "transform" => collaboration::transform(&m, (&list_of(call.get("entities"))).into(), &map_of(call.get("spec")).into())?.into(),
                    "transformSimple" => collaboration::transform_simple(
                        &m,
                        (&list_of(call.get("entities"))).into(),
                        SelectionTransform {
                            scale: json::as_double(call.get("scale"))?,
                            dx: json::as_double(call.get("dx"))?,
                            dy: json::as_double(call.get("dy"))?,
                        },
                    )?
                    .into(),
                    "diagram" => actions::diagram(&m, (&map_of(call.get("input"))).into())?.into(),
                    "inverse" => {
                        let mut r = Vec::new();
                        for op in list_of(call.get("ops")) {
                            match actions::inverse(&m, (op.as_object().unwrap()).into()) {
                                Ok(v) => r.push(Value::from(v)),
                                Err(e) => r.push(Value::String(err(&e))),
                            }
                        }
                        r.into()
                    }
                    "validateReferences" => {
                        let ops = list_of(call.get("ops"));
                        for op in &ops {
                            m.apply((json::map(op)?).into())?;
                        }
                        actions::validate_references(&m, (&ops).into())?;
                        "OK".into()
                    }
                    "select" => {
                        let region = match call.get("region") {
                            None | Some(Value::Null) => None,
                            Some(r) => Some(Polygon::try_from(r)?),
                        };
                        let layer = LayerId::from(s("layer"));
                        let page = PageNumber(json::as_int(call.get("page"))?);
                        let top = if json::is_true(call.get("topmost")) { HitMode::Topmost } else { HitMode::All };
                        let r = region.as_ref();
                        format!(
                            "{} | {} | {}",
                            wl(&LayerEditor::select_in(&m, &layer, page, r, top, &FACTORY)?),
                            wl(&LayerEditor::new(&m, &layer, page)?.select(r, top, &FACTORY)?),
                            wl(&LayerEditor::with_filter(&m, &layer, page, ItemFilter::IncludeInactive)?.select(r, top, &FACTORY)?)
                        )
                        .into()
                    }
                    "paintBounds" => {
                        let mut b = String::new();
                        let editor = LayerEditor::new(&m, &LayerId::from(s("layer")), PageNumber(json::as_int(call.get("page"))?))?;
                        for item in editor.items() {
                            b.push_str(&format!("{}:{}=", item.kind, item.id));
                            match item.paint_bounds(&FACTORY) {
                                Ok(p) => b.push_str(&arr(&bounds4(p))),
                                Err(e) => b.push_str(&err(&e)),
                            }
                            b.push(' ');
                        }
                        b.into()
                    }
                    "requireReferences" => {
                        LayerEditor::require_references(&m, &LayerId::from(s("layer")), PageNumber(json::as_int(call.get("page"))?), (&list_of(call.get("refs"))).into())?;
                        "OK".into()
                    }
                    "locks" => {
                        let mut b = String::new();
                        let mut snap = State::snapshot(&m);
                        for (k, v) in map_of(call.get("state")) {
                            snap.insert(k, v);
                        }
                        match collaboration::require_unlocked(&mut snap, &actor("me"), (&list_of(call.get("ops"))).into(), EpochMillis(1000)) {
                            Ok(()) => b.push_str("unlocked"),
                            Err(e) => b.push_str(&err(&e)),
                        }
                        b.push_str(&format!(" | leases={}", w(snap.get("leases").unwrap_or(&Value::Null))));
                        match collaboration::require_incoming_unlocked(&mut snap, (&map_of(call.get("incoming"))).into(), EpochMillis(1000)) {
                            Ok(()) => b.push_str(" | incoming ok"),
                            Err(e) => b.push_str(&format!(" | {}", err(&e))),
                        }
                        b.push_str(&format!(" | affected={}", wl(&collaboration::affected((&list_of(call.get("ops"))).into())?)));
                        let refs = vec![Value::Object(object! {"kind" => "object", "id" => "app"}), Value::Object(object! {"kind" => "object", "id" => "state", "page" => 1})];
                        b.push_str(&format!(" | conflicts={}", wl(&collaboration::conflicts(&snap, &actor("me"), (&refs).into())?)));
                        b.push_str(&format!(" | key={},{}", collaboration::key(&EntityKind::from("object"), &EntityId::from("x"), None), collaboration::key(&EntityKind::from("object"), &EntityId::from("x"), Some((&json::double(2.0)).into()))));
                        let one = vec![Value::Object(object! {"kind" => "object", "id" => "app"})];
                        match collaboration::require_selection(&mut snap, &m, &actor("other"), &SessionId::from("s2"), (&one).into()) {
                            Ok(()) => b.push_str(" | selection ok"),
                            Err(e) => b.push_str(&format!(" | {}", err(&e))),
                        }
                        match collaboration::require_selection(&mut snap, &m, &actor("me"), &SessionId::from("s1"), (&one).into()) {
                            Ok(()) => b.push_str(" | selection ok"),
                            Err(e) => b.push_str(&format!(" | {}", err(&e))),
                        }
                        b.into()
                    }
                    other => panic!("Unknown fn {}", other),
                })
            })();
            match result {
                Ok(v) => {
                    let first_copy = fn_name == "copySelection" && clipboard.is_none();
                    let as_list = if let Shown::Json(Value::Array(a)) = &v { Some(a.clone()) } else { None };
                    self.line(&format!("{} {}", label, show(v)));
                    if first_copy {
                        clipboard = as_list;
                    }
                }
                Err(e) => self.line(&format!("{} {}", label, err(&e))),
            }
        }
    }

    // ------------------------------------------------------------------ store
    fn read_store(&mut self, dir: &Path, document: &str) {
        self.begin(&format!("store/{}", document));
        let store = match Store::new(dir, &DocumentName::from(document)) {
            Ok(s) => s,
            Err(e) => {
                self.line(&err(&e));
                return;
            }
        };
        self.attempt("file", |_| Ok(file_name(&store.file)));
        self.attempt("read", |_| store.read());
        self.attempt("model", |_| store.model().map(|m| m.json()));
        self.attempt("replica", |_| store.replica(ReplicaCursor(0), ReplicaCursor(0)));
        self.attempt("replica-tail", |_| store.replica(ReplicaCursor(1), ReplicaCursor(2)));
        self.attempt("replica-bad", |_| store.replica(ReplicaCursor(-1), ReplicaCursor(0)));
        self.attempt("needsUpload", |_| store.needs_upload("peer"));
        self.attempt("conflicts", |_| store.conflicts());
        self.attempt("page", |_| Ok(Value::from(Store::page((json::list_at(&*store.read()?, "events")?).into(), ReplicaCursor(0))?.len() as i64)));
        self.attempt("writeRequiresProxy", |_| store.append(&object! {"kind" => "active", "stroke" => "x", "active" => true}.into(), &actor("me")));
    }

    fn dump(&mut self, dir: &Path) {
        for p in sorted_files(dir) {
            let name = file_name(&p);
            if name.ends_with(".journal") {
                let b = std::fs::read(&p).unwrap();
                self.line(&format!("file {} bytes={}", name, b.len()));
                let mut i = 0usize;
                while i + 4 <= b.len() {
                    let len = i32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
                    if i + 8 + len > b.len() {
                        self.line(&format!("  torn tail at {}", i));
                        break;
                    }
                    let stored = i32::from_be_bytes([b[i + 4 + len], b[i + 5 + len], b[i + 6 + len], b[i + 7 + len]]);
                    let ok = crc32fast::hash(&b[i + 4..i + 4 + len]) as i32 == stored;
                    let text = String::from_utf8_lossy(&b[i + 4..i + 4 + len]).into_owned();
                    self.line(&format!("  record len={} crcok={} {}", len, ok, text));
                    i += 8 + len;
                }
            } else if name.ends_with(".json") {
                let text = read_text(&p);
                self.line(&format!("file {} {}", name, text));
            } else {
                self.line(&format!("file {} bytes={}", name, std::fs::metadata(&p).unwrap().len()));
            }
        }
    }

    fn write_sequence(&mut self, dir: &Path) {
        self.begin("write");
        let w = Store::writer(dir, &DocumentName::from("parity-doc")).unwrap();
        let diagram = json::parse(&read_text(&self.fixtures.join("../../../../../apps/cli/examples/diagram.json"))).unwrap().as_object().unwrap().clone();
        let pts = |v: &[(f64, f64, f64)]| Value::Array(v.iter().map(|(x, y, p)| point(*x, *y, *p)).collect());
        let int_point = |x: i64, y: i64, p: f64| Value::Array(vec![Value::from(x), Value::from(y), json::double(p)]);
        let _ = pts;
        let doc = Value::Object(object! {"kind" => "document", "type" => "pages", "id" => "op-doc", "actor" => "server"});
        let seed_op = Value::Object(object! {
            "kind" => "stroke", "stroke" => "seed", "layer" => "base", "page" => 0, "width" => 3, "color" => -16777216,
            "points" => Value::Array(vec![int_point(1, 2, 0.5), Value::Array(vec![json::double(3.25), Value::from(4), Value::from(1)])]),
            "id" => "op-seed", "actor" => "server",
        });
        self.attempt("accept", |_| w.accept((&[doc.clone(), seed_op.clone()]).into(), Some((&Value::from("root-1")).into())));
        self.attempt("accept-again", |_| w.accept((std::slice::from_ref(&doc)).into(), Some((&Value::from("root-1")).into())));
        let conflicting_doc = Value::Object(object! {"kind" => "document", "type" => "fixed", "id" => "op-doc"});
        self.attempt("accept-conflict", |_| w.accept((&[conflicting_doc]).into(), None));
        self.attempt("accept-root", |_| w.accept((&[]).into(), Some((&Value::from("root-2")).into())));
        let s1 = object! {"kind" => "stroke", "stroke" => "s1", "layer" => "base", "page" => 0, "width" => 3, "color" => -16777216, "points" => Value::Array(vec![int_point(1, 2, 0.5)])};
        self.attempt("append", |_| w.append((&s1).into(), &actor("alice")).map(|s| s.get("revision").cloned().unwrap_or(Value::Null)));
        self.attempt("commit", |_| w.commit(&actor("alice"), &mut |m: &mut Model| actions::diagram(m, (&diagram).into())));
        let star = |x: i64, fill: bool| {
            let mut o = object! {"object" => "bob-1", "shape" => "star", "x" => x, "y" => 6};
            if fill {
                o.insert("fill".into(), Value::from(-256));
            }
            o
        };
        self.attempt("commit-bob", |_| w.commit(&actor("bob"), &mut |_m: &mut Model| Ok(vec![Value::from(actions::drawing((&star(5, true)).into(), &LayerId::from("base"))?)].into())));
        self.attempt("undo", |_| w.history(&actor("alice"), HistoryDirection::Undo));
        self.attempt("redo", |_| w.history(&actor("alice"), HistoryDirection::Redo));
        self.attempt("undo-bob", |_| w.history(&actor("bob"), HistoryDirection::Undo));
        self.attempt("undo-empty", |_| w.history(&actor("carol"), HistoryDirection::Undo));
        self.attempt("redo-bob", |_| w.history(&actor("bob"), HistoryDirection::Redo));
        self.attempt("redo-bob-2", |_| w.history(&actor("bob"), HistoryDirection::Redo));
        let hide = object! {"kind" => "active", "stroke" => "s1", "active" => false};
        let settings = object! {"hidden" => Value::Array(vec![Value::from("x")]), "undo" => Value::Array(vec![Value::from("u1")]), "redo" => Value::Array(vec![]), "selected" => "base", "page" => 0};
        self.attempt("settings", |_| EditorPreferences::try_from(&settings).and_then(|settings| w.append_with_settings((&hide).into(), &actor("alice"), Some(&settings))).map(|s| s.get("revision").cloned().unwrap_or(Value::Null)));
        self.attempt("settings-bad", |_| EditorPreferences::try_from(&object! {"color" => 1}).and_then(|settings| w.append_with_settings((&hide).into(), &actor("alice"), Some(&settings))));
        self.attempt("lists", |_| {
            w.edit(&mut |s: &mut State| {
                s.insert("undo".into(), Value::Array(vec![Value::from("u1"), Value::from("u2")]));
                s.insert("redo".into(), Value::Array(vec![Value::from("r")]));
                Ok(())
            })
            .map(|s| s.get("revision").cloned().unwrap_or(Value::Null))
        });
        self.attempt("meta", |_| {
            w.edit(&mut |s: &mut State| {
                s.insert("custom".into(), Value::Object(object! {"k" => 1.5}));
                Ok(())
            })
            .map(|s| s.get("revision").cloned().unwrap_or(Value::Null))
        });
        self.attempt("lease", |_| {
            w.edit_transient(&mut |s: &mut State| {
                json::map_at_mut(s, "leases")?.insert("0:object:bob-1".into(), Value::Object(object! {"actor" => "alice", "session" => "s9", "expires" => i64::MAX}));
                json::map_at_mut(s, "activeSessions")?.insert("alice".into(), Value::from("s9"));
                Ok(())
            })
            .map(|s| s.get("leases").cloned().unwrap_or(Value::Null))
        });
        self.attempt("locked", |_| w.commit(&actor("bob"), &mut |_m: &mut Model| Ok(vec![Value::from(actions::drawing((&star(9, false)).into(), &LayerId::from("base"))?)].into())));
        let sel = vec![Value::Object(object! {"kind" => "object", "id" => "bob-1"})];
        self.attempt("selected", |_| w.commit_selected(&actor("alice"), &SessionId::from("s9"), (&sel).into(), &mut |m: &mut Model| collaboration::transform_simple(m, (&sel).into(), SelectionTransform { scale: 2.0, dx: 1.0, dy: 1.0 }), Default::default()));
        self.attempt("unlease", |_| {
            w.edit_transient(&mut |s: &mut State| {
                s.insert("leases".into(), Value::Object(Map::new()));
                Ok(())
            })
            .map(|s| s.get("leases").cloned().unwrap_or(Value::Null))
        });
        for i in 0..6 {
            let mut p = Vec::new();
            for j in 0..(10 + i * 6) {
                p.push(Value::Array(vec![json::double(j as f64 * 1.5), json::double(((j * 7) % 13) as f64 / 4.0), json::double(0.5)]));
            }
            let op = object! {"kind" => "stroke", "stroke" => "grow", "layer" => "base", "page" => 0, "width" => 4, "color" => -65536, "points" => Value::Array(p)};
            self.attempt(&format!("grow-{}", i), |_| w.append((&op).into(), &actor("alice")).map(|s| s.get("revision").cloned().unwrap_or(Value::Null)));
        }
        self.attempt("commit-move", |_| {
            w.edit(&mut |s: &mut State| {
                let first = json::list_at_mut(s, "pending")?.remove(1);
                json::list_at_mut(s, "events")?.push(first);
                s.insert("cursor".into(), Value::from(1));
                Ok(())
            })
            .map(|s| s.get("revision").cloned().unwrap_or(Value::Null))
        });
        self.attempt("checkpoint", |_| w.checkpoint());
        self.attempt("checkpoint-again", |_| w.checkpoint());
        let remote = Value::Object(object! {
            "kind" => "batch", "id" => "op-remote", "actor" => "server",
            "actions" => Value::Array(vec![Value::Object(object! {"kind" => "object", "object" => "bob-1", "layer" => "base", "page" => 0, "shape" => "star", "x" => 50, "y" => 60, "w" => 160, "h" => 80, "color" => -16777216, "width" => 3, "fontSize" => 24, "text" => ""})]),
            "bases" => object!{"object:bob-1" => "op-old"},
        });
        self.attempt("conflicting", |_| w.accept((&[remote]).into(), None));
        self.attempt("conflicts", |_| w.conflicts());
        self.attempt("resolve", |_| {
            let c = w.conflicts()?;
            if c.is_empty() {
                return Ok(Shown::from("none"));
            }
            let id = json::string(json::map(&c[0])?, "id")?.to_string();
            Ok(Shown::from(resolve(&w, &id, "both", "alice")?))
        });
        self.attempt("resolve-bad", |_| resolve(&w, "nope", "local", "alice"));
        self.attempt("resolve-choice", |_| resolve(&w, "nope", "mine", "alice"));
        self.attempt("model", |_| w.model().map(|m| m.json()));
        self.attempt("delete", |_| w.delete_document(&actor("alice")));
        self.attempt("delete-again", |_| w.delete_document(&actor("alice")));
        self.attempt("after-delete", |_| w.commit(&actor("alice"), &mut |_m: &mut Model| Ok(vec![Value::from(actions::drawing(&object! {"shape" => "rect"}.into(), &LayerId::from("base"))?)].into())));
        self.attempt("read", |_| w.read());
        self.attempt("invalid-doc", |_| Store::writer(dir, &DocumentName::from("")).map(|_| "store"));
        self.flush();
        self.begin("write/files");
        self.dump(dir);
    }

    fn mutate(&mut self, dir: &Path, document: &str) {
        self.begin(&format!("mutate/{}", document));
        let w = Store::writer(dir, &DocumentName::from(document)).unwrap();
        self.attempt("commit", |_| {
            w.commit(&actor("cross"), &mut |_m: &mut Model| {
                Ok(vec![Value::from(actions::drawing(&object! {"object" => "cross-1", "shape" => "hexagon", "x" => 11, "y" => 12, "text" => "from mutate"}.into(), &LayerId::from("base"))?)].into())
            })
        });
        let op = object! {"kind" => "stroke", "stroke" => "cross-s", "layer" => "base", "page" => 0, "width" => 3, "color" => -16777216,
            "points" => Value::Array(vec![Value::Array(vec![Value::from(1), Value::from(2), json::double(0.5)]), Value::Array(vec![Value::from(5), Value::from(6), json::double(0.7)])])};
        self.attempt("append", |_| w.append((&op).into(), &actor("cross")).map(|s| s.get("revision").cloned().unwrap_or(Value::Null)));
        self.attempt("undo", |_| w.history(&actor("cross"), HistoryDirection::Undo));
        self.attempt("checkpoint", |_| w.checkpoint().map(|m| m.get("journalBytesAfter").cloned().unwrap_or(Value::Null)));
        self.attempt("redo", |_| w.history(&actor("cross"), HistoryDirection::Redo));
        self.attempt("model", |_| w.model().map(|m| m.json()));
    }
}

/// Old `Store.conflict(id, choice, actor)`: the choice is validated first.
fn resolve(w: &Store, id: &str, choice: &str, actor: &str) -> Result<notemaker_core::store::CommitResult> {
    let choice = ConflictChoice::parse(&ConflictChoiceName::from(choice))?;
    w.conflict(&ConflictId::from(id), choice, &ActorId::from(actor))
}

trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}
impl<T> Pipe for T {}

/// Entry point shared by the example and the integration test.
pub fn run(fixtures: &Path, command: &str, args: &[String]) -> String {
    let mut driver = Driver::new(fixtures);
    driver.run(command, args);
    driver.finish()
}

pub fn unused(_: Arc<()>) {}
