use super::*;
use crate::flate::inflate_zlib;
use crate::raster::Canvas;

fn lcg(n: usize, seed: u32) -> Vec<u8> {
    std::iter::successors(Some(seed), |x| {
        Some(x.wrapping_mul(1103515245).wrapping_add(12345) & 0x7fff_ffff)
    })
    .skip(1)
    .take(n)
    .map(|x| (x >> 16) as u8)
    .collect()
}

#[test]
fn crc32_of_a_known_string() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}

#[test]
fn deflate_round_trips() {
    let cases: Vec<Vec<u8>> = vec![
        vec![],
        b"a".to_vec(),
        b"abcabcabcabc".to_vec(),
        vec![0; 100_000],
        lcg(70_000, 5),
        // a match of distance 32768 and length 258: a random block repeated
        [lcg(32768, 9), lcg(32768, 9)[..1000].to_vec()].concat(),
        // a match of length 3 at distance 1
        b"xxxx".to_vec(),
    ];
    cases
        .iter()
        .for_each(|data| assert_eq!(&inflate_zlib(&zlib(data)).unwrap(), data, "{}", data.len()));
}

#[test]
fn deflate_finds_the_longest_and_farthest_matches() {
    let data = [lcg(32768, 9), lcg(32768, 9)[..1000].to_vec()].concat();
    // the repeated kilobyte at distance 32768 costs a few matches, not a thousand literals
    assert!(zlib(&data).len() < zlib(&lcg(32768, 9)).len() + 40);
    assert!(zlib(&vec![7; 100_000]).len() < 1000);
    // the zlib header of the fastest level and no dictionary
    assert_eq!(&zlib(b"x")[..2], &[0x78, 0x01]);
}

#[test]
fn filters_are_chosen_by_the_smallest_sum() {
    // row 0 is flat: None and Up both sum to 0 for zeros, None wins the tie
    // row 1 increases by 1 each byte: Sub gives 1s; row 2 equals row 1: Up gives 0s
    let width = 4;
    let row0 = vec![0u8; width * 3];
    let row1: Vec<u8> = (0..width * 3).map(|i| (i * 10) as u8).collect();
    let rows = [row0.clone(), row1.clone(), row1.clone()];
    let types: Vec<u8> = rows
        .iter()
        .enumerate()
        .map(|(y, row)| {
            choose_filter(
                row,
                if y == 0 {
                    None
                } else {
                    Some(rows[y - 1].as_slice())
                },
            )
            .0
        })
        .collect();
    assert_eq!(types, vec![0, 1, 2]);
}

#[test]
fn paeth_predictor() {
    assert_eq!(paeth(10, 20, 15), 15);
    assert_eq!(paeth(10, 10, 10), 10);
    assert_eq!(paeth(0, 255, 255), 0);
    assert_eq!(paeth(255, 0, 255), 0);
}

/// Reads the chunks of a PNG, checking the signature and each CRC.
fn chunks(png: &[u8]) -> Vec<(String, Vec<u8>)> {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut out = Vec::new();
    let mut i = 8;
    while i < png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
        let kind = &png[i + 4..i + 8];
        let data = &png[i + 8..i + 8 + len];
        let crc = u32::from_be_bytes(png[i + 8 + len..i + 12 + len].try_into().unwrap());
        assert_eq!(crc, crc32(&[kind, data].concat()));
        out.push((String::from_utf8(kind.to_vec()).unwrap(), data.to_vec()));
        i += 12 + len;
    }
    out
}

/// Undoes the filters of the rows.
fn unfilter(raw: &[u8], width: usize, height: usize) -> Vec<u8> {
    let stride = width * 3;
    let mut out: Vec<u8> = Vec::with_capacity(stride * height);
    (0..height).for_each(|y| {
        let line = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let kind = line[0];
        (0..stride).for_each(|x| {
            let a = if x >= 3 { out[y * stride + x - 3] } else { 0 };
            let b = if y > 0 { out[(y - 1) * stride + x] } else { 0 };
            let c = if x >= 3 && y > 0 {
                out[(y - 1) * stride + x - 3]
            } else {
                0
            };
            let predicted = match kind {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                4 => paeth(a, b, c),
                _ => panic!("filter {kind}"),
            };
            out.push(line[1 + x].wrapping_add(predicted));
        });
    });
    out
}

fn canvas(width: usize, height: usize, pixels: Vec<u8>) -> Canvas {
    Canvas {
        width,
        height,
        pixels,
    }
}

#[test]
fn png_files_hold_the_pixels() {
    let images = [
        canvas(3, 2, vec![255; 18]),
        canvas(
            4,
            3,
            (0..36)
                .map(|i| if (i / 12) % 2 == 0 { 0 } else { 255 })
                .collect(),
        ),
        canvas(
            4,
            3,
            (0..36)
                .map(|i| if (i / 3) % 2 == 0 { 10 } else { 200 })
                .collect(),
        ),
        canvas(17, 13, lcg(17 * 13 * 3, 3)),
    ];
    images.iter().for_each(|c| {
        let png = encode_png(c);
        let parts = chunks(&png);
        assert_eq!(
            parts.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            vec!["IHDR", "IDAT", "IEND"]
        );
        let ihdr = &parts[0].1;
        assert_eq!(
            u32::from_be_bytes(ihdr[0..4].try_into().unwrap()) as usize,
            c.width
        );
        assert_eq!(
            u32::from_be_bytes(ihdr[4..8].try_into().unwrap()) as usize,
            c.height
        );
        assert_eq!(&ihdr[8..], &[8, 2, 0, 0, 0]);
        let raw = inflate_zlib(&parts[1].1).unwrap();
        assert_eq!(unfilter(&raw, c.width, c.height), c.pixels);
        assert!(parts[2].1.is_empty());
    });
}

