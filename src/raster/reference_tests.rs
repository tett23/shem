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
    let mut rng = Rng(0x0013_0004);
    let (w, h) = (40, 30);
    let mut scratch = Scratch::default();
    (0..200).for_each(|_| {
        // Some points are outside the canvas.
        let cmds = random_cmds(&mut rng, true, -2560, 12800);
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
