//! The rasterizer of v0.1.0-rc2, before the changes of docs/adr/0013, kept
//! as the reference that the new one must equal.

#![allow(dead_code)]

use crate::fixed::{div_round, isqrt};

/// A point in units of 1/256 pixel.
pub type Point = (i32, i32);

/// Subpixels per pixel.
pub const ONE: i32 = 256;
const ONE64: i64 = ONE as i64;

/// The coverage of a whole pixel: the area of a cell, doubled.
const FULL: i64 = ONE64 * ONE64 * 2;

/// A curve is split until its control points are within 1/16 pixel.
const FLATNESS: i64 = ONE64 / 16;
const MAX_DEPTH: u32 = 16;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Subpath {
    pub points: Vec<Point>,
    pub closed: bool,
}

/// A path of line segments in device space.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Path {
    subpaths: Vec<Subpath>,
}

impl Path {
    pub fn new() -> Path {
        Path::default()
    }

    pub fn move_to(&mut self, p: Point) {
        self.subpaths.push(Subpath {
            points: vec![p],
            closed: false,
        });
    }

    /// The current point: the last point, or the start of a closed subpath.
    pub fn current_point(&self) -> Option<Point> {
        self.subpaths.last().and_then(|s| {
            if s.closed {
                s.points.first().copied()
            } else {
                s.points.last().copied()
            }
        })
    }

    pub fn line_to(&mut self, p: Point) {
        match self.subpaths.last_mut() {
            Some(s) if !s.closed => s.points.push(p),
            _ => {
                // After h, a segment starts a new subpath at the start of the closed one.
                let start = self.current_point().unwrap_or(p);
                self.subpaths.push(Subpath {
                    points: vec![start, p],
                    closed: false,
                });
            }
        }
    }

    pub fn curve_to(&mut self, c1: Point, c2: Point, p: Point) {
        let start = self.current_point().unwrap_or(c1);
        flatten_curve(start, c1, c2, p)
            .into_iter()
            .for_each(|q| self.line_to(q));
    }

