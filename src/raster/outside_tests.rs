//! The segments and the curves off the canvas take no time, and the pixels
//! are those of the reference of v0.1.0-rc2, which skips nothing
//! (docs/adr/0022).

use super::reference as old;
use super::reference_tests::{Cmd, Rng, build_old, old_rule, random_cmds_from};
use super::*;

const LO: i64 = i32::MIN as i64;
const HI: i64 = i32::MAX as i64;

fn build_within(cmds: &[Cmd], frame: &Frame) -> Path {
    let mut path = Path::new();
    cmds.iter().for_each(|&c| match c {
        Cmd::Move(p) => path.move_to(p),
        Cmd::Line(p) => path.line_to(p),
        Cmd::Curve(a, b, p) => path.curve_to_within(a, b, p, frame),
        Cmd::Close => path.close(),
    });
    path
}

/// A point of a path on a `w` × `h` canvas: far from it (within 2^22
/// units, as the reference walks every row of a segment), near it (within
/// 4 pixels), on its edges, or inside it.
fn mixed_point(rng: &mut Rng, w: usize, h: usize) -> Point {
    let (wu, hu) = (w as i64 * ONE64, h as i64 * ONE64);
    let near = |rng: &mut Rng, end: i64| rng.range(-4 * ONE64, end + 4 * ONE64 + 1);
    let on_edge = |rng: &mut Rng, end: i64| [0, end][rng.range(0, 2) as usize];
    let far = |rng: &mut Rng| rng.range(-(1 << 22), 1 << 22);
    let (x, y) = match rng.range(0, 12) {
        0 | 1 => (far(rng), far(rng)),
        2 => (on_edge(rng, wu), near(rng, hu)),
        3 => (near(rng, wu), on_edge(rng, hu)),
        4 => (on_edge(rng, wu), far(rng)),
        5 => (far(rng), on_edge(rng, hu)),
        6..=8 => (near(rng, wu), near(rng, hu)),
        _ => (rng.range(0, wu + 1), rng.range(0, hu + 1)),
    };
    (x as i32, y as i32)
}

/// The coverage of the path of `cmds` flattened within `frame`, under both
/// rules, equals the reference, which flattens every curve.
fn assert_coverage(cmds: &[Cmd], frame: &Frame, w: usize, h: usize, scratch: &mut Scratch) {
    let (path, reference) = (build_within(cmds, frame), build_old(cmds));
    [FillRule::NonZero, FillRule::EvenOdd]
        .iter()
        .for_each(|&rule| {
            let expected = old::coverage(&reference, old_rule(rule), w, h);
            assert_eq!(
                coverage_with(&path, rule, w, h, scratch),
                expected.as_slice(),
                "{w}x{h} {rule:?} {cmds:?}"
            );
        });
}

#[test]
fn a_segment_across_the_range_adds_few_cells() {
    // A segment from (−2^30, −2^30) to (2^30, 2^30) crosses 2^23 rows, of
    // which a 100 × 100 canvas has 100: at most 4 (w + h) + 8 cells.
    let (w, h) = (100, 100);
    let area = Area::new(w, h);
    let (p0, p1) = ((-(1 << 30), -(1 << 30)), (1 << 30, 1 << 30));
    [(p0, p1), (p1, p0)].iter().for_each(|&(a, b)| {
        let ((), adds) = counting_adds(|| render_line(&mut |_, _, _, _| {}, a, b, area));
        assert!(adds <= 4 * (w + h) as u64 + 8, "{a:?} {b:?}: {adds}");
    });
}

#[test]
fn curves_with_far_control_points_flatten_to_few_points() {
    // On a 100 × 100 canvas, a curve across it with control points at
    // ±2^30 is split only near the canvas; one in a region beyond the
    // canvas is one segment.
    let (w, h) = (100, 100);
    let frame = Frame::around(w, h, 0);
    let big = 1 << 30;
    let across = flatten_curve_within((-big, -big), (big, -big), (-big, big), (big, big), &frame);
    assert!(across.len() <= 4096, "{}", across.len());
    let (wu, hu) = (w as i32 * ONE, h as i32 * ONE);
    let beyond = [
        // above, below, right and left of the canvas
        [(-big, -big), (big, -big), (-big, -1), (big, 0)],
        [(-big, hu), (big, hu + 1), (-big, big), (big, big)],
        [(wu, -big), (wu + 1, big), (big, -big), (big, big)],
        [(-1, -big), (-2, big), (-big, -big), (-big, big)],
    ];
    beyond.iter().for_each(|&[p0, p1, p2, p3]| {
        let points = flatten_curve_within(p0, p1, p2, p3, &frame);
        assert_eq!(points, vec![p3], "{p0:?} {p1:?} {p2:?} {p3:?}");
    });
}

