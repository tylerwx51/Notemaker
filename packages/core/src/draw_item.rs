//! Port of `DrawItem.java`: adapter retaining existing journal identities.

use crate::coverage::{Coverage, CoverageFactory};
use crate::error::Result;
use crate::geometry::{self, Polygon};
use crate::jmath;
use crate::json::{self, Map, Value};
use crate::model::{Entity, Model};
use crate::object;
use crate::render;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Per-geometry lazily computed data, shared between `DrawItem`s built over
/// the same (identical) geometry map, like Java's identity-keyed cache.
#[derive(Default)]
struct Prepared {
    bounds: Mutex<Option<[f64; 4]>>,
    local: Mutex<Option<Arc<Map>>>,
}

/// Identity-keyed LRU (10,000 entries, access order). The entry keeps the
/// geometry `Arc` alive so its address cannot be reused while cached.
type PreparedCache = IndexMap<usize, (Entity, Arc<Prepared>)>;

fn prepared_cache() -> &'static Mutex<PreparedCache> {
    static CACHE: OnceLock<Mutex<PreparedCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(IndexMap::new()))
}

const STRIPPED: [&str; 8] = ["id", "actor", "seq", "at", "action", "session", "bases", "visibility"];

/// `DrawItem`.
pub struct DrawItem {
    pub kind: String,
    pub id: String,
    pub content: Entity,
    active: bool,
    geometry: Entity,
    cached_coverage: Mutex<Option<(usize, Arc<dyn Coverage>)>>,
    prepared: OnceLock<Arc<Prepared>>,
}

fn factory_identity(factory: &dyn CoverageFactory) -> usize {
    factory as *const dyn CoverageFactory as *const () as usize
}

fn rotation(o: &Map) -> Result<f64> {
    match o.get("rotation") {
        None => Ok(0.0),
        v => json::as_double(v),
    }
}

impl DrawItem {
    /// `new DrawItem(model, content)`.
    pub fn new(model: &Model, content: Entity) -> Result<DrawItem> {
        let kind = json::string(&content, "kind")?.to_string();
        let id = json::string(&content, &kind)?.to_string();
        let table = if kind == "stroke" { &model.active } else { &model.object_active };
        let active = table.get(&id) != Some(&false);
        let geometry = DrawItem::resolve_geometry(model, &kind, &content)?;
        Ok(DrawItem { kind, id, content, active, geometry, cached_coverage: Mutex::new(None), prepared: OnceLock::new() })
    }

    /// Convenience for a freshly built (unshared) content map.
    pub fn from_map(model: &Model, content: Map) -> Result<DrawItem> {
        DrawItem::new(model, Arc::new(content))
    }

    fn prepared(&self) -> Arc<Prepared> {
        self.prepared
            .get_or_init(|| {
                let mut cache = prepared_cache().lock().unwrap_or_else(|e| e.into_inner());
                let key = Arc::as_ptr(&self.geometry) as usize;
                if let Some(entry) = cache.shift_remove(&key) {
                    let prepared = entry.1.clone();
                    cache.insert(key, entry);
                    return prepared;
                }
                let prepared = Arc::new(Prepared::default());
                cache.insert(key, (self.geometry.clone(), prepared.clone()));
                if cache.len() > 10000 {
                    cache.shift_remove_index(0);
                }
                prepared
            })
            .clone()
    }

