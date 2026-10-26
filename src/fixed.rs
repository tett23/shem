//! Q32.32 fixed-point numbers and matrices (docs/adr/0001).
//!
//! A [`Fixed`] is a signed 64-bit integer with 32 fraction bits. Every
//! operation rounds once, to the nearest value with ties to even, and
//! reports an overflow as an error instead of wrapping.

use crate::error::{Error, Result};

/// A Q32.32 fixed-point number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Fixed(i64);

const FRAC_BITS: u32 = 32;
const INT_LIMIT: i64 = 1 << 31;

impl Fixed {
    pub const ZERO: Fixed = Fixed(0);
    pub const ONE: Fixed = Fixed(1 << FRAC_BITS);

    pub const fn from_raw(v: i64) -> Fixed {
        Fixed(v)
    }

    pub const fn raw(self) -> i64 {
        self.0
    }

    /// An integer whose absolute value is less than 2^31.
    pub fn from_int(v: i64) -> Result<Fixed> {
        if v > -INT_LIMIT && v < INT_LIMIT {
            Ok(Fixed(v << FRAC_BITS))
        } else {
            Err(Error::new(format!("number out of range: {v}")))
        }
    }

    /// A value held as `v` units of 2^-32 in an `i128`, if it fits.
    pub fn from_wide(v: i128) -> Result<Fixed> {
        i64::try_from(v).map(Fixed).map_err(|_| Error::overflow())
    }

    /// Parses a PDF number: an optional sign, digits, and an optional
    /// fraction (ISO 32000-1 7.3.3). The value is rounded exactly to the
    /// nearest Q32.32 value, ties to even.
    pub fn parse(s: &[u8]) -> Result<Fixed> {
        let bad = || Error::new(format!("malformed number: {}", String::from_utf8_lossy(s)));
        let (negative, body) = match s.first() {
            Some(b'-') => (true, &s[1..]),
            Some(b'+') => (false, &s[1..]),
            _ => (false, s),
        };
        let (int_digits, frac_digits) = match body.iter().position(|&c| c == b'.') {
            Some(i) => (&body[..i], &body[i + 1..]),
            None => (body, &body[body.len()..]),
        };
        let all_digits = |d: &[u8]| d.iter().all(u8::is_ascii_digit);
        if int_digits.len() + frac_digits.len() == 0
            || !all_digits(int_digits)
            || !all_digits(frac_digits)
        {
            return Err(bad());
        }
        let int = int_digits.iter().try_fold(0i64, |acc, &c| {
            let v = acc * 10 + i64::from(c - b'0');
            (v < INT_LIMIT).then_some(v)
        });
        let int = int.ok_or_else(|| {
            Error::new(format!(
                "number out of range: {}",
                String::from_utf8_lossy(s)
            ))
        })?;
        let magnitude = (i128::from(int) << FRAC_BITS) + fraction_units(frac_digits);
        let value = if negative { -magnitude } else { magnitude };
        if value <= -(i128::from(INT_LIMIT) << FRAC_BITS)
            || value >= i128::from(INT_LIMIT) << FRAC_BITS
        {
            return Err(Error::new(format!(
                "number out of range: {}",
                String::from_utf8_lossy(s)
            )));
        }
        Fixed::from_wide(value)
    }

    pub fn checked_add(self, o: Fixed) -> Result<Fixed> {
        self.0
            .checked_add(o.0)
            .map(Fixed)
            .ok_or_else(Error::overflow)
    }

    pub fn checked_sub(self, o: Fixed) -> Result<Fixed> {
        self.0
            .checked_sub(o.0)
            .map(Fixed)
            .ok_or_else(Error::overflow)
    }

    pub fn checked_neg(self) -> Result<Fixed> {
        self.0.checked_neg().map(Fixed).ok_or_else(Error::overflow)
    }

    pub fn abs(self) -> Result<Fixed> {
        self.0.checked_abs().map(Fixed).ok_or_else(Error::overflow)
    }

