//! Images: the samples of every depth and color space, the bilinear
//! interpolation, the SMask, and the errors (docs/adr/0027).

use super::tests::render_pdf;
use crate::clip::mul_alpha;
use crate::fixed::div_round;
use crate::png::zlib;
use crate::raster::{Canvas, composite};

/// A one-page PDF whose page has the XObjects `/X1`, `/X2`, … as the
/// objects 5, 6, …, with their stream data as bytes.
pub(crate) fn pdf_with_streams(
    media_box: &str,
    content: &str,
    streams: &[(String, Vec<u8>)],
) -> Vec<u8> {
    let names: Vec<String> = (0..streams.len())
        .map(|i| format!("/X{} {} 0 R", i + 1, i + 5))
        .collect();
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [{media_box}] /Resources << /XObject << {} >> >> /Contents 4 0 R >>",
        names.join(" ")
    );
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        page.into_bytes(),
        format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            content
        )
        .into_bytes(),
    ];
    streams.iter().for_each(|(dict, data)| {
        let mut body = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        body.extend(data);
        body.extend(b"\nendstream");
        objects.push(body);
    });
    crate::pdf::tests::build_pdf_bytes(&objects, "")
}

/// An image XObject with Flate samples.
fn image(dict: &str, samples: &[u8]) -> (String, Vec<u8>) {
    (
        format!("/Type /XObject /Subtype /Image /Filter /FlateDecode {dict}"),
        zlib(samples),
    )
}

/// An image XObject with raw samples.
fn raw_image(dict: &str, samples: &[u8]) -> (String, Vec<u8>) {
    (
        format!("/Type /XObject /Subtype /Image {dict}"),
        samples.to_vec(),
    )
}

fn draw(media_box: &str, content: &str, streams: &[(String, Vec<u8>)]) -> Canvas {
    render_pdf(&pdf_with_streams(media_box, content, streams), 72).unwrap()
}

fn error(media_box: &str, content: &str, streams: &[(String, Vec<u8>)]) -> String {
    render_pdf(&pdf_with_streams(media_box, content, streams), 72)
        .unwrap_err()
        .to_string()
}

/// The sample of `b` bits `v` as 8 bits: `div_round(v × 255, 2^b − 1)`.
fn scaled(v: u32, b: u32) -> u8 {
    div_round(i128::from(v) * 255, (1i128 << b) - 1) as u8
}

/// A 2 × 2 image placed on a 2 × 2 point page at 72 dpi: each pixel is
/// one sample.
fn two_by_two(dict: &str, samples: &[u8]) -> Canvas {
    draw(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[image(&format!("/Width 2 /Height 2 {dict}"), samples)],
    )
}

/// The dictionary entries of an ICCBased stream of 3 components, whose
/// profile is empty since it is not read.
const SRGB: &str = "/N 3 /Alternate /DeviceRGB";

#[test]
fn gray_samples_of_each_depth_match_the_formula() {
    // 1 bit: 1 0 / 0 1, each row in its own byte (ISO 32000-1 8.9.3).
    let c = two_by_two("/ColorSpace /DeviceGray /BitsPerComponent 1", &[0x80, 0x40]);
    assert_eq!(c.pixel(0, 0), [255; 3]);
    assert_eq!(c.pixel(1, 0), [0; 3]);
    assert_eq!(c.pixel(0, 1), [0; 3]);
    assert_eq!(c.pixel(1, 1), [255; 3]);
    // 2 bits: 1 2 / 3 0
    let c = two_by_two("/ColorSpace /DeviceGray /BitsPerComponent 2", &[0x60, 0xC0]);
    assert_eq!(c.pixel(0, 0), [scaled(1, 2); 3]);
    assert_eq!(c.pixel(1, 0), [scaled(2, 2); 3]);
    assert_eq!(c.pixel(0, 1), [scaled(3, 2); 3]);
    assert_eq!(c.pixel(1, 1), [0; 3]);
    assert_eq!(scaled(1, 2), 85);
    // 4 bits: 5 10 / 15 1
    let c = two_by_two("/ColorSpace /DeviceGray /BitsPerComponent 4", &[0x5A, 0xF1]);
    assert_eq!(c.pixel(0, 0), [scaled(5, 4); 3]);
    assert_eq!(c.pixel(1, 0), [scaled(10, 4); 3]);
    assert_eq!(c.pixel(0, 1), [255; 3]);
    assert_eq!(c.pixel(1, 1), [scaled(1, 4); 3]);
    assert_eq!(scaled(5, 4), 85);
    // 8 bits
    let c = two_by_two(
        "/ColorSpace /DeviceGray /BitsPerComponent 8",
        &[7, 100, 200, 255],
    );
    assert_eq!(c.pixel(0, 0), [7; 3]);
    assert_eq!(c.pixel(1, 0), [100; 3]);
    assert_eq!(c.pixel(0, 1), [200; 3]);
    assert_eq!(c.pixel(1, 1), [255; 3]);
    // 16 bits: div_round(v × 255, 65535)
    let c = two_by_two(
        "/ColorSpace /DeviceGray /BitsPerComponent 16",
        &[0x00, 0x00, 0x80, 0x00, 0x80, 0x80, 0xFF, 0xFF],
    );
    assert_eq!(c.pixel(0, 0), [0; 3]);
    assert_eq!(c.pixel(1, 0), [scaled(0x8000, 16); 3]);
    assert_eq!(c.pixel(0, 1), [scaled(0x8080, 16); 3]);
    assert_eq!(c.pixel(1, 1), [255; 3]);
    assert_eq!(scaled(0x8000, 16), 128);
}