    /// `ref()`: `{kind, id, page}`.
    pub fn ref_(&self) -> Map {
        object! {"kind" => self.kind.clone(), "id" => self.id.clone(), "page" => self.content.get("page").cloned().unwrap_or(Value::Null)}
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// `geometry()`: content, or a resolved copy for attached connectors.
    pub fn geometry(&self) -> &Entity {
        &self.geometry
    }

    fn resolve_geometry(model: &Model, kind: &str, content: &Entity) -> Result<Entity> {
        if kind == "object" && (content.contains_key("from") || content.contains_key("to")) {
            let mut o: Map = (**content).clone();
            let e = render::endpoints(model, content)?;
            o.insert("x".into(), json::double(e[0]));
            o.insert("y".into(), json::double(e[1]));
            o.insert("w".into(), json::double(e[2] - e[0]));
            o.insert("h".into(), json::double(e[3] - e[1]));
            o.shift_remove("from");
            o.shift_remove("to");
            o.insert("rotation".into(), Value::from(0));
            o.insert("flipX".into(), Value::Bool(false));
            o.insert("flipY".into(), Value::Bool(false));
            return Ok(Arc::new(o));
        }
        Ok(content.clone())
    }

    /// `local(x, y)`: document point in item-local paint coordinates.
    pub fn local(&self, x: f64, y: f64) -> Result<[f64; 2]> {
        let o = &self.geometry;
        if self.kind == "stroke" {
            let points = json::list_at(o, "points")?;
            let p = json::list(points.first().ok_or_else(|| index_error(0, 0))?)?;
            return Ok([x - json::as_double(p.first())?, y - json::as_double(p.get(1))?]);
        }
        let w = json::number(o, "w")?;
        let h = json::number(o, "h")?;
        let a = jmath::to_radians(rotation(o)?);
        let dx = x - json::number(o, "x")? - w / 2.0;
        let dy = y - json::number(o, "y")? - h / 2.0;
        let fx = if json::is_true(o.get("flipX")) { -1.0 } else { 1.0 };
        let fy = if json::is_true(o.get("flipY")) { -1.0 } else { 1.0 };
        Ok([
            (dx * jmath::cos(a) + dy * jmath::sin(a)) * fx + w / 2.0,
            (-dx * jmath::sin(a) + dy * jmath::cos(a)) * fy + h / 2.0,
        ])
    }

    /// `localContent()`: geometry translated to item-local coordinates (cached).
    pub fn local_content(&self) -> Result<Arc<Map>> {
        let prepared = self.prepared();
        if let Some(local) = prepared.local.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            return Ok(local.clone());
        }
        let mut o: Map = (*self.geometry).clone();
        if self.kind == "object" {
            o.insert("x".into(), Value::from(0));
            o.insert("y".into(), Value::from(0));
            o.insert("rotation".into(), Value::from(0));
            o.insert("flipX".into(), Value::Bool(false));
            o.insert("flipY".into(), Value::Bool(false));
        } else {
            let mut points = Vec::new();
            for v in json::list_at(&self.content, "points")? {
                let p = json::list(v)?;
                let q = self.local(json::as_double(p.first())?, json::as_double(p.get(1))?)?;
                points.push(Value::Array(vec![json::double(q[0]), json::double(q[1]), p.get(2).cloned().unwrap_or(Value::Null)]));
            }
            o.insert("points".into(), Value::Array(points));
        }
        let local = Arc::new(o);
        *prepared.local.lock().unwrap_or_else(|e| e.into_inner()) = Some(local.clone());
        Ok(local)
    }

    fn coverage(&self, factory: &dyn CoverageFactory) -> Result<Arc<dyn Coverage>> {
        let identity = factory_identity(factory);
        let mut cached = self.cached_coverage.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((owner, coverage)) = cached.as_ref() {
            if *owner == identity {
                return Ok(coverage.clone());
            }
        }
        let coverage: Arc<dyn Coverage> = Arc::from(factory.create(&*self.local_content()?)?);
        *cached = Some((identity, coverage.clone()));
        Ok(coverage)
    }

