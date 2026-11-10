//! The clipping of `W` and `W*`, and the painting operators `f*`, `B`,
//! `B*` and `n` (docs/adr/0027).

use super::tests::{page_pdf, render_pdf};
use super::*;
use crate::fixed::div_round;

fn render(media_box: &str, content: &str) -> Canvas {
    render_pdf(&page_pdf(media_box, "", content, &[]), 72).unwrap()
}

#[test]
fn a_rectangular_clip_limits_a_fill() {
    // 0 0 10 10 re W n on a 20 × 20 page: a fill of the whole page paints
    // the lower left quarter only, with the pixels of the rectangle.
    let clipped = render("0 0 20 20", "0 0 10 10 re W n 0 0 20 20 re f");
    assert_eq!(clipped, render("0 0 20 20", "0 0 10 10 re f"));
    assert_eq!(clipped.pixel(5, 15), [0, 0, 0]);
    assert_eq!(clipped.pixel(5, 5), [255, 255, 255]);
    assert_eq!(clipped.pixel(15, 15), [255, 255, 255]);
    // The edge pixels of a rectangle off the grid have the coverage of
    // the rectangle, as a fill of it has.
    let partial = render("0 0 20 20", "0.5 0.5 9.3 9.3 re W n 0 0 20 20 re f");
    assert_eq!(partial, render("0 0 20 20", "0.5 0.5 9.3 9.3 re f"));
    let edge = partial.pixel(9, 10)[0];
    assert!(edge > 0 && edge < 255, "{edge}");
}

#[test]
fn q_and_q_restore_the_clip_and_two_clips_multiply() {
    let whole = render("0 0 20 20", "0 0 20 20 re f");
    assert_eq!(
        render("0 0 20 20", "q 0 0 10 10 re W n Q 0 0 20 20 re f"),
        whole
    );
    assert_eq!(
        render("0 0 20 20", "q 0 0 10 10 re W n q 0 0 20 20 re f Q Q"),
        render("0 0 20 20", "0 0 10 10 re f")
    );
    assert_eq!(
        render(
            "0 0 20 20",
            "0 0 10 10 re W n 5 5 10 10 re W n 0 0 20 20 re f"
        ),
        render("0 0 20 20", "5 5 5 5 re f")
    );
    // A clip inside q Q and another after it: the second starts from
    // the clip before the q.
    assert_eq!(
        render(
            "0 0 20 20",
            "0 0 10 10 re W n q 5 5 10 10 re W n Q 0 5 10 10 re W n 0 0 20 20 re f"
        ),
        render("0 0 20 20", "0 5 10 5 re f")
    );
}

#[test]
fn an_empty_clip_paints_nothing() {
    for content in [
        "0 0 0 0 re W n 0 0 20 20 re f",
        "30 30 10 10 re W n 0 0 20 20 re f",
        "0 0 10 10 re W n 10 10 10 10 re W n 0 0 20 20 re f",
        "0 0 10 10 re W n 10 10 10 10 re W n 4 w 0 0 m 20 20 l S",
    ] {
        let canvas = render("0 0 20 20", content);
        assert!(canvas.pixels.iter().all(|&v| v == 255), "{content}");
    }
}

#[test]
fn the_clip_applies_to_strokes_and_to_b() {
    let stroke = render("0 0 20 20", "0 0 10 10 re W n 4 w 0 5 m 20 5 l S");
    assert_eq!(stroke.pixel(5, 15), [0, 0, 0]);
    assert_eq!(stroke.pixel(15, 15), [255, 255, 255]);
    assert_eq!(stroke, render("0 0 20 20", "4 w 0 5 m 10 5 l S"));
    let both = render(
        "0 0 20 20",
        "0 0 10 10 re W n 0 0 1 0 k 1 0 0 0 K 2 w 2 2 16 16 re B",
    );
    assert_eq!(both.pixel(5, 15), [255, 255, 0]);
    assert_eq!(both.pixel(5, 17), [0, 255, 255]);
    assert_eq!(both.pixel(15, 15), [255, 255, 255]);
    assert_eq!(both.pixel(15, 3), [255, 255, 255]);
}

#[test]
fn a_slanted_clip_multiplies_the_coverages() {
    // A triangle as the clip and a disc as the fill: at every pixel, the
    // opacity is div_round(c × p, 255) of the coverage of the clip and the
    // coverage of the fill, so the pixel is 255 − that.
    let triangle = "2 1 m 19 7 l 5 18 l h";
    let disc = "16 10 m 16 13.313 13.313 16 10 16 c 6.687 16 4 13.313 4 10 c \
                4 6.687 6.687 4 10 4 c 13.313 4 16 6.687 16 10 c h";
    let c = render("0 0 20 20", &format!("{triangle} f"));
    let p = render("0 0 20 20", &format!("{disc} f"));
    let x = render("0 0 20 20", &format!("{triangle} W n {disc} f"));
    let mut partial = 0;
    (0..20).for_each(|y| {
        (0..20).for_each(|x_| {
            let (c, p) = (255 - c.pixel(x_, y)[0], 255 - p.pixel(x_, y)[0]);
            let a = div_round(i128::from(c) * i128::from(p), 255) as u8;
            assert_eq!(x.pixel(x_, y), [255 - a; 3], "({x_}, {y}): {c} × {p}");
            if a > 0 && a < 255 {
                partial += 1;
            }
        });
    });
    assert!(partial > 10, "{partial}");
    // The product is at most either coverage, and the even-odd rule
    // clips with its own coverage.
    let star = "10 2 m 14 18 l 2 8 l 18 8 l 6 18 l h";
    let nonzero = render("0 0 20 20", &format!("{star} W n 0 0 20 20 re f"));
    let even_odd = render("0 0 20 20", &format!("{star} W* n 0 0 20 20 re f"));
    assert_eq!(nonzero, render("0 0 20 20", &format!("{star} f")));
    assert_eq!(even_odd, render("0 0 20 20", &format!("{star} f*")));
    assert_eq!(nonzero.pixel(10, 10), [0, 0, 0]);
    assert_eq!(even_odd.pixel(10, 10), [255, 255, 255]);
}

