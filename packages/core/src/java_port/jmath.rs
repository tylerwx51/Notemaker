//! Java `Math`/`StrictMath` semantics where Rust's std differs.
//!
//! OpenJDK's `Math.atan2`, `Math.acos` and `Math.hypot` delegate to fdlibm
//! (`StrictMath`), which is not what glibc or bionic compute bit-for-bit, so
//! they are ported here. `Math.sin`/`Math.cos` are platform intrinsics in
//! Java (HotSpot stubs on the JVM, bionic libm on Android); they use the
//! platform libm here as well (see the crate README for measured parity).
// fdlibm/FloatingDecimal constants and idioms are kept verbatim for bit-exact results.
#![allow(clippy::approx_constant, clippy::excessive_precision, clippy::eq_op)]

#[inline]
fn hi(x: f64) -> i32 {
    (x.to_bits() >> 32) as i32
}
#[inline]
fn lo(x: f64) -> u32 {
    x.to_bits() as u32
}
#[inline]
fn with_hi(x: f64, high: i32) -> f64 {
    f64::from_bits(((high as u32 as u64) << 32) | (x.to_bits() & 0xffff_ffff))
}
#[inline]
fn with_lo(x: f64, low: u32) -> f64 {
    f64::from_bits((x.to_bits() & 0xffff_ffff_0000_0000) | low as u64)
}
#[inline]
fn from_words(high: u32, low: u32) -> f64 {
    f64::from_bits(((high as u64) << 32) | low as u64)
}

const ATANHI: [f64; 4] = [
    4.63647609000806093515e-01,
    7.85398163397448278999e-01,
    9.82793723247329054082e-01,
    1.57079632679489655800e+00,
];
const ATANLO: [f64; 4] = [
    2.26987774529616870924e-17,
    3.06161699786838301793e-17,
    1.39033110312309984516e-17,
    6.12323399573676603587e-17,
];
const AT: [f64; 11] = [
    3.33333333333329318027e-01,
    -1.99999999998764832476e-01,
    1.42857142725034663711e-01,
    -1.11111104054623557880e-01,
    9.09088713343650656196e-02,
    -7.69187620504482999495e-02,
    6.66107313738753120669e-02,
    -5.83357013379057348645e-02,
    4.97687799461593236017e-02,
    -3.65315727442169155270e-02,
    1.62858201153657823623e-02,
];

/// fdlibm `s_atan.c` (`StrictMath.atan`).
pub fn atan(mut x: f64) -> f64 {
    let hx = hi(x);
    let ix = hx & 0x7fffffff;
    let id: i32;
    if ix >= 0x44100000 {
        if ix > 0x7ff00000 || (ix == 0x7ff00000 && lo(x) != 0) {
            return x + x;
        }
        return if hx > 0 { ATANHI[3] + ATANLO[3] } else { -ATANHI[3] - ATANLO[3] };
    }
    if ix < 0x3fdc0000 {
        if ix < 0x3e200000 && 1.0e300 + x > 1.0 {
            return x;
        }
        id = -1;
    } else {
        x = x.abs();
        if ix < 0x3ff30000 {
            if ix < 0x3fe60000 {
                id = 0;
                x = (2.0 * x - 1.0) / (2.0 + x);
            } else {
                id = 1;
                x = (x - 1.0) / (x + 1.0);
            }
        } else if ix < 0x40038000 {
            id = 2;
            x = (x - 1.5) / (1.0 + 1.5 * x);
        } else {
            id = 3;
            x = -1.0 / x;
        }
    }
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    if id < 0 {
        x - x * (s1 + s2)
    } else {
        let z = ATANHI[id as usize] - ((x * (s1 + s2) - ATANLO[id as usize]) - x);
        if hx < 0 { -z } else { z }
    }
}

