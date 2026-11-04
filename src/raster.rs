//! Paths in device space, coverage, stroking and compositing (docs/adr/0003).
//!
//! Coordinates are integers in units of 1/256 pixel. The coverage is
//! accumulated in cells as FreeType's `ftgrays.c` does: each cell holds
//! the signed height of the edges that cross it (cover) and twice the
//! signed area to their left (area), and a row is swept from the left.

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

/// A subpath of a [`Path`]: its points, and whether it is closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subpath<'a> {
    pub points: &'a [Point],
    pub closed: bool,
}

/// A path of line segments in device space: the points of every subpath in
/// one `Vec`, and where each subpath starts, so that a path cleared and
/// built again keeps its buffers (docs/adr/0013).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Path {
    points: Vec<Point>,
    /// The index of the first point of each subpath, and whether it is closed.
    starts: Vec<(usize, bool)>,
}

impl Path {
    pub fn new() -> Path {
        Path::default()
    }

    /// Removes the subpaths and keeps the capacity of the buffers.
    pub fn clear(&mut self) {
        self.points.clear();
        self.starts.clear();
    }

    pub fn move_to(&mut self, p: Point) {
        self.starts.push((self.points.len(), false));
        self.points.push(p);
    }

    /// The current point: the last point, or the start of a closed subpath.
    pub fn current_point(&self) -> Option<Point> {
        self.starts.last().map(|&(start, closed)| {
            if closed {
                self.points[start]
            } else {
                self.points[self.points.len() - 1]
            }
        })
    }

    pub fn line_to(&mut self, p: Point) {
        match self.starts.last() {
            Some(&(_, false)) => self.points.push(p),
            _ => {
                // After h, a segment starts a new subpath at the start of the closed one.
                let start = self.current_point().unwrap_or(p);
                self.move_to(start);
                self.points.push(p);
            }
        }
    }

    pub fn curve_to(&mut self, c1: Point, c2: Point, p: Point) {
        let start = self.current_point().unwrap_or(c1);
        // After the first point, the subpath is open: the others are pushed.
        let mut first = true;
        flatten_curve_into(start, c1, c2, p, |q| {
            if first {
                self.line_to(q);
                first = false;
            } else {
                self.points.push(q);
            }
        });
    }

