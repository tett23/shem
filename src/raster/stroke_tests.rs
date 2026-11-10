//! The strokes of the paths of a page: round caps, dashes, and the work
//! off the canvas (docs/adr/0025).

use super::outside_tests::{build_within, mixed_point};
use super::reference_tests::{Cmd, Rng, build, random_cmds_from};
use super::*;

const HI: i64 = i32::MAX as i64;

/// The style of docs/adr/0025: miter joins of limit 10, butt or round caps.
fn styled(half: i64, round_cap: bool, dash: &[i64], phase: i64) -> Stroke<'_> {
    Stroke {
        half,
        cap: if round_cap { Cap::Round } else { Cap::Butt },
        join: MITER_10,
        dash,
        phase,
        flatness: FLATNESS,
    }
}

fn solid(half: i64, round_cap: bool) -> Stroke<'static> {
    styled(half, round_cap, &[], 0)
}

// --- butt caps

#[test]
fn butt_strokes_are_those_of_stroke_into() {
    // Without round caps and dashes, the polygons are those of the stroke
    // of docs/adr/0003, point for point.
    let mut rng = Rng(0x0025_0001);
    (0..300).for_each(|k| {
        let span = [1 << 10, 1 << 16, 1 << 22][k % 3];
        let cmds = random_cmds_from(&mut rng, k % 2 == 0, |rng| rng.point(-span, span));
        let path = build(&cmds);
        let half = rng.range(1, 1 << 20);
        assert_eq!(
            stroke_styled(&path, &solid(half, false), &Frame::NONE),
            stroke(&path, half),
            "{cmds:?} half {half}"
        );
    });
}

// --- the disc of the round caps

/// The squared distance of a point from the origin.
fn dist2(p: Point) -> i128 {
    i128::from(p.0) * i128::from(p.0) + i128::from(p.1) * i128::from(p.1)
}

