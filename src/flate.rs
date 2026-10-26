//! Flate: the zlib format (RFC 1950) and Deflate (RFC 1951).
//!
//! The decoder follows the canonical-code decoding of puff.c of zlib: a
//! Huffman code is held as the number of codes of each length and the
//! symbols in code order, and read one bit at a time.

use crate::error::{Error, Result};

const MAX_BITS: usize = 15;

/// The Adler-32 checksum (RFC 1950 8.2).
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    // 5552 is the largest n for which the sums cannot overflow u32 (zlib's NMAX).
    let (a, b) = data.chunks(5552).fold((1u32, 0u32), |(a, b), chunk| {
        let (a, b) = chunk.iter().fold((a, b), |(a, b), &c| {
            let a = a + u32::from(c);
            (a, b + a)
        });
        (a % MOD, b % MOD)
    });
    (b << 16) | a
}

fn corrupt(what: &str) -> Error {
    Error::new(format!("corrupt Flate data: {what}"))
}

/// Inflates a zlib stream and checks its Adler-32.
pub fn inflate_zlib(data: &[u8]) -> Result<Vec<u8>> {
    let (cmf, flg) = match data {
        [cmf, flg, ..] => (*cmf, *flg),
        _ => return Err(corrupt("no zlib header")),
    };
    if cmf & 0x0f != 8 || cmf >> 4 > 7 {
        return Err(corrupt("not the Deflate method"));
    }
    if (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 {
        return Err(corrupt("wrong header check"));
    }
    if flg & 0x20 != 0 {
        return Err(Error::new(
            "unsupported Flate data with a preset dictionary",
        ));
    }
    let mut input = BitReader::new(&data[2..]);
    let out = inflate(&mut input)?;
    let end = 2 + input.byte_position();
    let check = data
        .get(end..end + 4)
        .ok_or_else(|| corrupt("no Adler-32"))?;
    if u32::from_be_bytes([check[0], check[1], check[2], check[3]]) != adler32(&out) {
        return Err(corrupt("wrong Adler-32"));
    }
    Ok(out)
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader {
            data,
            pos: 0,
            bit: 0,
        }
    }

    fn bit(&mut self) -> Result<u32> {
        let byte = *self
            .data
            .get(self.pos)
            .ok_or_else(|| corrupt("unexpected end"))?;
        let v = u32::from(byte >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.pos += 1;
        }
        Ok(v)
    }

    /// `count` bits, least significant first.
    fn bits(&mut self, count: u32) -> Result<u32> {
        (0..count).try_fold(0, |acc, i| Ok(acc | (self.bit()? << i)))
    }

    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self
            .data
            .get(self.pos..self.pos + n)
            .ok_or_else(|| corrupt("unexpected end"))?;
        self.pos += n;
        Ok(s)
    }

    /// The position of the first byte after the bits read so far.
    fn byte_position(&self) -> usize {
        self.pos + usize::from(self.bit != 0)
    }
}

/// A canonical Huffman code.
struct Huffman {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl Huffman {
    /// The code from the length of each symbol's code (0: no code). An
    /// over-subscribed code is an error; an incomplete code is allowed only
    /// when it has a single code, as puff.c allows.
    fn new(lengths: &[u8]) -> Result<Huffman> {
        let counts = lengths.iter().fold([0u16; MAX_BITS + 1], |mut c, &l| {
            c[usize::from(l)] += 1;
            c
        });
        let left = (1..=MAX_BITS).try_fold(1i32, |left, len| {
            let left = left * 2 - i32::from(counts[len]);
            (left >= 0).then_some(left)
        });
        let left = left.ok_or_else(|| corrupt("over-subscribed Huffman code"))?;
        let used: u16 = counts[1..].iter().sum();
        if left > 0 && used > 1 {
            return Err(corrupt("incomplete Huffman code"));
        }
        let symbols = (1..=MAX_BITS)
            .flat_map(|len| {
                lengths
                    .iter()
                    .enumerate()
                    .filter(move |&(_, &l)| usize::from(l) == len)
                    .map(|(s, _)| s as u16)
            })
            .collect();
        Ok(Huffman { counts, symbols })
    }

