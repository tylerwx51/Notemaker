//! Port of `InkGeometry.java`: packed paint primitives, six doubles per capsule/dot.

use crate::geometry::{Bounds, DocPoint, DocRect, Segment};
use crate::jmath;
use crate::style::{RectShape, StrokeWidth};

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
    fn size(&self) -> usize {
        self.values.len() / 6
    }
    fn value(&self, part: usize, field: usize) -> f64 {
        self.values[part * 6 + field]
    }
    /// A round-capped line `width` wide.
    pub fn line(&mut self, segment: Segment, width: StrokeWidth) {
        let Segment { start, end } = segment;
        self.add(0.0, start.x, start.y, end.x, end.y, width.0);
    }
    /// A filled rectangle or ellipse.
    pub fn dot(&mut self, rect: DocRect, shape: RectShape) {
        self.add(if shape == RectShape::Ellipse { 1.0 } else { 2.0 }, rect.x, rect.y, rect.width, rect.height, 0.0);
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
    pub fn bounds(&self) -> Bounds {
        let b = if self.values.is_empty() { [0.0; 4] } else { self.box_ };
        Bounds { left: b[0], top: b[1], right: b[2], bottom: b[3] }
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
    fn overlaps(&self, i: usize, area: &Bounds) -> bool {
        self.left(i) <= area.right && self.right(i) >= area.left && self.top(i) <= area.bottom && self.bottom(i) >= area.top
    }
    fn part(&self, i: usize) -> InkPart {
        let v = |field| self.value(i, field);
        if v(0) == 0.0 {
            InkPart::Line {
                segment: Segment { start: DocPoint { x: v(1), y: v(2) }, end: DocPoint { x: v(3), y: v(4) } },
                width: StrokeWidth(v(5)),
            }
        } else {
            let shape = if v(0) == 1.0 { RectShape::Ellipse } else { RectShape::Rectangle };
            InkPart::Dot { rect: DocRect { x: v(1), y: v(2), width: v(3), height: v(4) }, shape }
        }
    }
    /// Every part in drawing order, with whether its box overlaps `area`.
    pub fn parts(&self, area: Bounds) -> impl Iterator<Item = (InkPart, AreaOverlap)> + '_ {
        (0..self.size()).map(move |i| {
            let overlap = if self.overlaps(i, &area) { AreaOverlap::Overlapping } else { AreaOverlap::Outside };
            (self.part(i), overlap)
        })
    }
}

/// One paint primitive of an [`InkGeometry`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InkPart {
    Line { segment: Segment, width: StrokeWidth },
    Dot { rect: DocRect, shape: RectShape },
}

/// Whether a part's bounding box overlaps the queried area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaOverlap {
    Overlapping,
    Outside,
}