fn sorted(points: impl Iterator<Item = Point>) -> Vec<Point> {
    let mut v: Vec<Point> = points.collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[test]
fn the_disc_is_a_circle_within_the_tolerance() {
    // Every point of the disc is within 1/16 pixel and 2 units of the
    // circle of the half width, and the cubic approximation of the
    // quarter circle, within 0.0273% of the radius (docs/adr/0025).
    let mut rng = Rng(0x0025_0002);
    let halves: Vec<i64> = [1, 2, 3, 17, 128, 255, 256, 1 << 12, 1 << 20]
        .into_iter()
        .chain((0..200).map(|_| rng.range(1, (1 << 20) + 1)))
        .collect();
    halves.iter().for_each(|&half| {
        let disc = disc(half);
        assert!(
            disc.len() >= 4 && disc.len().is_multiple_of(4),
            "half {half}: {}",
            disc.len()
        );
        assert!(signed_area(&disc) > 0, "half {half}");
        // The error of the cubic: 0.00027253 · half, as (18 / 65536) · half.
        let cubic = (i128::from(half) * 18 + 65535) / 65536;
        let tolerance = 16 + 2 + cubic;
        let (lo, hi) = (
            (i128::from(half) - tolerance).max(0),
            i128::from(half) + tolerance,
        );
        disc.iter().for_each(|&p| {
            let d2 = dist2(p);
            assert!(
                lo * lo <= d2 && d2 <= hi * hi,
                "half {half}: {p:?} is {d2} squared from the center"
            );
        });
    });
}

#[test]
fn the_disc_is_symmetric_in_the_axes_and_the_diagonals() {
    let mut rng = Rng(0x0025_0003);
    (0..200).for_each(|k| {
        let half = if k < 8 {
            k + 1
        } else {
            rng.range(1, (1 << 20) + 1)
        };
        let disc = disc(half);
        let points = sorted(disc.iter().copied());
        assert_eq!(points.len(), disc.len(), "half {half}: a point repeats");
        [
            |(x, y): Point| (-x, y),
            |(x, y): Point| (x, -y),
            |(x, y): Point| (y, x),
            |(x, y): Point| (-y, -x),
        ]
        .iter()
        .for_each(|reflect| {
            assert_eq!(
                sorted(disc.iter().map(|&p| reflect(p))),
                points,
                "half {half}"
            );
        });
    });
}

#[test]
fn the_disc_of_a_cap_is_built_once_per_stroke() {
    // The discs at the ends of each dash and each subpath are the same
    // polygon moved: the same points relative to their ends.
    let mut path = Path::new();
    path.move_to((10 * ONE, 10 * ONE));
    path.line_to((40 * ONE, 10 * ONE));
    path.move_to((10 * ONE, 20 * ONE));
    path.line_to((10 * ONE, 50 * ONE));
    let out = stroke_styled(&path, &solid(300, true), &Frame::NONE);
    let disc = disc(300);
    let discs: Vec<Vec<Point>> = out
        .subpaths()
        .filter(|s| s.points.len() == disc.len())
        .map(|s| s.points.to_vec())
        .collect();
    assert_eq!(discs.len(), 4);
    [
        (10 * ONE, 10 * ONE),
        (40 * ONE, 10 * ONE),
        (10 * ONE, 20 * ONE),
        (10 * ONE, 50 * ONE),
    ]
    .iter()
    .zip(&discs)
    .for_each(|(&(cx, cy), d)| {
        let moved: Vec<Point> = disc.iter().map(|&(x, y)| (x + cx, y + cy)).collect();
        assert_eq!(*d, moved);
    });
}

// --- dashes, against a slow reference of finer lengths

/// A point in units of 2^−32 device units.
type Fine = (i128, i128);

fn fine(p: Point) -> Fine {
    (i128::from(p.0) << 32, i128::from(p.1) << 32)
}

/// The length of a segment in units of 2^−32 device units, rounded down.
fn fine_length(a: Point, b: Point) -> i128 {
    let (dx, dy) = direction(a, b);
    let d2 = (i128::from(dx) * i128::from(dx) + i128::from(dy) * i128::from(dy)) as u128;
    isqrt(d2 << 64) as i128
}

/// The point `at` fine units from `a` on the segment to `b` of `length`.
fn fine_point(a: Point, b: Point, at: i128, length: i128) -> Fine {
    let (dx, dy) = direction(a, b);
    let part = |d: i64| div_round(i128::from(d) * at, length);
    let (ax, ay) = fine(a);
    (ax + (part(dx) << 32), ay + (part(dy) << 32))
}

/// The dashes of a polyline, cut with lengths 2^16 times finer than the
/// dash units: the points of each dash that is drawn, in fine units.
fn reference_dashes(points: &[Point], closed: bool, dash: &[i64], phase: i64) -> Vec<Vec<Fine>> {
    let elements: Vec<i128> = dash.iter().map(|&d| i128::from(d) << 16).collect();
    let cycle = if dash.len() % 2 == 1 {
        2 * dash.len()
    } else {
        dash.len()
    };
    let element = |i: usize| elements[i % elements.len()];
    let period: i128 = (0..cycle).map(element).sum();
    let (mut index, mut left) = (0, element(0));
    let mut phase = (i128::from(phase) << 16) % period;
    if phase > 0 {
        while phase >= left {
            phase -= left;
            index = (index + 1) % cycle;
            left = element(index);
        }
        left -= phase;
    }
    let on = |index: usize| index.is_multiple_of(2);
    let (first, last) = (points[0], points[points.len() - 1]);
    let mut segments: Vec<(Point, Point)> = points.windows(2).map(|w| (w[0], w[1])).collect();
    if closed && first != last {
        segments.push((last, first));
    }
    let mut dashes = Vec::new();
    let mut piece: Vec<Fine> = Vec::new();
    if on(index) {
        piece.push(fine(first));
    }
    segments.iter().for_each(|&(a, b)| {
        let length = fine_length(a, b);
        let mut at = 0;
        loop {
            if left > length - at {
                left -= length - at;
                if on(index) {
                    piece.push(fine(b));
                }
                break;
            }
            at += left;
            let p = if at == length {
                fine(b)
            } else {
                fine_point(a, b, at, length)
            };
            if on(index) {
                piece.push(p);
                dashes.push(std::mem::take(&mut piece));
            }
            index = (index + 1) % cycle;
            left = element(index);
            if on(index) {
                piece.push(p);
            }
            if at == length {
                break;
            }
        }
    });
    if on(index) && piece.len() > 1 {
        dashes.push(piece);
    }
    dashes
}

/// The dashes of [`for_each_dash`] without a frame, as owned points.
fn dashes_of(points: &[Point], closed: bool, dash: &[i64], phase: i64) -> Vec<Vec<Point>> {
    let style = styled(1, false, dash, phase);
    let mut out = Vec::new();
    let mut piece = Vec::new();
    for_each_dash(
        points,
        closed,
        &style,
        &Frame::NONE,
        &mut piece,
        |d, caps| {
            assert_eq!(caps, (true, true));
            out.push(d.to_vec());
        },
    );
    out
}

/// The length of a polyline, in dash units, by [`segment_length`].
fn polyline_length(points: &[Point]) -> i128 {
    points
        .windows(2)
        .map(|w| i128::from(segment_length(w[0], w[1])))
        .sum()
}

/// The length of a polyline in fine units, from its fine points: the
/// distance between consecutive points, rounded down.
fn fine_polyline_length(points: &[Fine]) -> i128 {
    points
        .windows(2)
        .map(|w| {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            isqrt((dx * dx + dy * dy) as u128) as i128
        })
        .sum()
}

/// Compares the dashes of a polyline with the reference: as many, with
/// the same number of points, each within 2 units; the sum of the
/// lengths within 2 dash units per segment and the rounding of the ends
/// of each dash to the device grid.
fn assert_dashes(points: &[Point], closed: bool, dash: &[i64], phase: i64) {
    let actual = dashes_of(points, closed, dash, phase);
    let expected = reference_dashes(points, closed, dash, phase);
    let what = || format!("{points:?} closed {closed} dash {dash:?} phase {phase}");
    assert_eq!(actual.len(), expected.len(), "{}", what());
    actual.iter().zip(&expected).for_each(|(a, e)| {
        assert_eq!(a.len(), e.len(), "{}", what());
        a.iter().zip(e).for_each(|(&p, &q)| {
            let (dx, dy) = ((fine(p).0 - q.0).abs(), (fine(p).1 - q.1).abs());
            assert!(
                dx <= 2 << 32 && dy <= 2 << 32,
                "{}: {p:?} against {q:?}",
                what()
            );
        });
    });
    let total: i128 = actual.iter().map(|d| polyline_length(d)).sum();
    let fine_total: i128 = expected
        .iter()
        .map(|d| fine_polyline_length(d))
        .sum::<i128>()
        >> 16;
    let segments = points.len() as i128;
    let tolerance = 2 * segments + (actual.len() as i128) * (2 << 16);
    assert!(
        (total - fine_total).abs() <= tolerance,
        "{}: {total} against {fine_total}",
        what()
    );
}

/// `n` points as a polyline: in dash units, 10 pixels is 10 · 256 · 2^16.
const PX: i64 = 256 << 16;

#[test]
fn a_dash_array_cuts_a_horizontal_line() {
    let line = [(0, 0), (10 * ONE, 0)];
    let dash = [3 * PX, 2 * PX];
    let x = |d: &Vec<Point>| (d[0].0, d[d.len() - 1].0);
    let dashes = dashes_of(&line, false, &dash, 0);
    assert_eq!(
        dashes.iter().map(x).collect::<Vec<_>>(),
        vec![(0, 3 * ONE), (5 * ONE, 8 * ONE)]
    );
    let dashes = dashes_of(&line, false, &dash, PX);
    assert_eq!(
        dashes.iter().map(x).collect::<Vec<_>>(),
        vec![(0, 2 * ONE), (4 * ONE, 7 * ONE), (9 * ONE, 10 * ONE)]
    );
    assert_dashes(&line, false, &dash, 0);
    assert_dashes(&line, false, &dash, PX);
    // An odd array is doubled.
    assert_eq!(
        dashes_of(&line, false, &[3 * PX], 0),
        dashes_of(&line, false, &[3 * PX, 3 * PX], 0)
    );
}

#[test]
fn a_dash_across_a_vertex_keeps_the_vertex() {
    // An L of 6 and 6 pixels with [9 9]: one dash of three points.
    let l = [(2 * ONE, 8 * ONE), (2 * ONE, 2 * ONE), (8 * ONE, 2 * ONE)];
    let dashes = dashes_of(&l, false, &[9 * PX, 9 * PX], 0);
    assert_eq!(
        dashes,
        vec![vec![
            (2 * ONE, 8 * ONE),
            (2 * ONE, 2 * ONE),
            (5 * ONE, 2 * ONE)
        ]]
    );
    assert_dashes(&l, false, &[9 * PX, 9 * PX], 0);
}

#[test]
fn dots_are_dashes_of_no_length() {
    // [0 3] on a line of 10 pixels: dots at 0, 3, 6 and 9, each two equal points.
    let line = [(0, 0), (10 * ONE, 0)];
    let dashes = dashes_of(&line, false, &[0, 3 * PX], 0);
    assert_eq!(
        dashes,
        (0..4)
            .map(|i| vec![(3 * i * ONE, 0), (3 * i * ONE, 0)])
            .collect::<Vec<_>>()
    );
    // A dot exactly at the end is not drawn; one at a vertex is.
    let dashes = dashes_of(&[(0, 0), (9 * ONE, 0)], false, &[0, 3 * PX], 0);
    assert_eq!(dashes.len(), 3);
    let dashes = dashes_of(
        &[(0, 0), (3 * ONE, 0), (3 * ONE, 5 * ONE)],
        false,
        &[0, 3 * PX],
        0,
    );
    assert_eq!(dashes.len(), 3);
    assert_eq!(dashes[1], vec![(3 * ONE, 0), (3 * ONE, 0)]);
}

#[test]
fn a_closed_polyline_is_dashed_to_its_start() {
    // A square of 6 pixels with [4 2]: the last side has a dash from
    // its start, and the first dash starts at the start of the square.
    let square = [
        (2 * ONE, 2 * ONE),
        (8 * ONE, 2 * ONE),
        (8 * ONE, 8 * ONE),
        (2 * ONE, 8 * ONE),
    ];
    let dashes = dashes_of(&square, true, &[4 * PX, 2 * PX], 0);
    assert_eq!(dashes.len(), 4);
    assert_eq!(dashes[0], vec![(2 * ONE, 2 * ONE), (6 * ONE, 2 * ONE)]);
    assert_eq!(dashes[3], vec![(2 * ONE, 8 * ONE), (2 * ONE, 4 * ONE)]);
    assert_dashes(&square, true, &[4 * PX, 2 * PX], 0);
    // Open, the last side is not there.
    assert_eq!(dashes_of(&square, false, &[4 * PX, 2 * PX], 0).len(), 3);
}

#[test]
fn random_polylines_are_dashed_as_the_reference() {
    let mut rng = Rng(0x0025_0004);
    (0..2000).for_each(|k| {
        // Segments of 1 to 2^20 units.
        let n = rng.range(2, 8);
        let mut p: Point = (
            rng.range(-(1 << 24), 1 << 24) as i32,
            rng.range(-(1 << 24), 1 << 24) as i32,
        );
        let points: Vec<Point> = (0..n)
            .map(|i| {
                if i > 0 {
                    let span = 1i64 << rng.range(0, 21);
                    p = (
                        (i64::from(p.0) + rng.range(-span, span + 1)) as i32,
                        (i64::from(p.1) + rng.range(-span, span + 1)) as i32,
                    );
                }
                p
            })
            .collect();
        // A dash array of 1 to 4 elements, from 0 to 4 pixels, with a
        // period of at least 1/16 pixel.
        let dash: Vec<i64> = loop {
            let m = rng.range(1, 5);
            let dash: Vec<i64> = (0..m)
                .map(|_| {
                    if rng.range(0, 4) == 0 {
                        0
                    } else {
                        rng.range(0, 4 * PX + 1)
                    }
                })
                .collect();
            let doubled = if m % 2 == 1 { 2 } else { 1 };
            if dash.iter().sum::<i64>() * doubled >= 16 * DASH_UNIT {
                break dash;
            }
        };
        let doubled = if dash.len() % 2 == 1 { 2 } else { 1 };
        let period = dash.iter().sum::<i64>() * doubled;
        let phase = rng.range(0, period);
        assert_dashes(&points, k % 3 == 0, &dash, phase);
    });
}

// --- the work off the canvas

/// The coverage of a stroke of the path of `cmds` with the frame of the
/// margin of the stroke, against the stroke without a frame. A solid
/// stroke is flattened within the frame; a dashed one is flattened whole,
/// as the renderer does (docs/adr/0025), and only its far segments are
/// skipped.
fn assert_pruned_stroke(cmds: &[Cmd], style: &Stroke, w: usize, h: usize, scratch: &mut Scratch) {
    let frame = Frame::around(w, h, 16 * style.half + 16);
    let path = if style.dash.is_empty() {
        build_within(cmds, &frame)
    } else {
        build(cmds)
    };
    let pruned = stroke_styled(&path, style, &frame);
    let whole = stroke_styled(&build(cmds), style, &Frame::NONE);
    let expected = coverage(&whole, FillRule::NonZero, w, h);
    assert_eq!(
        coverage_with(&pruned, FillRule::NonZero, w, h, scratch),
        expected.as_slice(),
        "{w}x{h} {style:?} {cmds:?}"
    );
}

/// Random commands whose points leave room for the margin of `half`.
fn cmds_within_margin(rng: &mut Rng, half: i64, w: usize, h: usize) -> Vec<Cmd> {
    let limit = HI - (16 * half + 16);
    random_cmds_from(rng, true, |rng| {
        let (x, y) = mixed_point(rng, w, h);
        (
            i64::from(x).clamp(-limit, limit) as i32,
            i64::from(y).clamp(-limit, limit) as i32,
        )
    })
}

#[test]
fn solid_strokes_of_paths_off_the_canvas_equal_the_whole() {
    // With butt and with round caps, the curves beyond the margin of the
    // stroke are one segment each, and the pixels are the same.
    let mut rng = Rng(0x0025_0005);
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
                let cmds = cmds_within_margin(&mut rng, half, w, h);
                assert_pruned_stroke(&cmds, &solid(half, false), w, h, &mut scratch);
                assert_pruned_stroke(&cmds, &solid(half, true), w, h, &mut scratch);
            });
        });
}

