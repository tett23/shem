//! Fonts: the Type 0 fonts and CIDFonts of PDF, and the outlines of their
//! glyphs (docs/adr/0004).

pub mod cff;
pub mod truetype;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::error::{Error, Result};
use crate::fixed::{Fixed, Matrix};
use crate::pdf::{Dict, Document, Object};
use cff::Cff;
use truetype::TrueType;

/// A segment of a glyph's outline, in glyph units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    Move(Fixed, Fixed),
    Line(Fixed, Fixed),
    Curve([Fixed; 6]),
    Close,
}

enum Program {
    Cff(Cff),
    TrueType(TrueType),
}

/// Metrics of CIDs: ranges of CIDs and their values (W or W2, ISO 32000-1 9.7.4.3).
struct Metrics {
    ranges: Vec<(u32, u32, Vec<Fixed>)>,
}

impl Metrics {
    /// Reads W (`per` = 1) or W2 (`per` = 3).
    fn load(doc: &Document, obj: Option<Object>, per: usize) -> Result<Metrics> {
        let Some(obj) = obj else {
            return Ok(Metrics { ranges: Vec::new() });
        };
        let items = match doc.resolve(&obj)? {
            Object::Array(a) => a,
            other => {
                return Err(Error::new(format!(
                    "W or W2 is not an array: {}",
                    other.describe()
                )));
            }
        };
        let cid = |o: &Object| -> Result<u32> {
            o.as_int()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| Error::new("malformed W or W2: a CID is not a non-negative integer"))
        };
        let mut ranges = Vec::new();
        let mut i = 0;
        while i < items.len() {
            let first = cid(&items[i])?;
            match doc.resolve(
                items
                    .get(i + 1)
                    .ok_or_else(|| Error::new("malformed W or W2"))?,
            )? {
                Object::Array(values) => {
                    if values.len() % per != 0 {
                        return Err(Error::new(
                            "malformed W or W2: the values are not in groups",
                        ));
                    }
                    values.chunks(per).enumerate().try_for_each(|(k, group)| {
                        let c = first + k as u32;
                        let v = group
                            .iter()
                            .map(|o| doc.number(o))
                            .collect::<Result<Vec<_>>>()?;
                        ranges.push((c, c, v));
                        Ok::<_, Error>(())
                    })?;
                    i += 2;
                }
                last => {
                    let last = cid(&last)?;
                    let v = items
                        .get(i + 2..i + 2 + per)
                        .ok_or_else(|| Error::new("malformed W or W2"))?
                        .iter()
                        .map(|o| doc.number(o))
                        .collect::<Result<Vec<_>>>()?;
                    ranges.push((first, last, v));
                    i += 2 + per;
                }
            }
        }
        Ok(Metrics { ranges })
    }

    fn get(&self, cid: u32) -> Option<&[Fixed]> {
        self.ranges
            .iter()
            .find(|(a, b, _)| (*a..=*b).contains(&cid))
            .map(|(_, _, v)| v.as_slice())
    }
}

/// A Type 0 font with one CIDFont, as Golem embeds it.
pub struct Font {
    program: Program,
    /// The outline of each glyph ID, interpreted at its first use (docs/adr/0011).
    outlines: Vec<OnceLock<Result<Arc<[Segment]>>>>,
    /// How many outlines were interpreted, for the tests of the cache.
    interpretations: AtomicUsize,
    vertical: bool,
    widths: Metrics,
    default_width: Fixed,
    vertical_metrics: Metrics,
    /// DW2: the vertical component of the position vector and of the displacement.
    default_vertical: (Fixed, Fixed),
}

fn unsupported(key: &str, value: &Object) -> Error {
    Error::new(format!("unsupported {key} {}", value.describe()))
}

fn dict_of(doc: &Document, obj: &Object, what: &str) -> Result<Dict> {
    doc.resolve(obj)?
        .as_dict()
        .cloned()
        .ok_or_else(|| Error::new(format!("{what} is not a dictionary")))
}