#[test]
fn icc_based_samples_are_read_by_their_number_of_components() {
    let gray = "/N 1";
    let cmyk = "/N 4";
    // N 1: DeviceGray
    let c = draw(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[
            image(
                "/Width 2 /Height 2 /ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 8",
                &[10, 20, 30, 40],
            ),
            (gray.into(), vec![]),
        ],
    );
    assert_eq!(c.pixel(0, 0), [10; 3]);
    assert_eq!(c.pixel(1, 1), [40; 3]);
    // N 3: RGB as it is
    let c = draw(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[
            image(
                "/Width 2 /Height 2 /ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 8",
                &[255, 0, 0, 0, 255, 0, 0, 0, 255, 10, 20, 30],
            ),
            (SRGB.into(), vec![]),
        ],
    );
    assert_eq!(c.pixel(0, 0), [255, 0, 0]);
    assert_eq!(c.pixel(1, 0), [0, 255, 0]);
    assert_eq!(c.pixel(0, 1), [0, 0, 255]);
    assert_eq!(c.pixel(1, 1), [10, 20, 30]);
    // N 3 with 16 bits
    let c = draw(
        "0 0 1 1",
        "q 1 0 0 1 0 0 cm /X1 Do Q",
        &[
            image(
                "/Width 1 /Height 1 /ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 16",
                &[0xFF, 0xFF, 0x80, 0x00, 0x00, 0x01],
            ),
            (SRGB.into(), vec![]),
        ],
    );
    assert_eq!(c.pixel(0, 0), [255, 128, 0]);
    // N 4: DeviceCMYK, R = 255 − min(255, C + K)
    let c = draw(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[
            image(
                "/Width 2 /Height 2 /ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 8",
                &[255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 100, 100, 100, 200],
            ),
            (cmyk.into(), vec![]),
        ],
    );
    assert_eq!(c.pixel(0, 0), [0, 255, 255]);
    assert_eq!(c.pixel(1, 0), [255, 0, 255]);
    assert_eq!(c.pixel(0, 1), [0, 0, 0]);
    assert_eq!(c.pixel(1, 1), [0, 0, 0]);
    // DeviceCMYK by its name, 4 bits
    let c = two_by_two(
        "/ColorSpace /DeviceCMYK /BitsPerComponent 4",
        &[0xF0, 0x00, 0x0F, 0x00, 0x00, 0xF0, 0x00, 0x00],
    );
    assert_eq!(c.pixel(0, 0), [0, 255, 255]);
    assert_eq!(c.pixel(1, 0), [255, 0, 255]);
    assert_eq!(c.pixel(0, 1), [255, 255, 0]);
    assert_eq!(c.pixel(1, 1), [255, 255, 255]);
    // N 2 is an error
    let two = "/N 2";
    let e = error(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[
            image(
                "/Width 2 /Height 2 /ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 8",
                &[0; 8],
            ),
            (two.into(), vec![]),
        ],
    );
    assert!(e.contains("unsupported ICCBased with N 2"), "{e}");
}

