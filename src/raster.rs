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
        self.curve_to_within(c1, c2, p, &Frame::NONE);
    }

    /// [`Path::curve_to`] on a canvas: the parts of the curve beyond the
    /// frame are one segment each (docs/adr/0022).
    pub fn curve_to_within(&mut self, c1: Point, c2: Point, p: Point, frame: &Frame) {
        let start = self.current_point().unwrap_or(c1);
        // After the first point, the subpath is open: the others are pushed.
        let mut first = true;
        flatten_curve_into(start, c1, c2, p, frame, |q| {
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

    /// The points of every subpath.
    pub fn points(&self) -> &[Point] {
        &self.points
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

/// The size of a canvas in pixels, for the arithmetic of the cells
/// (docs/adr/0022).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub width: i64,
    pub height: i64,
}

impl Area {
    pub fn new(width: usize, height: usize) -> Area {
        Area {
            width: width as i64,
            height: height as i64,
        }
    }
}

/// The regions beyond a canvas and a margin around it, in device units: a
/// curve whose control points are all in one of them is one segment there
/// (docs/adr/0022). A point is beyond the frame above if y ≤ `top`, below
/// if y ≥ `bottom`, right if x ≥ `right`, and left if x ≤ `left`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Frame {
    /// No region: every curve is split as [`flatten_curve`] splits it.
    pub const NONE: Frame = Frame {
        left: i64::MIN,
        top: i64::MIN,
        right: i64::MAX,
        bottom: i64::MAX,
    };

    /// The frame `margin` units around a `width` × `height` canvas.
    pub fn around(width: usize, height: usize, margin: i64) -> Frame {
        Frame {
            left: -margin - 1,
            top: -margin,
            right: width as i64 * ONE64 + margin,
            bottom: height as i64 * ONE64 + margin,
        }
    }

    /// The bounds of four points: the least and the largest x and y.
    fn bounds(a: Point, b: Point, c: Point, d: Point) -> (i64, i64, i64, i64) {
        let (xs, ys) = (
            [a.0, b.0, c.0, d.0].map(i64::from),
            [a.1, b.1, c.1, d.1].map(i64::from),
        );
        (
            xs[0].min(xs[1]).min(xs[2]).min(xs[3]),
            xs[0].max(xs[1]).max(xs[2]).max(xs[3]),
            ys[0].min(ys[1]).min(ys[2]).min(ys[3]),
            ys[0].max(ys[1]).max(ys[2]).max(ys[3]),
        )
    }

    /// Whether the four points are all in one region beyond the frame.
    fn holds(&self, a: Point, b: Point, c: Point, d: Point) -> bool {
        let (xlo, xhi, ylo, yhi) = Frame::bounds(a, b, c, d);
        yhi <= self.top || ylo >= self.bottom || xlo >= self.right || xhi <= self.left
    }

    /// Whether both ends of a segment are in one region beyond the frame.
    fn holds_segment(&self, a: Point, b: Point) -> bool {
        self.holds(a, b, a, b)
    }

    /// Whether some point of the bounds of the four points is in a region
    /// beyond the frame. If none is, no curve within those bounds is held
    /// by a region, so the curves split from one need not be checked.
    fn reaches(&self, a: Point, b: Point, c: Point, d: Point) -> bool {
        let (xlo, xhi, ylo, yhi) = Frame::bounds(a, b, c, d);
        ylo <= self.top || yhi >= self.bottom || xhi >= self.right || xlo <= self.left
    }
}

/// The points that approximate a cubic Bézier curve, without `p0` and with
/// `p3`, by de Casteljau's subdivision at the midpoint.
pub fn flatten_curve(p0: Point, p1: Point, p2: Point, p3: Point) -> Vec<Point> {
    flatten_curve_within(p0, p1, p2, p3, &Frame::NONE)
}

/// [`flatten_curve`] on a canvas: the parts of the curve beyond the frame
/// are one segment each (docs/adr/0022).
pub fn flatten_curve_within(
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
    frame: &Frame,
) -> Vec<Point> {
    let mut out = Vec::new();
    flatten_curve_into(p0, p1, p2, p3, frame, |q| out.push(q));
    out
}

/// The points of [`flatten_curve`], given to `emit` in order: a curve that
/// is flat enough, or at the deepest level, gives its end point; another is
/// split at its midpoint, first half first. The recursion is at most
/// MAX_DEPTH deep, and keeps only the curves of the levels it is in
/// (docs/adr/0015). A curve whose control points are all in one region
/// beyond the frame gives its end point too: the points it would give are
/// within the bounds of its control points (a midpoint is between its
/// ends), so they and the one segment are in the same region, where the
/// cells change no pixel, or only add to a row the cover from the start
/// to the end (docs/adr/0022).
fn flatten_curve_into(
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
    frame: &Frame,
    mut emit: impl FnMut(Point),
) {
    // The regions are checked only for a curve whose bounds reach one:
    // the control points of the curves split from it, and so their
    // bounds, are within its bounds.
    fn split(
        a: Point,
        b: Point,
        c: Point,
        d: Point,
        depth: u32,
        frame: Option<&Frame>,
        emit: &mut impl FnMut(Point),
    ) {
        if depth >= MAX_DEPTH
            || flatness(a, b, c, d) <= FLATNESS
            || frame.is_some_and(|f| f.holds(a, b, c, d))
        {
            emit(d);
            return;
        }
        let ab = mid(a, b);
        let bc = mid(b, c);
        let cd = mid(c, d);
        let abc = mid(ab, bc);
        let bcd = mid(bc, cd);
        let m = mid(abc, bcd);
        split(a, ab, abc, m, depth + 1, frame, emit);
        split(m, bcd, cd, d, depth + 1, frame, emit);
    }
    let frame = frame.reaches(p0, p1, p2, p3).then_some(frame);
    split(p0, p1, p2, p3, 0, frame, &mut emit);
}

#[cfg(test)]
thread_local! {
    /// The calls of `add` by [`render_line`] on this thread, for the tests
    /// of the work off the canvas (docs/adr/0022).
    static ADDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Counts a call of `add`.
#[inline]
fn count_add() {
    #[cfg(test)]
    ADDS.with(|c| c.set(c.get() + 1));
}

/// The number of calls of `add` by [`render_line`] while `f` runs on this
/// thread, with its result.
#[cfg(test)]
pub(crate) fn counting_adds<R>(f: impl FnOnce() -> R) -> (R, u64) {
    let before = ADDS.with(std::cell::Cell::get);
    let result = f();
    (result, ADDS.with(std::cell::Cell::get) - before)
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
    crossings_within(a, b, i64::MIN, i64::MAX)
}

/// The multiples k · ONE of [`crossings`] with `k0 ≤ k ≤ k1`, in the same
/// order: the crossings on the rows or the columns of a canvas, without
/// walking the others (docs/adr/0022).
fn crossings_within(a: i64, b: i64, k0: i64, k1: i64) -> impl Iterator<Item = i64> {
    let (lo, hi) = (a.min(b), a.max(b));
    let first = (lo.div_euclid(ONE64) + 1).max(k0);
    let last = (hi - 1).div_euclid(ONE64).min(k1);
    let count = (last - first + 1).max(0);
    (0..count).map(move |i| if a <= b { first + i } else { last - i } * ONE64)
}

/// b at the point where a is `at`, on the line from (a0, b0) to (a1, b1).
/// The product fits in i64 for most segments, whose division is faster
/// and gives the same quotient (docs/adr/0015). Only the tests compare with
/// it now, since [`interpolations`] carries the crossings (docs/adr/0020).
#[cfg(test)]
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

/// The integers the crossings are carried in: i64 for the segments within
/// its bounds, i128 for the others (docs/adr/0020).
trait Carry:
    Copy
    + Ord
    + From<i64>
    + std::ops::Add<Output = Self>
    + std::ops::Sub<Output = Self>
    + std::ops::Mul<Output = Self>
{
    /// The Euclidean quotient and remainder by a positive `d`.
    fn euclid(self, d: Self) -> (Self, Self);
    /// Those of the product `db · delta`, which may pass the integers of
    /// the carrier when `delta` is far from 0 (docs/adr/0022).
    fn product_euclid(db: Self, delta: Self, d: Self) -> (Self, Self);
    fn is_odd(self) -> bool;
    /// The value, which the caller knows to be within i64.
    fn narrow(self) -> i64;
}

impl Carry for i64 {
    fn euclid(self, d: i64) -> (i64, i64) {
        (self.div_euclid(d), self.rem_euclid(d))
    }
    fn product_euclid(db: i64, delta: i64, d: i64) -> (i64, i64) {
        // The quotient is at most |db| and the remainder below d, which
        // fit; only the product may not.
        match db.checked_mul(delta) {
            Some(n) => n.euclid(d),
            None => {
                let (q, r) = (i128::from(db) * i128::from(delta)).euclid(i128::from(d));
                (q as i64, r as i64)
            }
        }
    }
    fn is_odd(self) -> bool {
        self & 1 == 1
    }
    fn narrow(self) -> i64 {
        self
    }
}

impl Carry for i128 {
    fn euclid(self, d: i128) -> (i128, i128) {
        (self.div_euclid(d), self.rem_euclid(d))
    }
    fn product_euclid(db: i128, delta: i128, d: i128) -> (i128, i128) {
        (db * delta).euclid(d)
    }
    fn is_odd(self) -> bool {
        self & 1 == 1
    }
    fn narrow(self) -> i64 {
        self as i64
    }
}

/// The crossings of one segment, carried in `T`: the products are taken
/// to a positive divisor `d`; after the first crossing, the Euclidean
/// quotient and remainder of the last one and those of the step.
struct Carrier<T> {
    a0: T,
    b0: T,
    db: T,
    d: T,
    last: Option<(T, T, T)>,
    step: Option<(T, T)>,
}

impl<T: Carry> Carrier<T> {
    fn new(a0: T, a1: T, b0: T, b1: T) -> Carrier<T> {
        let (db, d) = if a1 < a0 {
            (b0 - b1, a0 - a1)
        } else {
            (b1 - b0, a1 - a0)
        };
        Carrier {
            a0,
            b0,
            db,
            d,
            last: None,
            step: None,
        }
    }

    /// b at the crossing `at`, rounded to the nearest, ties to even, as
    /// `div_round` of its own product gives it.
    fn at(&mut self, at: i64) -> i64 {
        let (at, d) = (T::from(at), self.d);
        let (q, r) = match self.last {
            None => T::product_euclid(self.db, at - self.a0, d),
            Some((last, q, r)) => {
                let db = self.db;
                let (dq, dr) = *self
                    .step
                    .get_or_insert_with(|| (db * (at - last)).euclid(d));
                let (q, r) = (q + dq, r + dr);
                if r >= d {
                    (q + T::from(1), r - d)
                } else {
                    (q, r)
                }
            }
        };
        self.last = Some((at, q, r));
        let up = r > d - r || (r == d - r && q.is_odd());
        (self.b0 + q + T::from(i64::from(up))).narrow()
    }
}

/// The carrier in i64 of the segment from (a0, b0) to (a1, b1), if its
/// values stay within i64: with |db| ≤ 2^54 and d ≤ 2^61, the product of
/// the step (256) is at most 2^62 in size, the quotient at most |db|, and
/// the remainders below 2d; the rounded b is between b0 and b1
/// (docs/adr/0020). The first crossing is within 256 of a0 for a whole
/// segment, but may be anywhere between a0 and a1 for the rows of a
/// canvas, so its product is taken in i128 when it passes i64
/// (docs/adr/0022).
fn narrow_carrier(a0: i64, a1: i64, b0: i64, b1: i64) -> Option<Carrier<i64>> {
    let (db, d) = if a1 < a0 {
        (b0.checked_sub(b1)?, a0.checked_sub(a1)?)
    } else {
        (b1.checked_sub(b0)?, a1.checked_sub(a0)?)
    };
    (db.unsigned_abs() <= 1 << 54 && d <= 1 << 61).then(|| Carrier::new(a0, a1, b0, b1))
}

/// The path [`interpolations`] takes for a segment, for the tests.
#[cfg(test)]
fn crossing_path(a0: i64, a1: i64, b0: i64, b1: i64) -> &'static str {
    narrow_carrier(a0, a1, b0, b1).map_or("i128", |_| "i64")
}

/// The carrier of a segment: in i64 within its bounds, or in i128.
enum Carriers {
    Narrow(Carrier<i64>),
    Wide(Carrier<i128>),
}

/// `(at, b)` for each `at` of `ats`, with b at `at` on the line from
/// (a0, b0) to (a1, b1), rounded as one division in i128 would. The `ats`
/// are `crossings(a0, a1)`: the first within 256 of a0, then 256 apart, so
/// the quotient and the remainder are carried from one to the next with
/// one division for the first and one for the step: the Euclidean quotient
/// and remainder of n + Δ are those of n plus those of Δ, with one carry
/// (docs/adr/0019). Any i64 is taken, in i64 or in i128 (docs/adr/0020).
fn interpolations(
    a0: i64,
    a1: i64,
    b0: i64,
    b1: i64,
    ats: impl Iterator<Item = i64>,
) -> impl Iterator<Item = (i64, i64)> {
    let w = i128::from;
    let mut carrier = narrow_carrier(a0, a1, b0, b1).map_or_else(
        || Carriers::Wide(Carrier::new(w(a0), w(a1), w(b0), w(b1))),
        Carriers::Narrow,
    );
    ats.map(move |at| {
        let b = match &mut carrier {
            Carriers::Narrow(c) => c.at(at),
            Carriers::Wide(c) => c.at(at),
        };
        (at, b)
    })
}

/// Adds a line segment to the cells through `add(row, column, cover, area)`.
/// The segment is split at the rows, with y exact and x interpolated, and
/// each piece at the columns, with x exact and y interpolated within the
/// row. This is the innermost loop of filling, so the pieces are walked
/// with plain loops (docs/adr/0014).
///
/// Only the crossings with the rows of the canvas are walked
/// (docs/adr/0022): a piece with an end above or below them is in a row
/// beyond the canvas, where the cells change no pixel, and
/// [`render_row_piece`] drops it by its row. The loop is the one of a whole
/// segment, so that the crossings are carried as fast.
pub(crate) fn render_line(
    add: &mut impl FnMut(i32, i32, i64, i64),
    p0: Point,
    p1: Point,
    area: Area,
) {
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
    for (y, x) in interpolations(y0, y1, x0, x1, crossings_within(y0, y1, 0, area.height)) {
        let p = (x, y);
        render_row_piece(add, last, p, area);
        last = p;
    }
    render_row_piece(add, last, (x1, y1), area);
}

/// Adds the piece of a segment within one row of a canvas (docs/adr/0022).
/// A piece in a row beyond the canvas changes no pixel and is dropped.
/// Only the columns of the canvas are walked: a cell right of them (x ≥ W)
/// changes no pixel and is dropped; the cells left of them (x < 0) only
/// add their cover to the row, so the part of the piece there, which is at
/// most the first or the last cell, is one cell of column −1 without area,
/// with the height of the part.
fn render_row_piece(
    add: &mut impl FnMut(i32, i32, i64, i64),
    a: (i64, i64),
    b: (i64, i64),
    area: Area,
) {
    let ((ax, ay), (bx, by)) = (a, b);
    let row = ay.min(by).div_euclid(ONE64);
    if !(0..area.height).contains(&row) {
        return;
    }
    let row = row as i32;
    let (ylo, yhi) = (ay.min(by), ay.max(by));
    let width = area.width;
    let wu = width * ONE64;
    let (xlo, xhi) = (ax.min(bx), ax.max(bx));
    if xlo >= 0 && xhi < wu {
        // Within the columns of the canvas, as most pieces are.
        let mut last = (ax, ay);
        if ax != bx {
            for (x, y) in interpolations(ax, bx, ay, by, crossings(ax, bx)) {
                let p = (x, y.clamp(ylo, yhi));
                cell(add, row, last, p);
                last = p;
            }
        }
        cell(add, row, last, (bx, by));
        return;
    }
    // A cell with x < 0 is left of the canvas, one with x ≥ W right of it.
    let mut cell_within = |c: (i64, i64), d: (i64, i64)| {
        let x = c.0.min(d.0);
        if x < 0 {
            count_add();
            add(row, -1, d.1 - c.1, 0);
        } else if x < wu {
            cell(add, row, c, d);
        }
    };
    let mut last = (ax, ay);
    if ax != bx {
        for (x, y) in interpolations(ax, bx, ay, by, crossings_within(ax, bx, 0, width)) {
            let p = (x, y.clamp(ylo, yhi));
            cell_within(last, p);
            last = p;
        }
    }
    cell_within(last, (bx, by));
}

/// Adds the cell of the piece from `c` to `d` within one column of `row`.
#[inline]
fn cell(
    add: &mut impl FnMut(i32, i32, i64, i64),
    row: i32,
    (cx, cy): (i64, i64),
    (dx, dy): (i64, i64),
) {
    let col = cx.min(dx).div_euclid(ONE64);
    let (fx0, fx1) = (cx - col * ONE64, dx - col * ONE64);
    let height = dy - cy;
    count_add();
    add(row, col as i32, height, (fx0 + fx1) * height);
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

/// The buffers of filling: the cells, the dense grid of a small path (zero
/// between fills) and the touched span of each of its rows, the pixels, and
/// the opacities of one row, kept between the fills of a page (docs/adr/0013,
/// 0014).
#[derive(Clone, Debug, Default)]
pub struct Scratch {
    cells: Vec<Cell>,
    grid: Vec<(i64, i64)>,
    spans: Vec<(i64, i64)>,
    pixels: Vec<(usize, usize, u8)>,
    alphas: Vec<u8>,
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
    visit_rows(path, rule, width, height, scratch, |y, x0, alphas| {
        alphas
            .iter()
            .zip(x0..)
            .filter(|&(&a, _)| a > 0)
            .for_each(|(&a, x)| visit(x, y, a));
    });
}

/// Calls `visit(y, x, opacities)` with the pixels of [`coverage`] by runs of
/// a row: the pixels from `x` have the opacities, 0 included, in order. The
/// runs come in the order of the pixels, and no run is empty.
fn visit_rows(
    path: &Path,
    rule: FillRule,
    width: usize,
    height: usize,
    scratch: &mut Scratch,
    mut visit: impl FnMut(usize, usize, &[u8]),
) {
    let Scratch {
        cells,
        grid,
        spans,
        pixels,
        alphas,
    } = scratch;
    let Some((row0, row1, col0, col1)) = grid_bounds(path, width, height) else {
        return;
    };
    let cols = col1 - col0 + 1;
    let rows = row1 - row0 + 1;
    if rows * cols <= MAX_GRID {
        // The grid is zero between fills: it only grows, and the slots a
        // fill touches are zeroed after its sweep.
        if grid.len() < (rows * cols) as usize {
            grid.resize((rows * cols) as usize, (0, 0));
        }
        // The first and the last slot of each row that a cell was added to.
        spans.clear();
        spans.resize(rows as usize, (i64::MAX, i64::MIN));
        let mut add = |row: i32, col: i32, cover: i64, area: i64| {
            let (row, col) = (i64::from(row), i64::from(col).max(-1));
            if (row0..=row1).contains(&row) && col <= col1 {
                let (i, j) = ((row - row0) as usize, col - col0);
                let slot = &mut grid[i * cols as usize + j as usize];
                slot.0 += cover;
                slot.1 += area;
                let span = &mut spans[i];
                *span = (span.0.min(j), span.1.max(j));
            }
        };
        let area = Area::new(width, height);
        for_each_edge(path, |a, b| render_line(&mut add, a, b, area));
        // Each row is swept from the left over the slots between the first
        // and the last added to: a slot of 0 gives the pixel of the span it
        // is in (docs/adr/0015), so the slots before the first give no
        // pixel, and those after the last the pixel of the cover of the row,
        // as the columns right of the grid do. The slot of column −1, if
        // any, only adds its cover.
        let w = width as i64;
        let left = i64::from(col0 < 0);
        grid.chunks_mut(cols as usize)
            .zip(spans.iter())
            .zip(row0..)
            .filter(|((_, span), _)| span.0 <= span.1)
            .for_each(|((slots, &(lo, hi)), row)| {
                let first = lo.max(left);
                let mut cover = slots[lo as usize..first as usize]
                    .iter()
                    .map(|&(c, _)| c)
                    .sum::<i64>();
                alphas.clear();
                if first <= hi {
                    alphas.extend(
                        slots[first as usize..=hi as usize]
                            .iter()
                            .map(|&(c, area)| {
                                cover += c;
                                alpha(cover * 2 * ONE64 - area, rule)
                            }),
                    );
                    visit(row as usize, (col0 + first) as usize, alphas);
                }
                slots[lo as usize..=hi as usize].fill((0, 0));
                let a = alpha(cover * 2 * ONE64, rule);
                let tail = col0 + hi + 1;
                if a > 0 && tail < w {
                    alphas.clear();
                    alphas.resize((w - tail) as usize, a);
                    visit(row as usize, tail as usize, alphas);
                }
            });
    } else {
        cells.clear();
        let mut add = |row, col, cover, area| cells.push((row, col, cover, area));
        let area = Area::new(width, height);
        for_each_edge(path, |a, b| render_line(&mut add, a, b, area));
        merge(cells);
        pixels.clear();
        sweep_into(cells, rule, width, height, pixels);
        pixels.chunk_by(|a, b| a.1 == b.1).for_each(|row| {
            let (x0, y) = (row[0].0, row[0].1);
            let x1 = row[row.len() - 1].0;
            alphas.clear();
            alphas.resize(x1 - x0 + 1, 0);
            row.iter().for_each(|&(x, _, a)| alphas[x - x0] = a);
            visit(y, x0, alphas);
        });
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
    let a = add(v, normal(d1, half * sign));
    let b = add(v, normal(d2, half * sign));
    match miter(d1, d2, half) {
        Some(m) => out.push_positive(&[v, a, add(v, m), b]),
        None => out.push_positive(&[v, a, b]),
    }
}

/// The offset of the miter point of the join between the directions `d1`
/// and `d2` from its vertex, or `None` for a bevel: when the directions
/// are parallel or the miter ratio is past the limit of 10.
fn miter(d1: (i64, i64), d2: (i64, i64), half: i64) -> Option<(i64, i64)> {
    let cross = i128::from(d1.0) * i128::from(d2.1) - i128::from(d1.1) * i128::from(d2.0);
    if cross == 0 {
        return None;
    }
    let sign = if cross > 0 { -1 } else { 1 };
    let n1 = normal(d1, half * sign);
    let n2 = normal(d2, half * sign);
    let dot = i128::from(d1.0) * i128::from(d2.0) + i128::from(d1.1) * i128::from(d2.1);
    let len2 = |d: (i64, i64)| {
        (i128::from(d.0) * i128::from(d.0) + i128::from(d.1) * i128::from(d.1)) as u128
    };
    let lens = isqrt(len2(d1).saturating_mul(len2(d2))) as i128;
    // The miter ratio 1 / sin(phi / 2) is at most 10 if (1 − cos phi) · 10² ≥ 2,
    // where cos phi = −dot / lens.
    let within_limit = 100 * dot + 98 * lens >= 0;
    let det = i128::from(n1.0) * i128::from(n2.1) - i128::from(n1.1) * i128::from(n2.0);
    if !within_limit || det == 0 {
        return None;
    }
    // m = half (N1 + N2) / (1 + cos θ), from the directions shifted under
    // 2^28 and their lengths to 2^−32 (docs/adr/0023).
    let (e1, e2) = (under_2_28(d1), under_2_28(d2));
    let length = |e: (i128, i128)| isqrt(((e.0 * e.0 + e.1 * e.1) as u128) << 64) as i128;
    let (l1, l2) = (length(e1), length(e2));
    let sign = i128::from(sign);
    let (p1, p2) = ((-sign * e1.1, sign * e1.0), (-sign * e2.1, sign * e2.0));
    let den = l1 * l2 + ((e1.0 * e2.0 + e1.1 * e2.1) << 64);
    if den <= 0 {
        return None;
    }
    let den = div_round(den, 1 << 32);
    let half = i128::from(half);
    let part = |a: i128, b: i128| -> Option<i64> {
        let num = half.checked_mul(a * l2 + b * l1)?;
        i64::try_from(div_round(num, den)).ok()
    };
    Some((part(p1.0, p2.0)?, part(p1.1, p2.1)?))
}

/// A direction shifted right until its components are under 2^28.
fn under_2_28(d: (i64, i64)) -> (i128, i128) {
    let bits = 64 - (d.0.unsigned_abs() | d.1.unsigned_abs()).leading_zeros();
    let shift = bits.saturating_sub(28);
    (i128::from(d.0 >> shift), i128::from(d.1 >> shift))
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
    path.subpaths()
        .for_each(|s| stroke_polyline(s.points, s.closed, half, None, (false, false), out));
}

/// Adds the polygons of the stroke of one polyline to `out`: the
/// quadrilateral of each segment, then the joins between the segments
/// (and, when it is closed, the join at its start), then, when it is open
/// and `disc` is the disc of round caps, the disc at each end that `caps`
/// asks for. Repeated points make no segment; a polyline with no segment
/// adds nothing.
fn stroke_polyline(
    points: &[Point],
    closed: bool,
    half: i64,
    disc: Option<&[Point]>,
    caps: (bool, bool),
    out: &mut Path,
) {
    // The segments between the points, without the repeated points,
    // and the segment back to the start of a closed subpath.
    let open = points
        .windows(2)
        .map(|w| (w[0], w[1]))
        .filter(|(a, b)| a != b);
    if open.clone().next().is_none() {
        return;
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    let closing = (closed && first != last).then_some((last, first));
    let segments = open.chain(closing);
    segments.clone().for_each(|(a, b)| {
        let n = normal(direction(a, b), half);
        out.push_positive(&[add(a, n), add(b, n), add(b, neg(n)), add(a, neg(n))]);
    });
    let joins = segments.clone().zip(segments.clone().skip(1));
    let around = segments
        .clone()
        .nth(1)
        .and(closed.then_some(()))
        .and_then(|()| segments.clone().last().zip(segments.clone().next()));
    joins
        .chain(around)
        .for_each(|(p, q)| join(out, p.1, direction(p.0, p.1), direction(q.0, q.1), half));
    if let Some(disc) = disc
        && !closed
    {
        if caps.0 {
            push_disc(out, disc, first);
        }
        if caps.1 {
            push_disc(out, disc, last);
        }
    }
}

/// Adds the disc, a positive polygon around the origin, moved to `center`.
fn push_disc(out: &mut Path, disc: &[Point], center: Point) {
    out.starts.push((out.points.len(), true));
    out.points.extend(
        disc.iter()
            .map(|&(x, y)| add(center, (i64::from(x), i64::from(y)))),
    );
}

/// The style of the stroke of a page's path (docs/adr/0025): half the
/// width in device units, the caps, and the dash array in units of
/// 2^−16 device units with its phase, both scaled to the device already,
/// the phase within the period. An empty array is a solid line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stroke<'a> {
    pub half: i64,
    pub round_cap: bool,
    pub dash: &'a [i64],
    pub phase: i64,
}

/// The units of the lengths of the dashes in one device unit (1/256 pixel).
pub const DASH_UNIT: i64 = 1 << 16;

/// The buffers of the strokes of a page, kept between them (docs/adr/0013):
/// the disc of the round caps, and the points of one dash.
#[derive(Clone, Debug, Default)]
pub struct StrokeScratch {
    disc: Vec<Point>,
    piece: Vec<Point>,
}

/// The factor of the distance of the control points of a quarter circle
/// from its ends, `4 (√2 − 1) / 3 ≈ 0.552285`, times 2^16.
const QUARTER_CONTROL: i128 = 36195;

/// The polygon of the disc of a round cap, `half` units around the origin,
/// as a positive polygon (docs/adr/0025): the quarter from (half, 0) to
/// (0, half), a cubic Bézier curve with its control points
/// `half · 36195 / 65536` from its ends, flattened as the curves of a path
/// are, then its reflections in the axes, so that the disc is symmetric
/// in the axes and the diagonals. `half` is below 2^31.
pub fn disc_into(half: i64, out: &mut Vec<Point>) {
    out.clear();
    let h = half as i32;
    let k = div_round(i128::from(half) * QUARTER_CONTROL, 65536) as i32;
    out.push((h, 0));
    flatten_curve_into((h, 0), (h, k), (k, h), (0, h), &Frame::NONE, |p| {
        out.push(p)
    });
    let n = out.len();
    // From (0, half) to (−half, 0), to (0, −half), and back to (half, 0).
    (0..n - 1).rev().for_each(|i| {
        let (x, y) = out[i];
        out.push((-x, y));
    });
    (1..n).for_each(|i| {
        let (x, y) = out[i];
        out.push((-x, -y));
    });
    (1..n - 1).rev().for_each(|i| {
        let (x, y) = out[i];
        out.push((x, -y));
    });
}

/// [`disc_into`] as a new polygon.
pub fn disc(half: i64) -> Vec<Point> {
    let mut out = Vec::new();
    disc_into(half, &mut out);
    out
}

/// The length of a segment in units of 2^−16 device units, rounded down:
/// `isqrt((dx² + dy²) · 2^32)` (docs/adr/0025).
pub fn segment_length(a: Point, b: Point) -> i64 {
    let (dx, dy) = direction(a, b);
    let d2 = (i128::from(dx) * i128::from(dx) + i128::from(dy) * i128::from(dy)) as u128;
    // Most segments are shorter than 256 pixels, where the product fits
    // in u64, whose square root is faster.
    match u32::try_from(d2) {
        Ok(d2) => (u64::from(d2) << 32).isqrt() as i64,
        Err(_) => isqrt(d2 << 32) as i64,
    }
}

/// The point `at` units of 2^−16 device units from `a` on the segment to
/// `b` of `length`: `a + div_round((b − a) · at, length)`, by component.
fn point_at(a: Point, b: Point, at: i64, length: i64) -> Point {
    let (dx, dy) = direction(a, b);
    let part = |d: i64| div_round(i128::from(d) * i128::from(at), i128::from(length)) as i64;
    add(a, (part(dx), part(dy)))
}

/// Where a dash array is in its cycle: the element and the length left in
/// it. An array of an odd number of elements is its double
/// (docs/adr/0025); the elements at even indices are drawn.
struct Dashes<'a> {
    dash: &'a [i64],
    cycle: usize,
    /// The sum of the elements of the cycle.
    period: i128,
    index: usize,
    left: i64,
}

impl<'a> Dashes<'a> {
    /// The dash array `phase` into its cycle; the phase is within the
    /// period, and the array has an element above 0.
    fn start(dash: &'a [i64], phase: i64) -> Dashes<'a> {
        let cycle = if dash.len() % 2 == 1 {
            2 * dash.len()
        } else {
            dash.len()
        };
        let period =
            dash.iter().map(|&l| i128::from(l)).sum::<i128>() * (cycle / dash.len()) as i128;
        let mut dashes = Dashes {
            dash,
            cycle,
            period,
            index: 0,
            left: dash[0],
        };
        // Elements the phase passes whole are skipped; a phase of 0 starts
        // at the first, even one of length 0.
        if phase > 0 {
            dashes.advance(phase);
        }
        dashes
    }

    fn on(&self) -> bool {
        self.index.is_multiple_of(2)
    }

    fn next(&mut self) {
        self.index = (self.index + 1) % self.cycle;
        self.left = self.dash[self.index % self.dash.len()];
    }

    /// Moves `length` along the cycle: an element that ends within it, or
    /// exactly at its end, is passed. Whole cycles come back to the same
    /// place, so only the rest of the length is walked, within one cycle.
    fn advance(&mut self, length: i64) {
        let mut length = (i128::from(length) % self.period) as i64;
        while length >= self.left {
            length -= self.left;
            self.next();
        }
        self.left -= length;
    }
}

/// Cuts a polyline by the dash array of `style`, from the array's start
/// moved by the phase, and calls `emit(points, caps)` with each dash that
/// is drawn: its ends and the vertices between them, and whether its
/// start and its end get a cap (docs/adr/0025). A dash of no length is
/// two equal points. A segment whose ends are in one region beyond
/// `frame` only moves along the array: the dash through its start ends
/// there without a cap, and the dash through its end starts there without
/// one; both are in the region, where no pixel changes. In a closed
/// polyline the segment back to the start is dashed as the others, and the
/// last dash is not joined with the first.
fn for_each_dash(
    points: &[Point],
    closed: bool,
    style: &Stroke,
    frame: &Frame,
    piece: &mut Vec<Point>,
    mut emit: impl FnMut(&[Point], (bool, bool)),
) {
    let (first, last) = (points[0], points[points.len() - 1]);
    let closing = (closed && first != last).then_some((last, first));
    let segments = points.windows(2).map(|w| (w[0], w[1])).chain(closing);
    let mut dashes = Dashes::start(style.dash, style.phase);
    piece.clear();
    let mut capped = true;
    if dashes.on() {
        piece.push(first);
    }
    segments.for_each(|(a, b)| {
        let length = segment_length(a, b);
        if frame.holds_segment(a, b) {
            if dashes.on() && !piece.is_empty() {
                emit(piece, (capped, false));
            }
            piece.clear();
            dashes.advance(length);
            capped = false;
            if dashes.on() {
                piece.push(b);
            }
            return;
        }
        let mut at = 0;
        loop {
            if dashes.left > length - at {
                // The element goes on past b, a vertex of its dash.
                dashes.left -= length - at;
                if dashes.on() {
                    piece.push(b);
                }
                break;
            }
            at += dashes.left;
            let p = if at == length {
                b
            } else {
                point_at(a, b, at, length)
            };
            if dashes.on() {
                piece.push(p);
                emit(piece, (capped, true));
                piece.clear();
            }
            dashes.next();
            capped = true;
            if dashes.on() {
                piece.push(p);
            }
            if at == length {
                break;
            }
        }
    });
    // The dash that is on at the end, if it has a length: one of no length
    // that starts exactly at the end is not drawn.
    if dashes.on() && piece.len() > 1 {
        emit(piece, (capped, true));
    }
}

/// The outline of the stroke of a page's path in the style, as positive
/// polygons to fill with the nonzero rule (docs/adr/0025), as
/// [`stroke_styled_into`] gives it.
pub fn stroke_styled(path: &Path, style: &Stroke, frame: &Frame) -> Path {
    let mut out = Path::new();
    stroke_styled_into(path, style, frame, &mut StrokeScratch::default(), &mut out);
    out
}

/// Adds the polygons of the stroke of a page's path to `out`: those of
/// [`stroke_into`], with the disc of a round cap at the ends of each open
/// subpath, and each subpath cut by the dash array into dashes stroked as
/// open subpaths (docs/adr/0025). The disc is made once and moved to each
/// end. A subpath of one point draws nothing; a closed one, or one whose
/// points are all the same, draws the disc with round caps when the dash
/// array is on at its start (ISO 32000-1 8.5.3.2). The frame is the one
/// of the margin of the stroke, beyond which a dashed segment only moves
/// along the array.
pub fn stroke_styled_into(
    path: &Path,
    style: &Stroke,
    frame: &Frame,
    scratch: &mut StrokeScratch,
    out: &mut Path,
) {
    let StrokeScratch { disc, piece } = scratch;
    let round = style.round_cap.then(|| {
        disc_into(style.half, disc);
        disc.as_slice()
    });
    let half = style.half;
    path.subpaths().for_each(|s| {
        let points = s.points;
        if points.iter().all(|&p| p == points[0]) {
            let on = style.dash.is_empty() || Dashes::start(style.dash, style.phase).on();
            if let Some(disc) = round
                && (s.closed || points.len() > 1)
                && on
            {
                push_disc(out, disc, points[0]);
            }
        } else if style.dash.is_empty() {
            stroke_polyline(points, s.closed, half, round, (true, true), out);
        } else {
            for_each_dash(points, s.closed, style, frame, piece, |dash, caps| {
                if dash.iter().all(|&p| p == dash[0]) {
                    // A dash of no length is a disc with round caps.
                    if let Some(disc) = round
                        && caps.0
                        && caps.1
                    {
                        push_disc(out, disc, dash[0]);
                    }
                } else {
                    stroke_polyline(dash, false, half, round, caps, out);
                }
            });
        }
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
        paint_bytes(&mut self.pixels[i..i + 3], a, rgb);
    }

    /// Fills a path with a color, compositing the pixels of each run of a
    /// row as it is swept ([`visit_rows`]); the same pixels as
    /// [`Canvas::paint`] of the coverage.
    pub fn fill(&mut self, path: &Path, rule: FillRule, rgb: [u8; 3], scratch: &mut Scratch) {
        let (width, height) = (self.width, self.height);
        // Black, the color of most fills, has no term of its own: the
        // products c · a are 0.
        let black = rgb == [0, 0, 0];
        visit_rows(path, rule, width, height, scratch, |y, x0, alphas| {
            let run = &mut self.pixels[(y * width + x0) * 3..][..alphas.len() * 3];
            if black {
                paint_run(run, alphas, paint_black);
            } else {
                paint_run(run, alphas, |p, a| paint_bytes(p, a, rgb));
            }
        });
    }
}

/// Paints the pixels of `run`, three bytes each, with the opacities.
fn paint_run(run: &mut [u8], alphas: &[u8], paint: impl Fn(&mut [u8], u8)) {
    run.as_chunks_mut::<3>()
        .0
        .iter_mut()
        .zip(alphas)
        .for_each(|(p, &a)| paint(p, a));
}

/// Composites a color over the three bytes of a pixel with opacity `a`. An
/// opacity of 0 leaves the pixel: (p · 255 + 127) / 255 = p.
fn paint_bytes(p: &mut [u8], a: u8, rgb: [u8; 3]) {
    let [r, g, b] = p else { return };
    // An opaque pixel is the color: (c · 255 + 127) / 255 = c.
    match a {
        0 => {}
        255 => (*r, *g, *b) = (rgb[0], rgb[1], rgb[2]),
        a => {
            *r = composite(rgb[0], a, *r);
            *g = composite(rgb[1], a, *g);
            *b = composite(rgb[2], a, *b);
        }
    }
}

/// [`paint_bytes`] with black: `composite(0, a, p)` is `(p (255 − a) + 127) / 255`.
fn paint_black(p: &mut [u8], a: u8) {
    let [r, g, b] = p else { return };
    let over = |p: u8| ((u32::from(p) * (255 - u32::from(a)) + 127) / 255) as u8;
    match a {
        0 => {}
        255 => (*r, *g, *b) = (0, 0, 0),
        _ => (*r, *g, *b) = (over(*r), over(*g), over(*b)),
    }
}

#[cfg(test)]
mod reference;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod reference_tests;

#[cfg(test)]
mod miter_tests;

#[cfg(test)]
mod outside_tests;

#[cfg(test)]
mod stroke_tests;
