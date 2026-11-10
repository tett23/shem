//! The arithmetic of the inverse mapping and of the interpolation
//! (docs/adr/0027).

use super::*;
use crate::fixed::Fixed;

/// `floor(n × m / d)` by i128 when it fits, for the comparison.
fn slow(n: i128, m: i128, d: i128) -> (i128, i128) {
    let product = n * m;
    (product.div_euclid(d), product.rem_euclid(d))
}

#[test]
fn mul_div_is_the_exact_floor_and_remainder() {
    let mut state = 0x0027_0001u64;
    let mut next = || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_f491_4f6c_dd1d)
    };
    (0..20000).for_each(|_| {
        // n and d within 2^80, so that n × m fits in i128 for the
        // comparison; m below 2^43.
        let n = (next() as i128) << (next() % 17) >> 47;
        let d = ((next() as i128).abs() >> (48 + next() % 40)).max(1);
        let m = (next() as i128) >> 21;
        let m = m.abs() & ((1 << 43) - 1);
        let (q, r) = slow(n, m, d);
        assert_eq!(mul_div(n, m, d), Value::Exact { q, r }, "{n} {m} {d}");
    });
    // Small cases by hand.
    assert_eq!(mul_div(7, 3, 2), Value::Exact { q: 10, r: 1 });
    assert_eq!(mul_div(-7, 3, 2), Value::Exact { q: -11, r: 1 });
    assert_eq!(mul_div(0, 5, 9), Value::Exact { q: 0, r: 0 });
    assert_eq!(mul_div(1, 0, 9), Value::Exact { q: 0, r: 0 });
}

#[test]
fn mul_div_of_a_huge_quotient_is_far_with_its_sign() {
    let n = i128::MAX / 2;
    assert_eq!(mul_div(n, 1 << 42, 3), Value::Far(false));
    assert_eq!(mul_div(-n, 1 << 42, 3), Value::Far(true));
    // Within the bound, it is exact even when n × m overflows i128.
    let n = 1i128 << 100;
    let d = 1i128 << 90;
    assert_eq!(
        mul_div(n + 1, 1 << 40, d),
        Value::Exact {
            q: 1 << 50,
            r: 1 << 40
        }
    );
}

#[test]
fn the_steps_of_an_axis_agree_with_the_direct_values() {
    // An axis with a small denominator, whose steps are exact, and one
    // with a tiny denominator, whose steps are far.
    let axis = Axis::new(2 << 16, 300 << 32, 7 << 31, 1 << 32, 0);
    let mut v = axis.at(0, 0);
    (1..600).for_each(|x| {
        v = axis.next(v, x, 0);
        assert_eq!(v, axis.at(x, 0), "{x}");
    });
    let tiny = Axis::new(1 << 42, 1, 0, 1 << 100, 0);
    assert_eq!(tiny.step, None);
    assert_eq!(tiny.at(0, 0), Value::Exact { q: 0, r: 0 });
    assert_eq!(tiny.at(1, 0), Value::Far(false));
    assert_eq!(tiny.next(tiny.at(0, 0), 1, 0), Value::Far(false));
}

#[test]
fn bilinear_rounds_to_the_nearest_ties_to_even() {
    assert_eq!(bilinear(0, 0, 0, 0, 0, 0), 0);
    assert_eq!(bilinear(255, 255, 255, 255, 12345, 54321), 255);
    assert_eq!(bilinear(10, 20, 30, 40, 0, 0), 10);
    assert_eq!(bilinear(10, 20, 30, 40, 1 << 15, 0), 15);
    // Halfway between 10 and 11 at fx = 1/2: ties to even, 10.
    assert_eq!(bilinear(10, 11, 10, 11, 1 << 15, 0), 10);
    assert_eq!(bilinear(11, 12, 11, 12, 1 << 15, 0), 12);
    // The four corners each weigh a quarter at the center.
    assert_eq!(bilinear(0, 0, 0, 200, 1 << 15, 1 << 15), 50);
}

#[test]
fn split_takes_half_a_sample_off_and_floors() {
    assert_eq!(split(1 << 15), (0, 0));
    assert_eq!(split(0), (-1, 1 << 15));
    assert_eq!(split(3 << 15), (1, 0));
    assert_eq!(split((3 << 15) + 1), (1, 1));
    assert_eq!(split(-1), (-1, (1 << 15) - 1));
    assert_eq!(split(-(1 << 15) - 1), (-2, (1 << 16) - 1));
}

#[test]
fn a_mapping_sends_the_centers_of_pixels_into_the_image() {
    // The unit square on 4 × 2 pixels: the centers are at u = 1/8, 3/8,
    // … of the width, in samples of a 2 × 2 image u = 1/4, 3/4, 5/4, 7/4.
    let n = |v: i64| Fixed::from_int(v).unwrap();
    let ctm = Matrix::new([n(4), n(0), n(0), n(-2), n(0), n(2)]);
    let m = Mapping::new(&ctm, 2, 2, None).unwrap().unwrap();
    (0..4).for_each(|x| {
        let u = m.u.at(x, 0).units();
        assert_eq!(u, ((2 * x as i128 + 1) << 16) / 4, "{x}");
        let v = m.v.at(x, 1).units();
        assert_eq!(v, 3 << 15);
    });
    // A singular CTM has no mapping.
    let singular = Matrix::new([n(4), n(0), n(2), n(0), n(0), n(0)]);
    assert!(Mapping::new(&singular, 2, 2, None).unwrap().is_none());
    // A 90-degree turn of the unit square onto x 0..2, y 0..4: the center
    // of the pixel (0, 0), at (1/2, 1/2), is (s, t) = (7/8, 3/4), so
    // u = 2 s = 7/4 and v = 2 (1 − t) = 1/2.
    let turned = Matrix::new([n(0), n(-4), n(-2), n(0), n(2), n(4)]);
    let m = Mapping::new(&turned, 2, 2, None).unwrap().unwrap();
    assert_eq!(m.u.at(0, 0).units(), 7 << 14);
    assert_eq!(m.v.at(0, 0).units(), 1 << 15);
}