    fn decode(&self, input: &mut BitReader) -> Result<u16> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..=MAX_BITS {
            code |= input.bit()? as i32;
            let count = i32::from(self.counts[len]);
            if code - count < first {
                return Ok(self.symbols[(index + code - first) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(corrupt("invalid Huffman code"))
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// The order of the code length code lengths (RFC 1951 3.2.7).
const CLEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn inflate(input: &mut BitReader) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let last = input.bit()? == 1;
        match input.bits(2)? {
            0 => stored(input, &mut out)?,
            1 => {
                let (lit, dist) = fixed_codes()?;
                codes(input, &mut out, &lit, &dist)?
            }
            2 => {
                let (lit, dist) = dynamic_codes(input)?;
                codes(input, &mut out, &lit, &dist)?
            }
            _ => return Err(corrupt("invalid block type")),
        }
        if last {
            return Ok(out);
        }
    }
}

fn stored(input: &mut BitReader, out: &mut Vec<u8>) -> Result<()> {
    input.align();
    let header = input.bytes(4)?;
    let len = u16::from_le_bytes([header[0], header[1]]);
    let nlen = u16::from_le_bytes([header[2], header[3]]);
    if len != !nlen {
        return Err(corrupt("stored block length does not match its complement"));
    }
    out.extend_from_slice(input.bytes(usize::from(len))?);
    Ok(())
}

fn fixed_codes() -> Result<(Huffman, Huffman)> {
    let lit: Vec<u8> = (0..288)
        .map(|s| match s {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        })
        .collect();
    // 32 distance codes make the code complete; 30 and 31 are rejected when read.
    Ok((Huffman::new(&lit)?, Huffman::new(&[5; 32])?))
}

fn dynamic_codes(input: &mut BitReader) -> Result<(Huffman, Huffman)> {
    let nlen = input.bits(5)? as usize + 257;
    let ndist = input.bits(5)? as usize + 1;
    let ncode = input.bits(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(corrupt("too many length or distance codes"));
    }
    let mut clen = [0u8; 19];
    for &i in &CLEN_ORDER[..ncode] {
        clen[i] = input.bits(3)? as u8;
    }
    let clen_code = Huffman::new(&clen)?;
    let mut lengths: Vec<u8> = Vec::with_capacity(nlen + ndist);
    while lengths.len() < nlen + ndist {
        let sym = clen_code.decode(input)?;
        let (value, repeat) = match sym {
            0..=15 => (sym as u8, 1),
            16 => {
                let prev = *lengths
                    .last()
                    .ok_or_else(|| corrupt("repeat with no previous length"))?;
                (prev, 3 + input.bits(2)?)
            }
            17 => (0, 3 + input.bits(3)?),
            _ => (0, 11 + input.bits(7)?),
        };
        if lengths.len() + repeat as usize > nlen + ndist {
            return Err(corrupt("too many code lengths"));
        }
        lengths.extend(std::iter::repeat_n(value, repeat as usize));
    }
    if lengths[256] == 0 {
        return Err(corrupt("no end-of-block code"));
    }
    Ok((
        Huffman::new(&lengths[..nlen])?,
        Huffman::new(&lengths[nlen..])?,
    ))
}

fn codes(input: &mut BitReader, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman) -> Result<()> {
    loop {
        let sym = lit.decode(input)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let i = usize::from(sym - 257);
                let len =
                    usize::from(LENGTH_BASE[i]) + input.bits(u32::from(LENGTH_EXTRA[i]))? as usize;
                let d = usize::from(dist.decode(input)?);
                if d >= 30 {
                    return Err(corrupt("invalid distance code"));
                }
                let distance =
                    usize::from(DIST_BASE[d]) + input.bits(u32::from(DIST_EXTRA[d]))? as usize;
                if distance > out.len() {
                    return Err(corrupt("distance beyond the output"));
                }
                let start = out.len() - distance;
                // The copy may overlap its own output, so it goes byte by byte.
                (0..len).for_each(|k| out.push(out[start + k]));
            }
            _ => return Err(corrupt("invalid literal/length code")),
        }
    }
}

#[cfg(test)]
mod tests;
