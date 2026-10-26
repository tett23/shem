//! The Compact Font Format (Adobe TN 5176) and Type 2 charstrings (TN 5177).
//!
//! v0.1 reads the CFF that Golem embeds as `FontFile3 /CIDFontType0C`: one
//! font, CID-keyed or not, with Type 2 charstrings whose outlines it
//! interprets without hints (docs/adr/0004).

use std::collections::BTreeMap;

use super::Segment;
use crate::error::{Error, Result};
use crate::fixed::Fixed;

fn bad(what: &str) -> Error {
    Error::new(format!("malformed CFF: {what}"))
}

fn u8_at(d: &[u8], i: usize) -> Result<u8> {
    d.get(i).copied().ok_or_else(|| bad("unexpected end"))
}

fn u16_at(d: &[u8], i: usize) -> Result<u16> {
    Ok(u16::from(u8_at(d, i)?) << 8 | u16::from(u8_at(d, i + 1)?))
}

fn offset_at(d: &[u8], i: usize, size: usize) -> Result<usize> {
    (0..size).try_fold(
        0usize,
        |acc, k| Ok(acc << 8 | usize::from(u8_at(d, i + k)?)),
    )
}

/// An INDEX (TN 5176 5): its items and the offset after it.
pub fn parse_index(data: &[u8], offset: usize) -> Result<(Vec<&[u8]>, usize)> {
    let count = usize::from(u16_at(data, offset)?);
    if count == 0 {
        return Ok((Vec::new(), offset + 2));
    }
    let size = usize::from(u8_at(data, offset + 2)?);
    if !(1..=4).contains(&size) {
        return Err(bad("INDEX offset size"));
    }
    let offsets = (0..=count)
        .map(|i| offset_at(data, offset + 3 + i * size, size))
        .collect::<Result<Vec<_>>>()?;
    let base = offset + 3 + (count + 1) * size - 1;
    if offsets[0] != 1 || offsets.windows(2).any(|w| w[0] > w[1]) {
        return Err(bad("INDEX offsets"));
    }
    let end = base + offsets[count];
    if end > data.len() {
        return Err(bad("INDEX past the end"));
    }
    let items = offsets
        .windows(2)
        .map(|w| &data[base + w[0]..base + w[1]])
        .collect();
    Ok((items, end))
}

/// A real number of a DICT: nibbles of digits, a point, an exponent and a sign.
fn dict_real(data: &[u8], mut i: usize) -> Result<(Fixed, usize)> {
    let mut nibbles = Vec::new();
    'outer: loop {
        let b = u8_at(data, i)?;
        i += 1;
        for nib in [b >> 4, b & 0x0f] {
            if nib == 0x0f {
                break 'outer;
            }
            nibbles.push(nib);
        }
    }
    let mut negative = false;
    let mut digits = String::new();
    let mut point: Option<usize> = None;
    let mut exponent: Option<(bool, i32)> = None;
    for nib in nibbles {
        match (nib, exponent.as_mut()) {
            (0..=9, Some((_, e))) => {
                *e = e
                    .checked_mul(10)
                    .and_then(|e| e.checked_add(i32::from(nib)))
                    .ok_or_else(|| bad("real exponent"))?
            }
            (0..=9, None) => digits.push(char::from(b'0' + nib)),
            (0x0a, None) if point.is_none() => point = Some(digits.len()),
            (0x0b, None) => exponent = Some((false, 0)),
            (0x0c, None) => exponent = Some((true, 0)),
            (0x0e, None) if digits.is_empty() && !negative => negative = true,
            _ => return Err(bad("real number")),
        }
    }
    if digits.is_empty() {
        return Err(bad("real number without digits"));
    }
    let exp = exponent.map_or(0, |(neg, e)| if neg { -e } else { e });
    if exp.abs() > 40 {
        return Err(bad("real exponent"));
    }
    let pos = point.unwrap_or(digits.len()) as i32 + exp;
    let len = digits.len() as i32;
    let text = if pos <= 0 {
        format!("0.{}{}", "0".repeat((-pos) as usize), digits)
    } else if pos >= len {
        format!("{}{}", digits, "0".repeat((pos - len) as usize))
    } else {
        format!("{}.{}", &digits[..pos as usize], &digits[pos as usize..])
    };
    let v = Fixed::parse(text.as_bytes())?;
    Ok((if negative { v.checked_neg()? } else { v }, i))
}

