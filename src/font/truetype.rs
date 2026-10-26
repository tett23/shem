//! The outlines of TrueType fonts: `head`, `maxp`, `loca` and `glyf`
//! (OpenType specification), without hinting (docs/adr/0004).

use super::Segment;
use crate::error::{Error, Result};
use crate::fixed::{Fixed, Matrix, div_round};

fn bad(what: &str) -> Error {
    Error::new(format!("malformed TrueType font: {what}"))
}

fn u8_at(d: &[u8], i: usize) -> Result<u8> {
    d.get(i).copied().ok_or_else(|| bad("unexpected end"))
}

fn u16_at(d: &[u8], i: usize) -> Result<u16> {
    Ok(u16::from(u8_at(d, i)?) << 8 | u16::from(u8_at(d, i + 1)?))
}

fn i16_at(d: &[u8], i: usize) -> Result<i16> {
    Ok(u16_at(d, i)? as i16)
}

fn u32_at(d: &[u8], i: usize) -> Result<u32> {
    Ok(u32::from(u16_at(d, i)?) << 16 | u32::from(u16_at(d, i + 2)?))
}

const MAX_DEPTH: usize = 8;

pub struct TrueType {
    units_per_em: u16,
    /// The offsets of the glyphs in `glyf`, one more than the glyphs.
    loca: Vec<usize>,
    glyf: Vec<u8>,
}

/// A point of a contour, in font units, and whether it is on the curve.
#[derive(Clone, Copy)]
struct Point {
    x: Fixed,
    y: Fixed,
    on: bool,
}

fn mid(a: Point, b: Point) -> Result<Point> {
    let m = |p: Fixed, q: Fixed| Fixed::from_wide((i128::from(p.raw()) + i128::from(q.raw())) / 2);
    Ok(Point {
        x: m(a.x, b.x)?,
        y: m(a.y, b.y)?,
        on: true,
    })
}

/// The cubic Bézier curve of the quadratic `p0 q p2`, its control points
/// `p0 + 2(q − p0)/3` and `p2 + 2(q − p2)/3` rounded once.
fn cubic(p0: Point, q: Point, p2: Point) -> Result<Segment> {
    let third = |a: Fixed, b: Fixed| {
        Fixed::from_wide(
            i128::from(a.raw()) + div_round(2 * (i128::from(b.raw()) - i128::from(a.raw())), 3),
        )
    };
    Ok(Segment::Curve([
        third(p0.x, q.x)?,
        third(p0.y, q.y)?,
        third(p2.x, q.x)?,
        third(p2.y, q.y)?,
        p2.x,
        p2.y,
    ]))
}

/// The segments of a closed contour of quadratic points.
fn contour(points: &[Point]) -> Result<Vec<Segment>> {
    if points.is_empty() {
        return Ok(Vec::new());
    }
    let (start, sequence): (Point, Vec<Point>) = match points.iter().position(|p| p.on) {
        Some(s) => (
            points[s],
            points[s + 1..]
                .iter()
                .chain(&points[..s])
                .copied()
                .collect(),
        ),
        None if points.len() == 1 => (
            Point {
                on: true,
                ..points[0]
            },
            Vec::new(),
        ),
        None => (
            mid(points[0], points[1])?,
            points[1..].iter().chain(&points[..1]).copied().collect(),
        ),
    };
    let mut out = vec![Segment::Move(start.x, start.y)];
    let mut current = start;
    let mut control: Option<Point> = None;
    for q in sequence.into_iter().chain(std::iter::once(start)) {
        match (q.on, control) {
            (true, Some(c)) => {
                out.push(cubic(current, c, q)?);
                current = q;
                control = None;
            }
            (true, None) => {
                out.push(Segment::Line(q.x, q.y));
                current = q;
            }
            (false, Some(c)) => {
                let m = mid(c, q)?;
                out.push(cubic(current, c, m)?);
                current = m;
                control = Some(q);
            }
            (false, None) => control = Some(q),
        }
    }
    // The last line back to the start is the closing of the contour.
    if out.last() == Some(&Segment::Line(start.x, start.y)) && out.len() > 1 {
        out.pop();
    }
    out.push(Segment::Close);
    Ok(out)
}

