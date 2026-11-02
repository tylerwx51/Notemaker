//! Java-compatible number formatting and parsing.
//!
//! `double`/`float` are ports of OpenJDK 17 `jdk.internal.math.FloatingDecimal`
//! (`BinaryToASCIIBuffer.dtoa` + `getChars`), which is what `Double.toString`,
//! `String.valueOf(double)` and `StringBuilder.append(double)` use on the JDK 17
//! JVM and on Android. Note that this algorithm is *not* always the shortest
//! round-trip representation (for example `1e23` prints `9.999999999999999E22`),
//! so a generic shortest formatter would not be byte compatible.
// fdlibm/FloatingDecimal constants and idioms are kept verbatim for bit-exact results.
#![allow(clippy::approx_constant, clippy::excessive_precision, clippy::eq_op)]

use crate::error::{Error, Result};

const EXP_SHIFT: i32 = 52;
const FRACT_HOB: i64 = 1 << EXP_SHIFT;
const EXP_ONE: i64 = 0x3ff << EXP_SHIFT;
const MAX_SMALL_BIN_EXP: i32 = 62;
const MIN_SMALL_BIN_EXP: i32 = -(63 / 3);
const SIGNIF_BIT_MASK: i64 = 0x000F_FFFF_FFFF_FFFF;
const EXP_BIT_MASK: i64 = 0x7FF0_0000_0000_0000;
const SIGN_BIT_MASK: i64 = i64::MIN;
const EXP_BIAS: i32 = 1023;

const SMALL_5_POW: [i32; 14] = [
    1, 5, 25, 125, 625, 3125, 15625, 78125, 390625, 1953125, 9765625, 48828125, 244140625,
    1220703125,
];
const LONG_5_POW: [i64; 27] = {
    let mut t = [1i64; 27];
    let mut i = 1;
    while i < 27 {
        t[i] = t[i - 1] * 5;
        i += 1;
    }
    t
};
const N_5_BITS: [i32; 27] = [
    0, 3, 5, 7, 10, 12, 14, 17, 19, 21, 24, 26, 28, 31, 33, 35, 38, 40, 42, 45, 47, 49, 52, 54,
    56, 59, 61,
];
const INSIGNIFICANT_DIGITS_NUMBER: [i32; 64] = [
    0, 0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 5, 5, 5, 6, 6, 6, 6, 7, 7, 7, 8, 8, 8, 9,
    9, 9, 9, 10, 10, 10, 11, 11, 11, 12, 12, 12, 12, 13, 13, 13, 14, 14, 14, 15, 15, 15, 15, 16,
    16, 16, 17, 17, 17, 18, 18, 18, 19,
];

/// Minimal exact unsigned big integer (little-endian u32 limbs). The Java
/// `FDBigInteger` normalization bias scales every operand by the same power of
/// two, so exact arithmetic without the bias yields identical decisions.
#[derive(Clone, Debug)]
struct Big(Vec<u32>);