impl Font {
    pub fn load(doc: &Document, obj: &Object) -> Result<Font> {
        let font = dict_of(doc, obj, "the font")?;
        match doc.get(&font, "Subtype")? {
            Some(Object::Name(n)) if n == b"Type0" => {}
            Some(other) => return Err(unsupported("font Subtype", &other)),
            None => return Err(Error::new("a font without a Subtype")),
        }
        let vertical = match doc.get(&font, "Encoding")? {
            Some(Object::Name(n)) if n == b"Identity-H" => false,
            Some(Object::Name(n)) if n == b"Identity-V" => true,
            Some(other) => return Err(unsupported("Encoding", &other)),
            None => return Err(Error::new("a Type 0 font without an Encoding")),
        };
        let descendant = match doc.get(&font, "DescendantFonts")? {
            Some(Object::Array(a)) if a.len() == 1 => dict_of(doc, &a[0], "the CIDFont")?,
            _ => return Err(Error::new("DescendantFonts is not an array of one CIDFont")),
        };
        let descriptor = doc
            .get_dict(&descendant, "FontDescriptor")?
            .ok_or_else(|| Error::new("a CIDFont without a FontDescriptor"))?;
        let program = match doc
            .get(&descendant, "Subtype")?
            .as_ref()
            .and_then(Object::as_name)
        {
            Some(b"CIDFontType0") => {
                let file = match doc.get(&descriptor, "FontFile3")? {
                    Some(Object::Stream(s)) => s,
                    _ => return Err(Error::new("font program not embedded: no FontFile3")),
                };
                match doc.get(&file.dict, "Subtype")? {
                    Some(Object::Name(n)) if n == b"CIDFontType0C" => {}
                    Some(other) => return Err(unsupported("FontFile3 Subtype", &other)),
                    None => return Err(Error::new("unsupported FontFile3 without a Subtype")),
                }
                Program::Cff(Cff::parse(&doc.stream_data(&file)?)?)
            }
            Some(b"CIDFontType2") => {
                match doc.get(&descendant, "CIDToGIDMap")? {
                    None => {}
                    Some(Object::Name(n)) if n == b"Identity" => {}
                    Some(other) => return Err(unsupported("CIDToGIDMap", &other)),
                }
                let file = match doc.get(&descriptor, "FontFile2")? {
                    Some(Object::Stream(s)) => s,
                    _ => return Err(Error::new("font program not embedded: no FontFile2")),
                };
                Program::TrueType(TrueType::parse(&doc.stream_data(&file)?)?)
            }
            _ => return Err(Error::new("unsupported CIDFont Subtype")),
        };
        let default_width = match doc.get(&descendant, "DW")? {
            Some(o) => doc.number(&o)?,
            None => Fixed::from_int(1000)?,
        };
        let default_vertical = match doc.get(&descendant, "DW2")? {
            Some(Object::Array(a)) if a.len() == 2 => (doc.number(&a[0])?, doc.number(&a[1])?),
            Some(other) => return Err(unsupported("DW2", &other)),
            None => (Fixed::from_int(880)?, Fixed::from_int(-1000)?),
        };
        let glyphs = match &program {
            Program::Cff(c) => c.glyph_count(),
            Program::TrueType(t) => t.glyph_count(),
        };
        Ok(Font {
            program,
            outlines: (0..glyphs).map(|_| OnceLock::new()).collect(),
            interpretations: AtomicUsize::new(0),
            vertical,
            widths: Metrics::load(doc, doc.get(&descendant, "W")?, 1)?,
            default_width,
            vertical_metrics: Metrics::load(doc, doc.get(&descendant, "W2")?, 3)?,
            default_vertical,
        })
    }

    pub fn is_vertical(&self) -> bool {
        self.vertical
    }

