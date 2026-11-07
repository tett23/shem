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
    random_cmds_from(rng, curves, |rng| rng.point(lo, hi))
}

/// [`random_cmds`] with the points that `point` draws.
fn random_cmds_from(
    rng: &mut Rng,
    curves: bool,
    mut point: impl FnMut(&mut Rng) -> Point,
) -> Vec<Cmd> {
    let n = rng.range(1, 30);
    let mut last = point(rng);
    let mut cmds = vec![Cmd::Move(last)];
    (0..n).for_each(|_| {
        let p = if rng.range(0, 6) == 0 {
            last
        } else {
            point(rng)
        };
        let cmd = match rng.range(0, if curves { 10 } else { 8 }) {
            0 => Cmd::Move(p),
            1 => Cmd::Close,
            8 | 9 => Cmd::Curve(point(rng), point(rng), p),
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

/// The coverage of a path equals the reference, and filling `filled` with
/// it, with `scratch` kept from the fills before, gives `painted` with the
/// coverage painted (docs/adr/0019). The grid is zero afterwards.
fn check_fill(
    cmds: &[Cmd],
    rule: FillRule,
    rgb: [u8; 3],
    filled: &mut Canvas,
    painted: &mut Canvas,
    scratch: &mut Scratch,
) -> Vec<(usize, usize, u8)> {
    let (w, h) = (filled.width, filled.height);
    let (path, reference) = (build(cmds), build_old(cmds));
    let expected = old::coverage(&reference, old_rule(rule), w, h);
    assert_eq!(
        coverage_with(&path, rule, w, h, scratch),
        expected.as_slice(),
        "{w}x{h} {rule:?} {cmds:?}"
    );
    filled.fill(&path, rule, rgb, scratch);
    painted.paint(&expected, rgb);
    assert!(filled == painted, "{w}x{h} {rule:?} {cmds:?}");
    assert!(
        scratch.grid.iter().all(|&s| s == (0, 0)),
        "{w}x{h} {rule:?} {cmds:?}"
    );
    expected
}

/// The rule and the color of the k-th fill of a test: both rules, black
/// (painted on its own in docs/adr/0019) and other colors.
fn rule_and_color(rng: &mut Rng, k: usize) -> (FillRule, [u8; 3]) {
    let rule = [FillRule::NonZero, FillRule::EvenOdd][k % 2];
    let rgb = match k % 3 {
        0 => [0, 0, 0],
        _ => [rng.range(0, 256) as u8, rng.range(0, 256) as u8, k as u8],
    };
    (rule, rgb)
}

/// [`render_line`] before docs/adr/0019: one `interpolate` at each row.
fn render_line_divided(add: &mut impl FnMut(i32, i32, i64, i64), p0: Point, p1: Point) {
    let (x0, y0, x1, y1) = (
        i64::from(p0.0),
        i64::from(p0.1),
        i64::from(p1.0),
        i64::from(p1.1),
    );
    if y0 == y1 {
        return;
    }
    let mut last = (x0, y0);
    for y in crossings(y0, y1) {
        let p = (interpolate(y0, y1, x0, x1, y), y);
        render_row_piece(add, last, p);
        last = p;
    }
    render_row_piece(add, last, (x1, y1));
}

#[test]
fn a_segment_whose_products_pass_i64_is_divided_at_each_row() {
    // docs/adr/0019: the quotient is carried only when the product of the
    // differences of a segment fits in i64. A segment across more than
    // half of the range of i32 in both x and y is divided at each row
    // instead, as before. The pieces within a row never are: their |dy|
    // is at most 256, so their product is below 2^40. The reference of
    // v0.1.0-rc2 allocates at each of the 2^23 rows, so the expected cells
    // come from the form before docs/adr/0019.
    let (lo, hi) = (i32::MIN, i32::MAX);
    // (hi − lo) · (1 − lo) = (2^32 − 1) (2^31 + 1) passes i64::MAX.
    assert!(
        (i64::from(hi) - i64::from(lo))
            .checked_mul(1 - i64::from(lo))
            .is_none()
    );
    // Down in y and up in x, and the reverse.
    let segments = [((lo, 1), (hi, lo)), ((hi, lo), (lo, 1))];
    // The cells of a few rows across the range, with the first and the
    // last, as the whole are too many to hold.
    let keep = |row: i32| row.rem_euclid(1 << 20) < 2 || row == -1 || row == i32::MIN >> 8;
    segments.iter().for_each(|&(p0, p1)| {
        let mut cells = Vec::new();
        let mut expected = Vec::new();
        render_line(
            &mut |row, col, cover, area| {
                if keep(row) {
                    cells.push((row, col, cover, area));
                }
            },
            p0,
            p1,
        );
        render_line_divided(
            &mut |row, col, cover, area| {
                if keep(row) {
                    expected.push((row, col, cover, area));
                }
            },
            p0,
            p1,
        );
        merge(&mut cells);
        merge(&mut expected);
        assert!(cells.len() > 16, "{p0:?} {p1:?}: {}", cells.len());
        assert_eq!(cells, expected, "{p0:?} {p1:?}");
    });
}

#[test]
fn paths_left_of_the_canvas_give_no_pixels_and_wider_ones_their_cover() {
    // docs/adr/0019: the cells left of the canvas are summed in the slot of
    // column −1, which gives no pixel of its own. A path entirely left of
    // the canvas has every cell there, and gives nothing; a path wider than
    // the canvas touches only that slot in its rows, and its cover gives
    // the row from the left edge to the right one.
    let mut rng = Rng(0x0019_0008);
    [
        (40, 30, -5000, -100),
        (1, 30, -5000, -100),
        (8, 8, -5000, 5000),
        (1, 30, -5000, 5000),
        (3, 2, -5000, 5000),
    ]
    .iter()
    .for_each(|&(w, h, lo, hi)| {
        let mut scratch = Scratch::default();
        let mut filled = Canvas::new(w, h);
        let mut painted = Canvas::new(w, h);
        (0..40).for_each(|k| {
            let cmds = random_cmds(&mut rng, true, lo, hi);
            let (rule, rgb) = rule_and_color(&mut rng, k);
            let pixels = check_fill(&cmds, rule, rgb, &mut filled, &mut painted, &mut scratch);
            if hi < 0 {
                assert!(pixels.is_empty(), "{cmds:?}");
            }
        });
        if hi < 0 {
            assert!(filled == Canvas::new(w, h));
        }
    });
    // A band wider than an 8 × 8 canvas, twice and three times, under both
    // rules: the slot of column −1 holds a cover of 2 and 3 rows, and the
    // even-odd rule folds it.
    (2..=3).for_each(|n| {
        let mut path = Path::new();
        (0..n).for_each(|_| {
            path.move_to((-2 * ONE, 2 * ONE));
            path.line_to((20 * ONE, 2 * ONE));
            path.line_to((20 * ONE, 6 * ONE));
            path.line_to((-2 * ONE, 6 * ONE));
            path.close();
        });
        let band: Vec<(usize, usize, u8)> = (2..6)
            .flat_map(|y| (0..8).map(move |x| (x, y, 255)))
            .collect();
        assert_eq!(coverage(&path, FillRule::NonZero, 8, 8), band, "{n}");
        let even_odd = coverage(&path, FillRule::EvenOdd, 8, 8);
        if n % 2 == 0 {
            assert!(even_odd.is_empty(), "{n}");
        } else {
            assert_eq!(even_odd, band, "{n}");
        }
    });
}

#[test]
fn canvases_of_one_pixel_equal_the_reference() {
    // docs/adr/0019: a canvas one pixel wide has the columns −1 and 0
    // only, and one pixel high has one row; the runs of a row stay within
    // the canvas.
    let mut rng = Rng(0x0019_0009);
    [
        (1, 30, -3000, 3000),
        (40, 1, -3000, 3000),
        (1, 1, -1000, 1500),
        (2, 2, -1000, 1500),
    ]
    .iter()
    .for_each(|&(w, h, lo, hi)| {
        let mut scratch = Scratch::default();
        let mut filled = Canvas::new(w, h);
        let mut painted = Canvas::new(w, h);
        (0..60).for_each(|k| {
            let cmds = random_cmds(&mut rng, true, lo, hi);
            let (rule, rgb) = rule_and_color(&mut rng, k);
            check_fill(&cmds, rule, rgb, &mut filled, &mut painted, &mut scratch);
        });
    });
}

#[test]
fn paths_on_the_rows_and_the_columns_equal_the_reference() {
    // docs/adr/0019: points on the rows, the columns and the edges of the
    // canvas, one unit off them, and one pixel beyond them: edges of no
    // height, edges on a column that give cells of no area, and vertices
    // at the corners of the canvas, in the combinations the generator
    // draws.
    let mut rng = Rng(0x0019_000a);
    [(8, 6), (40, 30), (1, 1)].iter().for_each(|&(w, h)| {
        let near = |n: usize| {
            let end = n as i64 * ONE64;
            [
                -ONE64,
                -1,
                0,
                1,
                ONE64 - 1,
                ONE64,
                ONE64 + 1,
                end - ONE64 - 1,
                end - ONE64,
                end - 1,
                end,
                end + 1,
                end + ONE64,
            ]
        };
        let (xs, ys) = (near(w), near(h));
        let mut scratch = Scratch::default();
        let mut filled = Canvas::new(w, h);
        let mut painted = Canvas::new(w, h);
        (0..150).for_each(|k| {
            let cmds = random_cmds_from(&mut rng, true, |rng| {
                (
                    xs[rng.range(0, xs.len() as i64) as usize] as i32,
                    ys[rng.range(0, ys.len() as i64) as usize] as i32,
                )
            });
            let (rule, rgb) = rule_and_color(&mut rng, k);
            check_fill(&cmds, rule, rgb, &mut filled, &mut painted, &mut scratch);
        });
    });
}

#[test]
fn the_largest_dense_grid_and_the_smallest_sparse_one_equal_the_reference() {
    // docs/adr/0014, 0019: a path whose rows × columns are MAX_GRID is
    // summed in the dense grid, one column more is sorted; the column −1
    // counts. Both equal the reference and paint the coverage.
    let mut rng = Rng(0x0019_000b);
    let side = 256;
    assert_eq!(side * side, MAX_GRID);
    let (w, h) = (300, 300);
    // The square from (x0, 0) to (x1, side · ONE − 1) fixes the bounds; the
    // random subpaths are within it.
    [
        (0, side * ONE64 - 1, true),
        (0, side * ONE64 + ONE64 - 1, false),
        (-1, side * ONE64 - ONE64 - 1, true),
        (-1, side * ONE64 - 1, false),
    ]
    .iter()
    .for_each(|&(x0, x1, dense)| {
        let mut filled = Canvas::new(w, h);
        let mut painted = Canvas::new(w, h);
        (0..6).for_each(|k| {
            let y1 = side * ONE64 - 1;
            let mut cmds = vec![
                Cmd::Move((x0 as i32, 0)),
                Cmd::Line((x1 as i32, 0)),
                Cmd::Line((x1 as i32, y1 as i32)),
                Cmd::Line((x0 as i32, y1 as i32)),
                Cmd::Close,
            ];
            cmds.extend(random_cmds(&mut rng, true, x0.max(0), y1.min(x1)));
            let (rule, rgb) = rule_and_color(&mut rng, k);
            // A new scratch shows the branch: the dense grid grows to the
            // cells, and the sorted cells stay in the other buffer.
            let mut scratch = Scratch::default();
            check_fill(&cmds, rule, rgb, &mut filled, &mut painted, &mut scratch);
            if dense {
                assert_eq!(scratch.grid.len() as i64, MAX_GRID, "{x0} {x1}");
                assert!(scratch.cells.is_empty(), "{x0} {x1}");
            } else {
                assert!(scratch.grid.is_empty(), "{x0} {x1}");
                assert!(!scratch.cells.is_empty(), "{x0} {x1}");
            }
        });
    });
}

#[test]
fn one_scratch_serves_canvases_of_different_sizes() {
    // docs/adr/0014, 0019: a thread keeps one scratch across the pages it
    // renders. The grid is laid out by the columns of each path, so a
    // smaller page after a larger one, or a dense fill after a sparse one,
    // finds the grid zero and the buffers large enough.
    let mut rng = Rng(0x0019_000c);
    let mut scratch = Scratch::default();
    let mut filled = Canvas::new(0, 0);
    let mut painted = Canvas::new(0, 0);
    [
        (40, 30),
        (600, 500),
        (12, 9),
        (1, 1),
        (300, 300),
        (40, 30),
        (0, 0),
        (2, 300),
    ]
    .iter()
    .enumerate()
    .for_each(|(i, &(w, h))| {
        filled.reset(w, h);
        painted.reset(w, h);
        assert_eq!(filled.pixels.len(), w * h * 3);
        let side = w.max(h).max(1) as i64;
        let (lo, hi) = (-side * 64, side * 320);
        (0..12).for_each(|k| {
            let cmds = random_cmds(&mut rng, true, lo, hi);
            let (rule, rgb) = rule_and_color(&mut rng, i * 12 + k);
            let before = (scratch.grid.len(), scratch.cells.capacity());
            check_fill(&cmds, rule, rgb, &mut filled, &mut painted, &mut scratch);
            // The buffers only grow.
            assert!(scratch.grid.len() >= before.0 && scratch.cells.capacity() >= before.1);
            // A fill with a new scratch gives the same image over it.
            let mut fresh = painted.clone();
            fresh.fill(&build(&cmds), rule, rgb, &mut Scratch::default());
            filled.fill(&build(&cmds), rule, rgb, &mut scratch);
            assert!(filled == fresh, "{w}x{h} {k}");
            painted = fresh;
        });
    });
}

#[test]
fn the_runs_of_a_row_are_in_order_and_within_the_canvas() {
    // docs/adr/0019: `Canvas::fill` slices the pixels of a row by the runs
    // that `visit_rows` gives, so no run is empty or past the right edge,
    // the runs come in the order of the pixels without overlapping, and
    // their pixels of an opacity above 0 are the coverage.
    let mut rng = Rng(0x0019_000d);
    [
        (40, 30, -2560, 12800),
        (600, 500, -25600, 179200),
        (1, 30, -3000, 3000),
    ]
    .iter()
    .for_each(|&(w, h, lo, hi)| {
        let mut scratch = Scratch::default();
        (0..40).for_each(|k| {
            let cmds = random_cmds(&mut rng, true, lo, hi);
            let path = build(&cmds);
            let rule = [FillRule::NonZero, FillRule::EvenOdd][k % 2];
            let mut pixels = Vec::new();
            let mut end: Option<(usize, usize)> = None;
            visit_rows(&path, rule, w, h, &mut scratch, |y, x0, alphas| {
                assert!(!alphas.is_empty(), "{cmds:?}");
                assert!(y < h && x0 + alphas.len() <= w, "{cmds:?}");
                assert!(end.is_none_or(|e| e <= (y, x0)), "{cmds:?}");
                end = Some((y, x0 + alphas.len()));
                pixels.extend(
                    alphas
                        .iter()
                        .zip(x0..)
                        .filter(|&(&a, _)| a > 0)
                        .map(|(&a, x)| (x, y, a)),
                );
            });
            assert_eq!(pixels, coverage(&path, rule, w, h), "{cmds:?}");
        });
    });
}

#[test]
fn painting_a_color_is_compositing_it() {
    // docs/adr/0019: `paint_bytes` writes the color at 255 and leaves the
    // pixel at 0, as `composite` gives, for every opacity and pixel.
    [[0, 0, 0], [255, 255, 255], [1, 254, 128], [200, 10, 77]]
        .iter()
        .for_each(|&rgb| {
            (0..=255u8).for_each(|a| {
                (0..=255u8).for_each(|p| {
                    let mut px = [p, 255 - p, p ^ 0x55];
                    let expected = [
                        composite(rgb[0], a, px[0]),
                        composite(rgb[1], a, px[1]),
                        composite(rgb[2], a, px[2]),
                    ];
                    paint_bytes(&mut px, a, rgb);
                    assert_eq!(px, expected, "{rgb:?} {a} {p}");
                })
            });
        });
}

/// The crossing of `at` divided in i128 on its own (docs/adr/0020): unlike
/// `interpolate`, the differences of any two i64 are taken in i128.
fn divided_in_i128(a0: i64, a1: i64, b0: i64, b1: i64, at: i64) -> i64 {
    let w = i128::from;
    (w(b0) + crate::fixed::div_round((w(b1) - w(b0)) * (w(at) - w(a0)), w(a1) - w(a0))) as i64
}

fn carried_equal_divided_in_i128(a0: i64, a1: i64, b0: i64, b1: i64) {
    // Compared as they come, without collecting the crossings of a long
    // segment.
    let each = crossings(a0, a1).map(|at| (at, divided_in_i128(a0, a1, b0, b1, at)));
    let differ = interpolations(a0, a1, b0, b1, crossings(a0, a1))
        .zip(each)
        .find(|(carried, each)| carried != each);
    assert_eq!(differ, None, "{a0} {a1} {b0} {b1}");
    assert_eq!(
        interpolations(a0, a1, b0, b1, crossings(a0, a1)).count(),
        crossings(a0, a1).count()
    );
}

#[test]
fn carried_crossings_are_right_for_every_i64() {
    // docs/adr/0020: any a0, a1, b0, b1 of i64, with a1 within 40 rows of
    // a0, give the crossings divided one by one in i128.
    let mut rng = Rng(0x0020_0001);
    let near = [
        i64::MIN,
        i64::MAX,
        0,
        1,
        -1,
        1 << 31,
        -(1 << 31),
        1 << 54,
        -(1 << 54),
        1 << 61,
        -(1 << 61),
    ];
    (0..200_000).for_each(|k| {
        let mut v = || match rng.next() % 4 {
            0 => near[(rng.next() % near.len() as u64) as usize]
                .saturating_add((rng.next() % 513) as i64 - 256),
            1 => (rng.next() as i64) >> (rng.next() % 64),
            _ => rng.next() as i64,
        };
        let (a0, b0, b1) = (v(), v(), v());
        let a1 = a0.saturating_add(rng.range(-40 * 256, 40 * 256));
        if a0 != a1 {
            carried_equal_divided_in_i128(a0, a1, b0, b1);
        }
        // Both directions of the same segment.
        if k % 2 == 0 && a0 != a1 {
            carried_equal_divided_in_i128(a1, a0, b1, b0);
        }
    });
}

#[test]
fn the_two_paths_of_the_crossings_meet_at_their_bounds() {
    // docs/adr/0020: i64 while |db| ≤ 2^54 and d ≤ 2^61, i128 past them;
    // both give the same crossings at the bounds, ties included.
    let (db_max, d_max) = (1i64 << 54, 1i64 << 61);
    [db_max - 1, db_max, db_max + 1].iter().for_each(|&db| {
        [-1i64, 1].iter().for_each(|&sign| {
            // A short segment: a few crossings, b over db.
            let (a0, a1) = (-1000, 3 * 256 + 17);
            carried_equal_divided_in_i128(a0, a1, 7, 7 + sign * db);
            carried_equal_divided_in_i128(a1, a0, 7 + sign * db, 7);
            let expected = if db <= db_max { "i64" } else { "i128" };
            assert_eq!(crossing_path(a0, a1, 7, 7 + sign * db), expected, "{db}");
        })
    });
    [d_max - 1, d_max, d_max + 1].iter().for_each(|&d| {
        // A long segment: its first and last crossings, and ties of the
        // rounding when db is half of d.
        let (a0, a1) = (-5, d - 5);
        [1, d / 2, -(d / 2), 1 << 54].iter().for_each(|&db| {
            let firsts: Vec<(i64, i64)> = interpolations(a0, a1, 3, 3 + db, crossings(a0, a1))
                .take(40)
                .collect();
            let each: Vec<(i64, i64)> = crossings(a0, a1)
                .take(40)
                .map(|at| (at, divided_in_i128(a0, a1, 3, 3 + db, at)))
                .collect();
            assert_eq!(firsts, each, "{d} {db}");
            let expected = if d <= d_max && db.abs() <= db_max {
                "i64"
            } else {
                "i128"
            };
            assert_eq!(crossing_path(a0, a1, 3, 3 + db), expected, "{d} {db}");
        });
    });
    // Differences that do not fit in i64 at all.
    assert_eq!(
        crossing_path(i64::MIN, i64::MIN + 600, i64::MIN, i64::MAX),
        "i128"
    );
    carried_equal_divided_in_i128(i64::MIN, i64::MIN + 600, i64::MIN, i64::MAX);
    carried_equal_divided_in_i128(i64::MAX, i64::MAX - 600, i64::MAX, i64::MIN);
}

fn cells_equal_the_divided_ones(p0: Point, p1: Point) {
    let mut carried = Vec::new();
    render_line(
        &mut |r, c, cover, area| carried.push((r, c, cover, area)),
        p0,
        p1,
    );
    let mut each = Vec::new();
    render_line_divided(
        &mut |r, c, cover, area| each.push((r, c, cover, area)),
        p0,
        p1,
    );
    assert!(carried == each, "{p0:?} {p1:?}");
}

#[test]
fn segments_near_the_ends_of_i32_are_cut_as_before() {
    // docs/adr/0020: the segments between points near the ends of i32 and
    // near 0, short enough to walk, give the cells of the division at each
    // row; a few cross the whole range.
    let ends = [
        i32::MIN,
        i32::MIN + 1,
        i32::MIN + 300,
        -1,
        0,
        1,
        i32::MAX - 300,
        i32::MAX - 1,
        i32::MAX,
    ];
    let short = |a: i32, b: i32| (i64::from(a) - i64::from(b)).abs() <= 1 << 16;
    ends.iter().for_each(|&x0| {
        ends.iter().for_each(|&y0| {
            ends.iter().filter(|&&x1| short(x0, x1)).for_each(|&x1| {
                ends.iter()
                    .filter(|&&y1| y1 != y0 && short(y0, y1))
                    .for_each(|&y1| cells_equal_the_divided_ones((x0, y0), (x1, y1)))
            })
        })
    });
    let (lo, hi) = (i32::MIN, i32::MAX);
    cells_equal_the_divided_ones((lo, hi), (hi, lo));
    cells_equal_the_divided_ones((lo, 0), (hi, 1));
}

#[test]
fn pieces_of_a_row_between_the_ends_of_i32_are_divided_as_before() {
    // docs/adr/0020: the column crossings of a piece within one row, with x
    // at the ends of i32 and the row near the ends too.
    let xs = [
        i64::from(i32::MIN),
        i64::from(i32::MIN) + 300,
        -1,
        0,
        1,
        i64::from(i32::MAX) - 300,
        i64::from(i32::MAX),
    ];
    let rows = [i64::from(i32::MIN), -256, 0, i64::from(i32::MAX) - 256];
    xs.iter().for_each(|&ax| {
        xs.iter()
            .filter(|&&bx| bx != ax && (bx - ax).abs() <= 1 << 16)
            .for_each(|&bx| {
                rows.iter().for_each(|&row| {
                    [0, 1, 128, 255, 256].iter().for_each(|&dy| {
                        carried_equal_divided_in_i128(ax, bx, row, row + dy);
                        interpolations_equal_interpolate(ax, bx, row, row + dy);
                    })
                })
            })
    });
    // A piece across the whole range of i32.
    let (lo, hi) = (i64::from(i32::MIN), i64::from(i32::MAX));
    carried_equal_divided_in_i128(lo, hi, i64::from(i32::MAX) - 256, i64::from(i32::MAX));
}