#[test]
fn rectangles_off_the_canvas_add_few_cells() {
    // Finding 6 of docs/audit/2026-11-08-vulnerability-assessment.md: 200
    // rectangles of 2,000,000 points on a page of 100 points add at most
    // 4 (w + h) + 8 cells each at 72 dpi.
    let content = "0 0 2000000 2000000 re f\n".repeat(200);
    let pdf = crate::render::tests::page_pdf("0 0 100 100", "", &content, &[]);
    let (canvas, adds) = counting_adds(|| crate::render::tests::render_pdf(&pdf, 72).unwrap());
    assert!(canvas.pixels.iter().all(|&v| v == 0));
    assert!(adds <= 200 * (4 * 200 + 8), "{adds}");
}

#[test]
fn coverages_of_paths_off_the_canvas_equal_the_reference() {
    // Segments and curves between points anywhere in i32, near the canvas
    // and on its edges, on canvases of a dense grid and of sorted cells.
    let mut rng = Rng(0x0022_0001);
    [
        (40, 30, 60),
        (300, 300, 20),
        (1, 1, 40),
        (1, 40, 30),
        (40, 1, 30),
    ]
    .iter()
    .for_each(|&(w, h, n)| {
        let mut scratch = Scratch::default();
        let frame = Frame::around(w, h, 0);
        (0..n).for_each(|_| {
            let cmds = random_cmds_from(&mut rng, true, |rng| mixed_point(rng, w, h));
            assert_coverage(&cmds, &frame, w, h, &mut scratch);
        });
    });
}

#[test]
fn curves_at_the_borders_of_the_regions_equal_the_reference() {
    // An S across the left region, curves across the borders of the
    // regions, and curves whose ends are on the borders.
    let (w, h) = (40, 30);
    let (wu, hu) = (w as i32 * ONE, h as i32 * ONE);
    let frame = Frame::around(w, h, 0);
    let mut scratch = Scratch::default();
    let big = 1 << 20;
    let s_curve = |x: i32| {
        vec![
            Cmd::Move((x, 0)),
            Cmd::Curve((x - big, hu), (x + 5 * ONE, hu / 3), (x - 3 * ONE, hu / 2)),
            Cmd::Curve((x + 2 * ONE, 2 * hu), (x - big, -hu), (x, hu)),
            Cmd::Curve((x - big, big), (x + ONE, -big), (x, 0)),
            Cmd::Close,
        ]
    };
    let borders = [-1, 0, 1, wu - 1, wu, wu + 1];
    let rows = [-1, 0, 1, hu - 1, hu, hu + 1];
    let mut cases: Vec<Vec<Cmd>> = [-ONE, -1, 0, 1, ONE / 2]
        .iter()
        .map(|&x| s_curve(x))
        .collect();
    borders.iter().for_each(|&x| {
        rows.iter().for_each(|&y| {
            // A loop around (x, y) across the borders, and a curve from the
            // border to the corner of the canvas.
            cases.push(vec![
                Cmd::Move((x, y)),
                Cmd::Curve((x - big, y - big), (x + big, y - big), (x + big, y + big)),
                Cmd::Curve((x - big, y + big), (x, y - 3 * ONE), (x, y)),
                Cmd::Close,
            ]);
            cases.push(vec![
                Cmd::Move((x, y)),
                Cmd::Curve((x, -big), (wu, hu), (ONE, ONE)),
                Cmd::Line((wu / 2, hu / 2)),
                Cmd::Curve((-big, y), (x, hu + big), (x, y)),
            ]);
        });
    });
    cases
        .iter()
        .for_each(|cmds| assert_coverage(cmds, &frame, w, h, &mut scratch));
    // The same shapes shifted by random amounts.
    let mut rng = Rng(0x0022_0002);
    (0..200).for_each(|_| {
        let (dx, dy) = (
            rng.range(-3 * ONE64, 3 * ONE64),
            rng.range(-3 * ONE64, 3 * ONE64),
        );
        let shift = |p: Point| ((p.0 as i64 + dx) as i32, (p.1 as i64 + dy) as i32);
        let cmds: Vec<Cmd> = cases[rng.range(0, cases.len() as i64) as usize]
            .iter()
            .map(|&c| match c {
                Cmd::Move(p) => Cmd::Move(shift(p)),
                Cmd::Line(p) => Cmd::Line(shift(p)),
                Cmd::Curve(a, b, p) => Cmd::Curve(shift(a), shift(b), shift(p)),
                Cmd::Close => Cmd::Close,
            })
            .collect();
        assert_coverage(&cmds, &frame, w, h, &mut scratch);
    });
}