/// A DICT (TN 5176 4): operator to operands. An escaped operator 12 x is 1200 + x.
pub fn parse_dict(data: &[u8]) -> Result<BTreeMap<u16, Vec<Fixed>>> {
    let mut dict = BTreeMap::new();
    let mut operands = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let int = |v: i64| Fixed::from_int(v);
        match b0 {
            0..=21 => {
                let op = if b0 == 12 {
                    i += 1;
                    1200 + u16::from(u8_at(data, i)?)
                } else {
                    u16::from(b0)
                };
                dict.insert(op, std::mem::take(&mut operands));
                i += 1;
            }
            28 => {
                operands.push(int(i64::from(u16_at(data, i + 1)? as i16))?);
                i += 3;
            }
            29 => {
                let v = (0..4).try_fold(0u32, |acc, k| {
                    Ok::<_, Error>(acc << 8 | u32::from(u8_at(data, i + 1 + k)?))
                })?;
                operands.push(int(i64::from(v as i32))?);
                i += 5;
            }
            30 => {
                let (v, next) = dict_real(data, i + 1)?;
                operands.push(v);
                i = next;
            }
            32..=246 => {
                operands.push(int(i64::from(b0) - 139)?);
                i += 1;
            }
            247..=250 => {
                operands.push(int((i64::from(b0) - 247) * 256
                    + i64::from(u8_at(data, i + 1)?)
                    + 108)?);
                i += 2;
            }
            251..=254 => {
                operands.push(int(-(i64::from(b0) - 251) * 256
                    - i64::from(u8_at(data, i + 1)?)
                    - 108)?);
                i += 2;
            }
            _ => return Err(bad("DICT operator")),
        }
    }
    Ok(dict)
}

/// The SID (or the CID of a CID-keyed font) of each glyph (TN 5176 13).
pub fn parse_charset(data: &[u8], offset: usize, glyphs: usize) -> Result<Vec<u16>> {
    let mut out = vec![0u16];
    match u8_at(data, offset)? {
        0 => {
            (1..glyphs).try_for_each(|g| {
                out.push(u16_at(data, offset + 1 + (g - 1) * 2)?);
                Ok::<_, Error>(())
            })?;
        }
        format @ (1 | 2) => {
            let mut i = offset + 1;
            while out.len() < glyphs {
                let first = u16_at(data, i)?;
                let left = if format == 1 {
                    u16::from(u8_at(data, i + 2)?)
                } else {
                    u16_at(data, i + 2)?
                };
                i += if format == 1 { 3 } else { 4 };
                (0..=left)
                    .map_while(|k| first.checked_add(k))
                    .take(glyphs - out.len())
                    .for_each(|v| out.push(v));
            }
        }
        _ => return Err(bad("charset format")),
    }
    Ok(out)
}

/// The Font DICT of each glyph (TN 5176 19).
pub fn parse_fd_select(data: &[u8], offset: usize, glyphs: usize) -> Result<Vec<u8>> {
    match u8_at(data, offset)? {
        0 => (0..glyphs).map(|g| u8_at(data, offset + 1 + g)).collect(),
        3 => {
            let ranges = usize::from(u16_at(data, offset + 1)?);
            let mut out = Vec::with_capacity(glyphs);
            (0..ranges).try_for_each(|r| {
                let at = offset + 3 + r * 3;
                let first = usize::from(u16_at(data, at)?);
                let fd = u8_at(data, at + 2)?;
                let next = usize::from(u16_at(data, at + 3)?);
                if first != out.len() || next < first {
                    return Err(bad("FDSelect ranges"));
                }
                out.extend(std::iter::repeat_n(fd, next - first));
                Ok(())
            })?;
            if out.len() != glyphs {
                return Err(bad("FDSelect does not cover the glyphs"));
            }
            Ok(out)
        }
        _ => Err(bad("FDSelect format")),
    }
}