    pub fn checked_mul(self, o: Fixed) -> Result<Fixed> {
        Fixed::from_wide(round_shift(i128::from(self.0) * i128::from(o.0), FRAC_BITS))
    }

    pub fn checked_div(self, o: Fixed) -> Result<Fixed> {
        if o.0 == 0 {
            return Err(Error::new("division by zero"));
        }
        Fixed::from_wide(div_round(i128::from(self.0) << FRAC_BITS, i128::from(o.0)))
    }

    /// The square root, rounded down.
    pub fn sqrt(self) -> Result<Fixed> {
        let v =
            u128::try_from(self.0).map_err(|_| Error::new("square root of a negative number"))?;
        Fixed::from_wide(isqrt(v << FRAC_BITS) as i128)
    }

    /// The nearest integer, ties to even.
    pub fn round(self) -> i64 {
        self.to_units(0)
    }

    pub fn floor(self) -> i64 {
        self.0 >> FRAC_BITS
    }

    pub fn ceil(self) -> i64 {
        -((-i128::from(self.0)) >> FRAC_BITS) as i64
    }

    /// The value in units of 2^-bits, rounded to the nearest, ties to even.
    pub fn to_units(self, bits: u32) -> i64 {
        round_shift(i128::from(self.0), FRAC_BITS - bits) as i64
    }
}

/// The units of 2^-32 of a decimal fraction `0.d1d2...`, rounded to the
/// nearest, ties to even. The fraction is doubled digit by digit, so the
/// result is exact for any number of digits.
fn fraction_units(digits: &[u8]) -> i128 {
    let mut fraction: Vec<u8> = digits.iter().map(|c| c - b'0').collect();
    // Doubling the decimal fraction moves its first binary digit out as the carry.
    let mut next_bit = || {
        fraction.iter_mut().rev().fold(0u8, |carry, d| {
            let v = *d * 2 + carry;
            *d = v % 10;
            v / 10
        })
    };
    let units = (0..FRAC_BITS).fold(0i128, |acc, _| (acc << 1) | i128::from(next_bit()));
    let guard = next_bit() == 1;
    let sticky = fraction.iter().any(|&d| d != 0);
    if guard && (sticky || units & 1 == 1) {
        units + 1
    } else {
        units
    }
}

/// `v / 2^s`, rounded to the nearest, ties to even.
pub fn round_shift(v: i128, s: u32) -> i128 {
    if s == 0 {
        return v;
    }
    let q = v >> s;
    let r = v - (q << s);
    let half = 1i128 << (s - 1);
    if r > half || (r == half && q & 1 == 1) {
        q + 1
    } else {
        q
    }
}

/// `n / d`, rounded to the nearest, ties to even. `d` is not zero.
pub fn div_round(n: i128, d: i128) -> i128 {
    let (n, d) = if d < 0 { (-n, -d) } else { (n, d) };
    let q = n.div_euclid(d);
    let r = n.rem_euclid(d);
    match (2 * r).cmp(&d) {
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal if q & 1 == 1 => q + 1,
        _ => q,
    }
}

/// The integer square root, rounded down.
pub fn isqrt(v: u128) -> u128 {
    if v < 2 {
        return v;
    }
    // Newton's method from an upper bound; the sequence decreases to the floor.
    let bits = 128 - v.leading_zeros();
    let start = 1u128 << bits.div_ceil(2);
    std::iter::successors(Some(start), |&x| {
        let next = (x + v / x) / 2;
        (next < x).then_some(next)
    })
    .last()
    .unwrap_or(start)
}

/// An affine matrix `[a b c d e f]` of PDF (ISO 32000-1 8.3.4): a point
/// `(x, y)` goes to `(a x + c y + e, b x + d y + f)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    pub a: Fixed,
    pub b: Fixed,
    pub c: Fixed,
    pub d: Fixed,
    pub e: Fixed,
    pub f: Fixed,
}

