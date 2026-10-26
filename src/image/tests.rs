use super::*;
use crate::png::{crc32, encode_png, encode_ppm, zlib};
use crate::raster::Canvas;

fn canvas(width: usize, height: usize, seed: u8) -> Canvas {
    let pixels = (0..width * height * 3)
        .map(|i| (i as u8).wrapping_mul(37).wrapping_add(seed))
        .collect();
    Canvas {
        width,
        height,
        pixels,
    }
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(data);
    out.extend(crc32(&[&kind[..], data].concat()).to_be_bytes());
    out
}

fn ihdr(width: u32, height: u32, depth: u8, color: u8, interlace: u8) -> Vec<u8> {
    let mut d = width.to_be_bytes().to_vec();
    d.extend(height.to_be_bytes());
    d.extend([depth, color, 0, 0, interlace]);
    chunk(b"IHDR", &d)
}

/// A PNG of an image with no filter, with the IDAT split at `split` bytes and extra chunks.
fn png_with(c: &Canvas, split: usize, extra: &[Vec<u8>]) -> Vec<u8> {
    let raw: Vec<u8> = c
        .pixels
        .chunks(c.width * 3)
        .flat_map(|row| std::iter::once(0).chain(row.iter().copied()))
        .collect();
    let z = zlib(&raw);
    let (a, b) = z.split_at(split.min(z.len()));
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(ihdr(c.width as u32, c.height as u32, 8, 2, 0));
    extra.iter().for_each(|e| out.extend(e));
    out.extend(chunk(b"IDAT", a));
    out.extend(chunk(b"IDAT", b));
    out.extend(chunk(b"IEND", &[]));
    out
}

#[test]
fn reads_the_png_and_ppm_that_shem_writes() {
    for c in [canvas(1, 1, 0), canvas(7, 5, 3), canvas(33, 17, 9)] {
        assert_eq!(decode(&encode_png(&c)).unwrap(), c);
        assert_eq!(decode(&encode_ppm(&c)).unwrap(), c);
    }
}

#[test]
fn reads_split_idat_and_ancillary_chunks() {
    let c = canvas(9, 4, 1);
    let text = chunk(b"tEXt", b"Comment\0hello");
    assert_eq!(decode(&png_with(&c, 10, &[text])).unwrap(), c);
}

#[test]
fn ppm_headers_with_comments_and_spaces() {
    let c = canvas(2, 1, 5);
    let mut data = b"P6 # a comment\n2\t1 # another\r\n255\n".to_vec();
    data.extend(&c.pixels);
    assert_eq!(decode(&data).unwrap(), c);
}

fn error_of(data: &[u8]) -> String {
    decode(data).unwrap_err().to_string()
}

#[test]
fn rejects_what_shem_does_not_write() {
    let c = canvas(4, 3, 2);
    let good = encode_png(&c);
    let with_ihdr = |h: Vec<u8>| [&good[..8], &h[..], &good[8 + 25..]].concat();
    assert!(error_of(&with_ihdr(ihdr(4, 3, 8, 6, 0))).contains("unsupported"));
    assert!(error_of(&with_ihdr(ihdr(4, 3, 16, 2, 0))).contains("unsupported"));
    assert!(error_of(&with_ihdr(ihdr(4, 3, 8, 2, 1))).contains("unsupported"));
    let plte = chunk(b"PLTE", &[0, 0, 0]);
    assert!(error_of(&png_with(&c, 5, &[plte])).contains("unsupported chunk PLTE"));
    let mut bad_crc = good.clone();
    let last = bad_crc.len() - 13;
    bad_crc[last] ^= 1;
    assert!(error_of(&bad_crc).contains("CRC"));
    let mut bad_zlib = png_with(&c, 1000, &[]);
    // break the zlib data of the first IDAT and fix its CRC
    let at = bad_zlib.windows(4).position(|w| w == b"IDAT").unwrap();
    let len = u32::from_be_bytes(bad_zlib[at - 4..at].try_into().unwrap()) as usize;
    bad_zlib[at + 4 + len - 1] ^= 0xff;
    let crc = crc32(&bad_zlib[at..at + 4 + len]);
    bad_zlib[at + 4 + len..at + 8 + len].copy_from_slice(&crc.to_be_bytes());
    assert!(decode(&bad_zlib).is_err());
    assert!(decode(&good[..good.len() - 20]).is_err());
    assert!(error_of(b"P3\n1 1\n255\n0 0 0\n").contains("unsupported"));
    assert!(error_of(b"P6\n1 1\n65535\n\0\0\0\0\0\0").contains("unsupported"));
    assert!(decode(b"P6\n2 2\n255\n\0\0\0").is_err());
    assert!(error_of(b"GIF89a").contains("neither PNG nor PPM"));
    assert!(error_of(b"P6\n100000 100000\n255\n").contains("too large"));
}
