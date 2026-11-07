//! The error of shem: where it happened and what it was (docs/adr/0002).

use std::fmt;

/// An error, with the place where it happened as far as it is known. The
/// fields are behind a `Box`, so that a `Result` of a value is small and
/// the path without an error moves little.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(Box<Place>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    page: Option<usize>,
    object: Option<u32>,
    offset: Option<usize>,
    message: String,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(message: impl Into<String>) -> Error {
        Error(Box::new(Place {
            page: None,
            object: None,
            offset: None,
            message: message.into(),
        }))
    }

    pub fn overflow() -> Error {
        Error::new("arithmetic overflow")
    }

    pub fn page(&self) -> Option<usize> {
        self.0.page
    }

    pub fn object(&self) -> Option<u32> {
        self.0.object
    }

    pub fn offset(&self) -> Option<usize> {
        self.0.offset
    }

    pub fn message(&self) -> &str {
        &self.0.message
    }

    /// The error with the place `update` makes of its place.
    fn with(mut self, update: impl FnOnce(&mut Place)) -> Error {
        update(&mut self.0);
        self
    }

    /// The error with the page number, unless it already has one.
    pub fn at_page(self, page: usize) -> Error {
        self.with(|p| p.page = p.page.or(Some(page)))
    }

    /// The error with the object number, unless it already has one.
    pub fn at_object(self, object: u32) -> Error {
        self.with(|p| p.object = p.object.or(Some(object)))
    }

    /// The error with the byte offset, unless it already has one.
    pub fn at_offset(self, offset: usize) -> Error {
        self.with(|p| p.offset = p.offset.or(Some(offset)))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let places = [
            self.0.page.map(|p| format!("page {p}")),
            self.0.object.map(|o| format!("object {o}")),
            self.0.offset.map(|o| format!("offset {o}")),
        ];
        places
            .iter()
            .flatten()
            .try_for_each(|place| write!(f, "{place}: "))?;
        write!(f, "{}", self.0.message)
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
        assert_eq!(e.page(), Some(1));
    }

    #[test]
    fn a_result_of_a_number_is_small() {
        // docs/adr/0019: the fields are behind a Box.
        assert!(std::mem::size_of::<Result<crate::fixed::Fixed>>() <= 16);
    }

    #[test]
    fn places_stack_as_before() {
        let e = Error::new("bad")
            .at_offset(7)
            .at_object(4)
            .at_page(2)
            .at_offset(9)
            .at_object(5)
            .at_page(3);
        assert_eq!(e.to_string(), "page 2: object 4: offset 7: bad");
        assert_eq!(
            (e.page(), e.object(), e.offset()),
            (Some(2), Some(4), Some(7))
        );
        assert_eq!(e.message(), "bad");
    }

    #[test]
    fn every_combination_of_places_reads_and_prints_as_before() {
        // docs/adr/0019: the accessors read the fields behind the Box, and
        // Display lists the known places in the order page, object, offset,
        // whatever the order they were given in.
        [None, Some(3usize)].iter().for_each(|&page| {
            [None, Some(12u32)].iter().for_each(|&object| {
                [None, Some(7usize)].iter().for_each(|&offset| {
                    let e = Error::new("bad");
                    let e = page.into_iter().fold(e, Error::at_page);
                    let e = object.into_iter().fold(e, Error::at_object);
                    let e = offset.into_iter().fold(e, Error::at_offset);
                    let reversed = Error::new("bad");
                    let reversed = offset.into_iter().fold(reversed, Error::at_offset);
                    let reversed = object.into_iter().fold(reversed, Error::at_object);
                    let reversed = page.into_iter().fold(reversed, Error::at_page);
                    let expected: String = [
                        page.map(|p| format!("page {p}: ")),
                        object.map(|o| format!("object {o}: ")),
                        offset.map(|o| format!("offset {o}: ")),
                    ]
                    .into_iter()
                    .flatten()
                    .chain(std::iter::once("bad".to_string()))
                    .collect();
                    assert_eq!(e.to_string(), expected);
                    assert_eq!(reversed, e);
                    assert_eq!((e.page(), e.object(), e.offset()), (page, object, offset));
                    assert_eq!(e.message(), "bad");
                    assert_eq!(e.clone(), e);
                    // A place that is known is kept; one that is not is taken.
                    let again = e.clone().at_page(99).at_object(99).at_offset(99);
                    assert_eq!(
                        (again.page(), again.object(), again.offset()),
                        (page.or(Some(99)), object.or(Some(99)), offset.or(Some(99)))
                    );
                    assert!(format!("{e:?}").contains("bad"));
                });
            });
        });
    }

    #[test]
    fn messages_and_the_error_trait() {
        assert_eq!(Error::overflow().to_string(), "arithmetic overflow");
        assert_eq!(Error::new(String::from("owned")).message(), "owned");
        assert_eq!(Error::new("").at_offset(0).to_string(), "offset 0: ");
        let e: Box<dyn std::error::Error> = Box::new(Error::new("boxed").at_page(1));
        assert_eq!(e.to_string(), "page 1: boxed");
        assert_ne!(Error::new("a"), Error::new("b"));
        assert_ne!(Error::new("a"), Error::new("a").at_page(1));
    }
}
