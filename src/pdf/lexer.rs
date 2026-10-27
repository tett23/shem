//! The lexical conventions of PDF (ISO 32000-1 7.2) and the parser of
//! objects (7.3), shared by files and content streams.

use super::{Dict, Object, Ref};
use crate::error::{Error, Result};
use crate::fixed::Fixed;
use std::borrow::Cow;

/// A token. Keywords, and names without `#` escapes, are slices of the data,
/// so that reading them allocates nothing (docs/adr/0013).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token<'a> {
    Int(i64),
    Real(Fixed),
    Name(Cow<'a, [u8]>),
    String(Vec<u8>),
    ArrayOpen,
    ArrayClose,
    DictOpen,
    DictClose,
    Keyword(&'a [u8]),
}

pub fn is_whitespace(c: u8) -> bool {
    matches!(c, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

fn is_delimiter(c: u8) -> bool {
    matches!(
        c,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn is_regular(c: u8) -> bool {
    !is_whitespace(c) && !is_delimiter(c)
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Whether a run of regular characters is a number (7.3.3).
fn is_number(s: &[u8]) -> bool {
    let body = s
        .strip_prefix(b"+")
        .or_else(|| s.strip_prefix(b"-"))
        .unwrap_or(s);
    let dots = body.iter().filter(|&&c| c == b'.').count();
    !body.is_empty()
        && dots <= 1
        && body.iter().any(u8::is_ascii_digit)
        && body.iter().all(|&c| c.is_ascii_digit() || c == b'.')
}

#[derive(Clone)]
pub struct Lexer<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(data: &'a [u8], pos: usize) -> Lexer<'a> {
        Lexer { data, pos }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    pub fn set_position(&mut self, pos: usize) {
        self.pos = pos;
    }

    fn err(&self, at: usize, message: impl Into<String>) -> Error {
        Error::new(message).at_offset(at)
    }

    /// Skips whitespace and comments.
    fn skip(&mut self) {
        while let Some(&c) = self.data.get(self.pos) {
            if is_whitespace(c) {
                self.pos += 1;
            } else if c == b'%' {
                while self
                    .data
                    .get(self.pos)
                    .is_some_and(|&c| c != b'\n' && c != b'\r')
                {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    /// The next token and its offset, or `None` at the end.
    pub fn next_token(&mut self) -> Result<Option<(usize, Token<'a>)>> {
        self.skip();
        let start = self.pos;
        let Some(&c) = self.data.get(start) else {
            return Ok(None);
        };
        let token = match c {
            b'[' => {
                self.pos += 1;
                Token::ArrayOpen
            }
            b']' => {
                self.pos += 1;
                Token::ArrayClose
            }
            b'<' if self.data.get(start + 1) == Some(&b'<') => {
                self.pos += 2;
                Token::DictOpen
            }
            b'>' if self.data.get(start + 1) == Some(&b'>') => {
                self.pos += 2;
                Token::DictClose
            }
            b'<' => self.hex_string()?,
            b'(' => self.literal_string()?,
            b'/' => self.name()?,
            b'>' | b')' | b'{' | b'}' => {
                return Err(self.err(start, format!("unexpected {}", c as char)));
            }
            _ => {
                let end = self.data[start..]
                    .iter()
                    .position(|&c| !is_regular(c))
                    .map_or(self.data.len(), |n| start + n);
                self.pos = end;
                let word = &self.data[start..end];
                if is_number(word) {
                    number(word).map_err(|e| e.at_offset(start))?
                } else {
                    Token::Keyword(word)
                }
            }
        };
        Ok(Some((start, token)))
    }

    pub fn peek_token(&self) -> Result<Option<(usize, Token<'a>)>> {
        self.clone().next_token()
    }

    fn name(&mut self) -> Result<Token<'a>> {
        let start = self.pos;
        self.pos += 1;
        let end = self.data[self.pos..]
            .iter()
            .position(|&c| !is_regular(c))
            .map_or(self.data.len(), |n| self.pos + n);
        let raw: &'a [u8] = &self.data[self.pos..end];
        self.pos = end;
        if !raw.contains(&b'#') {
            return Ok(Token::Name(Cow::Borrowed(raw)));
        }
        let mut out = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            if raw[i] == b'#' {
                let hi = raw.get(i + 1).copied().and_then(hex_value);
                let lo = raw.get(i + 2).copied().and_then(hex_value);
                match (hi, lo) {
                    (Some(hi), Some(lo)) => {
                        out.push(hi << 4 | lo);
                        i += 3;
                    }
                    _ => return Err(self.err(start, "malformed name")),
                }
            } else {
                out.push(raw[i]);
                i += 1;
            }
        }
        Ok(Token::Name(Cow::Owned(out)))
    }

    fn hex_string(&mut self) -> Result<Token<'a>> {
        let start = self.pos;
        self.pos += 1;
        // The bytes, and the high digit of the byte still to complete.
        let mut out = Vec::new();
        let mut high: Option<u8> = None;
        loop {
            match self.data.get(self.pos) {
                None => return Err(self.err(start, "unterminated hexadecimal string")),
                Some(b'>') => {
                    self.pos += 1;
                    break;
                }
                Some(&c) if is_whitespace(c) => self.pos += 1,
                Some(&c) => {
                    let v = hex_value(c)
                        .ok_or_else(|| self.err(start, "malformed hexadecimal string"))?;
                    high = match high {
                        None => Some(v),
                        Some(h) => {
                            out.push(h << 4 | v);
                            None
                        }
                    };
                    self.pos += 1;
                }
            }
        }
        // An odd final digit is followed by an implied 0 (7.3.4.3).
        out.extend(high.map(|h| h << 4));
        Ok(Token::String(out))
    }

    fn literal_string(&mut self) -> Result<Token<'a>> {
        let start = self.pos;
        self.pos += 1;
        let mut out = Vec::new();
        let mut depth = 1;
        loop {
            let c = *self
                .data
                .get(self.pos)
                .ok_or_else(|| self.err(start, "unterminated string"))?;
            self.pos += 1;
            match c {
                b'(' => {
                    depth += 1;
                    out.push(c);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(Token::String(out));
                    }
                    out.push(c);
                }
                b'\r' => {
                    // An end of line in a string is a line feed (7.3.4.2).
                    if self.data.get(self.pos) == Some(&b'\n') {
                        self.pos += 1;
                    }
                    out.push(b'\n');
                }
                b'\\' => self.escape(&mut out, start)?,
                _ => out.push(c),
            }
        }
    }

    fn escape(&mut self, out: &mut Vec<u8>, start: usize) -> Result<()> {
        let c = *self
            .data
            .get(self.pos)
            .ok_or_else(|| self.err(start, "unterminated string"))?;
        self.pos += 1;
        match c {
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'\r' => {
                if self.data.get(self.pos) == Some(&b'\n') {
                    self.pos += 1;
                }
            }
            b'\n' => {}
            b'0'..=b'7' => {
                let mut v = u32::from(c - b'0');
                for _ in 0..2 {
                    match self.data.get(self.pos) {
                        Some(&d @ b'0'..=b'7') => {
                            v = v * 8 + u32::from(d - b'0');
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                out.push((v & 0xff) as u8);
            }
            // A backslash before any other character is ignored (7.3.4.2).
            _ => out.push(c),
        }
        Ok(())
    }
}

fn number(word: &[u8]) -> Result<Token<'static>> {
    if word.contains(&b'.') {
        Fixed::parse(word).map(Token::Real)
    } else {
        std::str::from_utf8(word)
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .map(Token::Int)
            .ok_or_else(|| {
                Error::new(format!(
                    "integer out of range: {}",
                    String::from_utf8_lossy(word)
                ))
            })
    }
}

/// Parses one object. With `refs`, `a b R` is an indirect reference.
pub fn parse_object(lx: &mut Lexer, refs: bool) -> Result<Object> {
    let (at, token) = lx
        .next_token()?
        .ok_or_else(|| Error::new("unexpected end of data").at_offset(lx.position()))?;
    object_from(lx, at, token, refs)
}

/// The object that begins with `token`, read at offset `at`.
pub fn object_from(lx: &mut Lexer, at: usize, token: Token, refs: bool) -> Result<Object> {
    match token {
        Token::Int(i) => Ok(if refs { reference(lx, i)? } else { None }.unwrap_or(Object::Int(i))),
        Token::Real(v) => Ok(Object::Real(v)),
        Token::Name(n) => Ok(Object::Name(n.into_owned())),
        Token::String(s) => Ok(Object::String(s)),
        Token::ArrayOpen => {
            let mut items = Vec::new();
            loop {
                match lx.next_token()? {
                    None => return Err(Error::new("unterminated array").at_offset(at)),
                    Some((_, Token::ArrayClose)) => return Ok(Object::Array(items)),
                    Some((a, t)) => items.push(object_from(lx, a, t, refs)?),
                }
            }
        }
        Token::DictOpen => {
            let mut dict = Dict::new();
            loop {
                match lx.next_token()? {
                    None => return Err(Error::new("unterminated dictionary").at_offset(at)),
                    Some((_, Token::DictClose)) => return Ok(Object::Dict(dict)),
                    Some((_, Token::Name(key))) => {
                        let (a, t) = lx
                            .next_token()?
                            .ok_or_else(|| Error::new("unterminated dictionary").at_offset(at))?;
                        if t == Token::DictClose {
                            return Err(Error::new("dictionary key without a value").at_offset(a));
                        }
                        dict.insert(key.into_owned(), object_from(lx, a, t, refs)?);
                    }
                    Some((a, _)) => {
                        return Err(Error::new("dictionary key is not a name").at_offset(a));
                    }
                }
            }
        }
        Token::Keyword(k) => match k {
            b"true" => Ok(Object::Bool(true)),
            b"false" => Ok(Object::Bool(false)),
            b"null" => Ok(Object::Null),
            _ => Err(
                Error::new(format!("unexpected keyword {}", String::from_utf8_lossy(k)))
                    .at_offset(at),
            ),
        },
        Token::ArrayClose | Token::DictClose => {
            Err(Error::new("unexpected end of an array or a dictionary").at_offset(at))
        }
    }
}

/// `num generation R` after the integer `num`, if it is there.
fn reference(lx: &mut Lexer, num: i64) -> Result<Option<Object>> {
    let mut ahead = lx.clone();
    let generation = match ahead.next_token()? {
        Some((_, Token::Int(g))) => g,
        _ => return Ok(None),
    };
    match ahead.next_token()? {
        Some((at, Token::Keyword(k))) if k == b"R" => {
            let num = u32::try_from(num)
                .map_err(|_| Error::new("object number out of range").at_offset(at))?;
            let generation = u16::try_from(generation)
                .map_err(|_| Error::new("generation out of range").at_offset(at))?;
            *lx = ahead;
            Ok(Some(Object::Ref(Ref { num, generation })))
        }
        _ => Ok(None),
    }
}
