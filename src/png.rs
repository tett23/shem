//! PNG and PPM files, and the Deflate compression for PNG (docs/adr/0007).
//!
//! The bytes of a PNG depend only on the pixels and the version of shem:
//! the chunks are IHDR, one IDAT and IEND; the filter of each row is the
//! one with the smallest sum of absolute values; the compression is a
//! greedy LZ77 with hash chains and the fixed Huffman code.

use crate::flate::adler32;
use crate::raster::Canvas;

/// The table of the CRC-32 (ISO 3309, polynomial 0xEDB88320).
const CRC_TABLE: [u32; 256] = {
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

/// The tables of the CRC-32 for 8 bytes at a time: table k gives the
/// register of a byte followed by k zero bytes (slicing-by-8).
const CRC_TABLES: [[u32; 256]; 8] = {
    let mut t = [[0u32; 256]; 8];
    t[0] = CRC_TABLE;
    let mut k = 1;
    while k < 8 {
        let mut n = 0;
        while n < 256 {
            let c = t[k - 1][n];
            t[k][n] = (c >> 8) ^ CRC_TABLE[(c & 0xff) as usize];
            n += 1;
        }
        k += 1;
    }
    t
};

/// The CRC-32 register after `data`, from the register `c`: 8 bytes at a
/// time, the same register as byte by byte (docs/adr/0015).
fn crc32_update(c: u32, data: &[u8]) -> u32 {
    let words = data.chunks_exact(8);
    let rest = words.remainder();
    let c = words.fold(c, |c, w| {
        let lo = c ^ u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
        let hi = u32::from_le_bytes([w[4], w[5], w[6], w[7]]);
        let t = &CRC_TABLES;
        t[7][(lo & 0xff) as usize]
            ^ t[6][((lo >> 8) & 0xff) as usize]
            ^ t[5][((lo >> 16) & 0xff) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][(hi & 0xff) as usize]
            ^ t[2][((hi >> 8) & 0xff) as usize]
            ^ t[1][((hi >> 16) & 0xff) as usize]
            ^ t[0][(hi >> 24) as usize]
    });
    rest.iter().fold(c, |c, &b| {
        CRC_TABLE[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8)
    })
}

/// The CRC-32 of PNG and zlib (ISO 3309, polynomial 0xEDB88320).
pub fn crc32(data: &[u8]) -> u32 {
    !crc32_update(!0, data)
}

/// Writes bits least significant first.
#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    acc: u64,
    count: u32,
}

