//! The images of `Do`: their dictionaries, their samples as 8-bit RGB
//! with the opacity of an `SMask`, and their drawing by bilinear
//! interpolation (docs/adr/0027).
//!
//! [`load`] reads an image XObject and decodes it to the end, so that the
//! check of a page finds every error of an image before anything is
//! drawn. [`Mapping`] maps the center of a device pixel back into the
//! image by the exact inverse of the CTM, truncated to 2^−16 sample, and
//! [`draw`] fills the parallelogram of the image with the interpolated
//! samples, within the clip. Every computation is in integers.

use crate::clip::{Clip, mul_alpha};
use crate::color::Device;
use crate::error::{Error, Result};
use crate::fixed::Matrix;
use crate::jpeg;
use crate::pdf::{Dict, Document, Object, Stream};
use crate::raster::{Canvas, FillRule, Path, Scratch, composite, visit_rows};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// The most samples (`Width × Height × components`) an image may have
/// (docs/adr/0027).
pub const MAX_SAMPLES: u64 = jpeg::MAX_SAMPLES;

/// A decoded image: 8-bit RGB, row by row from the top, and the opacity
/// of its `SMask` at the size of the mask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    /// `width × height × 3` bytes.
    pub rgb: Vec<u8>,
    pub mask: Option<Mask>,
}

/// The opacity of an `SMask`: `width × height` bytes, row by row from
/// the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    pub width: usize,
    pub height: usize,
    pub alpha: Vec<u8>,
}

impl Picture {
    /// The bytes of the samples and the mask.
    pub fn bytes(&self) -> usize {
        self.rgb.len() + self.mask.as_ref().map_or(0, |m| m.alpha.len())
    }
}

fn unsupported(key: &str, value: &Object) -> Error {
    Error::new(format!("unsupported {key} {}", value.describe()))
}

/// The error of an image over [`MAX_SAMPLES`] samples.
fn too_large() -> Error {
    jpeg::too_large()
}

/// The color space of an image: a device space, or an `Indexed` space
/// over one (ISO 32000-1 8.6.6.3).
enum Space {
    Device(Device),
    Indexed {
        base: Device,
        hival: u8,
        table: Vec<u8>,
    },
}

impl Space {
    /// The number of components of a sample.
    fn components(&self) -> usize {
        match self {
            Space::Device(d) => d.components(),
            Space::Indexed { .. } => 1,
        }
    }

    fn load(doc: &Document, obj: &Object) -> Result<Space> {
        let space = doc.resolve(obj)?;
        let items = match &space {
            Object::Array(a) if a.first().and_then(Object::as_name) == Some(b"Indexed") => a,
            _ => return Device::load(doc, &space).map(Space::Device),
        };
        let [_, base, hival, table] = items.as_slice() else {
            return Err(Error::new("malformed Indexed color space"));
        };
        let base = Device::load(doc, base)?;
        let hival = doc
            .resolve(hival)?
            .as_int()
            .and_then(|h| u8::try_from(h).ok())
            .ok_or_else(|| Error::new("malformed hival of an Indexed color space"))?;
        let table = match doc.resolve(table)? {
            Object::String(s) => s,
            other => {
                return Err(Error::new(format!(
                    "unsupported lookup table of an Indexed color space: {}",
                    other.describe()
                )));
            }
        };
        if table.len() != (usize::from(hival) + 1) * base.components() {
            return Err(Error::new(
                "the lookup table of an Indexed color space does not have (hival + 1) × n bytes",
            ));
        }
        Ok(Space::Indexed { base, hival, table })
    }
}

/// The filter of an image: none, `FlateDecode` or `DCTDecode`
/// (docs/adr/0027).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    None,
    Flate,
    Dct,
}

fn filter(doc: &Document, dict: &Dict) -> Result<Filter> {
    if let Some(parms) = doc.get(dict, "DecodeParms")? {
        return Err(unsupported("DecodeParms", &parms));
    }
    let filters = match doc.get(dict, "Filter")? {
        None => vec![],
        Some(Object::Array(a)) => a
            .iter()
            .map(|f| doc.resolve(f))
            .collect::<Result<Vec<_>>>()?,
        Some(f) => vec![f],
    };
    match filters.as_slice() {
        [] => Ok(Filter::None),
        [f] if f.as_name() == Some(b"FlateDecode") => Ok(Filter::Flate),
        [f] if f.as_name() == Some(b"DCTDecode") => Ok(Filter::Dct),
        [f] => Err(unsupported("Filter", f)),
        many => Err(unsupported("Filter", &Object::Array(many.to_vec()))),
    }
}

