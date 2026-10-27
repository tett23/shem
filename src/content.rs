//! The operators of a page's content stream (docs/adr/0002).
//!
//! [`parse`] reads the operators that v0.1 interprets, checks their
//! operands, and checks where they may appear: the stack of graphics
//! states, text objects, path objects and marked content. Any other
//! operator is an error.

use crate::error::{Error, Result};
use crate::fixed::{Fixed, Matrix};
use crate::pdf::lexer::{Lexer, Token, object_from};
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
    CurveTo([Fixed; 6]),
    ClosePath,
    Rect([Fixed; 4]),
    Fill,
    FillCmyk([Fixed; 4]),
    StrokeCmyk([Fixed; 4]),
    FillSpace(Cow<'a, [u8]>),
    FillColor(Fixed),
    BeginText,
    EndText,
    Font(Cow<'a, [u8]>, Fixed),
    HScale(Fixed),
    TextMatrix(Matrix),
    RenderMode(i64),
    ShowText(Vec<u8>),
    Do(Cow<'a, [u8]>),
    BeginMarked,
    EndMarked,
}

/// An operator and the offset of its keyword in the content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Op<'a> {
    pub offset: usize,
    pub operator: Operator<'a>,
}

/// An operand: a name as its token, or another object.
#[derive(Debug)]
enum Operand<'a> {
    Name(Cow<'a, [u8]>),
    Object(Object),
}

impl Operand<'_> {
    fn number(&self) -> Option<Fixed> {
        match self {
            Operand::Object(o) => o.as_number().and_then(|r| r.ok()),
            Operand::Name(_) => None,
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
    /// The construction of a path: c h, which need a current point.
    Path,
    /// The painting of a path: f.
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
        Operand::Object(_) => None,
    };
    Ok(match k {
        b"q" => return none(Save, Place::Page),
        b"Q" => return none(Restore, Place::Page),
        b"cm" => numbers::<6>(operands).map(|v| (Concat(Matrix::new(v)), Place::Page)),
        b"w" => numbers::<1>(operands)
            .filter(|[v]| *v >= Fixed::ZERO)
            .map(|[v]| (LineWidth(v), Place::Anywhere)),
        b"m" => numbers::<2>(operands).map(|[x, y]| (MoveTo(x, y), Place::Page)),
        b"c" => numbers::<6>(operands).map(|v| (CurveTo(v), Place::Path)),
        b"h" => return none(ClosePath, Place::Path),
        b"re" => numbers::<4>(operands).map(|v| (Rect(v), Place::Page)),
        b"f" => return none(Fill, Place::Paint),
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
            [Operand::Object(Object::Int(mode))] => Some((RenderMode(*mode), Place::Anywhere)),
            _ => None,
        },
        b"Tj" => match operands {
            [Operand::Object(Object::String(s))] => {
                Some((ShowText(std::mem::take(s)), Place::Text))
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
}

impl State {
    /// Checks an operator; the name of its keyword `k` is made only for an error.
    fn check(&mut self, k: &[u8], op: &Operator, place: Place) -> std::result::Result<(), String> {
        let k = || keyword_name(k);
        if self.in_path
            && !matches!(
                op,
                Operator::MoveTo(..)
                    | Operator::CurveTo(_)
                    | Operator::ClosePath
                    | Operator::Rect(_)
                    | Operator::Fill
            )
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
            Operator::Fill => self.in_path = false,
            Operator::BeginMarked => self.marked += 1,
            Operator::EndMarked => {
                self.marked = self.marked.checked_sub(1).ok_or("EMC without BDC")?;
            }
            Operator::RenderMode(m) if *m != 0 && *m != 2 => {
                return Err(format!("unsupported Tr {m}"));
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
    let mut lx = Lexer::new(content, 0);
    let mut operands: Vec<Operand> = Vec::new();
    let mut state = State::default();
    let mut ops = Vec::new();
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
            Token::Name(n) => operands.push(Operand::Name(n)),
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
        .map_err(|m| Error::new(m).at_offset(content.len()))?;
    Ok(ops)
}

/// Checks that the names of `Tf`, `cs` and `Do` are in the page's resources.
pub fn check_resources(doc: &Document, resources: &Dict, ops: &[Op]) -> Result<()> {
    let category = |key: &str| doc.get_dict(resources, key).map(Option::unwrap_or_default);
    let fonts = category("Font")?;
    let spaces = category("ColorSpace")?;
    let xobjects = category("XObject")?;
    ops.iter().try_for_each(|op| {
        let (kind, dict, name) = match &op.operator {
            Operator::Font(n, _) => ("font", &fonts, n),
            Operator::FillSpace(n) => ("color space", &spaces, n),
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
