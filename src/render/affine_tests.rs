//! The strokes under a CTM that does not keep angles: built in user space
//! and mapped to the device (docs/adr/0027).

use super::tests::{page_pdf, render_pdf};
use super::*;

fn render(media_box: &str, content: &str) -> Canvas {
    render_pdf(&page_pdf(media_box, "", content, &[]), 72).unwrap()
}

/// The message of the error of a page, and the offset of its operator.
fn error(media_box: &str, content: &str) -> (String, Option<usize>) {
    let e = render_pdf(&page_pdf(media_box, "", content, &[]), 72).unwrap_err();
    (e.message().to_string(), e.offset())
}

fn at_s(content: &str) -> Option<usize> {
    content.rfind('S')
}

fn row(canvas: &Canvas, y: usize) -> Vec<u8> {
    (0..canvas.width).map(|x| canvas.pixel(x, y)[0]).collect()
}

fn column(canvas: &Canvas, x: usize) -> Vec<u8> {
    (0..canvas.height).map(|y| canvas.pixel(x, y)[0]).collect()
}

fn dark(v: u8) -> bool {
    v < 128
}

fn white(v: u8) -> bool {
    v == 255
}

/// A circle of radius 10 around (15, 20), as Golem writes one.
const CIRCLE: &str = "25 20 m 25 25.523 20.523 30 15 30 c 9.477 30 5 25.523 5 20 c \
                      5 14.477 9.477 10 15 10 c 20.523 10 25 14.477 25 20 c h";

#[test]
fn a_circle_under_a_stretch_is_twice_as_thick_across() {
    // 2 0 0 1 0 0 cm: the line of width 2 is 4 pixels thick where the
    // circle is vertical and 2 where it is horizontal.
    let canvas = render("0 0 60 40", &format!("2 0 0 1 0 0 cm 2 w {CIRCLE} S"));
    let across = row(&canvas, 20);
    let dark_across: Vec<usize> = (0..30).filter(|&x| dark(across[x])).collect();
    assert_eq!(dark_across, vec![8, 9, 10, 11], "{across:?}");
    let down = column(&canvas, 30);
    let dark_down: Vec<usize> = (0..20).filter(|&y| dark(down[y])).collect();
    assert_eq!(dark_down, vec![9, 10], "{down:?}");
}

#[test]
fn round_caps_under_a_stretch_are_ellipses() {
    // 2 0 0 1 0 0 cm, 1 J, 4 w: the disc of radius 2 at the end (20, 10)
    // of user space is an ellipse of half axes 4 and 2 at x = 40 of the
    // device.
    let canvas = render("0 0 60 20", "2 0 0 1 0 0 cm 1 J 4 w 10 10 m 20 10 l S");
    let middle = row(&canvas, 9);
    assert!(dark(middle[16]) && white(middle[15]), "{middle:?}");
    assert!(dark(middle[43]) && white(middle[44]), "{middle:?}");
    let top = row(&canvas, 7);
    assert!(top.iter().all(|&v| white(v)), "{top:?}");
    let column = column(&canvas, 42);
    assert!(dark(column[9]) && dark(column[10]), "{column:?}");
    assert!(white(column[7]) && white(column[12]), "{column:?}");
}

/// Whether a pixel of `canvas` is on an edge of the strokes: it or one of
/// its neighbours is partly covered, or it and a neighbour are on
/// different sides.
fn on_an_edge(canvas: &Canvas, x: usize, y: usize) -> bool {
    let around: Vec<u8> = [(0i64, 0i64), (-1, 0), (1, 0), (0, -1), (0, 1)]
        .iter()
        .filter_map(|&(dx, dy)| {
            let (px, py) = (x as i64 + dx, y as i64 + dy);
            (px >= 0 && py >= 0 && px < canvas.width as i64 && py < canvas.height as i64)
                .then(|| canvas.pixel(px as usize, py as usize)[0])
        })
        .collect();
    around.iter().any(|&v| v != 0 && v != 255) || (around.contains(&0) && around.contains(&255))
}