/// A positive integer entry.
fn positive(doc: &Document, dict: &Dict, key: &str) -> Result<usize> {
    match doc.get(dict, key)? {
        Some(Object::Int(n)) if n > 0 => {
            usize::try_from(n).map_err(|_| Error::new(format!("{key} too large")))
        }
        Some(other) => Err(Error::new(format!(
            "{key} of an image is not a positive integer: {}",
            other.describe()
        ))),
        None => Err(Error::new(format!("an image without {key}"))),
    }
}

/// Whether the `Decode` array inverts the samples: `None` or the default
/// array `[0 1 …]` (`[0 hival]` for `Indexed`) is `false`, `[1 0 …]` is
/// `true`, and any other array is an error (docs/adr/0027).
fn inverted(doc: &Document, dict: &Dict, space: &Space) -> Result<bool> {
    let Some(decode) = doc.get(dict, "Decode")? else {
        return Ok(false);
    };
    let values = match &decode {
        Object::Array(a) => a
            .iter()
            .map(|v| doc.number(v))
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(unsupported("Decode", &decode)),
    };
    let n = space.components();
    let top = match space {
        Space::Device(_) => crate::fixed::Fixed::ONE,
        Space::Indexed { hival, .. } => crate::fixed::Fixed::from_int(i64::from(*hival))?,
    };
    let pairs = || values.chunks(2);
    if values.len() != 2 * n {
        return Err(unsupported("Decode", &decode));
    }
    if pairs().all(|p| p[0] == crate::fixed::Fixed::ZERO && p[1] == top) {
        return Ok(false);
    }
    if matches!(space, Space::Device(_))
        && pairs().all(|p| p[0] == crate::fixed::Fixed::ONE && p[1] == crate::fixed::Fixed::ZERO)
    {
        return Ok(true);
    }
    Err(unsupported("Decode", &decode))
}

/// The keys of the dictionary of an image, and the mask they are read in:
/// the image, its `SMask`, or both (docs/adr/0027).
const IMAGE: u8 = 1;
const SMASK: u8 = 2;
const KEYS: [(&[u8], u8); 12] = [
    (b"Type", IMAGE | SMASK),
    (b"Subtype", IMAGE | SMASK),
    (b"Width", IMAGE | SMASK),
    (b"Height", IMAGE | SMASK),
    (b"BitsPerComponent", IMAGE | SMASK),
    (b"ColorSpace", IMAGE | SMASK),
    (b"Filter", IMAGE | SMASK),
    (b"Decode", IMAGE | SMASK),
    (b"Length", IMAGE | SMASK),
    (b"DecodeParms", IMAGE | SMASK),
    (b"SMask", IMAGE),
    (b"Intent", IMAGE),
];

/// Checks the keys of an image or an SMask: `ImageMask` and
/// `Interpolate` may be false, and any other key not in [`KEYS`] is an
/// error (`Mask`, `Matte` and the rest).
fn check_keys(dict: &Dict, kind: u8) -> Result<()> {
    dict.iter().try_for_each(|(k, v)| {
        if KEYS
            .iter()
            .any(|(key, kinds)| *key == k.as_slice() && kinds & kind != 0)
        {
            return Ok(());
        }
        if kind == IMAGE && (k == b"ImageMask" || k == b"Interpolate") && *v == Object::Bool(false)
        {
            return Ok(());
        }
        Err(unsupported(&String::from_utf8_lossy(k), v))
    })
}

/// The samples of an image, after the filter.
enum Samples {
    /// Packed rows of `bits` bits per component, a row's end on a byte.
    Packed { data: Vec<u8>, bits: u32 },
    /// 8-bit samples after the colour transform of the JPEG.
    Jpeg(Vec<u8>),
}

