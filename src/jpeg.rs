//! JPEG (ITU-T T.81) for `DCTDecode` streams (docs/adr/0027).
//!
//! The decoder reads baseline (SOF0), extended sequential (SOF1) and
//! progressive (SOF2) frames with Huffman coding, 8 or 12 bits, 1, 3 or 4
//! components, sampling factors 1 to 4 and restart intervals. The inverse
//! DCT is the integer one of libjpeg's `jidctint.c` (`jpeg_idct_islow`),
//! the chroma upsampling the "fancy" triangle filter of `jdsample.c`, and
//! the colour transform the integer one of `jdcolor.c`, all written again
//! here in integer arithmetic. The samples come out as 8 bits per
//! component after the colour transform (YCbCr → RGB, YCCK → CMYK, or
//! none), 12-bit samples reduced by `div_round(v × 255, 4095)`.
//!
//! Every allocation is bounded by the frame's dimensions, which are
//! limited to [`MAX_SAMPLES`] samples before anything is allocated. Data
//! that ends early or breaks the format is an error, never a panic.

use crate::error::{Error, Result};
use crate::fixed::div_round_i64;

/// The most samples (`width × height × components`) an image may have
/// (docs/adr/0027).
pub const MAX_SAMPLES: u64 = 1 << 26;

/// A decoded image: `samples` holds `width × height × components` bytes,
/// row by row, the components of a pixel together, after the colour
/// transform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub components: u8,
    /// The precision of the frame (8 or 12); the samples are 8 bits either way.
    pub precision: u8,
    pub samples: Vec<u8>,
}

fn corrupt(what: impl std::fmt::Display) -> Error {
    Error::new(format!("corrupt JPEG data: {what}"))
}

fn unsupported(what: impl std::fmt::Display) -> Error {
    Error::new(format!("unsupported JPEG {what}"))
}

/// The error of a frame over [`MAX_SAMPLES`] samples (docs/adr/0027).
pub fn too_large() -> Error {
    Error::new(format!("image larger than {MAX_SAMPLES} samples"))
}

/// The natural-order index of the k-th coefficient in zigzag order
/// (T.81 Figure A.6).
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

// ---------------------------------------------------------------------------
// The integer inverse DCT of jidctint.c.

const CONST_BITS: u32 = 13;
const FIX_0_298631336: i64 = 2446;
const FIX_0_390180644: i64 = 3196;
const FIX_0_541196100: i64 = 4433;
const FIX_0_765366865: i64 = 6270;
const FIX_0_899976223: i64 = 7373;
const FIX_1_175875602: i64 = 9633;
const FIX_1_501321110: i64 = 12299;
const FIX_1_847759065: i64 = 15137;
const FIX_1_961570560: i64 = 16069;
const FIX_2_053119869: i64 = 16819;
const FIX_2_562915447: i64 = 20995;
const FIX_3_072711026: i64 = 25172;

/// `x / 2^n`, rounded to the nearest, halves up (jidctint.c's `DESCALE`).
#[inline]
fn descale(x: i64, n: u32) -> i64 {
    (x + (1 << (n - 1))) >> n
}

/// One pass of the 1-D transform over eight values (jidctint.c): the eight
/// outputs before descaling, in the order 0 to 7, each scaled by
/// 2^CONST_BITS.
#[inline]
fn idct_1d(input: [i64; 8]) -> [i64; 8] {
    // Even part.
    let z2 = input[2];
    let z3 = input[6];
    let z1 = (z2 + z3) * FIX_0_541196100;
    let tmp2 = z1 + z3 * -FIX_1_847759065;
    let tmp3 = z1 + z2 * FIX_0_765366865;
    let tmp0 = (input[0] + input[4]) << CONST_BITS;
    let tmp1 = (input[0] - input[4]) << CONST_BITS;
    let tmp10 = tmp0 + tmp3;
    let tmp13 = tmp0 - tmp3;
    let tmp11 = tmp1 + tmp2;
    let tmp12 = tmp1 - tmp2;
    // Odd part.
    let tmp0 = input[7];
    let tmp1 = input[5];
    let tmp2 = input[3];
    let tmp3 = input[1];
    let z1 = tmp0 + tmp3;
    let z2 = tmp1 + tmp2;
    let z3 = tmp0 + tmp2;
    let z4 = tmp1 + tmp3;
    let z5 = (z3 + z4) * FIX_1_175875602;
    let tmp0 = tmp0 * FIX_0_298631336;
    let tmp1 = tmp1 * FIX_2_053119869;
    let tmp2 = tmp2 * FIX_3_072711026;
    let tmp3 = tmp3 * FIX_1_501321110;
    let z1 = z1 * -FIX_0_899976223;
    let z2 = z2 * -FIX_2_562915447;
    let z3 = z3 * -FIX_1_961570560 + z5;
    let z4 = z4 * -FIX_0_390180644 + z5;
    let tmp0 = tmp0 + z1 + z3;
    let tmp1 = tmp1 + z2 + z4;
    let tmp2 = tmp2 + z2 + z3;
    let tmp3 = tmp3 + z1 + z4;
    [
        tmp10 + tmp3,
        tmp11 + tmp2,
        tmp12 + tmp1,
        tmp13 + tmp0,
        tmp13 - tmp0,
        tmp12 - tmp1,
        tmp11 - tmp2,
        tmp10 - tmp3,
    ]
}