#[test]
fn dashed_strokes_of_paths_off_the_canvas_equal_the_whole() {
    // The segments beyond the margin only move along the dash array, and
    // make no polygon: the pixels are those of the dashes of every
    // segment. Dashes of 8 to 64 pixels, with and without round caps.
    let mut rng = Rng(0x0025_0006);
    [(40, 30, 40), (300, 300, 10), (1, 1, 20)]
        .iter()
        .for_each(|&(w, h, n)| {
            let mut scratch = Scratch::default();
            (0..n).for_each(|k| {
                let half = [1, 64, 128, 1 << 12][k % 4];
                let cmds = cmds_within_margin(&mut rng, half, w, h);
                let m = rng.range(1, 4);
                let dash: Vec<i64> = (0..m)
                    .map(|i| {
                        if i > 0 && rng.range(0, 3) == 0 {
                            0
                        } else {
                            rng.range(8 * PX, 64 * PX + 1)
                        }
                    })
                    .collect();
                let doubled = if m % 2 == 1 { 2 } else { 1 };
                let phase = rng.range(0, dash.iter().sum::<i64>() * doubled);
                let style = styled(half, k % 2 == 0, &dash, phase);
                assert_pruned_stroke(&cmds, &style, w, h, &mut scratch);
            });
        });
}

