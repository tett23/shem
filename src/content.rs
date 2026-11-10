//! The operators of a page's content stream (docs/adr/0002).
//!
//! [`parse`] reads the operators that v0.1 interprets, checks their
//! operands, and checks where they may appear: the stack of graphics
//! states, text objects, path objects and marked content. Any other
//! operator is an error.

use crate::error::{Error, Result};
use crate::fixed::{Fixed, Matrix};
use crate::pdf::lexer::{Lexer, Token, hex_bytes, object_from};
use crate::pdf::{Dict, Document, Object};
use std::borrow::Cow;

/// An operator. The names of `cs`, `Tf` and `Do` are slices of the content
/// when they have no `#` escapes, so that reading them allocates nothing
/// (docs/adr/0013).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operator<'a> {
    Save,
    Restore,
    Concat(Matrix),
    LineWidth(Fixed),
    MoveTo(Fixed, Fixed),
    LineTo(Fixed, Fixed),
    CurveTo([Fixed; 6]),
    ClosePath,
    Rect([Fixed; 4]),
    Fill,
    Stroke,
    /// `J`: 0 for butt caps, 1 for round caps (docs/adr/0025), 2 for
    /// square caps (docs/adr/0027).
    LineCap(i64),
    /// `d`: the dash array and the phase, in user space (docs/adr/0025).
    Dash(std::sync::Arc<[Fixed]>, Fixed),
    /// `j`: 0 for miter joins, 1 for round joins, 2 for bevel joins
    /// (docs/adr/0027).
    LineJoin(i64),
    /// `M`: the miter limit, 1 or more (docs/adr/0027).
    MiterLimit(Fixed),
    /// `f*`, `B`, `B*` and `n` (docs/adr/0027).
    FillEvenOdd,
    FillStroke,
    FillStrokeEvenOdd,
    EndPath,
    /// `W` and `W*`: the path that the next painting operator ends
    /// becomes the clip (docs/adr/0027).
    Clip,
    ClipEvenOdd,
    FillCmyk([Fixed; 4]),
    StrokeCmyk([Fixed; 4]),
    FillSpace(Cow<'a, [u8]>),
    /// `CS` (docs/adr/0027).
    StrokeSpace(Cow<'a, [u8]>),
    /// `sc` and `SC`: the components of a color in the fill or the
    /// stroke color space (docs/adr/0027).
    FillComponents(Components),
    StrokeComponents(Components),
    FillColor(Fixed),
    BeginText,
    EndText,
    Font(Cow<'a, [u8]>, Fixed),
    HScale(Fixed),
    TextMatrix(Matrix),
    RenderMode(i64),
    ShowText(Text<'a>),
    Do(Cow<'a, [u8]>),
    BeginMarked,
    EndMarked,
}

/// The components of `sc` and `SC`: one to four numbers, held without an
/// allocation (docs/adr/0013).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Components {
    count: u8,
    values: [Fixed; 4],
}

impl Components {
    /// The components of one to four values.
    pub fn new(values: &[Fixed]) -> Option<Components> {
        if values.is_empty() || values.len() > 4 {
            return None;
        }
        let mut all = [Fixed::ZERO; 4];
        all[..values.len()].copy_from_slice(values);
        Some(Components {
            count: values.len() as u8,
            values: all,
        })
    }

    pub fn as_slice(&self) -> &[Fixed] {
        &self.values[..usize::from(self.count)]
    }
}

/// The string of `Tj`: a hexadecimal string as the slice of the content,
/// decoded as it is read, or the bytes of a literal string. Two texts are
/// equal, and print, as their bytes (docs/adr/0014).
#[derive(Clone)]
pub enum Text<'a> {
    Hex(&'a [u8]),
    Bytes(Vec<u8>),
}

impl Text<'_> {
    pub fn bytes(&self) -> impl Iterator<Item = u8> + Clone + '_ {
        let (hex, bytes) = match self {
            Text::Hex(raw) => (Some(hex_bytes(raw)), None),
            Text::Bytes(b) => (None, Some(b.iter().copied())),
        };
        hex.into_iter().flatten().chain(bytes.into_iter().flatten())
    }
}

impl PartialEq for Text<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes().eq(other.bytes())
    }
}

impl Eq for Text<'_> {}

impl std::fmt::Debug for Text<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.bytes()).finish()
    }
}

/// An operator and the offset of its keyword in the content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Op<'a> {
    pub offset: usize,
    pub operator: Operator<'a>,
}

