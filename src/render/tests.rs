use super::*;
use crate::fixed::{Fixed, Matrix};
use crate::pdf::Document;

fn n(s: &str) -> Fixed {
    Fixed::parse(s.as_bytes()).unwrap()
}

/// A one-page PDF with the given MediaBox, resources and content.
pub(crate) fn page_pdf(media_box: &str, resources: &str, content: &str, extra: &[&str]) -> Vec<u8> {
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [{media_box}] /Resources << {resources} >> /Contents 4 0 R >>"
    );
    let content = format!(
        "<< /Length {} >>\nstream\n{}\nendstream",
        content.len(),
        content
    );
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        page,
        content,
    ];
    objects.extend(extra.iter().map(|s| s.to_string()));
    let refs: Vec<&str> = objects.iter().map(String::as_str).collect();
    crate::pdf::tests::build_pdf(&refs, "")
}

pub(crate) fn render_pdf(data: &[u8], dpi: u32) -> crate::error::Result<Canvas> {
    let doc = Document::parse(data)?;
    let pages = doc.pages()?;
    render_page(&doc, &pages[0], dpi)
}

#[test]
fn page_sizes_round_up() {
    let size = |w: &str, h: &str, dpi| page_size(&[n("0"), n("0"), n(w), n(h)], dpi);
    assert_eq!(size("362.8346", "515.9055", 150).unwrap(), (756, 1075));
    assert_eq!(size("72", "72", 72).unwrap(), (72, 72));
    assert_eq!(size("72", "36", 144).unwrap(), (144, 72));
    assert!(size("0", "72", 72).is_err());
    assert!(size("100000", "100000", 1200).is_err());
}

#[test]
fn the_page_is_white() {
    let canvas = render_pdf(&page_pdf("0 0 10 10", "", "", &[]), 72).unwrap();
    assert!(canvas.pixels.iter().all(|&v| v == 255));
}

#[test]
fn y_goes_up_in_user_space() {
    let canvas = render_pdf(&page_pdf("0 0 72 72", "", "0 0 36 36 re f", &[]), 72).unwrap();
    assert_eq!(canvas.pixel(0, 71), [0, 0, 0]);
    assert_eq!(canvas.pixel(35, 36), [0, 0, 0]);
    assert_eq!(canvas.pixel(36, 36), [255, 255, 255]);
    assert_eq!(canvas.pixel(0, 35), [255, 255, 255]);
}

#[test]
fn the_media_box_origin_is_the_corner() {
    let canvas = render_pdf(
        &page_pdf("100 200 172 272", "", "100 200 10 10 re f", &[]),
        72,
    )
    .unwrap();
    assert_eq!(canvas.pixel(0, 71), [0, 0, 0]);
    assert_eq!(canvas.pixel(10, 71), [255, 255, 255]);
}

#[test]
fn cm_and_the_graphics_state_stack() {
    let content = "q 1 0 0 1 10 10 cm 0 0 2 2 re f Q 0 0 2 2 re f";
    let canvas = render_pdf(&page_pdf("0 0 20 20", "", content, &[]), 72).unwrap();
    assert_eq!(canvas.pixel(10, 9), [0, 0, 0]);
    assert_eq!(canvas.pixel(0, 19), [0, 0, 0]);
    assert_eq!(canvas.pixel(5, 9), [255, 255, 255]);
}

#[test]
fn curves_and_close_path() {
    // A disc of radius 5 at (10, 10), as Golem writes it (control points at 0.5523)
    let content = "15 10 m 15 12.7615 12.7615 15 10 15 c 7.2385 15 5 12.7615 5 10 c \
                   5 7.2385 7.2385 5 10 5 c 12.7615 5 15 7.2385 15 10 c h f";
    let canvas = render_pdf(&page_pdf("0 0 20 20", "", content, &[]), 72).unwrap();
    assert_eq!(canvas.pixel(10, 10), [0, 0, 0]);
    assert_eq!(canvas.pixel(1, 1), [255, 255, 255]);
    // a corner of the bounding square is outside the disc
    assert_eq!(canvas.pixel(5, 5), [255, 255, 255]);
}

#[test]
fn stroke_widths() {
    let d = Matrix::new(["2", "0", "0", "-2", "0", "100"].map(n));
    // 1 point at 144 dpi is 2 pixels: half is 256 units
    assert_eq!(stroke_half_width(&d, n("1")).unwrap(), 256);
    // a line width of 0 is 1 pixel
    assert_eq!(stroke_half_width(&d, n("0")).unwrap(), 128);
    let rotated = Matrix::new(["0", "-2", "2", "0", "0", "0"].map(n));
    assert_eq!(stroke_half_width(&rotated, n("1")).unwrap(), 256);
    let stretched = Matrix::new(["4", "0", "0", "-2", "0", "0"].map(n));
    assert!(
        stroke_half_width(&stretched, n("1"))
            .unwrap_err()
            .to_string()
            .contains("unsupported stroke")
    );
}
