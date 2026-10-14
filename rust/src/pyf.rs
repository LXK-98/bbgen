//! Python's float arithmetic, bit for bit, where it differs from the obvious Rust: `round(x, n)` (correctly
//! rounded to n decimals, ties to even), `math.dist` and `math.hypot` (CPython's vector_norm, more accurate
//! than `f64::hypot`), and `sum()` over floats (Neumaier's compensated summation since Python 3.12).
//!
//! The port has to make the same choices as the Python, and many choices are ties broken by comparing costs
//! that came out of these: a last-bit difference would pick another wire.

/// `round(x, n)` for n >= 0: the double nearest the decimal x rounds to.
pub fn round(x: f64, n: u32) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let k = round_scaled(x, n);
    let s = format!("{}e-{}", k, n);
    let r: f64 = s.parse().unwrap();
    if r == 0.0 && x.is_sign_negative() { -0.0 } else { r }
}

/// x * 10**n rounded to the nearest integer, ties to even, from x's exact binary value.
pub fn round_scaled(x: f64, n: u32) -> i64 {
    if x == 0.0 {
        return 0;
    }
    let bits = x.to_bits();
    let neg = bits >> 63 == 1;
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (mant, e) = if exp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), exp - 1075) };
    // |x| = mant * 2**e; |x| * 10**n = mant * 10**n * 2**e
    let p = 10u128.pow(n);
    let m = mant as u128 * p;
    let v: i128 = if e >= 0 {
        (m << e) as i128
    } else {
        let s = (-e) as u32;
        if s >= 127 {
            0
        } else {
            let q = m >> s;
            let rem = m - (q << s);
            let half = 1u128 << (s - 1);
            let q = if rem > half || (rem == half && q & 1 == 1) { q + 1 } else { q };
            q as i128
        }
    };
    (if neg { -v } else { v }) as i64
}

/// Python's `_key`: a position rounded to three decimals, as exact integers (thousandths).
#[inline]
pub fn key(xy: (f64, f64)) -> (i64, i64) {
    (round_scaled(xy.0, 3), round_scaled(xy.1, 3))
}

/// x * y and its rounding error, exactly: Dekker's split (CPython's own fallback when fma() can't be trusted,
/// and the same values as fma gives), as fma has no hardware instruction in a generic x86-64 build.
#[inline]
fn two_prod(x: f64, y: f64) -> (f64, f64) {
    #[inline]
    fn split(x: f64) -> (f64, f64) {
        let t = x * 134217729.0; // 2**27 + 1
        let hi = t - (t - x);
        (hi, x - hi)
    }
    let (xh, xl) = split(x);
    let (yh, yl) = split(y);
    let p = xh * yh;
    let q = xh * yl + xl * yh;
    let z = p + q;
    let zz = p - z + q + xl * yl;
    (z, zz)
}

#[inline]
fn fast_sum(a: f64, b: f64) -> (f64, f64) {
    let x = a + b;
    (x, (a - x) + b)
}