    pub fn close(&mut self) {
        if let Some(s) = self.subpaths.last_mut() {
            s.closed = true;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.subpaths.is_empty()
    }

    pub fn subpaths(&self) -> &[Subpath] {
        &self.subpaths
    }

    pub fn push(&mut self, subpath: Subpath) {
        self.subpaths.push(subpath);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

fn mid(a: Point, b: Point) -> Point {
    (
        ((i64::from(a.0) + i64::from(b.0)) >> 1) as i32,
        ((i64::from(a.1) + i64::from(b.1)) >> 1) as i32,
    )
}

/// The flatness of a curve: the larger second difference of its control points.
fn flatness(p0: Point, p1: Point, p2: Point, p3: Point) -> i64 {
    let second = |a: Point, b: Point, c: Point| {
        let dx = i64::from(a.0) - 2 * i64::from(b.0) + i64::from(c.0);
        let dy = i64::from(a.1) - 2 * i64::from(b.1) + i64::from(c.1);
        dx.abs().max(dy.abs())
    };
    second(p0, p1, p2).max(second(p1, p2, p3))
}

/// The points that approximate a cubic Bézier curve, without `p0` and with
/// `p3`, by de Casteljau's subdivision at the midpoint.
pub fn flatten_curve(p0: Point, p1: Point, p2: Point, p3: Point) -> Vec<Point> {
    let mut out = Vec::new();
    // A stack of curves still to split, the first curve on top.
    let mut stack = vec![([p0, p1, p2, p3], 0u32)];
    while let Some(([a, b, c, d], depth)) = stack.pop() {
        if depth >= MAX_DEPTH || flatness(a, b, c, d) <= FLATNESS {
            out.push(d);
            continue;
        }
        let ab = mid(a, b);
        let bc = mid(b, c);
        let cd = mid(c, d);
        let abc = mid(ab, bc);
        let bcd = mid(bc, cd);
        let m = mid(abc, bcd);
        stack.push(([m, bcd, cd, d], depth + 1));
        stack.push(([a, ab, abc, m], depth + 1));
    }
    out
}

/// A cell: (row, column, cover, area).
pub(crate) type Cell = (i32, i32, i64, i64);

/// The cells sorted by row and column, with the values of the same cell
/// added. The sums of integers do not depend on the order of the additions,
/// so this gives the cells of a map from (row, column) (docs/adr/0011).
fn merge(mut cells: Vec<Cell>) -> Vec<Cell> {
    cells.sort_unstable_by_key(|&(row, col, _, _)| (row, col));
    cells
        .into_iter()
        .fold(Vec::new(), |mut out: Vec<Cell>, (row, col, cover, area)| {
            match out.last_mut() {
                Some(last) if (last.0, last.1) == (row, col) => {
                    last.2 += cover;
                    last.3 += area;
                }
                _ => out.push((row, col, cover, area)),
            }
            out
        })
}

/// The points where a segment crosses the multiples of ONE of one
/// coordinate, from `a` to `b`. `y_of` interpolates the other coordinate.
fn crossings(a: i64, b: i64) -> Vec<i64> {
    let (lo, hi) = (a.min(b), a.max(b));
    let first = lo.div_euclid(ONE64) + 1;
    let last = (hi - 1).div_euclid(ONE64);
    let ks: Vec<i64> = (first..=last).map(|k| k * ONE64).collect();
    if a <= b {
        ks
    } else {
        ks.into_iter().rev().collect()
    }
}

fn interpolate(a0: i64, a1: i64, b0: i64, b1: i64, at: i64) -> i64 {
    // b at the point where a is `at`, on the line from (a0, b0) to (a1, b1).
    b0 + div_round(
        i128::from(b1 - b0) * i128::from(at - a0),
        i128::from(a1 - a0),
    ) as i64
}

/// Adds a line segment to the cells through `add(row, column, cover, area)`.
pub(crate) fn render_line(add: &mut impl FnMut(i32, i32, i64, i64), p0: Point, p1: Point) {
    let (x0, y0, x1, y1) = (
        i64::from(p0.0),
        i64::from(p0.1),
        i64::from(p1.0),
        i64::from(p1.1),
    );
    if y0 == y1 {
        return;
    }
    // Split at the rows, with y exact and x interpolated.
    let mut row_points = vec![(x0, y0)];
    row_points.extend(
        crossings(y0, y1)
            .into_iter()
            .map(|y| (interpolate(y0, y1, x0, x1, y), y)),
    );
    row_points.push((x1, y1));
    row_points.windows(2).for_each(|w| {
        let ((ax, ay), (bx, by)) = (w[0], w[1]);
        let row = ay.min(by).div_euclid(ONE64);
        let (ylo, yhi) = (ay.min(by), ay.max(by));
        // Split at the columns, with x exact and y interpolated within the row.
        let mut points = vec![(ax, ay)];
        if ax != bx {
            points.extend(
                crossings(ax, bx)
                    .into_iter()
                    .map(|x| (x, interpolate(ax, bx, ay, by, x).clamp(ylo, yhi))),
            );
        }
        points.push((bx, by));
        points.windows(2).for_each(|v| {
            let ((cx, cy), (dx, dy)) = (v[0], v[1]);
            let col = cx.min(dx).div_euclid(ONE64);
            let (fx0, fx1) = (cx - col * ONE64, dx - col * ONE64);
            let height = dy - cy;
            add(row as i32, col as i32, height, (fx0 + fx1) * height);
        });
    });
}

/// The opacity of a pixel from its signed coverage.
fn alpha(value: i64, rule: FillRule) -> u8 {
    let v = value.abs();
    let v = match rule {
        FillRule::NonZero => v.min(FULL),
        FillRule::EvenOdd => {
            let v = v % (2 * FULL);
            if v > FULL { 2 * FULL - v } else { v }
        }
    };
    ((v * 255 + FULL / 2) / FULL) as u8
}

/// The pixels a path covers on a `width` × `height` canvas, as (x, y,
/// opacity) in the order of rows and columns, without the empty ones. Every
/// subpath is closed for filling.
pub fn coverage(
    path: &Path,
    rule: FillRule,
    width: usize,
    height: usize,
) -> Vec<(usize, usize, u8)> {
    let mut cells: Vec<Cell> = Vec::new();
    let mut add = |row, col, cover, area| cells.push((row, col, cover, area));
    path.subpaths
        .iter()
        .filter(|s| s.points.len() > 1)
        .for_each(|s| {
            s.points
                .windows(2)
                .for_each(|w| render_line(&mut add, w[0], w[1]));
            render_line(&mut add, s.points[s.points.len() - 1], s.points[0]);
        });
    sweep(&merge(cells), rule, width, height)
}

/// The pixels of the cells, sorted by row and column with one cell each.
pub(crate) fn sweep(
    cells: &[Cell],
    rule: FillRule,
    width: usize,
    height: usize,
) -> Vec<(usize, usize, u8)> {
    let (w, h) = (width as i64, height as i64);
    let mut out = Vec::new();
    let mut row_cells = cells.iter().peekable();
    while let Some(&&(row, _, _, _)) = row_cells.peek() {
        let cells_of_row: Vec<(i32, i64, i64)> = std::iter::from_fn(|| {
            row_cells
                .next_if(|c| c.0 == row)
                .map(|&(_, col, cover, area)| (col, cover, area))
        })
        .collect();
        if !(0..h).contains(&i64::from(row)) {
            continue;
        }
        let mut cover = 0i64;
        cells_of_row
            .iter()
            .enumerate()
            .for_each(|(i, &(col, c, area))| {
                cover += c;
                let col = i64::from(col);
                if (0..w).contains(&col) {
                    let a = alpha(cover * 2 * ONE64 - area, rule);
                    if a > 0 {
                        out.push((col as usize, row as usize, a));
                    }
                }
                let next = cells_of_row.get(i + 1).map_or(w, |&(c, _, _)| i64::from(c));
                let a = alpha(cover * 2 * ONE64, rule);
                if a > 0 {
                    (col + 1).max(0)..next.min(w)
                } else {
                    0..0
                }
                .for_each(|x| out.push((x as usize, row as usize, a)));
            });
    }
    out
}

/// Twice the signed area of a polygon (the shoelace formula).
pub fn signed_area(points: &[Point]) -> i128 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| i128::from(a.0) * i128::from(b.1) - i128::from(b.0) * i128::from(a.1))
        .sum()
}

/// The polygon with a positive signed area.
fn positive(mut points: Vec<Point>) -> Subpath {
    if signed_area(&points) < 0 {
        points.reverse();
    }
    Subpath {
        points,
        closed: true,
    }
}

fn add(p: Point, v: (i64, i64)) -> Point {
    ((i64::from(p.0) + v.0) as i32, (i64::from(p.1) + v.1) as i32)
}

fn neg(v: (i64, i64)) -> (i64, i64) {
    (-v.0, -v.1)
}

/// The left normal of a direction, `half` units long.
fn normal(d: (i64, i64), half: i64) -> (i64, i64) {
    let len = isqrt((i128::from(d.0) * i128::from(d.0) + i128::from(d.1) * i128::from(d.1)) as u128)
        as i128;
    (
        div_round(-i128::from(d.1) * i128::from(half), len) as i64,
        div_round(i128::from(d.0) * i128::from(half), len) as i64,
    )
}

fn direction(a: Point, b: Point) -> (i64, i64) {
    (
        i64::from(b.0) - i64::from(a.0),
        i64::from(b.1) - i64::from(a.1),
    )
}

/// The polygon of the join at `v` between the directions `d1` and `d2`:
/// a miter if it is within the limit of 10, a bevel otherwise.
fn join(v: Point, d1: (i64, i64), d2: (i64, i64), half: i64) -> Option<Subpath> {
    let cross = i128::from(d1.0) * i128::from(d2.1) - i128::from(d1.1) * i128::from(d2.0);
    if cross == 0 {
        return None;
    }
    let sign = if cross > 0 { -1 } else { 1 };
    let n1 = normal(d1, half * sign);
    let n2 = normal(d2, half * sign);
    let a = add(v, n1);
    let b = add(v, n2);
    let dot = i128::from(d1.0) * i128::from(d2.0) + i128::from(d1.1) * i128::from(d2.1);
    let len2 = |d: (i64, i64)| {
        (i128::from(d.0) * i128::from(d.0) + i128::from(d.1) * i128::from(d.1)) as u128
    };
    let lens = isqrt(len2(d1).saturating_mul(len2(d2))) as i128;
    // The miter ratio 1 / sin(phi / 2) is at most 10 if (1 − cos phi) · 10² ≥ 2,
    // where cos phi = −dot / lens.
    let within_limit = 100 * dot + 98 * lens >= 0;
    let det = i128::from(n1.0) * i128::from(n2.1) - i128::from(n1.1) * i128::from(n2.0);
    if within_limit && det != 0 {
        let h2 = i128::from(half) * i128::from(half);
        let m = (
            div_round(h2 * i128::from(n2.1 - n1.1), det) as i64,
            div_round(h2 * i128::from(n1.0 - n2.0), det) as i64,
        );
        Some(positive(vec![v, a, add(v, m), b]))
    } else {
        Some(positive(vec![v, a, b]))
    }
}

/// The outline of the stroke of a path, `half` units on each side, with
/// butt caps and miter joins (limit 10), as positive polygons to fill with
/// the nonzero rule.
pub fn stroke(path: &Path, half: i64) -> Path {
    let polygons = path.subpaths.iter().flat_map(|s| {
        let mut points: Vec<Point> = s.points.clone();
        points.dedup();
        if s.closed && points.len() > 1 && points.first() != points.last() {
            points.push(points[0]);
        }
        let segments: Vec<(Point, Point)> = points
            .windows(2)
            .map(|w| (w[0], w[1]))
            .filter(|(a, b)| a != b)
            .collect();
        let quads = segments.iter().map(move |&(a, b)| {
            let n = normal(direction(a, b), half);
            positive(vec![add(a, n), add(b, n), add(b, neg(n)), add(a, neg(n))])
        });
        let closing =
            (s.closed && segments.len() > 1).then(|| (segments[segments.len() - 1], segments[0]));
        let joins = segments
            .windows(2)
            .map(|w| (w[0], w[1]))
            .chain(closing)
            .filter_map(move |(p, q)| join(p.1, direction(p.0, p.1), direction(q.0, q.1), half));
        quads.chain(joins).collect::<Vec<_>>()
    });
    Path {
        subpaths: polygons.collect(),
    }
}