#[test]
fn indexed_samples_look_up_the_table() {
    // 2 bits, hival 3, an RGB table of four colors.
    let table = "<ff0000 00ff00 0000ff 102030>";
    let c = draw(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[
            image(
                &format!(
                    "/Width 2 /Height 2 /ColorSpace [/Indexed [/ICCBased 6 0 R] 3 {table}] /BitsPerComponent 2"
                ),
                // 0 1 / 2 3, each row in a byte
                &[0x10, 0xB0],
            ),
            (SRGB.into(), vec![]),
        ],
    );
    assert_eq!(c.pixel(0, 0), [255, 0, 0]);
    assert_eq!(c.pixel(1, 0), [0, 255, 0]);
    assert_eq!(c.pixel(0, 1), [0, 0, 255]);
    assert_eq!(c.pixel(1, 1), [16, 32, 48]);
    // A DeviceGray base with 8 bits, and a DeviceCMYK base with 1 bit.
    let c = two_by_two(
        "/ColorSpace [/Indexed /DeviceGray 1 <40c0>] /BitsPerComponent 8",
        &[0, 1, 1, 0],
    );
    assert_eq!(c.pixel(0, 0), [0x40; 3]);
    assert_eq!(c.pixel(1, 0), [0xC0; 3]);
    // 1 bit: 1 0 / 0 1, the index 1 white and the index 0 cyan.
    let c = two_by_two(
        "/ColorSpace [/Indexed /DeviceCMYK 1 <ff000000 00000000>] /BitsPerComponent 1",
        &[0x80, 0x40],
    );
    assert_eq!(c.pixel(0, 0), [255, 255, 255]);
    assert_eq!(c.pixel(1, 0), [0, 255, 255]);
    assert_eq!(c.pixel(0, 1), [0, 255, 255]);
    assert_eq!(c.pixel(1, 1), [255, 255, 255]);
}

#[test]
fn an_index_out_of_range_is_an_error() {
    let e = error(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[image(
            "/Width 2 /Height 2 /ColorSpace [/Indexed /DeviceGray 1 <40c0>] /BitsPerComponent 8",
            &[0, 1, 2, 0],
        )],
    );
    assert!(e.contains("out of range"), "{e}");
    let e = error(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[image(
            "/Width 2 /Height 2 /ColorSpace [/Indexed /DeviceGray 1 <40c0ff>] /BitsPerComponent 8",
            &[0, 1, 1, 0],
        )],
    );
    assert!(e.contains("Indexed"), "{e}");
}

#[test]
fn an_inverted_decode_inverts_the_samples() {
    let c = two_by_two(
        "/ColorSpace /DeviceGray /BitsPerComponent 8 /Decode [1 0]",
        &[0, 100, 200, 255],
    );
    assert_eq!(c.pixel(0, 0), [255; 3]);
    assert_eq!(c.pixel(1, 0), [155; 3]);
    assert_eq!(c.pixel(0, 1), [55; 3]);
    assert_eq!(c.pixel(1, 1), [0; 3]);
    // 2 bits, inverted after the scaling: 255 − scaled(1, 2)
    let c = two_by_two(
        "/ColorSpace /DeviceGray /BitsPerComponent 2 /Decode [1 0]",
        &[0x40, 0x00],
    );
    assert_eq!(c.pixel(0, 0), [255 - scaled(1, 2); 3]);
    assert_eq!(c.pixel(1, 0), [255; 3]);
    // CMYK inverted: 0 0 0 0 becomes 1 1 1 1, black
    let c = two_by_two(
        "/ColorSpace /DeviceCMYK /BitsPerComponent 8 /Decode [1 0 1 0 1 0 1 0]",
        &[
            0, 0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 255, 0, 255, 255, 0,
        ],
    );
    assert_eq!(c.pixel(0, 0), [0, 0, 0]);
    assert_eq!(c.pixel(1, 0), [255, 255, 255]);
    assert_eq!(c.pixel(0, 1), [255, 0, 0]);
    // The default array is the same as none.
    let plain = two_by_two(
        "/ColorSpace /DeviceGray /BitsPerComponent 8",
        &[0, 100, 200, 255],
    );
    let default = two_by_two(
        "/ColorSpace /DeviceGray /BitsPerComponent 8 /Decode [0 1]",
        &[0, 100, 200, 255],
    );
    assert_eq!(plain, default);
}

#[test]
fn raw_samples_and_rows_aligned_to_bytes() {
    // 3 × 2 with 1 bit: each row takes a byte (ISO 32000-1 8.9.3)
    let c = draw(
        "0 0 3 2",
        "q 3 0 0 2 0 0 cm /X1 Do Q",
        &[raw_image(
            "/Width 3 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 1",
            &[0b1010_0000, 0b0100_0000],
        )],
    );
    assert_eq!(c.pixel(0, 0), [255; 3]);
    assert_eq!(c.pixel(1, 0), [0; 3]);
    assert_eq!(c.pixel(2, 0), [255; 3]);
    assert_eq!(c.pixel(0, 1), [0; 3]);
    assert_eq!(c.pixel(1, 1), [255; 3]);
    assert_eq!(c.pixel(2, 1), [0; 3]);
}

