//! Port of `Geometry.java`: platform-independent query geometry (even/odd
//! fill rule), plus the document-space value types used in signatures.
//!
//! All coordinates here are document units (`f64`). Screen and bitmap
//! coordinates have their own types in the apps.

use crate::error::{require, Result};
use crate::jmath;
use crate::json::{self, Value};
use std::ops::{Deref, DerefMut};

/// A point in document coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DocPoint {
    pub x: f64,
    pub y: f64,
}

/// A rectangle as origin and size (`x, y, w, h`), the form objects store.
/// Kept separate from [`Bounds`] so values are never recomputed through the
/// other form, which would change floating-point results.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DocRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl DocRect {
    pub fn origin(&self) -> DocPoint {
        DocPoint { x: self.x, y: self.y }
    }

    pub fn size(&self) -> Size {
        Size { width: self.width, height: self.height }
    }

    /// `Geometry.rect(x,y,w,h)`: the four corners, clockwise from the origin.
    pub fn polygon(&self) -> Polygon {
        let DocRect { x, y, width: w, height: h } = *self;
        Polygon(vec![DocPoint { x, y }, DocPoint { x: x + w, y }, DocPoint { x: x + w, y: y + h }, DocPoint { x, y: y + h }])
    }
}

/// Edge coordinates `{left, top, right, bottom}` (`double[4]` bounds in Java).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Bounds {
    /// The empty bounds that every [`Bounds::include`] widens.
    pub const EMPTY: Bounds = Bounds { left: f64::INFINITY, top: f64::INFINITY, right: f64::NEG_INFINITY, bottom: f64::NEG_INFINITY };

    /// Widens these bounds to contain `p` (`Math.min`/`Math.max`).
    pub fn include(&mut self, p: DocPoint) {
        self.left = jmath::min(self.left, p.x);
        self.top = jmath::min(self.top, p.y);
        self.right = jmath::max(self.right, p.x);
        self.bottom = jmath::max(self.bottom, p.y);
    }
}

/// A width and height in document units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// A line segment, such as the two ends of a connector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub start: DocPoint,
    pub end: DocPoint,
}

/// A radius in document units, such as an eraser's.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Radius(pub f64);

/// An angle in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Degrees(pub f64);

/// A closed polygon or an open point list in document coordinates (Java
/// `double[][]`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polygon(Vec<DocPoint>);

impl Polygon {
    pub fn push(&mut self, point: DocPoint) {
        self.0.push(point);
    }

    /// `Geometry.bounds(p)`.
    pub fn bounds(&self) -> Bounds {
        let mut b = Bounds::EMPTY;
        for v in &self.0 {
            b.include(*v);
        }
        b
    }

