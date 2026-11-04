//! `java.lang.Math` for `float` (core's `jmath` covers `double`) and
//! `Number.floatValue()` of a JSON number.

use crate::error::AppResult;
use notemaker_core::json::{self, Value};

/// Java `Math.round(float)`: `(int) floor(x + 0.5f)` with NaN → 0 and
/// saturation at the `int` range.
pub fn round_f32(x: f32) -> i32 {
    if x.is_nan() {
        return 0;
    }
    let r = (x + 0.5).floor();
    if r >= i32::MAX as f32 {
        i32::MAX
    } else if r <= i32::MIN as f32 {
        i32::MIN
    } else {
        r as i32
    }
}

/// `((Number) value).floatValue()`.
pub fn float_value(value: Option<&Value>) -> AppResult<f32> {
    match value {
        Some(Value::Number(n)) => Ok(match n.as_i64() {
            Some(i) => i as f32,
            None => json::number_double(n) as f32,
        }),
        other => Ok(json::as_double(other)? as f32),
    }
}

/// Java `Math.min(float, float)` / `Math.max` (NaN propagates).
pub fn fmin(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else {
        a.min(b)
    }
}
pub fn fmax(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else {
        a.max(b)
    }
}