impl Big {
    fn from_u64(v: u64) -> Big {
        let mut b = Big(vec![v as u32, (v >> 32) as u32]);
        b.trim();
        b
    }
    fn trim(&mut self) {
        while let Some(&0) = self.0.last() {
            self.0.pop();
        }
    }
    fn is_zero(&self) -> bool {
        self.0.is_empty()
    }
    fn mul_small(&mut self, m: u32) {
        let mut carry: u64 = 0;
        for limb in self.0.iter_mut() {
            let v = (*limb as u64) * (m as u64) + carry;
            *limb = v as u32;
            carry = v >> 32;
        }
        if carry != 0 {
            self.0.push(carry as u32);
        }
    }
    fn mul_pow5(&mut self, mut n: i32) {
        while n >= 13 {
            self.mul_small(1220703125);
            n -= 13;
        }
        if n > 0 {
            self.mul_small(SMALL_5_POW[n as usize] as u32);
        }
    }
    fn shl(&mut self, bits: i32) {
        if self.is_zero() || bits == 0 {
            return;
        }
        let words = (bits / 32) as usize;
        let rem = (bits % 32) as u32;
        let mut out = vec![0u32; words];
        if rem == 0 {
            out.extend_from_slice(&self.0);
        } else {
            let mut carry = 0u32;
            for &limb in &self.0 {
                out.push((limb << rem) | carry);
                carry = limb >> (32 - rem);
            }
            if carry != 0 {
                out.push(carry);
            }
        }
        self.0 = out;
    }
    fn pow52(p5: i32, p2: i32) -> Big {
        let mut b = Big(vec![1]);
        b.mul_pow5(p5);
        b.shl(p2);
        b
    }
    fn cmp(&self, other: &Big) -> std::cmp::Ordering {
        if self.0.len() != other.0.len() {
            return self.0.len().cmp(&other.0.len());
        }
        for i in (0..self.0.len()).rev() {
            if self.0[i] != other.0[i] {
                return self.0[i].cmp(&other.0[i]);
            }
        }
        std::cmp::Ordering::Equal
    }
    fn add(&self, other: &Big) -> Big {
        let n = self.0.len().max(other.0.len());
        let mut out = Vec::with_capacity(n + 1);
        let mut carry = 0u64;
        for i in 0..n {
            let v = *self.0.get(i).unwrap_or(&0) as u64 + *other.0.get(i).unwrap_or(&0) as u64 + carry;
            out.push(v as u32);
            carry = v >> 32;
        }
        if carry != 0 {
            out.push(carry as u32);
        }
        let mut b = Big(out);
        b.trim();
        b
    }
    /// self -= other, requires self >= other.
    fn sub_assign(&mut self, other: &Big) {
        let mut borrow = 0i64;
        for i in 0..self.0.len() {
            let v = self.0[i] as i64 - *other.0.get(i).unwrap_or(&0) as i64 - borrow;
            if v < 0 {
                self.0[i] = (v + (1i64 << 32)) as u32;
                borrow = 1;
            } else {
                self.0[i] = v as u32;
                borrow = 0;
            }
        }
        self.trim();
    }
    /// `FDBigInteger.quoRemIteration`: q = this / s; this = (this % s) * 10.
    fn quo_rem_iteration(&mut self, s: &Big) -> i32 {
        let mut q = 0;
        while self.cmp(s) != std::cmp::Ordering::Less {
            self.sub_assign(s);
            q += 1;
        }
        self.mul_small(10);
        q
    }
}

struct Buffer {
    is_negative: bool,
    dec_exponent: i32,
    first_digit_index: i32,
    n_digits: i32,
    digits: [u8; 20],
}

impl Buffer {
    fn develop_long_digits(&mut self, mut dec_exponent: i32, mut lvalue: i64, insignificant_digits: i32) {
        if insignificant_digits != 0 {
            let pow10 = LONG_5_POW[insignificant_digits as usize] << insignificant_digits;
            let residue = lvalue % pow10;
            lvalue /= pow10;
            dec_exponent += insignificant_digits;
            if residue >= (pow10 >> 1) {
                lvalue += 1;
            }
        }
        let mut digitno = self.digits.len() as i32 - 1;
        let mut c: i32;
        if lvalue <= i32::MAX as i64 {
            let mut ivalue = lvalue as i32;
            c = ivalue % 10;
            ivalue /= 10;
            while c == 0 {
                dec_exponent += 1;
                c = ivalue % 10;
                ivalue /= 10;
            }
            while ivalue != 0 {
                self.digits[digitno as usize] = (c as u8) + b'0';
                digitno -= 1;
                dec_exponent += 1;
                c = ivalue % 10;
                ivalue /= 10;
            }
            self.digits[digitno as usize] = (c as u8) + b'0';
        } else {
            c = (lvalue % 10) as i32;
            lvalue /= 10;
            while c == 0 {
                dec_exponent += 1;
                c = (lvalue % 10) as i32;
                lvalue /= 10;
            }
            while lvalue != 0 {
                self.digits[digitno as usize] = (c as u8) + b'0';
                digitno -= 1;
                dec_exponent += 1;
                c = (lvalue % 10) as i32;
                lvalue /= 10;
            }
            self.digits[digitno as usize] = (c as u8) + b'0';
        }
        self.dec_exponent = dec_exponent + 1;
        self.first_digit_index = digitno;
        self.n_digits = self.digits.len() as i32 - digitno;
    }