#[test]
fn w_before_a_painting_operator_paints_then_clips() {
    // W f paints the path, and the next fill is clipped to it.
    let canvas = render("0 0 20 20", "0 0 10 10 re W f 0 0 0 1 k 5 5 10 10 re f");
    assert_eq!(canvas.pixel(2, 17), [0, 0, 0]);
    assert_eq!(canvas.pixel(7, 12), [0, 0, 0]);
    assert_eq!(canvas.pixel(12, 7), [255, 255, 255]);
    // W S strokes the path, and clips to its interior.
    let stroked = render("0 0 20 20", "1 0 0 0 K 4 w 2 2 10 10 re W S 0 0 20 20 re f");
    assert_eq!(stroked.pixel(6, 12), [0, 0, 0]);
    assert_eq!(stroked.pixel(1, 12), [0, 255, 255]);
    assert_eq!(stroked.pixel(16, 3), [255, 255, 255]);
}

#[test]
fn n_ends_the_path_without_painting() {
    let canvas = render("0 0 20 20", "0 0 10 10 re n 5 5 m 15 15 l n");
    assert!(canvas.pixels.iter().all(|&v| v == 255));
    // The path is empty after n: the next subpath starts anew.
    let canvas = render("0 0 20 20", "0 0 10 10 re n 10 10 10 10 re f");
    assert_eq!(canvas.pixel(15, 5), [0, 0, 0]);
    assert_eq!(canvas.pixel(5, 15), [255, 255, 255]);
}

#[test]
fn f_star_fills_with_the_even_odd_rule() {
    let two = "0 0 10 10 re 5 5 10 10 re";
    let nonzero = render("0 0 20 20", &format!("{two} f"));
    let even_odd = render("0 0 20 20", &format!("{two} f*"));
    assert_eq!(nonzero.pixel(7, 12), [0, 0, 0]);
    assert_eq!(even_odd.pixel(7, 12), [255, 255, 255]);
    assert_eq!(even_odd.pixel(2, 17), [0, 0, 0]);
    assert_eq!(even_odd.pixel(12, 7), [0, 0, 0]);
}

#[test]
fn b_fills_then_strokes() {
    let both = render("0 0 20 20", "0 0 1 0 k 1 0 0 0 K 2 w 5 5 10 10 re B");
    assert_eq!(both.pixel(10, 10), [255, 255, 0]);
    assert_eq!(both.pixel(5, 10), [0, 255, 255]);
    assert_eq!(both.pixel(10, 5), [0, 255, 255]);
    assert_eq!(both.pixel(2, 10), [255, 255, 255]);
    // B is f then S, with the same bytes.
    assert_eq!(
        both,
        render(
            "0 0 20 20",
            "0 0 1 0 k 1 0 0 0 K 2 w 5 5 10 10 re f 5 5 10 10 re S"
        )
    );
    let two = "0 0 10 10 re 5 5 10 10 re";
    let star = render("0 0 20 20", &format!("0 0 1 0 k 1 0 0 0 K 2 w {two} B*"));
    assert_eq!(star.pixel(7, 12), [255, 255, 255]);
    assert_eq!(star.pixel(2, 17), [255, 255, 0]);
    assert_eq!(star.pixel(5, 15), [0, 255, 255]);
    assert_eq!(
        star,
        render(
            "0 0 20 20",
            &format!("0 0 1 0 k 1 0 0 0 K 2 w {two} f* {two} S")
        )
    );
}

#[test]
fn the_clip_is_kept_within_the_bounds_of_its_pixels() {
    // docs/adr/0027: the mask holds the pixels of its bounding box only,
    // and the clip of a path beyond the page is empty.
    let mut ws = Workspace::default();
    let doc = Document::parse(&page_pdf(
        "0 0 20 20",
        "",
        "2.5 3 4 5 re W n 0 0 20 20 re f",
        &[],
    ))
    .unwrap();
    let pages = doc.pages().unwrap();
    let session = Session::from_pages(&doc, &pages, 72);
    session.render_with(&mut ws, 0).unwrap();
    let clip = ws.clips.last().unwrap();
    // The rectangle from (2.5, 3) to (6.5, 8) covers the device pixels of
    // columns 2 to 6 and rows 12 to 16.
    assert_eq!(clip.bounds(), Some((2, 12, 7, 17)));
    assert_eq!(clip.value(2, 12), 128);
    assert_eq!(clip.value(3, 13), 255);
    assert_eq!(clip.value(7, 13), 0);
    assert_eq!(clip.value(19, 19), 0);
}
