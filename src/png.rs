//! PNG and PPM files, and the Deflate compression for PNG (docs/adr/0007).
//!
//! The bytes of a PNG depend only on the pixels and the version of shem:
//! the chunks are IHDR, one IDAT and IEND; the filter of each row is the
//! one with the smallest sum of absolute values; the compression is a
//! greedy LZ77 with hash chains and the fixed Huffman code.

use crate::flate::adler32;
use crate::raster::Canvas;

/// The CRC-32 of PNG and zlib (ISO 3309, polynomial 0xEDB88320).
pub fn crc32(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut n = 0;
        while n < 256 {
            let mut c = n as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
                k += 1;
            }
            table[n] = c;
            n += 1;
        }
        table
    };
    !data.iter().fold(!0u32, |c, &b| {
        TABLE[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8)
    })
}

/// Writes bits least significant first.
#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    acc: u64,
    count: u32,
}

impl BitWriter {
    fn put(&mut self, value: u32, bits: u32) {
        self.acc |= u64::from(value) << self.count;
        self.count += bits;
        while self.count >= 8 {
            self.bytes.push(self.acc as u8);
            self.acc >>= 8;
            self.count -= 8;
        }
    }

    /// A Huffman code, most significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        let reversed = (0..len).fold(0, |r, i| r << 1 | ((code >> i) & 1));
        self.put(reversed, len);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.bytes.push(self.acc as u8);
        }
        self.bytes
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

/// The fixed literal/length code of a symbol (RFC 1951 3.2.6).
fn literal(w: &mut BitWriter, sym: u32) {
    match sym {
        0..=143 => w.code(0x30 + sym, 8),
        144..=255 => w.code(0x190 + sym - 144, 9),
        256..=279 => w.code(sym - 256, 7),
        _ => w.code(0xc0 + sym - 280, 8),
    }
}

/// The index of the last base that is at most `v`.
fn bucket(bases: &[u16], v: usize) -> usize {
    bases
        .iter()
        .rposition(|&b| usize::from(b) <= v)
        .unwrap_or(0)
}

fn length_and_distance(w: &mut BitWriter, length: usize, distance: usize) {
    // 258 has its own code (285) although 227 + 31 is also 258
    let l = if length == 258 {
        28
    } else {
        bucket(&LENGTH_BASE[..28], length)
    };
    literal(w, 257 + l as u32);
    w.put(
        (length - usize::from(LENGTH_BASE[l])) as u32,
        u32::from(LENGTH_EXTRA[l]),
    );
    let d = bucket(&DIST_BASE, distance);
    w.code(d as u32, 5);
    w.put(
        (distance - usize::from(DIST_BASE[d])) as u32,
        u32::from(DIST_EXTRA[d]),
    );
}

const WINDOW: usize = 32768;
const HASH_BITS: u32 = 15;
const MAX_CHAIN: usize = 128;
const MAX_MATCH: usize = 258;
const MIN_MATCH: usize = 3;

fn hash(d: &[u8], i: usize) -> usize {
    ((usize::from(d[i]) << 10) ^ (usize::from(d[i + 1]) << 5) ^ usize::from(d[i + 2]))
        & ((1 << HASH_BITS) - 1)
}

/// The length of the match of `data[i..i + max]` at `candidate`: 8 bytes at
/// a time, then the first byte that differs (docs/adr/0011).
fn match_length(data: &[u8], candidate: usize, i: usize, max: usize) -> usize {
    let word = |at: usize| u64::from_le_bytes(data[at..at + 8].try_into().unwrap_or([0; 8]));
    let mut len = 0;
    while len + 8 <= max {
        let diff = word(candidate + len) ^ word(i + len);
        if diff != 0 {
            return len + (diff.trailing_zeros() / 8) as usize;
        }
        len += 8;
    }
    len + data[candidate + len..]
        .iter()
        .zip(&data[i + len..i + max])
        .take_while(|(a, b)| a == b)
        .count()
}

/// Raw Deflate: one block with the fixed Huffman code, greedy LZ77 over a
/// window of 32 KiB with hash chains of at most 128 candidates. The
/// longest match wins; among equal lengths, the nearest.
fn deflate(data: &[u8]) -> Vec<u8> {
    deflate_with(data, match_length)
}