/// The bias of subroutine numbers (TN 5177 4.7).
pub fn bias(count: usize) -> i64 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

const MAX_STACK: usize = 48;
const MAX_DEPTH: usize = 10;

/// The interpreter of a Type 2 charstring. Numbers are 16.16 fixed point.
struct Interpreter<'a> {
    global: &'a [Vec<u8>],
    local: &'a [Vec<u8>],
    stack: Vec<i64>,
    x: i64,
    y: i64,
    out: Vec<Segment>,
    open: bool,
    stems: usize,
    width_seen: bool,
    ended: bool,
}

fn fixed16(v: i64) -> Result<Fixed> {
    // 16.16 to Q32.32
    v.checked_mul(1 << 16)
        .map(Fixed::from_raw)
        .ok_or_else(Error::overflow)
}

impl Interpreter<'_> {
    fn point(&self) -> Result<(Fixed, Fixed)> {
        Ok((fixed16(self.x)?, fixed16(self.y)?))
    }

    /// Drops the width before the first stack-clearing operator: there is
    /// one when the number of arguments is `odd`-ly more than expected.
    fn width(&mut self, extra: bool) {
        if !self.width_seen {
            self.width_seen = true;
            if extra && !self.stack.is_empty() {
                self.stack.remove(0);
            }
        }
    }

    fn close(&mut self) {
        if self.open {
            self.out.push(Segment::Close);
            self.open = false;
        }
    }

    fn move_to(&mut self, dx: i64, dy: i64) -> Result<()> {
        self.close();
        self.x += dx;
        self.y += dy;
        let (x, y) = self.point()?;
        self.out.push(Segment::Move(x, y));
        self.open = true;
        Ok(())
    }

    fn line_to(&mut self, dx: i64, dy: i64) -> Result<()> {
        self.x += dx;
        self.y += dy;
        let (x, y) = self.point()?;
        self.out.push(Segment::Line(x, y));
        Ok(())
    }

    fn curve_to(&mut self, d: [i64; 6]) -> Result<()> {
        let (x1, y1) = (self.x + d[0], self.y + d[1]);
        let (x2, y2) = (x1 + d[2], y1 + d[3]);
        self.x = x2 + d[4];
        self.y = y2 + d[5];
        self.out.push(Segment::Curve([
            fixed16(x1)?,
            fixed16(y1)?,
            fixed16(x2)?,
            fixed16(y2)?,
            fixed16(self.x)?,
            fixed16(self.y)?,
        ]));
        Ok(())
    }

    fn args(&mut self) -> Vec<i64> {
        std::mem::take(&mut self.stack)
    }

    fn wrong(name: &str) -> Error {
        Error::new(format!(
            "wrong number of arguments of the charstring operator {name}"
        ))
    }

    fn run(&mut self, code: &[u8], depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(Error::new("charstring subroutine calls too deep"));
        }
        let mut i = 0;
        while i < code.len() {
            let b0 = code[i];
            let push = |v: i64, stack: &mut Vec<i64>| {
                if stack.len() >= MAX_STACK {
                    Err(Error::new("charstring stack overflow"))
                } else {
                    stack.push(v);
                    Ok(())
                }
            };
            match b0 {
                32..=246 => {
                    push((i64::from(b0) - 139) << 16, &mut self.stack)?;
                    i += 1;
                }
                247..=250 => {
                    let v = (i64::from(b0) - 247) * 256 + i64::from(u8_at(code, i + 1)?) + 108;
                    push(v << 16, &mut self.stack)?;
                    i += 2;
                }
                251..=254 => {
                    let v = -(i64::from(b0) - 251) * 256 - i64::from(u8_at(code, i + 1)?) - 108;
                    push(v << 16, &mut self.stack)?;
                    i += 2;
                }
                28 => {
                    push(
                        i64::from(u16_at(code, i + 1)? as i16) << 16,
                        &mut self.stack,
                    )?;
                    i += 3;
                }
                255 => {
                    let v = (0..4).try_fold(0u32, |acc, k| {
                        Ok::<_, Error>(acc << 8 | u32::from(u8_at(code, i + 1 + k)?))
                    })?;
                    push(i64::from(v as i32), &mut self.stack)?;
                    i += 5;
                }
                12 => {
                    let op = u8_at(code, i + 1)?;
                    i += 2;
                    self.escaped(op)?;
                }
                _ => {
                    i += 1;
                    match b0 {
                        19 | 20 => {
                            // hintmask, cntrmask: operands here are an implicit vstem
                            self.width(self.stack.len() % 2 == 1);
                            self.stems += self.stack.len() / 2;
                            self.stack.clear();
                            i += self.stems.div_ceil(8);
                        }
                        10 | 29 => {
                            let index =
                                self.stack.pop().ok_or_else(|| Self::wrong("callsubr"))? >> 16;
                            let subrs = if b0 == 10 { self.local } else { self.global };
                            let n = usize::try_from(index + bias(subrs.len()))
                                .ok()
                                .filter(|&n| n < subrs.len())
                                .ok_or_else(|| {
                                    Error::new(
                                        "call of a charstring subroutine that does not exist",
                                    )
                                })?;
                            self.run(&subrs[n], depth + 1)?;
                            if self.ended {
                                return Ok(());
                            }
                        }
                        11 => return Ok(()),
                        14 => {
                            self.width(self.stack.len() % 2 == 1);
                            if self.stack.len() >= 4 {
                                return Err(Error::new(
                                    "unsupported endchar with the arguments of seac",
                                ));
                            }
                            self.stack.clear();
                            self.close();
                            self.ended = true;
                            return Ok(());
                        }
                        op => self.operator(op)?,
                    }
                }
            }
        }
        Ok(())
    }

    fn operator(&mut self, op: u8) -> Result<()> {
        match op {
            1 | 3 | 18 | 23 => {
                // hstem, vstem, hstemhm, vstemhm
                self.width(self.stack.len() % 2 == 1);
                self.stems += self.stack.len() / 2;
                self.stack.clear();
            }
            21 => {
                self.width(self.stack.len() > 2);
                match self.args().as_slice() {
                    [dx, dy] => self.move_to(*dx, *dy)?,
                    _ => return Err(Self::wrong("rmoveto")),
                }
            }
            22 | 4 => {
                self.width(self.stack.len() > 1);
                match (op, self.args().as_slice()) {
                    (22, [dx]) => self.move_to(*dx, 0)?,
                    (4, [dy]) => self.move_to(0, *dy)?,
                    _ => return Err(Self::wrong("hmoveto or vmoveto")),
                }
            }
            5 => {
                let a = self.args();
                if a.is_empty() || !a.len().is_multiple_of(2) {
                    return Err(Self::wrong("rlineto"));
                }
                a.chunks(2).try_for_each(|p| self.line_to(p[0], p[1]))?;
            }
            6 | 7 => {
                let a = self.args();
                if a.is_empty() {
                    return Err(Self::wrong("hlineto or vlineto"));
                }
                a.iter().enumerate().try_for_each(|(k, &d)| {
                    if (k % 2 == 0) == (op == 6) {
                        self.line_to(d, 0)
                    } else {
                        self.line_to(0, d)
                    }
                })?;
            }
            8 => {
                let a = self.args();
                if a.is_empty() || !a.len().is_multiple_of(6) {
                    return Err(Self::wrong("rrcurveto"));
                }
                a.chunks(6)
                    .try_for_each(|c| self.curve_to([c[0], c[1], c[2], c[3], c[4], c[5]]))?;
            }
            27 | 26 => {
                // hhcurveto, vvcurveto
                let a = self.args();
                let (first, rest) = if a.len() % 4 == 1 {
                    (a[0], &a[1..])
                } else {
                    (0, &a[..])
                };
                if rest.is_empty() || rest.len() % 4 != 0 {
                    return Err(Self::wrong("hhcurveto or vvcurveto"));
                }
                rest.chunks(4).enumerate().try_for_each(|(k, c)| {
                    let extra = if k == 0 { first } else { 0 };
                    if op == 27 {
                        self.curve_to([c[0], extra, c[1], c[2], c[3], 0])
                    } else {
                        self.curve_to([extra, c[0], c[1], c[2], 0, c[3]])
                    }
                })?;
            }
            31 | 30 => {
                // hvcurveto, vhcurveto
                let a = self.args();
                if a.len() < 4 || !(a.len().is_multiple_of(4) || a.len() % 4 == 1) {
                    return Err(Self::wrong("hvcurveto or vhcurveto"));
                }
                let curves = a.len() / 4;
                (0..curves).try_for_each(|k| {
                    let c = &a[k * 4..k * 4 + 4];
                    let last = if k == curves - 1 && a.len() % 4 == 1 {
                        a[a.len() - 1]
                    } else {
                        0
                    };
                    let horizontal = (k % 2 == 0) == (op == 31);
                    if horizontal {
                        self.curve_to([c[0], 0, c[1], c[2], last, c[3]])
                    } else {
                        self.curve_to([0, c[0], c[1], c[2], c[3], last])
                    }
                })?;
            }
            24 => {
                let a = self.args();
                if a.len() < 8 || !(a.len() - 2).is_multiple_of(6) {
                    return Err(Self::wrong("rcurveline"));
                }
                let (curves, line) = a.split_at(a.len() - 2);
                curves
                    .chunks(6)
                    .try_for_each(|c| self.curve_to([c[0], c[1], c[2], c[3], c[4], c[5]]))?;
                self.line_to(line[0], line[1])?;
            }
            25 => {
                let a = self.args();
                if a.len() < 8 || !(a.len() - 6).is_multiple_of(2) {
                    return Err(Self::wrong("rlinecurve"));
                }
                let (lines, c) = a.split_at(a.len() - 6);
                lines.chunks(2).try_for_each(|p| self.line_to(p[0], p[1]))?;
                self.curve_to([c[0], c[1], c[2], c[3], c[4], c[5]])?;
            }
            _ => return Err(Error::new(format!("unsupported charstring operator {op}"))),
        }
        Ok(())
    }

    fn escaped(&mut self, op: u8) -> Result<()> {
        let a = self.args();
        match (op, a.as_slice()) {
            // dotsection, deprecated: no effect
            (0, _) => {}
            (35, [d @ .., _fd]) if d.len() == 12 => {
                self.curve_to([d[0], d[1], d[2], d[3], d[4], d[5]])?;
                self.curve_to([d[6], d[7], d[8], d[9], d[10], d[11]])?;
            }
            (34, [dx1, dx2, dy2, dx3, dx4, dx5, dx6]) => {
                self.curve_to([*dx1, 0, *dx2, *dy2, *dx3, 0])?;
                self.curve_to([*dx4, 0, *dx5, -*dy2, *dx6, 0])?;
            }
            (36, [dx1, dy1, dx2, dy2, dx3, dx4, dx5, dy5, dx6]) => {
                let y0 = self.y;
                self.curve_to([*dx1, *dy1, *dx2, *dy2, *dx3, 0])?;
                let dy6 = y0 - (self.y + dy5);
                self.curve_to([*dx4, 0, *dx5, *dy5, *dx6, dy6])?;
            }
            (37, d) if d.len() == 11 => {
                let (x0, y0) = (self.x, self.y);
                let dx: i64 = d[..10].iter().step_by(2).sum();
                let dy: i64 = d[1..10].iter().step_by(2).sum();
                self.curve_to([d[0], d[1], d[2], d[3], d[4], d[5]])?;
                let (x5, y5) = (self.x + d[6] + d[8], self.y + d[7] + d[9]);
                let (dx6, dy6) = if dx.abs() > dy.abs() {
                    (d[10], y0 - y5)
                } else {
                    (x0 - x5, d[10])
                };
                self.curve_to([d[6], d[7], d[8], d[9], dx6, dy6])?;
            }
            (34..=37, _) => return Err(Self::wrong("flex")),
            _ => {
                return Err(Error::new(format!(
                    "unsupported charstring operator 12 {op}"
                )));
            }
        }
        Ok(())
    }
}

