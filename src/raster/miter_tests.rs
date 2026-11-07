//! The miter points of the joins, from the directions of the segments
//! (docs/adr/0023).

use super::*;

/// A xorshift64* generator, for inputs that are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// An integer in `lo..=hi`.
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }

    fn direction(&mut self, r: i64) -> (i64, i64) {
        (self.range(-r, r), self.range(-r, r))
    }

    /// Two directions: short ones, nearly the same ones, or long ones.
    fn directions(&mut self) -> ((i64, i64), (i64, i64)) {
        match self.range(0, 2) {
            0 => (self.direction(300), self.direction(300)),
            1 => {
                let d = self.direction(300);
                (d, (d.0 + self.range(-3, 3), d.1 + self.range(-3, 3)))
            }
            _ => {
                let r = (1 << 31) - 1;
                (self.direction(r), self.direction(r))
            }
        }
    }

    fn half(&mut self) -> i64 {
        match self.range(0, 3) {
            0 => 1,
            1 => 128,
            2 => 1 << 20,
            _ => self.range(1, 1 << 20),
        }
    }
}

fn within_16_half(m: (i64, i64), half: i64) -> bool {
    let (x, y) = (i128::from(m.0), i128::from(m.1));
    x * x + y * y <= (16 * i128::from(half)).pow(2)
}

#[test]
fn miter_points_are_within_16_half_widths_of_the_vertex() {
    let mut rng = Rng(0x2302_3a02);
    let mut miters = 0;
    (0..200_000).for_each(|_| {
        let (d1, d2) = rng.directions();
        let half = rng.half();
        if d1 == (0, 0) || d2 == (0, 0) {
            return;
        }
        if let Some(m) = miter(d1, d2, half) {
            miters += 1;
            assert!(within_16_half(m, half), "{d1:?} {d2:?} {half}: {m:?}");
        }
    });
    assert!(miters > 50_000, "{miters}");
}

#[test]
fn the_join_that_jumped_stays_near_its_vertex() {
    // The join of docs/adr/0023: 5,461 units from its vertex before.
    let m = miter((60, 62), (59, 61), 128).unwrap();
    assert!(within_16_half(m, 128), "{m:?}");
}

/// The miter point of directions under 2^16, to `half · 2^−28`:
/// `half (N1 + N2) / (1 + cos θ)` with the lengths to 2^−32.
fn precise_miter(d1: (i64, i64), d2: (i64, i64), half: i64) -> (i64, i64) {
    let cross = i128::from(d1.0) * i128::from(d2.1) - i128::from(d1.1) * i128::from(d2.0);
    let sign: i128 = if cross > 0 { -1 } else { 1 };
    let length = |d: (i64, i64)| {
        let (x, y) = (i128::from(d.0), i128::from(d.1));
        isqrt(((x * x + y * y) as u128) << 64) as i128
    };
    let (l1, l2) = (length(d1), length(d2));
    let perp = |d: (i64, i64)| (-sign * i128::from(d.1), sign * i128::from(d.0));
    let (p1, p2) = (perp(d1), perp(d2));
    let dot = i128::from(d1.0) * i128::from(d2.0) + i128::from(d1.1) * i128::from(d2.1);
    let den = l1 * l2 + (dot << 64);
    let ratio = |a: i128, b: i128| div_round((a * l2 + b * l1) << 60, den);
    let (rx, ry) = (ratio(p1.0, p2.0), ratio(p1.1, p2.1));
    (
        div_round(i128::from(half) * rx, 1 << 28) as i64,
        div_round(i128::from(half) * ry, 1 << 28) as i64,
    )
}

#[test]
fn miter_points_are_those_of_the_directions() {
    let mut rng = Rng(0x0023_0001);
    let mut miters = 0;
    (0..100_000).for_each(|_| {
        let r = if rng.range(0, 1) == 0 {
            300
        } else {
            (1 << 16) - 1
        };
        let d1 = rng.direction(r);
        let d2 = if rng.range(0, 1) == 0 {
            (d1.0 + rng.range(-3, 3), d1.1 + rng.range(-3, 3))
        } else {
            rng.direction(r)
        };
        let half = rng.half();
        if d1 == (0, 0) || d2 == (0, 0) {
            return;
        }
        if let Some(m) = miter(d1, d2, half) {
            miters += 1;
            let p = precise_miter(d1, d2, half);
            assert!(
                (m.0 - p.0).abs() <= 2 && (m.1 - p.1).abs() <= 2,
                "{d1:?} {d2:?} {half}: {m:?}, not {p:?}"
            );
        }
    });
    assert!(miters > 30_000, "{miters}");
}

#[test]
fn a_right_angle_has_its_miter_at_the_corner() {
    let m = miter((256, 0), (0, 256), 128).unwrap();
    assert_eq!((m.0.abs(), m.1.abs()), (128, 128));
}

/// Whether the join is a miter, as before docs/adr/0023: within the
/// limit, and with rounded normals that are not parallel.
fn mitered_before(d1: (i64, i64), d2: (i64, i64), half: i64) -> bool {
    let cross = i128::from(d1.0) * i128::from(d2.1) - i128::from(d1.1) * i128::from(d2.0);
    if cross == 0 {
        return false;
    }
    let sign = if cross > 0 { -1 } else { 1 };
    let (n1, n2) = (normal(d1, half * sign), normal(d2, half * sign));
    let dot = i128::from(d1.0) * i128::from(d2.0) + i128::from(d1.1) * i128::from(d2.1);
    let len2 = |d: (i64, i64)| {
        (i128::from(d.0) * i128::from(d.0) + i128::from(d.1) * i128::from(d.1)) as u128
    };
    let lens = isqrt(len2(d1).saturating_mul(len2(d2))) as i128;
    let det = i128::from(n1.0) * i128::from(n2.1) - i128::from(n1.1) * i128::from(n2.0);
    100 * dot + 98 * lens >= 0 && det != 0
}

#[test]
fn miters_and_bevels_are_chosen_as_before() {
    let mut rng = Rng(0x0023_0002);
    (0..200_000).for_each(|_| {
        let (d1, d2) = rng.directions();
        let half = rng.half();
        if d1 == (0, 0) || d2 == (0, 0) {
            return;
        }
        assert_eq!(
            miter(d1, d2, half).is_some(),
            mitered_before(d1, d2, half),
            "{d1:?} {d2:?} {half}"
        );
    });
}

#[test]
fn the_stroke_of_a_curve_above_the_canvas_draws_nothing() {
    // The curve of docs/adr/0023, above a canvas of 40 × 30 pixels with a
    // line of 1 point at 72 dpi: before, a join of its flattening jumped
    // into the canvas.
    let mut path = Path::new();
    path.move_to((34, -2402));
    path.curve_to((98, -2321), (17, -2406), (289, -2125));
    let outline = stroke(&path, 128);
    assert!(coverage(&outline, FillRule::NonZero, 40, 30).is_empty());
}