#[test]
fn a_ctm_scaled_by_a_hair_differs_only_at_the_edges() {
    // The CTM 1.0000001 0 0 1 0 0 does not keep angles, so the stroke
    // takes the general path; its pixels differ from those of the
    // angle-keeping path only where the strokes have an edge.
    let path =
        "6 w 1 j 10 10 m 30 60 50 10 70 60 c 90 30 l 40 20 l S 1 J [5 3] 0 d 20 70 m 80 75 l S";
    let kept = render("0 0 100 80", path);
    let hair = render("0 0 100 80", &format!("1.0000001 0 0 1 0 0 cm {path}"));
    assert!(kept.pixels.contains(&0));
    let mut differences = 0;
    (0..kept.height).for_each(|y| {
        (0..kept.width).for_each(|x| {
            if kept.pixel(x, y) != hair.pixel(x, y) {
                differences += 1;
                assert!(
                    on_an_edge(&kept, x, y),
                    "({x}, {y}): {:?} and {:?}",
                    kept.pixel(x, y),
                    hair.pixel(x, y)
                );
            }
        });
    });
    // Both ways of the test draw something near each other.
    assert!(differences < kept.width * kept.height / 10, "{differences}");
}

#[test]
fn a_skew_draws_a_slanted_line() {
    // 1 0 0.5 1 0 0 cm: the vertical line from (10, 10) to (10, 30) of
    // user space goes from (15, 10) to (25, 30), that is from (15, 30) to
    // (25, 10) of the device.
    let canvas = render("0 0 40 40", "1 0 0.5 1 0 0 cm 2 w 10 10 m 10 30 l S");
    assert!(dark(canvas.pixel(15, 28)[0]));
    assert!(dark(canvas.pixel(24, 12)[0]));
    assert!(white(canvas.pixel(10, 28)[0]));
    assert!(white(canvas.pixel(15, 12)[0]));
}

#[test]
fn dashes_under_a_stretch_are_measured_in_user_space() {
    // [3 2] on a line of 10 points under 2 0 0 1 0 0 cm: dashes of 6
    // pixels and gaps of 4.
    let canvas = render("0 0 20 10", "2 0 0 1 0 0 cm [3 2] 0 d 2 w 0 5 m 10 5 l S");
    let dashes: String = row(&canvas, 4)
        .iter()
        .map(|&v| if dark(v) { '#' } else { '.' })
        .collect();
    assert_eq!(dashes, "######....######....");
}

#[test]
fn a_zero_width_line_under_a_general_ctm_is_an_error() {
    let content = "2 0 0 1 0 0 cm 0 w 0 5 m 5 5 l S";
    assert_eq!(
        error("0 0 10 10", content),
        (
            "zero-width stroke with a CTM that does not keep angles".to_string(),
            at_s(content)
        )
    );
    // The check finds it without drawing.
    let doc = Document::parse(&page_pdf("0 0 10 10", "", content, &[])).unwrap();
    let e = check_page(&doc, &doc.pages().unwrap()[0], 72).unwrap_err();
    assert_eq!(e.offset(), at_s(content));
    // A zero-width line under a CTM that keeps angles is one pixel.
    render("0 0 10 10", "2 0 0 2 0 0 cm 0 w 0 2 m 5 2 l S");
}

#[test]
fn a_general_stroke_beyond_the_range_of_coordinates_is_an_error() {
    // At 72 dpi, i32::MAX is 8388607.996 pixels; under 2 0 0 1 0 0 cm the
    // vertical line at 4194303.5 is at 8388607 pixels, within it, but the
    // corners of its stroke, 1 point right of it in user space, are at
    // 8388609 pixels, past it.
    let content = "2 0 0 1 0 0 cm 2 w 4194303.5 2 m 4194303.5 8 l S";
    assert_eq!(
        error("0 0 40 40", content),
        (
            "stroke outside the range of coordinates".to_string(),
            at_s(content)
        )
    );
    let doc = Document::parse(&page_pdf("0 0 40 40", "", content, &[])).unwrap();
    let e = check_page(&doc, &doc.pages().unwrap()[0], 72).unwrap_err();
    assert_eq!(
        (e.message().to_string(), e.offset()),
        (
            "stroke outside the range of coordinates".to_string(),
            at_s(content)
        )
    );
    render("0 0 40 40", "2 0 0 1 0 0 cm 2 w 4194302 2 m 4194302 8 l S");
}

