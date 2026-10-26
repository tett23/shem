//! Reading the images that shem writes: 8-bit RGB PNG without interlacing,
//! and binary PPM with the maximum value 255 (docs/adr/0010). Anything else
//! is an error.

use crate::error::{Error, Result};
use crate::flate::inflate_zlib;
use crate::png::{crc32, paeth};
use crate::raster::Canvas;

/// The largest number of pixels of an image (docs/adr/0007).
const MAX_PIXELS: u64 = 1 << 28;

fn bad(what: &str) -> Error {
    Error::new(format!("malformed image: {what}"))
}

fn check_size(width: u64, height: u64) -> Result<(usize, usize)> {
    if width == 0 || height == 0 {
        return Err(bad("no pixels"));
    }
    if width * height > MAX_PIXELS {
        return Err(Error::new(format!(
            "the image is too large: {width} × {height} pixels"
        )));
    }
    Ok((width as usize, height as usize))
}

/// An image, PNG or PPM by its first bytes.
pub fn decode(data: &[u8]) -> Result<Canvas> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(data)
    } else if data.starts_with(b"P6") {
        decode_ppm(data)
    } else if data.starts_with(b"P") && data.len() >= 2 && data[1].is_ascii_digit() {
        Err(Error::new(format!(
            "unsupported Netpbm format P{}",
            char::from(data[1])
        )))
    } else {
        Err(Error::new("the image is neither PNG nor PPM"))
    }
}

fn be32(d: &[u8], i: usize) -> Result<u32> {
    d.get(i..i + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| bad("unexpected end"))
}

fn decode_png(data: &[u8]) -> Result<Canvas> {
    let mut i = 8;
    let mut header: Option<(usize, usize)> = None;
    let mut idat = Vec::new();
    loop {
        let len = be32(data, i)? as usize;
        let kind = data
            .get(i + 4..i + 8)
            .ok_or_else(|| bad("unexpected end"))?;
        let body = data
            .get(i + 8..i + 8 + len)
            .ok_or_else(|| bad("unexpected end"))?;
        if be32(data, i + 8 + len)? != crc32(&data[i + 4..i + 8 + len]) {
            return Err(bad(&format!(
                "wrong CRC of {}",
                String::from_utf8_lossy(kind)
            )));
        }
        i += 12 + len;
        match kind {
            b"IHDR" if header.is_none() => {
                let (w, h) = (u64::from(be32(body, 0)?), u64::from(be32(body, 4)?));
                if body.get(8..13) != Some(&[8, 2, 0, 0, 0][..]) {
                    return Err(Error::new(
                        "unsupported PNG: only 8-bit RGB without interlacing",
                    ));
                }
                header = Some(check_size(w, h)?);
            }
            b"IDAT" if header.is_some() => idat.extend_from_slice(body),
            b"IEND" if header.is_some() => break,
            // An ancillary chunk has a lower-case first letter.
            k if k[0].is_ascii_lowercase() && header.is_some() => {}
            k => {
                return Err(Error::new(format!(
                    "unsupported chunk {}",
                    String::from_utf8_lossy(k)
                )));
            }
        }
    }
    let (width, height) = header.ok_or_else(|| bad("no IHDR"))?;
    let raw = inflate_zlib(&idat)?;
    let stride = width * 3;
    if raw.len() != height * (stride + 1) {
        return Err(bad("the image data does not have the size of the image"));
    }
    let mut pixels: Vec<u8> = Vec::with_capacity(stride * height);
    raw.chunks(stride + 1)
        .enumerate()
        .try_for_each(|(y, line)| {
            let kind = line[0];
            (0..stride).try_for_each(|x| {
                let a = if x >= 3 {
                    pixels[y * stride + x - 3]
                } else {
                    0
                };
                let b = if y > 0 {
                    pixels[(y - 1) * stride + x]
                } else {
                    0
                };
                let c = if x >= 3 && y > 0 {
                    pixels[(y - 1) * stride + x - 3]
                } else {
                    0
                };
                let predicted = match kind {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    4 => paeth(a, b, c),
                    _ => return Err(bad("filter type")),
                };
                pixels.push(line[1 + x].wrapping_add(predicted));
                Ok(())
            })
        })?;
    Ok(Canvas {
        width,
        height,
        pixels,
    })
}

/// Reads a decimal number of a PPM header after whitespace and comments.
fn ppm_number(data: &[u8], i: &mut usize) -> Result<u64> {
    loop {
        match data.get(*i) {
            Some(c) if c.is_ascii_whitespace() => *i += 1,
            Some(b'#') => {
                while data.get(*i).is_some_and(|&c| c != b'\n' && c != b'\r') {
                    *i += 1;
                }
            }
            _ => break,
        }
    }
    let start = *i;
    while data.get(*i).is_some_and(u8::is_ascii_digit) {
        *i += 1;
    }
    std::str::from_utf8(&data[start..*i])
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| bad("PPM header"))
}

fn decode_ppm(data: &[u8]) -> Result<Canvas> {
    let mut i = 2;
    let width = ppm_number(data, &mut i)?;
    let height = ppm_number(data, &mut i)?;
    let max = ppm_number(data, &mut i)?;
    if max != 255 {
        return Err(Error::new(format!(
            "unsupported PPM with the maximum value {max}"
        )));
    }
    let (width, height) = check_size(width, height)?;
    // One whitespace character ends the header.
    if !data.get(i).is_some_and(u8::is_ascii_whitespace) {
        return Err(bad("PPM header"));
    }
    let pixels = data
        .get(i + 1..i + 1 + width * height * 3)
        .ok_or_else(|| bad("the PPM has fewer pixels than its size"))?
        .to_vec();
    Ok(Canvas {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests;
