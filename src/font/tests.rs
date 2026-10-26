//! Fonts in PDF and the drawing of text (docs/adr/0004).

use crate::error::Result;
use crate::font::truetype::tests::{font, simple};
use crate::pdf::Document;
use crate::pdf::tests::build_pdf_bytes;
use crate::raster::Canvas;
use crate::render::render_page;

/// A TrueType font whose glyph 1 is the square from (0, 0) to (1000, 1000).
fn square_font() -> Vec<u8> {
    font(
        &[
            vec![],
            simple(&[&[
                (0, 0, true),
                (1000, 0, true),
                (1000, 1000, true),
                (0, 1000, true),
            ]]),
        ],
        false,
        1000,
    )
}

struct Text<'a> {
    encoding: &'a str,
    cidfont_extra: &'a str,
    descriptor_extra: &'a str,
    program: Option<Vec<u8>>,
    subtype: &'a str,
}

impl Default for Text<'_> {
    fn default() -> Self {
        Text {
            encoding: "/Identity-H",
            cidfont_extra: "/W [1 [500]]",
            descriptor_extra: "",
            program: Some(square_font()),
            subtype: "/CIDFontType2",
        }
    }
}

fn text_pdf(t: &Text, content: &str) -> Vec<u8> {
    let program = t.program.clone().unwrap_or_default();
    let mut stream = format!("<< /Length {} >>\nstream\n", program.len()).into_bytes();
    stream.extend(&program);
    stream.extend(b"\nendstream");
    let file_key = if t.subtype == "/CIDFontType2" {
        "/FontFile2"
    } else {
        "/FontFile3"
    };
    let file = if t.program.is_some() {
        format!("{file_key} 8 0 R")
    } else {
        String::new()
    };
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content).into_bytes(),
        format!("<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding {} /DescendantFonts [6 0 R] >>", t.encoding).into_bytes(),
        format!(
            "<< /Type /Font /Subtype {} /BaseFont /X /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 7 0 R {} >>",
            t.subtype, t.cidfont_extra
        )
        .into_bytes(),
        format!("<< /Type /FontDescriptor /FontName /X /Flags 4 {file} {} >>", t.descriptor_extra).into_bytes(),
        stream,
    ];
    build_pdf_bytes(&objects, "")
}

fn render(t: &Text, content: &str) -> Result<Canvas> {
    let data = text_pdf(t, content);
    let doc = Document::parse(&data)?;
    let pages = doc.pages()?;
    render_page(&doc, &pages[0], 72)
}

const BLACK: [u8; 3] = [0, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];

/// The pixel at (x, y) of user space, at 72 dpi on a page 40 points high.
fn at(c: &Canvas, x: usize, y: usize) -> [u8; 3] {
    c.pixel(x, 39 - y)
}

#[test]
fn a_glyph_at_its_text_matrix() {
    let c = render(&Text::default(), "BT /F1 10 Tf 1 0 0 1 5 5 Tm <0001> Tj ET").unwrap();
    assert_eq!(at(&c, 10, 10), BLACK);
    assert_eq!(at(&c, 5, 5), BLACK);
    assert_eq!(at(&c, 14, 14), BLACK);
    assert_eq!(at(&c, 4, 10), WHITE);
    assert_eq!(at(&c, 15, 10), WHITE);
    assert_eq!(at(&c, 10, 15), WHITE);
}

#[test]
fn horizontal_scaling() {
    let c = render(
        &Text::default(),
        "BT /F1 10 Tf 50 Tz 1 0 0 1 5 5 Tm <0001> Tj ET",
    )
    .unwrap();
    assert_eq!(at(&c, 7, 10), BLACK);
    assert_eq!(at(&c, 11, 10), WHITE);
}

#[test]
fn a_slanted_matrix_leans_the_glyph() {
    let c = render(
        &Text::default(),
        "BT /F1 10 Tf 1 0 0.5 1 5 5 Tm <0001> Tj ET",
    )
    .unwrap();
    // at the top, the square is shifted right by about 5
    assert_eq!(at(&c, 18, 13), BLACK);
    assert_eq!(at(&c, 6, 13), WHITE);
    assert_eq!(at(&c, 6, 5), BLACK);
}

#[test]
fn a_rotated_matrix_turns_the_glyph_clockwise() {
    // 0 -1 1 0 x y: the glyph's x goes down the page
    let c = render(
        &Text::default(),
        "BT /F1 10 Tf 0 -1 1 0 5 30 Tm <0001> Tj ET",
    )
    .unwrap();
    assert_eq!(at(&c, 10, 25), BLACK);
    assert_eq!(at(&c, 10, 31), WHITE);
    assert_eq!(at(&c, 10, 19), WHITE);
}

#[test]
fn two_glyphs_in_one_string_advance_by_their_width() {
    let c = render(
        &Text::default(),
        "BT /F1 10 Tf 1 0 0 1 5 5 Tm <00010001> Tj ET",
    )
    .unwrap();
    assert_eq!(at(&c, 17, 10), BLACK);
    assert_eq!(at(&c, 21, 10), WHITE);
    // the default width DW is 1000 when W has no entry
    let wide = Text {
        cidfont_extra: "",
        ..Text::default()
    };
    let c = render(&wide, "BT /F1 10 Tf 1 0 0 1 5 5 Tm <00010001> Tj ET").unwrap();
    assert_eq!(at(&c, 22, 10), BLACK);
}