    fn roundup(&mut self) {
        let first = self.first_digit_index as usize;
        let mut i = (self.first_digit_index + self.n_digits - 1) as usize;
        let mut q = self.digits[i];
        if q == b'9' {
            while q == b'9' && i > first {
                self.digits[i] = b'0';
                i -= 1;
                q = self.digits[i];
            }
            if q == b'9' {
                self.dec_exponent += 1;
                self.digits[first] = b'1';
                return;
            }
        }
        self.digits[i] = q + 1;
    }

    fn dtoa(&mut self, bin_exp: i32, mut fract_bits: i64, n_significant_bits: i32, is_compatible_format: bool) {
        let tail_zeros = fract_bits.trailing_zeros() as i32;
        let n_fract_bits = EXP_SHIFT + 1 - tail_zeros;
        let n_tiny_bits = 0.max(n_fract_bits - bin_exp - 1);
        if (MIN_SMALL_BIN_EXP..=MAX_SMALL_BIN_EXP).contains(&bin_exp)
            && (n_tiny_bits as usize) < LONG_5_POW.len()
            && (n_fract_bits + N_5_BITS[n_tiny_bits as usize]) < 64
            && n_tiny_bits == 0
        {
            let insignificant = if bin_exp > n_significant_bits {
                insignificant_digits_for_pow2(bin_exp - n_significant_bits - 1)
            } else {
                0
            };
            if bin_exp >= EXP_SHIFT {
                fract_bits <<= bin_exp - EXP_SHIFT;
            } else {
                fract_bits = ((fract_bits as u64) >> (EXP_SHIFT - bin_exp)) as i64;
            }
            self.develop_long_digits(0, fract_bits, insignificant);
            return;
        }
        let mut dec_exp = estimate_dec_exp(fract_bits, bin_exp);
        let b5 = 0.max(-dec_exp);
        let mut b2 = b5 + n_tiny_bits + bin_exp;
        let s5 = 0.max(dec_exp);
        let mut s2 = s5 + n_tiny_bits;
        let m5 = b5;
        let mut m2 = b2 - n_significant_bits;
        fract_bits = ((fract_bits as u64) >> tail_zeros) as i64;
        b2 -= n_fract_bits - 1;
        let common2factor = b2.min(s2);
        b2 -= common2factor;
        s2 -= common2factor;
        m2 -= common2factor;
        if n_fract_bits == 1 {
            m2 -= 1;
        }
        if m2 < 0 {
            b2 -= m2;
            s2 -= m2;
            m2 = 0;
        }
        let mut ndigit: usize = 0;
        let mut low: bool;
        let mut high: bool;
        let low_digit_difference: i64;
        let mut q: i32;
        let bbits = n_fract_bits + b2 + if (b5 as usize) < N_5_BITS.len() { N_5_BITS[b5 as usize] } else { b5 * 3 };
        let ten_sbits = s2 + 1 + if ((s5 + 1) as usize) < N_5_BITS.len() { N_5_BITS[(s5 + 1) as usize] } else { (s5 + 1) * 3 };
        if bbits < 64 && ten_sbits < 64 {
            if bbits < 32 && ten_sbits < 32 {
                let mut b: i32 = (fract_bits as i32).wrapping_mul(SMALL_5_POW[b5 as usize]).wrapping_shl(b2 as u32);
                let s: i32 = SMALL_5_POW[s5 as usize].wrapping_shl(s2 as u32);
                let mut m: i32 = SMALL_5_POW[m5 as usize].wrapping_shl(m2 as u32);
                let tens: i32 = s.wrapping_mul(10);
                q = b / s;
                b = 10i32.wrapping_mul(b % s);
                m = m.wrapping_mul(10);
                low = b < m;
                high = b.wrapping_add(m) > tens;
                if q == 0 && !high {
                    dec_exp -= 1;
                } else {
                    self.digits[ndigit] = b'0' + q as u8;
                    ndigit += 1;
                }
                if !is_compatible_format || !(-3..8).contains(&dec_exp) {
                    high = false;
                    low = false;
                }
                while !low && !high {
                    q = b / s;
                    b = 10i32.wrapping_mul(b % s);
                    m = m.wrapping_mul(10);
                    if m > 0 {
                        low = b < m;
                        high = b.wrapping_add(m) > tens;
                    } else {
                        low = true;
                        high = true;
                    }
                    self.digits[ndigit] = b'0' + q as u8;
                    ndigit += 1;
                }
                // Java computes (b<<1)-tens in int arithmetic, then widens.
                low_digit_difference = (b.wrapping_shl(1)).wrapping_sub(tens) as i64;
            } else {
                let mut b: i64 = fract_bits.wrapping_mul(LONG_5_POW[b5 as usize]).wrapping_shl(b2 as u32);
                let s: i64 = LONG_5_POW[s5 as usize].wrapping_shl(s2 as u32);
                let mut m: i64 = LONG_5_POW[m5 as usize].wrapping_shl(m2 as u32);
                let tens: i64 = s.wrapping_mul(10);
                q = (b / s) as i32;
                b = 10i64.wrapping_mul(b % s);
                m = m.wrapping_mul(10);
                low = b < m;
                high = b.wrapping_add(m) > tens;
                if q == 0 && !high {
                    dec_exp -= 1;
                } else {
                    self.digits[ndigit] = b'0' + q as u8;
                    ndigit += 1;
                }
                if !is_compatible_format || !(-3..8).contains(&dec_exp) {
                    high = false;
                    low = false;
                }
                while !low && !high {
                    q = (b / s) as i32;
                    b = 10i64.wrapping_mul(b % s);
                    m = m.wrapping_mul(10);
                    if m > 0 {
                        low = b < m;
                        high = b.wrapping_add(m) > tens;
                    } else {
                        low = true;
                        high = true;
                    }
                    self.digits[ndigit] = b'0' + q as u8;
                    ndigit += 1;
                }
                low_digit_difference = b.wrapping_shl(1).wrapping_sub(tens);
            }
        } else {
            let s_val = Big::pow52(s5, s2);
            let mut b_val = Big::from_u64(fract_bits as u64);
            b_val.mul_pow5(b5);
            b_val.shl(b2);
            let mut m_val = Big::pow52(m5 + 1, m2 + 1);
            let ten_s_val = Big::pow52(s5 + 1, s2 + 1);
            q = b_val.quo_rem_iteration(&s_val);
            low = b_val.cmp(&m_val) == std::cmp::Ordering::Less;
            high = ten_s_val.cmp(&b_val.add(&m_val)) != std::cmp::Ordering::Greater;
            if q == 0 && !high {
                dec_exp -= 1;
            } else {
                self.digits[ndigit] = b'0' + q as u8;
                ndigit += 1;
            }
            if !is_compatible_format || !(-3..8).contains(&dec_exp) {
                high = false;
                low = false;
            }
            while !low && !high {
                q = b_val.quo_rem_iteration(&s_val);
                m_val.mul_small(10);
                low = b_val.cmp(&m_val) == std::cmp::Ordering::Less;
                high = ten_s_val.cmp(&b_val.add(&m_val)) != std::cmp::Ordering::Greater;
                self.digits[ndigit] = b'0' + q as u8;
                ndigit += 1;
            }
            if high && low {
                let mut doubled = b_val.clone();
                doubled.shl(1);
                low_digit_difference = match doubled.cmp(&ten_s_val) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                };
            } else {
                low_digit_difference = 0;
            }
        }
        self.finish(dec_exp, ndigit, high, low, low_digit_difference);
    }

    fn finish(&mut self, dec_exp: i32, ndigit: usize, high: bool, low: bool, low_digit_difference: i64) {
        self.dec_exponent = dec_exp + 1;
        self.first_digit_index = 0;
        self.n_digits = ndigit as i32;
        if high {
            if low {
                if low_digit_difference == 0 {
                    if (self.digits[(self.first_digit_index + self.n_digits - 1) as usize] & 1) != 0 {
                        self.roundup();
                    }
                } else if low_digit_difference > 0 {
                    self.roundup();
                }
            } else {
                self.roundup();
            }
        }
    }

    fn chars(&self, out: &mut String) {
        let digits = &self.digits[self.first_digit_index as usize..(self.first_digit_index + self.n_digits) as usize];
        let n_digits = self.n_digits;
        let dec_exponent = self.dec_exponent;
        if self.is_negative {
            out.push('-');
        }
        let push = |out: &mut String, d: &[u8]| {
            for &c in d {
                out.push(c as char);
            }
        };
        if dec_exponent > 0 && dec_exponent < 8 {
            let char_length = n_digits.min(dec_exponent);
            push(out, &digits[..char_length as usize]);
            if char_length < dec_exponent {
                for _ in 0..(dec_exponent - char_length) {
                    out.push('0');
                }
                out.push_str(".0");
            } else {
                out.push('.');
                if char_length < n_digits {
                    push(out, &digits[char_length as usize..]);
                } else {
                    out.push('0');
                }
            }
        } else if dec_exponent <= 0 && dec_exponent > -3 {
            out.push_str("0.");
            for _ in 0..(-dec_exponent) {
                out.push('0');
            }
            push(out, digits);
        } else {
            out.push(digits[0] as char);
            out.push('.');
            if n_digits > 1 {
                push(out, &digits[1..]);
            } else {
                out.push('0');
            }
            out.push('E');
            let e = if dec_exponent <= 0 {
                out.push('-');
                -dec_exponent + 1
            } else {
                dec_exponent - 1
            };
            out.push_str(&e.to_string());
        }
    }
}

