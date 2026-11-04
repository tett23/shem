use super::*;

fn p(s: &str) -> Fixed {
    Fixed::parse(s.as_bytes()).unwrap()
}

fn raw(v: i64) -> Fixed {
    Fixed::from_raw(v)
}

const ONE: i64 = 1 << 32;

#[test]
fn parses_integers_and_simple_fractions() {
    assert_eq!(p("0"), raw(0));
    assert_eq!(p("1"), raw(ONE));
    assert_eq!(p("-1"), raw(-ONE));
    assert_eq!(p("+2"), raw(2 * ONE));
    assert_eq!(p("0.5"), raw(ONE / 2));
    assert_eq!(p(".5"), raw(ONE / 2));
    assert_eq!(p("-.5"), raw(-ONE / 2));
    assert_eq!(p("3."), raw(3 * ONE));
    assert_eq!(p("-0"), raw(0));
    assert_eq!(p("0.25"), raw(ONE / 4));
}

#[test]
fn rounds_to_nearest() {
    // 362.8346 * 2^32 = 1558362740857.24...
    assert_eq!(p("362.8346").raw(), 1558362740857);
    // 0.00001 * 2^32 = 42949.67296
    assert_eq!(p("0.00001").raw(), 42950);
    // 0.1 * 2^32 = 429496729.6
    assert_eq!(p("0.1").raw(), 429496730);
    assert_eq!(p("-0.1").raw(), -429496730);
}

#[test]
fn rounds_ties_to_even() {
    // 2^-33 is a tie between 0 and 2^-32: rounds to 0 (even)
    assert_eq!(p("0.000000000116415321826934814453125").raw(), 0);
    // 3 * 2^-33 is a tie between 1 and 2 units: rounds to 2 (even)
    assert_eq!(p("0.000000000349245965480804443359375").raw(), 2);
    assert_eq!(p("-0.000000000349245965480804443359375").raw(), -2);
    // just above the tie rounds up
    assert_eq!(p("0.0000000001164153218269348144531251").raw(), 1);
    // just below the tie rounds down
    assert_eq!(p("0.0000000001164153218269348144531249").raw(), 0);
}

#[test]
fn many_fraction_digits_are_exact() {
    let s = format!("0.5{}", "0".repeat(200));
    assert_eq!(p(&s), raw(ONE / 2));
    let s = format!("0.000000000116415321826934814453125{}1", "0".repeat(100));
    assert_eq!(p(&s).raw(), 1);
}

#[test]
fn rejects_malformed_and_out_of_range_numbers() {
    for s in ["", "-", "+", ".", "-.", "1.2.3", "1e5", "abc", "--1", " 1"] {
        assert!(Fixed::parse(s.as_bytes()).is_err(), "{s:?}");
    }
    assert!(Fixed::parse(b"2147483648").is_err());
    assert!(Fixed::parse(b"-2147483648").is_err());
    assert!(Fixed::parse(b"2147483647.9").is_ok());
    assert!(Fixed::parse(b"99999999999999999999999999").is_err());
}

#[test]
fn from_int_checks_the_range() {
    assert_eq!(Fixed::from_int(5).unwrap(), raw(5 * ONE));
    assert!(Fixed::from_int(1 << 31).is_err());
    assert!(Fixed::from_int(-(1 << 31)).is_err());
}

#[test]
fn arithmetic() {
    assert_eq!(p("1.5").checked_mul(p("2")).unwrap(), p("3"));
    assert_eq!(p("-1.5").checked_mul(p("2")).unwrap(), p("-3"));
    assert_eq!(p("1").checked_div(p("4")).unwrap(), p("0.25"));
    assert!(p("1").checked_div(p("0")).is_err());
    assert_eq!(p("1").checked_add(p("2")).unwrap(), p("3"));
    assert_eq!(p("1").checked_sub(p("2")).unwrap(), p("-1"));
    assert!(p("2147483647").checked_add(p("1")).is_err());
    assert!(p("65536").checked_mul(p("65536")).is_err());
    // the smallest unit squared rounds to 0 (a tie at 2^-64? no: below half)
    assert_eq!(raw(1).checked_mul(raw(1)).unwrap(), raw(0));
    // half a unit rounds to even
    assert_eq!(raw(1).checked_mul(p("0.5")).unwrap(), raw(0));
    assert_eq!(raw(3).checked_mul(p("0.5")).unwrap(), raw(2));
}