/// The outline of a Type 2 charstring, without hints.
pub fn charstring_outline(
    code: &[u8],
    global: &[Vec<u8>],
    local: &[Vec<u8>],
) -> Result<Vec<Segment>> {
    let mut it = Interpreter {
        global,
        local,
        stack: Vec::new(),
        x: 0,
        y: 0,
        out: Vec::new(),
        open: false,
        stems: 0,
        width_seen: false,
        ended: false,
    };
    it.run(code, 0)?;
    if !it.ended {
        return Err(Error::new("charstring without endchar"));
    }
    Ok(it.out)
}

/// A CFF font.
pub struct Cff {
    charstrings: Vec<Vec<u8>>,
    global: Vec<Vec<u8>>,
    /// The local subroutines of each Font DICT (one for a font that is not CID-keyed).
    local: Vec<Vec<Vec<u8>>>,
    /// The Font DICT of each glyph, for a CID-keyed font.
    fd_select: Option<Vec<u8>>,
    /// CID to glyph ID, for a CID-keyed font.
    cid_to_gid: Option<BTreeMap<u16, u16>>,
}

fn dict_offset(dict: &BTreeMap<u16, Vec<Fixed>>, op: u16, index: usize) -> Result<Option<usize>> {
    match dict.get(&op).and_then(|v| v.get(index)) {
        None => Ok(None),
        Some(v) if v.raw() >= 0 && v.raw() & 0xffff_ffff == 0 => Ok(Some(v.floor() as usize)),
        Some(_) => Err(bad("DICT offset")),
    }
}

