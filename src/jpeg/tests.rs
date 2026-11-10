use super::vectors::{VECTORS_8BIT, VECTORS_12BIT};
use super::*;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/jpeg/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The samples of a reference decoding: `width`, `height`, `components`,
/// `maxval` and the samples. A `.pnm` is djpeg's output (P5 or P6; 12-bit
/// samples are two bytes, big-endian), a `.raw` is the output of the
/// `dump` tool of the fixtures' README (`w h c p` then little-endian
/// samples).
struct Reference {
    width: u32,
    height: u32,
    components: u8,
    maxval: u32,
    samples: Vec<u16>,
}

fn header_fields(data: &[u8], count: usize) -> (Vec<u32>, usize) {
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < count {
        while data[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while !data[i].is_ascii_whitespace() {
            i += 1;
        }
        let text = std::str::from_utf8(&data[start..i]).unwrap();
        if let Ok(v) = text.parse() {
            fields.push(v);
        }
    }
    (fields, i + 1)
}

fn reference(name: &str) -> Reference {
    let data = fixture(name);
    let (fields, body) = if name.ends_with(".raw") {
        header_fields(&data, 4)
    } else {
        let components = match data[1] {
            b'5' => 1,
            b'6' => 3,
            _ => panic!("{name}: not P5 or P6"),
        };
        let (mut f, body) = header_fields(&data[2..], 3);
        f.insert(2, components);
        (f, body + 2)
    };
    let (width, height, components, last) = (fields[0], fields[1], fields[2] as u8, fields[3]);
    let maxval = if name.ends_with(".raw") {
        (1 << last) - 1
    } else {
        last
    };
    let body = &data[body..];
    let samples: Vec<u16> = if maxval > 255 {
        let (pairs, rest) = body.as_chunks::<2>();
        assert!(rest.is_empty());
        pairs
            .iter()
            .map(|p| {
                if name.ends_with(".raw") {
                    u16::from_le_bytes(*p)
                } else {
                    u16::from_be_bytes(*p)
                }
            })
            .collect()
    } else {
        body.iter().map(|&b| u16::from(b)).collect()
    };
    assert_eq!(
        samples.len(),
        (width * height * u32::from(components)) as usize,
        "{name}"
    );
    Reference {
        width,
        height,
        components,
        maxval,
        samples,
    }
}

/// The 8-bit samples a reference decoding stands for (docs/adr/0027: 12-bit
/// samples become `div_round(v × 255, 4095)` after the colour transform).
fn expected(name: &str) -> Reference {
    let r = reference(name);
    if r.maxval == 255 {
        return r;
    }
    let samples = r
        .samples
        .iter()
        .map(|&v| crate::fixed::div_round_i64(i64::from(v) * 255, i64::from(r.maxval)) as u16)
        .collect();
    Reference {
        maxval: 255,
        samples,
        ..r
    }
}

/// Decodes `<name>.jpg` and compares it with its reference decoding: the
/// number of samples that differ and the largest difference.
fn compare(name: &str, reference_file: &str) -> (usize, u16) {
    let image = decode(&fixture(&format!("{name}.jpg"))).unwrap_or_else(|e| panic!("{name}: {e}"));
    let want = expected(reference_file);
    assert_eq!(
        (image.width, image.height, image.components),
        (want.width, want.height, want.components),
        "{name}"
    );
    assert_eq!(image.samples.len(), want.samples.len(), "{name}");
    image
        .samples
        .iter()
        .zip(&want.samples)
        .map(|(&a, &b)| u16::from(a).abs_diff(b))
        .fold((0, 0), |(n, max), d| (n + usize::from(d != 0), max.max(d)))
}

/// The fixtures whose decoding equals djpeg's (or the library's, for the
/// four-component files) sample for sample.
const EXACT: [(&str, &str, u8); 20] = [
    ("baseline-420", "baseline-420.pnm", 8),
    ("baseline-444", "baseline-444.pnm", 8),
    ("baseline-422", "baseline-422.pnm", 8),
    ("baseline-411", "baseline-411.pnm", 8),
    ("tiny-420", "tiny-420.pnm", 8),
    ("photo", "photo.pnm", 8),
    ("progressive-420", "progressive-420.pnm", 8),
    ("progressive-444", "progressive-444.pnm", 8),
    ("extended-12bit-420", "extended-12bit-420.pnm", 12),
    ("progressive-12bit-420", "progressive-12bit-420.pnm", 12),
    ("gray-12bit", "gray-12bit.pnm", 12),
    ("restart-420", "restart-420.pnm", 8),
    ("progressive-restart-420", "progressive-restart-420.pnm", 8),
    ("gray", "gray.pnm", 8),
    ("cmyk", "cmyk.raw", 8),
    ("ycck", "ycck.raw", 8),
    ("ycbcr-adobe", "ycbcr-adobe.pnm", 8),
    ("rgb-adobe", "rgb-adobe.pnm", 8),
    (
        "sequential-noninterleaved",
        "sequential-noninterleaved.pnm",
        8,
    ),
    ("idct-8bit", "idct-8bit.pnm", 8),
];

#[test]
fn decodes_the_fixtures_as_djpeg_does() {
    EXACT.iter().for_each(|&(name, reference_file, precision)| {
        assert_eq!(compare(name, reference_file), (0, 0), "{name}");
        let image = decode(&fixture(&format!("{name}.jpg"))).unwrap();
        assert_eq!(image.precision, precision, "{name}");
    });
    assert_eq!(compare("idct-12bit", "idct-12bit.pnm"), (0, 0));
}

#[test]
fn h1v2_sampling_replicates_where_libjpeg_turbo_interpolates() {
    // docs/adr/0027: only 2×1 and 2×2 get the fancy upsampling; libjpeg-turbo
    // also interpolates 1×2 (h1v2_fancy_upsample), so the chroma of this
    // file differs from djpeg's by the interpolation. The luma is the same.
    // Measured with libjpeg-turbo 3.2.0: 637 of the 741 samples differ, by
    // at most 60 (the pattern of the file has steep chroma gradients).
    let (differing, max) = compare("baseline-440", "baseline-440.pnm");
    assert!(
        differing == 637 && max == 60,
        "{differing} samples differ, by at most {max}"
    );
}

#[test]
fn the_golem_photo_is_two_by_three_with_restarts() {
    let image = decode(&fixture("photo.jpg")).unwrap();
    assert_eq!(
        (image.width, image.height, image.components, image.precision),
        (2, 3, 3, 8)
    );
    assert_eq!(image.samples.len(), 18);
}

fn segments(data: &[u8]) -> Vec<(u8, usize, usize)> {
    // (marker, start, end) of every segment before the first SOS.
    let mut out = Vec::new();
    let mut i = 2;
    while i + 4 <= data.len() {
        assert_eq!(data[i], 0xFF);
        let marker = data[i + 1];
        let len = usize::from(u16::from_be_bytes([data[i + 2], data[i + 3]]));
        out.push((marker, i, i + 2 + len));
        if marker == 0xDA {
            break;
        }
        i += 2 + len;
    }
    out
}

fn without(data: &[u8], marker: u8) -> Vec<u8> {
    let cut: Vec<(usize, usize)> = segments(data)
        .into_iter()
        .filter(|&(m, _, _)| m == marker)
        .map(|(_, s, e)| (s, e))
        .collect();
    data.iter()
        .enumerate()
        .filter(|&(i, _)| !cut.iter().any(|&(s, e)| i >= s && i < e))
        .map(|(_, &b)| b)
        .collect()
}

fn with_sof(data: &[u8], marker: u8) -> Vec<u8> {
    let (_, start, _) = segments(data)
        .into_iter()
        .find(|&(m, _, _)| m == 0xC0)
        .unwrap();
    let mut out = data.to_vec();
    out[start + 1] = marker;
    out
}

#[test]
fn sof1_with_eight_bits_decodes_as_baseline() {
    let data = with_sof(&fixture("baseline-444.jpg"), 0xC1);
    let image = decode(&data).unwrap();
    let want = expected("baseline-444.pnm");
    assert_eq!(
        image.samples,
        want.samples.iter().map(|&v| v as u8).collect::<Vec<u8>>()
    );
}

#[test]
fn arithmetic_lossless_and_hierarchical_frames_are_unsupported() {
    assert_eq!(
        decode(&fixture("arithmetic.jpg")).unwrap_err().message(),
        "unsupported JPEG SOF9"
    );
    assert_eq!(
        decode(&fixture("lossless.jpg")).unwrap_err().message(),
        "unsupported JPEG SOF3"
    );
    let baseline = fixture("baseline-444.jpg");
    [3u8, 5, 6, 7, 9, 10, 11, 13, 14, 15].iter().for_each(|&n| {
        let e = decode(&with_sof(&baseline, 0xC0 + n)).unwrap_err();
        assert_eq!(e.message(), format!("unsupported JPEG SOF{n}"));
    });
}

#[test]
fn truncated_data_is_an_error() {
    let data = fixture("baseline-420.jpg");
    (0..data.len() - 1).step_by(7).for_each(|n| {
        let e = decode(&data[..n]).unwrap_err();
        assert!(e.message().contains("JPEG"), "{n}: {e}");
    });
    let progressive = fixture("progressive-420.jpg");
    (0..progressive.len() - 1).step_by(13).for_each(|n| {
        assert!(decode(&progressive[..n]).is_err(), "{n}");
    });
}

#[test]
fn missing_tables_are_errors() {
    let data = fixture("baseline-420.jpg");
    assert_eq!(
        decode(&without(&data, 0xC4)).unwrap_err().message(),
        "corrupt JPEG data: no DC Huffman table 0"
    );
    assert_eq!(
        decode(&without(&data, 0xDB)).unwrap_err().message(),
        "corrupt JPEG data: no quantization table 0"
    );
    let progressive = fixture("progressive-420.jpg");
    assert_eq!(
        decode(&without(&progressive, 0xC4)).unwrap_err().message(),
        "corrupt JPEG data: no DC Huffman table 0"
    );
}

#[test]
fn not_a_jpeg_is_an_error() {
    assert_eq!(
        decode(b"").unwrap_err().message(),
        "corrupt JPEG data: no SOI marker"
    );
    assert_eq!(
        decode(b"\x89PNG\r\n").unwrap_err().message(),
        "corrupt JPEG data: no SOI marker"
    );
    assert_eq!(
        decode(b"\xFF\xD8\xFF\xD9").unwrap_err().message(),
        "corrupt JPEG data: no frame"
    );
}

#[test]
fn an_image_over_the_limit_is_refused_before_anything_is_read() {
    let data = fixture("baseline-444.jpg");
    let (_, start, _) = segments(&data)
        .into_iter()
        .find(|&(m, _, _)| m == 0xC0)
        .unwrap();
    // SOF: FF C0 len(2) P(1) Y(2) X(2): 4730 × 4730 × 3 = 67,118,700 > 2^26.
    let mut big = data.clone();
    big[start + 5..start + 9].copy_from_slice(&[0x12, 0x7A, 0x12, 0x7A]);
    assert_eq!(
        decode(&big).unwrap_err().message(),
        "image larger than 67108864 samples"
    );
    // 4729 × 4729 × 3 = 67,090,323 ≤ 2^26 passes the check (and then fails
    // on the data, which is for 19 × 13).
    let mut under = data.clone();
    under[start + 5..start + 9].copy_from_slice(&[0x12, 0x79, 0x12, 0x79]);
    assert_ne!(
        decode(&under).unwrap_err().message(),
        "image larger than 67108864 samples"
    );
    let mut zero = data.clone();
    zero[start + 5..start + 9].copy_from_slice(&[0, 0, 0, 19]);
    assert_eq!(
        decode(&zero).unwrap_err().message(),
        "unsupported JPEG frame with height 0 (DNL)"
    );
}

#[test]
fn idct_matches_the_vectors_of_libjpeg() {
    VECTORS_8BIT
        .iter()
        .enumerate()
        .for_each(|(i, (coefs, samples))| {
            assert_eq!(idct_islow(coefs, 8), *samples, "8-bit block {i}");
        });
    VECTORS_12BIT
        .iter()
        .enumerate()
        .for_each(|(i, (coefs, samples))| {
            assert_eq!(idct_islow(coefs, 12), *samples, "12-bit block {i}");
        });
}

/// cos(k π / 16) × 2^40, rounded to the nearest integer.
const COS: [i128; 8] = [
    1099511627776,
    1078384820155,
    1015816288660,
    914210506869,
    777472127994,
    610855931251,
    420764883643,
    214504077523,
];

/// The exact 8 × 8 inverse DCT of ITU-T T.81 A.3.3, in fixed point with 2^40
/// units: f(x, y) = 1/4 Σ C(u) C(v) F(u, v) cos((2x+1)uπ/16) cos((2y+1)vπ/16),
/// with C(0) = 1/√2, level-shifted and clamped. Each sample is rounded to the
/// nearest integer.
fn reference_idct(coefs: &[i32; 64], precision: u8) -> [u16; 64] {
    let center: i128 = 1 << (precision - 1);
    let max: i128 = (1 << precision) - 1;
    // C(u) C(v) cos cos in 2^80 units: C(0) = cos(π/4) = COS[4] / 2^40.
    let c = |u: usize, x: usize| -> i128 {
        let k = ((2 * x + 1) * u) % 32;
        let (k, sign) = match k {
            0..=8 => (k, 1),
            9..=16 => (16 - k, -1),
            17..=24 => (k - 16, -1),
            _ => (32 - k, 1),
        };
        let cu = if u == 0 { COS[4] } else { 1 << 40 };
        (sign * cu * COS[k]) >> 40
    };
    let mut out = [0u16; 64];
    (0..8).for_each(|y| {
        (0..8).for_each(|x| {
            let sum: i128 = (0..8)
                .flat_map(|v| (0..8).map(move |u| (u, v)))
                .map(|(u, v)| i128::from(coefs[v * 8 + u]) * c(u, x) * c(v, y))
                .sum();
            // sum is in 2^80 units; divide by 4 and round.
            let value = crate::fixed::div_round(sum, 4 << 80) + center;
            out[y * 8 + x] = value.clamp(0, max) as u16;
        });
    });
    out
}

fn lcg(seed: u32) -> impl FnMut() -> u32 {
    let mut x = seed;
    move || {
        x = x.wrapping_mul(1103515245).wrapping_add(12345);
        (x >> 8) & 0xff_ffff
    }
}

#[test]
fn idct_is_within_one_of_the_exact_transform() {
    // Blocks of random coefficients in the ranges of a quantized photograph:
    // the DC anywhere in its range, the AC terms small and sparse.
    [(8u8, 1016i32, 48i32), (12, 16376, 768)].iter().for_each(
        |&(precision, dc_range, ac_range)| {
            let mut next = lcg(u32::from(precision));
            (0..500).for_each(|i| {
                let mut coefs = [0i32; 64];
                coefs[0] = (next() % (2 * dc_range as u32 + 1)) as i32 - dc_range;
                (1..64).for_each(|k| {
                    if next().is_multiple_of(4) {
                        coefs[k] = (next() % (2 * ac_range as u32 + 1)) as i32 - ac_range;
                    }
                });
                let got = idct_islow(&coefs, precision);
                let want = reference_idct(&coefs, precision);
                got.iter().zip(&want).enumerate().for_each(|(k, (&g, &w))| {
                    assert!(
                        g.abs_diff(w) <= 1,
                        "{precision}-bit block {i} sample {k}: {g} vs {w}"
                    );
                });
            });
        },
    );
}

#[test]
fn idct_of_a_dc_only_block_is_flat() {
    let mut coefs = [0i32; 64];
    coefs[0] = -500;
    assert_eq!(idct_islow(&coefs, 8), [66u16; 64]);
    coefs[0] = 8 * 2000;
    assert_eq!(idct_islow(&coefs, 8), [255u16; 64]);
    coefs[0] = -8 * 2000;
    assert_eq!(idct_islow(&coefs, 12), [48u16; 64]);
    coefs[0] = -8 * 3000;
    assert_eq!(idct_islow(&coefs, 12), [0u16; 64]);
}

#[test]
fn fancy_upsampling_follows_jdsample() {
    let mut out = [0u16; 8];
    h2v1_fancy(&[10, 20, 30, 40], &mut out);
    assert_eq!(out, [10, 13, 17, 23, 27, 33, 37, 40]);
    let mut out = [0u16; 6];
    h2v2_fancy(&[10, 20, 30], &[50, 60, 70], &mut out);
    assert_eq!(out, [20, 22, 28, 32, 38, 40]);
}

#[test]
fn ycbcr_to_rgb_follows_jdcolor() {
    assert_eq!(ycbcr_to_rgb([128, 128, 128], 8), [128, 128, 128]);
    assert_eq!(ycbcr_to_rgb([255, 128, 128], 8), [255, 255, 255]);
    assert_eq!(ycbcr_to_rgb([0, 128, 128], 8), [0, 0, 0]);
    assert_eq!(ycbcr_to_rgb([128, 255, 128], 8), [128, 84, 255]);
    assert_eq!(ycbcr_to_rgb([128, 0, 0], 8), [0, 255, 0]);
    assert_eq!(ycbcr_to_rgb([2048, 2048, 2048], 12), [2048, 2048, 2048]);
    // 12-bit: R = 2048 + round(1.402 × 2047) = 2048 + 2870.
    assert_eq!(ycbcr_to_rgb([2048, 2048, 4095], 12)[0], 4095);
    assert_eq!(ycbcr_to_rgb([1000, 2048, 4095], 12)[0], 3870);
}

#[test]
fn twelve_bit_samples_are_reduced_after_the_colour_transform() {
    assert_eq!(to_eight_bits(4095, 12), 255);
    assert_eq!(to_eight_bits(0, 12), 0);
    assert_eq!(to_eight_bits(2048, 12), 128);
    assert_eq!(to_eight_bits(2047, 12), 127);
    assert_eq!(to_eight_bits(200, 8), 200);
}

#[test]
fn an_unknown_adobe_transform_is_an_error() {
    let data = fixture("ycck.jpg");
    let (_, start, _) = segments(&data)
        .into_iter()
        .find(|&(m, _, _)| m == 0xEE)
        .unwrap();
    // APP14: FF EE len(2) "Adobe"(5) version(2) flags0(2) flags1(2) transform(1).
    let mut bad = data.clone();
    bad[start + 4 + 11] = 3;
    assert_eq!(
        decode(&bad).unwrap_err().message(),
        "unsupported JPEG APP14 transform 3"
    );
    let mut ycbcr_of_four = data;
    ycbcr_of_four[start + 4 + 11] = 1;
    assert_eq!(
        decode(&ycbcr_of_four).unwrap_err().message(),
        "unsupported JPEG APP14 transform 1 with 4 components"
    );
}

#[test]
fn corrupted_data_never_panics() {
    // Every byte of each fixture overwritten in turn (by a value of the
    // generator, by 0xFF and by 0), and every truncation of the smallest
    // ones: the decoder returns a value or an error, never panics or runs
    // on.
    let files = [
        "baseline-420.jpg",
        "progressive-420.jpg",
        "progressive-restart-420.jpg",
        "restart-420.jpg",
        "extended-12bit-420.jpg",
        "progressive-12bit-420.jpg",
        "sequential-noninterleaved.jpg",
        "ycck.jpg",
        "gray.jpg",
        "photo.jpg",
    ];
    files.iter().for_each(|name| {
        let data = fixture(name);
        let mut next = lcg(data.len() as u32);
        (0..data.len()).for_each(|i| {
            let mut bad = data.clone();
            bad[i] = (next() % 256) as u8;
            let _ = decode(&bad);
            bad[i] = 0xFF;
            let _ = decode(&bad);
            bad[i] = 0;
            let _ = decode(&bad);
        });
    });
    ["tiny-420.jpg", "photo.jpg", "gray.jpg"]
        .iter()
        .for_each(|name| {
            let data = fixture(name);
            (0..data.len()).for_each(|n| {
                let _ = decode(&data[..n]);
            });
        });
}

#[test]
fn a_progressive_file_that_stops_after_some_scans_decodes_what_it_has() {
    // libjpeg decodes the scans that are there (with a warning). The DC
    // scan alone gives the blocks' means; EOI closes the file properly.
    let data = fixture("progressive-444.jpg");
    let first_sos = segments(&data)
        .into_iter()
        .find(|&(m, _, _)| m == 0xDA)
        .map(|(_, s, _)| s)
        .unwrap();
    let second_sos = (first_sos + 2..data.len() - 1)
        .find(|&i| data[i] == 0xFF && data[i + 1] == 0xDA)
        .unwrap();
    let mut short = data[..second_sos].to_vec();
    short.extend_from_slice(&[0xFF, 0xD9]);
    let image = decode(&short).unwrap();
    assert_eq!((image.width, image.height, image.components), (19, 13, 3));
    assert_ne!(image.samples, decode(&data).unwrap().samples);
}