    pub fn close(&mut self) {
        if let Some(last) = self.starts.last_mut() {
            last.1 = true;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    pub fn subpaths(&self) -> impl Iterator<Item = Subpath<'_>> {
        let ends = self
            .starts
            .iter()
            .skip(1)
            .map(|&(start, _)| start)
            .chain(std::iter::once(self.points.len()));
        self.starts
            .iter()
            .zip(ends)
            .map(|(&(start, closed), end)| Subpath {
                points: &self.points[start..end],
                closed,
            })
    }

    /// Adds a closed polygon, reversed if its signed area is negative.
    fn push_positive(&mut self, polygon: &[Point]) {
        let start = self.points.len();
        self.starts.push((start, true));
        self.points.extend_from_slice(polygon);
        if signed_area(polygon) < 0 {
            self.points[start..].reverse();
        }
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
    flatten_curve_into(p0, p1, p2, p3, |q| out.push(q));
    out
}

/// The points of [`flatten_curve`], given to `emit` in order: a curve that
/// is flat enough, or at the deepest level, gives its end point; another is
/// split at its midpoint, first half first. The recursion is at most
/// MAX_DEPTH deep, and keeps only the curves of the levels it is in
/// (docs/adr/0015).
fn flatten_curve_into(p0: Point, p1: Point, p2: Point, p3: Point, mut emit: impl FnMut(Point)) {
    fn split(a: Point, b: Point, c: Point, d: Point, depth: u32, emit: &mut impl FnMut(Point)) {
        if depth >= MAX_DEPTH || flatness(a, b, c, d) <= FLATNESS {
            emit(d);
            return;
        }
        let ab = mid(a, b);
        let bc = mid(b, c);
        let cd = mid(c, d);
        let abc = mid(ab, bc);
        let bcd = mid(bc, cd);
        let m = mid(abc, bcd);
        split(a, ab, abc, m, depth + 1, emit);
        split(m, bcd, cd, d, depth + 1, emit);
    }
    split(p0, p1, p2, p3, 0, &mut emit);
}

/// A cell: (row, column, cover, area).
pub(crate) type Cell = (i32, i32, i64, i64);

/// Sorts the cells by row and column and adds the values of the same cell
/// into one. The sums of integers do not depend on the order of the
/// additions, so this gives the cells of a map from (row, column)
/// (docs/adr/0011).
fn merge(cells: &mut Vec<Cell>) {
    cells.sort_unstable_by_key(|&(row, col, _, _)| (row, col));
    // dedup_by gives the later cell first, and keeps the earlier one.
    cells.dedup_by(|later, earlier| {
        let same = (later.0, later.1) == (earlier.0, earlier.1);
        if same {
            earlier.2 += later.2;
            earlier.3 += later.3;
        }
        same
    });
}

/// The multiples of ONE strictly between `a` and `b`, from `a` to `b`.
fn crossings(a: i64, b: i64) -> impl Iterator<Item = i64> {
    let (lo, hi) = (a.min(b), a.max(b));
    let first = lo.div_euclid(ONE64) + 1;
    let last = (hi - 1).div_euclid(ONE64);
    let count = (last - first + 1).max(0);
    (0..count).map(move |i| if a <= b { first + i } else { last - i } * ONE64)
}

/// b at the point where a is `at`, on the line from (a0, b0) to (a1, b1).
/// The product fits in i64 for most segments, whose division is faster
/// and gives the same quotient (docs/adr/0015).
fn interpolate(a0: i64, a1: i64, b0: i64, b1: i64, at: i64) -> i64 {
    // With a negative divisor, both are negated; a product that does not
    // fit, or whose negation does not, goes through i128.
    let d = a1 - a0;
    let n = (b1 - b0).checked_mul(at - a0);
    let (n, d) = if d < 0 {
        (n.and_then(i64::checked_neg), -d)
    } else {
        (n, d)
    };
    match n {
        Some(n) => b0 + crate::fixed::div_round_i64(n, d),
        None => {
            b0 + div_round(
                i128::from(b1 - b0) * i128::from(at - a0),
                i128::from(a1 - a0),
            ) as i64
        }
    }
}

/// Adds a line segment to the cells through `add(row, column, cover, area)`.
/// The segment is split at the rows, with y exact and x interpolated, and
/// each piece at the columns, with x exact and y interpolated within the
/// row. This is the innermost loop of filling, so the pieces are walked
/// with plain loops (docs/adr/0014).
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
    let mut last = (x0, y0);
    for y in crossings(y0, y1) {
        let p = (interpolate(y0, y1, x0, x1, y), y);
        render_row_piece(add, last, p);
        last = p;
    }
    render_row_piece(add, last, (x1, y1));
}

/// Adds the piece of a segment within one row.
fn render_row_piece(add: &mut impl FnMut(i32, i32, i64, i64), a: (i64, i64), b: (i64, i64)) {
    let ((ax, ay), (bx, by)) = (a, b);
    let row = ay.min(by).div_euclid(ONE64) as i32;
    let (ylo, yhi) = (ay.min(by), ay.max(by));
    let mut cell = |(cx, cy): (i64, i64), (dx, dy): (i64, i64)| {
        let col = cx.min(dx).div_euclid(ONE64);
        let (fx0, fx1) = (cx - col * ONE64, dx - col * ONE64);
        let height = dy - cy;
        add(row, col as i32, height, (fx0 + fx1) * height);
    };
    let mut last = (ax, ay);
    if ax != bx {
        for x in crossings(ax, bx) {
            let p = (x, interpolate(ax, bx, ay, by, x).clamp(ylo, yhi));
            cell(last, p);
            last = p;
        }
    }
    cell(last, (bx, by));
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

/// The buffers of filling: the cells, the dense grid of a small path, and
/// the pixels, kept between the fills of a page (docs/adr/0013, 0014).
#[derive(Clone, Debug, Default)]
pub struct Scratch {
    cells: Vec<Cell>,
    grid: Vec<(i64, i64)>,
    pixels: Vec<(usize, usize, u8)>,
}

/// The largest number of cells of a dense grid (docs/adr/0014).
const MAX_GRID: i64 = 1 << 16;

/// The pixels a path covers on a `width` × `height` canvas, as (x, y,
/// opacity) in the order of rows and columns, without the empty ones. Every
/// subpath is closed for filling.
pub fn coverage(
    path: &Path,
    rule: FillRule,
    width: usize,
    height: usize,
) -> Vec<(usize, usize, u8)> {
    coverage_with(path, rule, width, height, &mut Scratch::default()).to_vec()
}

/// Calls `f` with the segments of the subpaths, closed for filling.
fn for_each_edge(path: &Path, mut f: impl FnMut(Point, Point)) {
    path.subpaths()
        .filter(|s| s.points.len() > 1)
        .for_each(|s| {
            s.points.windows(2).for_each(|w| f(w[0], w[1]));
            f(s.points[s.points.len() - 1], s.points[0]);
        });
}

/// The rows and the columns of the cells that can change the pixels of a
/// path on a `width` × `height` canvas: rows in the canvas, and columns
/// from −1, where the cells left of the canvas are summed, to the last
/// column of the canvas. `None` if no cell can.
fn grid_bounds(path: &Path, width: usize, height: usize) -> Option<(i64, i64, i64, i64)> {
    let (min, max) = path
        .subpaths()
        .filter(|s| s.points.len() > 1)
        .flat_map(|s| s.points.iter())
        .fold(None, |acc: Option<(Point, Point)>, &(x, y)| {
            Some(match acc {
                None => ((x, y), (x, y)),
                Some((lo, hi)) => ((lo.0.min(x), lo.1.min(y)), (hi.0.max(x), hi.1.max(y))),
            })
        })?;
    let cell = |v: i32| i64::from(v).div_euclid(ONE64);
    let rows = (cell(min.1).max(0), cell(max.1).min(height as i64 - 1));
    let cols = (cell(min.0).max(-1), cell(max.0).min(width as i64 - 1));
    (rows.0 <= rows.1 && cols.0 <= cols.1).then_some((rows.0, rows.1, cols.0, cols.1))
}

/// [`coverage`] in the buffers of `scratch`. The cells of a path whose
/// cells fit in MAX_GRID are summed in a dense grid; the others are sorted
/// and merged. Both give the same pixels: a cell left of the canvas only
/// adds its cover to the row, one right of it or outside the rows changes
/// no pixel, and an empty cell gives the pixel of the span it is in.
pub fn coverage_with<'s>(
    path: &Path,
    rule: FillRule,
    width: usize,
    height: usize,
    scratch: &'s mut Scratch,
) -> &'s [(usize, usize, u8)] {
    let mut pixels = std::mem::take(&mut scratch.pixels);
    pixels.clear();
    visit_coverage(path, rule, width, height, scratch, |x, y, a| {
        pixels.push((x, y, a))
    });
    scratch.pixels = pixels;
    &scratch.pixels
}