/// Reads the samples of `stream`, which has `width × height` pixels of
/// `components` components (docs/adr/0027).
fn samples(
    doc: &Document,
    stream: &Stream,
    width: usize,
    height: usize,
    components: usize,
    bits: u32,
) -> Result<Samples> {
    match filter(doc, &stream.dict)? {
        Filter::Dct => {
            let image = jpeg::decode(&stream.data)?;
            let check = |key: &str, dict: usize, frame: usize| {
                if dict != frame {
                    return Err(Error::new(format!(
                        "{key} {dict} of the image does not match the JPEG frame ({frame})"
                    )));
                }
                Ok(())
            };
            check("Width", width, image.width as usize)?;
            check("Height", height, image.height as usize)?;
            check(
                "BitsPerComponent",
                bits as usize,
                usize::from(image.precision),
            )?;
            if components != usize::from(image.components) {
                return Err(Error::new(format!(
                    "the color space of the image has {components} components but the JPEG frame {}",
                    image.components
                )));
            }
            Ok(Samples::Jpeg(image.samples))
        }
        f => {
            let row = (width * components * bits as usize).div_ceil(8);
            let expected = row * height;
            let data = match f {
                Filter::None => stream.data.clone(),
                _ => crate::flate::inflate_zlib(&stream.data, expected + 1)?,
            };
            if data.len() != expected {
                return Err(Error::new(
                    "the image data does not have the size of the image",
                ));
            }
            Ok(Samples::Packed { data, bits })
        }
    }
}

/// The component `i` of a packed row.
fn packed(row: &[u8], i: usize, bits: u32) -> u32 {
    match bits {
        8 => u32::from(row[i]),
        16 => u32::from(u16::from_be_bytes([row[2 * i], row[2 * i + 1]])),
        _ => {
            let bit = i * bits as usize;
            let shift = 8 - bits as usize - bit % 8;
            (u32::from(row[bit / 8]) >> shift) & ((1 << bits) - 1)
        }
    }
}

/// A sample of `bits` bits as 8 bits: `div_round(v × 255, 2^bits − 1)`,
/// never halfway since the divisor is odd.
fn eight_bits(v: u32, bits: u32) -> u8 {
    let max = (1u32 << bits) - 1;
    ((v * 255 + max / 2) / max) as u8
}

/// The dimensions and the number of samples of an image, within
/// [`MAX_SAMPLES`].
fn dimensions(doc: &Document, dict: &Dict, components: usize) -> Result<(usize, usize)> {
    let width = positive(doc, dict, "Width")?;
    let height = positive(doc, dict, "Height")?;
    let count = (width as u64)
        .checked_mul(height as u64)
        .and_then(|n| n.checked_mul(components as u64))
        .ok_or_else(too_large)?;
    if count > MAX_SAMPLES {
        return Err(too_large());
    }
    Ok((width, height))
}

fn bits_per_component(doc: &Document, dict: &Dict, allowed: &[i64]) -> Result<u32> {
    let bits = doc
        .get(dict, "BitsPerComponent")?
        .ok_or_else(|| Error::new("an image without BitsPerComponent"))?;
    match bits.as_int() {
        Some(b) if allowed.contains(&b) => Ok(b as u32),
        _ => Err(unsupported("BitsPerComponent", &bits)),
    }
}