#[test]
fn far_segments_only_move_along_the_dashes() {
    // A segment above the canvas, beyond the margin, and a line into the
    // canvas: the dash starts where the far segment left it, and the far
    // segment adds no polygon.
    let (w, h) = (40, 30);
    let half = 128;
    let frame = Frame::around(w, h, 16 * half + 16);
    let mut path = Path::new();
    path.move_to((-10 * ONE, -100 * ONE));
    path.line_to((13 * ONE, -100 * ONE));
    path.line_to((13 * ONE, 20 * ONE));
    let dash = [5 * PX, 5 * PX];
    let style = styled(half, true, &dash, 0);
    let pruned = stroke_styled(&path, &style, &frame);
    let whole = stroke_styled(&path, &style, &Frame::NONE);
    assert!(pruned.subpaths().count() < whole.subpaths().count());
    assert_eq!(
        coverage(&pruned, FillRule::NonZero, w, h),
        coverage(&whole, FillRule::NonZero, w, h)
    );
}

#[test]
fn a_dot_at_the_end_of_an_open_polyline_is_not_drawn() {
    // docs/adr/0026: [0 12] on an open line of 36 pixels makes dots at 0,
    // 12 and 24, and none at its end, as pdftoppm does.
    let line = [(0, 0), (36 * ONE, 0)];
    let dash = [0, 12 * PX];
    let dots = dashes_of(&line, false, &dash, 0);
    assert_eq!(
        dots.iter().map(|d| d[0].0).collect::<Vec<_>>(),
        vec![0, 12 * ONE, 24 * ONE]
    );
    // The dots at the vertices of an open polyline are drawn; the one at
    // its end is not.
    let corner = [(0, 0), (12 * ONE, 0), (12 * ONE, 12 * ONE)];
    let dots = dashes_of(&corner, false, &dash, 0);
    assert_eq!(
        dots.iter().map(|d| d[0]).collect::<Vec<_>>(),
        vec![(0, 0), (12 * ONE, 0)]
    );
    // Closed, the dots are those of the four corners: none is drawn again
    // where the square comes back to its start.
    let square = [(0, 0), (12 * ONE, 0), (12 * ONE, 12 * ONE), (0, 12 * ONE)];
    let dots = dashes_of(&square, true, &dash, 0);
    assert_eq!(
        dots.iter().map(|d| d[0]).collect::<Vec<_>>(),
        square.to_vec()
    );
}