#[test]
fn strokes_of_paths_off_the_canvas_equal_the_reference() {
    // The outline of a glyph of Tr 2 is flattened within a margin of
    // 16 · half + 16, then filled and stroked: both equal the reference.
    let mut rng = Rng(0x0022_0003);
    [(40, 30, 80), (300, 300, 20), (1, 1, 30)]
        .iter()
        .for_each(|&(w, h, n)| {
            let mut scratch = Scratch::default();
            (0..n).for_each(|k| {
                let half = 1i64 << rng.range(0, 21);
                let half = if k % 3 == 0 {
                    half
                } else {
                    rng.range(1, half + 1)
                };
                let margin = 16 * half + 16;
                let frame = Frame::around(w, h, margin);
                // Every point stays within the range with the margin.
                let limit = HI - margin;
                let cmds = random_cmds_from(&mut rng, true, |rng| {
                    let (x, y) = mixed_point(rng, w, h);
                    (
                        i64::from(x).clamp(-limit, limit) as i32,
                        i64::from(y).clamp(-limit, limit) as i32,
                    )
                });
                assert_coverage(&cmds, &frame, w, h, &mut scratch);
                let (path, reference) = (build_within(&cmds, &frame), build_old(&cmds));
                let expected =
                    old::coverage(&old::stroke(&reference, half), old::FillRule::NonZero, w, h);
                assert_eq!(
                    coverage_with(&stroke(&path, half), FillRule::NonZero, w, h, &mut scratch),
                    expected.as_slice(),
                    "{w}x{h} half {half} {cmds:?}"
                );
            });
        });
}

#[test]
fn the_joins_of_a_stroke_stay_within_the_margin() {
    // The polygons of the joins are within 16 · half + 16 of their vertex
    // (docs/adr/0023), so the margin of the frame of a glyph of Tr 2 holds
    // them. Nearly straight joins of short segments, which once jumped, and
    // others.
    let mut rng = Rng(0x0022_0004);
    (0..20_000).for_each(|k| {
        let half = [1, 2, 3, 128, 256, 1 << 12, 1 << 20][k % 7];
        let margin = 16 * half + 16;
        let v: Point = (
            rng.range(-(1 << 20), 1 << 20) as i32,
            rng.range(-(1 << 20), 1 << 20) as i32,
        );
        let span = [2, 16, 256, 1 << 16][k % 4];
        let mut d = || (rng.range(-span, span + 1), rng.range(-span, span + 1));
        let d1 = d();
        let d2 = if k % 2 == 0 {
            (d1.0 * 8 + rng.range(-1, 2), d1.1 * 8 + rng.range(-1, 2))
        } else {
            d()
        };
        if d1 == (0, 0) || d2 == (0, 0) {
            return;
        }
        let a = ((v.0 as i64 - d1.0) as i32, (v.1 as i64 - d1.1) as i32);
        let c = ((v.0 as i64 + d2.0) as i32, (v.1 as i64 + d2.1) as i32);
        let mut path = Path::new();
        path.move_to(a);
        path.line_to(v);
        path.line_to(c);
        let mut out = Path::new();
        stroke_into(&path, half, &mut out);
        // The third polygon, if any, is the join.
        if let Some(join) = out.subpaths().nth(2) {
            join.points.iter().for_each(|p| {
                let (dx, dy) = (
                    (p.0 as i64 - v.0 as i64).abs(),
                    (p.1 as i64 - v.1 as i64).abs(),
                );
                assert!(
                    dx <= margin && dy <= margin,
                    "half {half} at {v:?} from {d1:?} to {d2:?}: {p:?}"
                );
            });
        }
    });
}