    /// Conservative unmasked local bounds; `None` for text (glyph overhangs need native coverage).
    fn coarse_bounds(&self) -> Result<Option<[f64; 4]>> {
        let prepared = self.prepared();
        if let Some(b) = *prepared.bounds.lock().unwrap_or_else(|e| e.into_inner()) {
            return Ok(Some(b));
        }
        let o = &self.geometry;
        let mut pad = json::number(o, "width")? * 2.0;
        if self.kind == "stroke" {
            let (mut l, mut t, mut r, mut b) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
            for raw in json::list_at(o, "points")? {
                let p = json::list(raw)?;
                let x = json::as_double(p.first())?;
                let y = json::as_double(p.get(1))?;
                l = jmath::min(l, x);
                t = jmath::min(t, y);
                r = jmath::max(r, x);
                b = jmath::max(b, y);
            }
            let first = self.local(l, t)?;
            let last = self.local(r, b)?;
            let bounds = [first[0] - pad, first[1] - pad, last[0] + pad, last[1] + pad];
            *prepared.bounds.lock().unwrap_or_else(|e| e.into_inner()) = Some(bounds);
            return Ok(Some(bounds));
        }
        let text = match o.get("text") {
            None => "",
            v => json::as_string_opt(v)?.ok_or_else(crate::error::Error::null_pointer)?,
        };
        if !text.is_empty() {
            return Ok(None);
        }
        if matches!(o.get("shape").and_then(Value::as_str), Some("arrow") | Some("arrowLine")) {
            pad += jmath::max(12.0, json::number(o, "width")? * 4.0);
        }
        pad += 1.0;
        let w = json::number(o, "w")?;
        let h = json::number(o, "h")?;
        let bounds = [jmath::min(0.0, w) - pad, jmath::min(0.0, h) - pad, jmath::max(0.0, w) + pad, jmath::max(0.0, h) + pad];
        *prepared.bounds.lock().unwrap_or_else(|e| e.into_inner()) = Some(bounds);
        Ok(Some(bounds))
    }

    fn overlaps(b: &[f64; 4], q: &[f64; 4]) -> bool {
        b[2] >= q[0] && q[2] >= b[0] && b[3] >= q[1] && q[3] >= b[1]
    }

    /// `intersects(region, factory)`: painted area hit test in document coordinates.
    pub fn intersects(&self, region: &[[f64; 2]], factory: &dyn CoverageFactory) -> Result<bool> {
        let coarse = self.coarse_bounds()?;
        if self.kind == "stroke" {
            let mut q = geometry::bounds(region);
            let origin = self.local(0.0, 0.0)?;
            q[0] += origin[0];
            q[2] += origin[0];
            q[1] += origin[1];
            q[3] += origin[1];
            if !DrawItem::overlaps(coarse.as_ref().unwrap(), &q) {
                return Ok(false);
            }
        }
        let mut p: Polygon = Vec::with_capacity(region.len());
        if self.kind == "object" {
            let o = &self.geometry;
            let w = json::number(o, "w")?;
            let h = json::number(o, "h")?;
            let angle = jmath::to_radians(rotation(o)?);
            let cos = jmath::cos(angle);
            let sin = jmath::sin(angle);
            let cx = json::number(o, "x")? + w / 2.0;
            let cy = json::number(o, "y")? + h / 2.0;
            let flip_x = if json::is_true(o.get("flipX")) { -1.0 } else { 1.0 };
            let flip_y = if json::is_true(o.get("flipY")) { -1.0 } else { 1.0 };
            for v in region {
                let dx = v[0] - cx;
                let dy = v[1] - cy;
                p.push([(dx * cos + dy * sin) * flip_x + w / 2.0, (-dx * sin + dy * cos) * flip_y + h / 2.0]);
            }
        } else {
            for v in region {
                p.push(self.local(v[0], v[1])?);
            }
        }
        let q = geometry::bounds(&p);
        if let Some(c) = coarse {
            if !DrawItem::overlaps(&c, &q) {
                return Ok(false);
            }
        }
        let area = self.coverage(factory)?;
        Ok(DrawItem::overlaps(&area.bounds(), &q) && area.intersects(&p)?)
    }

    /// `hasCoverage(factory)`.
    pub fn has_coverage(&self, factory: &dyn CoverageFactory) -> Result<bool> {
        let coverage = self.coverage(factory)?;
        let b = coverage.bounds();
        Ok(b[2] > b[0] && b[3] > b[1] && coverage.intersects(&geometry::rect(b[0], b[1], b[2] - b[0], b[3] - b[1]))?)
    }

