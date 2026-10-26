//! Images and forms as gray boxes with a cross (docs/adr/0006).

use super::tests::{page_pdf, render_pdf};
use crate::raster::Canvas;

const IMAGE: &str = "<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode /Length 3 >>\nstream\nxyz\nendstream";
const FORM: &str = "<< /Type /XObject /Subtype /Form /BBox [0 0 200 100] /Matrix [0.005 0 0 0.01 0 0] /Length 3 >>\nstream\nq Q\nendstream";
const GRAY: [u8; 3] = [192, 192, 192];
const LINE: [u8; 3] = [96, 96, 96];
const WHITE: [u8; 3] = [255, 255, 255];

fn draw(xobject: &str, content: &str, dpi: u32) -> Canvas {
    render_pdf(
        &page_pdf(
            "0 0 200 100",
            "/XObject << /X1 5 0 R >>",
            content,
            &[xobject],
        ),
        dpi,
    )
    .unwrap()
}

#[test]
fn an_image_is_a_gray_box_with_a_cross() {
    let c = draw(IMAGE, "q 100 0 0 50 10 20 cm /X1 Do Q", 144);
    // the box: x from 20 to 220, y from 60 to 160 in pixels at 144 dpi
    assert_eq!(c.pixel(24, 110), GRAY);
    assert_eq!(c.pixel(216, 110), GRAY);
    assert_eq!(c.pixel(18, 110), WHITE);
    assert_eq!(c.pixel(222, 110), WHITE);
    assert_eq!(c.pixel(120, 58), WHITE);
    // the diagonals cross at the center, 1 point (2 pixels) wide
    assert_eq!(c.pixel(119, 109), LINE);
    // a point on the diagonal from the bottom left (20, 160) to the top right (220, 60)
    assert_eq!(c.pixel(70, 134), LINE);
}

#[test]
fn a_form_with_the_matrix_of_golem_is_the_same_box() {
    let image = draw(IMAGE, "q 100 0 0 50 10 20 cm /X1 Do Q", 144);
    let form = draw(FORM, "q 100 0 0 50 10 20 cm /X1 Do Q", 144);
    assert_eq!(image, form);
}

#[test]
fn a_form_without_a_matrix_maps_its_bbox() {
    let form =
        "<< /Type /XObject /Subtype /Form /BBox [10 20 110 70] /Length 0 >>\nstream\n\nendstream";
    let image = draw(IMAGE, "q 100 0 0 50 10 20 cm /X1 Do Q", 72);
    assert_eq!(draw(form, "/X1 Do", 72), image);
}

#[test]
fn a_rotated_box() {
    // 90 degrees: 0 50 -100 0 110 20 maps the unit square to x 10..110, y 20..70 turned
    let c = draw(IMAGE, "q 0 50 -100 0 110 20 cm /X1 Do Q", 72);
    let straight = draw(IMAGE, "q 100 0 0 50 10 20 cm /X1 Do Q", 72);
    assert_eq!(c.pixel(12, 55), GRAY);
    assert_eq!(c.pixel(12, 55), straight.pixel(12, 55));
}

#[test]
fn a_box_beyond_the_page_is_cut() {
    let c = draw(IMAGE, "q 400 0 0 300 -100 -100 cm /X1 Do Q", 72);
    assert_ne!(c.pixel(5, 5), WHITE);
}

#[test]
fn a_degenerate_matrix_draws_nothing() {
    let c = draw(IMAGE, "q 0 0 0 50 10 20 cm /X1 Do Q", 72);
    assert!(c.pixels.iter().all(|&v| v == 255));
}

#[test]
fn the_contents_of_xobjects_are_not_read() {
    let broken = "<< /Type /XObject /Subtype /Image /Filter /FlateDecode /Length 3 >>\nstream\nxyz\nendstream";
    let unknown = "<< /Type /XObject /Subtype /Image /Filter /JBIG2Decode /DecodeParms << >> /Length 3 >>\nstream\nxyz\nendstream";
    draw(broken, "q 10 0 0 10 0 0 cm /X1 Do Q", 72);
    draw(unknown, "q 10 0 0 10 0 0 cm /X1 Do Q", 72);
}

#[test]
fn errors() {
    let e = |xobject: &str| {
        render_pdf(
            &page_pdf(
                "0 0 200 100",
                "/XObject << /X1 5 0 R >>",
                "/X1 Do",
                &[xobject],
            ),
            72,
        )
        .unwrap_err()
        .to_string()
    };
    assert!(
        e("<< /Subtype /PS /Length 0 >>\nstream\n\nendstream")
            .contains("unsupported XObject Subtype PS")
    );
    assert!(e("<< /Subtype /Form /Length 0 >>\nstream\n\nendstream").contains("BBox"));
    let missing = render_pdf(&page_pdf("0 0 200 100", "", "/X1 Do", &[]), 72)
        .unwrap_err()
        .to_string();
    assert!(missing.contains("no XObject X1"));
}

#[test]
fn a_box_does_not_change_the_colors() {
    let c = draw(
        IMAGE,
        "0 0 1 0 k q 10 0 0 10 0 0 cm /X1 Do Q 100 0 10 10 re f",
        72,
    );
    assert_eq!(c.pixel(105, 95), [255, 255, 0]);
}