/// The fill and the stroke of a path of `cmds`, flattened within the
/// margin of `half`, equal the reference, which flattens every curve.
fn assert_stroke(cmds: &[Cmd], half: i64, w: usize, h: usize, scratch: &mut Scratch) {
    let frame = Frame::around(w, h, 16 * half + 16);
    assert_coverage(cmds, &frame, w, h, scratch);
    let (path, reference) = (build_within(cmds, &frame), build_old(cmds));
    let expected = old::coverage(&old::stroke(&reference, half), old::FillRule::NonZero, w, h);
    assert_eq!(
        coverage_with(&stroke(&path, half), FillRule::NonZero, w, h, scratch),
        expected.as_slice(),
        "{w}x{h} half {half} {cmds:?}"
    );
}

#[test]
fn the_curve_whose_join_jumped_strokes_as_the_reference() {
    // The counterexample of docs/adr/0023: above a 40 × 30 canvas, beyond
    // the margin of a line of half 128, a small curve whose flattening had
    // a join that jumped into the canvas. Pruned or not, it now draws
    // nothing; the shifted copies that reach the canvas draw the same.
    let (w, h) = (40, 30);
    let mut scratch = Scratch::default();
    let curve = |dx: i32, dy: i32| {
        vec![
            Cmd::Move((34 + dx, -2402 + dy)),
            Cmd::Curve(
                (98 + dx, -2321 + dy),
                (17 + dx, -2406 + dy),
                (289 + dx, -2125 + dy),
            ),
        ]
    };
    let frame = Frame::around(w, h, 16 * 128 + 16);
    assert!(
        coverage(
            &stroke(&build_within(&curve(0, 0), &frame), 128),
            FillRule::NonZero,
            w,
            h
        )
        .is_empty()
    );
    [
        (0, 0),
        (0, 2000),
        (0, 2100),
        (0, 2500),
        (3000, 0),
        (-500, 0),
    ]
    .iter()
    .for_each(|&(dx, dy)| assert_stroke(&curve(dx, dy), 128, w, h, &mut scratch));
}

#[test]
fn strokes_of_small_curves_beyond_the_margin_equal_the_reference() {
    // Small curves just beyond the margin above the canvas, with lines of
    // half 128 to 2^16: the search that found the jumps of docs/adr/0023,
    // now comparing the pruned stroke with the reference.
    let mut rng = Rng(0x0022_ffff);
    let (w, h) = (40, 30);
    let mut scratch = Scratch::default();
    (0..3000).for_each(|k| {
        let half = [128, 512, 4096, 1 << 16][k % 4];
        let margin = 16 * half + 16;
        let top = -margin - 1 - rng.range(0, 4 * half);
        let span = [100, 300, 1000, 5000, 30000][(k / 4) % 5];
        let p = |rng: &mut Rng| {
            (
                rng.range(-span, span) as i32,
                (top - rng.range(0, span)) as i32,
            )
        };
        let [p0, p1, p2, p3] = std::array::from_fn(|_| p(&mut rng));
        let cmds = vec![Cmd::Move(p0), Cmd::Curve(p1, p2, p3)];
        assert_stroke(&cmds, half, w, h, &mut scratch);
    });
}

/// The cells of `render_line_divided` as `render_line` gives them on a
/// canvas: the rows of the canvas only, the cells left of it summed in
/// column −1 without area, merged.
pub(super) fn on_canvas(cells: &[Cell], area: Area) -> Vec<Cell> {
    let mut out: Vec<Cell> = cells
        .iter()
        .filter(|&&(row, _, _, _)| (0..area.height).contains(&i64::from(row)))
        .filter(|&&(_, col, _, _)| i64::from(col) < area.width)
        .map(|&(row, col, cover, area)| {
            if col < 0 {
                (row, -1, cover, 0)
            } else {
                (row, col, cover, area)
            }
        })
        .collect();
    merge(&mut out);
    out
}