/// The RGB of a 2 × 2 image placed on `w` × `h` pixels, by the formulas
/// of docs/adr/0027: the center of the pixel `(x + 1/2, y + 1/2)` maps to
/// `(2 (x + 1/2) / w, 2 (y + 1/2) / h)` samples, truncated to 2^−16, and
/// the four samples around it are weighted bilinearly.
fn expected_bilinear(samples: &[[u8; 3]; 4], w: i128, h: i128, x: i128, y: i128) -> [u8; 3] {
    let coord = |p: i128, size: i128| {
        // floor(2 (2p + 1) / (2 size) × 2^16) − 2^15 as (integer, fraction)
        let fixed = ((2 * (2 * p + 1)) << 16) / (2 * size) - (1 << 15);
        (fixed.div_euclid(1 << 16), fixed.rem_euclid(1 << 16))
    };
    let (i0, fx) = coord(x, w);
    let (j0, fy) = coord(y, h);
    let at = |i: i128, j: i128, k: usize| {
        let (i, j) = (i.clamp(0, 1) as usize, j.clamp(0, 1) as usize);
        i128::from(samples[j * 2 + i][k])
    };
    std::array::from_fn(|k| {
        let top = at(i0, j0, k) * ((1 << 16) - fx) + at(i0 + 1, j0, k) * fx;
        let bottom = at(i0, j0 + 1, k) * ((1 << 16) - fx) + at(i0 + 1, j0 + 1, k) * fx;
        let sum = top * ((1 << 16) - fy) + bottom * fy;
        // Rounded to the nearest, ties to even.
        let (q, r) = (sum >> 32, sum & ((1 << 32) - 1));
        let up = r > (1 << 31) || (r == (1 << 31) && q & 1 == 1);
        (q + i128::from(up)) as u8
    })
}

const CORNERS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];

fn rgb_2x2() -> (String, Vec<u8>) {
    image(
        "/Width 2 /Height 2 /ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 8",
        &CORNERS.concat(),
    )
}

#[test]
fn a_small_image_scaled_up_is_interpolated_bilinearly() {
    let c = draw(
        "0 0 300 150",
        "q 300 0 0 150 0 0 cm /X1 Do Q",
        &[rgb_2x2(), (SRGB.into(), vec![])],
    );
    assert_eq!((c.width, c.height), (300, 150));
    (0..150).for_each(|y| {
        (0..300).for_each(|x| {
            let expected = expected_bilinear(&CORNERS, 300, 150, x as i128, y as i128);
            assert_eq!(c.pixel(x, y), expected, "({x}, {y})");
        });
    });
    // The corners are the samples, clamped at the edges, and the middle
    // is the mean of all four.
    assert_eq!(c.pixel(0, 0), [255, 0, 0]);
    assert_eq!(c.pixel(299, 0), [0, 255, 0]);
    assert_eq!(c.pixel(0, 149), [0, 0, 255]);
    assert_eq!(c.pixel(299, 149), [255, 255, 255]);
    assert_eq!(c.pixel(74, 37), [255, 0, 0]);
    let mid = c.pixel(150, 75);
    assert!(mid.iter().all(|&v| (120..=136).contains(&v)), "{mid:?}");
}

#[test]
fn a_rotated_image_is_the_rotated_image() {
    let straight = draw(
        "0 0 300 150",
        "q 300 0 0 150 0 0 cm /X1 Do Q",
        &[rgb_2x2(), (SRGB.into(), vec![])],
    );
    // 90 degrees counterclockwise: the unit square goes to x 0..150, y 0..300.
    let rotated = draw(
        "0 0 150 300",
        "q 0 300 -150 0 150 0 cm /X1 Do Q",
        &[rgb_2x2(), (SRGB.into(), vec![])],
    );
    assert_eq!((rotated.width, rotated.height), (150, 300));
    (0..300).for_each(|y| {
        (0..150).for_each(|x| {
            assert_eq!(
                rotated.pixel(x, y),
                straight.pixel(299 - y, x),
                "({x}, {y})"
            );
        });
    });
}

#[test]
fn the_edges_of_an_image_are_antialiased_like_a_fill() {
    // An image placed off the pixel grid: its edge pixels have the
    // coverage of the rectangle, as a fill of it has.
    let c = draw(
        "0 0 20 20",
        "q 9.3 0 0 9.3 0.5 0.5 cm /X1 Do Q",
        &[image(
            "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[0],
        )],
    );
    let fill = render_pdf(
        &super::tests::page_pdf("0 0 20 20", "", "0.5 0.5 9.3 9.3 re f", &[]),
        72,
    )
    .unwrap();
    assert_eq!(c, fill);
}

