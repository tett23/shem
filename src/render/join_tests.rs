//! The joins (`j`), the miter limit (`M`) and the square caps (`2 J`) of
//! the strokes of a page (docs/adr/0027).

use super::tests::{page_pdf, render_pdf};
use super::*;

fn render(media_box: &str, content: &str) -> Canvas {
    render_pdf(&page_pdf(media_box, "", content, &[]), 72).unwrap()
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

/// An L of width 4 from (5, 5) up to (5, 15) and right to (15, 15) on a
/// 20 × 20 page: its vertex is at (5, 5) of the device, and its outer
/// corner is the square of pixels (3, 3) to (4, 4).
const L: &str = "4 w 5 5 m 5 15 l 15 15 l S";

#[test]
fn the_defaults_draw_the_same_bytes() {
    // 0 j, 10 M and 0 J are the initial values: with them, the strokes of
    // docs/adr/0025 are the same bytes.
    let content = "3 w 2 2 m 8 2 l 3 7 l 8 8 l 2 8 l 9 5 l S 1 J [2 1] 0 d 1 1 m 9 9 l S";
    let plain = render("0 0 10 10", content);
    assert_eq!(render("0 0 10 10", &format!("0 j {content}")), plain);
    assert_eq!(render("0 0 10 10", &format!("10 M {content}")), plain);
    assert_eq!(render("0 0 10 10", &format!("0 J {content}")), plain);
    assert_eq!(
        render("0 0 10 10", &format!("0 j 10 M 0 J {content}")),
        plain
    );
}

#[test]
fn a_round_join_is_a_disc_at_the_vertex() {
    let miter = render("0 0 20 20", L);
    let round = render("0 0 20 20", &format!("1 j {L}"));
    let bevel = render("0 0 20 20", &format!("2 j {L}"));
    // The outer corner: the miter fills it, the bevel cuts it off, and
    // the round join is between them, a quarter disc of radius 2.
    assert_eq!(miter.pixel(3, 3), [0, 0, 0]);
    assert_eq!(bevel.pixel(3, 3), [255, 255, 255]);
    let corner = round.pixel(3, 3)[0];
    assert!(corner > 0 && corner < 255, "{corner}");
    assert_eq!(round.pixel(4, 4), [0, 0, 0]);
    assert!(dark(round.pixel(3, 4)[0]));
    assert!(dark(round.pixel(4, 3)[0]));
    // Away from the corner, the three are the same.
    assert_eq!(row(&round, 10), row(&miter, 10));
    assert_eq!(row(&bevel, 10), row(&miter, 10));
    // The round join is the disc of a round cap at the vertex
    // (src/raster/stroke_tests.rs): the pixel at the tip of the quarter
    // disc is the one a round cap at the vertex gives.
    let joined = render("0 0 10 10", "1 j 4 w -9 5 m 5 5 l 5 19 l S");
    let capped = render("0 0 10 10", "1 J 4 w -9 5 m 5 5 l S");
    assert_eq!(joined.pixel(6, 6), capped.pixel(6, 6));
    assert!(joined.pixel(6, 6)[0] > 0 && joined.pixel(6, 6)[0] < 255);
}

#[test]
fn a_round_join_at_a_reversal_is_a_disc() {
    // A line that goes right and back: with a miter join (parallel
    // directions, no join) it ends at x = 8 with a butt cap; with a round
    // join, a disc of radius 2 reaches x = 10.
    let miter = row(&render("0 0 12 10", "4 w 2 5 m 8 5 l 2 5 l S"), 4);
    assert!(white(miter[8]) && white(miter[9]), "{miter:?}");
    let round = row(&render("0 0 12 10", "1 j 4 w 2 5 m 8 5 l 2 5 l S"), 4);
    assert!(dark(round[8]) && dark(round[9]), "{round:?}");
    assert!(white(round[10]), "{round:?}");
}

#[test]
fn a_closed_path_has_a_join_at_its_start() {
    // A square of side 10 from (5, 5), closed: with round joins, every
    // outer corner, the start included, is a quarter disc.
    let square = "4 w 5 5 m 15 5 l 15 15 l 5 15 l h S";
    let miter = render("0 0 20 20", square);
    let round = render("0 0 20 20", &format!("1 j {square}"));
    [(3, 3), (16, 3), (3, 16), (16, 16)]
        .iter()
        .for_each(|&(x, y)| {
            assert_eq!(miter.pixel(x, y), [0, 0, 0], "{x} {y}");
            let v = round.pixel(x, y)[0];
            assert!(v > 0 && v < 255, "{x} {y}: {v}");
        });
}

#[test]
fn the_miter_limit_cuts_a_right_angle_between_1_41_and_1_42() {
    // The miter ratio of a right angle is √2 = 1.41421…: at 1.41 M the
    // join is a bevel, at 1.42 M a miter (docs/adr/0027).
    assert_eq!(
        render("0 0 20 20", &format!("1.41 M {L}")).pixel(3, 3),
        [255, 255, 255]
    );
    assert_eq!(
        render("0 0 20 20", &format!("1.42 M {L}")).pixel(3, 3),
        [0, 0, 0]
    );
    assert_eq!(
        render("0 0 20 20", &format!("1.4142 M {L}")).pixel(3, 3),
        [255, 255, 255]
    );
    assert_eq!(
        render("0 0 20 20", &format!("1.4143 M {L}")).pixel(3, 3),
        [0, 0, 0]
    );
    // 1 M cuts every join; a huge limit keeps a sharp one that 10 M cuts.
    assert_eq!(
        render("0 0 20 20", &format!("1 M {L}")).pixel(3, 3),
        [255, 255, 255]
    );
    // The ratio of the angle 2 · atan(0.5 / 8) is 16.
    let sharp = "2 w 2 2 m 10 2.5 l 2 3 l S";
    let cut = render("0 0 20 20", sharp);
    let kept = render("0 0 20 20", &format!("100 M {sharp}"));
    assert_ne!(cut, kept);
    assert!(dark(kept.pixel(11, 17)[0]));
    assert!(white(cut.pixel(11, 17)[0]));
}

#[test]
fn q_and_q_restore_the_join_and_the_limit() {
    let plain = render("0 0 20 20", L);
    assert_eq!(render("0 0 20 20", &format!("q 1 j 1 M Q {L}")), plain);
    assert_ne!(render("0 0 20 20", &format!("q 1 j 1 M {L} Q")), plain);
}

#[test]
fn a_square_cap_extends_the_line_by_half_the_width() {
    // A line of width 4 from (3, 5) to (7, 5) on a 10 × 10 page: butt
    // caps end at x = 3 and 7, square caps at x = 1 and 9.
    let butt = row(&render("0 0 10 10", "4 w 3 5 m 7 5 l S"), 4);
    assert_eq!(butt, [255, 255, 255, 0, 0, 0, 0, 255, 255, 255]);
    let square = row(&render("0 0 10 10", "2 J 4 w 3 5 m 7 5 l S"), 4);
    assert_eq!(square, [255, 0, 0, 0, 0, 0, 0, 0, 0, 255]);
    // The rows above and below the line are not widened.
    let above = row(&render("0 0 10 10", "2 J 4 w 3 5 m 7 5 l S"), 2);
    assert!(above.iter().all(|&v| white(v)), "{above:?}");
}

#[test]
fn square_caps_on_dashes_and_slanted_lines() {
    // [2 2] on a line of width 2 at x from 0: butt dashes [0, 2], [4, 6]
    // and [8, 10]; square caps extend each by 1 at both ends, so the
    // dashes touch and the line is solid.
    let butt: String = row(&render("0 0 10 10", "[2 2] 0 d 2 w 0 5 m 10 5 l S"), 4)
        .iter()
        .map(|&v| if dark(v) { '#' } else { '.' })
        .collect();
    assert_eq!(butt, "##..##..##");
    let square: String = row(&render("0 0 10 10", "2 J [2 2] 0 d 2 w 0 5 m 10 5 l S"), 4)
        .iter()
        .map(|&v| if dark(v) { '#' } else { '.' })
        .collect();
    assert_eq!(square, "##########");
    // A diagonal line extends along its direction: the end at (8, 8) of
    // width 2 reaches (8.7, 8.7) with square caps, so the pixel from
    // (8, 8) to (9, 9) is mostly covered; with butt caps, it is not.
    let butt = render("0 0 12 12", "2 w 2 2 m 8 8 l S");
    let square = render("0 0 12 12", "2 J 2 w 2 2 m 8 8 l S");
    assert!(white(butt.pixel(8, 3)[0]));
    assert!(dark(square.pixel(8, 3)[0]));
    assert!(white(square.pixel(9, 2)[0]));
}

#[test]
fn the_joins_of_a_stroke_in_mode_2_and_of_a_box_do_not_change() {
    // The glyphs of 2 Tr and the crosses of the boxes keep miter joins of
    // limit 10 and butt caps, whatever j, M and J say (docs/adr/0025).
    let doc = crate::pdf::Document::parse(
        &std::fs::read(format!(
            "{}/tests/fixtures/golem/sample.pdf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    let pages = doc.pages().unwrap();
    let text = "0.4 w 2 Tr BT /F1 20 Tf 1 0 0 1 20 20 Tm <73ED> Tj ET";
    let page = |content: &str| crate::pdf::Page {
        contents: content.as_bytes().to_vec(),
        ..pages[0].clone()
    };
    let plain = render_page(&doc, &page(text), 72).unwrap();
    let styled = render_page(&doc, &page(&format!("1 j 2 J 1 M {text}")), 72).unwrap();
    assert_eq!(plain, styled);
}