#[test]
fn a_user_point_of_2_30_points_is_beyond_the_range_though_the_device_point_is_not() {
    // docs/adr/0028: the points of user space are in units of 2^−32, and
    // their differences must fit in i64, so a point of 2^30 points or more
    // is an error. The CTM shrinks x by 1000, so the device point
    // (about 1.07e6 pixels) is far within the range of i32 and the error is
    // not the one of the mapping (src/render.rs, USER_REACH).
    let beyond = "0.001 0 0 0.5 0 0 cm 2 w 1073741824 5 m 1073741824 8 l S";
    assert_eq!(
        error("0 0 40 40", beyond),
        (
            "stroke outside the range of coordinates".to_string(),
            at_s(beyond)
        )
    );
    // The same for y and for a negative point, and for the end of a curve.
    for content in [
        "0.5 0 0 0.001 0 0 cm 2 w 5 5 m 5 -1073741825 l S",
        "0.001 0 0 0.5 0 0 cm 2 w -1073741824 5 m 5 5 l S",
        "0.001 0 0 0.5 0 0 cm 2 w 5 5 m 5 5 6 6 1073741824 8 c S",
    ] {
        assert_eq!(
            error("0 0 40 40", content),
            (
                "stroke outside the range of coordinates".to_string(),
                at_s(content)
            ),
            "{content}"
        );
    }
    // One point below 2^30 passes the check: it draws, off the page.
    render(
        "0 0 40 40",
        "0.001 0 0 0.5 0 0 cm 2 w 1073741823 5 m 1073741823 8 l S",
    );
    // The check finds it without drawing.
    let doc = Document::parse(&page_pdf("0 0 40 40", "", beyond, &[])).unwrap();
    let e = check_page(&doc, &doc.pages().unwrap()[0], 72).unwrap_err();
    assert_eq!(e.message(), "stroke outside the range of coordinates");
}

#[test]
fn a_dash_period_too_short_for_the_stretch_of_the_ctm_is_an_error() {
    // docs/adr/0028: 2 0 0 1 0 0 cm stretches the most by 2.24 (the bound
    // sqrt(a² + b² + c² + d²)), so a period of 0.002 is at most 0.0045
    // pixels, below 1/16.
    let short = "2 0 0 1 0 0 cm [0.001 0.001] 0 d 1 w 0 5 m 10 5 l S";
    assert_eq!(
        error("0 0 20 10", short),
        (
            "dash pattern shorter than 1/16 pixel".to_string(),
            at_s(short)
        )
    );
    let doc = Document::parse(&page_pdf("0 0 20 10", "", short, &[])).unwrap();
    let e = check_page(&doc, &doc.pages().unwrap()[0], 72).unwrap_err();
    assert_eq!(
        (e.message().to_string(), e.offset()),
        (
            "dash pattern shorter than 1/16 pixel".to_string(),
            at_s(short)
        )
    );
    // An odd array counts twice; a period of 0.1 · 2.24 is long enough.
    render(
        "0 0 20 10",
        "2 0 0 1 0 0 cm [0.05 0.05] 0 d 1 w 0 5 m 10 5 l S",
    );
    render("0 0 20 10", "2 0 0 1 0 0 cm [0.05] 0 d 1 w 0 5 m 10 5 l S");
}

#[test]
fn the_general_stroke_of_a_scaled_path_is_the_angle_keeping_stroke_of_the_scaled_points() {
    // A line of width 4 from (5, 10) to (15, 10) with square caps under
    // 2 0 0 1 0 0 cm is the rectangle from (3, 8) to (17, 12) of user
    // space, which the CTM maps to the one from (6, 8) to (34, 12).
    let general = render("0 0 40 40", "2 0 0 1 0 0 cm 2 J 4 w 5 10 m 15 10 l S");
    let by_hand = render("0 0 40 40", "6 8 28 4 re f");
    assert_eq!(general, by_hand);
}

#[test]
fn b_strokes_under_a_general_ctm() {
    // B fills, then strokes, both under the general CTM.
    let canvas = render(
        "0 0 40 20",
        "2 0 0 1 0 0 cm 0 0 1 0 k 1 0 0 0 K 2 w 5 5 10 10 re B",
    );
    assert_eq!(canvas.pixel(20, 10), [255, 255, 0]);
    assert_eq!(canvas.pixel(10, 10), [0, 255, 255]);
    assert_eq!(canvas.pixel(20, 5), [0, 255, 255]);
    assert_eq!(canvas.pixel(2, 10), [255, 255, 255]);
}