fn cells_of(p0: Point, p1: Point, area: Area) -> Vec<Cell> {
    let mut cells = Vec::new();
    render_line(
        &mut |r, c, cover, area| cells.push((r, c, cover, area)),
        p0,
        p1,
        area,
    );
    merge(&mut cells);
    cells
}

/// The cells of a segment on the rows of a canvas, divided at each row
/// and at each column as the reference of v0.1.0-rc2 does, then put on
/// the canvas by [`on_canvas`]. The pieces of the rows of the canvas are
/// those between its points with 0 ≤ y ≤ H: a piece with an end beyond
/// them is in a row beyond the canvas.
fn divided_within(p0: Point, p1: Point, area: Area) -> Vec<Cell> {
    let (x0, y0, x1, y1) = (
        i64::from(p0.0),
        i64::from(p0.1),
        i64::from(p1.0),
        i64::from(p1.1),
    );
    let mut cells = Vec::new();
    if y0 == y1 {
        return cells;
    }
    let hu = area.height * ONE64;
    let within = |y: i64| (0..=hu).contains(&y);
    let (lo, hi) = (y0.min(y1), y0.max(y1));
    let (first, last) = (lo.div_euclid(ONE64) + 1, (hi - 1).div_euclid(ONE64));
    let (first, last) = (first.max(0), last.min(area.height));
    let mut ks: Vec<i64> = (first..=last).collect();
    if y0 > y1 {
        ks.reverse();
    }
    let mut points: Vec<(i64, i64)> = Vec::new();
    if within(y0) {
        points.push((x0, y0));
    }
    points.extend(
        ks.iter()
            .map(|&k| k * ONE64)
            .map(|y| (interpolate(y0, y1, x0, x1, y), y)),
    );
    if within(y1) {
        points.push((x1, y1));
    }
    points.windows(2).for_each(|w| {
        let ((ax, ay), (bx, by)) = (w[0], w[1]);
        let row = ay.min(by).div_euclid(ONE64);
        let (ylo, yhi) = (ay.min(by), ay.max(by));
        let mut pts = vec![(ax, ay)];
        if ax != bx {
            pts.extend(
                crossings(ax, bx).map(|x| (x, interpolate(ax, bx, ay, by, x).clamp(ylo, yhi))),
            );
        }
        pts.push((bx, by));
        pts.windows(2).for_each(|v| {
            let ((cx, cy), (dx, dy)) = (v[0], v[1]);
            let col = cx.min(dx).div_euclid(ONE64);
            let (fx0, fx1) = (cx - col * ONE64, dx - col * ONE64);
            let height = dy - cy;
            cells.push((row as i32, col as i32, height, (fx0 + fx1) * height));
        });
    });
    on_canvas(&cells, area)
}

#[test]
fn segments_across_the_range_meet_the_canvas_as_divided() {
    // Steep segments between the ends of i32 on small canvases: the first
    // crossing within the canvas is far from the start, so its product
    // passes i64 and is divided in i128. The cells are those of the
    // division at each row and column, within the canvas.
    let xs = [LO, LO + 300, -1, 0, 1, 1 << 20, HI - 300, HI];
    let ys = [LO, LO + 1, -1, 0, 1, HI - 1, HI];
    let areas = [
        Area::new(100, 100),
        Area::new(1, 1),
        Area::new(3000, 1),
        Area::new(1, 3000),
    ];
    let mut n = 0;
    xs.iter().for_each(|&x0| {
        ys.iter().for_each(|&y0| {
            xs.iter().for_each(|&x1| {
                ys.iter()
                    .filter(|&&y1| y1 != y0)
                    // Steep enough for the pieces of a row to be short.
                    .filter(|&&y1| (x1 - x0).abs() <= 4 * (y1 - y0).abs())
                    .for_each(|&y1| {
                        areas.iter().for_each(|&area| {
                            let (p0, p1) = ((x0 as i32, y0 as i32), (x1 as i32, y1 as i32));
                            let expected = divided_within(p0, p1, area);
                            n += expected.len();
                            assert_eq!(cells_of(p0, p1, area), expected, "{p0:?} {p1:?} {area:?}");
                        });
                    });
            });
        });
    });
    assert!(n > 1000, "{n}");
}
