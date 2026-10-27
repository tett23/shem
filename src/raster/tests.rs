use super::*;

const PX: i32 = 256;

fn square(path: &mut Path, x0: i32, y0: i32, x1: i32, y1: i32) {
    path.move_to((x0, y0));
    path.line_to((x1, y0));
    path.line_to((x1, y1));
    path.line_to((x0, y1));
    path.close();
}

fn alpha_map(path: &Path, rule: FillRule, w: usize, h: usize) -> Vec<u8> {
    let mut map = vec![0u8; w * h];
    coverage(path, rule, w, h)
        .into_iter()
        .for_each(|(x, y, a)| map[y * w + x] = a);
    map
}

// --- flattening

#[test]
fn a_straight_curve_is_one_segment() {
    let points = flatten_curve((0, 0), (PX, PX), (2 * PX, 2 * PX), (3 * PX, 3 * PX));
    assert_eq!(points, vec![(3 * PX, 3 * PX)]);
}

/// The point of the Bézier curve at t = k / n, exactly, as a rational with
/// denominator n^3.
fn bezier_exact(p: [(i64, i64); 4], k: i64, n: i64) -> (i128, i128) {
    let (t, s) = (i128::from(k), i128::from(n - k));
    let w = [s * s * s, 3 * s * s * t, 3 * s * t * t, t * t * t];
    let x = (0..4).map(|i| w[i] * i128::from(p[i].0)).sum();
    let y = (0..4).map(|i| w[i] * i128::from(p[i].1)).sum();
    (x, y)
}

/// The distance from a point to a segment, in units, squared, for a point
/// given with a denominator.
fn dist2_to_polyline(px: exact::Q, py: exact::Q, line: &[(i32, i32)]) -> i128 {
    line.windows(2)
        .map(|w| exact::dist2_point_segment(px, py, w[0], w[1]))
        .min()
        .unwrap()
}

/// Exact rational helpers (no floating-point numbers in this crate).
mod exact {
    #[derive(Clone, Copy)]
    pub struct Q {
        pub num: i128,
        pub den: i128,
    }

    /// The squared distance, rounded down to an integer, from (px, py) to the segment a-b.
    pub fn dist2_point_segment(px: Q, py: Q, a: (i32, i32), b: (i32, i32)) -> i128 {
        let d = px.den;
        let (ax, ay) = (i128::from(a.0) * d, i128::from(a.1) * d);
        let (bx, by) = (i128::from(b.0) * d, i128::from(b.1) * d);
        let (vx, vy) = (bx - ax, by - ay);
        let (wx, wy) = (px.num - ax, py.num - ay);
        let len2 = vx * vx + vy * vy;
        let dot = vx * wx + vy * wy;
        let (cx, cy) = if len2 == 0 || dot <= 0 {
            (ax, ay)
        } else if dot >= len2 {
            (bx, by)
        } else {
            (ax + vx * dot / len2, ay + vy * dot / len2)
        };
        let (ex, ey) = (px.num - cx, py.num - cy);
        (ex * ex + ey * ey) / (d * d)
    }
}

#[test]
fn a_quarter_circle_stays_within_the_tolerance() {
    // A quarter circle of radius 100 pixels from (100, 0) to (0, 100)
    let r = 100 * PX;
    let k = r * 5523 / 10000;
    let p = [(r, 0), (r, k), (k, r), (0, r)];
    let mut line = vec![p[0]];
    line.extend(flatten_curve(p[0], p[1], p[2], p[3]));
    let p64 = p.map(|(x, y)| (i64::from(x), i64::from(y)));
    (0..=64).for_each(|k| {
        let (x, y) = bezier_exact(p64, k, 64);
        let den = 64i128 * 64 * 64;
        let d2 = dist2_to_polyline(exact::Q { num: x, den }, exact::Q { num: y, den }, &line);
        // 1/16 pixel is 16 units; the rounding of the midpoints adds a few units
        assert!(d2 <= 20 * 20, "t = {k}/64: squared distance {d2}");
    });
}

#[test]
fn a_degenerate_curve_stops() {
    let points = flatten_curve((5, 5), (5, 5), (5, 5), (5, 5));
    assert_eq!(points, vec![(5, 5)]);
    // a cusp with far control points still ends at depth 16
    let points = flatten_curve((0, 0), (1 << 20, -(1 << 20)), (-(1 << 20), 1 << 20), (0, 0));
    assert!(points.len() <= 1 << 16);
    assert_eq!(*points.last().unwrap(), (0, 0));
}