/// Reads and decodes an image XObject (docs/adr/0027).
pub fn load(doc: &Document, stream: &Stream) -> Result<Picture> {
    let dict = &stream.dict;
    check_keys(dict, IMAGE)?;
    let space = doc
        .get(dict, "ColorSpace")?
        .ok_or_else(|| Error::new("an image without ColorSpace"))?;
    let space = Space::load(doc, &space)?;
    let n = space.components();
    let (width, height) = dimensions(doc, dict, n)?;
    let dct = filter(doc, dict)? == Filter::Dct;
    let allowed: &[i64] = match (&space, dct) {
        (_, true) => &[8, 12],
        (Space::Indexed { .. }, false) => &[1, 2, 4, 8],
        (Space::Device(_), false) => &[1, 2, 4, 8, 16],
    };
    let bits = bits_per_component(doc, dict, allowed)?;
    let invert = inverted(doc, dict, &space)?;
    let samples = samples(doc, stream, width, height, n, bits)?;
    let mut rgb = vec![0u8; width * height * 3];
    let mut comps = [0u8; 4];
    let convert = |comps: &mut [u8; 4], out: &mut [u8]| -> Result<()> {
        let color = match &space {
            Space::Device(d) => {
                if invert {
                    comps.iter_mut().for_each(|c| *c = 255 - *c);
                }
                d.rgb8(&comps[..n])
            }
            Space::Indexed { base, hival, table } => {
                let index = usize::from(comps[0]);
                if index > usize::from(*hival) {
                    return Err(Error::new(format!(
                        "index {index} out of range of an Indexed color space of hival {hival}"
                    )));
                }
                let m = base.components();
                base.rgb8(&table[index * m..index * m + m])
            }
        };
        out.copy_from_slice(&color);
        Ok(())
    };
    match &samples {
        Samples::Jpeg(data) => data
            .chunks_exact(n)
            .zip(rgb.as_chunks_mut::<3>().0.iter_mut())
            .try_for_each(|(pixel, out)| {
                comps[..n].copy_from_slice(pixel);
                convert(&mut comps, out)
            })?,
        Samples::Packed { data, bits } => {
            let row_bytes = (width * n * *bits as usize).div_ceil(8);
            let indexed = matches!(space, Space::Indexed { .. });
            data.chunks_exact(row_bytes)
                .zip(rgb.chunks_exact_mut(width * 3))
                .try_for_each(|(row, out)| {
                    out.as_chunks_mut::<3>()
                        .0
                        .iter_mut()
                        .enumerate()
                        .try_for_each(|(x, out)| {
                            (0..n).for_each(|k| {
                                let v = packed(row, x * n + k, *bits);
                                comps[k] = if indexed {
                                    v as u8
                                } else {
                                    eight_bits(v, *bits)
                                };
                            });
                            convert(&mut comps, out)
                        })
                })?;
        }
    }
    let mask = match dict.get(b"SMask".as_slice()) {
        None => None,
        Some(obj) => {
            let num = match obj {
                Object::Ref(r) => Some(r.num),
                _ => None,
            };
            let mask = match doc.resolve(obj)? {
                Object::Stream(s) => load_mask(doc, &s),
                other => Err(unsupported("SMask", &other)),
            };
            Some(mask.map_err(|e| num.map_or(e.clone(), |n| e.at_object(n)))?)
        }
    };
    Ok(Picture {
        width,
        height,
        rgb,
        mask,
    })
}

/// Reads an `SMask`: a DeviceGray image of 8 bits with no filter or
/// `FlateDecode`, which may have its own size (docs/adr/0027).
fn load_mask(doc: &Document, stream: &Stream) -> Result<Mask> {
    let dict = &stream.dict;
    check_keys(dict, SMASK)?;
    if let Some(sub) = doc.get(dict, "Subtype")?
        && sub.as_name() != Some(b"Image")
    {
        return Err(unsupported("Subtype", &sub));
    }
    let space = doc
        .get(dict, "ColorSpace")?
        .ok_or_else(|| Error::new("an SMask without ColorSpace"))?;
    if space.as_name() != Some(b"DeviceGray") {
        return Err(Error::new(format!(
            "unsupported ColorSpace {} of an SMask",
            space.describe()
        )));
    }
    let (width, height) = dimensions(doc, dict, 1)?;
    let bits = bits_per_component(doc, dict, &[8])?;
    if inverted(doc, dict, &Space::Device(Device::Gray))? {
        return Err(Error::new("unsupported Decode [1 0] of an SMask"));
    }
    if filter(doc, dict)? == Filter::Dct {
        return Err(Error::new("unsupported Filter DCTDecode of an SMask"));
    }
    let alpha = match samples(doc, stream, width, height, 1, bits)? {
        Samples::Packed { data, .. } => data,
        Samples::Jpeg(_) => unreachable!("the filter is not DCTDecode"),
    };
    Ok(Mask {
        width,
        height,
        alpha,
    })
}

/// The decoded images of a document, by their object numbers, shared by
/// the pages, the check and the rendering (docs/adr/0027). Whether an
/// image comes from the cache changes nothing of the output. The cache
/// holds at most [`PictureCache::BUDGET`] bytes of samples; an image
/// beyond it is decoded again when it is placed.
#[derive(Default)]
pub struct PictureCache {
    table: Mutex<BTreeMap<u32, Arc<Picture>>>,
    bytes: Mutex<usize>,
}