// --- the strokes of user space, in i64 (docs/adr/0027)

/// The outline of a path of user space whose points are those of a device
/// path times 2^16, with the style scaled the same, covers the pixels that
/// the device outline covers, within the rounding of the finer units. With
/// 2^16, the lengths of the dashes are the same integers in both spaces,
/// so the dashes are cut the same.
#[test]
fn the_stroke_in_i64_is_the_stroke_in_i32_scaled() {
    const K: u32 = 16;
    let mut rng = Rng(0x0027_0001);
    let mut polygons = 0;
    (0..200).for_each(|k| {
        let dashed = k % 5 != 0;
        let cmds = random_cmds_from(&mut rng, false, |rng| rng.point(-(1 << 14), 1 << 14));
        let path = build(&cmds);
        let mut user = PathOf::<i64>::new();
        path.subpaths().for_each(|s| {
            s.points.iter().enumerate().for_each(|(i, &(x, y))| {
                let p = (i64::from(x) << K, i64::from(y) << K);
                if i == 0 {
                    user.move_to(p);
                } else {
                    user.line_to(p);
                }
            });
            if s.closed {
                user.close();
            }
        });
        let half = rng.range(1, 1 << 12);
        let cap = [Cap::Butt, Cap::Round, Cap::Square][k % 3];
        let join = [MITER_10, Join::Round, Join::Bevel, Join::Miter(3 << 32)][k % 4];
        let dash: Vec<i64> = match k % 5 {
            0 => vec![],
            1 => vec![3 * PX, 2 * PX],
            _ => vec![
                rng.range(1, 8) * PX,
                rng.range(0, 8) * PX,
                rng.range(1, 8) * PX,
            ],
        };
        let device = Stroke {
            half,
            cap,
            join,
            dash: &dash,
            phase: 0,
            flatness: FLATNESS,
        };
        // The lengths of user space are in its units; those of device
        // space in 2^−16 device units: the same integers with K = 16.
        let user_dash: Vec<i64> = dash.iter().map(|&d| d << (K - 16)).collect();
        let user_style = Stroke {
            half: half << K,
            cap,
            join,
            dash: &user_dash,
            phase: 0,
            flatness: FLATNESS << K,
        };
        let expected = stroke_styled(&path, &device, &Frame::NONE);
        let mut out = PathOf::<i64>::new();
        stroke_styled_into(
            &user,
            &user_style,
            &Frame::NONE,
            &mut StrokeScratchOf::default(),
            &mut out,
        );
        polygons += out.subpaths().count();
        if dashed {
            // The dashes are cut at the same points, within the rounding
            // of the finer units. Their outlines are not compared: a dash
            // that ends within a unit of a vertex makes a segment of a
            // unit in one space and none in the other, and the join with
            // it can reach far.
            path.subpaths().for_each(|s| {
                let mut piece = Vec::new();
                let mut on_device: Vec<Vec<Point>> = Vec::new();
                for_each_dash(
                    s.points,
                    s.closed,
                    &device,
                    &Frame::NONE,
                    &mut piece,
                    |p, _| on_device.push(p.to_vec()),
                );
                let points: Vec<(i64, i64)> = s
                    .points
                    .iter()
                    .map(|&(x, y)| (i64::from(x) << K, i64::from(y) << K))
                    .collect();
                let mut piece = Vec::new();
                let mut in_user: Vec<Vec<(i64, i64)>> = Vec::new();
                for_each_dash(
                    &points,
                    s.closed,
                    &user_style,
                    &Frame::NONE,
                    &mut piece,
                    |p, _| in_user.push(p.to_vec()),
                );
                assert_eq!(on_device.len(), in_user.len(), "{cmds:?} {device:?}");
                on_device.iter().zip(&in_user).for_each(|(d, u)| {
                    assert_eq!(d.len(), u.len(), "{cmds:?} {device:?}");
                    d.iter().zip(u).for_each(|(&(dx, dy), &(ux, uy))| {
                        let (ux, uy) = (ux >> K, uy >> K);
                        assert!(
                            (ux - i64::from(dx)).abs() <= 1 && (uy - i64::from(dy)).abs() <= 1,
                            "{cmds:?} {device:?}: ({dx}, {dy}) and ({ux}, {uy})"
                        );
                    });
                });
            });
            return;
        }
        // The polygons are not compared one by one: the control points of
        // the discs round once more in the finer units, which can split a
        // disc differently. The coverages of the two outlines on a canvas
        // around the points, the user outline rounded back to device
        // units, agree within the rounding of the edges.
        let (w, h) = (160, 160);
        let shift = 80 * ONE64;
        let mut device_outline = Path::new();
        expected.subpaths().for_each(|s| {
            s.points.iter().enumerate().for_each(|(i, &(x, y))| {
                let p = ((i64::from(x) + shift) as i32, (i64::from(y) + shift) as i32);
                if i == 0 {
                    device_outline.move_to(p);
                } else {
                    device_outline.line_to(p);
                }
            });
            device_outline.close();
        });
        let mut user_outline = Path::new();
        out.subpaths().for_each(|s| {
            s.points.iter().enumerate().for_each(|(i, &(x, y))| {
                let round = |v: i64| ((v + (1 << (K - 1))) >> K) + shift;
                let p = (round(x) as i32, round(y) as i32);
                if i == 0 {
                    user_outline.move_to(p);
                } else {
                    user_outline.line_to(p);
                }
            });
            user_outline.close();
        });
        let mut grid = vec![0u8; w * h];
        coverage(&device_outline, FillRule::NonZero, w, h)
            .iter()
            .for_each(|&(x, y, a)| grid[y * w + x] = a);
        let mut got = vec![0u8; w * h];
        coverage(&user_outline, FillRule::NonZero, w, h)
            .iter()
            .for_each(|&(x, y, a)| got[y * w + x] = a);
        // Where discs overlap, the accumulated coverage of an edge pixel
        // adds the difference of each, so a pixel may differ by more than
        // the rounding of one edge; over the canvas, the difference is
        // below 1 part in 200 of the coverage.
        let mut total = 0i64;
        grid.iter().zip(&got).enumerate().for_each(|(i, (&e, &g))| {
            let d = (i32::from(e) - i32::from(g)).abs();
            total += i64::from(d);
            assert!(
                d <= 32,
                "{cmds:?} {device:?}: pixel ({}, {}) is {e} and {g}",
                i % w,
                i / w,
            );
        });
        let covered: i64 = grid.iter().map(|&v| i64::from(v)).sum();
        assert!(
            total * 200 <= covered.max(1),
            "{cmds:?} {device:?}: {total} of {covered}"
        );
    });
    assert!(polygons > 2000, "{polygons}");
}

