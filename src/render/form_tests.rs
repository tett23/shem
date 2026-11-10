//! Form XObjects: the BBox, the Matrix, the graphics state, the nesting
//! and the errors (docs/adr/0027).

use super::image_tests::pdf_with_streams;
use super::tests::{page_pdf, render_pdf};
use crate::raster::Canvas;

/// A Form XObject with the given dictionary entries and content.
fn form(dict: &str, content: &str) -> (String, Vec<u8>) {
    (
        format!("/Type /XObject /Subtype /Form {dict}"),
        content.as_bytes().to_vec(),
    )
}

const NO_RESOURCES: &str = "/Resources << >>";

fn draw(media_box: &str, content: &str, streams: &[(String, Vec<u8>)]) -> Canvas {
    render_pdf(&pdf_with_streams(media_box, content, streams), 72).unwrap()
}

fn error(media_box: &str, content: &str, streams: &[(String, Vec<u8>)]) -> String {
    render_pdf(&pdf_with_streams(media_box, content, streams), 72)
        .unwrap_err()
        .to_string()
}

fn plain(media_box: &str, content: &str) -> Canvas {
    render_pdf(&page_pdf(media_box, "", content, &[]), 72).unwrap()
}

#[test]
fn the_bbox_clips_the_contents() {
    // The form fills the whole page, but its BBox is 10 × 10 at (5, 5).
    let c = draw(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [5 5 15 15] {NO_RESOURCES}"),
            "0 0 100 100 re f",
        )],
    );
    assert_eq!(c, plain("0 0 20 20", "5 5 10 10 re f"));
    // The BBox is mapped by the CTM, and its edges are antialiased as a
    // clip is: the fill reaches beyond the BBox on every side.
    let c = draw(
        "0 0 20 20",
        "q 1 0 0 1 0.5 0.5 cm /X1 Do Q",
        &[form(
            &format!("/BBox [0 0 9.3 9.3] {NO_RESOURCES}"),
            "-10 -10 100 100 re f",
        )],
    );
    assert_eq!(c, plain("0 0 20 20", "0.5 0.5 9.3 9.3 re f"));
}

#[test]
fn the_matrix_multiplies_the_ctm() {
    let c = draw(
        "0 0 20 20",
        "q 1 0 0 1 2 2 cm /X1 Do Q",
        &[form(
            &format!("/BBox [0 0 100 100] /Matrix [2 0 0 2 1 1] {NO_RESOURCES}"),
            "0 0 5 5 re f",
        )],
    );
    // (0, 0) to (5, 5), by the Matrix to (1, 1) to (11, 11), by the CTM
    // to (3, 3) to (13, 13).
    assert_eq!(c, plain("0 0 20 20", "3 3 10 10 re f"));
    // Golem's form: the BBox mapped to the unit square, placed by cm.
    let c = draw(
        "0 0 200 100",
        "q 100 0 0 50 10 20 cm /X1 Do Q",
        &[form(
            &format!("/BBox [0 0 200 100] /Matrix [0.005 0 0 0.01 0 0] {NO_RESOURCES}"),
            "0 0 100 50 re f",
        )],
    );
    assert_eq!(c, plain("0 0 200 100", "10 20 50 25 re f"));
}

#[test]
fn the_graphics_state_is_restored_after_do() {
    // The form changes the fill color, the line width and the CTM; the
    // rectangle after it is black and 1 point wide, where it was.
    let c = draw(
        "0 0 40 40",
        "/X1 Do 0 0 10 10 re f 5 w 20 20 m 40 20 l S",
        &[form(
            &format!("/BBox [0 0 100 100] {NO_RESOURCES}"),
            "0 0 1 0 k 1 0 0 0 K 20 w 2 0 0 2 10 10 cm 0 0 5 5 re f",
        )],
    );
    assert_eq!(
        c,
        plain(
            "0 0 40 40",
            "0 0 1 0 k 10 10 10 10 re f 0 0 0 1 k 0 0 10 10 re f 5 w 20 20 m 40 20 l S"
        )
    );
    // The clip of the BBox is gone after Do.
    let c = draw(
        "0 0 20 20",
        "/X1 Do 0 0 20 20 re f",
        &[form(&format!("/BBox [0 0 5 5] {NO_RESOURCES}"), "")],
    );
    assert!(c.pixels.iter().all(|&v| v == 0));
    // A form does not change the clip of the page.
    let c = draw(
        "0 0 20 20",
        "0 0 10 10 re W n /X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] {NO_RESOURCES}"),
            "0 0 100 100 re f",
        )],
    );
    assert_eq!(c, plain("0 0 20 20", "0 0 10 10 re f"));
}