impl PictureCache {
    /// The most bytes of samples the cache keeps.
    pub const BUDGET: usize = 1 << 28;

    pub fn new() -> PictureCache {
        PictureCache::default()
    }

    /// The decoded image of object `num` if it is in the cache.
    pub fn cached(&self, num: u32) -> Option<Arc<Picture>> {
        self.table
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&num)
            .cloned()
    }

    /// The decoded image of object `num`, loaded by `load` if it is not
    /// in the cache.
    pub fn get(&self, num: u32, load: impl FnOnce() -> Result<Picture>) -> Result<Arc<Picture>> {
        if let Some(p) = self
            .table
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&num)
        {
            return Ok(p.clone());
        }
        let picture = Arc::new(load()?);
        let size = picture.bytes();
        let mut bytes = self.bytes.lock().unwrap_or_else(|e| e.into_inner());
        if *bytes + size <= Self::BUDGET {
            let mut table = self.table.lock().unwrap_or_else(|e| e.into_inner());
            if let std::collections::btree_map::Entry::Vacant(slot) = table.entry(num) {
                *bytes += size;
                slot.insert(picture.clone());
            }
        }
        Ok(picture)
    }
}

// ---------------------------------------------------------------------------
// The inverse mapping of pixels into the image.

/// A value of an axis in 2^−16 sample units, or one beyond the range of
/// i128 on one side, which is beyond every edge of the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Value {
    Exact { q: i128, r: i128 },
    Far(bool),
}

impl Value {
    /// The value in 2^−16 units, a far one as ±2^120.
    fn units(self) -> i128 {
        match self {
            Value::Exact { q, .. } => q,
            Value::Far(negative) => {
                if negative {
                    -(1 << 120)
                } else {
                    1 << 120
                }
            }
        }
    }
}

/// Values beyond this are far: beyond every edge of an image, whatever
/// the direction, with room for the steps.
const FAR: i128 = 1 << 120;

/// `floor(n × m / d)` and the remainder `n × m − floor × d`, exactly, for
/// `d > 0` and `0 ≤ m < 2^43` (docs/adr/0027). The product `n × m` may
/// exceed i128: the quotient of `n` by `d` is multiplied first, and the
/// remainder times `m` is divided bit by bit.
fn mul_div(n: i128, m: i128, d: i128) -> Value {
    debug_assert!(d > 0 && (0..1 << 43).contains(&m));
    let (qn, rn) = (n.div_euclid(d), n.rem_euclid(d));
    // floor(rn × m / d) with rn < d < 2^127: the bits of m from the top.
    let (mut q, mut acc) = (0i128, 0i128);
    (0..43).rev().for_each(|bit| {
        acc *= 2;
        q *= 2;
        if m >> bit & 1 == 1 {
            acc += rn;
        }
        while acc >= d {
            acc -= d;
            q += 1;
        }
    });
    match qn.checked_mul(m).and_then(|v| v.checked_add(q)) {
        Some(v) if v.abs() < FAR => Value::Exact { q: v, r: acc },
        _ => Value::Far(qn < 0),
    }
}

/// One axis of the inverse mapping: `floor(m (sx·x + sy·y + k) / d)` in
/// 2^−16 sample units for the pixel `(x, y)`, with `d > 0`.
#[derive(Clone, Copy, Debug)]
struct Axis {
    m: i128,
    d: i128,
    k: i128,
    sx: i128,
    sy: i128,
    /// The step of a pixel in x: `floor(m sx / d)` and its remainder, when
    /// they fit.
    step: Option<(i128, i128)>,
}

impl Axis {
    fn new(m: i128, d: i128, k: i128, sx: i128, sy: i128) -> Axis {
        let step = match mul_div(sx, m, d) {
            Value::Exact { q, r } => Some((q, r)),
            Value::Far(_) => None,
        };
        Axis {
            m,
            d,
            k,
            sx,
            sy,
            step,
        }
    }

    /// The value at a pixel. The numerator fits in i128: the bounds of
    /// [`Mapping::new`] hold its terms below 2^120.
    fn at(&self, x: usize, y: usize) -> Value {
        let n = self.sx * x as i128 + self.sy * y as i128 + self.k;
        mul_div(n, self.m, self.d)
    }