#[test]
fn a_round_join_is_the_disc_at_the_vertex() {
    // docs/adr/0027: the polygons of an L with round joins are the two
    // rectangles and the disc of the round caps moved to the vertex.
    let mut path = Path::new();
    path.move_to((10 * ONE, 10 * ONE));
    path.line_to((40 * ONE, 10 * ONE));
    path.line_to((40 * ONE, 50 * ONE));
    let style = Stroke {
        half: 300,
        cap: Cap::Butt,
        join: Join::Round,
        dash: &[],
        phase: 0,
        flatness: FLATNESS,
    };
    let out = stroke_styled(&path, &style, &Frame::NONE);
    let polygons: Vec<Vec<Point>> = out.subpaths().map(|s| s.points.to_vec()).collect();
    assert_eq!(polygons.len(), 3);
    assert_eq!(polygons[0].len(), 4);
    assert_eq!(polygons[1].len(), 4);
    let moved: Vec<Point> = disc(300)
        .iter()
        .map(|&(x, y)| (x + 40 * ONE, y + 10 * ONE))
        .collect();
    assert_eq!(polygons[2], moved);
    // A bevel join is the triangle, and a miter join the quadrilateral.
    let bevel = stroke_styled(
        &path,
        &Stroke {
            join: Join::Bevel,
            ..style
        },
        &Frame::NONE,
    );
    assert_eq!(
        bevel.subpaths().map(|s| s.points.len()).collect::<Vec<_>>(),
        [4, 4, 3]
    );
    let miter = stroke_styled(
        &path,
        &Stroke {
            join: MITER_10,
            ..style
        },
        &Frame::NONE,
    );
    assert_eq!(
        miter.subpaths().map(|s| s.points.len()).collect::<Vec<_>>(),
        [4, 4, 4]
    );
}