/// The inverse DCT of `jpeg_idct_islow`: `coefs` are the dequantized
/// coefficients in natural order (row-major, the vertical frequency first);
/// the result is the 8 × 8 samples row by row, level-shifted and clamped to
/// 0 to 2^precision − 1. `CONST_BITS` is 13 and `PASS1_BITS` 2 for 8 bits,
/// 1 for 12 bits. The sums are taken in i64, where libjpeg's `JLONG` is
/// `long`, so nothing wraps (docs/adr/0027).
pub fn idct_islow(coefs: &[i32; 64], precision: u8) -> [u16; 64] {
    let pass1_bits: u32 = if precision == 8 { 2 } else { 1 };
    let center: i64 = 1 << (precision - 1);
    let max: i64 = (1 << precision) - 1;
    // Pass 1: columns, scaled up by 2^PASS1_BITS.
    let mut workspace = [0i64; 64];
    (0..8).for_each(|col| {
        let column = std::array::from_fn(|row| i64::from(coefs[row * 8 + col]));
        let out = idct_1d(column);
        (0..8).for_each(|row| {
            workspace[row * 8 + col] = descale(out[row], CONST_BITS - pass1_bits);
        });
    });
    // Pass 2: rows, descaled by 2^(CONST_BITS + PASS1_BITS + 3).
    let mut samples = [0u16; 64];
    (0..8).for_each(|row| {
        let line = std::array::from_fn(|i| workspace[row * 8 + i]);
        let out = idct_1d(line);
        (0..8).for_each(|i| {
            let v = descale(out[i], CONST_BITS + pass1_bits + 3) + center;
            samples[row * 8 + i] = v.clamp(0, max) as u16;
        });
    });
    samples
}

// ---------------------------------------------------------------------------
// The upsampling of jdsample.c.

/// `h2v1_fancy_upsample`: `input` has at least three samples; `out` gets
/// twice as many.
pub fn h2v1_fancy(input: &[u16], out: &mut [u16]) {
    let n = input.len();
    let at = |i: usize| i64::from(input[i]);
    out[0] = input[0];
    out[1] = ((at(0) * 3 + at(1) + 2) >> 2) as u16;
    (1..n - 1).for_each(|i| {
        let near = at(i) * 3;
        out[2 * i] = ((near + at(i - 1) + 1) >> 2) as u16;
        out[2 * i + 1] = ((near + at(i + 1) + 2) >> 2) as u16;
    });
    out[2 * n - 2] = ((at(n - 1) * 3 + at(n - 2) + 1) >> 2) as u16;
    out[2 * n - 1] = input[n - 1];
}

/// `h2v2_fancy_upsample` for one output row: `near` is the input row the
/// output row lies in, `far` the next nearest (above for the first of the
/// two output rows, below for the second); both have at least three
/// samples, and `out` gets twice as many.
pub fn h2v2_fancy(near: &[u16], far: &[u16], out: &mut [u16]) {
    let n = near.len();
    let colsum = |i: usize| i64::from(near[i]) * 3 + i64::from(far[i]);
    let (this, next) = (colsum(0), colsum(1));
    out[0] = ((this * 4 + 8) >> 4) as u16;
    out[1] = ((this * 3 + next + 7) >> 4) as u16;
    let (mut last, mut this) = (this, next);
    (1..n - 1).for_each(|i| {
        let next = colsum(i + 1);
        out[2 * i] = ((this * 3 + last + 8) >> 4) as u16;
        out[2 * i + 1] = ((this * 3 + next + 7) >> 4) as u16;
        last = this;
        this = next;
    });
    out[2 * n - 2] = ((this * 3 + last + 8) >> 4) as u16;
    out[2 * n - 1] = ((this * 4 + 7) >> 4) as u16;
}

// ---------------------------------------------------------------------------
// The colour transform of jdcolor.c.

const SCALEBITS: u32 = 16;
const ONE_HALF: i64 = 1 << (SCALEBITS - 1);
/// `FIX(1.40200)`, `FIX(1.77200)`, `FIX(0.71414)`, `FIX(0.34414)`.
const FIX_1_40200: i64 = 91881;
const FIX_1_77200: i64 = 116130;
const FIX_0_71414: i64 = 46802;
const FIX_0_34414: i64 = 22554;

/// YCbCr to RGB as `ycc_rgb_convert` with its tables (`SCALEBITS` 16),
/// clamped to 0 to 2^precision − 1.
pub fn ycbcr_to_rgb(ycc: [u16; 3], precision: u8) -> [u16; 3] {
    let center: i64 = 1 << (precision - 1);
    let max: i64 = (1 << precision) - 1;
    let y = i64::from(ycc[0]);
    let cb = i64::from(ycc[1]) - center;
    let cr = i64::from(ycc[2]) - center;
    let r = y + ((FIX_1_40200 * cr + ONE_HALF) >> SCALEBITS);
    let g = y + ((-FIX_0_34414 * cb + ONE_HALF - FIX_0_71414 * cr) >> SCALEBITS);
    let b = y + ((FIX_1_77200 * cb + ONE_HALF) >> SCALEBITS);
    [r, g, b].map(|v| v.clamp(0, max) as u16)
}

/// A sample of `precision` bits as 8 bits: `div_round(v × 255, 2^p − 1)`
/// (docs/adr/0027).
pub fn to_eight_bits(v: u16, precision: u8) -> u8 {
    if precision == 8 {
        return v as u8;
    }
    let max = (1i64 << precision) - 1;
    div_round_i64(i64::from(v) * 255, max) as u8
}

// ---------------------------------------------------------------------------
// Huffman codes and the entropy-coded segment.

