//! The rasterizer of docs/adr/0013 equals the one before it, kept in
//! `reference`, on curves, paths and polylines from a fixed seed.

use super::reference as old;
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

    /// An integer in `lo..hi`.
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo) as u64) as i64
    }

    fn point(&mut self, lo: i64, hi: i64) -> Point {
        (self.range(lo, hi) as i32, self.range(lo, hi) as i32)
    }
}

#[derive(Clone, Copy, Debug)]
enum Cmd {
    Move(Point),
    Line(Point),
    Curve(Point, Point, Point),
    Close,
}

/// The commands of a random path: segments, curves, closings, and points
/// repeated, with coordinates in `lo..hi`.
fn random_cmds(rng: &mut Rng, curves: bool, lo: i64, hi: i64) -> Vec<Cmd> {
    let n = rng.range(1, 30);
    let mut last = rng.point(lo, hi);
    let mut cmds = vec![Cmd::Move(last)];
    (0..n).for_each(|_| {
        let p = if rng.range(0, 6) == 0 {
            last
        } else {
            rng.point(lo, hi)
        };
        let cmd = match rng.range(0, if curves { 10 } else { 8 }) {
            0 => Cmd::Move(p),
            1 => Cmd::Close,
            8 | 9 => Cmd::Curve(rng.point(lo, hi), rng.point(lo, hi), p),
            _ => Cmd::Line(p),
        };
        if !matches!(cmd, Cmd::Close) {
            last = p;
        }
        cmds.push(cmd);
    });
    cmds
}

fn build(cmds: &[Cmd]) -> Path {
    let mut path = Path::new();
    cmds.iter().for_each(|&c| match c {
        Cmd::Move(p) => path.move_to(p),
        Cmd::Line(p) => path.line_to(p),
        Cmd::Curve(a, b, p) => path.curve_to(a, b, p),
        Cmd::Close => path.close(),
    });
    path
}

fn build_old(cmds: &[Cmd]) -> old::Path {
    let mut path = old::Path::new();
    cmds.iter().for_each(|&c| match c {
        Cmd::Move(p) => path.move_to(p),
        Cmd::Line(p) => path.line_to(p),
        Cmd::Curve(a, b, p) => path.curve_to(a, b, p),
        Cmd::Close => path.close(),
    });
    path
}

type Owned = Vec<(Vec<Point>, bool)>;

fn owned(path: &Path) -> Owned {
    path.subpaths()
        .map(|s| (s.points.to_vec(), s.closed))
        .collect()
}

fn owned_old(path: &old::Path) -> Owned {
    path.subpaths()
        .iter()
        .map(|s| (s.points.clone(), s.closed))
        .collect()
}

fn old_rule(rule: FillRule) -> old::FillRule {
    match rule {
        FillRule::NonZero => old::FillRule::NonZero,
        FillRule::EvenOdd => old::FillRule::EvenOdd,
    }
}

#[test]
fn flattened_curves_equal_the_reference() {
    let mut rng = Rng(0x0013_0001);
    (0..1000).for_each(|i| {
        // A few curves are as large as the coordinates allow.
        let span = match i % 10 {
            0 => 1 << 30,
            1..=3 => 1 << 20,
            _ => 1 << 14,
        };
        let [p0, p1, p2, p3] = std::array::from_fn(|_| rng.point(-span, span));
        assert_eq!(
            flatten_curve(p0, p1, p2, p3),
            old::flatten_curve(p0, p1, p2, p3),
            "{p0:?} {p1:?} {p2:?} {p3:?}"
        );
    });
}

#[test]
fn the_largest_curve_is_split_to_the_deepest_level() {
    // With i32 coordinates, the second differences are below 2^34, and a
    // split divides them by 4: a curve is flat after at most 15 levels,
    // which gives 2^15 points. The stack of the splits holds them.
    let (lo, hi) = (i32::MIN, i32::MAX);
    let points = flatten_curve((lo, lo), (hi, lo), (lo, hi), (hi, hi));
    assert_eq!(
        points,
        old::flatten_curve((lo, lo), (hi, lo), (lo, hi), (hi, hi))
    );
    assert!(points.len() >= 1 << 14, "{}", points.len());
}

#[test]
fn paths_equal_the_reference() {
    let mut rng = Rng(0x0013_0002);
    (0..200).for_each(|_| {
        let cmds = random_cmds(&mut rng, true, -2560, 12800);
        assert_eq!(
            owned(&build(&cmds)),
            owned_old(&build_old(&cmds)),
            "{cmds:?}"
        );
    });
}