/// The local subroutines of a Private DICT given as `[size offset]`.
fn local_subrs(data: &[u8], dict: &BTreeMap<u16, Vec<Fixed>>) -> Result<Vec<Vec<u8>>> {
    let (Some(size), Some(offset)) = (dict_offset(dict, 18, 0)?, dict_offset(dict, 18, 1)?) else {
        return Ok(Vec::new());
    };
    let private = data
        .get(offset..offset + size)
        .ok_or_else(|| bad("Private DICT"))?;
    let private = parse_dict(private)?;
    match dict_offset(&private, 19, 0)? {
        None => Ok(Vec::new()),
        Some(subrs) => Ok(parse_index(data, offset + subrs)?
            .0
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect()),
    }
}

fn font_matrix(dict: &BTreeMap<u16, Vec<Fixed>>) -> Result<Option<crate::fixed::Matrix>> {
    match dict.get(&1207) {
        None => Ok(None),
        Some(v) if v.len() == 6 => Ok(Some(crate::fixed::Matrix::new([
            v[0], v[1], v[2], v[3], v[4], v[5],
        ]))),
        Some(_) => Err(bad("FontMatrix")),
    }
}

impl Cff {
    pub fn parse(data: &[u8]) -> Result<Cff> {
        let header_size = usize::from(u8_at(data, 2)?);
        let (names, at) = parse_index(data, header_size)?;
        if names.len() != 1 {
            return Err(Error::new("unsupported CFF with more than one font"));
        }
        let (tops, at) = parse_index(data, at)?;
        let (_strings, at) = parse_index(data, at)?;
        let (global, _) = parse_index(data, at)?;
        let top = parse_dict(tops.first().ok_or_else(|| bad("no Top DICT"))?)?;
        if let Some(t) = top.get(&1206).and_then(|v| v.first())
            && *t != Fixed::from_int(2)?
        {
            return Err(Error::new(format!("unsupported CharstringType {t}")));
        }
        let charstrings_at = dict_offset(&top, 17, 0)?.ok_or_else(|| bad("no CharStrings"))?;
        let charstrings: Vec<Vec<u8>> = parse_index(data, charstrings_at)?
            .0
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect();
        let glyphs = charstrings.len();
        let thousandth = crate::fixed::Matrix::new([
            Fixed::parse(b"0.001")?,
            Fixed::ZERO,
            Fixed::ZERO,
            Fixed::parse(b"0.001")?,
            Fixed::ZERO,
            Fixed::ZERO,
        ]);
        let top_matrix = font_matrix(&top)?.unwrap_or(thousandth);
        let check_matrix = |m: crate::fixed::Matrix| {
            if m == thousandth {
                Ok(())
            } else {
                Err(Error::new(
                    "unsupported CFF FontMatrix other than [0.001 0 0 0.001 0 0]",
                ))
            }
        };
        let global: Vec<Vec<u8>> = global.into_iter().map(<[u8]>::to_vec).collect();
        if top.contains_key(&1230) {
            // CID-keyed: ROS, charset, FDArray and FDSelect
            let charset_at = dict_offset(&top, 15, 0)?
                .filter(|&o| o > 2)
                .ok_or_else(|| bad("no charset"))?;
            let cids = parse_charset(data, charset_at, glyphs)?;
            let cid_to_gid = cids
                .iter()
                .enumerate()
                .rev()
                .map(|(gid, &cid)| (cid, gid as u16))
                .collect::<BTreeMap<_, _>>();
            let fd_array_at = dict_offset(&top, 1236, 0)?.ok_or_else(|| bad("no FDArray"))?;
            let fds = parse_index(data, fd_array_at)?
                .0
                .into_iter()
                .map(parse_dict)
                .collect::<Result<Vec<_>>>()?;
            fds.iter().try_for_each(|fd| match font_matrix(fd)? {
                None => check_matrix(top_matrix),
                Some(m) => check_matrix(m.concat(&top_matrix)?),
            })?;
            let local = fds
                .iter()
                .map(|fd| local_subrs(data, fd))
                .collect::<Result<Vec<_>>>()?;
            let fd_select_at = dict_offset(&top, 1237, 0)?.ok_or_else(|| bad("no FDSelect"))?;
            let fd_select = parse_fd_select(data, fd_select_at, glyphs)?;
            if fd_select.iter().any(|&fd| usize::from(fd) >= local.len()) {
                return Err(bad("FDSelect refers to a missing Font DICT"));
            }
            Ok(Cff {
                charstrings,
                global,
                local,
                fd_select: Some(fd_select),
                cid_to_gid: Some(cid_to_gid),
            })
        } else {
            check_matrix(top_matrix)?;
            let local = vec![local_subrs(data, &top)?];
            Ok(Cff {
                charstrings,
                global,
                local,
                fd_select: None,
                cid_to_gid: None,
            })
        }
    }