/// A Huffman table of a DHT segment, decoded as T.81 F.2.2.3 with an
/// eight-bit lookahead.
struct Huffman {
    /// For the next eight bits: `length << 8 | value`, or 0 when the code
    /// is longer than eight bits.
    lookup: Vec<u16>,
    /// The largest code of each length, or −1 for none (index 1 to 16).
    maxcode: [i32; 18],
    /// `valptr[l] − mincode[l]`: the index into `values` of a code of
    /// length `l` is the code plus this.
    valoffset: [i32; 17],
    values: Vec<u8>,
}

impl Huffman {
    fn new(counts: &[u8; 16], values: &[u8]) -> Result<Huffman> {
        let mut lookup = vec![0u16; 256];
        let mut maxcode = [-1i32; 18];
        let mut valoffset = [0i32; 17];
        let mut code = 0i32;
        let mut k = 0usize;
        for len in 1..=16u32 {
            let count = usize::from(counts[len as usize - 1]);
            valoffset[len as usize] = k as i32 - code;
            for _ in 0..count {
                if code >= 1 << len {
                    return Err(corrupt("over-subscribed Huffman table"));
                }
                if len <= 8 {
                    let first = (code as usize) << (8 - len);
                    let entry = (len as u16) << 8 | u16::from(values[k]);
                    lookup[first..first + (1 << (8 - len))].fill(entry);
                }
                code += 1;
                k += 1;
            }
            maxcode[len as usize] = code - 1;
            code <<= 1;
        }
        maxcode[17] = i32::MAX;
        Ok(Huffman {
            lookup,
            maxcode,
            valoffset,
            values: values.to_vec(),
        })
    }

    #[inline]
    fn decode(&self, input: &mut BitReader) -> Result<u8> {
        let (bits, available) = input.peek(8);
        let entry = self.lookup[bits as usize];
        let len = u32::from(entry >> 8);
        if len > 0 && len <= available {
            input.skip(len);
            return Ok(entry as u8);
        }
        let mut code = 0i32;
        for len in 1..=16 {
            code = (code << 1) | input.bits(1)? as i32;
            if code <= self.maxcode[len] {
                let index = (code + self.valoffset[len]) as usize;
                return self
                    .values
                    .get(index)
                    .copied()
                    .ok_or_else(|| corrupt("invalid Huffman code"));
            }
        }
        Err(corrupt("invalid Huffman code"))
    }
}

/// The bits of an entropy-coded segment, most significant first, with the
/// stuffed zero after 0xFF removed. The reader stops at a marker, and the
/// bits past it (or past the data) are an error to read.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u64,
    bits: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8], pos: usize) -> BitReader<'a> {
        BitReader {
            data,
            pos,
            acc: 0,
            bits: 0,
        }
    }

    /// Adds bytes to the accumulator up to a marker or the end of the data.
    #[inline]
    fn fill(&mut self) {
        while self.bits <= 56 {
            let Some(&byte) = self.data.get(self.pos) else {
                return;
            };
            if byte == 0xFF {
                match self.data.get(self.pos + 1) {
                    Some(0) => self.pos += 2,
                    _ => return,
                }
            } else {
                self.pos += 1;
            }
            self.acc |= u64::from(byte) << (56 - self.bits);
            self.bits += 8;
        }
    }

    /// The next `count` bits (at most 16) without reading them, the bits
    /// past the end as 0, and how many of them are in the data.
    #[inline]
    fn peek(&mut self, count: u32) -> (u32, u32) {
        if self.bits < count {
            self.fill();
        }
        ((self.acc >> (64 - count)) as u32, self.bits.min(count))
    }

    #[inline]
    fn skip(&mut self, count: u32) {
        self.acc <<= count;
        self.bits -= count;
    }

    /// `count` bits (at most 16), most significant first.
    #[inline]
    fn bits(&mut self, count: u32) -> Result<u32> {
        if count == 0 {
            return Ok(0);
        }
        let (v, available) = self.peek(count);
        if available < count {
            return Err(corrupt("entropy-coded data ends early"));
        }
        self.skip(count);
        Ok(v)
    }

    /// `count` bits as the value of T.81 F.2.2.1's `EXTEND`.
    #[inline]
    fn receive_extend(&mut self, count: u32) -> Result<i32> {
        if count == 0 {
            return Ok(0);
        }
        if count > 16 {
            return Err(corrupt("coefficient category over 16"));
        }
        let v = self.bits(count)? as i32;
        Ok(if v < 1 << (count - 1) {
            v - (1 << count) + 1
        } else {
            v
        })
    }

    /// Drops the bits of the current byte and moves to the marker that
    /// must follow: the first byte after the marker's second byte, with
    /// the marker.
    fn marker(&mut self) -> Result<(u8, usize)> {
        self.acc = 0;
        self.bits = 0;
        // Fill bytes (0xFF) may precede a marker (T.81 B.1.1.2).
        while self.data.get(self.pos..self.pos + 2) == Some(&[0xFF, 0xFF]) {
            self.pos += 1;
        }
        match self.data.get(self.pos..self.pos + 2) {
            Some(&[0xFF, m]) if m != 0 => Ok((m, self.pos + 2)),
            Some(_) => Err(corrupt("no marker after the entropy-coded data")),
            None => Err(corrupt("data ends in the entropy-coded data")),
        }
    }
}

// ---------------------------------------------------------------------------
// The frame.