#[test]
fn a_user_half_width_too_large_for_the_products_still_miters() {
    // A half width of 2^40 in user space (256 points of width) with
    // directions of 2^40: the miter point is computed with the low bits
    // dropped, and lands at the corner of the right angle.
    let m = miter((1 << 40, 0), (0, 1 << 40), 1 << 40, 10 << 32).unwrap();
    assert!(
        (m.0.abs() - (1 << 40)).abs() <= 2 && (m.1.abs() - (1 << 40)).abs() <= 2,
        "{m:?}"
    );
    // And so does the join of a right angle in user space, in units of
    // 2^−32: the corner is within 2 units of the vertex plus half.
    let mut path = PathOf::<i64>::new();
    path.move_to((0, 0));
    path.line_to((1 << 40, 0));
    path.line_to((1 << 40, 1 << 40));
    let mut out = PathOf::<i64>::new();
    stroke_styled_into(
        &path,
        &Stroke {
            half: 1 << 36,
            cap: Cap::Butt,
            join: MITER_10,
            dash: &[],
            phase: 0,
            flatness: FLATNESS << 20,
        },
        &Frame::NONE,
        &mut StrokeScratchOf::default(),
        &mut out,
    );
    let corner = ((1i64 << 40) + (1 << 36), -(1i64 << 36));
    assert!(
        out.points()
            .iter()
            .any(|&(x, y)| (x - corner.0).abs() <= 2 && (y - corner.1).abs() <= 2),
        "{:?}",
        out.points()
    );
}

#[test]
fn the_period_of_an_odd_dash_array_is_doubled() {
    // docs/adr/0026: the period of [3] is 6, so the phase 7 is the phase 1.
    let line = [(0, 0), (20 * ONE, 0)];
    assert_eq!(
        dashes_of(&line, false, &[3 * PX], 7 * PX),
        dashes_of(&line, false, &[3 * PX], PX)
    );
    assert_ne!(
        dashes_of(&line, false, &[3 * PX], 4 * PX),
        dashes_of(&line, false, &[3 * PX], PX)
    );
}