#[test]
fn sqrt_floors() {
    assert_eq!(p("4").sqrt().unwrap(), p("2"));
    assert_eq!(p("0.25").sqrt().unwrap(), p("0.5"));
    assert_eq!(p("0").sqrt().unwrap(), p("0"));
    // sqrt(2) * 2^32 = 6074000999.95...
    assert_eq!(p("2").sqrt().unwrap().raw(), 6074000999);
    assert!(p("-1").sqrt().is_err());
}

#[test]
fn rounding_to_integers() {
    assert_eq!(p("2.5").round(), 2);
    assert_eq!(p("3.5").round(), 4);
    assert_eq!(p("-2.5").round(), -2);
    assert_eq!(p("2.4").round(), 2);
    assert_eq!(p("2.1").ceil(), 3);
    assert_eq!(p("-2.1").ceil(), -2);
    assert_eq!(p("2").ceil(), 2);
    assert_eq!(p("2.9").floor(), 2);
    assert_eq!(p("-2.1").floor(), -3);
    // to 1/256: 0.5 is 128 units
    assert_eq!(p("0.5").to_units(8), 128);
    assert_eq!(raw(1 << 23).to_units(8), 0); // half a unit, rounds to even
    assert_eq!(raw(3 << 23).to_units(8), 2);
}

#[test]
fn round_shift_and_div_round_are_half_even() {
    assert_eq!(round_shift(5, 1), 2);
    assert_eq!(round_shift(7, 1), 4);
    assert_eq!(round_shift(-5, 1), -2);
    assert_eq!(round_shift(-7, 1), -4);
    assert_eq!(round_shift(6, 2), 2);
    assert_eq!(div_round(7, 2), 4);
    assert_eq!(div_round(5, 2), 2);
    assert_eq!(div_round(-5, 2), -2);
    assert_eq!(div_round(5, -2), -2);
    assert_eq!(div_round(10, 3), 3);
    assert_eq!(div_round(11, 3), 4);
}

#[test]
fn isqrt_is_the_floor() {
    assert_eq!(isqrt(0), 0);
    assert_eq!(isqrt(1), 1);
    assert_eq!(isqrt(15), 3);
    assert_eq!(isqrt(16), 4);
    assert_eq!(isqrt(u128::MAX), u64::MAX as u128);
}

fn m(v: [&str; 6]) -> Matrix {
    Matrix::new(v.map(p))
}

#[test]
fn identity_and_translation() {
    let t = m(["1", "0", "0", "1", "10", "20"]);
    assert_eq!(Matrix::IDENTITY.concat(&t).unwrap(), t);
    assert_eq!(t.concat(&Matrix::IDENTITY).unwrap(), t);
    assert_eq!(t.apply(p("1"), p("2")).unwrap(), (p("11"), p("22")));
}

#[test]
fn rotation_by_90_degrees() {
    // 0 -1 1 0 x y: (1, 0) goes to (0, -1)
    let r = m(["0", "-1", "1", "0", "0", "0"]);
    assert_eq!(r.apply(p("1"), p("0")).unwrap(), (p("0"), p("-1")));
    let rr = r.concat(&r).unwrap();
    assert_eq!(rr, m(["-1", "0", "0", "-1", "0", "0"]));
}

#[test]
fn concat_applies_the_first_matrix_first() {
    // scale by 2, then translate by (10, 0)
    let s = m(["2", "0", "0", "2", "0", "0"]);
    let t = m(["1", "0", "0", "1", "10", "0"]);
    assert_eq!(
        s.concat(&t).unwrap().apply(p("1"), p("1")).unwrap(),
        (p("12"), p("2"))
    );
    assert_eq!(
        t.concat(&s).unwrap().apply(p("1"), p("1")).unwrap(),
        (p("22"), p("2"))
    );
}