#[test]
fn a_cleared_path_is_like_a_new_one() {
    let mut rng = Rng(0x0013_0003);
    let mut path = Path::new();
    (0..50).for_each(|_| {
        let cmds = random_cmds(&mut rng, true, -2560, 12800);
        path.clear();
        cmds.iter().for_each(|&c| match c {
            Cmd::Move(p) => path.move_to(p),
            Cmd::Line(p) => path.line_to(p),
            Cmd::Curve(a, b, p) => path.curve_to(a, b, p),
            Cmd::Close => path.close(),
        });
        assert_eq!(path, build(&cmds));
    });
}

#[test]
fn coverages_equal_the_reference_with_one_scratch() {
    coverages_equal_the_reference(0x0013_0004, 40, 30, -2560, 12800, 200);
}

#[test]
fn coverages_of_small_and_large_paths_equal_the_reference() {
    // docs/adr/0014: small paths are summed in a dense grid, large ones by
    // sorting the cells; both, and paths far outside, equal the reference.
    coverages_equal_the_reference(0x0014_0003, 12, 9, -512, 3584, 300);
    coverages_equal_the_reference(0x0014_0004, 600, 500, -25600, 179200, 60);
    coverages_equal_the_reference(0x0014_0005, 40, 30, -40000, -20000, 50);
    coverages_equal_the_reference(0x0014_0006, 40, 30, 20000, 40000, 50);
}

fn coverages_equal_the_reference(seed: u64, w: usize, h: usize, lo: i64, hi: i64, n: usize) {
    let mut rng = Rng(seed);
    let mut scratch = Scratch::default();
    (0..n).for_each(|_| {
        // Some points are outside the canvas.
        let cmds = random_cmds(&mut rng, true, lo, hi);
        let (path, reference) = (build(&cmds), build_old(&cmds));
        [FillRule::NonZero, FillRule::EvenOdd]
            .iter()
            .for_each(|&rule| {
                let expected = old::coverage(&reference, old_rule(rule), w, h);
                assert_eq!(coverage(&path, rule, w, h), expected, "{cmds:?}");
                assert_eq!(
                    coverage_with(&path, rule, w, h, &mut scratch),
                    expected.as_slice(),
                    "{cmds:?}"
                );
            });
    });
}

#[test]
fn strokes_equal_the_reference() {
    let mut rng = Rng(0x0013_0005);
    let mut out = Path::new();
    (0..200).for_each(|i| {
        let cmds = random_cmds(&mut rng, false, -2560, 12800);
        let half = rng.range(1, 600);
        let expected = owned_old(&old::stroke(&build_old(&cmds), half));
        assert_eq!(owned(&stroke(&build(&cmds), half)), expected, "{cmds:?}");
        // stroke_into adds to the polygons already there.
        if i % 2 == 0 {
            out.clear();
        }
        let before = owned(&out);
        stroke_into(&build(&cmds), half, &mut out);
        assert_eq!(owned(&out), [before, expected].concat(), "{cmds:?}");
    });
}

#[test]
fn interpolation_in_i64_equals_the_one_in_i128() {
    // docs/adr/0015: the i64 path of `interpolate` gives the quotient of
    // `div_round` in i128, also at the extremes of the coordinates.
    let mut rng = Rng(0x0015_0006);
    let extreme = [i64::from(i32::MIN), i64::from(i32::MAX), 0, 1, -1];
    (0..200_000).for_each(|k| {
        let mut v = |span: i64| match k % 7 {
            0 => extreme[rng.range(0, 5) as usize],
            _ => rng.range(-span, span),
        };
        let span = [256, 1 << 16, 1 << 31][k % 3];
        let (a0, a1, b0, b1) = (v(span), v(span), v(span), v(span));
        if a0 == a1 {
            return;
        }
        // `at` is between a0 and a1, as the crossings of a segment are.
        let (lo, hi) = (a0.min(a1), a0.max(a1));
        let at = match k % 5 {
            0 => lo,
            1 => hi,
            _ => lo + rng.range(0, hi - lo + 1),
        };
        let expected = b0
            + crate::fixed::div_round(
                i128::from(b1 - b0) * i128::from(at - a0),
                i128::from(a1 - a0),
            ) as i64;
        assert_eq!(
            interpolate(a0, a1, b0, b1, at),
            expected,
            "{a0} {a1} {b0} {b1} {at}"
        );
    });
}

#[test]
fn filling_a_canvas_paints_the_coverage() {
    // docs/adr/0015: compositing as the pixels are swept gives the image of
    // painting the coverage, for small and large paths, over paths before.
    let mut rng = Rng(0x0015_0008);
    [(40, 30, -2560, 12800), (600, 500, -25600, 179200)]
        .iter()
        .for_each(|&(w, h, lo, hi)| {
            let mut scratch = Scratch::default();
            let mut filled = Canvas::new(w, h);
            let mut painted = Canvas::new(w, h);
            (0..40).for_each(|k| {
                let path = build(&random_cmds(&mut rng, true, lo, hi));
                let rule = [FillRule::NonZero, FillRule::EvenOdd][k % 2];
                let rgb = [rng.range(0, 256) as u8, rng.range(0, 256) as u8, k as u8];
                filled.fill(&path, rule, rgb, &mut scratch);
                painted.paint(&coverage(&path, rule, w, h), rgb);
                assert!(filled == painted, "{k}");
            });
        });
}