    /// The value one pixel to the right of an exact value.
    fn next(&self, v: Value, x: usize, y: usize) -> Value {
        let (Value::Exact { q, r }, Some((qs, rs))) = (v, self.step) else {
            return self.at(x, y);
        };
        let (mut q, mut r) = (q + qs, r + rs);
        if r >= self.d {
            r -= self.d;
            q += 1;
        }
        if q.abs() < FAR {
            Value::Exact { q, r }
        } else {
            self.at(x, y)
        }
    }
}

/// The inverse of the CTM of an image, from the centers of the pixels to
/// 2^−16 sample units of the image and of its mask (docs/adr/0027).
pub struct Mapping {
    u: Axis,
    v: Axis,
    mask: Option<(Axis, Axis)>,
}

/// The raw values of a CTM whose corners map within the device
/// (`to_device` of the unit square): |e|, |f| ≤ 2^55 and |a|, |b|, |c|,
/// |d| ≤ 2^56, rounded.
const RAW_BOUND: i128 = 1 << 57;

impl Mapping {
    /// The mapping of the unit square under `ctm` (whose entries are
    /// Q32.32) to a `width` × `height` image with a mask of `mask`.
    /// `None` if the CTM is singular, when nothing is drawn. An error if
    /// the entries are beyond the bounds that the corners imply.
    pub fn new(
        ctm: &Matrix,
        width: usize,
        height: usize,
        mask: Option<(usize, usize)>,
    ) -> Result<Option<Mapping>> {
        let [a, b, c, d, e, f] =
            [ctm.a, ctm.b, ctm.c, ctm.d, ctm.e, ctm.f].map(|v| i128::from(v.raw()));
        if [a, b, c, d, e, f].iter().any(|v| v.abs() > RAW_BOUND) {
            return Err(Error::new("image outside the range of coordinates"));
        }
        let det = a * d - b * c;
        if det == 0 {
            return Ok(None);
        }
        // The center of the pixel (0, 0) is (2^31, 2^31) in raw units, and
        // a pixel is 2^32.
        let half = 1i128 << 31;
        let one = 1i128 << 32;
        // s = (d (X − e) − c (Y − f)) / det, in samples u = W s
        let (mut ku, mut sxu, mut syu) = (d * (half - e) - c * (half - f), d * one, -c * one);
        // t = (−b (X − e) + a (Y − f)) / det, in samples v = H (1 − t)
        let (mut kv, mut sxv, mut syv) = (det + b * (half - e) - a * (half - f), b * one, -a * one);
        let mut d = det;
        if d < 0 {
            [
                &mut d, &mut ku, &mut sxu, &mut syu, &mut kv, &mut sxv, &mut syv,
            ]
            .iter_mut()
            .for_each(|v| **v = -**v);
        }
        let axes = |w: usize, h: usize| {
            (
                Axis::new((w as i128) << 16, d, ku, sxu, syu),
                Axis::new((h as i128) << 16, d, kv, sxv, syv),
            )
        };
        let (u, v) = axes(width, height);
        Ok(Some(Mapping {
            u,
            v,
            mask: mask.map(|(w, h)| axes(w, h)),
        }))
    }
}

/// The index of the sample before a coordinate and the weight of the
/// next one: `floor(c − 1/2)` and its fraction in 2^−16 units.
fn split(c: i128) -> (i128, i128) {
    let c = c - (1 << 15);
    (c.div_euclid(1 << 16), c.rem_euclid(1 << 16))
}

/// The bilinear interpolation of four samples at `(fx, fy)`, in 2^−16
/// units, rounded to the nearest, ties to even (docs/adr/0027).
fn bilinear(s00: u8, s10: u8, s01: u8, s11: u8, fx: i128, fy: i128) -> u8 {
    let (fx, fy) = (fx as u64, fy as u64);
    let weigh = |a: u8, b: u8| u64::from(a) * ((1 << 16) - fx) + u64::from(b) * fx;
    let sum = weigh(s00, s10) * ((1 << 16) - fy) + weigh(s01, s11) * fy;
    let (q, r) = (sum >> 32, sum & ((1 << 32) - 1));
    let up = r > 1 << 31 || (r == 1 << 31 && q & 1 == 1);
    (q + u64::from(up)) as u8
}