struct Component {
    id: u8,
    h: usize,
    v: usize,
    /// The quantization table of the SOF, and the table itself once the
    /// component's first scan latched it.
    tq: u8,
    quant: Option<[u16; 64]>,
    /// The blocks that hold samples of the image: `ceil(downsampled / 8)`.
    blocks_w: usize,
    blocks_h: usize,
    /// The downsampled width and height: `ceil(width × h / hmax)`.
    width: usize,
    height: usize,
    /// The blocks a row of MCUs covers, including the dummy blocks past the
    /// image.
    stride: usize,
    /// The quantized coefficients of `stride × (mcus_y × v)` blocks, natural
    /// order.
    coefs: Vec<i16>,
    dc_pred: i32,
}

struct Frame {
    progressive: bool,
    precision: u8,
    width: u32,
    height: u32,
    hmax: usize,
    vmax: usize,
    mcus_x: usize,
    mcus_y: usize,
    components: Vec<Component>,
}

fn parse_frame(marker: u8, body: &[u8]) -> Result<Frame> {
    let progressive = marker == 0xC2;
    let [precision, h1, h0, w1, w0, count, rest @ ..] = body else {
        return Err(corrupt("SOF segment too short"));
    };
    let (precision, count) = (*precision, usize::from(*count));
    let height = u32::from(u16::from_be_bytes([*h1, *h0]));
    let width = u32::from(u16::from_be_bytes([*w1, *w0]));
    if precision != 8 && precision != 12 {
        return Err(unsupported(format!("precision {precision}")));
    }
    if width == 0 {
        return Err(corrupt("frame with width 0"));
    }
    if height == 0 {
        return Err(unsupported("frame with height 0 (DNL)"));
    }
    if !matches!(count, 1 | 3 | 4) {
        return Err(unsupported(format!("frame with {count} components")));
    }
    if rest.len() != 3 * count {
        return Err(corrupt("SOF segment length"));
    }
    // The limit, before anything is sized by the frame (docs/adr/0027).
    if u64::from(width) * u64::from(height) * count as u64 > MAX_SAMPLES {
        return Err(too_large());
    }
    let specs: Vec<(u8, usize, usize, u8)> = rest
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| (c[0], usize::from(c[1] >> 4), usize::from(c[1] & 15), c[2]))
        .collect();
    if specs
        .iter()
        .any(|&(_, h, v, tq)| !(1..=4).contains(&h) || !(1..=4).contains(&v) || tq > 3)
    {
        return Err(corrupt(
            "sampling factor or quantization table of a component",
        ));
    }
    if (1..specs.len()).any(|i| specs[..i].iter().any(|s| s.0 == specs[i].0)) {
        return Err(corrupt("two components with one id"));
    }
    let hmax = specs.iter().map(|s| s.1).max().unwrap_or(1);
    let vmax = specs.iter().map(|s| s.2).max().unwrap_or(1);
    if specs
        .iter()
        .any(|&(_, h, v, _)| hmax % h != 0 || vmax % v != 0)
    {
        return Err(unsupported(
            "sampling factors that do not divide the largest",
        ));
    }
    let mcus_x = (width as usize).div_ceil(8 * hmax);
    let mcus_y = (height as usize).div_ceil(8 * vmax);
    let components = specs
        .into_iter()
        .map(|(id, h, v, tq)| {
            let (cw, ch) = (
                (width as usize * h).div_ceil(hmax),
                (height as usize * v).div_ceil(vmax),
            );
            let stride = mcus_x * h;
            Component {
                id,
                h,
                v,
                tq,
                quant: None,
                blocks_w: cw.div_ceil(8),
                blocks_h: ch.div_ceil(8),
                width: cw,
                height: ch,
                stride,
                coefs: vec![0i16; stride * mcus_y * v * 64],
                dc_pred: 0,
            }
        })
        .collect();
    Ok(Frame {
        progressive,
        precision,
        width,
        height,
        hmax,
        vmax,
        mcus_x,
        mcus_y,
        components,
    })
}

// ---------------------------------------------------------------------------
// Scans.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Sequential,
    DcFirst,
    DcRefine,
    AcFirst,
    AcRefine,
}

struct Scan {
    /// The components in the scan: the index into the frame and the DC and
    /// AC tables (where the scan needs them).
    components: Vec<(usize, Option<usize>, Option<usize>)>,
    kind: Kind,
    ss: usize,
    se: usize,
    al: u32,
}

/// The tables of the stream: the quantization tables and the DC and AC
/// Huffman tables, by their numbers.
#[derive(Default)]
struct Tables {
    quant: [Option<[u16; 64]>; 4],
    dc: [Option<Huffman>; 4],
    ac: [Option<Huffman>; 4],
}

fn parse_dqt(body: &[u8], tables: &mut Tables) -> Result<()> {
    let mut rest = body;
    while let Some((&pq_tq, after)) = rest.split_first() {
        let (pq, tq) = (pq_tq >> 4, usize::from(pq_tq & 15));
        if tq > 3 || pq > 1 {
            return Err(corrupt("quantization table header"));
        }
        let size = if pq == 0 { 64 } else { 128 };
        let Some((data, after)) = after.split_at_checked(size) else {
            return Err(corrupt("DQT segment too short"));
        };
        let mut table = [0u16; 64];
        (0..64).for_each(|k| {
            table[ZIGZAG[k]] = if pq == 0 {
                u16::from(data[k])
            } else {
                u16::from_be_bytes([data[2 * k], data[2 * k + 1]])
            };
        });
        tables.quant[tq] = Some(table);
        rest = after;
    }
    Ok(())
}