/// The product of two Q32.32 values as an `i128` with 64 fraction bits.
fn wide(x: Fixed, y: Fixed) -> i128 {
    i128::from(x.0) * i128::from(y.0)
}

/// A Q32.32 value widened to 64 fraction bits.
fn widen(x: Fixed) -> i128 {
    i128::from(x.0) << FRAC_BITS
}

/// A sum with 64 fraction bits, rounded once to Q32.32.
fn narrow(v: i128) -> Result<Fixed> {
    Fixed::from_wide(round_shift(v, FRAC_BITS))
}

impl Matrix {
    pub const IDENTITY: Matrix = Matrix {
        a: Fixed::ONE,
        b: Fixed::ZERO,
        c: Fixed::ZERO,
        d: Fixed::ONE,
        e: Fixed::ZERO,
        f: Fixed::ZERO,
    };

    pub fn new(v: [Fixed; 6]) -> Matrix {
        Matrix {
            a: v[0],
            b: v[1],
            c: v[2],
            d: v[3],
            e: v[4],
            f: v[5],
        }
    }

    pub fn translation(x: Fixed, y: Fixed) -> Matrix {
        Matrix {
            e: x,
            f: y,
            ..Matrix::IDENTITY
        }
    }

    /// `self × o`: the transformation that applies `self` first, then `o`.
    /// Each element is a sum of products computed exactly and rounded once.
    pub fn concat(&self, o: &Matrix) -> Result<Matrix> {
        Ok(Matrix {
            a: narrow(wide(self.a, o.a) + wide(self.b, o.c))?,
            b: narrow(wide(self.a, o.b) + wide(self.b, o.d))?,
            c: narrow(wide(self.c, o.a) + wide(self.d, o.c))?,
            d: narrow(wide(self.c, o.b) + wide(self.d, o.d))?,
            e: narrow(wide(self.e, o.a) + wide(self.f, o.c) + widen(o.e))?,
            f: narrow(wide(self.e, o.b) + wide(self.f, o.d) + widen(o.f))?,
        })
    }

    /// The image of the point `(x, y)`, each coordinate rounded once.
    pub fn apply(&self, x: Fixed, y: Fixed) -> Result<(Fixed, Fixed)> {
        Ok((
            narrow(wide(self.a, x) + wide(self.c, y) + widen(self.e))?,
            narrow(wide(self.b, x) + wide(self.d, y) + widen(self.f))?,
        ))
    }

    /// The image of the vector `(x, y)`, without the translation.
    pub fn apply_vector(&self, x: Fixed, y: Fixed) -> Result<(Fixed, Fixed)> {
        Ok((
            narrow(wide(self.a, x) + wide(self.c, y))?,
            narrow(wide(self.b, x) + wide(self.d, y))?,
        ))
    }

    /// The determinant `a d − b c`, rounded once.
    pub fn det(&self) -> Result<Fixed> {
        narrow(wide(self.a, self.d) - wide(self.b, self.c))
    }
}

#[cfg(test)]
mod tests;

/// The value in decimal with up to six fraction digits, truncated toward
/// zero, for messages.
impl std::fmt::Display for Fixed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let negative = self.0 < 0;
        let magnitude = self.0.unsigned_abs();
        let int = magnitude >> FRAC_BITS;
        let frac = magnitude & ((1 << FRAC_BITS) - 1);
        let digits: String =
            std::iter::successors(Some(frac), |&r| Some((r * 10) & ((1 << FRAC_BITS) - 1)))
                .take(6)
                .map(|r| char::from(b'0' + ((r * 10) >> FRAC_BITS) as u8))
                .collect();
        let digits = digits.trim_end_matches('0');
        let sign = if negative && (int != 0 || !digits.is_empty()) {
            "-"
        } else {
            ""
        };
        if digits.is_empty() {
            write!(f, "{sign}{int}")
        } else {
            write!(f, "{sign}{int}.{digits}")
        }
    }
}