#[test]
fn an_smask_gives_the_opacity() {
    // A black 1 × 1 image with a 2 × 2 SMask of 0, 255, 128, 64, placed on
    // 4 × 4 pixels: the opacity is interpolated at the size of the SMask.
    let alpha = [0u8, 255, 128, 64];
    let c = draw(
        "0 0 4 4",
        "q 4 0 0 4 0 0 cm /X1 Do Q",
        &[
            image(
                "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask 6 0 R",
                &[0],
            ),
            image(
                "/Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &alpha,
            ),
        ],
    );
    let samples = alpha.map(|a| [a; 3]);
    (0..4).for_each(|y| {
        (0..4).for_each(|x| {
            let a = expected_bilinear(&samples, 4, 4, x as i128, y as i128)[0];
            // The coverage is 255 inside, the opacity is 255 × a, and black
            // over white is 255 − a.
            let expected = composite(0, mul_alpha(255, a), 255);
            assert_eq!(c.pixel(x, y), [expected; 3], "({x}, {y}) alpha {a}");
        });
    });
    assert_eq!(c.pixel(0, 0), [255; 3]);
    assert_eq!(c.pixel(3, 0), [0; 3]);
}

#[test]
fn an_smask_composites_over_what_was_drawn() {
    // A red image with an SMask of 128 over a blue rectangle.
    let c = draw(
        "0 0 2 2",
        "0 0 1 0 k 0 0 2 2 re f q 2 0 0 2 0 0 cm /X1 Do Q",
        &[
            image(
                "/Width 1 /Height 1 /ColorSpace [/ICCBased 7 0 R] /BitsPerComponent 8 /SMask 6 0 R",
                &[255, 0, 0],
            ),
            image(
                "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &[128],
            ),
            (SRGB.into(), vec![]),
        ],
    );
    let over = |c: u8, p: u8| composite(c, 128, p);
    assert_eq!(c.pixel(0, 0), [over(255, 255), over(0, 255), over(0, 0)]);
    assert_eq!(c.pixel(1, 1), [over(255, 255), over(0, 255), over(0, 0)]);
}

#[test]
fn an_image_within_a_clip() {
    // The opacity is the coverage times the SMask times the clip.
    let c = draw(
        "0 0 4 4",
        "0 0 2 4 re W n q 4 0 0 4 0 0 cm /X1 Do Q",
        &[
            image(
                "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask 6 0 R",
                &[0],
            ),
            image(
                "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &[100],
            ),
        ],
    );
    assert_eq!(c.pixel(0, 0), [255 - 100; 3]);
    assert_eq!(c.pixel(1, 3), [255 - 100; 3]);
    assert_eq!(c.pixel(2, 0), [255; 3]);
    assert_eq!(c.pixel(3, 3), [255; 3]);
    // A slanted clip: the product with the coverage of the clip.
    let clip = render_pdf(
        &super::tests::page_pdf("0 0 20 20", "", "2 1 m 19 7 l 5 18 l h f", &[]),
        72,
    )
    .unwrap();
    let c = draw(
        "0 0 20 20",
        "2 1 m 19 7 l 5 18 l h W n q 20 0 0 20 0 0 cm /X1 Do Q",
        &[
            image(
                "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask 6 0 R",
                &[0],
            ),
            image(
                "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &[200],
            ),
        ],
    );
    (0..20).for_each(|y| {
        (0..20).for_each(|x| {
            let cv = 255 - clip.pixel(x, y)[0];
            let a = mul_alpha(mul_alpha(255, 200), cv);
            assert_eq!(c.pixel(x, y), [255 - a; 3], "({x}, {y})");
        });
    });
}

#[test]
fn a_degenerate_matrix_draws_nothing() {
    let c = draw(
        "0 0 2 2",
        "q 0 0 0 2 0 0 cm /X1 Do Q",
        &[image(
            "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[0],
        )],
    );
    assert!(c.pixels.iter().all(|&v| v == 255));
}

#[test]
fn an_image_beyond_the_page_is_cut() {
    let c = draw(
        "0 0 20 20",
        "q 400 0 0 300 -100 -100 cm /X1 Do Q",
        &[image(
            "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[0],
        )],
    );
    assert!(c.pixels.iter().all(|&v| v == 0));
}