fn parse_dht(body: &[u8], tables: &mut Tables) -> Result<()> {
    let mut rest = body;
    while let Some((&tc_th, after)) = rest.split_first() {
        let (tc, th) = (tc_th >> 4, usize::from(tc_th & 15));
        if tc > 1 || th > 3 {
            return Err(corrupt("Huffman table header"));
        }
        let Some((counts, after)) = after.split_at_checked(16) else {
            return Err(corrupt("DHT segment too short"));
        };
        let counts: [u8; 16] = counts.try_into().unwrap_or([0; 16]);
        let total: usize = counts.iter().map(|&c| usize::from(c)).sum();
        if total > 256 {
            return Err(corrupt("Huffman table with over 256 values"));
        }
        let Some((values, after)) = after.split_at_checked(total) else {
            return Err(corrupt("DHT segment too short"));
        };
        let table = Huffman::new(&counts, values)?;
        if tc == 0 {
            tables.dc[th] = Some(table);
        } else {
            tables.ac[th] = Some(table);
        }
        rest = after;
    }
    Ok(())
}

fn parse_sos(body: &[u8], frame: &mut Frame, tables: &Tables) -> Result<Scan> {
    let Some((&count, rest)) = body.split_first() else {
        return Err(corrupt("SOS segment too short"));
    };
    let count = usize::from(count);
    if !(1..=4).contains(&count) || count > frame.components.len() {
        return Err(corrupt("number of components in a scan"));
    }
    if rest.len() != 2 * count + 3 {
        return Err(corrupt("SOS segment length"));
    }
    let (specs, params) = rest.split_at(2 * count);
    let (ss, se, ah, al) = (
        usize::from(params[0]),
        usize::from(params[1]),
        u32::from(params[2] >> 4),
        u32::from(params[2] & 15),
    );
    let kind = if frame.progressive {
        let dc = ss == 0;
        let bad = (dc && se != 0)
            || (!dc && (ss > se || se > 63 || count != 1))
            || (ah != 0 && al != ah - 1)
            || al > 13;
        if bad {
            return Err(corrupt(format!(
                "progression parameters Ss {ss} Se {se} Ah {ah} Al {al}"
            )));
        }
        match (dc, ah == 0) {
            (true, true) => Kind::DcFirst,
            (true, false) => Kind::DcRefine,
            (false, true) => Kind::AcFirst,
            (false, false) => Kind::AcRefine,
        }
    } else {
        if ss != 0 || se != 63 || ah != 0 || al != 0 {
            return Err(corrupt("scan parameters of a sequential frame"));
        }
        Kind::Sequential
    };
    let needs_dc = matches!(kind, Kind::Sequential | Kind::DcFirst);
    let needs_ac = matches!(kind, Kind::Sequential | Kind::AcFirst | Kind::AcRefine);
    let mut components = Vec::with_capacity(count);
    for spec in specs.as_chunks::<2>().0 {
        let (id, td, ta) = (
            spec[0],
            usize::from(spec[1] >> 4),
            usize::from(spec[1] & 15),
        );
        let Some(index) = frame.components.iter().position(|c| c.id == id) else {
            return Err(corrupt(format!("scan of an unknown component {id}")));
        };
        if components.iter().any(|&(i, _, _)| i == index) {
            return Err(corrupt("a component twice in a scan"));
        }
        if td > 3 || ta > 3 {
            return Err(corrupt("Huffman table number of a scan"));
        }
        if needs_dc && tables.dc[td].is_none() {
            return Err(corrupt(format!("no DC Huffman table {td}")));
        }
        if needs_ac && tables.ac[ta].is_none() {
            return Err(corrupt(format!("no AC Huffman table {ta}")));
        }
        // The quantization table is latched at the component's first scan,
        // as libjpeg does.
        let component = &mut frame.components[index];
        if component.quant.is_none() {
            let tq = usize::from(component.tq);
            component.quant = Some(
                tables.quant[tq].ok_or_else(|| corrupt(format!("no quantization table {tq}")))?,
            );
        }
        components.push((index, needs_dc.then_some(td), needs_ac.then_some(ta)));
    }
    Ok(Scan {
        components,
        kind,
        ss,
        se,
        al,
    })
}

/// The DC predictor after a difference; an error where it leaves i32.
#[inline]
fn predict(dc_pred: i32, diff: i32) -> Result<i32> {
    dc_pred
        .checked_add(diff)
        .ok_or_else(|| corrupt("DC predictor out of range"))
}

/// Stores a coefficient, which must fit the 16 bits libjpeg keeps.
#[inline]
fn store(block: &mut [i16], k: usize, v: i64) -> Result<()> {
    block[k] = i16::try_from(v).map_err(|_| corrupt("coefficient out of range"))?;
    Ok(())
}

fn decode_sequential(
    input: &mut BitReader,
    block: &mut [i16],
    dc_pred: &mut i32,
    dc: &Huffman,
    ac: &Huffman,
) -> Result<()> {
    let t = dc.decode(input)?;
    let diff = input.receive_extend(u32::from(t))?;
    *dc_pred = predict(*dc_pred, diff)?;
    store(block, 0, i64::from(*dc_pred))?;
    let mut k = 1;
    while k < 64 {
        let rs = ac.decode(input)?;
        let (r, s) = (usize::from(rs >> 4), u32::from(rs & 15));
        if s == 0 {
            if r == 15 {
                k += 16;
                continue;
            }
            break;
        }
        k += r;
        if k > 63 {
            return Err(corrupt("coefficient index past the block"));
        }
        let v = input.receive_extend(s)?;
        store(block, ZIGZAG[k], i64::from(v))?;
        k += 1;
    }
    Ok(())
}