/// An F2Dot14 number as Q32.32.
fn f2dot14(v: i16) -> Fixed {
    Fixed::from_raw(i64::from(v) << 18)
}

fn transform(segments: Vec<Segment>, m: &Matrix) -> Result<Vec<Segment>> {
    let t = |x: Fixed, y: Fixed| m.apply(x, y);
    segments
        .into_iter()
        .map(|s| {
            Ok(match s {
                Segment::Move(x, y) => {
                    let (x, y) = t(x, y)?;
                    Segment::Move(x, y)
                }
                Segment::Line(x, y) => {
                    let (x, y) = t(x, y)?;
                    Segment::Line(x, y)
                }
                Segment::Curve(c) => {
                    let (x1, y1) = t(c[0], c[1])?;
                    let (x2, y2) = t(c[2], c[3])?;
                    let (x3, y3) = t(c[4], c[5])?;
                    Segment::Curve([x1, y1, x2, y2, x3, y3])
                }
                Segment::Close => Segment::Close,
            })
        })
        .collect()
}

impl TrueType {
    pub fn parse(data: &[u8]) -> Result<TrueType> {
        match data.get(..4) {
            Some([0, 1, 0, 0]) | Some(b"true") => {}
            Some(b"OTTO") => return Err(Error::new("unsupported FontFile2 with CFF outlines")),
            Some(b"ttcf") => return Err(Error::new("unsupported TrueType collection")),
            _ => return Err(bad("sfnt version")),
        }
        let tables = usize::from(u16_at(data, 4)?);
        let table = |tag: &[u8; 4]| -> Result<&[u8]> {
            (0..tables)
                .map(|i| 12 + i * 16)
                .find(|&at| data.get(at..at + 4) == Some(tag.as_slice()))
                .ok_or_else(|| bad(&format!("no {} table", String::from_utf8_lossy(tag))))
                .and_then(|at| {
                    let offset = u32_at(data, at + 8)? as usize;
                    let length = u32_at(data, at + 12)? as usize;
                    data.get(offset..offset + length)
                        .ok_or_else(|| bad("table past the end"))
                })
        };
        let head = table(b"head")?;
        let units_per_em = u16_at(head, 18)?;
        if units_per_em == 0 {
            return Err(bad("unitsPerEm"));
        }
        let long = match i16_at(head, 50)? {
            0 => false,
            1 => true,
            _ => return Err(bad("indexToLocFormat")),
        };
        let glyphs = usize::from(u16_at(table(b"maxp")?, 4)?);
        let loca_table = table(b"loca")?;
        let loca = (0..=glyphs)
            .map(|i| {
                if long {
                    u32_at(loca_table, i * 4).map(|v| v as usize)
                } else {
                    u16_at(loca_table, i * 2).map(|v| usize::from(v) * 2)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let glyf = table(b"glyf")?.to_vec();
        if loca.windows(2).any(|w| w[0] > w[1]) || loca.last().is_some_and(|&e| e > glyf.len()) {
            return Err(bad("loca"));
        }
        Ok(TrueType {
            units_per_em,
            loca,
            glyf,
        })
    }

    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    pub fn glyph_count(&self) -> usize {
        self.loca.len() - 1
    }

    /// The outline of a glyph, in font units.
    pub fn outline(&self, gid: u16) -> Result<Vec<Segment>> {
        self.glyph(gid, 0)
    }

    fn glyph(&self, gid: u16, depth: usize) -> Result<Vec<Segment>> {
        if depth > MAX_DEPTH {
            return Err(Error::new("composite glyphs nested too deep"));
        }
        let g = usize::from(gid);
        if g >= self.glyph_count() {
            return Err(Error::new(format!("glyph {gid} is not in the font")));
        }
        let data = &self.glyf[self.loca[g]..self.loca[g + 1]];
        if data.is_empty() {
            return Ok(Vec::new());
        }
        let contours = i16_at(data, 0)?;
        if contours >= 0 {
            simple(data, contours as usize)
        } else {
            self.composite(data, depth)
        }
    }

    fn composite(&self, data: &[u8], depth: usize) -> Result<Vec<Segment>> {
        let mut out = Vec::new();
        let mut i = 10;
        loop {
            let flags = u16_at(data, i)?;
            let component = u16_at(data, i + 2)?;
            i += 4;
            if flags & 0x0002 == 0 {
                return Err(Error::new(
                    "unsupported composite glyph that matches points",
                ));
            }
            if flags & 0x0800 != 0 {
                return Err(Error::new(
                    "unsupported composite glyph with SCALED_COMPONENT_OFFSET",
                ));
            }
            let (dx, dy) = if flags & 0x0001 != 0 {
                let v = (i16_at(data, i)?, i16_at(data, i + 2)?);
                i += 4;
                (i64::from(v.0), i64::from(v.1))
            } else {
                let v = (u8_at(data, i)? as i8, u8_at(data, i + 1)? as i8);
                i += 2;
                (i64::from(v.0), i64::from(v.1))
            };
            let (a, b, c, d) = if flags & 0x0008 != 0 {
                let s = f2dot14(i16_at(data, i)?);
                i += 2;
                (s, Fixed::ZERO, Fixed::ZERO, s)
            } else if flags & 0x0040 != 0 {
                let (sx, sy) = (f2dot14(i16_at(data, i)?), f2dot14(i16_at(data, i + 2)?));
                i += 4;
                (sx, Fixed::ZERO, Fixed::ZERO, sy)
            } else if flags & 0x0080 != 0 {
                let v = (0..4)
                    .map(|k| i16_at(data, i + 2 * k).map(f2dot14))
                    .collect::<Result<Vec<_>>>()?;
                i += 8;
                (v[0], v[1], v[2], v[3])
            } else {
                (Fixed::ONE, Fixed::ZERO, Fixed::ZERO, Fixed::ONE)
            };
            // x' = xscale x + scale10 y + dx, y' = scale01 x + yscale y + dy
            let m = Matrix::new([a, b, c, d, Fixed::from_int(dx)?, Fixed::from_int(dy)?]);
            out.extend(transform(self.glyph(component, depth + 1)?, &m)?);
            if flags & 0x0020 == 0 {
                return Ok(out);
            }
        }
    }
}

/// The outline of a simple glyph.
fn simple(data: &[u8], contours: usize) -> Result<Vec<Segment>> {
    let ends = (0..contours)
        .map(|k| u16_at(data, 10 + 2 * k).map(usize::from))
        .collect::<Result<Vec<_>>>()?;
    if ends.windows(2).any(|w| w[0] >= w[1]) {
        return Err(bad("contour end points"));
    }
    let count = ends.last().map_or(0, |&e| e + 1);
    let instructions = usize::from(u16_at(data, 10 + 2 * contours)?);
    let mut i = 12 + 2 * contours + instructions;
    let mut flags = Vec::with_capacity(count);
    while flags.len() < count {
        let f = u8_at(data, i)?;
        i += 1;
        let repeat = if f & 0x08 != 0 {
            i += 1;
            usize::from(u8_at(data, i - 1)?)
        } else {
            0
        };
        flags.extend(std::iter::repeat_n(f, repeat + 1));
    }
    flags.truncate(count);
    let mut coordinates = |short: u8, same: u8| -> Result<Vec<i64>> {
        let mut v = 0i64;
        flags
            .iter()
            .map(|&f| {
                let delta = if f & short != 0 {
                    let b = i64::from(u8_at(data, i)?);
                    i += 1;
                    if f & same != 0 { b } else { -b }
                } else if f & same != 0 {
                    0
                } else {
                    let d = i64::from(i16_at(data, i)?);
                    i += 2;
                    d
                };
                v += delta;
                Ok(v)
            })
            .collect()
    };
    let xs = coordinates(0x02, 0x10)?;
    let ys = coordinates(0x04, 0x20)?;
    let points = flags
        .iter()
        .zip(xs.iter().zip(&ys))
        .map(|(&f, (&x, &y))| {
            Ok(Point {
                x: Fixed::from_int(x)?,
                y: Fixed::from_int(y)?,
                on: f & 0x01 != 0,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let starts = std::iter::once(0).chain(ends.iter().map(|e| e + 1));
    starts
        .zip(&ends)
        .map(|(s, &e)| contour(&points[s..=e]))
        .collect::<Result<Vec<_>>>()
        .map(|v| v.concat())
}

#[cfg(test)]
pub(crate) mod tests;