fn insignificant_digits_for_pow2(p2: i32) -> i32 {
    if p2 > 1 && (p2 as usize) < INSIGNIFICANT_DIGITS_NUMBER.len() {
        INSIGNIFICANT_DIGITS_NUMBER[p2 as usize]
    } else {
        0
    }
}

fn estimate_dec_exp(fract_bits: i64, bin_exp: i32) -> i32 {
    let d2 = f64::from_bits((EXP_ONE | (fract_bits & SIGNIF_BIT_MASK)) as u64);
    let d = (d2 - 1.5) * 0.289529654 + 0.176091259 + (bin_exp as f64) * 0.301029995663981;
    let d_bits = d.to_bits() as i64;
    let exponent = (((d_bits & EXP_BIT_MASK) >> EXP_SHIFT) as i32) - EXP_BIAS;
    let is_negative = (d_bits & SIGN_BIT_MASK) != 0;
    if (0..52).contains(&exponent) {
        let mask = SIGNIF_BIT_MASK >> exponent;
        let r = (((d_bits & SIGNIF_BIT_MASK) | FRACT_HOB) >> (EXP_SHIFT - exponent)) as i32;
        if is_negative {
            if (mask & d_bits) == 0 { -r } else { -r - 1 }
        } else {
            r
        }
    } else if exponent < 0 {
        if (d_bits & !SIGN_BIT_MASK) == 0 {
            0
        } else if is_negative {
            -1
        } else {
            0
        }
    } else {
        d as i32
    }
}