fn interpolations_equal_interpolate(a0: i64, a1: i64, b0: i64, b1: i64) {
    let carried: Vec<(i64, i64)> = interpolations(a0, a1, b0, b1, crossings(a0, a1)).collect();
    let each: Vec<(i64, i64)> = crossings(a0, a1)
        .map(|at| (at, interpolate(a0, a1, b0, b1, at)))
        .collect();
    assert_eq!(carried, each, "{a0} {a1} {b0} {b1}");
}

#[test]
fn carried_crossings_equal_the_divided_ones() {
    // docs/adr/0019: the quotient and the remainder are carried from one
    // crossing to the next, with the same rounding as one division each.
    let mut rng = Rng(0x0019_0005);
    let extreme = [i64::from(i32::MIN), i64::from(i32::MAX), 0, 1, -1];
    (0..200_000).for_each(|k| {
        let span = [256, 1 << 16, 1 << 31][k % 3];
        let mut v = |span: i64| match k % 7 {
            0 => extreme[rng.range(0, 5) as usize],
            _ => rng.range(-span, span),
        };
        let (a0, b0, b1) = (v(1 << 31), v(span), v(span));
        // Up to 40 crossings, one, or none; both directions.
        let a1 =
            (a0 + rng.range(-40 * 256, 40 * 256)).clamp(i64::from(i32::MIN), i64::from(i32::MAX));
        if a0 != a1 {
            interpolations_equal_interpolate(a0, a1, b0, b1);
        }
    });
    // Products past i64: every crossing of the whole range of i32.
    let (lo, hi) = (i64::from(i32::MIN), i64::from(i32::MAX));
    interpolations_equal_interpolate(lo, hi, lo, hi);
    interpolations_equal_interpolate(hi, lo, lo, hi);
}

#[test]
fn filling_paths_cut_at_the_edges_paints_the_coverage() {
    // docs/adr/0019: only the touched span of a row of the grid is swept;
    // paths cut at the left and the right edges, and cells of column −1.
    let mut rng = Rng(0x0019_0006);
    [
        (40, 30, -5000, 15000),
        (40, 30, 8000, 30000),
        (12, 9, -3000, 1000),
        (600, 500, -60000, 200000),
    ]
    .iter()
    .for_each(|&(w, h, lo, hi)| {
        let mut scratch = Scratch::default();
        let mut filled = Canvas::new(w, h);
        let mut painted = Canvas::new(w, h);
        (0..60).for_each(|k| {
            let path = build(&random_cmds(&mut rng, true, lo, hi));
            let rule = [FillRule::NonZero, FillRule::EvenOdd][k % 2];
            let rgb = match k % 3 {
                0 => [0, 0, 0],
                _ => [rng.range(0, 256) as u8, rng.range(0, 256) as u8, k as u8],
            };
            filled.fill(&path, rule, rgb, &mut scratch);
            painted.paint(&coverage(&path, rule, w, h), rgb);
            assert!(filled == painted, "{k}");
        });
    });
}

#[test]
fn the_grid_is_zero_between_fills() {
    // docs/adr/0019: the swept slots are zeroed after each fill, and fills
    // with one scratch give the image of fills with new ones.
    let mut rng = Rng(0x0019_0007);
    let (w, h) = (40, 30);
    let mut scratch = Scratch::default();
    let mut kept = Canvas::new(w, h);
    let mut fresh = Canvas::new(w, h);
    (0..200).for_each(|k| {
        let hi = [3000, 12800, 30000][k % 3];
        let path = build(&random_cmds(&mut rng, true, -2560, hi));
        let rule = [FillRule::NonZero, FillRule::EvenOdd][k % 2];
        let rgb = [k as u8, 0, 255 - k as u8];
        kept.fill(&path, rule, rgb, &mut scratch);
        fresh.fill(&path, rule, rgb, &mut Scratch::default());
        assert!(kept == fresh, "{k}");
        assert!(scratch.grid.iter().all(|&s| s == (0, 0)), "{k}");
    });
}

#[test]
fn painting_black_is_compositing_black() {
    // docs/adr/0019: black has no products of its own.
    (0..=255u8).for_each(|a| {
        (0..=255u8).for_each(|p| {
            let mut px = [p, p, 255 - p];
            paint_black(&mut px, a);
            let expected = [
                composite(0, a, p),
                composite(0, a, p),
                composite(0, a, 255 - p),
            ];
            assert_eq!(px, expected, "{a} {p}");
        })
    });
}
