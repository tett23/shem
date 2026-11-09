//! The strokes of the paths of a page: `l` and `S`, round caps (`J`) and
//! dashes (`d`) (docs/adr/0025).

use super::tests::{page_pdf, render_pdf};
use super::*;

fn render(media_box: &str, content: &str, dpi: u32) -> Canvas {
    render_pdf(&page_pdf(media_box, "", content, &[]), dpi).unwrap()
}

/// The message of the error of a page, and the offset of its operator.
fn error(media_box: &str, content: &str, dpi: u32) -> (String, Option<usize>) {
    let e = render_pdf(&page_pdf(media_box, "", content, &[]), dpi).unwrap_err();
    (e.message().to_string(), e.offset())
}

/// The offset of the `S` of a content.
fn at_s(content: &str) -> Option<usize> {
    content.rfind('S')
}

/// The red channel of the pixels of a row, left to right.
fn row(canvas: &Canvas, y: usize) -> Vec<u8> {
    (0..canvas.width).map(|x| canvas.pixel(x, y)[0]).collect()
}

fn dark(v: u8) -> bool {
    v < 128
}

fn white(v: u8) -> bool {
    v == 255
}

#[test]
fn the_strokes_that_golem_writes_render() {
    // The three outputs of the test of Golem's Writer (docs/adr/0025).
    for content in [
        "0.5 w\n0 0 m\n1 0 l\nS\n",
        "1 w [3 2] 0 d\n0 0 m\n1 0 l\n1 1 l\nh S\n",
        "1 w 1 J [0 3] 0 d\n0 0 m\n1 0 l\nS\n",
    ] {
        let canvas = render("0 0 10 10", content, 72);
        assert!(canvas.pixels.iter().any(|&v| v != 255), "{content}");
    }
}

#[test]
fn a_round_cap_is_a_half_disc() {
    // A line of width 4 from (3, 5) to (7, 5) on a 10 × 10 page at 72 dpi:
    // with butt caps, it starts at x = 3; with round caps, a half disc of
    // radius 2 reaches x = 1, so the pixel 1 left of the end is drawn and
    // the pixel 2 left of it is not.
    let butt = row(&render("0 0 10 10", "4 w 3 5 m 7 5 l S", 72), 4);
    assert!(butt[..3].iter().all(|&v| white(v)), "{butt:?}");
    assert!(butt[3..7].iter().all(|&v| v == 0), "{butt:?}");
    assert!(butt[7..].iter().all(|&v| white(v)), "{butt:?}");
    let round = row(&render("0 0 10 10", "1 J 4 w 3 5 m 7 5 l S", 72), 4);
    assert!(white(round[0]), "{round:?}");
    assert!(dark(round[1]), "{round:?}");
    assert!(round[2..8].iter().all(|&v| v == 0), "{round:?}");
    assert!(dark(round[8]), "{round:?}");
    assert!(white(round[9]), "{round:?}");
}

#[test]
fn q_and_q_restore_the_cap_and_the_dash() {
    let plain = render("0 0 10 10", "4 w 3 5 m 7 5 l S", 72);
    let restored = render("0 0 10 10", "q 1 J [3 2] 0 d Q 4 w 3 5 m 7 5 l S", 72);
    assert_eq!(restored, plain);
    let inside = render("0 0 10 10", "q 1 J [3 2] 0 d 4 w 3 5 m 7 5 l S Q", 72);
    assert_ne!(inside, plain);
}

#[test]
fn subpaths_of_no_length() {
    // m h and m l to the same point draw a disc with round caps and
    // nothing with butt caps; m alone draws nothing with either.
    let disc = render("0 0 10 10", "1 J 4 w 5 5 m h S", 72);
    assert_eq!(disc.pixel(4, 4), [0, 0, 0]);
    assert_eq!(disc.pixel(5, 5), [0, 0, 0]);
    assert_eq!(disc.pixel(1, 1), [255, 255, 255]);
    assert_eq!(render("0 0 10 10", "1 J 4 w 5 5 m 5 5 l S", 72), disc);
    assert_eq!(render("0 0 10 10", "1 J 4 w 5 5 m 5 5 l h S", 72), disc);
    for content in [
        "0 J 4 w 5 5 m h S",
        "4 w 5 5 m 5 5 l S",
        "1 J 4 w 5 5 m S",
        "4 w 5 5 m S",
    ] {
        let canvas = render("0 0 10 10", content, 72);
        assert!(canvas.pixels.iter().all(|&v| v == 255), "{content}");
    }
}

/// The pixels of the row through a horizontal line of width 2 on y = 5 of
/// a 10 × 10 page, dark or white, as a string of `#` and `.`.
fn dashes_of(content: &str) -> String {
    row(&render("0 0 10 10", content, 72), 4)
        .iter()
        .map(|&v| if dark(v) { '#' } else { '.' })
        .collect()
}