/// fdlibm `e_atan2.c` (`Math.atan2` / `StrictMath.atan2`).
pub fn atan2(y: f64, x: f64) -> f64 {
    const TINY: f64 = 1.0e-300;
    const PI_O_4: f64 = 7.8539816339744827900E-01;
    const PI_O_2: f64 = 1.5707963267948965580E+00;
    const PI: f64 = 3.1415926535897931160E+00;
    const PI_LO: f64 = 1.2246467991473531772E-16;
    let hx = hi(x);
    let ix = hx & 0x7fffffff;
    let lx = lo(x);
    let hy = hi(y);
    let iy = hy & 0x7fffffff;
    let ly = lo(y);
    if (ix as u32 | ((lx | lx.wrapping_neg()) >> 31)) > 0x7ff00000
        || (iy as u32 | ((ly | ly.wrapping_neg()) >> 31)) > 0x7ff00000
    {
        return x + y;
    }
    if (hx.wrapping_sub(0x3ff00000) as u32 | lx) == 0 {
        return atan(y);
    }
    let m = ((hy >> 31) & 1) | ((hx >> 30) & 2);
    if (iy as u32 | ly) == 0 {
        match m {
            0 | 1 => return y,
            2 => return PI + TINY,
            _ => return -PI - TINY,
        }
    }
    if (ix as u32 | lx) == 0 {
        return if hy < 0 { -PI_O_2 - TINY } else { PI_O_2 + TINY };
    }
    if ix == 0x7ff00000 {
        if iy == 0x7ff00000 {
            return match m {
                0 => PI_O_4 + TINY,
                1 => -PI_O_4 - TINY,
                2 => 3.0 * PI_O_4 + TINY,
                _ => -3.0 * PI_O_4 - TINY,
            };
        } else {
            return match m {
                0 => 0.0,
                1 => -0.0,
                2 => PI + TINY,
                _ => -PI - TINY,
            };
        }
    }
    if iy == 0x7ff00000 {
        return if hy < 0 { -PI_O_2 - TINY } else { PI_O_2 + TINY };
    }
    let k = (iy - ix) >> 20;
    let z = if k > 60 {
        PI_O_2 + 0.5 * PI_LO
    } else if hx < 0 && k < -60 {
        0.0
    } else {
        atan((y / x).abs())
    };
    match m {
        0 => z,
        1 => with_hi(z, hi(z) ^ (0x80000000u32 as i32)),
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

/// fdlibm `e_acos.c` (`Math.acos` / `StrictMath.acos`).
pub fn acos(x: f64) -> f64 {
    const PI: f64 = 3.14159265358979311600e+00;
    const PIO2_HI: f64 = 1.57079632679489655800e+00;
    const PIO2_LO: f64 = 6.12323399573676603587e-17;
    const PS0: f64 = 1.66666666666666657415e-01;
    const PS1: f64 = -3.25565818622400915405e-01;
    const PS2: f64 = 2.01212532134862925881e-01;
    const PS3: f64 = -4.00555345006794114027e-02;
    const PS4: f64 = 7.91534994289814532176e-04;
    const PS5: f64 = 3.47933107596021167570e-05;
    const QS1: f64 = -2.40339491173441421878e+00;
    const QS2: f64 = 2.02094576023350569471e+00;
    const QS3: f64 = -6.88283971605453293030e-01;
    const QS4: f64 = 7.70381505559019352791e-02;
    let hx = hi(x);
    let ix = hx & 0x7fffffff;
    if ix >= 0x3ff00000 {
        if ((ix - 0x3ff00000) as u32 | lo(x)) == 0 {
            return if hx > 0 { 0.0 } else { PI + 2.0 * PIO2_LO };
        }
        return (x - x) / (x - x);
    }
    if ix < 0x3fe00000 {
        if ix <= 0x3c600000 {
            return PIO2_HI + PIO2_LO;
        }
        let z = x * x;
        let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
        let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
        let r = p / q;
        PIO2_HI - (x - (PIO2_LO - x * r))
    } else if hx < 0 {
        let z = (1.0 + x) * 0.5;
        let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
        let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
        let s = z.sqrt();
        let r = p / q;
        let w = r * s - PIO2_LO;
        PI - 2.0 * (s + w)
    } else {
        let z = (1.0 - x) * 0.5;
        let s = z.sqrt();
        let df = with_lo(s, 0);
        let c = (z - df * df) / (s + df);
        let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
        let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
        let r = p / q;
        let w = r * s + c;
        2.0 * (df + w)
    }
}

/// OpenJDK 17 `FdLibm.Hypot.compute` (`Math.hypot`).
pub fn hypot(x: f64, y: f64) -> f64 {
    let mut a = x.abs();
    let mut b = y.abs();
    if !a.is_finite() || !b.is_finite() {
        if a == f64::INFINITY || b == f64::INFINITY {
            return f64::INFINITY;
        }
        return a + b;
    }
    if b > a {
        std::mem::swap(&mut a, &mut b);
    }
    let mut ha = hi(a);
    let mut hb = hi(b);
    if ha.wrapping_sub(hb) > 0x3c00000 {
        return a + b;
    }
    let mut k = 0;
    if a > from_words(0x5f300000, 0xffffffff) {
        // a > ~2**500 (0x1.00000_ffff_ffffp500)
        ha -= 0x25800000;
        hb -= 0x25800000;
        a *= from_words(0x1a700000, 0); // 2^-600
        b *= from_words(0x1a700000, 0);
        k += 600;
    }
    let mut t1: f64;
    let t2: f64;
    if b < from_words(0x20b00000, 0) {
        // b < 2**-500
        if b < f64::MIN_POSITIVE {
            if b == 0.0 {
                return a;
            }
            t1 = from_words(0x7fd00000, 0); // 2^1022
            b *= t1;
            a *= t1;
            k -= 1022;
        } else {
            ha += 0x25800000;
            hb += 0x25800000;
            a *= from_words(0x65700000, 0); // 2^600
            b *= from_words(0x65700000, 0);
            k -= 600;
        }
    }
    let mut w = a - b;
    if w > b {
        t1 = with_hi(0.0, ha);
        t2 = a - t1;
        w = (t1 * t1 - (b * (-b) - t2 * (a + t1))).sqrt();
    } else {
        a += a;
        let y1 = with_hi(0.0, hb);
        let y2 = b - y1;
        t1 = with_hi(0.0, ha + 0x00100000);
        let t2 = a - t1;
        w = (t1 * y1 - (w * (-w) - (t1 * y2 + t2 * b))).sqrt();
    }
    if k != 0 {
        power_of_two(k) * w
    } else {
        w
    }
}

fn power_of_two(n: i32) -> f64 {
    f64::from_bits(((n + 1023) as u64) << 52)
}

/// `Math.toRadians` (JDK 9+: `angdeg * DEGREES_TO_RADIANS`).
#[inline]
pub fn to_radians(angdeg: f64) -> f64 {
    angdeg * 0.017453292519943295
}

/// `Math.sin` (platform libm, like the Java intrinsic).
#[inline]
pub fn sin(x: f64) -> f64 {
    x.sin()
}

/// `Math.cos` (platform libm, like the Java intrinsic).
#[inline]
pub fn cos(x: f64) -> f64 {
    x.cos()
}

/// `Math.round(double)` (JDK 8+ exact algorithm).
pub fn round(a: f64) -> i64 {
    let long_bits = a.to_bits() as i64;
    let biased_exp = (long_bits & 0x7FF0_0000_0000_0000) >> 52;
    let shift = (52 - 1 + 1023) - biased_exp;
    if (shift & -64) == 0 {
        let mut r = (long_bits & 0x000F_FFFF_FFFF_FFFF) | 0x0010_0000_0000_0000;
        if long_bits < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        a as i64
    }
}

/// `Math.rint` (round half to even).
#[inline]
pub fn rint(a: f64) -> f64 {
    a.round_ties_even()
}

/// Java `(int) d`: truncation toward zero, saturating, NaN to 0.
#[inline]
pub fn d2i(d: f64) -> i32 {
    d as i32
}

/// Java `(long) d`.
#[inline]
pub fn d2l(d: f64) -> i64 {
    d as i64
}

/// `Math.floorDiv(int,int)`.
#[inline]
pub fn floor_div(x: i32, y: i32) -> i32 {
    let q = x.wrapping_div(y);
    if (x ^ y) < 0 && q.wrapping_mul(y) != x { q - 1 } else { q }
}

/// `Math.min(double,double)` with Java NaN/-0.0 semantics.
#[inline]
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if b.is_nan() {
        return b;
    }
    if a == 0.0 && b == 0.0 && (a.is_sign_negative() || b.is_sign_negative()) {
        return -0.0;
    }
    if a <= b { a } else { b }
}

/// `Math.max(double,double)` with Java NaN/-0.0 semantics.
#[inline]
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if b.is_nan() {
        return b;
    }
    if a == 0.0 && b == 0.0 && (a.is_sign_positive() || b.is_sign_positive()) {
        return 0.0;
    }
    if a >= b { a } else { b }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_matches_java() {
        assert_eq!(round(0.49999999999999994), 0);
        assert_eq!(round(-0.5), 0);
        assert_eq!(round(2.5), 3);
        assert_eq!(round(-2.5), -2);
        assert_eq!(round(f64::NAN), 0);
        assert_eq!(round(1e300), i64::MAX);
    }
    #[test]
    fn fdlibm_specials() {
        assert_eq!(atan2(0.0, -1.0), std::f64::consts::PI);
        assert_eq!(acos(-1.0), std::f64::consts::PI);
        assert_eq!(hypot(3.0, 4.0), 5.0);
        assert_eq!(floor_div(-1, 64), -1);
        assert_eq!(floor_div(64, 64), 1);
    }
}