// --- filling

#[test]
fn an_aligned_square_covers_whole_pixels() {
    let mut path = Path::new();
    square(&mut path, 2 * PX, 3 * PX, 12 * PX, 13 * PX);
    let map = alpha_map(&path, FillRule::NonZero, 16, 16);
    (0..16).for_each(|y| {
        (0..16).for_each(|x| {
            let inside = (2..12).contains(&x) && (3..13).contains(&y);
            assert_eq!(map[y * 16 + x], if inside { 255 } else { 0 }, "({x}, {y})");
        })
    });
}

#[test]
fn a_square_shifted_by_half_a_pixel() {
    let mut path = Path::new();
    square(
        &mut path,
        2 * PX + PX / 2,
        2 * PX + PX / 2,
        6 * PX + PX / 2,
        6 * PX + PX / 2,
    );
    let map = alpha_map(&path, FillRule::NonZero, 8, 8);
    let at = |x: usize, y: usize| map[y * 8 + x];
    assert_eq!(at(4, 4), 255);
    assert_eq!(at(2, 4), 128);
    assert_eq!(at(6, 4), 128);
    assert_eq!(at(4, 2), 128);
    assert_eq!(at(2, 2), 64);
    assert_eq!(at(6, 6), 64);
    assert_eq!(at(1, 4), 0);
    assert_eq!(at(7, 4), 0);
}

#[test]
fn nonzero_and_even_odd() {
    let mut same = Path::new();
    square(&mut same, 0, 0, 8 * PX, 8 * PX);
    square(&mut same, 2 * PX, 2 * PX, 6 * PX, 6 * PX);
    let mut opposite = Path::new();
    square(&mut opposite, 0, 0, 8 * PX, 8 * PX);
    opposite.move_to((2 * PX, 2 * PX));
    opposite.line_to((2 * PX, 6 * PX));
    opposite.line_to((6 * PX, 6 * PX));
    opposite.line_to((6 * PX, 2 * PX));
    opposite.close();
    let center = 4 * 8 + 4;
    assert_eq!(alpha_map(&same, FillRule::NonZero, 8, 8)[center], 255);
    assert_eq!(alpha_map(&opposite, FillRule::NonZero, 8, 8)[center], 0);
    assert_eq!(alpha_map(&same, FillRule::EvenOdd, 8, 8)[center], 0);
    assert_eq!(alpha_map(&same, FillRule::EvenOdd, 8, 8)[8 + 1], 255);
}

#[test]
fn an_unclosed_subpath_is_filled_as_closed() {
    let mut path = Path::new();
    path.move_to((0, 0));
    path.line_to((4 * PX, 0));
    path.line_to((4 * PX, 4 * PX));
    path.line_to((0, 4 * PX));
    assert_eq!(alpha_map(&path, FillRule::NonZero, 4, 4), vec![255; 16]);
}

#[test]
fn paths_outside_the_canvas_are_clipped() {
    let mut path = Path::new();
    square(&mut path, -10 * PX, -10 * PX, 2 * PX, 2 * PX);
    square(&mut path, 6 * PX, 6 * PX, 100 * PX, 100 * PX);
    let map = alpha_map(&path, FillRule::NonZero, 8, 8);
    assert_eq!(map[0], 255);
    assert_eq!(map[8 + 1], 255);
    assert_eq!(map[3 * 8 + 3], 0);
    assert_eq!(map[7 * 8 + 7], 255);
}

#[test]
fn compositing() {
    assert_eq!(composite(200, 255, 10), 200);
    assert_eq!(composite(200, 0, 10), 10);
    // (200 * 128 + 10 * 127 + 127) / 255 = 105
    assert_eq!(composite(200, 128, 10), 105);
    let mut canvas = Canvas::new(2, 1);
    assert_eq!(canvas.pixel(0, 0), [255, 255, 255]);
    canvas.paint(&[(1, 0, 255)], [0, 0, 0]);
    assert_eq!(canvas.pixel(0, 0), [255, 255, 255]);
    assert_eq!(canvas.pixel(1, 0), [0, 0, 0]);
}

// --- stroking

fn stroked(path: &Path, half: i64, w: usize, h: usize) -> Vec<u8> {
    alpha_map(&stroke(path, half), FillRule::NonZero, w, h)
}