#[test]
fn errors_of_images() {
    let one = |dict: &str, samples: &[u8]| {
        error(
            "0 0 2 2",
            "q 2 0 0 2 0 0 cm /X1 Do Q",
            &[image(&format!("/Width 2 /Height 2 {dict}"), samples)],
        )
    };
    let gray8 = "/ColorSpace /DeviceGray /BitsPerComponent 8";
    let e = one(&format!("{gray8} /Decode [0 0.5]"), &[0; 4]);
    assert!(e.contains("unsupported Decode"), "{e}");
    let e = one(&format!("{gray8} /Decode [1 0 1 0]"), &[0; 4]);
    assert!(e.contains("unsupported Decode"), "{e}");
    let e = one("/ImageMask true /BitsPerComponent 1", &[0; 2]);
    assert!(e.contains("unsupported ImageMask"), "{e}");
    let e = one(&format!("{gray8} /Mask [0 0]"), &[0; 4]);
    assert!(e.contains("unsupported Mask"), "{e}");
    let e = one(&format!("{gray8} /Interpolate true"), &[0; 4]);
    assert!(e.contains("unsupported Interpolate"), "{e}");
    let e = one(&format!("{gray8} /Name /Im1"), &[0; 4]);
    assert!(e.contains("unsupported Name"), "{e}");
    let e = one("/ColorSpace /DeviceGray /BitsPerComponent 3", &[0; 4]);
    assert!(e.contains("unsupported BitsPerComponent 3"), "{e}");
    let e = one("/ColorSpace /DeviceRGB /BitsPerComponent 8", &[0; 12]);
    assert!(e.contains("unsupported ColorSpace DeviceRGB"), "{e}");
    let e = one(
        "/ColorSpace [/Indexed /DeviceGray 1 <40c0>] /BitsPerComponent 16",
        &[0; 8],
    );
    assert!(e.contains("unsupported BitsPerComponent 16"), "{e}");
    let e = one(
        &format!("{gray8} /DecodeParms << /Predictor 15 >>"),
        &[0; 4],
    );
    assert!(e.contains("unsupported DecodeParms"), "{e}");
    // The samples do not have the size of the image.
    let e = one(gray8, &[0; 3]);
    assert!(e.contains("size of the image"), "{e}");
    let e = one(gray8, &[0; 5]);
    assert!(e.contains("size of the image"), "{e}");
    // The filter is unknown, or two filters.
    let e = error(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[(
            format!("/Type /XObject /Subtype /Image /Filter /LZWDecode /Width 2 /Height 2 {gray8}"),
            vec![0; 4],
        )],
    );
    assert!(e.contains("unsupported Filter LZWDecode"), "{e}");
    // The Flate data is corrupt, or cut short.
    let e = error(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[(
            format!(
                "/Type /XObject /Subtype /Image /Filter /FlateDecode /Width 2 /Height 2 {gray8}"
            ),
            b"xyz".to_vec(),
        )],
    );
    assert!(!e.is_empty());
    let cut = &zlib(&[0; 4])[..6];
    let e = error(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[(
            format!(
                "/Type /XObject /Subtype /Image /Filter /FlateDecode /Width 2 /Height 2 {gray8}"
            ),
            cut.to_vec(),
        )],
    );
    assert!(!e.is_empty());
    // Width or Height missing or not positive.
    let e = error(
        "0 0 2 2",
        "/X1 Do",
        &[image(
            "/Width 0 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[],
        )],
    );
    assert!(e.contains("Width"), "{e}");
    let e = error(
        "0 0 2 2",
        "/X1 Do",
        &[image(
            "/Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[],
        )],
    );
    assert!(e.contains("Width"), "{e}");
}

#[test]
fn an_indexed_decode_is_absent_or_zero_to_hival() {
    // docs/adr/0029: hival 3; [3 0] (the inversion of the components) and
    // [0 1] are errors, [0 3] is the default.
    let indexed = "/ColorSpace [/Indexed /DeviceGray 3 <104080f0>] /BitsPerComponent 8";
    let samples = [0, 1, 2, 3];
    let plain = two_by_two(indexed, &samples);
    assert_eq!(plain.pixel(0, 0), [0x10; 3]);
    assert_eq!(plain.pixel(1, 0), [0x40; 3]);
    assert_eq!(plain.pixel(0, 1), [0x80; 3]);
    assert_eq!(plain.pixel(1, 1), [0xf0; 3]);
    assert_eq!(
        two_by_two(&format!("{indexed} /Decode [0 3]"), &samples),
        plain
    );
    let fails = |decode: &str| {
        error(
            "0 0 2 2",
            "q 2 0 0 2 0 0 cm /X1 Do Q",
            &[image(
                &format!("/Width 2 /Height 2 {indexed} /Decode {decode}"),
                &samples,
            )],
        )
    };
    let e = fails("[3 0]");
    assert!(e.contains("unsupported Decode [3 0]"), "{e}");
    let e = fails("[0 1]");
    assert!(e.contains("unsupported Decode [0 1]"), "{e}");
    let e = fails("[0 3 0 3]");
    assert!(e.contains("unsupported Decode"), "{e}");
}