/// An operand: a number, a name or a hexadecimal string as its token, or
/// another object. Numbers are kept as their tokens, so that the operands
/// of most operators are made and dropped without an [`Object`].
#[derive(Debug)]
enum Operand<'a> {
    Int(i64),
    Real(Fixed),
    Name(Cow<'a, [u8]>),
    Hex(&'a [u8]),
    Object(Object),
}

impl Operand<'_> {
    fn number(&self) -> Option<Fixed> {
        match self {
            Operand::Int(i) => Fixed::from_int(*i).ok(),
            Operand::Real(v) => Some(*v),
            Operand::Object(o) => o.as_number().and_then(|r| r.ok()),
            Operand::Name(_) | Operand::Hex(_) => None,
        }
    }
}

/// Where an operator is allowed (ISO 32000-1, Annex L, Figure 9).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// The page level only: q Q cm Do BT, and the start of a path.
    Page,
    /// The page level and text objects: the graphics and text state, colors, marked content.
    Anywhere,
    /// Text objects only: Tm Tj ET.
    Text,
    /// The construction of a path: l c h, which need a current point.
    Path,
    /// The painting of a path: f S.
    Paint,
}

fn numbers<const N: usize>(operands: &[Operand]) -> Option<[Fixed; N]> {
    if operands.len() != N {
        return None;
    }
    let values: [Option<Fixed>; N] = std::array::from_fn(|i| operands[i].number());
    values
        .iter()
        .all(Option::is_some)
        .then(|| values.map(Option::unwrap))
}

/// The operator of keyword `k` with its operands, which it may take, or
/// `None` if the operands are wrong. An unknown keyword is `Err`.
fn operator<'a>(
    k: &[u8],
    operands: &mut [Operand<'a>],
) -> std::result::Result<Option<(Operator<'a>, Place)>, ()> {
    use Operator::*;
    let empty = operands.is_empty();
    let none = |op: Operator<'a>, place| Ok(empty.then_some((op, place)));
    let name = |o: &mut Operand<'a>| match o {
        Operand::Name(n) => Some(std::mem::take(n)),
        Operand::Int(_) | Operand::Real(_) | Operand::Object(_) | Operand::Hex(_) => None,
    };
    Ok(match k {
        b"q" => return none(Save, Place::Page),
        b"Q" => return none(Restore, Place::Page),
        b"cm" => numbers::<6>(operands).map(|v| (Concat(Matrix::new(v)), Place::Page)),
        b"w" => numbers::<1>(operands)
            .filter(|[v]| *v >= Fixed::ZERO)
            .map(|[v]| (LineWidth(v), Place::Anywhere)),
        b"m" => numbers::<2>(operands).map(|[x, y]| (MoveTo(x, y), Place::Page)),
        b"l" => numbers::<2>(operands).map(|[x, y]| (LineTo(x, y), Place::Path)),
        b"c" => numbers::<6>(operands).map(|v| (CurveTo(v), Place::Path)),
        b"h" => return none(ClosePath, Place::Path),
        b"re" => numbers::<4>(operands).map(|v| (Rect(v), Place::Page)),
        b"f" => return none(Fill, Place::Paint),
        b"S" => return none(Stroke, Place::Paint),
        b"f*" => return none(FillEvenOdd, Place::Paint),
        b"B" => return none(FillStroke, Place::Paint),
        b"B*" => return none(FillStrokeEvenOdd, Place::Paint),
        b"n" => return none(EndPath, Place::Paint),
        b"W" => return none(Clip, Place::Path),
        b"W*" => return none(ClipEvenOdd, Place::Path),
        b"J" => match operands {
            [Operand::Int(cap)] => Some((LineCap(*cap), Place::Anywhere)),
            _ => None,
        },
        b"j" => match operands {
            [Operand::Int(join)] => Some((LineJoin(*join), Place::Anywhere)),
            _ => None,
        },
        b"M" => numbers::<1>(operands).map(|[v]| (MiterLimit(v), Place::Anywhere)),
        b"sc" => components(operands).map(|c| (FillComponents(c), Place::Anywhere)),
        b"SC" => components(operands).map(|c| (StrokeComponents(c), Place::Anywhere)),
        b"CS" => match operands {
            [n] => name(n).map(|n| (StrokeSpace(n), Place::Anywhere)),
            _ => None,
        },
        b"d" => match operands {
            [Operand::Object(Object::Array(array)), phase] => dash_array(array)
                .zip(phase.number())
                .and_then(|(array, phase)| {
                    (phase >= Fixed::ZERO).then_some((Dash(array, phase), Place::Anywhere))
                }),
            _ => None,
        },
        b"k" => numbers::<4>(operands).map(|v| (FillCmyk(v), Place::Anywhere)),
        b"K" => numbers::<4>(operands).map(|v| (StrokeCmyk(v), Place::Anywhere)),
        b"cs" => match operands {
            [n] => name(n).map(|n| (FillSpace(n), Place::Anywhere)),
            _ => None,
        },
        b"scn" => numbers::<1>(operands).map(|[v]| (FillColor(v), Place::Anywhere)),
        b"BT" => return none(BeginText, Place::Page),
        b"ET" => return none(EndText, Place::Text),
        b"Tf" => match operands {
            [n, size] => size
                .number()
                .and_then(|s| name(n).map(|n| (Font(n, s), Place::Anywhere))),
            _ => None,
        },
        b"Tz" => numbers::<1>(operands).map(|[v]| (HScale(v), Place::Anywhere)),
        b"Tm" => numbers::<6>(operands).map(|v| (TextMatrix(Matrix::new(v)), Place::Text)),
        b"Tr" => match operands {
            [Operand::Int(mode)] => Some((RenderMode(*mode), Place::Anywhere)),
            _ => None,
        },
        b"Tj" => match operands {
            [Operand::Hex(raw)] => Some((ShowText(Text::Hex(raw)), Place::Text)),
            [Operand::Object(Object::String(s))] => {
                Some((ShowText(Text::Bytes(std::mem::take(s))), Place::Text))
            }
            _ => None,
        },
        b"Do" => match operands {
            [n] => name(n).map(|n| (Do(n), Place::Page)),
            _ => None,
        },
        b"BDC" => match operands {
            [
                Operand::Name(_),
                Operand::Name(_) | Operand::Object(Object::Dict(_)),
            ] => Some((BeginMarked, Place::Anywhere)),
            _ => None,
        },
        b"EMC" => return none(EndMarked, Place::Anywhere),
        _ => return Err(()),
    })
}