impl BitWriter {
    /// Puts at most 32 bits. The bits are written 32 at a time: fewer than
    /// 32 wait in `acc`, so at most 63 are there (docs/adr/0015).
    fn put(&mut self, value: u32, bits: u32) {
        self.acc |= u64::from(value) << self.count;
        self.count += bits;
        if self.count >= 32 {
            self.bytes
                .extend_from_slice(&(self.acc as u32).to_le_bytes());
            self.acc >>= 32;
            self.count -= 32;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        let rest = self.count.div_ceil(8) as usize;
        self.bytes
            .extend_from_slice(&self.acc.to_le_bytes()[..rest]);
        self.bytes
    }
}

/// `code` of `len` bits, most significant bit first, as the bits to put
/// least significant first.
const fn reverse(code: u32, len: u32) -> u32 {
    let mut r = 0;
    let mut i = 0;
    while i < len {
        r = r << 1 | ((code >> i) & 1);
        i += 1;
    }
    r
}

/// The fixed literal/length codes (RFC 1951 3.2.6), reversed, and their
/// lengths (docs/adr/0015).
const LITERALS: [(u32, u32); 288] = {
    let mut t = [(0, 0); 288];
    let mut sym = 0;
    while sym < 288 {
        let (code, len) = match sym {
            0..=143 => (0x30 + sym, 8),
            144..=255 => (0x190 + sym - 144, 9),
            256..=279 => (sym - 256, 7),
            _ => (0xc0 + sym - 280, 8),
        };
        t[sym as usize] = (reverse(code, len), len);
        sym += 1;
    }
    t
};

/// The fixed literal/length code of a symbol.
fn literal(w: &mut BitWriter, sym: u32) {
    let (code, len) = LITERALS[sym as usize];
    w.put(code, len);
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

/// The index of the last base that is at most `v`, for every `v` below `N`.
const fn buckets<const N: usize>(bases: &[u16]) -> [u8; N] {
    let mut t = [0u8; N];
    let mut v = 0;
    while v < N {
        let mut k = 0;
        let mut i = 0;
        while i < bases.len() {
            if bases[i] as usize <= v {
                k = i;
            }
            i += 1;
        }
        t[v] = k as u8;
        v += 1;
    }
    t
}

/// The length code of each length; 258 has its own code (285) although
/// 227 + 31 is also 258.
const LENGTH_CODE: [u8; 259] = {
    let mut t = buckets::<259>(&[
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227,
    ]);
    t[258] = 28;
    t
};

/// The distance code of the distances up to 256, and of the others by
/// their bits above the seventh, as zlib does.
const DIST_CODE_LOW: [u8; 257] = buckets::<257>(&DIST_BASE);
const DIST_CODE_HIGH: [u8; 257] = {
    let mut t = [0u8; 257];
    let mut v = 2;
    while v < 257 {
        // The code of the distances (v << 7) + 1 ..= (v + 1) << 7.
        let d = (v << 7) + 1;
        let mut k = 0;
        let mut i = 0;
        while i < DIST_BASE.len() {
            if DIST_BASE[i] as usize <= d {
                k = i;
            }
            i += 1;
        }
        t[v] = k as u8;
        v += 1;
    }
    t
};

fn distance_code(distance: usize) -> usize {
    if distance <= 256 {
        usize::from(DIST_CODE_LOW[distance])
    } else {
        usize::from(DIST_CODE_HIGH[(distance - 1) >> 7])
    }
}

fn length_and_distance(w: &mut BitWriter, length: usize, distance: usize) {
    let l = usize::from(LENGTH_CODE[length]);
    literal(w, 257 + l as u32);
    w.put(
        (length - usize::from(LENGTH_BASE[l])) as u32,
        u32::from(LENGTH_EXTRA[l]),
    );
    let d = distance_code(distance);
    w.put(reverse(d as u32, 5), 5);
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
/// No position: the entries of the chains are positions as u32 (docs/adr/0015).
const NONE: u32 = u32::MAX;

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

/// What a run of candidates gives: the match that ends the search, or the
/// best match among them if it is longer than the one before, the last
/// candidate of the run, and how many candidates it has.
enum Group {
    Done(usize, usize),
    Next {
        best: Option<(usize, usize)>,
        last: usize,
        count: usize,
    },
}

/// The candidates `c`, `c − 1`, … that lie in one run of the byte `b =
/// data[c]`, taken at once, if there are at least two of them, as many as
/// the tries left and the window allow.
///
/// Each position of the run starts the three bytes `b b b`, so it was
/// chained to the one before it. With A the length of the run from a
/// candidate and R the length of the run of `b` at `i`, the match is A if A
/// < R, R if A > R, and at least R if A = R, all at most `max`. Along the
/// candidates, A grows by one, so the lengths grow while A < R, peak at A =
/// R, and stay at R after it. A candidate wins only with a longer match, so
/// the best of the run is the first with its longest length.
fn run_group(
    data: &[u8],
    i: usize,
    c: usize,
    max: usize,
    tries: usize,
    best_len: usize,
    run_at_i: &mut Option<(u8, usize)>,
) -> Option<Group> {
    let b = data[c];
    if c == 0 || data[c - 1] != b || data[c + 1] != b || data[c + 2] != b {
        return None;
    }
    let r = match *run_at_i {
        Some((v, r)) if v == b => r,
        _ => {
            let r = run_length(&data[i..i + max], b);
            *run_at_i = Some((b, r));
            r
        }
    };
    // The candidate just before a run of at least `max` matches it to `max`.
    if r == max && c + 1 == i {
        return Some(Group::Done(max, 1));
    }
    // The lowest candidate the tries and the window allow.
    let low = c
        .saturating_sub(MAX_CHAIN - tries - 1)
        .max(i.saturating_sub(WINDOW));
    let last = c - run_back(data, low, c, b);
    let count = c - last + 1;
    if count < 2 {
        return None;
    }
    // The run from c matters up to max, and when r < max, up to r + 1.
    let a0 = run_length(&data[c..c + max.min(r + 1)], b);
    let at = |k: usize| (a0 + k, i - (c - k));
    let better = |len: usize, dist: usize| (len > best_len).then_some((len, dist));
    if r == max {
        // The run at i is at least `max`: the lengths are min(A, max).
        if a0 + count > max {
            let k = max.saturating_sub(a0);
            return Some(Group::Done(max, i - (c - k)));
        }
        let (len, dist) = at(count - 1);
        return Some(Group::Next {
            best: better(len, dist),
            last,
            count,
        });
    }
    // r < max.
    let best = if a0 > r {
        better(r, i - c)
    } else if a0 + count - 1 < r {
        let (len, dist) = at(count - 1);
        better(len, dist)
    } else {
        let k = r - a0;
        let before = if k > 0 {
            better(r - 1, i - (c - (k - 1)))
        } else {
            None
        };
        let peak = c - k;
        let len = match_length(data, peak, i, max);
        let floor = before.map_or(best_len, |(l, _)| l);
        let best = if len > floor {
            Some((len, i - peak))
        } else {
            before
        };
        if len == max {
            return Some(Group::Done(max, i - peak));
        }
        best
    };
    Some(Group::Next { best, last, count })
}

/// The number of bytes at the start of `s` that are `b`, 8 at a time.
fn run_length(s: &[u8], b: u8) -> usize {
    let word = u64::from_ne_bytes([b; 8]);
    let mut n = 0;
    while let Some(w) = s.get(n..n + 8) {
        let diff = u64::from_le_bytes(w.try_into().unwrap_or([0; 8])) ^ word;
        if diff != 0 {
            return n + (diff.trailing_zeros() / 8) as usize;
        }
        n += 8;
    }
    n + s[n..].iter().take_while(|&&v| v == b).count()
}

/// The number of bytes before `end`, down to `low`, that are `b`, 8 at a
/// time.
fn run_back(data: &[u8], low: usize, end: usize, b: u8) -> usize {
    let word = u64::from_ne_bytes([b; 8]);
    let mut n = 0;
    while end - n >= low + 8 {
        let at = end - n - 8;
        let diff = u64::from_le_bytes(data[at..at + 8].try_into().unwrap_or([0; 8])) ^ word;
        if diff != 0 {
            return n + (diff.leading_zeros() / 8) as usize;
        }
        n += 8;
    }
    n + data[low..end - n]
        .iter()
        .rev()
        .take_while(|&&v| v == b)
        .count()
}

/// Raw Deflate: one block with the fixed Huffman code, greedy LZ77 over a
/// window of 32 KiB with hash chains of at most 128 candidates. The
/// longest match wins; among equal lengths, the nearest.
fn deflate(data: &[u8]) -> Vec<u8> {
    deflate_with(data, match_length)
}

/// Deflate with a function for the length of a match. Positions are u32,
/// so the data must be shorter than 4 GiB; a page is at most 2^28 pixels
/// (docs/adr/0007), 3 · 2^28 + 2^28 / 1 bytes with the filter bytes.
fn deflate_with(data: &[u8], match_len: impl Fn(&[u8], usize, usize, usize) -> usize) -> Vec<u8> {
    assert!(data.len() < NONE as usize, "data too large for Deflate");
    let mut w = BitWriter {
        bytes: Vec::with_capacity(data.len() / 8 + 16),
        ..BitWriter::default()
    };
    w.put(1, 1); // BFINAL
    w.put(1, 2); // fixed Huffman
    let mut head = vec![NONE; 1 << HASH_BITS];
    let mut prev = vec![NONE; WINDOW];
    // The last position that starts three bytes, plus one.
    let last = data.len().saturating_sub(MIN_MATCH - 1);
    let insert = |head: &mut [u32], prev: &mut [u32], i: usize| {
        let h = hash(data, i);
        prev[i % WINDOW] = head[h];
        head[h] = i as u32;
    };
    let mut i = 0;
    while i < data.len() {
        let (mut best_len, mut best_dist) = (0, 0);
        if i < last {
            let max = MAX_MATCH.min(data.len() - i);
            let mut candidate = head[hash(data, i)];
            let mut tries = 0;
            let mut run_at_i = None;
            while candidate != NONE && i - candidate as usize <= WINDOW && tries < MAX_CHAIN {
                let c = candidate as usize;
                // The candidates in a run of one byte come in the chain one
                // after another, and their lengths follow from the lengths
                // of the runs, so a run is taken at once (docs/adr/0015).
                if let Some(group) = run_group(data, i, c, max, tries, best_len, &mut run_at_i) {
                    match group {
                        Group::Done(len, dist) => {
                            best_len = len;
                            best_dist = dist;
                            break;
                        }
                        Group::Next { best, last, count } => {
                            if let Some((len, dist)) = best {
                                best_len = len;
                                best_dist = dist;
                            }
                            tries += count;
                            if tries >= MAX_CHAIN {
                                break;
                            }
                            let next = prev[last % WINDOW];
                            if next == NONE || next as usize >= last {
                                break;
                            }
                            candidate = next;
                            continue;
                        }
                    }
                }
                // A candidate whose byte at the end of the best match differs
                // cannot be longer, so its length is not counted
                // (docs/adr/0015); it still counts as a try.
                let len = if best_len > 0 && data[c + best_len] != data[i + best_len] {
                    0
                } else {
                    match_len(data, c, i, max)
                };
                if len > best_len {
                    best_len = len;
                    best_dist = i - c;
                    if len == max {
                        break;
                    }
                }
                tries += 1;
                let next = prev[c % WINDOW];
                // An entry older than the window was overwritten; stop there.
                if next == NONE || next >= candidate {
                    break;
                }
                candidate = next;
            }
        }
        if best_len >= MIN_MATCH {
            length_and_distance(&mut w, best_len, best_dist);
            // In a run of one byte b, the positions that start `b b b` share
            // their hash, and each is chained to the one before it; the run
            // is found 8 bytes at a time (docs/adr/0015). Elsewhere, the hash
            // of the next position is the last one shifted by 5 bits with the
            // new byte: the mask drops the byte that leaves.
            let end = (i + best_len).min(last);
            let mask = (1 << HASH_BITS) - 1;
            let mut k = i;
            let mut h = if k < end { hash(data, k) } else { 0 };
            while k < end {
                let b = data[k];
                let g = if data[k + 1] == b && data[k + 2] == b {
                    // Positions k.. whose three bytes are in the run.
                    let run = run_length(&data[k..end + 2], b);
                    k + run - 2
                } else {
                    k + 1
                };
                prev[k % WINDOW] = head[h];
                (k + 1..g).for_each(|j| prev[j % WINDOW] = (j - 1) as u32);
                head[h] = (g - 1) as u32;
                if g < end {
                    h = if g == k + 1 {
                        ((h << 5) ^ usize::from(data[g + 2])) & mask
                    } else {
                        hash(data, g)
                    };
                }
                k = g;
            }
            i += best_len;
        } else {
            literal(&mut w, u32::from(data[i]));
            if i < last {
                insert(&mut head, &mut prev, i);
            }
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
    let mut out = vec![0x78, 0x01];
    out.extend(deflate(data));
    out.extend(adler32(data).to_be_bytes());
    out
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

/// Gives `$body` the bytes of the row filtered by `$kind` as two
/// iterators: `$first`, of the first pixel, which has no left neighbor, and
/// `$rest`, of the others. The first row has no row above (PNG 9.2). Each
/// type is a loop of its own over slices, which the compiler can vectorize
/// (docs/adr/0015).
macro_rules! with_filtered {
    ($kind:expr, $row:expr, $prev:expr, |$first:ident, $rest:ident| $body:expr) => {{
        let (row, prev): (&[u8], Option<&[u8]>) = ($row, $prev);
        let head = BPP.min(row.len());
        let (row_first, row_rest) = row.split_at(head);
        match ($kind, prev) {
            (0, _) | (2, None) => {
                let $first = row_first.iter().copied();
                let $rest = row_rest.iter().copied();
                $body
            }
            (1, _) | (4.., None) => {
                let $first = row_first.iter().copied();
                let $rest = row_rest.iter().zip(row).map(|(&b, &l)| b.wrapping_sub(l));
                $body
            }
            (2, Some(up)) => {
                let $first = row_first.iter().zip(up).map(|(&b, &u)| b.wrapping_sub(u));
                let $rest = row_rest
                    .iter()
                    .zip(&up[head..])
                    .map(|(&b, &u)| b.wrapping_sub(u));
                $body
            }
            (3, None) => {
                let $first = row_first.iter().copied();
                let $rest = row_rest
                    .iter()
                    .zip(row)
                    .map(|(&b, &l)| b.wrapping_sub(l / 2));
                $body
            }
            (3, Some(up)) => {
                let $first = row_first
                    .iter()
                    .zip(up)
                    .map(|(&b, &u)| b.wrapping_sub(u / 2));
                let $rest = row_rest
                    .iter()
                    .zip(row)
                    .zip(&up[head..])
                    .map(|((&b, &l), &u)| {
                        b.wrapping_sub(((u16::from(l) + u16::from(u)) / 2) as u8)
                    });
                $body
            }
            (_, Some(up)) => {
                let $first = row_first.iter().zip(up).map(|(&b, &u)| b.wrapping_sub(u));
                let $rest = row_rest
                    .iter()
                    .zip(row)
                    .zip(up[head..].iter().zip(up))
                    .map(|((&b, &l), (&u, &ul))| b.wrapping_sub(paeth(l, u, ul)));
                $body
            }
        }
    }};
}

/// The sum of the bytes of the row filtered by `kind`, as signed values.
fn filter_cost(kind: u8, row: &[u8], prev: Option<&[u8]>) -> u64 {
    let cost = |b: u8| u64::from((b as i8).unsigned_abs());
    with_filtered!(kind, row, prev, |first, rest| first.map(cost).sum::<u64>()
        + rest.map(cost).sum::<u64>())
}

/// Writes the row filtered by `kind` into `out`, of the length of the row.
fn filter_into(kind: u8, row: &[u8], prev: Option<&[u8]>, out: &mut [u8]) {
    let (out_first, out_rest) = out.split_at_mut(BPP.min(row.len()));
    with_filtered!(kind, row, prev, |first, rest| {
        out_first.iter_mut().zip(first).for_each(|(o, b)| *o = b);
        out_rest.iter_mut().zip(rest).for_each(|(o, b)| *o = b);
    })
}

/// The filter type with the smallest sum of the filtered bytes as signed
/// values; the lowest type wins a tie. The types are tried in order, and a
/// sum of 0 ends the search, since no later type can be smaller.
fn choose_kind(row: &[u8], prev: Option<&[u8]>) -> u8 {
    let mut best = (u64::MAX, 0u8);
    for kind in 0..5u8 {
        let sum = filter_cost(kind, row, prev);
        if sum < best.0 {
            best = (sum, kind);
        }
        if best.0 == 0 {
            break;
        }
    }
    best.1
}

/// The filter type and the filtered row with the smallest sum of the bytes
/// as signed values; the lowest type wins a tie.
pub fn choose_filter(row: &[u8], prev: Option<&[u8]>) -> (u8, Vec<u8>) {
    let kind = choose_kind(row, prev);
    let mut out = vec![0; row.len()];
    filter_into(kind, row, prev, &mut out);
    (kind, out)
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(kind);
    out.extend(data);
    out.extend((!crc32_update(crc32_update(!0, kind), data)).to_be_bytes());
}

/// A PNG of 8-bit RGB without interlacing and without ancillary chunks.
pub fn encode_png(canvas: &Canvas) -> Vec<u8> {
    let stride = canvas.width * BPP;
    let rows: Vec<&[u8]> = canvas
        .pixels
        .chunks(stride.max(1))
        .take(canvas.height)
        .collect();
    // The filtered rows, each after its filter type, in one buffer.
    let mut raw = vec![0u8; (stride + 1) * rows.len()];
    raw.chunks_mut(stride + 1)
        .zip(rows.iter().enumerate())
        .for_each(|(out, (y, row))| {
            let prev = (y > 0).then(|| rows[y - 1]);
            let kind = choose_kind(row, prev);
            out[0] = kind;
            filter_into(kind, row, prev, &mut out[1..]);
        });
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
    let mut out = Vec::new();
    write_ppm(canvas, &mut out).expect("writing to a Vec does not fail");
    out
}

/// Writes the bytes of [`encode_ppm`] without making them in memory first
/// (docs/adr/0013).
pub fn write_ppm(canvas: &Canvas, out: &mut impl std::io::Write) -> std::io::Result<()> {
    write!(out, "P6\n{} {}\n255\n", canvas.width, canvas.height)?;
    out.write_all(&canvas.pixels)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod reference;

#[cfg(test)]
mod reference_tests;