    /// The matrix from glyph space to text space: the FontMatrix of a CFF
    /// (always 1/1000 in v0.1), 1/unitsPerEm for TrueType.
    pub fn glyph_matrix(&self) -> Result<Matrix> {
        let s = match &self.program {
            Program::Cff(_) => Fixed::parse(b"0.001")?,
            Program::TrueType(t) => {
                Fixed::ONE.checked_div(Fixed::from_int(i64::from(t.units_per_em()))?)?
            }
        };
        Ok(Matrix::new([
            s,
            Fixed::ZERO,
            Fixed::ZERO,
            s,
            Fixed::ZERO,
            Fixed::ZERO,
        ]))
    }

    /// The glyph ID of a CID (ISO 32000-1 9.7.4.2).
    fn gid(&self, cid: u16) -> Result<u16> {
        match &self.program {
            Program::Cff(c) => c.gid(cid),
            Program::TrueType(_) => Ok(cid),
        }
    }

    /// The outline of the glyph of a CID, in glyph units, interpreted once
    /// per glyph and then reused (docs/adr/0011).
    pub fn outline(&self, cid: u16) -> Result<Arc<[Segment]>> {
        let gid = self.gid(cid)?;
        match self.outlines.get(usize::from(gid)) {
            Some(cell) => cell
                .get_or_init(|| self.interpret_outline(cid).map(Arc::from))
                .clone(),
            None => Err(Error::new(format!("glyph {gid} is not in the font"))),
        }
    }

    /// The outline of the glyph of a CID, interpreted now, without the cache.
    pub fn interpret_outline(&self, cid: u16) -> Result<Vec<Segment>> {
        self.interpretations.fetch_add(1, Ordering::Relaxed);
        match &self.program {
            Program::Cff(c) => c.outline(c.gid(cid)?),
            Program::TrueType(t) => t.outline(cid),
        }
    }

    /// How many outlines were interpreted.
    pub fn interpretations(&self) -> usize {
        self.interpretations.load(Ordering::Relaxed)
    }

    /// The horizontal displacement w0 of a CID, in 1/1000 em.
    pub fn width(&self, cid: u16) -> Fixed {
        self.widths
            .get(u32::from(cid))
            .map_or(self.default_width, |v| v[0])
    }

    /// The vertical displacement w1 and the position vector (vx, vy) of a
    /// CID, in 1/1000 em. Without W2, vx is half of w0 (9.7.4.3).
    pub fn vertical_metrics(&self, cid: u16) -> Result<(Fixed, Fixed, Fixed)> {
        match self.vertical_metrics.get(u32::from(cid)) {
            Some(v) => Ok((v[0], v[1], v[2])),
            None => {
                let vx = self.width(cid).checked_div(Fixed::from_int(2)?)?;
                Ok((self.default_vertical.1, vx, self.default_vertical.0))
            }
        }
    }
}

/// The font of one object, read at its first use; an error is kept too.
type FontCell = Arc<OnceLock<Result<Arc<Font>>>>;

/// The fonts of one document, read once each and shared by the pages, the
/// check and the rendering, and the threads (docs/adr/0011). A font is keyed
/// by the object number of its Type 0 font dictionary; a font given directly
/// in a resource dictionary is not kept.
#[derive(Default)]
pub struct FontCache {
    table: Mutex<BTreeMap<u32, FontCell>>,
    loads: AtomicUsize,
}

impl FontCache {
    pub fn new() -> FontCache {
        FontCache::default()
    }

    /// The font of a resource: from the cache if it is an indirect reference.
    pub fn get(&self, doc: &Document, obj: &Object) -> Result<Arc<Font>> {
        let Object::Ref(r) = obj else {
            return Font::load(doc, obj).map(Arc::new);
        };
        // The lock is held only to take the cell; the font is read outside it.
        let cell = self
            .table
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(r.num)
            .or_default()
            .clone();
        cell.get_or_init(|| {
            self.loads.fetch_add(1, Ordering::Relaxed);
            Font::load(doc, obj).map(Arc::new)
        })
        .clone()
    }

    /// How many fonts were read into the cache.
    pub fn loads(&self) -> usize {
        self.loads.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cache_tests;
