//! JPEG (DCTDecode): the stub the tests of docs/adr/0027 were written against.

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub components: u8,
    pub precision: u8,
    pub samples: Vec<u8>,
}

pub fn decode(_data: &[u8]) -> Result<Image> {
    Err(Error::new("not implemented"))
}

pub fn idct_islow(_coefs: &[i32; 64], _precision: u8) -> [u16; 64] {
    [0; 64]
}

pub fn h2v1_fancy(_input: &[u16], _out: &mut [u16]) {}

pub fn h2v2_fancy(_near: &[u16], _far: &[u16], _out: &mut [u16]) {}

pub fn ycbcr_to_rgb(_ycc: [u16; 3], _precision: u8) -> [u16; 3] {
    [0; 3]
}

pub fn to_eight_bits(_v: u16, _precision: u8) -> u8 {
    0
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod vectors;