#[test]
fn vertical_writing_moves_the_glyph_by_its_position_vector() {
    // v = (500, 880) for CID 1: the glyph goes left by 5 and down by 8.8
    let vertical = Text {
        encoding: "/Identity-V",
        cidfont_extra: "/W [1 [1000]] /W2 [1 [-1000 500 880]] /DW2 [880 -1000]",
        ..Text::default()
    };
    let c = render(&vertical, "BT /F1 10 Tf 1 0 0 1 10 20 Tm <0001> Tj ET").unwrap();
    assert_eq!(at(&c, 6, 12), BLACK);
    assert_eq!(at(&c, 14, 12), BLACK);
    assert_eq!(at(&c, 16, 12), WHITE);
    assert_eq!(at(&c, 6, 22), WHITE);
    // the next glyph goes down by w1 = -1000
    let c = render(&vertical, "BT /F1 10 Tf 1 0 0 1 10 30 Tm <00010001> Tj ET").unwrap();
    assert_eq!(at(&c, 6, 13), BLACK);
    // without W2 and DW2: vy is 880 and vx is half the width W
    let defaults = Text {
        encoding: "/Identity-V",
        cidfont_extra: "/W [1 [600]]",
        ..Text::default()
    };
    let c = render(&defaults, "BT /F1 10 Tf 1 0 0 1 10 20 Tm <0001> Tj ET").unwrap();
    assert_eq!(at(&c, 7, 12), BLACK);
    assert_eq!(at(&c, 6, 12), WHITE);
}

#[test]
fn synthetic_bold_strokes_the_outline() {
    let plain = render(
        &Text::default(),
        "BT /F1 10 Tf 2 w 1 0 0 1 5 5 Tm <0001> Tj ET",
    )
    .unwrap();
    assert_eq!(at(&plain, 4, 10), WHITE);
    let bold = render(
        &Text::default(),
        "BT /F1 10 Tf 2 Tr 2 w 1 0 0 1 5 5 Tm <0001> Tj ET",
    )
    .unwrap();
    assert_eq!(at(&bold, 4, 10), BLACK);
    assert_eq!(at(&bold, 3, 10), WHITE);
    assert_eq!(at(&bold, 15, 15), BLACK);
}

#[test]
fn fill_and_stroke_colors_are_apart() {
    let c = render(
        &Text::default(),
        "BT /F1 10 Tf 2 Tr 2 w 1 0 0 0 K 0 0 1 0 k 1 0 0 1 5 5 Tm <0001> Tj ET",
    )
    .unwrap();
    assert_eq!(at(&c, 10, 10), [255, 255, 0]);
    assert_eq!(at(&c, 4, 10), [0, 255, 255]);
}

#[test]
fn a_stroke_needs_a_ctm_that_keeps_angles() {
    let e = render(
        &Text::default(),
        "2 0 0 1 0 0 cm BT /F1 10 Tf 2 Tr 1 0 0 1 5 5 Tm <0001> Tj ET",
    )
    .unwrap_err();
    assert!(e.to_string().contains("unsupported stroke"));
    // the text matrix may stretch: the line width is in user space
    assert!(
        render(
            &Text::default(),
            "BT /F1 10 Tf 2 Tr 2 0 0 1 5 5 Tm <0001> Tj ET"
        )
        .is_ok()
    );
}

#[test]
fn what_v0_1_does_not_read() {
    let e = |t: &Text, content: &str| render(t, content).unwrap_err().to_string();
    let draw = "BT /F1 10 Tf 1 0 0 1 5 5 Tm <0001> Tj ET";
    assert!(e(&Text::default(), "BT /F1 10 Tf <000100> Tj ET").contains("odd"));
    assert!(e(&Text::default(), "BT /F1 10 Tf <0005> Tj ET").contains("glyph 5"));
    assert!(e(&Text::default(), "BT <0001> Tj ET").contains("font"));
    assert!(
        e(
            &Text {
                encoding: "/UniJIS-UCS2-H",
                ..Text::default()
            },
            draw
        )
        .contains("unsupported Encoding UniJIS-UCS2-H")
    );
    assert!(
        e(
            &Text {
                encoding: "<< /Type /CMap >>",
                ..Text::default()
            },
            draw
        )
        .contains("unsupported Encoding")
    );
    assert!(
        e(
            &Text {
                cidfont_extra: "/CIDToGIDMap [1 2]",
                ..Text::default()
            },
            draw
        )
        .contains("unsupported CIDToGIDMap")
    );
    assert!(
        e(
            &Text {
                program: None,
                ..Text::default()
            },
            draw
        )
        .contains("not embedded")
    );
    let opentype = Text {
        subtype: "/CIDFontType0",
        descriptor_extra: "",
        ..Text::default()
    };
    assert!(e(&opentype, draw).contains("FontFile3"));
}