    pub fn glyph_count(&self) -> usize {
        self.charstrings.len()
    }

    pub fn is_cid_keyed(&self) -> bool {
        self.cid_to_gid.is_some()
    }

    /// The glyph ID of a CID: by the charset in a CID-keyed font, the CID
    /// itself otherwise (ISO 32000-1 9.7.4.2).
    pub fn gid(&self, cid: u16) -> Result<u16> {
        let gid = match &self.cid_to_gid {
            Some(map) => *map
                .get(&cid)
                .ok_or_else(|| Error::new(format!("CID {cid} is not in the charset")))?,
            None => cid,
        };
        if usize::from(gid) >= self.glyph_count() {
            return Err(Error::new(format!("glyph {gid} is not in the font")));
        }
        Ok(gid)
    }

    /// The outline of a glyph, in units of the FontMatrix (1/1000 em).
    pub fn outline(&self, gid: u16) -> Result<Vec<Segment>> {
        let g = usize::from(gid);
        let code = self
            .charstrings
            .get(g)
            .ok_or_else(|| Error::new(format!("glyph {gid} is not in the font")))?;
        let fd = self.fd_select.as_ref().map_or(0, |s| usize::from(s[g]));
        charstring_outline(code, &self.global, &self.local[fd])
    }
}

#[cfg(test)]
mod tests;