#[test]
fn the_names_of_a_form_come_from_its_resources() {
    // The form's color space /C1 is an ICCBased RGB space of its own
    // resources, which the page does not have.
    let c = draw(
        "0 0 20 20",
        "/X1 Do",
        &[
            form(
                "/BBox [0 0 100 100] /Resources << /ColorSpace << /C1 [/ICCBased 6 0 R] >> >>",
                "/C1 cs 1 0 0 sc 0 0 10 10 re f",
            ),
            ("/N 3".into(), vec![]),
        ],
    );
    assert_eq!(c.pixel(5, 15), [255, 0, 0]);
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] {NO_RESOURCES}"),
            "/C1 cs 1 0 0 sc 0 0 10 10 re f",
        )],
    );
    assert!(e.contains("no color space C1"), "{e}");
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form("/BBox [0 0 100 100]", "0 0 10 10 re f")],
    );
    assert!(e.contains("Resources"), "{e}");
}

#[test]
fn a_form_may_do_images_and_forms() {
    // The form places an image, and another form places that form.
    let image = (
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8".to_string(),
        vec![0u8],
    );
    let c = draw(
        "0 0 20 20",
        "/X1 Do",
        &[
            form(
                "/BBox [0 0 100 100] /Resources << /XObject << /F 6 0 R >> >>",
                "q 1 0 0 1 2 2 cm /F Do Q",
            ),
            form(
                "/BBox [0 0 100 100] /Resources << /XObject << /I 7 0 R >> >>",
                "q 10 0 0 10 0 0 cm /I Do Q",
            ),
            image,
        ],
    );
    assert_eq!(c, plain("0 0 20 20", "2 2 10 10 re f"));
}

/// The streams of `n` forms, each placing the next, the last filling.
fn nested(n: usize) -> Vec<(String, Vec<u8>)> {
    (0..n)
        .map(|i| {
            let content = if i + 1 == n {
                "0 0 10 10 re f".to_string()
            } else {
                "/F Do".to_string()
            };
            form(
                &format!(
                    "/BBox [0 0 100 100] /Resources << /XObject << /F {} 0 R >> >>",
                    i + 6
                ),
                &content,
            )
        })
        .collect()
}

#[test]
fn forms_nest_eight_deep_and_not_nine() {
    let c = draw("0 0 20 20", "/X1 Do", &nested(8));
    assert_eq!(c, plain("0 0 20 20", "0 0 10 10 re f"));
    let e = error("0 0 20 20", "/X1 Do", &nested(9));
    assert!(e.contains("forms nested deeper than 8"), "{e}");
    // A form that places itself stops with the same error.
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            "/BBox [0 0 100 100] /Resources << /XObject << /F 5 0 R >> >>",
            "0 0 1 1 re f /F Do",
        )],
    );
    assert!(e.contains("forms nested deeper than 8"), "{e}");
    // The check finds the error without drawing.
    use crate::pdf::Document;
    let data = pdf_with_streams("0 0 20 20", "/X1 Do", &nested(9));
    let doc = Document::parse(&data).unwrap();
    let pages = doc.pages().unwrap();
    let e = super::check_page(&doc, &pages[0], 72)
        .unwrap_err()
        .to_string();
    assert!(e.contains("forms nested deeper than 8"), "{e}");
}

#[test]
fn text_and_groups_in_a_form_are_errors() {
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] {NO_RESOURCES}"),
            "BT ET",
        )],
    );
    assert!(e.contains("text in a form"), "{e}");
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] /Group << /S /Transparency >> {NO_RESOURCES}"),
            "",
        )],
    );
    assert!(e.contains("unsupported Group"), "{e}");
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] /Ref << /F (a.pdf) >> {NO_RESOURCES}"),
            "",
        )],
    );
    assert!(e.contains("unsupported Ref"), "{e}");
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] /OC 6 0 R {NO_RESOURCES}"),
            "",
        )],
    );
    assert!(e.contains("unsupported OC"), "{e}");
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] /FormType 2 {NO_RESOURCES}"),
            "",
        )],
    );
    assert!(e.contains("unsupported FormType 2"), "{e}");
    let c = draw(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] /FormType 1 {NO_RESOURCES}"),
            "0 0 10 10 re f",
        )],
    );
    assert_eq!(c, plain("0 0 20 20", "0 0 10 10 re f"));
}