/// The components of `sc` and `SC`: one to four numbers (docs/adr/0027).
fn components(operands: &[Operand]) -> Option<Components> {
    let mut values = [Fixed::ZERO; 4];
    if operands.is_empty() || operands.len() > values.len() {
        return None;
    }
    operands
        .iter()
        .zip(values.iter_mut())
        .try_for_each(|(o, v)| o.number().map(|n| *v = n))?;
    Components::new(&values[..operands.len()])
}

/// The dash array of `d`: numbers, none negative (docs/adr/0025).
fn dash_array(array: &[Object]) -> Option<std::sync::Arc<[Fixed]>> {
    array
        .iter()
        .map(|o| o.as_number()?.ok().filter(|v| *v >= Fixed::ZERO))
        .collect()
}

fn keyword_name(k: &[u8]) -> String {
    String::from_utf8_lossy(k).into_owned()
}

/// Where the interpreter is: the checks of Annex L of ISO 32000-1.
#[derive(Default)]
struct State {
    saved: usize,
    marked: usize,
    in_text: bool,
    /// A path is being built, with a current point.
    in_path: bool,
    /// The keyword of a `W` or `W*` that the next operator must end the
    /// path of (docs/adr/0027).
    clipping: Option<&'static str>,
}

/// Whether an operator ends a path: `f`, `f*`, `S`, `B`, `B*` or `n`.
fn ends_path(op: &Operator) -> bool {
    matches!(
        op,
        Operator::Fill
            | Operator::FillEvenOdd
            | Operator::Stroke
            | Operator::FillStroke
            | Operator::FillStrokeEvenOdd
            | Operator::EndPath
    )
}