#[test]
fn a_ctm_beyond_2_57_is_an_error_of_the_corners_first() {
    // docs/adr/0029 says an image whose CTM has an entry above 2^57 is
    // `image outside the range of coordinates`. The corners of the unit
    // square are mapped to the device before the mapping is made, and such a
    // CTM fails there; the bound of the mapping is tested in
    // src/picture/tests.rs. The page is an error, "arithmetic overflow" of
    // the corners, and the message of the ADR is not reachable through `Do`.
    let e = error(
        "0 0 2 2",
        "q 100000000 0 0 2 0 0 cm /X1 Do Q",
        &[image(
            "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[0],
        )],
    );
    assert!(e.contains("arithmetic overflow"), "{e}");
}

#[test]
fn errors_of_smasks() {
    let with_mask = |mask: (String, Vec<u8>)| {
        error(
            "0 0 2 2",
            "q 2 0 0 2 0 0 cm /X1 Do Q",
            &[
                image(
                    "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask 6 0 R",
                    &[0],
                ),
                mask,
            ],
        )
    };
    let e = with_mask(image(
        "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Matte [0]",
        &[0],
    ));
    assert!(e.contains("unsupported Matte"), "{e}");
    let e = with_mask(image(
        "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 4",
        &[0],
    ));
    assert!(e.contains("BitsPerComponent 4"), "{e}");
    let e = with_mask(image(
        "/Width 1 /Height 1 /ColorSpace /DeviceCMYK /BitsPerComponent 8",
        &[0; 4],
    ));
    assert!(e.contains("DeviceCMYK"), "{e}");
    let e = with_mask(image(
        "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Decode [1 0]",
        &[0],
    ));
    assert!(e.contains("Decode"), "{e}");
    let e = with_mask((
        "/Type /XObject /Subtype /Image /Filter /DCTDecode /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8".into(),
        vec![0],
    ));
    assert!(e.contains("DCTDecode"), "{e}");
}

#[test]
fn an_image_too_large_is_an_error_before_anything_is_read() {
    // 8192 × 8193 gray samples are 2^26 + 8192: the error comes from the
    // dictionary, before the (missing) samples are read.
    let e = error(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[raw_image(
            "/Width 8192 /Height 8193 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[],
        )],
    );
    assert!(e.contains("image larger than 67108864 samples"), "{e}");
    let e = error(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[raw_image(
            "/Width 4096 /Height 4097 /ColorSpace /DeviceCMYK /BitsPerComponent 1",
            &[],
        )],
    );
    assert!(e.contains("image larger than 67108864 samples"), "{e}");
    // 2^26 samples exactly are allowed: 8192 × 8192 bits, raw.
    let c = draw(
        "0 0 2 2",
        "q 2 0 0 2 0 0 cm /X1 Do Q",
        &[raw_image(
            "/Width 8192 /Height 8192 /ColorSpace /DeviceGray /BitsPerComponent 1",
            &vec![0xFF; 8192 * 1024],
        )],
    );
    assert!(c.pixels.iter().all(|&v| v == 255));
}

