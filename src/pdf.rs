//! The file structure and the objects of PDF (docs/adr/0002).
//!
//! v0.1 reads a classic xref table, objects of generation 0, streams with
//! no filter or `FlateDecode`, and the page tree. Anything else stops with
//! an error.

pub mod lexer;

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{Error, Result};
use crate::fixed::Fixed;
use crate::flate;
use lexer::{Lexer, Token, parse_object};

pub type Dict = BTreeMap<Vec<u8>, Object>;

/// The xref table: the offset and the generation of each object in use.
type Xref = BTreeMap<u32, (usize, u16)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ref {
    pub num: u32,
    pub generation: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stream {
    pub dict: Dict,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Object {
    Null,
    Bool(bool),
    Int(i64),
    Real(Fixed),
    String(Vec<u8>),
    Name(Vec<u8>),
    Array(Vec<Object>),
    Dict(Dict),
    Stream(Stream),
    Ref(Ref),
}

impl Object {
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Object::Dict(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    pub fn as_name(&self) -> Option<&[u8]> {
        match self {
            Object::Name(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Object::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Object]> {
        match self {
            Object::Array(a) => Some(a),
            _ => None,
        }
    }

    /// A number, integer or real, as Q32.32.
    pub fn as_number(&self) -> Option<Result<Fixed>> {
        match self {
            Object::Int(i) => Some(Fixed::from_int(*i)),
            Object::Real(v) => Some(Ok(*v)),
            _ => None,
        }
    }

    /// A short description for error messages, such as `90` or `LZWDecode`.
    pub fn describe(&self) -> String {
        match self {
            Object::Null => "null".into(),
            Object::Bool(b) => b.to_string(),
            Object::Int(i) => i.to_string(),
            Object::Real(v) => v.to_string(),
            Object::String(_) => "(string)".into(),
            Object::Name(n) => String::from_utf8_lossy(n).into_owned(),
            Object::Array(a) => format!(
                "[{}]",
                a.iter().map(Object::describe).collect::<Vec<_>>().join(" ")
            ),
            Object::Dict(_) => "<<...>>".into(),
            Object::Stream(_) => "stream".into(),
            Object::Ref(r) => format!("{} {} R", r.num, r.generation),
        }
    }
}

/// Parses one object from bytes (for tests and small inputs).
pub fn parse_object_bytes(s: &[u8]) -> Result<Object> {
    parse_object(&mut Lexer::new(s, 0), true)
}

/// A page, with the inherited attributes applied.
#[derive(Clone, Debug)]
pub struct Page {
    /// The index from 0.
    pub index: usize,
    pub object: u32,
    /// `[llx lly urx ury]`, normalized so that `llx <= urx` and `lly <= ury`.
    pub media_box: [Fixed; 4],
    pub resources: Dict,
    /// The decoded content, the streams of an array joined with a space.
    pub contents: Vec<u8>,
}

pub struct Document {
    data: Vec<u8>,
    xref: Xref,
    trailer: Dict,
}

fn unsupported(key: &str, value: &Object) -> Error {
    Error::new(format!("unsupported {key} {}", value.describe()))
}

impl Document {
    pub fn parse(data: &[u8]) -> Result<Document> {
        if !data.starts_with(b"%PDF-") {
            return Err(Error::new("not a PDF: no %PDF- header"));
        }
        let tail_start = data.len().saturating_sub(1024);
        let at = data[tail_start..]
            .windows(9)
            .rposition(|w| w == b"startxref")
            .map(|p| tail_start + p)
            .ok_or_else(|| Error::new("no startxref"))?;
        let mut lx = Lexer::new(data, at + 9);
        let xref_at = match lx.next_token()? {
            Some((_, Token::Int(i))) => {
                usize::try_from(i).map_err(|_| Error::new("malformed startxref"))?
            }
            _ => return Err(Error::new("malformed startxref").at_offset(at)),
        };
        let (xref, trailer) = read_xref(data, xref_at)?;
        ["Prev", "Encrypt", "XRefStm"].iter().try_for_each(|key| {
            match trailer.get(key.as_bytes()) {
                Some(v) => Err(unsupported(key, v)),
                None => Ok(()),
            }
        })?;
        if !trailer.contains_key(b"Root".as_slice()) {
            return Err(Error::new("trailer has no Root"));
        }
        Ok(Document {
            data: data.to_vec(),
            xref,
            trailer,
        })
    }

    /// The object `num`, read from the offset of the xref table.
    pub fn load(&self, num: u32) -> Result<Object> {
        self.load_object(num).map_err(|e| e.at_object(num))
    }

    fn load_object(&self, num: u32) -> Result<Object> {
        let &(offset, _) = self
            .xref
            .get(&num)
            .ok_or_else(|| Error::new(format!("object {num} not in the xref table")))?;
        let mut lx = Lexer::new(&self.data, offset);
        let header = (lx.next_token()?, lx.next_token()?, lx.next_token()?);
        match header {
            (Some((_, Token::Int(n))), Some((_, Token::Int(0))), Some((_, Token::Keyword(k))))
                if n == i64::from(num) && k == b"obj" => {}
            _ => {
                return Err(
                    Error::new("the xref table does not point at the object").at_offset(offset)
                );
            }
        }
        let obj = parse_object(&mut lx, true)?;
        let obj = match lx.peek_token()? {
            Some((_, Token::Keyword(k))) if k == b"stream" => {
                lx.next_token()?;
                let dict = match obj {
                    Object::Dict(d) => d,
                    _ => return Err(Error::new("stream without a dictionary")),
                };
                Object::Stream(self.read_stream(&mut lx, dict)?)
            }
            _ => obj,
        };
        if let Some(t) = obj.as_dict().and_then(|d| d.get(b"Type".as_slice())) {
            match t.as_name() {
                Some(b"ObjStm") => return Err(unsupported("Type", t)),
                Some(b"XRef") => return Err(Error::new("unsupported xref stream")),
                _ => {}
            }
        }
        Ok(obj)
    }

    fn read_stream(&self, lx: &mut Lexer, dict: Dict) -> Result<Stream> {
        let length = match dict.get(b"Length".as_slice()) {
            Some(Object::Int(n)) => *n,
            Some(Object::Ref(r)) => self
                .resolve(&Object::Ref(*r))?
                .as_int()
                .ok_or_else(|| Error::new("Length is not an integer"))?,
            _ => return Err(Error::new("stream without an integer Length")),
        };
        let length = usize::try_from(length).map_err(|_| Error::new("negative Length"))?;
        // The keyword stream is followed by CRLF or LF (7.3.8.1).
        let pos = lx.position();
        let start = match (self.data.get(pos), self.data.get(pos + 1)) {
            (Some(b'\r'), Some(b'\n')) => pos + 2,
            (Some(b'\n'), _) => pos + 1,
            _ => {
                return Err(
                    Error::new("stream keyword not followed by an end of line").at_offset(pos)
                );
            }
        };
        let data = self
            .data
            .get(start..start + length)
            .ok_or_else(|| Error::new("stream runs past the end of the file").at_offset(start))?
            .to_vec();
        lx.set_position(start + length);
        match lx.next_token()? {
            Some((_, Token::Keyword(k))) if k == b"endstream" => Ok(Stream { dict, data }),
            _ => Err(Error::new("no endstream after Length bytes").at_offset(start + length)),
        }
    }

    /// The object, with indirect references followed.
    pub fn resolve(&self, obj: &Object) -> Result<Object> {
        (0..32)
            .try_fold(obj.clone(), |o, _| match o {
                Object::Ref(r) if r.generation != 0 => Err(Error::new(format!(
                    "unsupported generation {} of object {}",
                    r.generation, r.num
                ))),
                Object::Ref(r) => self.load(r.num),
                other => Ok(other),
            })
            .and_then(|o| match o {
                Object::Ref(_) => Err(Error::new("too many indirect references")),
                other => Ok(other),
            })
    }

    /// The entry `key` of a dictionary, resolved; `None` if it is absent or null.
    pub fn get(&self, dict: &Dict, key: &str) -> Result<Option<Object>> {
        match dict.get(key.as_bytes()) {
            None => Ok(None),
            Some(o) => self
                .resolve(o)
                .map(|o| if o == Object::Null { None } else { Some(o) }),
        }
    }

    /// The entry `key` of a dictionary as a dictionary, resolved.
    pub fn get_dict(&self, dict: &Dict, key: &str) -> Result<Option<Dict>> {
        match self.get(dict, key)? {
            None => Ok(None),
            Some(Object::Dict(d)) => Ok(Some(d)),
            Some(other) => Err(Error::new(format!(
                "{key} is not a dictionary: {}",
                other.describe()
            ))),
        }
    }

    /// The numbers of the objects in use, in order.
    pub fn object_numbers(&self) -> impl Iterator<Item = u32> + '_ {
        self.xref.keys().copied()
    }

    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    /// The decoded data of a stream: no filter or `FlateDecode` only.
    pub fn stream_data(&self, stream: &Stream) -> Result<Vec<u8>> {
        if let Some(parms) = self.get(&stream.dict, "DecodeParms")? {
            return Err(unsupported("DecodeParms", &parms));
        }
        let filters = match self.get(&stream.dict, "Filter")? {
            None => vec![],
            Some(Object::Array(a)) => a
                .iter()
                .map(|f| self.resolve(f))
                .collect::<Result<Vec<_>>>()?,
            Some(f) => vec![f],
        };
        match filters.as_slice() {
            [] => Ok(stream.data.clone()),
            [f] if f.as_name() == Some(b"FlateDecode") => flate::inflate_zlib(&stream.data),
            [f] => Err(unsupported("Filter", f)),
            many => Err(unsupported("Filter", &Object::Array(many.to_vec()))),
        }
    }

    /// The pages in order (ISO 32000-1 7.7.3).
    pub fn pages(&self) -> Result<Vec<Page>> {
        let catalog = self
            .get(&self.trailer, "Root")?
            .and_then(|o| o.as_dict().cloned())
            .ok_or_else(|| Error::new("Root is not a dictionary"))?;
        let root = catalog
            .get(b"Pages".as_slice())
            .ok_or_else(|| Error::new("the catalog has no Pages"))?;
        let mut leaves = Vec::new();
        self.walk(
            root,
            Inherited::default(),
            &mut BTreeSet::new(),
            &mut leaves,
        )?;
        leaves
            .into_iter()
            .enumerate()
            .map(|(index, (object, dict, inherited))| {
                self.page(index, object, &dict, inherited)
                    .map_err(|e| e.at_page(index + 1))
            })
            .collect()
    }

    fn walk(
        &self,
        node: &Object,
        inherited: Inherited,
        visited: &mut BTreeSet<u32>,
        leaves: &mut Vec<(u32, Dict, Inherited)>,
    ) -> Result<()> {
        let Object::Ref(r) = node else {
            return Err(Error::new("a page tree node is not an indirect reference"));
        };
        if !visited.insert(r.num) {
            return Err(Error::new("cycle in the page tree").at_object(r.num));
        }
        let dict =
            self.resolve(node)?.as_dict().cloned().ok_or_else(|| {
                Error::new("a page tree node is not a dictionary").at_object(r.num)
            })?;
        let inherited = inherited.update(&dict);
        match dict.get(b"Type".as_slice()).and_then(Object::as_name) {
            Some(b"Pages") => {
                let kids = self
                    .get(&dict, "Kids")?
                    .and_then(|k| k.as_array().map(<[Object]>::to_vec))
                    .ok_or_else(|| Error::new("Pages without Kids").at_object(r.num))?;
                kids.iter()
                    .try_for_each(|kid| self.walk(kid, inherited.clone(), visited, leaves))
            }
            Some(b"Page") => {
                leaves.push((r.num, dict, inherited));
                Ok(())
            }
            _ => Err(Error::new("a page tree node is neither Pages nor Page").at_object(r.num)),
        }
    }

    fn page(&self, index: usize, object: u32, dict: &Dict, inherited: Inherited) -> Result<Page> {
        let rotate = inherited
            .rotate
            .as_ref()
            .map(|o| self.resolve(o))
            .transpose()?;
        if let Some(rotate) = rotate.filter(|r| *r != Object::Int(0)) {
            return Err(unsupported("Rotate", &rotate));
        }
        if let Some(unit) = self.get(dict, "UserUnit")? {
            return Err(unsupported("UserUnit", &unit));
        }
        let media_box = inherited
            .media_box
            .as_ref()
            .ok_or_else(|| Error::new("page without a MediaBox"))
            .and_then(|o| self.rectangle(o))?;
        let resources = match inherited
            .resources
            .as_ref()
            .map(|o| self.resolve(o))
            .transpose()?
        {
            None => Dict::new(),
            Some(Object::Dict(d)) => d,
            Some(other) => {
                return Err(Error::new(format!(
                    "Resources is not a dictionary: {}",
                    other.describe()
                )));
            }
        };
        let contents = self.contents(dict)?;
        Ok(Page {
            index,
            object,
            media_box,
            resources,
            contents,
        })
    }

    /// A rectangle `[llx lly urx ury]`, normalized (7.9.5).
    pub fn rectangle(&self, obj: &Object) -> Result<[Fixed; 4]> {
        let values = self
            .resolve(obj)?
            .as_array()
            .map(|a| a.iter().map(|v| self.number(v)).collect::<Result<Vec<_>>>())
            .transpose()?
            .filter(|v| v.len() == 4)
            .ok_or_else(|| Error::new("a rectangle is not an array of four numbers"))?;
        Ok([
            values[0].min(values[2]),
            values[1].min(values[3]),
            values[0].max(values[2]),
            values[1].max(values[3]),
        ])
    }

    /// A number, resolved.
    pub fn number(&self, obj: &Object) -> Result<Fixed> {
        self.resolve(obj)?
            .as_number()
            .unwrap_or_else(|| Err(Error::new(format!("not a number: {}", obj.describe()))))
    }

    fn contents(&self, dict: &Dict) -> Result<Vec<u8>> {
        let streams = match dict.get(b"Contents".as_slice()) {
            None => vec![],
            Some(Object::Array(a)) => a.clone(),
            Some(other) => vec![other.clone()],
        };
        let decoded = streams
            .iter()
            .map(|s| match self.resolve(s)? {
                Object::Stream(st) => self.stream_data(&st),
                other => Err(Error::new(format!(
                    "Contents is not a stream: {}",
                    other.describe()
                ))),
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(decoded.join(&b' '))
    }
}

/// The inheritable attributes of the page tree (7.7.3.4).
#[derive(Clone, Default)]
struct Inherited {
    resources: Option<Object>,
    media_box: Option<Object>,
    rotate: Option<Object>,
}

impl Inherited {
    fn update(self, dict: &Dict) -> Inherited {
        let pick = |key: &str, old: Option<Object>| dict.get(key.as_bytes()).cloned().or(old);
        Inherited {
            resources: pick("Resources", self.resources),
            media_box: pick("MediaBox", self.media_box),
            rotate: pick("Rotate", self.rotate),
        }
    }
}

/// Reads the classic xref table at `at` and the trailer after it.
fn read_xref(data: &[u8], at: usize) -> Result<(Xref, Dict)> {
    let mut lx = Lexer::new(data, at);
    match lx.next_token()? {
        Some((_, Token::Keyword(k))) if k == b"xref" => {}
        Some((_, Token::Int(_))) => return Err(Error::new("unsupported xref stream").at_offset(at)),
        _ => return Err(Error::new("no xref table at startxref").at_offset(at)),
    }
    let mut xref = BTreeMap::new();
    loop {
        match lx.next_token()? {
            Some((_, Token::Keyword(k))) if k == b"trailer" => break,
            Some((sat, Token::Int(start))) => {
                let count = match lx.next_token()? {
                    Some((_, Token::Int(c))) if c >= 0 => c,
                    _ => return Err(Error::new("malformed xref subsection").at_offset(sat)),
                };
                (0..count).try_for_each(|i| {
                    let entry = (lx.next_token()?, lx.next_token()?, lx.next_token()?);
                    match entry {
                        (
                            Some((_, Token::Int(offset))),
                            Some((_, Token::Int(generation))),
                            Some((_, Token::Keyword(kind))),
                        ) => {
                            let num = u32::try_from(start + i)
                                .map_err(|_| Error::new("object number out of range"))?;
                            match kind.as_slice() {
                                b"n" => {
                                    let offset = usize::try_from(offset)
                                        .map_err(|_| Error::new("negative offset"))?;
                                    let generation = u16::try_from(generation)
                                        .map_err(|_| Error::new("generation out of range"))?;
                                    xref.insert(num, (offset, generation));
                                    Ok(())
                                }
                                b"f" => Ok(()),
                                _ => Err(Error::new("malformed xref entry").at_offset(sat)),
                            }
                        }
                        _ => Err(Error::new("malformed xref entry").at_offset(sat)),
                    }
                })?;
            }
            _ => return Err(Error::new("malformed xref table").at_offset(at)),
        }
    }
    match parse_object(&mut lx, true)? {
        Object::Dict(d) => Ok((xref, d)),
        _ => Err(Error::new("trailer is not a dictionary")),
    }
}

#[cfg(test)]
pub(crate) mod tests;
