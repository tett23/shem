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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operator {
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
    FillSpace(Vec<u8>),
    FillColor(Fixed),
    BeginText,
    EndText,
    Font(Vec<u8>, Fixed),
    HScale(Fixed),
    TextMatrix(Matrix),
    RenderMode(i64),
    ShowText(Vec<u8>),
    Do(Vec<u8>),
    BeginMarked,
    EndMarked,
}

/// An operator and the offset of its keyword in the content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Op {
    pub offset: usize,
    pub operator: Operator,
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

fn numbers<const N: usize>(operands: &[Object]) -> Option<[Fixed; N]> {
    if operands.len() != N {
        return None;
    }
    let values: Vec<Fixed> = operands
        .iter()
        .map(|o| o.as_number().and_then(|r| r.ok()))
        .collect::<Option<Vec<_>>>()?;
    values.try_into().ok()
}

/// The operator of keyword `k` with its operands, or `None` if the operands
/// are wrong. An unknown keyword is `Err`.
fn operator(k: &[u8], operands: &[Object]) -> std::result::Result<Option<(Operator, Place)>, ()> {
    use Operator::*;
    let none = |op: Operator, place| Ok(operands.is_empty().then_some((op, place)));
    let name = |o: &Object| o.as_name().map(<[u8]>::to_vec);
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
            [n, size] => name(n)
                .zip(size.as_number().and_then(|r| r.ok()))
                .map(|(n, s)| (Font(n, s), Place::Anywhere)),
            _ => None,
        },
        b"Tz" => numbers::<1>(operands).map(|[v]| (HScale(v), Place::Anywhere)),
        b"Tm" => numbers::<6>(operands).map(|v| (TextMatrix(Matrix::new(v)), Place::Text)),
        b"Tr" => match operands {
            [Object::Int(mode)] => Some((RenderMode(*mode), Place::Anywhere)),
            _ => None,
        },
        b"Tj" => match operands {
            [Object::String(s)] => Some((ShowText(s.clone()), Place::Text)),
            _ => None,
        },
        b"Do" => match operands {
            [n] => name(n).map(|n| (Do(n), Place::Page)),
            _ => None,
        },
        b"BDC" => match operands {
            [Object::Name(_), Object::Dict(_) | Object::Name(_)] => {
                Some((BeginMarked, Place::Anywhere))
            }
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
    fn check(&mut self, k: &str, op: &Operator, place: Place) -> std::result::Result<(), String> {
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
            return Err(format!("{k} inside a path"));
        }
        match place {
            Place::Page if self.in_text => return Err(format!("{k} inside a text object")),
            Place::Text if !self.in_text => return Err(format!("{k} outside a text object")),
            Place::Path if !self.in_path => return Err(format!("{k} without a current point")),
            Place::Paint if self.in_text => return Err(format!("{k} inside a text object")),
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
pub fn parse(content: &[u8]) -> Result<Vec<Op>> {
    let mut lx = Lexer::new(content, 0);
    let mut operands: Vec<Object> = Vec::new();
    let mut state = State::default();
    let mut ops = Vec::new();
    while let Some((at, token)) = lx.next_token()? {
        match token {
            Token::Keyword(k) if !matches!(k.as_slice(), b"true" | b"false" | b"null") => {
                let name = keyword_name(&k);
                let (op, place) = match operator(&k, &operands) {
                    Err(()) => {
                        return Err(
                            Error::new(format!("unsupported operator {name}")).at_offset(at)
                        );
                    }
                    Ok(None) => {
                        return Err(Error::new(format!("wrong operands of {name}")).at_offset(at));
                    }
                    Ok(Some(p)) => p,
                };
                state
                    .check(&name, &op, place)
                    .map_err(|m| Error::new(m).at_offset(at))?;
                ops.push(Op {
                    offset: at,
                    operator: op,
                });
                operands.clear();
            }
            token => operands.push(object_from(&mut lx, at, token, false)?),
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
        if dict.contains_key(name) {
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
