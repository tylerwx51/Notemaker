//! Exact polygon areas (AWT `java.awt.geom.Area` / Android `Path.op`) on
//! top of `i_overlay`. Curves are flattened first; every area is stored as
//! resolved shapes (outer contours counter-clockwise, holes clockwise), so
//! later operations do not depend on the source path's fill rule.

use crate::canvas::{CanvasPoint, FillType, FlattenTolerance, Path, RectF};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::overlay::FloatOverlay;
use i_overlay::float::relate::FloatRelate;
use i_overlay::float::single::SingleFloatOverlay;

/// Flattening tolerance in document units for curved outlines.
pub(crate) const TOLERANCE: FlattenTolerance = FlattenTolerance(0.01);

type Shapes = Vec<Vec<Vec<[f64; 2]>>>;

/// A resolved area.
#[derive(Clone, Debug, Default)]
pub(crate) struct Area {
    shapes: Shapes,
}

impl Area {
    /// `new Area(path)`: the region the path fills under its fill rule.
    pub(crate) fn of(path: &Path) -> Area {
        let contours: Vec<Vec<[f64; 2]>> = path
            .flatten(TOLERANCE)
            .0
            .into_iter()
            .filter(|c| c.len() >= 3)
            .map(|c| c.into_iter().map(|p| [p.x as f64, p.y as f64]).collect())
            .collect();
        if contours.is_empty() {
            return Area::default();
        }
        let rule = match path.fill_type() {
            FillType::Winding => FillRule::NonZero,
            FillType::EvenOdd => FillRule::EvenOdd,
        };
        Area {
            shapes: FloatOverlay::with_subj(&contours).overlay(OverlayRule::Subject, rule),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// `getBounds2D()`; `None` when empty.
    pub(crate) fn bounds(&self) -> Option<RectF> {
        let mut out: Option<RectF> = None;
        for p in self.shapes.iter().flatten().flatten() {
            let (x, y) = (p[0] as f32, p[1] as f32);
            match &mut out {
                None => out = Some(RectF { left: x, top: y, right: x, bottom: y }),
                Some(r) => {
                    r.left = r.left.min(x);
                    r.top = r.top.min(y);
                    r.right = r.right.max(x);
                    r.bottom = r.bottom.max(y);
                }
            }
        }
        out
    }

    fn op(&self, other: &Area, rule: OverlayRule) -> Area {
        if self.is_empty() && other.is_empty() {
            return Area::default();
        }
        Area {
            shapes: self.shapes.overlay(&other.shapes, rule, FillRule::NonZero),
        }
    }

    pub(crate) fn subtract(&self, other: &Area) -> Area {
        if self.is_empty() || other.is_empty() {
            return self.clone();
        }
        self.op(other, OverlayRule::Difference)
    }

    pub(crate) fn intersect(&self, other: &Area) -> Area {
        if self.is_empty() || other.is_empty() {
            return Area::default();
        }
        self.op(other, OverlayRule::Intersect)
    }

    pub(crate) fn union(&self, other: &Area) -> Area {
        if self.is_empty() {
            return other.clone();
        }
        if other.is_empty() {
            return self.clone();
        }
        self.op(other, OverlayRule::Union)
    }

    /// `!new Area(a).intersect(b).isEmpty()`: positive-area overlap.
    pub(crate) fn overlaps(&self, other: &Area) -> bool {
        !self.is_empty() && !other.is_empty() && self.shapes.interiors_intersect(&other.shapes)
    }

    /// The area as a fillable path (resolved contours, non-zero).
    pub(crate) fn to_path(&self) -> Path {
        let mut path = Path::new();
        for contour in self.shapes.iter().flatten() {
            let mut first = true;
            for p in contour {
                if first {
                    path.move_to(CanvasPoint { x: p[0] as f32, y: p[1] as f32 });
                    first = false;
                } else {
                    path.line_to(CanvasPoint { x: p[0] as f32, y: p[1] as f32 });
                }
            }
            path.close();
        }
        path
    }
}