    /// `Geometry.contains(p,x,y)` (even/odd).
    pub fn contains(&self, point: DocPoint) -> bool {
        let p = &self.0;
        let DocPoint { x, y } = point;
        let mut inside = false;
        let n = p.len();
        if n == 0 {
            return false;
        }
        let mut j = n - 1;
        for i in 0..n {
            let a = p[i];
            let b = p[j];
            if (a.y > y) != (b.y > y) && x < (b.x - a.x) * (y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    /// `Geometry.intersects(a,b)`.
    pub fn intersects(&self, other: &Polygon) -> bool {
        let (a, b) = (&self.0, &other.0);
        let x = self.bounds();
        let y = other.bounds();
        if x.right < y.left || y.right < x.left || x.bottom < y.top || y.bottom < x.top {
            return false;
        }
        for p in a {
            if other.contains(*p) {
                return true;
            }
        }
        for p in b {
            if self.contains(*p) {
                return true;
            }
        }
        for i in 0..a.len() {
            for j in 0..b.len() {
                let p = a[i];
                let q = a[(i + 1) % a.len()];
                let r = b[j];
                let s = b[(j + 1) % b.len()];
                if jmath::max(p.x, q.x) < jmath::min(r.x, s.x)
                    || jmath::max(r.x, s.x) < jmath::min(p.x, q.x)
                    || jmath::max(p.y, q.y) < jmath::min(r.y, s.y)
                    || jmath::max(r.y, s.y) < jmath::min(p.y, q.y)
                {
                    continue;
                }
                if cross(p, q, r) * cross(p, q, s) <= 0.0 && cross(r, s, p) * cross(r, s, q) <= 0.0 {
                    return true;
                }
            }
        }
        false
    }

    /// `Geometry.capsule(a,b,radius)`: round swept segment.
    pub fn capsule(segment: Segment, radius: Radius) -> Polygon {
        let Segment { start: a, end: b } = segment;
        let radius = radius.0;
        let pi = std::f64::consts::PI;
        let n = 512.min(16.max(jmath::d2i((pi / jmath::acos(jmath::max(-1.0, 1.0 - 0.1 / radius))).ceil()))) as usize;
        let angle = jmath::atan2(b.y - a.y, b.x - a.x);
        let mut p = vec![DocPoint::default(); 2 * (n + 1)];
        for i in 0..=n {
            let mut t = angle - pi / 2.0 + pi * i as f64 / n as f64;
            p[i] = DocPoint { x: b.x + radius * jmath::cos(t), y: b.y + radius * jmath::sin(t) };
            t += pi;
            p[n + 1 + i] = DocPoint { x: a.x + radius * jmath::cos(t), y: a.y + radius * jmath::sin(t) };
        }
        Polygon(p)
    }
}

impl Deref for Polygon {
    type Target = [DocPoint];
    fn deref(&self) -> &[DocPoint] {
        &self.0
    }
}

impl DerefMut for Polygon {
    fn deref_mut(&mut self) -> &mut [DocPoint] {
        &mut self.0
    }
}

impl From<Polygon> for Vec<DocPoint> {
    fn from(polygon: Polygon) -> Vec<DocPoint> {
        polygon.0
    }
}

impl From<Vec<DocPoint>> for Polygon {
    fn from(points: Vec<DocPoint>) -> Polygon {
        Polygon(points)
    }
}

impl FromIterator<DocPoint> for Polygon {
    fn from_iter<I: IntoIterator<Item = DocPoint>>(iter: I) -> Polygon {
        Polygon(iter.into_iter().collect())
    }
}

impl<'a> IntoIterator for &'a Polygon {
    type Item = &'a DocPoint;
    type IntoIter = std::slice::Iter<'a, DocPoint>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Defines a list-of-polygons newtype with `Vec`-like access.
macro_rules! polygon_list {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct $name(Vec<Polygon>);

        impl $name {
            pub fn push(&mut self, polygon: Polygon) {
                self.0.push(polygon);
            }
        }
        impl Deref for $name {
            type Target = [Polygon];
            fn deref(&self) -> &[Polygon] {
                &self.0
            }
        }
        impl From<$name> for Vec<Polygon> {
            fn from(list: $name) -> Vec<Polygon> {
                list.0
            }
        }
        impl From<Vec<Polygon>> for $name {
            fn from(polygons: Vec<Polygon>) -> $name {
                $name(polygons)
            }
        }
        impl FromIterator<Polygon> for $name {
            fn from_iter<I: IntoIterator<Item = Polygon>>(iter: I) -> $name {
                $name(iter.into_iter().collect())
            }
        }
        impl<'a> IntoIterator for &'a $name {
            type Item = &'a Polygon;
            type IntoIter = std::slice::Iter<'a, Polygon>;
            fn into_iter(self) -> Self::IntoIter {
                self.0.iter()
            }
        }
    };
}

polygon_list! {
    /// Eraser regions cut out of a stroke (its `erasures`).
    Erasures
}

polygon_list! {
    /// Continuous ribbon passes that make up one ink stroke.
    InkPasses
}

fn cross(a: DocPoint, b: DocPoint, c: DocPoint) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

impl TryFrom<&Value> for Polygon {
    type Error = crate::Error;
    /// `Geometry.polygon(raw)`: validated region vertices.
    fn try_from(raw: &Value) -> Result<Polygon> {
        let list = json::list(raw)?;
        require(list.len() >= 3 && list.len() <= 4096, "Region needs 3–4096 vertices")?;
        let mut p = vec![DocPoint::default(); list.len()];
        for (i, item) in list.iter().enumerate() {
            let v = json::list(item)?;
            require(v.len() == 2, "Region vertex needs x/y")?;
            let mut xy = [0.0f64; 2];
            for k in 0..2 {
                require(json::is_number(Some(&v[k])), "Invalid region vertex")?;
                xy[k] = json::as_double(Some(&v[k]))?;
                require(xy[k].is_finite() && xy[k].abs() <= 1e9, "Invalid region vertex")?;
            }
            p[i] = DocPoint { x: xy[0], y: xy[1] };
        }
        Ok(Polygon(p))
    }
}

impl From<&Polygon> for Value {
    /// `Geometry.json(p)`: `[[x,y],...]` with double values.
    fn from(p: &Polygon) -> Value {
        Value::Array(p.0.iter().map(|v| Value::Array(vec![json::double(v.x), json::double(v.y)])).collect())
    }
}
