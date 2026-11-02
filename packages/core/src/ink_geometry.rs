//! Port of `InkGeometry.java`: packed paint primitives, six doubles per capsule/dot.

use crate::jmath;

/// `InkGeometry`.
#[derive(Clone, Debug)]
pub struct InkGeometry {
    values: Vec<f64>,
    box_: [f64; 4],
}

impl Default for InkGeometry {
    fn default() -> Self {
        InkGeometry::new()
    }
}

impl InkGeometry {
    pub fn new() -> InkGeometry {
        InkGeometry {
            values: Vec::with_capacity(96),
            box_: [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
        }
    }
    pub fn size(&self) -> usize {
        self.values.len() / 6
    }
    pub fn value(&self, part: usize, field: usize) -> f64 {
        self.values[part * 6 + field]
    }
    pub fn line(&mut self, x: f64, y: f64, ex: f64, ey: f64, width: f64) {
        self.add(0.0, x, y, ex, ey, width);
    }
    pub fn dot(&mut self, x: f64, y: f64, w: f64, h: f64, ellipse: bool) {
        self.add(if ellipse { 1.0 } else { 2.0 }, x, y, w, h, 0.0);
    }
    fn add(&mut self, kind: f64, a: f64, b: f64, c: f64, d: f64, width: f64) {
        let part = self.size();
        self.values.extend_from_slice(&[kind, a, b, c, d, width]);
        self.box_[0] = jmath::min(self.box_[0], self.left(part));
        self.box_[1] = jmath::min(self.box_[1], self.top(part));
        self.box_[2] = jmath::max(self.box_[2], self.right(part));
        self.box_[3] = jmath::max(self.box_[3], self.bottom(part));
    }
    pub fn finish(&mut self) {
        self.values.shrink_to_fit();
    }
    pub fn bounds(&self) -> [f64; 4] {
        if self.values.is_empty() { [0.0; 4] } else { self.box_ }
    }
    fn left(&self, i: usize) -> f64 {
        if self.value(i, 0) == 0.0 { jmath::min(self.value(i, 1), self.value(i, 3)) - self.value(i, 5) / 2.0 } else { self.value(i, 1) }
    }
    fn top(&self, i: usize) -> f64 {
        if self.value(i, 0) == 0.0 { jmath::min(self.value(i, 2), self.value(i, 4)) - self.value(i, 5) / 2.0 } else { self.value(i, 2) }
    }
    fn right(&self, i: usize) -> f64 {
        if self.value(i, 0) == 0.0 { jmath::max(self.value(i, 1), self.value(i, 3)) + self.value(i, 5) / 2.0 } else { self.value(i, 1) + self.value(i, 3) }
    }
    fn bottom(&self, i: usize) -> f64 {
        if self.value(i, 0) == 0.0 { jmath::max(self.value(i, 2), self.value(i, 4)) + self.value(i, 5) / 2.0 } else { self.value(i, 2) + self.value(i, 4) }
    }
    pub fn overlaps(&self, i: usize, l: f64, t: f64, r: f64, b: f64) -> bool {
        self.left(i) <= r && self.right(i) >= l && self.top(i) <= b && self.bottom(i) >= t
    }
}