#[test]
fn a_font_selection_outside_bt_is_allowed_in_a_form() {
    // docs/adr/0029: only BT is an error; Tz and Tr outside a text object
    // are read as they are on a page, and the form draws on.
    let content = "100 Tz 0 Tr 0 0 10 10 re f";
    let c = draw(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] {NO_RESOURCES}"),
            content,
        )],
    );
    assert_eq!(c, plain("0 0 20 20", content));
    assert_eq!(c.pixel(0, 19), [0, 0, 0]);
    assert_eq!(c.pixel(15, 0), [255, 255, 255]);
    // Tf is read too: it gets as far as looking up the font, which this
    // form does not have, and the error is the one of the page, not
    // "text in a form" or an unsupported operator.
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] {NO_RESOURCES}"),
            "/F1 12 Tf",
        )],
    );
    assert!(e.ends_with("offset 7: no font F1 in the resources"), "{e}");
}

#[test]
fn the_errors_of_a_form_name_its_object_and_offset() {
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] {NO_RESOURCES}"),
            "0 0 10 10 re f 1 0 0 RG",
        )],
    );
    assert!(
        e.contains("page 1: object 5: offset 21: unsupported operator RG"),
        "{e}"
    );
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(&format!("/BBox [0 0 100 100] {NO_RESOURCES}"), "q")],
    );
    assert!(e.contains("q not closed"), "{e}");
}

#[test]
fn a_form_whose_content_is_corrupt_is_an_error() {
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[(
            format!(
                "/Type /XObject /Subtype /Form /Filter /FlateDecode /BBox [0 0 100 100] {NO_RESOURCES}"
            ),
            b"xyz".to_vec(),
        )],
    );
    assert!(!e.is_empty());
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(&format!("/BBox [0 0 100] {NO_RESOURCES}"), "")],
    );
    assert!(e.contains("rectangle"), "{e}");
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [0 0 100 100] /Matrix [1 0 0] {NO_RESOURCES}"),
            "",
        )],
    );
    assert!(e.contains("malformed Matrix"), "{e}");
    let e = error("0 0 20 20", "/X1 Do", &[form(NO_RESOURCES, "")]);
    assert!(e.contains("BBox"), "{e}");
    let e = error(
        "0 0 20 20",
        "/X1 Do",
        &[("/Type /XObject /Subtype /PS".into(), vec![])],
    );
    assert!(e.contains("unsupported XObject Subtype PS"), "{e}");
    let e = render_pdf(&page_pdf("0 0 20 20", "", "/X1 Do", &[]), 72)
        .unwrap_err()
        .to_string();
    assert!(e.contains("no XObject X1"), "{e}");
}

#[test]
fn corrupted_forms_never_panic() {
    // Every byte of the content of a form (with a clip, a stroke, a
    // nested form and an image) overwritten in turn, and every
    // truncation: an image or an error, never a panic.
    let content = b"q 0 0 50 50 re W n 1 0 0 1 2 2 cm 0.5 w 0 0 m 40 40 l S /F Do /I Do Q 0 0 1 0 k 5 5 10 10 re f";
    let mut state = 0x0027_4321u32;
    let mut next = || {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (state >> 16) as u8
    };
    let render = |content: &[u8]| {
        let pdf = pdf_with_streams(
            "0 0 20 20",
            "/X1 Do",
            &[
                (
                    "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /XObject << /F 6 0 R /I 7 0 R >> >>".into(),
                    content.to_vec(),
                ),
                form(
                    "/BBox [0 0 100 100] /Resources << /XObject << /I 7 0 R >> >>",
                    "q 10 0 0 10 0 0 cm /I Do Q",
                ),
                (
                    "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8".into(),
                    vec![0u8],
                ),
            ],
        );
        let _ = render_pdf(&pdf, 72);
    };
    render(content);
    (0..content.len()).for_each(|i| {
        let mut bad = content.to_vec();
        bad[i] = next();
        render(&bad);
        bad[i] = b' ';
        render(&bad);
        render(&content[..i]);
    });
}

#[test]
fn a_degenerate_bbox_draws_nothing() {
    let c = draw(
        "0 0 20 20",
        "/X1 Do",
        &[form(
            &format!("/BBox [5 5 5 15] {NO_RESOURCES}"),
            "0 0 100 100 re f",
        )],
    );
    assert!(c.pixels.iter().all(|&v| v == 255));
}