#[test]
fn a_dash_array_cuts_a_line() {
    // [3 2] on a line of 10 pixels: [0, 3] and [5, 8]; with the phase 1:
    // [0, 2], [4, 7] and [9, 10].
    assert_eq!(dashes_of("[3 2] 0 d 2 w 0 5 m 10 5 l S"), "###..###..");
    assert_eq!(dashes_of("[3 2] 1 d 2 w 0 5 m 10 5 l S"), "##..###..#");
    assert_eq!(dashes_of("[] 0 d 2 w 0 5 m 10 5 l S"), "##########");
}

#[test]
fn an_odd_dash_array_is_doubled() {
    assert_eq!(
        render("0 0 10 10", "[3] 0 d 2 w 0 5 m 10 5 l S", 72),
        render("0 0 10 10", "[3 3] 0 d 2 w 0 5 m 10 5 l S", 72)
    );
    assert_eq!(dashes_of("[3] 0 d 2 w 0 5 m 10 5 l S"), "###...###.");
}

#[test]
fn dots_are_discs() {
    // Round caps on dashes of length 0 every 4 pixels from x = 1: discs of
    // radius 1 at 1, 5 and 9.
    assert_eq!(dashes_of("1 J [0 4] 0 d 2 w 1 5 m 10 5 l S"), "##..##..##");
    // Without round caps, nothing.
    assert_eq!(dashes_of("[0 4] 0 d 2 w 1 5 m 10 5 l S"), "..........");
}

#[test]
fn a_dash_across_a_vertex_has_a_join() {
    // An L from (2, 2) up to (2, 8) and right to (8, 8), 2 wide: the
    // outer corner pixel is drawn by the miter when a dash runs across
    // the vertex, and not when a dash starts at the vertex with a butt
    // cap.
    let across = render("0 0 10 10", "[9 9] 0 d 2 w 2 2 m 2 8 l 8 8 l S", 72);
    assert_eq!(across.pixel(1, 1), [0, 0, 0]);
    let solid = render("0 0 10 10", "2 w 2 2 m 2 8 l 8 8 l S", 72);
    assert_eq!(solid.pixel(1, 1), [0, 0, 0]);
    let from_vertex = render("0 0 10 10", "[4 2] 0 d 2 w 2 2 m 2 8 l 8 8 l S", 72);
    assert_eq!(from_vertex.pixel(1, 1), [255, 255, 255]);
    assert_eq!(from_vertex.pixel(3, 1), [0, 0, 0]);
}

#[test]
fn every_subpath_starts_the_dash_array_again() {
    let canvas = render("0 0 10 10", "[3 2] 1 d 2 w 0 5 m 10 5 l 0 2 m 10 2 l S", 72);
    assert_eq!(row(&canvas, 4), row(&canvas, 7));
    assert_eq!(
        row(&canvas, 4)
            .iter()
            .map(|&v| if dark(v) { '#' } else { '.' })
            .collect::<String>(),
        "##..###..#"
    );
}

#[test]
fn a_closed_subpath_is_dashed_to_its_start_without_a_join() {
    // A square of sides 6 from (2, 2), 2 wide: the dash array [6 12 6 0]
    // draws the first and the last side. Solid, the corner at the start
    // is mitered; dashed, the two dashes meet there with butt caps, and
    // the outer corner pixel stays white. The last side is dashed too:
    // with [12 12], it is not drawn.
    let solid = render("0 0 10 10", "2 w 2 2 m 8 2 l 8 8 l 2 8 l h S", 72);
    assert!(dark(solid.pixel(1, 8)[0]));
    let dashed = render(
        "0 0 10 10",
        "[6 12 6 0] 0 d 2 w 2 2 m 8 2 l 8 8 l 2 8 l h S",
        72,
    );
    assert!(dark(dashed.pixel(2, 4)[0]));
    assert!(dark(dashed.pixel(4, 7)[0]));
    assert!(white(dashed.pixel(8, 4)[0]));
    assert!(white(dashed.pixel(1, 8)[0]));
    let half = render(
        "0 0 10 10",
        "[12 12] 0 d 2 w 2 2 m 8 2 l 8 8 l 2 8 l h S",
        72,
    );
    assert!(dark(half.pixel(4, 7)[0]));
    assert!(dark(half.pixel(8, 4)[0]));
    assert!(white(half.pixel(2, 4)[0]));
}