/// CPython's vector_norm for two coordinates (both already absolute values).
fn norm2(a: f64, b: f64) -> f64 {
    // along a row or a column (most distances on a breadboard): CPython's answer is the length itself
    if b == 0.0 && a.is_finite() {
        return a;
    }
    if a == 0.0 && b.is_finite() {
        return b;
    }
    let max = if b > a { b } else { a };
    if max.is_infinite() {
        return max;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if max == 0.0 {
        return max;
    }
    let (_, max_e) = frexp(max);
    assert!(max_e >= -1023, "subnormal distance");
    let scale = ldexp1(-max_e);
    let (mut csum, mut frac1, mut frac2) = (1.0f64, 0.0f64, 0.0f64);
    for v in [a, b] {
        let x = v * scale;
        let pr = two_prod(x, x);
        let sm = fast_sum(csum, pr.0);
        csum = sm.0;
        frac1 += pr.1;
        frac2 += sm.1;
    }
    let mut h = (csum - 1.0 + (frac1 + frac2)).sqrt();
    let pr = two_prod(-h, h);
    let sm = fast_sum(csum, pr.0);
    csum = sm.0;
    frac1 += pr.1;
    frac2 += sm.1;
    let x = csum - 1.0 + (frac1 + frac2);
    h += x / (2.0 * h);
    h / scale
}

fn frexp(x: f64) -> (f64, i32) {
    // x normal and positive here
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    if exp == 0 {
        let (m, e) = frexp(x * 2f64.powi(54));
        return (m, e - 54);
    }
    let e = exp - 1022;
    let m = f64::from_bits((bits & !(0x7ffu64 << 52)) | (1022u64 << 52));
    (m, e)
}

/// 2**e, built from its bits (e within the normal range).
fn ldexp1(e: i32) -> f64 {
    f64::from_bits(((e + 1023) as u64) << 52)
}

/// `math.dist(p, q)` for two points.
#[inline]
pub fn dist(p: (f64, f64), q: (f64, f64)) -> f64 {
    norm2((p.0 - q.0).abs(), (p.1 - q.1).abs())
}

/// `math.hypot(dx, dy)`.
#[inline]
pub fn hypot(dx: f64, dy: f64) -> f64 {
    norm2(dx.abs(), dy.abs())
}

/// `sum()` over floats as Python 3.12+ does it: the first term exactly, then Neumaier's compensation.
#[derive(Clone, Copy, Default)]
pub struct Sum {
    hi: f64,
    lo: f64,
    started: bool,
}

impl Sum {
    pub fn new() -> Self {
        Sum::default()
    }

    #[inline]
    pub fn add(&mut self, x: f64) {
        if !self.started {
            // sum() starts from the int 0: 0 + x is x (and -0.0 becomes 0.0)
            self.hi = 0.0 + x;
            self.started = true;
            return;
        }
        let t = self.hi + x;
        if self.hi.abs() >= x.abs() {
            self.lo += (self.hi - t) + x;
        } else {
            self.lo += (x - t) + self.hi;
        }
        self.hi = t;
    }

    pub fn value(&self) -> f64 {
        if self.lo != 0.0 && self.lo.is_finite() {
            self.hi + self.lo
        } else {
            self.hi
        }
    }
}

/// `sum(iterable)` of floats.
pub fn fsum<I: IntoIterator<Item = f64>>(it: I) -> f64 {
    let mut s = Sum::new();
    for x in it {
        s.add(x);
    }
    s.value()
}

/// Python's `repr` of a float (shortest round-trip digits, Python's choice of notation).
pub fn repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    // shortest digits and decimal exponent, from Rust's own shortest formatting in scientific notation
    let s = format!("{:e}", x);
    let (mant, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let n = digits.len() as i32;
    let decpt = exp + 1; // position of the decimal point relative to the digits
    let body = if -4 < decpt && decpt <= 16 {
        if decpt <= 0 {
            format!("0.{}{}", "0".repeat((-decpt) as usize), digits)
        } else if decpt >= n {
            format!("{}{}.0", digits, "0".repeat((decpt - n) as usize))
        } else {
            format!("{}.{}", &digits[..decpt as usize], &digits[decpt as usize..])
        }
    } else {
        let m = if n == 1 { digits.to_string() } else { format!("{}.{}", &digits[..1], &digits[1..]) };
        let e = decpt - 1;
        format!("{}e{}{:02}", m, if e < 0 { "-" } else { "+" }, e.abs())
    };
    if neg { format!("-{}", body) } else { body }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding() {
        assert_eq!(round(2.675, 2), 2.67); // 2.675 is a little under in binary
        assert_eq!(round(0.0625, 3), 0.062); // a true tie: to even
        assert_eq!(round(0.0635, 3), 0.064);
        assert_eq!(round(-1.0005, 3), -1.0); // -1.000499999...
        assert_eq!(round_scaled(12.5, 0), 12);
        assert_eq!(round_scaled(13.5, 0), 14);
        assert_eq!(key((3.0001, -7.9999)), (3000, -8000));
    }

    #[test]
    fn distances() {
        assert_eq!(dist((0.0, 0.0), (3.0, 4.0)), 5.0);
        assert_eq!(hypot(1.0, 1.0), std::f64::consts::SQRT_2);
    }

    #[test]
    fn sums() {
        assert_eq!(fsum([0.1, 0.2, 0.3]), 0.6); // plain addition gives 0.6000000000000001
        assert_eq!(fsum([1e100, 1.0, -1e100]), 1.0);
    }

    #[test]
    fn reprs() {
        assert_eq!(repr(0.1), "0.1");
        assert_eq!(repr(1e16), "1e+16");
        assert_eq!(repr(123456789012345680.0), "1.2345678901234568e+17");
        assert_eq!(repr(1e-5), "1e-05");
        assert_eq!(repr(0.0001), "0.0001");
        assert_eq!(repr(13.0), "13.0");
        assert_eq!(repr(2.5), "2.5");
    }
}
