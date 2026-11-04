//! `android.graphics.RectF` instance methods on the CLI's `RectF`.

use notemaker_cli::canvas::RectF;

/// `android.graphics.RectF` methods missing from the backend-neutral
/// `notemaker_cli::canvas::RectF` (which has `is_empty`, `intersects` and
/// `union`). Semantics follow the Android implementation.
pub trait RectFExt {
    fn width(&self) -> f32;
    fn height(&self) -> f32;
    fn center_x(&self) -> f32;
    fn center_y(&self) -> f32;
    /// `set(l, t, r, b)`.
    fn set(&mut self, left: f32, top: f32, right: f32, bottom: f32);
    /// `setEmpty()`.
    fn set_empty(&mut self);
    /// `contains(x, y)`.
    fn contains_point(&self, x: f32, y: f32) -> bool;
    /// `contains(RectF)`.
    fn contains_rect(&self, r: &RectF) -> bool;
    /// `intersect(l, t, r, b)`: shrinks to the intersection when non-empty.
    fn intersect(&mut self, left: f32, top: f32, right: f32, bottom: f32) -> bool;
    /// `offset(dx, dy)`.
    fn offset(&mut self, dx: f32, dy: f32);
    /// `inset(dx, dy)`.
    fn inset(&mut self, dx: f32, dy: f32);
}
impl RectFExt for RectF {
    fn width(&self) -> f32 {
        self.right - self.left
    }
    fn height(&self) -> f32 {
        self.bottom - self.top
    }
    fn center_x(&self) -> f32 {
        (self.left + self.right) * 0.5
    }
    fn center_y(&self) -> f32 {
        (self.top + self.bottom) * 0.5
    }
    fn set(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        *self = RectF { left, top, right, bottom };
    }
    fn set_empty(&mut self) {
        *self = RectF { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
    }
    fn contains_point(&self, x: f32, y: f32) -> bool {
        self.left < self.right && self.top < self.bottom && x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
    fn contains_rect(&self, r: &RectF) -> bool {
        self.left < self.right && self.top < self.bottom && self.left <= r.left && self.top <= r.top && self.right >= r.right && self.bottom >= r.bottom
    }
    fn intersect(&mut self, left: f32, top: f32, right: f32, bottom: f32) -> bool {
        if self.left < right && left < self.right && self.top < bottom && top < self.bottom {
            if self.left < left {
                self.left = left;
            }
            if self.top < top {
                self.top = top;
            }
            if self.right > right {
                self.right = right;
            }
            if self.bottom > bottom {
                self.bottom = bottom;
            }
            return true;
        }
        false
    }
    fn offset(&mut self, dx: f32, dy: f32) {
        self.left += dx;
        self.top += dy;
        self.right += dx;
        self.bottom += dy;
    }
    fn inset(&mut self, dx: f32, dy: f32) {
        self.left += dx;
        self.top += dy;
        self.right -= dx;
        self.bottom -= dy;
    }
}
