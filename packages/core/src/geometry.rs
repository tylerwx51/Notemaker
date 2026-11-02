//! Port of `Geometry.java`: platform-independent query geometry (even/odd fill rule).

use crate::error::{require, Result};
use crate::jmath;
use crate::json::{self, Value};

/// Java `double[][]` polygon / point list.
pub type Polygon = Vec<[f64; 2]>;

/// `Geometry.rect(x,y,w,h)`.
pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Polygon {
    vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}

/// `Geometry.bounds(p)`: `{left, top, right, bottom}`.
pub fn bounds(p: &[[f64; 2]]) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for v in p {
        b[0] = jmath::min(b[0], v[0]);
        b[1] = jmath::min(b[1], v[1]);
        b[2] = jmath::max(b[2], v[0]);
        b[3] = jmath::max(b[3], v[1]);
    }
    b
}

/// `Geometry.contains(p,x,y)` (even/odd).
pub fn contains(p: &[[f64; 2]], x: f64, y: f64) -> bool {
    let mut inside = false;
    let n = p.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let a = p[i];
        let b = p[j];
        if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// `Geometry.intersects(a,b)`.
pub fn intersects(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    let x = bounds(a);
    let y = bounds(b);
    if x[2] < y[0] || y[2] < x[0] || x[3] < y[1] || y[3] < x[1] {
        return false;
    }
    for p in a {
        if contains(b, p[0], p[1]) {
            return true;
        }
    }
    for p in b {
        if contains(a, p[0], p[1]) {
            return true;
        }
    }
    for i in 0..a.len() {
        for j in 0..b.len() {
            let p = a[i];
            let q = a[(i + 1) % a.len()];
            let r = b[j];
            let s = b[(j + 1) % b.len()];
            if jmath::max(p[0], q[0]) < jmath::min(r[0], s[0])
                || jmath::max(r[0], s[0]) < jmath::min(p[0], q[0])
                || jmath::max(p[1], q[1]) < jmath::min(r[1], s[1])
                || jmath::max(r[1], s[1]) < jmath::min(p[1], q[1])
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

/// `Geometry.polygon(raw)`: validated region vertices.
pub fn polygon(raw: &Value) -> Result<Polygon> {
    let list = json::list(raw)?;
    require(list.len() >= 3 && list.len() <= 4096, "Region needs 3–4096 vertices")?;
    let mut p = vec![[0.0f64; 2]; list.len()];
    for (i, item) in list.iter().enumerate() {
        let v = json::list(item)?;
        require(v.len() == 2, "Region vertex needs x/y")?;
        for k in 0..2 {
            require(json::is_number(Some(&v[k])), "Invalid region vertex")?;
            p[i][k] = json::as_double(Some(&v[k]))?;
            require(p[i][k].is_finite() && p[i][k].abs() <= 1e9, "Invalid region vertex")?;
        }
    }
    Ok(p)
}

/// `Geometry.json(p)`: `[[x,y],...]` with double values.
pub fn json(p: &[[f64; 2]]) -> Value {
    Value::Array(p.iter().map(|v| Value::Array(vec![json::double(v[0]), json::double(v[1])])).collect())
}

/// `Geometry.capsule(a,b,radius)`: round swept segment.
pub fn capsule(a: [f64; 2], b: [f64; 2], radius: f64) -> Polygon {
    let pi = std::f64::consts::PI;
    let n = 512.min(16.max(jmath::d2i((pi / jmath::acos(jmath::max(-1.0, 1.0 - 0.1 / radius))).ceil()))) as usize;
    let angle = jmath::atan2(b[1] - a[1], b[0] - a[0]);
    let mut p = vec![[0.0f64; 2]; 2 * (n + 1)];
    for i in 0..=n {
        let mut t = angle - pi / 2.0 + pi * i as f64 / n as f64;
        p[i] = [b[0] + radius * jmath::cos(t), b[1] + radius * jmath::sin(t)];
        t += pi;
        p[n + 1 + i] = [a[0] + radius * jmath::cos(t), a[1] + radius * jmath::sin(t)];
    }
    p
}