impl State {
    /// Checks an operator; the name of its keyword `k` is made only for an error.
    fn check(&mut self, k: &[u8], op: &Operator, place: Place) -> std::result::Result<(), String> {
        let k = || keyword_name(k);
        if let Some(clip) = self.clipping {
            // W and W* go right before the operator that ends the path.
            if !ends_path(op) {
                return Err(format!("{} after {clip}", k()));
            }
            self.clipping = None;
        }
        if self.in_path
            && !matches!(
                op,
                Operator::MoveTo(..)
                    | Operator::LineTo(..)
                    | Operator::CurveTo(_)
                    | Operator::ClosePath
                    | Operator::Rect(_)
                    | Operator::Clip
                    | Operator::ClipEvenOdd
            )
            && !ends_path(op)
        {
            return Err(format!("{} inside a path", k()));
        }
        match place {
            Place::Page if self.in_text => return Err(format!("{} inside a text object", k())),
            Place::Text if !self.in_text => return Err(format!("{} outside a text object", k())),
            Place::Path if !self.in_path => return Err(format!("{} without a current point", k())),
            Place::Paint if self.in_text => return Err(format!("{} inside a text object", k())),
            _ => {}
        }
        match op {
            Operator::Save => self.saved += 1,
            Operator::Restore => {
                self.saved = self.saved.checked_sub(1).ok_or("Q without q")?;
            }
            Operator::BeginText => self.in_text = true,
            Operator::EndText => self.in_text = false,
            Operator::MoveTo(..) | Operator::Rect(_) => self.in_path = true,
            Operator::Clip => self.clipping = Some("W"),
            Operator::ClipEvenOdd => self.clipping = Some("W*"),
            op if ends_path(op) => self.in_path = false,
            Operator::BeginMarked => self.marked += 1,
            Operator::EndMarked => {
                self.marked = self.marked.checked_sub(1).ok_or("EMC without BDC")?;
            }
            Operator::RenderMode(m) if *m != 0 && *m != 2 => {
                return Err(format!("unsupported Tr {m}"));
            }
            Operator::LineCap(c) if !(0..=2).contains(c) => {
                return Err(format!("malformed J {c}"));
            }
            Operator::LineJoin(j) if !(0..=2).contains(j) => {
                return Err(format!("malformed j {j}"));
            }
            // The miter limit is 1 or more (ISO 32000-1 8.4.3.5).
            Operator::MiterLimit(m) if *m < Fixed::ONE => return Err("malformed M".into()),
            // A dash array of zeros only is malformed (ISO 32000-1 8.4.3.6).
            Operator::Dash(array, _)
                if !array.is_empty() && array.iter().all(|v| *v == Fixed::ZERO) =>
            {
                return Err("malformed d".into());
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&self) -> std::result::Result<(), &'static str> {
        if self.in_path {
            Err("path not painted at the end of the content")
        } else if self.in_text {
            Err("BT not closed at the end of the content")
        } else if self.saved > 0 {
            Err("q not closed at the end of the content")
        } else if self.marked > 0 {
            Err("BDC not closed at the end of the content")
        } else {
            Ok(())
        }
    }
}

/// Reads and checks the operators of a content stream.
pub fn parse(content: &[u8]) -> Result<Vec<Op<'_>>> {
    let mut ops = Vec::new();
    parse_into(content, &mut ops)?;
    Ok(ops)
}

/// [`parse`] into `ops`, which is cleared first.
pub fn parse_into<'a>(content: &'a [u8], ops: &mut Vec<Op<'a>>) -> Result<()> {
    ops.clear();
    let mut lx = Lexer::new(content, 0);
    let mut operands: Vec<Operand> = Vec::new();
    let mut state = State::default();
    while let Some((at, token)) = lx.next_token()? {
        match token {
            Token::Keyword(k) if !matches!(k, b"true" | b"false" | b"null") => {
                let (op, place) = match operator(k, &mut operands) {
                    Err(()) => {
                        return Err(Error::new(format!(
                            "unsupported operator {}",
                            keyword_name(k)
                        ))
                        .at_offset(at));
                    }
                    Ok(None) => {
                        return Err(Error::new(format!("wrong operands of {}", keyword_name(k)))
                            .at_offset(at));
                    }
                    Ok(Some(p)) => p,
                };
                state
                    .check(k, &op, place)
                    .map_err(|m| Error::new(m).at_offset(at))?;
                ops.push(Op {
                    offset: at,
                    operator: op,
                });
                operands.clear();
            }
            Token::Int(i) => operands.push(Operand::Int(i)),
            Token::Real(v) => operands.push(Operand::Real(v)),
            Token::Name(n) => operands.push(Operand::Name(n)),
            Token::HexString(raw) => operands.push(Operand::Hex(raw)),
            token => operands.push(Operand::Object(object_from(&mut lx, at, token, false)?)),
        }
    }
    if !operands.is_empty() {
        return Err(
            Error::new("operands without an operator at the end of the content")
                .at_offset(content.len()),
        );
    }
    state
        .finish()
        .map_err(|m| Error::new(m).at_offset(content.len()))
}

/// Checks that the names of `Tf`, `cs`, `CS` and `Do` are in the page's
/// resources.
pub fn check_resources(doc: &Document, resources: &Dict, ops: &[Op]) -> Result<()> {
    let category = |key: &str| doc.get_dict(resources, key).map(Option::unwrap_or_default);
    let fonts = category("Font")?;
    let spaces = category("ColorSpace")?;
    let xobjects = category("XObject")?;
    ops.iter().try_for_each(|op| {
        let (kind, dict, name) = match &op.operator {
            Operator::Font(n, _) => ("font", &fonts, n),
            Operator::FillSpace(n) | Operator::StrokeSpace(n) => ("color space", &spaces, n),
            Operator::Do(n) => ("XObject", &xobjects, n),
            _ => return Ok(()),
        };
        if dict.contains_key(name.as_ref()) {
            Ok(())
        } else {
            Err(
                Error::new(format!("no {kind} {} in the resources", keyword_name(name)))
                    .at_offset(op.offset),
            )
        }
    })
}

#[cfg(test)]
mod tests;