fn decode_dc_first(
    input: &mut BitReader,
    block: &mut [i16],
    dc_pred: &mut i32,
    dc: &Huffman,
    al: u32,
) -> Result<()> {
    let t = dc.decode(input)?;
    let diff = input.receive_extend(u32::from(t))?;
    *dc_pred = predict(*dc_pred, diff)?;
    store(block, 0, i64::from(*dc_pred) << al)
}

fn decode_dc_refine(input: &mut BitReader, block: &mut [i16], al: u32) -> Result<()> {
    if input.bits(1)? == 1 {
        block[0] |= 1 << al;
    }
    Ok(())
}

fn decode_ac_first(
    input: &mut BitReader,
    block: &mut [i16],
    eobrun: &mut u32,
    ac: &Huffman,
    scan: &Scan,
) -> Result<()> {
    if *eobrun > 0 {
        *eobrun -= 1;
        return Ok(());
    }
    let mut k = scan.ss;
    while k <= scan.se {
        let rs = ac.decode(input)?;
        let (r, s) = (u32::from(rs >> 4), u32::from(rs & 15));
        if s == 0 {
            if r == 15 {
                k += 16;
                continue;
            }
            *eobrun = (1 << r) + input.bits(r)? - 1;
            break;
        }
        k += r as usize;
        if k > scan.se {
            return Err(corrupt("coefficient index past the band"));
        }
        let v = input.receive_extend(s)?;
        store(block, ZIGZAG[k], i64::from(v) << scan.al)?;
        k += 1;
    }
    Ok(())
}