#[test]
fn a_jpeg_image_is_decoded_and_checked_against_its_dictionary() {
    let jpeg = std::fs::read(format!(
        "{}/tests/fixtures/jpeg/gray.jpg",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let decoded = crate::jpeg::decode(&jpeg).unwrap();
    assert_eq!((decoded.components, decoded.precision), (1, 8));
    let (w, h) = (decoded.width as usize, decoded.height as usize);
    let dict = |dims: &str| {
        format!(
            "/Type /XObject /Subtype /Image /Filter /DCTDecode {dims} /ColorSpace /DeviceGray /BitsPerComponent 8"
        )
    };
    let c = draw(
        &format!("0 0 {w} {h}"),
        &format!("q {w} 0 0 {h} 0 0 cm /X1 Do Q"),
        &[(dict(&format!("/Width {w} /Height {h}")), jpeg.clone())],
    );
    (0..h).for_each(|y| {
        (0..w).for_each(|x| {
            let s = decoded.samples[y * w + x];
            assert_eq!(c.pixel(x, y), [s; 3], "({x}, {y})");
        });
    });
    let e = error(
        "0 0 2 2",
        "/X1 Do",
        &[(dict(&format!("/Width {} /Height {h}", w + 1)), jpeg.clone())],
    );
    assert!(e.contains("Width"), "{e}");
    let e = error(
        "0 0 2 2",
        "/X1 Do",
        &[(
            format!(
                "/Type /XObject /Subtype /Image /Filter /DCTDecode /Width {w} /Height {h} /ColorSpace /DeviceCMYK /BitsPerComponent 8"
            ),
            jpeg.clone(),
        )],
    );
    assert!(e.contains("components"), "{e}");
    let e = error(
        "0 0 2 2",
        "/X1 Do",
        &[(
            format!(
                "/Type /XObject /Subtype /Image /Filter /DCTDecode /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 16"
            ),
            jpeg.clone(),
        )],
    );
    assert!(e.contains("BitsPerComponent"), "{e}");
    // A corrupt JPEG is an error.
    let e = error(
        "0 0 2 2",
        "/X1 Do",
        &[(
            dict(&format!("/Width {w} /Height {h}")),
            jpeg[..jpeg.len() / 2].to_vec(),
        )],
    );
    assert!(e.contains("JPEG"), "{e}");
}

#[test]
fn an_adobe_cmyk_jpeg_with_an_inverted_decode() {
    let jpeg = std::fs::read(format!(
        "{}/tests/fixtures/jpeg/cmyk.jpg",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let decoded = crate::jpeg::decode(&jpeg).unwrap();
    assert_eq!(decoded.components, 4);
    let (w, h) = (decoded.width as usize, decoded.height as usize);
    let c = draw(
        &format!("0 0 {w} {h}"),
        &format!("q {w} 0 0 {h} 0 0 cm /X1 Do Q"),
        &[(
            format!(
                "/Type /XObject /Subtype /Image /Filter /DCTDecode /Width {w} /Height {h} /ColorSpace /DeviceCMYK /BitsPerComponent 8 /Decode [1 0 1 0 1 0 1 0]"
            ),
            jpeg,
        )],
    );
    (0..h).for_each(|y| {
        (0..w).for_each(|x| {
            let s = &decoded.samples[(y * w + x) * 4..][..4];
            let [c_, m, y_, k] = [s[0], s[1], s[2], s[3]].map(|v| u16::from(255 - v));
            let expected = [c_, m, y_].map(|v| (255 - (v + k).min(255)) as u8);
            assert_eq!(c.pixel(x, y), expected, "({x}, {y})");
        });
    });
}

#[test]
fn corrupted_images_never_panic() {
    // Every byte of the Flate data of an image with an SMask overwritten
    // in turn (by a value of a generator, by 0xFF and by 0), and every
    // truncation: the rendering returns an image or an error, never
    // panics.
    let mut state = 0x0027_1234u32;
    let mut next = || {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (state >> 16) as u8
    };
    let samples: Vec<u8> = (0..4 * 3 * 3).map(|i| (i * 37 % 256) as u8).collect();
    let data = zlib(&samples);
    let mask = zlib(&[0, 128, 255, 64, 32, 16, 8, 4, 2, 1, 200, 100]);
    let render = |data: &[u8], mask: &[u8]| {
        let pdf = pdf_with_streams(
            "0 0 20 20",
            "q 20 0 0 15 0 0 cm /X1 Do Q",
            &[
                (
                    "/Type /XObject /Subtype /Image /Filter /FlateDecode /Width 4 /Height 3 /ColorSpace [/ICCBased 7 0 R] /BitsPerComponent 8 /SMask 6 0 R".into(),
                    data.to_vec(),
                ),
                (
                    "/Type /XObject /Subtype /Image /Filter /FlateDecode /Width 3 /Height 4 /ColorSpace /DeviceGray /BitsPerComponent 8".into(),
                    mask.to_vec(),
                ),
                (SRGB.into(), vec![]),
            ],
        );
        let _ = render_pdf(&pdf, 72);
    };
    render(&data, &mask);
    (0..data.len()).for_each(|i| {
        let mut bad = data.clone();
        bad[i] = next();
        render(&bad, &mask);
        bad[i] = 0xFF;
        render(&bad, &mask);
        bad[i] = 0;
        render(&bad, &mask);
        render(&data[..i], &mask);
    });
    (0..mask.len()).for_each(|i| {
        let mut bad = mask.clone();
        bad[i] = next();
        render(&data, &bad);
        render(&data, &mask[..i]);
    });
}

#[test]
fn the_check_decodes_the_image_before_anything_is_drawn() {
    // A page whose first operator draws, and whose image is corrupt: the
    // check fails, so the rendering never starts.
    use crate::pdf::Document;
    let data = pdf_with_streams(
        "0 0 2 2",
        "0 0 2 2 re f q 2 0 0 2 0 0 cm /X1 Do Q",
        &[(
            "/Type /XObject /Subtype /Image /Filter /FlateDecode /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8".into(),
            b"xyz".to_vec(),
        )],
    );
    let doc = Document::parse(&data).unwrap();
    let pages = doc.pages().unwrap();
    assert!(super::check_page(&doc, &pages[0], 72).is_err());
}