/// Deflate with a function for the length of a match.
fn deflate_with(data: &[u8], match_len: impl Fn(&[u8], usize, usize, usize) -> usize) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.put(1, 1); // BFINAL
    w.put(1, 2); // fixed Huffman
    let none = usize::MAX;
    let mut head = vec![none; 1 << HASH_BITS];
    let mut prev = vec![none; WINDOW];
    let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, i: usize| {
        if i + MIN_MATCH <= data.len() {
            let h = hash(data, i);
            prev[i % WINDOW] = head[h];
            head[h] = i;
        }
    };
    let mut i = 0;
    while i < data.len() {
        let (mut best_len, mut best_dist) = (0, 0);
        if i + MIN_MATCH <= data.len() {
            let max = MAX_MATCH.min(data.len() - i);
            let mut candidate = head[hash(data, i)];
            let mut tries = 0;
            while candidate != none && i - candidate <= WINDOW && tries < MAX_CHAIN {
                let len = match_len(data, candidate, i, max);
                if len > best_len {
                    best_len = len;
                    best_dist = i - candidate;
                    if len == max {
                        break;
                    }
                }
                tries += 1;
                let next = prev[candidate % WINDOW];
                // An entry older than the window was overwritten; stop there.
                if next == none || next >= candidate {
                    break;
                }
                candidate = next;
            }
        }
        if best_len >= MIN_MATCH {
            length_and_distance(&mut w, best_len, best_dist);
            (i..i + best_len).for_each(|k| insert(&mut head, &mut prev, k));
            i += best_len;
        } else {
            literal(&mut w, u32::from(data[i]));
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    literal(&mut w, 256);
    w.finish()
}

/// A zlib stream with a function for the length of a match, for the tests.
#[cfg(test)]
fn zlib_with(data: &[u8], match_len: impl Fn(&[u8], usize, usize, usize) -> usize) -> Vec<u8> {
    [
        &[0x78, 0x01][..],
        &deflate_with(data, match_len),
        &adler32(data).to_be_bytes(),
    ]
    .concat()
}

/// A zlib stream (RFC 1950): the header 78 01, Deflate and the Adler-32.
pub fn zlib(data: &[u8]) -> Vec<u8> {
    [
        &[0x78, 0x01][..],
        &deflate(data),
        &adler32(data).to_be_bytes(),
    ]
    .concat()
}

/// The Paeth predictor (PNG 9.4).
pub fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

const BPP: usize = 3;

/// The filter type and the filtered row with the smallest sum of the bytes
/// as signed values; the lowest type wins a tie.
pub fn choose_filter(row: &[u8], prev: Option<&[u8]>) -> (u8, Vec<u8>) {
    let up = |x: usize| prev.map_or(0, |p| p[x]);
    let left = |x: usize| if x >= BPP { row[x - BPP] } else { 0 };
    let up_left = |x: usize| {
        if x >= BPP {
            prev.map_or(0, |p| p[x - BPP])
        } else {
            0
        }
    };
    let filtered = |kind: u8, x: usize| -> u8 {
        let predicted = match kind {
            0 => 0,
            1 => left(x),
            2 => up(x),
            3 => ((u16::from(left(x)) + u16::from(up(x))) / 2) as u8,
            _ => paeth(left(x), up(x), up_left(x)),
        };
        row[x].wrapping_sub(predicted)
    };
    // The sums of the five types in one pass, without making the rows
    // (docs/adr/0011); only the chosen row is made.
    let costs = (0..row.len()).fold([0u64; 5], |mut sums, x| {
        (0..5u8)
            .for_each(|k| sums[usize::from(k)] += u64::from((filtered(k, x) as i8).unsigned_abs()));
        sums
    });
    let kind = (0..5u8)
        .min_by_key(|&k| (costs[usize::from(k)], k))
        .unwrap_or(0);
    (kind, (0..row.len()).map(|x| filtered(kind, x)).collect())
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(kind);
    out.extend(data);
    out.extend(crc32(&[&kind[..], data].concat()).to_be_bytes());
}

/// A PNG of 8-bit RGB without interlacing and without ancillary chunks.
pub fn encode_png(canvas: &Canvas) -> Vec<u8> {
    let stride = canvas.width * BPP;
    let rows: Vec<&[u8]> = canvas
        .pixels
        .chunks(stride.max(1))
        .take(canvas.height)
        .collect();
    let raw: Vec<u8> = rows
        .iter()
        .enumerate()
        .flat_map(|(y, row)| {
            let (kind, filtered) =
                choose_filter(row, if y == 0 { None } else { Some(rows[y - 1]) });
            std::iter::once(kind).chain(filtered)
        })
        .collect();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend((canvas.width as u32).to_be_bytes());
    ihdr.extend((canvas.height as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A binary PPM (P6) of the pixels.
pub fn encode_ppm(canvas: &Canvas) -> Vec<u8> {
    [
        format!("P6\n{} {}\n255\n", canvas.width, canvas.height).as_bytes(),
        &canvas.pixels,
    ]
    .concat()
}

#[cfg(test)]
mod tests;