#[test]
fn ppm_files_hold_the_pixels() {
    let c = canvas(2, 1, vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(
        encode_ppm(&c),
        b"P6\n2 1\n255\n\x01\x02\x03\x04\x05\x06".to_vec()
    );
}

// --- the speed-ups that keep the bytes (docs/adr/0011)

/// The choice of filters before docs/adr/0011: every filtered row is made.
fn reference_choose_filter(row: &[u8], prev: Option<&[u8]>) -> (u8, Vec<u8>) {
    let up = |x: usize| prev.map_or(0, |p| p[x]);
    let left = |x: usize| if x >= BPP { row[x - BPP] } else { 0 };
    let up_left = |x: usize| {
        if x >= BPP {
            prev.map_or(0, |p| p[x - BPP])
        } else {
            0
        }
    };
    let filtered = |kind: u8| -> Vec<u8> {
        (0..row.len())
            .map(|x| {
                let predicted = match kind {
                    0 => 0,
                    1 => left(x),
                    2 => up(x),
                    3 => ((u16::from(left(x)) + u16::from(up(x))) / 2) as u8,
                    _ => paeth(left(x), up(x), up_left(x)),
                };
                row[x].wrapping_sub(predicted)
            })
            .collect()
    };
    let cost = |r: &[u8]| {
        r.iter()
            .map(|&b| u64::from((b as i8).unsigned_abs()))
            .sum::<u64>()
    };
    (0..5u8)
        .map(|k| (k, filtered(k)))
        .min_by_key(|(k, r)| (cost(r), *k))
        .unwrap_or((0, row.to_vec()))
}

#[test]
fn filters_are_chosen_as_before() {
    let width = 37 * 3;
    let random = lcg(width * 400, 21);
    let rows: Vec<Vec<u8>> = (0..1000)
        .map(|y| match y % 5 {
            0 => random[(y / 5 % 400) * width..(y / 5 % 400 + 1) * width].to_vec(),
            1 => vec![(y % 256) as u8; width],
            2 => (0..width)
                .map(|x| if (x + y) % 2 == 0 { 0 } else { 255 })
                .collect(),
            3 => (0..width).map(|x| (x * y % 256) as u8).collect(),
            _ => vec![255; width],
        })
        .collect();
    rows.iter().enumerate().for_each(|(y, row)| {
        let prev = if y == 0 {
            None
        } else {
            Some(rows[y - 1].as_slice())
        };
        assert_eq!(
            choose_filter(row, prev),
            reference_choose_filter(row, prev),
            "row {y}"
        );
    });
}

/// The length of a match before docs/adr/0011: one byte at a time.
fn reference_match_length(data: &[u8], candidate: usize, i: usize, max: usize) -> usize {
    data[candidate..]
        .iter()
        .zip(&data[i..i + max])
        .take_while(|(a, b)| a == b)
        .count()
}

#[test]
fn match_lengths_are_as_before() {
    for len in [0, 1, 3, 7, 8, 9, 15, 16, 17, 257, 258] {
        // a prefix of `len` equal bytes, then a different byte
        let mut data = lcg(300, 4);
        let mut second = data[..len.min(300)].to_vec();
        second.push(data.get(len).map_or(0, |b| b.wrapping_add(1)));
        second.extend(lcg(300, 5));
        let at = data.len();
        data.extend(second);
        let max = MAX_MATCH.min(data.len() - at);
        assert_eq!(
            match_length(&data, 0, at, max),
            reference_match_length(&data, 0, at, max),
            "{len}"
        );
    }
    // the maximum is a limit even when more bytes match
    let data = vec![9; 1000];
    for max in [3, 7, 8, 9, 258] {
        assert_eq!(match_length(&data, 0, 300, max), max);
    }
}

#[test]
fn zlib_streams_are_as_before() {
    let inputs: Vec<Vec<u8>> = vec![
        vec![],
        b"abcabcabcabc".to_vec(),
        vec![0; 100_000],
        lcg(70_000, 5),
        [lcg(32768, 9), lcg(32768, 9)[..1000].to_vec()].concat(),
    ];
    inputs
        .iter()
        .for_each(|d| assert_eq!(zlib(d), zlib_with(d, reference_match_length)));
    // the filtered rows of the goldens of Golem's fixtures
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    names
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .for_each(|p| {
            let c = crate::image::decode(&std::fs::read(p).unwrap()).unwrap();
            let rows: Vec<&[u8]> = c.pixels.chunks(c.width * 3).collect();
            let raw: Vec<u8> = rows
                .iter()
                .enumerate()
                .flat_map(|(y, row)| {
                    let (k, f) = choose_filter(row, if y == 0 { None } else { Some(rows[y - 1]) });
                    std::iter::once(k).chain(f)
                })
                .collect();
            assert_eq!(
                zlib(&raw),
                zlib_with(&raw, reference_match_length),
                "{}",
                p.display()
            );
        });
}