/// Append Java `Double.toString(d)` to `out`.
pub fn append_double(out: &mut String, d: f64) {
    let d_bits = d.to_bits() as i64;
    let is_negative = (d_bits & SIGN_BIT_MASK) != 0;
    let mut fract_bits = d_bits & SIGNIF_BIT_MASK;
    let mut bin_exp = ((d_bits & EXP_BIT_MASK) >> EXP_SHIFT) as i32;
    if bin_exp == (EXP_BIT_MASK >> EXP_SHIFT) as i32 {
        if fract_bits == 0 {
            out.push_str(if is_negative { "-Infinity" } else { "Infinity" });
        } else {
            out.push_str("NaN");
        }
        return;
    }
    let n_significant_bits;
    if bin_exp == 0 {
        if fract_bits == 0 {
            out.push_str(if is_negative { "-0.0" } else { "0.0" });
            return;
        }
        let leading_zeros = fract_bits.leading_zeros() as i32;
        let shift = leading_zeros - (63 - EXP_SHIFT);
        fract_bits <<= shift;
        bin_exp = 1 - shift;
        n_significant_bits = 64 - leading_zeros;
    } else {
        fract_bits |= FRACT_HOB;
        n_significant_bits = EXP_SHIFT + 1;
    }
    bin_exp -= EXP_BIAS;
    let mut buf = Buffer { is_negative, dec_exponent: 0, first_digit_index: 0, n_digits: 0, digits: [0; 20] };
    buf.dtoa(bin_exp, fract_bits, n_significant_bits, true);
    buf.chars(out);
}