/// Calls `visit(x, y, opacity)` with the pixels of [`coverage`], in the same
/// order, without collecting them (docs/adr/0015).
pub fn visit_coverage(
    path: &Path,
    rule: FillRule,
    width: usize,
    height: usize,
    scratch: &mut Scratch,
    mut visit: impl FnMut(usize, usize, u8),
) {
    let Scratch {
        cells,
        grid,
        pixels,
    } = scratch;
    let Some((row0, row1, col0, col1)) = grid_bounds(path, width, height) else {
        return;
    };
    let cols = col1 - col0 + 1;
    if (row1 - row0 + 1) * cols <= MAX_GRID {
        grid.clear();
        grid.resize(((row1 - row0 + 1) * cols) as usize, (0, 0));
        let mut add = |row: i32, col: i32, cover: i64, area: i64| {
            let (row, col) = (i64::from(row), i64::from(col).max(-1));
            if (row0..=row1).contains(&row) && col <= col1 {
                let slot = &mut grid[((row - row0) * cols + col - col0) as usize];
                slot.0 += cover;
                slot.1 += area;
            }
        };
        for_each_edge(path, |a, b| render_line(&mut add, a, b));
        // Each row is swept from the left over every slot: a slot of 0 gives
        // the pixel of the span it is in (docs/adr/0015).
        let w = width as i64;
        grid.chunks(cols as usize)
            .zip(row0..)
            .for_each(|(slots, row)| {
                let mut cover = 0i64;
                slots.iter().zip(col0..).for_each(|(&(c, area), col)| {
                    cover += c;
                    if col >= 0 {
                        let a = alpha(cover * 2 * ONE64 - area, rule);
                        if a > 0 {
                            visit(col as usize, row as usize, a);
                        }
                    }
                });
                let a = alpha(cover * 2 * ONE64, rule);
                if a > 0 {
                    ((col1 + 1).max(0)..w).for_each(|x| visit(x as usize, row as usize, a));
                }
            });
    } else {
        cells.clear();
        let mut add = |row, col, cover, area| cells.push((row, col, cover, area));
        for_each_edge(path, |a, b| render_line(&mut add, a, b));
        merge(cells);
        pixels.clear();
        sweep_into(cells, rule, width, height, pixels);
        pixels.iter().for_each(|&(x, y, a)| visit(x, y, a));
    }
}