fn decode_ac_refine(
    input: &mut BitReader,
    block: &mut [i16],
    eobrun: &mut u32,
    ac: &Huffman,
    scan: &Scan,
) -> Result<()> {
    let p1: i16 = 1 << scan.al;
    let m1: i16 = -1 << scan.al;
    // Appends the correction bit of a coefficient that is already nonzero.
    let refine = |input: &mut BitReader, coef: &mut i16| -> Result<()> {
        if input.bits(1)? == 1 && *coef & p1 == 0 {
            *coef = coef
                .checked_add(if *coef >= 0 { p1 } else { m1 })
                .ok_or_else(|| corrupt("coefficient out of range"))?;
        }
        Ok(())
    };
    let mut k = scan.ss;
    if *eobrun == 0 {
        while k <= scan.se {
            let rs = ac.decode(input)?;
            let (mut r, s) = (i32::from(rs >> 4), u32::from(rs & 15));
            let new = if s != 0 {
                if s != 1 {
                    return Err(corrupt("refinement of a coefficient by more than a bit"));
                }
                Some(if input.bits(1)? == 1 { p1 } else { m1 })
            } else {
                if r != 15 {
                    *eobrun = (1 << r) + input.bits(r as u32)?;
                    break;
                }
                None
            };
            // Over the nonzero coefficients, refining them, and r zero ones.
            loop {
                let coef = &mut block[ZIGZAG[k]];
                if *coef != 0 {
                    refine(input, coef)?;
                } else {
                    r -= 1;
                    if r < 0 {
                        break;
                    }
                }
                k += 1;
                if k > scan.se {
                    break;
                }
            }
            if let Some(v) = new {
                if k > scan.se {
                    return Err(corrupt("coefficient index past the band"));
                }
                block[ZIGZAG[k]] = v;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= scan.se {
            let coef = &mut block[ZIGZAG[k]];
            if *coef != 0 {
                refine(input, coef)?;
            }
            k += 1;
        }
        *eobrun -= 1;
    }
    Ok(())
}

/// Decodes the entropy-coded data of a scan that starts at `pos`, and
/// returns the position after it (at the marker that follows).
fn decode_scan(
    data: &[u8],
    pos: usize,
    frame: &mut Frame,
    tables: &Tables,
    scan: &Scan,
    restart_interval: usize,
) -> Result<usize> {
    let mut input = BitReader::new(data, pos);
    let single = scan.components.len() == 1;
    let (mcus_x, mcus_y) = if single {
        let c = &frame.components[scan.components[0].0];
        (c.blocks_w, c.blocks_h)
    } else {
        (frame.mcus_x, frame.mcus_y)
    };
    // The run of end-of-bands of a progressive AC scan (T.81 G.1.2.2).
    let mut eobrun = 0u32;
    frame.components.iter_mut().for_each(|c| c.dc_pred = 0);
    let block = |input: &mut BitReader,
                 frame: &mut Frame,
                 eobrun: &mut u32,
                 (index, td, ta): (usize, Option<usize>, Option<usize>),
                 row: usize,
                 col: usize|
     -> Result<()> {
        let component = &mut frame.components[index];
        let at = (row * component.stride + col) * 64;
        let block = &mut component.coefs[at..at + 64];
        let dc = td.and_then(|t| tables.dc[t].as_ref());
        let ac = ta.and_then(|t| tables.ac[t].as_ref());
        match (scan.kind, dc, ac) {
            (Kind::Sequential, Some(dc), Some(ac)) => {
                decode_sequential(input, block, &mut component.dc_pred, dc, ac)
            }
            (Kind::DcFirst, Some(dc), _) => {
                decode_dc_first(input, block, &mut component.dc_pred, dc, scan.al)
            }
            (Kind::DcRefine, _, _) => decode_dc_refine(input, block, scan.al),
            (Kind::AcFirst, _, Some(ac)) => decode_ac_first(input, block, eobrun, ac, scan),
            (Kind::AcRefine, _, Some(ac)) => decode_ac_refine(input, block, eobrun, ac, scan),
            _ => Err(corrupt("no Huffman table for the scan")),
        }
    };
    for m in 0..mcus_x * mcus_y {
        if restart_interval > 0 && m > 0 && m % restart_interval == 0 {
            let (marker, after) = input.marker()?;
            let expected = 0xD0 + ((m / restart_interval - 1) % 8) as u8;
            if marker != expected {
                return Err(corrupt(format!(
                    "marker FF{marker:02X} where RST{} was expected",
                    expected - 0xD0
                )));
            }
            input = BitReader::new(data, after);
            eobrun = 0;
            frame.components.iter_mut().for_each(|c| c.dc_pred = 0);
        }
        let (mx, my) = (m % mcus_x, m / mcus_x);
        if single {
            block(&mut input, frame, &mut eobrun, scan.components[0], my, mx)?;
        } else {
            for &spec in &scan.components {
                let (h, v) = {
                    let c = &frame.components[spec.0];
                    (c.h, c.v)
                };
                for by in 0..v {
                    for bx in 0..h {
                        block(
                            &mut input,
                            frame,
                            &mut eobrun,
                            spec,
                            my * v + by,
                            mx * h + bx,
                        )?;
                    }
                }
            }
        }
    }
    // The position of the marker after the scan; it is read by the caller.
    let (_, after) = input.marker()?;
    Ok(after - 2)
}

// ---------------------------------------------------------------------------
// The stream of markers.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Transform {
    None,
    YCbCr,
    Ycck,
}

/// The colour transform of docs/adr/0027: APP14's `transform` when there is
/// one, otherwise YCbCr for three components and none for one and four.
fn transform(components: usize, adobe: Option<u8>) -> Result<Transform> {
    match (components, adobe) {
        (1, _) => Ok(Transform::None),
        (3, None | Some(1)) => Ok(Transform::YCbCr),
        (4, Some(2)) => Ok(Transform::Ycck),
        (_, None | Some(0)) => Ok(Transform::None),
        (_, Some(t)) => Err(unsupported(format!(
            "APP14 transform {t} with {components} components"
        ))),
    }
}

struct Parsed {
    frame: Frame,
    adobe: Option<u8>,
}

fn parse(data: &[u8]) -> Result<Parsed> {
    if data.get(..2) != Some(&[0xFF, 0xD8]) {
        return Err(corrupt("no SOI marker"));
    }
    let mut pos = 2;
    let mut tables = Tables::default();
    let mut frame: Option<Frame> = None;
    let mut adobe = None;
    let mut restart_interval = 0usize;
    let mut scans = 0usize;
    loop {
        // Fill bytes may precede a marker.
        while data.get(pos..pos + 2) == Some(&[0xFF, 0xFF]) {
            pos += 1;
        }
        let marker = match data.get(pos..pos + 2) {
            Some(&[0xFF, m]) if m != 0 => m,
            Some(_) => return Err(corrupt("a marker was expected")),
            None => return Err(corrupt("data ends before EOI")),
        };
        pos += 2;
        match marker {
            0xD9 => break,
            0xD8 => return Err(corrupt("SOI marker twice")),
            0xD0..=0xD7 => return Err(corrupt("RST marker outside a scan")),
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                return Err(unsupported(format!("SOF{}", marker - 0xC0)));
            }
            0x01..=0xBF => return Err(corrupt(format!("marker FF{marker:02X}"))),
            _ => {}
        }
        let length = match data.get(pos..pos + 2) {
            Some(&[a, b]) => usize::from(u16::from_be_bytes([a, b])),
            _ => return Err(corrupt("data ends in a marker segment")),
        };
        if length < 2 {
            return Err(corrupt("marker segment shorter than its length field"));
        }
        let Some(body) = data.get(pos + 2..pos + length) else {
            return Err(corrupt("data ends in a marker segment"));
        };
        pos += length;
        match marker {
            0xC0..=0xC2 => {
                if frame.is_some() {
                    return Err(corrupt("a second frame"));
                }
                frame = Some(parse_frame(marker, body)?);
            }
            0xC4 => parse_dht(body, &mut tables)?,
            0xDB => parse_dqt(body, &mut tables)?,
            0xDD => {
                let [a, b] = body else {
                    return Err(corrupt("DRI segment length"));
                };
                restart_interval = usize::from(u16::from_be_bytes([*a, *b]));
            }
            0xDC => return Err(unsupported("DNL marker")),
            0xDA => {
                let Some(frame) = frame.as_mut() else {
                    return Err(corrupt("scan before the frame"));
                };
                let scan = parse_sos(body, frame, &tables)?;
                pos = decode_scan(data, pos, frame, &tables, &scan, restart_interval)?;
                scans += 1;
            }
            0xEE if body.len() >= 12 && &body[..5] == b"Adobe" => {
                if body[11] > 2 {
                    return Err(unsupported(format!("APP14 transform {}", body[11])));
                }
                adobe = Some(body[11]);
            }
            // APPn, COM, DHP/EXP (hierarchical, refused by their SOF), JPGn.
            _ => {}
        }
    }
    let frame = frame.ok_or_else(|| corrupt("no frame"))?;
    if scans == 0 {
        return Err(corrupt("no scan"));
    }
    Ok(Parsed { frame, adobe })
}