/// Java `Double.toString(d)`.
pub fn double(d: f64) -> String {
    let mut s = String::with_capacity(24);
    append_double(&mut s, d);
    s
}

/// Append Java `Float.toString(f)` to `out`.
pub fn append_float(out: &mut String, f: f32) {
    const SINGLE_EXP_SHIFT: i32 = 23;
    const SINGLE_FRACT_HOB: i32 = 1 << SINGLE_EXP_SHIFT;
    let f_bits = f.to_bits() as i32;
    let is_negative = f_bits < 0;
    let mut fract_bits = f_bits & 0x007F_FFFF;
    let mut bin_exp = (f_bits & 0x7F80_0000) >> SINGLE_EXP_SHIFT;
    if bin_exp == 0xFF {
        if fract_bits == 0 {
            out.push_str(if is_negative { "-Infinity" } else { "Infinity" });
        } else {
            out.push_str("NaN");
        }
        return;
    }
    let n_significant_bits;
    if bin_exp == 0 {
        if fract_bits == 0 {
            out.push_str(if is_negative { "-0.0" } else { "0.0" });
            return;
        }
        let leading_zeros = fract_bits.leading_zeros() as i32;
        let shift = leading_zeros - (31 - SINGLE_EXP_SHIFT);
        fract_bits <<= shift;
        bin_exp = 1 - shift;
        n_significant_bits = 32 - leading_zeros;
    } else {
        fract_bits |= SINGLE_FRACT_HOB;
        n_significant_bits = SINGLE_EXP_SHIFT + 1;
    }
    bin_exp -= 127;
    let mut buf = Buffer { is_negative, dec_exponent: 0, first_digit_index: 0, n_digits: 0, digits: [0; 20] };
    buf.dtoa(bin_exp, (fract_bits as i64) << (EXP_SHIFT - SINGLE_EXP_SHIFT), n_significant_bits, true);
    buf.chars(out);
}

/// Java `Float.toString(f)`.
pub fn float(f: f32) -> String {
    let mut s = String::with_capacity(16);
    append_float(&mut s, f);
    s
}

/// Java `Long.toString(v)`.
pub fn long(v: i64) -> String {
    v.to_string()
}

/// `String.format(Locale.ROOT, "#%06x", c & 0xffffff)`.
pub fn hex_color(c: i32) -> String {
    format!("#{:06x}", c & 0xffffff)
}