/// The pixels of the cells, sorted by row and column with one cell each.
#[cfg(test)]
pub(crate) fn sweep(
    cells: &[Cell],
    rule: FillRule,
    width: usize,
    height: usize,
) -> Vec<(usize, usize, u8)> {
    let mut out = Vec::new();
    sweep_into(cells, rule, width, height, &mut out);
    out
}

/// Adds the pixels of the cells to `out`.
fn sweep_into(
    cells: &[Cell],
    rule: FillRule,
    width: usize,
    height: usize,
    out: &mut Vec<(usize, usize, u8)>,
) {
    let (w, h) = (width as i64, height as i64);
    cells
        .chunk_by(|a, b| a.0 == b.0)
        .filter(|row_cells| (0..h).contains(&i64::from(row_cells[0].0)))
        .for_each(|row_cells| {
            let row = row_cells[0].0 as usize;
            let mut cover = 0i64;
            row_cells
                .iter()
                .enumerate()
                .for_each(|(i, &(_, col, c, area))| {
                    cover += c;
                    let col = i64::from(col);
                    if (0..w).contains(&col) {
                        let a = alpha(cover * 2 * ONE64 - area, rule);
                        if a > 0 {
                            out.push((col as usize, row, a));
                        }
                    }
                    let next = row_cells.get(i + 1).map_or(w, |&(_, c, _, _)| i64::from(c));
                    let a = alpha(cover * 2 * ONE64, rule);
                    if a > 0 {
                        (col + 1).max(0)..next.min(w)
                    } else {
                        0..0
                    }
                    .for_each(|x| out.push((x as usize, row, a)));
                });
        });
}

/// Twice the signed area of a polygon (the shoelace formula).
pub fn signed_area(points: &[Point]) -> i128 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| i128::from(a.0) * i128::from(b.1) - i128::from(b.0) * i128::from(a.1))
        .sum()
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