// ---------------------------------------------------------------------------
// From coefficients to samples.

/// The samples of a component at its own resolution: `stride` samples a
/// row, `blocks_h × 8` rows.
struct Plane {
    stride: usize,
    samples: Vec<u16>,
}

fn plane(component: &mut Component, precision: u8) -> Result<Plane> {
    let quant = component
        .quant
        .ok_or_else(|| corrupt(format!("component {} has no scan", component.id)))?;
    let stride = component.blocks_w * 8;
    let mut samples = vec![0u16; stride * component.blocks_h * 8];
    let coefs = std::mem::take(&mut component.coefs);
    (0..component.blocks_h).for_each(|by| {
        (0..component.blocks_w).for_each(|bx| {
            let at = (by * component.stride + bx) * 64;
            // The product is at most 2^31 for hostile data; it saturates where
            // libjpeg's int would wrap.
            let block: [i32; 64] = std::array::from_fn(|k| {
                (i64::from(coefs[at + k]) * i64::from(quant[k]))
                    .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
            });
            let out = idct_islow(&block, precision);
            (0..8).for_each(|y| {
                let row = (by * 8 + y) * stride + bx * 8;
                samples[row..row + 8].copy_from_slice(&out[y * 8..y * 8 + 8]);
            });
        });
    });
    Ok(Plane { stride, samples })
}

/// How a component is brought to the full resolution (jdsample.c's choice).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Upsampling {
    Full,
    /// Replication by the factors (jdsample.c's `int_upsample`, `h2v1_upsample`,
    /// `h2v2_upsample`).
    Replicate(usize, usize),
    H2v1Fancy,
    H2v2Fancy,
}

/// docs/adr/0027: the fancy upsampling for 2 × 1 and 2 × 2, replication for
/// the other ratios. libjpeg only interpolates a component wider than two
/// samples, so a narrower one is replicated too.
fn upsampling(hx: usize, vx: usize, width: usize) -> Upsampling {
    match (hx, vx) {
        (1, 1) => Upsampling::Full,
        (2, 1) if width > 2 => Upsampling::H2v1Fancy,
        (2, 2) if width > 2 => Upsampling::H2v2Fancy,
        _ => Upsampling::Replicate(hx, vx),
    }
}

/// Writes the samples of row `y` of the image for one component into
/// `out`, which has at least `2 × width` samples.
fn upsample_row(
    plane: &Plane,
    component: &Component,
    method: Upsampling,
    y: usize,
    width: usize,
    out: &mut [u16],
) {
    let row = |r: usize| &plane.samples[r * plane.stride..r * plane.stride + component.width];
    match method {
        Upsampling::Full => out[..width].copy_from_slice(&row(y)[..width]),
        Upsampling::Replicate(hx, vx) => {
            let input = row(y / vx);
            (0..width).for_each(|x| out[x] = input[x / hx]);
        }
        Upsampling::H2v1Fancy => h2v1_fancy(row(y), out),
        Upsampling::H2v2Fancy => {
            let r = y / 2;
            let far = if y.is_multiple_of(2) {
                r.saturating_sub(1)
            } else {
                (r + 1).min(component.height - 1)
            };
            h2v2_fancy(row(r), row(far), out);
        }
    }
}

/// Decodes a JPEG stream into its samples (docs/adr/0027).
pub fn decode(data: &[u8]) -> Result<Image> {
    let Parsed { mut frame, adobe } = parse(data)?;
    let transform = transform(frame.components.len(), adobe)?;
    let precision = frame.precision;
    let (width, height) = (frame.width as usize, frame.height as usize);
    let count = frame.components.len();
    let planes = frame
        .components
        .iter_mut()
        .map(|c| plane(c, precision))
        .collect::<Result<Vec<Plane>>>()?;
    let methods: Vec<Upsampling> = frame
        .components
        .iter()
        .map(|c| upsampling(frame.hmax / c.h, frame.vmax / c.v, c.width))
        .collect();
    let max = (1u16 << precision) - 1;
    let mut rows = vec![vec![0u16; 2 * width + 2]; count];
    let mut samples = vec![0u8; width * height * count];
    (0..height).for_each(|y| {
        frame
            .components
            .iter()
            .zip(&planes)
            .zip(&methods)
            .zip(rows.iter_mut())
            .for_each(|(((c, p), &m), row)| upsample_row(p, c, m, y, width, row));
        let out = &mut samples[y * width * count..(y + 1) * width * count];
        (0..width).for_each(|x| {
            let pixel = &mut out[x * count..(x + 1) * count];
            match transform {
                Transform::None => {
                    (0..count).for_each(|k| pixel[k] = to_eight_bits(rows[k][x], precision));
                }
                Transform::YCbCr => {
                    let rgb = ycbcr_to_rgb([rows[0][x], rows[1][x], rows[2][x]], precision);
                    (0..3).for_each(|k| pixel[k] = to_eight_bits(rgb[k], precision));
                }
                Transform::Ycck => {
                    let rgb = ycbcr_to_rgb([rows[0][x], rows[1][x], rows[2][x]], precision);
                    (0..3).for_each(|k| pixel[k] = to_eight_bits(max - rgb[k], precision));
                    pixel[3] = to_eight_bits(rows[3][x], precision);
                }
            }
        });
    });
    Ok(Image {
        width: frame.width,
        height: frame.height,
        components: count as u8,
        precision,
        samples,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod vectors;