/// Java `String.trim()`: strips leading/trailing chars `<= ' '`.
pub fn java_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// Java `Double.parseDouble(s)`, including the `NumberFormatException`
/// messages (`empty String`, `For input string: "..."`).
pub fn parse_double(s: &str) -> Result<f64> {
    let t = java_trim(s);
    let fail = || Error::number_format(format!("For input string: \"{}\"", t));
    if t.is_empty() {
        return Err(Error::number_format("empty String"));
    }
    let bytes = t.as_bytes();
    let mut i = 0;
    let mut negative = false;
    if bytes[0] == b'+' || bytes[0] == b'-' {
        negative = bytes[0] == b'-';
        i = 1;
    }
    let rest = &t[i..];
    if rest == "NaN" {
        return Ok(f64::NAN);
    }
    if rest == "Infinity" {
        return Ok(if negative { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    if rest.len() >= 2 && (rest.starts_with("0x") || rest.starts_with("0X")) {
        return parse_hex_double(&rest[2..], negative).ok_or_else(fail);
    }
    // [digits][.digits][(e|E)[+-]digits][fFdD]
    let b = rest.as_bytes();
    let mut j = 0;
    let mut mantissa_digits = 0;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
        mantissa_digits += 1;
    }
    if j < b.len() && b[j] == b'.' {
        j += 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
            mantissa_digits += 1;
        }
    }
    if mantissa_digits == 0 {
        return Err(fail());
    }
    
    if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
        j += 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == start {
            return Err(fail());
        }
    }
    let number_end = j;
    if j < b.len() && matches!(b[j], b'f' | b'F' | b'd' | b'D') {
        j += 1;
    }
    if j != b.len() {
        return Err(fail());
    }
    let mut text = &rest[..number_end];
    // Rust rejects a huge exponent digit string only by overflow; clamp is fine.
    if text.ends_with('.') {
        text = &text[..text.len() - 1];
    }
    let value: f64 = parse_decimal_lenient(text).ok_or_else(fail)?;
    Ok(if negative { -value } else { value })
}

fn parse_decimal_lenient(text: &str) -> Option<f64> {
    if let Ok(v) = text.parse::<f64>() {
        return Some(v);
    }
    // Exponents beyond Rust's accepted range: saturate like Java.
    let lower = text.to_ascii_lowercase();
    let (mant, exp) = lower.split_once('e')?;
    let mant_value: f64 = if mant.is_empty() { return None } else { mant.trim_end_matches('.').parse().ok()? };
    let negative_exp = exp.starts_with('-');
    if mant_value == 0.0 {
        return Some(0.0);
    }
    Some(if negative_exp { 0.0 } else { f64::INFINITY })
}