/// Adds the polygon of the join at `v` between the directions `d1` and
/// `d2` to `out`: a miter if it is within the limit of 10, a bevel otherwise.
fn join(out: &mut Path, v: Point, d1: (i64, i64), d2: (i64, i64), half: i64) {
    let cross = i128::from(d1.0) * i128::from(d2.1) - i128::from(d1.1) * i128::from(d2.0);
    if cross == 0 {
        return;
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
        out.push_positive(&[v, a, add(v, m), b]);
    } else {
        out.push_positive(&[v, a, b]);
    }
}

/// The outline of the stroke of a path, `half` units on each side, with
/// butt caps and miter joins (limit 10), as positive polygons to fill with
/// the nonzero rule.
pub fn stroke(path: &Path, half: i64) -> Path {
    let mut out = Path::new();
    stroke_into(path, half, &mut out);
    out
}

/// Adds the polygons of [`stroke`] to `out`.
pub fn stroke_into(path: &Path, half: i64, out: &mut Path) {
    path.subpaths().for_each(|s| {
        // The segments between the points, without the repeated points,
        // and the segment back to the start of a closed subpath.
        let points = s.points;
        let open = points
            .windows(2)
            .map(|w| (w[0], w[1]))
            .filter(|(a, b)| a != b);
        let (first, last) = (points[0], points[points.len() - 1]);
        let closing =
            (s.closed && open.clone().next().is_some() && first != last).then_some((last, first));
        let segments = open.chain(closing);
        segments.clone().for_each(|(a, b)| {
            let n = normal(direction(a, b), half);
            out.push_positive(&[add(a, n), add(b, n), add(b, neg(n)), add(a, neg(n))]);
        });
        let joins = segments.clone().zip(segments.clone().skip(1));
        let around = segments
            .clone()
            .nth(1)
            .and(s.closed.then_some(()))
            .and_then(|()| segments.clone().last().zip(segments.clone().next()));
        joins
            .chain(around)
            .for_each(|(p, q)| join(out, p.1, direction(p.0, p.1), direction(q.0, q.1), half));
    });
}

/// `c` over `p` with opacity `a`, rounded: `(c a + p (255 − a) + 127) / 255`.
pub fn composite(c: u8, a: u8, p: u8) -> u8 {
    let (c, a, p) = (u32::from(c), u32::from(a), u32::from(p));
    ((c * a + p * (255 - a) + 127) / 255) as u8
}

/// An RGB image, 8 bits per channel, white at first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Canvas {
        Canvas {
            width,
            height,
            pixels: vec![255; width * height * 3],
        }
    }

    /// Makes the canvas `width` × `height` and white, in its buffer.
    pub fn reset(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.pixels.clear();
        self.pixels.resize(width * height * 3, 255);
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 3;
        [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2]]
    }

    /// Composites a color over the pixels of a coverage.
    pub fn paint(&mut self, coverage: &[(usize, usize, u8)], rgb: [u8; 3]) {
        coverage
            .iter()
            .for_each(|&(x, y, a)| self.paint_pixel(x, y, a, rgb));
    }

    /// Composites a color over one pixel with opacity `a`.
    pub fn paint_pixel(&mut self, x: usize, y: usize, a: u8, rgb: [u8; 3]) {
        let i = (y * self.width + x) * 3;
        let p = &mut self.pixels[i..i + 3];
        p[0] = composite(rgb[0], a, p[0]);
        p[1] = composite(rgb[1], a, p[1]);
        p[2] = composite(rgb[2], a, p[2]);
    }

    /// Fills a path with a color, compositing each pixel as it is swept
    /// ([`visit_coverage`]); the same pixels as [`Canvas::paint`] of the
    /// coverage.
    pub fn fill(&mut self, path: &Path, rule: FillRule, rgb: [u8; 3], scratch: &mut Scratch) {
        let (width, height) = (self.width, self.height);
        visit_coverage(path, rule, width, height, scratch, |x, y, a| {
            self.paint_pixel(x, y, a, rgb)
        });
    }
}

#[cfg(test)]
mod reference;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod reference_tests;