/// The four samples around `(u, v)`, clamped to the edges, and the
/// weights: `(i0, i1, j0, j1, fx, fy)`.
fn corners(
    u: i128,
    v: i128,
    width: usize,
    height: usize,
) -> (usize, usize, usize, usize, i128, i128) {
    let (i0, fx) = split(u);
    let (j0, fy) = split(v);
    let clamp = |i: i128, n: usize| i.clamp(0, n as i128 - 1) as usize;
    (
        clamp(i0, width),
        clamp(i0 + 1, width),
        clamp(j0, height),
        clamp(j0 + 1, height),
        fx,
        fy,
    )
}

/// The RGB of the image at `(u, v)` in 2^−16 sample units.
fn sample(picture: &Picture, u: i128, v: i128) -> [u8; 3] {
    let (i0, i1, j0, j1, fx, fy) = corners(u, v, picture.width, picture.height);
    let at = |i: usize, j: usize| (j * picture.width + i) * 3;
    let (p00, p10, p01, p11) = (at(i0, j0), at(i1, j0), at(i0, j1), at(i1, j1));
    let rgb = &picture.rgb;
    std::array::from_fn(|k| {
        bilinear(
            rgb[p00 + k],
            rgb[p10 + k],
            rgb[p01 + k],
            rgb[p11 + k],
            fx,
            fy,
        )
    })
}

/// The opacity of the mask at `(u, v)` in 2^−16 units of the mask.
fn sample_mask(mask: &Mask, u: i128, v: i128) -> u8 {
    let (i0, i1, j0, j1, fx, fy) = corners(u, v, mask.width, mask.height);
    let at = |i: usize, j: usize| mask.alpha[j * mask.width + i];
    bilinear(at(i0, j0), at(i1, j0), at(i0, j1), at(i1, j1), fx, fy)
}

/// Draws `picture` within `path`, the parallelogram of its unit square,
/// on the canvas: each pixel the path covers gets the interpolated color
/// at the center of the pixel, with the opacity `coverage × mask × clip`,
/// composited as a fill is (docs/adr/0027).
pub fn draw(
    canvas: &mut Canvas,
    path: &Path,
    mapping: &Mapping,
    picture: &Picture,
    clip: Option<&Clip>,
    scratch: &mut Scratch,
) {
    if clip.is_some_and(Clip::is_empty) {
        return;
    }
    let (width, height) = (canvas.width, canvas.height);
    let pixels = &mut canvas.pixels;
    visit_rows(
        path,
        FillRule::NonZero,
        width,
        height,
        scratch,
        |y, x0, alphas| {
            // The run within the clip, and the clip values of its pixels.
            let (range, clips) = match clip {
                None => (x0..x0 + alphas.len(), None),
                Some(c) => match c.span(y, x0, x0 + alphas.len()) {
                    Some((range, clips)) => (range, Some(clips)),
                    None => return,
                },
            };
            let alphas = &alphas[range.start - x0..range.end - x0];
            let mut u = mapping.u.at(range.start, y);
            let mut v = mapping.v.at(range.start, y);
            let mut m = mapping
                .mask
                .map(|(mu, mv)| (mu.at(range.start, y), mv.at(range.start, y)));
            alphas.iter().enumerate().for_each(|(i, &a)| {
                let x = range.start + i;
                let mut opacity = a;
                if let (Some(mask), Some((mu, mv))) = (&picture.mask, m) {
                    opacity = mul_alpha(opacity, sample_mask(mask, mu.units(), mv.units()));
                }
                if let Some(clips) = clips {
                    opacity = mul_alpha(opacity, clips[i]);
                }
                if opacity > 0 {
                    let rgb = sample(picture, u.units(), v.units());
                    let p = &mut pixels[(y * width + x) * 3..][..3];
                    match opacity {
                        255 => p.copy_from_slice(&rgb),
                        a => (0..3).for_each(|k| p[k] = composite(rgb[k], a, p[k])),
                    }
                }
                u = mapping.u.next(u, x + 1, y);
                v = mapping.v.next(v, x + 1, y);
                if let (Some((mu, mv)), Some((au, av))) = (&mut m, mapping.mask) {
                    *mu = au.next(*mu, x + 1, y);
                    *mv = av.next(*mv, x + 1, y);
                }
            });
        },
    );
}

#[cfg(test)]
mod tests;