#[test]
fn a_dash_period_of_a_sixteenth_of_a_pixel_is_the_least() {
    // At 72 dpi, [0.03125 0.03125] is 1/16 pixel; 2^−24 pixel less (one
    // unit of the lengths of the dashes) is an error. An odd array is
    // doubled, so [0.03125] is 1/16 pixel too, and an element one unit
    // shorter makes the period two units short. At 300 dpi, the same
    // arrays are 4.17 times longer and pass.
    let least = "[0.03125 0.03125] 0 d 1 w 0 5 m 10 5 l S";
    let short = "[0.03125 0.031249940395355224609375] 0 d 1 w 0 5 m 10 5 l S";
    let odd_least = "[0.03125] 0 d 1 w 0 5 m 10 5 l S";
    let odd_short = "[0.031249940395355224609375] 0 d 1 w 0 5 m 10 5 l S";
    render("0 0 10 10", odd_least, 72);
    render("0 0 10 10", least, 72);
    let message = "dash pattern shorter than 1/16 pixel".to_string();
    assert_eq!(
        error("0 0 10 10", short, 72),
        (message.clone(), at_s(short))
    );
    assert_eq!(
        error("0 0 10 10", odd_short, 72),
        (message.clone(), at_s(odd_short))
    );
    render("0 0 10 10", short, 300);
    render("0 0 10 10", odd_short, 300);
    // The check finds it before anything is drawn: the error comes from
    // the check of the page, with the offset of S.
    let doc = Document::parse(&page_pdf("0 0 10 10", "", short, &[])).unwrap();
    let e = check_page(&doc, &doc.pages().unwrap()[0], 72).unwrap_err();
    assert_eq!(
        (e.message().to_string(), e.offset()),
        (message, at_s(short))
    );
}

#[test]
fn a_stroke_with_a_ctm_that_does_not_keep_angles_is_an_error() {
    let content = "2 0 0 1 0 0 cm 1 w 0 5 m 5 5 l S";
    assert_eq!(
        error("0 0 10 10", content, 72),
        (
            "unsupported stroke with a CTM that does not keep angles".to_string(),
            at_s(content)
        )
    );
    // A rotation keeps them.
    render("0 0 10 10", "0 1 -1 0 10 0 cm 1 w 0 5 m 5 5 l S", 72);
}

#[test]
fn a_stroke_beyond_the_range_of_coordinates_is_an_error() {
    // At 72 dpi, i32::MAX is 8388607.996 points. With a width of 2, the
    // margin is 16 · 256 + 16 units, so a point past 8388591.9 points
    // fails the check, and a point before it passes.
    let content = "2 w 8388590 5 m 8388600 5 l S";
    assert_eq!(
        error("0 0 40 40", content, 72),
        (
            "stroke outside the range of coordinates".to_string(),
            at_s(content)
        )
    );
    render("0 0 40 40", "2 w 8388500 5 m 8388590 5 l S", 72);
    // The check finds it without drawing.
    let doc = Document::parse(&page_pdf(
        "0 0 40 40",
        "",
        "2 w 8388590 5 m 8388600 5 l S",
        &[],
    ))
    .unwrap();
    assert!(check_page(&doc, &doc.pages().unwrap()[0], 72).is_err());
}

#[test]
fn far_dashed_curves_are_drawn_within_a_second() {
    // 100 curves with far control points, dashed with [1 1]: the curves
    // are flattened to 2^16 points each, and the segments beyond the
    // canvas only advance the dash (docs/adr/0025).
    let curve = "-2000000 -2000000 2000000 -2000000 -2000000 2000000 c\n";
    let content = format!("[1 1] 0 d 1 w 0 0 m\n{}S\n", curve.repeat(100));
    let pdf = page_pdf("0 0 100 100", "", &content, &[]);
    let start = std::time::Instant::now();
    render_pdf(&pdf, 72).unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(1), "{elapsed:?}");
}

#[test]
fn a_dotted_line_has_no_dot_at_its_end() {
    // docs/adr/0026: 4 w 1 J [0 12] 0 d on an open line of 36 points
    // draws dots at x = 10, 22 and 34, and none at its end, x = 46, as
    // pdftoppm does.
    let canvas = render("0 0 60 40", "4 w 1 J [0 12] 0 d 10 20 m 46 20 l S", 72);
    let dots: String = row(&canvas, 20)
        .iter()
        .map(|&v| if dark(v) { '#' } else { '.' })
        .collect();
    assert_eq!(
        dots,
        "........####........####........####........................"
    );
}

#[test]
fn an_odd_dash_array_has_the_doubled_period() {
    // docs/adr/0026: the period of [3] is 6; the phase 7 is the phase 1.
    assert_eq!(
        render("0 0 10 10", "[3] 7 d 2 w 0 5 m 10 5 l S", 72),
        render("0 0 10 10", "[3] 1 d 2 w 0 5 m 10 5 l S", 72)
    );
}