#[test]
fn eight_bits_is_div_round() {
    assert_eq!(eight_bits(1, 1), 255);
    assert_eq!(eight_bits(1, 2), 85);
    assert_eq!(eight_bits(2, 2), 170);
    assert_eq!(eight_bits(5, 4), 85);
    assert_eq!(eight_bits(8, 4), 136);
    assert_eq!(eight_bits(0x8000, 16), 128);
    assert_eq!(eight_bits(0x8080, 16), 128);
    assert_eq!(eight_bits(0x8081, 16), 128);
    assert_eq!(eight_bits(0x8100, 16), 128);
    assert_eq!(eight_bits(0x8101, 16), 129);
    assert_eq!(eight_bits(200, 8), 200);
}

#[test]
fn packed_reads_components_from_the_top_bit() {
    assert_eq!(packed(&[0b1010_0000], 0, 1), 1);
    assert_eq!(packed(&[0b1010_0000], 1, 1), 0);
    assert_eq!(packed(&[0b1010_0000], 2, 1), 1);
    assert_eq!(packed(&[0b0110_1100], 0, 2), 1);
    assert_eq!(packed(&[0b0110_1100], 1, 2), 2);
    assert_eq!(packed(&[0b0110_1100], 2, 2), 3);
    assert_eq!(packed(&[0b0110_1100], 3, 2), 0);
    assert_eq!(packed(&[0x5A, 0xF1], 1, 4), 10);
    assert_eq!(packed(&[0x5A, 0xF1], 3, 4), 1);
    assert_eq!(packed(&[0x12, 0x34], 0, 16), 0x1234);
}

#[test]
fn the_mapping_refuses_entries_beyond_2_57() {
    // docs/adr/0029. The renderer maps the corners of the unit square to
    // the device first, which fails for such a CTM with its own error, so
    // this bound is defensive; it is tested on `Mapping::new` itself.
    let at = |raw: i64| {
        let mut m = Matrix::IDENTITY;
        m.a = Fixed::from_raw(raw);
        Mapping::new(&m, 2, 2, None)
    };
    assert!(at(1 << 57).unwrap().is_some());
    let message = "image outside the range of coordinates";
    assert_eq!(
        at((1 << 57) + 1).err().map(|e| e.to_string()).as_deref(),
        Some(message)
    );
    assert_eq!(
        at(-(1 << 57) - 1).err().map(|e| e.to_string()).as_deref(),
        Some(message)
    );
    for entry in 0..6 {
        let mut m = Matrix::IDENTITY;
        let big = Fixed::from_raw(1 << 58);
        match entry {
            0 => m.a = big,
            1 => m.b = big,
            2 => m.c = big,
            3 => m.d = big,
            4 => m.e = big,
            _ => m.f = big,
        }
        assert_eq!(
            Mapping::new(&m, 2, 2, None)
                .err()
                .map(|e| e.to_string())
                .as_deref(),
            Some(message),
            "entry {entry}"
        );
    }
}

fn picture_of(bytes: usize) -> Picture {
    // Zeroed memory that the test never touches, so it costs no pages.
    Picture {
        width: bytes / 3,
        height: 1,
        rgb: vec![0; bytes],
        mask: None,
    }
}

#[test]
fn the_cache_keeps_pictures_up_to_its_budget_and_decodes_the_rest_again() {
    // docs/adr/0029: 2^28 bytes. Two pictures of 2^27 fill it; a third is
    // returned but not kept, so it is loaded again each time it is asked
    // for. The pictures that are kept are loaded once.
    let cache = PictureCache::new();
    let loads = std::sync::atomic::AtomicUsize::new(0);
    let get = |num: u32, bytes: usize| {
        cache
            .get(num, || {
                loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(picture_of(bytes))
            })
            .unwrap()
    };
    let half = PictureCache::BUDGET / 2;
    assert_eq!(get(1, half).bytes(), half);
    assert_eq!(get(2, half).bytes(), half);
    assert!(cache.cached(1).is_some() && cache.cached(2).is_some());
    assert_eq!(loads.load(std::sync::atomic::Ordering::SeqCst), 2);
    // Kept: no load.
    get(1, half);
    get(2, half);
    assert_eq!(loads.load(std::sync::atomic::Ordering::SeqCst), 2);
    // Beyond the budget by one byte: given back, not kept, loaded again.
    assert_eq!(get(3, 3).bytes(), 3);
    assert!(cache.cached(3).is_none());
    get(3, 3);
    assert_eq!(loads.load(std::sync::atomic::Ordering::SeqCst), 4);
    // The budget is exact: a cache with one byte to spare keeps a picture
    // of that size.
    let cache = PictureCache::new();
    cache.get(1, || Ok(picture_of(half))).unwrap();
    cache
        .get(2, || Ok(picture_of(PictureCache::BUDGET - half)))
        .unwrap();
    assert!(cache.cached(2).is_some());
}