#[test]
fn golem_form_matrix_maps_back_within_the_rounding_error() {
    // The Form's Matrix [1/w 0 0 1/h 0 0] and the placement [w 0 0 h x y]
    for (w, h) in [
        ("240", "120"),
        ("200", "100"),
        ("362.8346", "515.9055"),
        ("7", "3"),
    ] {
        let inv = Matrix::new([
            p("1").checked_div(p(w)).unwrap(),
            Fixed::ZERO,
            Fixed::ZERO,
            p("1").checked_div(p(h)).unwrap(),
            Fixed::ZERO,
            Fixed::ZERO,
        ]);
        let place = m([w, "0", "0", h, "10", "20"]);
        let both = inv.concat(&place).unwrap();
        for (corner, want) in [(("0", "0"), (p("0"), p("0"))), ((w, h), (p(w), p(h)))] {
            let (x, y) = both.apply(p(corner.0), p(corner.1)).unwrap();
            let (wx, wy) = (
                want.0.checked_add(p("10")).unwrap(),
                want.1.checked_add(p("20")).unwrap(),
            );
            // the error of 1/w grows as w^2 / 2 units: within 2^-15 of a point,
            // far below a pixel at any resolution
            assert!((x.raw() - wx.raw()).abs() < 1 << 17, "{w}: {x:?} {wx:?}");
            assert!((y.raw() - wy.raw()).abs() < 1 << 17, "{h}: {y:?} {wy:?}");
        }
    }
}

#[test]
fn matrix_overflow_is_an_error() {
    let big = m(["65536", "0", "0", "65536", "0", "0"]);
    assert!(big.concat(&big).is_err());
}

#[test]
fn determinant() {
    assert_eq!(m(["2", "0", "0", "3", "5", "5"]).det().unwrap(), p("6"));
    assert_eq!(m(["0", "-1", "1", "0", "0", "0"]).det().unwrap(), p("1"));
}

#[test]
fn display_in_decimal() {
    assert_eq!(p("90").to_string(), "90");
    assert_eq!(p("-2.5").to_string(), "-2.5");
    assert_eq!(p("0.0041666667").to_string(), "0.004166");
    assert_eq!(p("-0").to_string(), "0");
}

/// A xorshift64* generator, for inputs that are the same on every run.
fn next(state: &mut u64) -> u64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

#[test]
fn short_fractions_equal_the_doubling_of_the_digits() {
    // docs/adr/0014: up to 19 digits, the fraction is computed with one
    // division, which must give the units of the exact doubling.
    let mut state = 0x0014_0001u64;
    (0..20000).for_each(|k| {
        let len = 1 + (next(&mut state) % 19) as usize;
        let digits: Vec<u8> = (0..len)
            .map(|i| {
                // Some fractions are ties, halves and runs of 9 and 0.
                match k % 5 {
                    0 => b'5',
                    1 => b"09"[(i + k) % 2],
                    2 if i + 1 < len => b'0',
                    _ => b'0' + (next(&mut state) % 10) as u8,
                }
            })
            .collect();
        assert_eq!(
            fraction_units(&digits),
            fraction_units_by_doubling(&digits),
            "0.{}",
            String::from_utf8_lossy(&digits)
        );
    });
    // Exact ties of 2^-33 and its odd multiples have 33 digits, past the
    // short path; the doubling still reads them.
    [
        "5",
        "25",
        "0000000002328306436538696289062",
        "9999999999999999999",
    ]
    .iter()
    .for_each(|d| {
        assert_eq!(
            fraction_units(d.as_bytes()),
            fraction_units_by_doubling(d.as_bytes())
        )
    });
}

#[test]
fn products_by_the_identity_are_the_other_matrix() {
    // docs/adr/0015: concat skips the identity, which the products give.
    let mut state = 0x0015_0009u64;
    (0..5000).for_each(|k| {
        let m = Matrix::new(std::array::from_fn(|_| {
            Fixed::from_raw((next(&mut state) as i64) >> (k % 40))
        }));
        let product = |a: &Matrix, b: &Matrix| -> Result<Matrix> {
            Ok(Matrix {
                a: narrow(wide(a.a, b.a) + wide(a.b, b.c))?,
                b: narrow(wide(a.a, b.b) + wide(a.b, b.d))?,
                c: narrow(wide(a.c, b.a) + wide(a.d, b.c))?,
                d: narrow(wide(a.c, b.b) + wide(a.d, b.d))?,
                e: narrow(wide(a.e, b.a) + wide(a.f, b.c) + widen(b.e))?,
                f: narrow(wide(a.e, b.b) + wide(a.f, b.d) + widen(b.f))?,
            })
        };
        assert_eq!(m.concat(&Matrix::IDENTITY), product(&m, &Matrix::IDENTITY));
        assert_eq!(Matrix::IDENTITY.concat(&m), product(&Matrix::IDENTITY, &m));
        assert_eq!(Ok(m), product(&m, &Matrix::IDENTITY));
    });
}