    /// `paintBounds(factory)`: conservative document-space paint bounds.
    pub fn paint_bounds(&self, factory: &dyn CoverageFactory) -> Result<[f64; 4]> {
        let b = self.coverage(factory)?.bounds();
        let mut out = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        if self.kind == "stroke" {
            let origin = self.local(0.0, 0.0)?;
            return Ok([b[0] - origin[0], b[1] - origin[1], b[2] - origin[0], b[3] - origin[1]]);
        }
        let o = &self.geometry;
        let w = json::number(o, "w")?;
        let h = json::number(o, "h")?;
        let angle = jmath::to_radians(rotation(o)?);
        for x in [b[0], b[2]] {
            for y in [b[1], b[3]] {
                let dx = (x - w / 2.0) * if json::is_true(o.get("flipX")) { -1.0 } else { 1.0 };
                let dy = (y - h / 2.0) * if json::is_true(o.get("flipY")) { -1.0 } else { 1.0 };
                let px = json::number(o, "x")? + w / 2.0 + dx * jmath::cos(angle) - dy * jmath::sin(angle);
                let py = json::number(o, "y")? + h / 2.0 + dx * jmath::sin(angle) + dy * jmath::cos(angle);
                out[0] = jmath::min(out[0], px);
                out[1] = jmath::min(out[1], py);
                out[2] = jmath::max(out[2], px);
                out[3] = jmath::max(out[3], py);
            }
        }
        Ok(out)
    }

    /// `erase(regions)`: replacement operation with added local erasure masks.
    pub fn erase(&self, regions: &[Polygon]) -> Result<Map> {
        let mut next: Map = (*self.content).clone();
        for key in STRIPPED {
            next.shift_remove(key);
        }
        let mut masks: Vec<Value> = match next.get("erasures") {
            None => Vec::new(),
            Some(v) => json::list(v)?.clone(),
        };
        let o = &self.geometry;
        for region in regions {
            let mut p: Polygon = Vec::with_capacity(region.len());
            for v in region {
                let mut q = self.local(v[0], v[1])?;
                if self.kind == "object" {
                    q[0] /= unit(json::number(o, "w")?);
                    q[1] /= unit(json::number(o, "h")?);
                }
                p.push(q);
            }
            masks.push(geometry::json(&p));
        }
        next.insert("erasures".into(), Value::Array(masks));
        Model::validate(&next)?;
        Ok(next)
    }

    /// `DrawItem.erasures(o)`: persistent local masks in unrotated paint coordinates.
    pub fn erasures(o: &Map) -> Result<Vec<Polygon>> {
        let raw_masks = match o.get("erasures") {
            None => return Ok(Vec::new()),
            Some(v) => json::list(v)?,
        };
        if raw_masks.is_empty() {
            return Ok(Vec::new());
        }
        let stroke = o.get("kind").and_then(Value::as_str) == Some("stroke") || (!o.contains_key("kind") && o.contains_key("points"));
        let (x, y, w, h);
        if stroke {
            let points = json::list_at(o, "points")?;
            let p = json::list(points.first().ok_or_else(|| index_error(0, 0))?)?;
            x = json::as_double(p.first())?;
            y = json::as_double(p.get(1))?;
            w = 1.0;
            h = 1.0;
        } else {
            x = json::number(o, "x")?;
            y = json::number(o, "y")?;
            w = unit(json::number(o, "w")?);
            h = unit(json::number(o, "h")?);
        }
        let mut out = Vec::with_capacity(raw_masks.len());
        for raw in raw_masks {
            let mut p = geometry::polygon(raw)?;
            for v in p.iter_mut() {
                v[0] = x + v[0] * w;
                v[1] = y + v[1] * h;
            }
            out.push(p);
        }
        Ok(out)
    }
}

fn unit(n: f64) -> f64 {
    if n == 0.0 { 1.0 } else { n }
}

fn index_error(index: usize, len: usize) -> crate::error::Error {
    crate::error::Error::new(crate::error::ErrorKind::IndexOutOfBounds, format!("Index {} out of bounds for length {}", index, len))
}
