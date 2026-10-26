//! The error of shem: where it happened and what it was (docs/adr/0002).

use std::fmt;

/// An error, with the place where it happened as far as it is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub page: Option<usize>,
    pub object: Option<u32>,
    pub offset: Option<usize>,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(message: impl Into<String>) -> Error {
        Error {
            page: None,
            object: None,
            offset: None,
            message: message.into(),
        }
    }

    pub fn overflow() -> Error {
        Error::new("arithmetic overflow")
    }

    /// The error with the page number, unless it already has one.
    pub fn at_page(self, page: usize) -> Error {
        Error {
            page: self.page.or(Some(page)),
            ..self
        }
    }

    /// The error with the object number, unless it already has one.
    pub fn at_object(self, object: u32) -> Error {
        Error {
            object: self.object.or(Some(object)),
            ..self
        }
    }

    /// The error with the byte offset, unless it already has one.
    pub fn at_offset(self, offset: usize) -> Error {
        Error {
            offset: self.offset.or(Some(offset)),
            ..self
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let places = [
            self.page.map(|p| format!("page {p}")),
            self.object.map(|o| format!("object {o}")),
            self.offset.map(|o| format!("offset {o}")),
        ];
        places
            .iter()
            .flatten()
            .try_for_each(|place| write!(f, "{place}: "))?;
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_lists_the_known_places() {
        let e = Error::new("unsupported operator TJ")
            .at_offset(12)
            .at_page(3);
        assert_eq!(e.to_string(), "page 3: offset 12: unsupported operator TJ");
    }

    #[test]
    fn inner_place_wins() {
        let e = Error::new("x").at_page(1).at_page(2);
        assert_eq!(e.page, Some(1));
    }
}
