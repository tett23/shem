//! The limits on the expansion of forms, the nesting of q and of clips
//! (docs/adr/0030).

use super::image_tests::pdf_with_streams;
use super::tests::{page_pdf, render_pdf};
use crate::pdf::Document;

/// A form that places the form `next` (an object number) `times` times,
/// or fills a small square when it is the last.
fn form(next: Option<usize>, times: usize) -> (String, Vec<u8>) {
    match next {
        Some(n) => (
            format!(
                "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /XObject << /F {n} 0 R >> >>"
            ),
            "/F Do ".repeat(times).into_bytes(),
        ),
        None => (
            "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << >>".to_string(),
            b"0 0 1 1 re f".to_vec(),
        ),
    }
}

/// Forms 5 to 5 + levels − 1, each placing the next `times` times.
fn tree(levels: usize, times: usize) -> Vec<(String, Vec<u8>)> {
    (0..levels)
        .map(|i| form((i + 1 < levels).then_some(i + 6), times))
        .collect()
}

/// The error of the check of the first page, without drawing.
fn check_error(data: &[u8]) -> String {
    let doc = Document::parse(data).unwrap();
    let pages = doc.pages().unwrap();
    super::check_page(&doc, &pages[0], 72)
        .unwrap_err()
        .to_string()
}

#[test]
fn a_page_places_at_most_4096_forms() {
    // 1 + 4095 places of one leaf: 4096 Do.
    let at = pdf_with_streams("0 0 20 20", "/X1 Do", &tree(2, 4095));
    assert!(render_pdf(&at, 72).is_ok());
    let past = pdf_with_streams("0 0 20 20", "/X1 Do", &tree(2, 4096));
    let e = render_pdf(&past, 72).unwrap_err().to_string();
    assert!(e.contains("more than 4096 forms in a page"), "{e}");
    assert!(check_error(&past).contains("more than 4096 forms in a page"));
}

#[test]
fn a_tree_of_forms_stops_at_the_limit() {
    // Eight levels, each placing the next eight times: 8^7 leaves.
    let data = pdf_with_streams("0 0 20 20", "/X1 Do", &tree(8, 8));
    let start = std::time::Instant::now();
    let e = check_error(&data);
    assert!(e.contains("more than 4096 forms in a page"), "{e}");
    // A loose bound: before the limit the check took 19.7 s.
    assert!(start.elapsed().as_secs() < 5);
}

#[test]
fn the_contents_of_forms_in_a_page_are_at_most_2_26_bytes() {
    // A form of 2^16 bytes placed 1025 times: 2^26 + 2^16 bytes.
    let content = format!("{}0 0 1 1 re f", " ".repeat((1 << 16) - 12));
    assert_eq!(content.len(), 1 << 16);
    let leaf = (
        "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << >>".to_string(),
        content.into_bytes(),
    );
    let at = pdf_with_streams("0 0 20 20", &"/X1 Do ".repeat(1024), &[leaf.clone()]);
    assert!(render_pdf(&at, 72).is_ok());
    let past = pdf_with_streams("0 0 20 20", &"/X1 Do ".repeat(1025), &[leaf]);
    let e = check_error(&past);
    assert!(
        e.contains("form contents larger than 67108864 bytes in a page"),
        "{e}"
    );
}

#[test]
fn q_nests_at_most_64_deep() {
    let at = format!("{}{}", "q ".repeat(64), "Q ".repeat(64));
    assert!(render_pdf(&page_pdf("0 0 10 10", "", &at, &[]), 72).is_ok());
    let past = format!("{}{}", "q ".repeat(65), "Q ".repeat(65));
    let e = render_pdf(&page_pdf("0 0 10 10", "", &past, &[]), 72)
        .unwrap_err()
        .to_string();
    assert!(e.contains("q nested deeper than 64"), "{e}");
}

#[test]
fn a_form_counts_as_a_q() {
    // 63 q and the q of the form: 64; one more q is past the limit.
    let leaf = form(None, 0);
    let at = format!("{}/X1 Do{}", "q ".repeat(63), " Q".repeat(63));
    assert!(render_pdf(&pdf_with_streams("0 0 10 10", &at, &[leaf.clone()]), 72).is_ok());
    let past = format!("{}/X1 Do{}", "q ".repeat(64), " Q".repeat(64));
    let e = render_pdf(&pdf_with_streams("0 0 10 10", &past, &[leaf]), 72)
        .unwrap_err()
        .to_string();
    assert!(e.contains("q nested deeper than 64"), "{e}");
}

#[test]
fn at_most_16_clips_are_nested() {
    let clip = "q 0 0 10 10 re W n ";
    let at = format!("{}0 0 5 5 re f{}", clip.repeat(16), " Q".repeat(16));
    assert!(render_pdf(&page_pdf("0 0 10 10", "", &at, &[]), 72).is_ok());
    let past = format!("{}0 0 5 5 re f{}", clip.repeat(17), " Q".repeat(17));
    let data = page_pdf("0 0 10 10", "", &past, &[]);
    let e = render_pdf(&data, 72).unwrap_err().to_string();
    assert!(e.contains("clips nested deeper than 16"), "{e}");
    // The check finds it without drawing a mask.
    assert!(check_error(&data).contains("clips nested deeper than 16"));
    // Clips in one state count too: two W in one q are two clips.
    let two = format!("{}{}", "q 0 0 10 10 re W n 0 0 10 10 re W n ".repeat(9), "Q ".repeat(9));
    let e = render_pdf(&page_pdf("0 0 10 10", "", &two, &[]), 72)
        .unwrap_err()
        .to_string();
    assert!(e.contains("clips nested deeper than 16"), "{e}");
}

#[test]
fn the_bbox_of_a_form_is_a_clip() {
    // 15 clips and the BBox of the form: 16; one more clip is past it.
    let leaf = form(None, 0);
    let clip = "q 0 0 10 10 re W n ";
    let at = format!("{}/X1 Do{}", clip.repeat(15), " Q".repeat(15));
    assert!(render_pdf(&pdf_with_streams("0 0 10 10", &at, &[leaf.clone()]), 72).is_ok());
    let past = format!("{}/X1 Do{}", clip.repeat(16), " Q".repeat(16));
    let e = render_pdf(&pdf_with_streams("0 0 10 10", &past, &[leaf]), 72)
        .unwrap_err()
        .to_string();
    assert!(e.contains("clips nested deeper than 16"), "{e}");
}

#[test]
fn clips_freed_by_q_do_not_count() {
    // Many clips, one at a time.
    let content = "q 0 0 10 10 re W n 0 0 5 5 re f Q ".repeat(100);
    assert!(render_pdf(&page_pdf("0 0 10 10", "", &content, &[]), 72).is_ok());
}