#[test]
fn a_horizontal_line_two_pixels_wide() {
    let mut path = Path::new();
    path.move_to((2 * PX, 5 * PX));
    path.line_to((8 * PX, 5 * PX));
    let map = stroked(&path, i64::from(PX), 10, 10);
    (0..10).for_each(|y| {
        (0..10).for_each(|x| {
            let inside = (2..8).contains(&x) && (4..6).contains(&y);
            assert_eq!(map[y * 10 + x], if inside { 255 } else { 0 }, "({x}, {y})");
        })
    });
}

#[test]
fn a_right_angle_has_a_miter() {
    // An L from (2, 8) up to (2, 2) and right to (8, 2), 2 pixels wide
    let mut path = Path::new();
    path.move_to((2 * PX, 8 * PX));
    path.line_to((2 * PX, 2 * PX));
    path.line_to((8 * PX, 2 * PX));
    let map = stroked(&path, i64::from(PX), 10, 10);
    // the outer corner pixel (1, 1) is filled by the miter
    assert_eq!(map[10 + 1], 255);
}

#[test]
fn a_sharp_angle_has_a_bevel() {
    // A spike: the angle is about 4 degrees, so the miter ratio exceeds 10
    let mut path = Path::new();
    path.move_to((0, 20 * PX));
    path.line_to((40 * PX, 21 * PX + PX / 2));
    path.line_to((0, 23 * PX));
    let map = stroked(&path, i64::from(PX), 64, 40);
    // no miter tip far beyond the vertex at x = 40
    (44..64).for_each(|x| (15..30).for_each(|y| assert_eq!(map[y * 64 + x], 0, "({x}, {y})")));
}

#[test]
fn a_closed_square_has_the_same_corner_at_its_start() {
    let mut path = Path::new();
    square(&mut path, 4 * PX, 4 * PX, 12 * PX, 12 * PX);
    let map = stroked(&path, i64::from(PX), 16, 16);
    let at = |x: usize, y: usize| map[y * 16 + x];
    // the start corner (4, 4) and the opposite corner (12, 12) are both mitered
    assert_eq!(at(3, 3), 255);
    assert_eq!(at(12, 12), 255);
    assert_eq!(at(12, 3), 255);
    assert_eq!(at(3, 12), 255);
    // the inside is empty
    assert_eq!(at(8, 8), 0);
}

#[test]
fn degenerate_segments_are_dropped() {
    let mut path = Path::new();
    path.move_to((PX, PX));
    path.line_to((PX, PX));
    assert!(stroke(&path, i64::from(PX)).subpaths().next().is_none());
}

#[test]
fn polygons_of_a_stroke_are_positive() {
    let mut path = Path::new();
    path.move_to((0, 0));
    path.line_to((8 * PX, 0));
    path.line_to((8 * PX, 8 * PX));
    stroke(&path, 64)
        .subpaths()
        .for_each(|s| assert!(signed_area(s.points) > 0));
}

// --- the cells in a Vec (docs/adr/0011)

/// The coverage before docs/adr/0011: the cells in a BTreeMap.
fn reference_coverage(path: &Path, rule: FillRule, w: usize, h: usize) -> Vec<(usize, usize, u8)> {
    let mut map: std::collections::BTreeMap<(i32, i32), (i64, i64)> =
        std::collections::BTreeMap::new();
    let mut add = |row: i32, col: i32, cover: i64, area: i64| {
        let e = map.entry((row, col)).or_insert((0, 0));
        e.0 += cover;
        e.1 += area;
    };
    path.subpaths()
        .filter(|s| s.points.len() > 1)
        .for_each(|s| {
            s.points
                .windows(2)
                .for_each(|w| render_line(&mut add, w[0], w[1]));
            render_line(&mut add, s.points[s.points.len() - 1], s.points[0]);
        });
    let cells: Vec<Cell> = map
        .into_iter()
        .map(|((r, c), (cover, area))| (r, c, cover, area))
        .collect();
    sweep(&cells, rule, w, h)
}

#[test]
fn coverage_is_as_before() {
    let mut x: u64 = 77;
    let mut next = |n: i32| {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((x >> 33) % n as u64) as i32
    };
    (0..200).for_each(|k| {
        let mut path = Path::new();
        (0..1 + next(3)).for_each(|_| {
            path.move_to((next(64 * PX) - 8 * PX, next(48 * PX) - 8 * PX));
            (0..2 + next(10))
                .for_each(|_| path.line_to((next(64 * PX) - 8 * PX, next(48 * PX) - 8 * PX)));
            if next(2) == 0 {
                path.close();
            }
        });
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            assert_eq!(
                coverage(&path, rule, 48, 32),
                reference_coverage(&path, rule, 48, 32),
                "path {k}"
            );
        }
    });
}