fn parse_hex_double(s: &str, negative: bool) -> Option<f64> {
    // 0x<hex>[.<hex>]p[+-]<dec>[fFdD]
    let lower = s.to_ascii_lowercase();
    let (mant, exp) = lower.split_once('p')?;
    let exp = exp.trim_end_matches(['f', 'd']);
    let exp: i32 = exp.parse().ok()?;
    let (int_part, frac_part) = match mant.split_once('.') {
        Some((a, b)) => (a, b),
        None => (mant, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let mut value = 0f64;
    for c in int_part.chars() {
        value = value * 16.0 + c.to_digit(16)? as f64;
    }
    let mut scale = 1.0 / 16.0;
    for c in frac_part.chars() {
        value += c.to_digit(16)? as f64 * scale;
        scale /= 16.0;
    }
    let v = value * 2f64.powi(exp);
    Some(if negative { -v } else { v })
}



const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `Base64.getEncoder().encodeToString(bytes)`.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(BASE64[(n >> 18) as usize & 63] as char);
        out.push(BASE64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { BASE64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { BASE64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// `Base64.getDecoder().decode(String)`, with Java's exact error messages.
/// Characters above U+00FF become `?` first (Java decodes ISO-8859-1 bytes).
pub fn base64_decode(text: &str) -> Result<Vec<u8>> {
    let src: Vec<u8> = text.chars().map(|c| if (c as u32) < 256 { c as u32 as u8 } else { b'?' }).collect();
    let from = |b: u8| -> i32 {
        match b {
            b'A'..=b'Z' => (b - b'A') as i32,
            b'a'..=b'z' => (b - b'a') as i32 + 26,
            b'0'..=b'9' => (b - b'0') as i32 + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => -2,
            _ => -1,
        }
    };
    let sl = src.len();
    if sl == 0 {
        return Ok(Vec::new());
    }
    if sl < 2 {
        return Err(Error::illegal_argument("Input byte[] should at least have 2 bytes for base64 bytes"));
    }
    let mut dst = Vec::with_capacity(sl / 4 * 3 + 3);
    let mut bits: i32 = 0;
    let mut shiftto = 18;
    let mut sp = 0;
    while sp < sl {
        let raw = src[sp];
        sp += 1;
        let b = from(raw);
        if b < 0 {
            if b == -2 {
                if (shiftto == 6 && (sp == sl || {
                    let next = src[sp];
                    sp += 1;
                    next != b'='
                })) || shiftto == 18
                {
                    return Err(Error::illegal_argument("Input byte array has wrong 4-byte ending unit"));
                }
                break;
            }
            return Err(Error::illegal_argument(format!(
                "Illegal base64 character {}",
                java_int_hex(raw as i8 as i32)
            )));
        }
        bits |= b << shiftto;
        shiftto -= 6;
        if shiftto < 0 {
            dst.push((bits >> 16) as u8);
            dst.push((bits >> 8) as u8);
            dst.push(bits as u8);
            shiftto = 18;
            bits = 0;
        }
    }
    if shiftto == 6 {
        dst.push((bits >> 16) as u8);
    } else if shiftto == 0 {
        dst.push((bits >> 16) as u8);
        dst.push((bits >> 8) as u8);
    } else if shiftto == 12 {
        return Err(Error::illegal_argument("Last unit does not have enough valid bits"));
    }
    if sp < sl {
        return Err(Error::illegal_argument(format!("Input byte array has incorrect ending byte at {}", sp)));
    }
    Ok(dst)
}

/// `Integer.toString(v, 16)` (signed, e.g. `-1e`).
fn java_int_hex(v: i32) -> String {
    if v < 0 { format!("-{:x}", -(v as i64)) } else { format!("{:x}", v) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn java_double_strings() {
        let cases: &[(f64, &str)] = &[
            (1.0, "1.0"),
            (0.1, "0.1"),
            (1e-5, "1.0E-5"),
            (1e21, "1.0E21"),
            (-0.0, "-0.0"),
            (0.001, "0.001"),
            (1e7, "1.0E7"),
            (9999999.999, "9999999.999"),
            (123456789.0, "1.23456789E8"),
            (1e23, "9.999999999999999E22"),
            (2e23, "1.9999999999999998E23"),
            (0.1 + 0.2, "0.30000000000000004"),
            (5e-324, "4.9E-324"),
            (f64::MAX, "1.7976931348623157E308"),
            (1.0 / 3.0, "0.3333333333333333"),
            (100.0 / 7.0, "14.285714285714286"),
            (2.82879384806159E17, "2.82879384806159008E17"),
            (1.1e-4, "1.1E-4"),
        ];
        for (v, s) in cases {
            assert_eq!(double(*v), *s, "{v:e}");
        }
    }
    #[test]
    fn java_parse_double() {
        assert_eq!(parse_double(" 7.5 ").unwrap(), 7.5);
        assert_eq!(parse_double("1.").unwrap(), 1.0);
        assert_eq!(parse_double("1e2d").unwrap(), 100.0);
        assert_eq!(parse_double("").unwrap_err().message(), "empty String");
        assert_eq!(parse_double("7.5px").unwrap_err().message(), "For input string: \"7.5px\"");
        assert!(parse_double("inf").is_err());
    }
}
