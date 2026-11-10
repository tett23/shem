use super::*;
use crate::fixed::div_round;
use crate::raster::{ONE, coverage};

/// A rectangle of pixels from (x0, y0) to (x1, y1), in device units.
fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Path {
    let mut path = Path::new();
    path.move_to((x0 * ONE, y0 * ONE));
    path.line_to((x1 * ONE, y0 * ONE));
    path.line_to((x1 * ONE, y1 * ONE));
    path.line_to((x0 * ONE, y1 * ONE));
    path.close();
    path
}

fn clip_of(previous: Option<&Clip>, path: &Path, rule: FillRule, w: usize, h: usize) -> Clip {
    let mut clip = Clip::default();
    clip.intersect_into(previous, path, rule, w, h, &mut Scratch::default());
    clip
}

#[test]
fn mul_alpha_is_div_round_by_255() {
    (0..=255u8).for_each(|a| {
        (0..=255u8).for_each(|b| {
            let expected = div_round(i128::from(a) * i128::from(b), 255) as u8;
            assert_eq!(mul_alpha(a, b), expected, "{a} × {b}");
        });
    });
    assert_eq!(mul_alpha(255, 255), 255);
    assert_eq!(mul_alpha(0, 255), 0);
}

#[test]
fn a_clip_holds_the_bounding_box_of_its_pixels() {
    let clip = clip_of(None, &rect(2, 3, 6, 8), FillRule::NonZero, 20, 20);
    assert_eq!(clip.bounds(), Some((2, 3, 6, 8)));
    assert_eq!(clip.value(2, 3), 255);
    assert_eq!(clip.value(5, 7), 255);
    assert_eq!(clip.value(6, 7), 0);
    assert_eq!(clip.value(1, 3), 0);
    assert_eq!(clip.value(19, 19), 0);
    assert_eq!(clip.span(4, 0, 20), Some((2..6, &[255u8; 4][..])));
    assert_eq!(clip.span(4, 4, 20), Some((4..6, &[255u8; 2][..])));
    assert_eq!(clip.span(9, 0, 20), None);
    assert_eq!(clip.span(4, 6, 20), None);
}

#[test]
fn the_values_are_the_coverage_of_the_path() {
    // A slanted triangle: every pixel of the clip is the coverage that a
    // fill of the path gives, and the box is the one of the pixels above 0.
    let mut path = Path::new();
    path.move_to((ONE / 2, ONE));
    path.line_to((17 * ONE + 100, 5 * ONE + 3));
    path.line_to((4 * ONE, 15 * ONE));
    path.close();
    let clip = clip_of(None, &path, FillRule::NonZero, 20, 20);
    let pixels = coverage(&path, FillRule::NonZero, 20, 20);
    let x0 = pixels.iter().map(|p| p.0).min().unwrap();
    let x1 = pixels.iter().map(|p| p.0).max().unwrap() + 1;
    let y0 = pixels.iter().map(|p| p.1).min().unwrap();
    let y1 = pixels.iter().map(|p| p.1).max().unwrap() + 1;
    assert_eq!(clip.bounds(), Some((x0, y0, x1, y1)));
    (0..20).for_each(|y| {
        (0..20).for_each(|x| {
            let expected = pixels
                .iter()
                .find(|p| (p.0, p.1) == (x, y))
                .map_or(0, |p| p.2);
            assert_eq!(clip.value(x, y), expected, "({x}, {y})");
        });
    });
}

#[test]
fn two_clips_multiply_within_their_intersection() {
    let first = clip_of(None, &rect(0, 0, 10, 10), FillRule::NonZero, 20, 20);
    let second = clip_of(Some(&first), &rect(5, 5, 15, 15), FillRule::NonZero, 20, 20);
    assert_eq!(second.bounds(), Some((5, 5, 10, 10)));
    assert_eq!(second.value(7, 7), 255);
    assert_eq!(second.value(12, 7), 0);
    // Partial coverages multiply.
    let mut half = Path::new();
    half.move_to((ONE / 2, 0));
    half.line_to((10 * ONE, 0));
    half.line_to((10 * ONE, 10 * ONE));
    half.line_to((ONE / 2, 10 * ONE));
    half.close();
    let a = clip_of(None, &half, FillRule::NonZero, 20, 20);
    assert_eq!(a.value(0, 0), 128);
    let b = clip_of(Some(&a), &half, FillRule::NonZero, 20, 20);
    assert_eq!(b.value(0, 0), mul_alpha(128, 128));
    assert_eq!(b.value(1, 0), 255);
    // Disjoint: empty.
    let none = clip_of(
        Some(&first),
        &rect(10, 10, 15, 15),
        FillRule::NonZero,
        20,
        20,
    );
    assert!(none.is_empty());
    assert_eq!(none.value(12, 12), 0);
    // A path off the canvas: empty.
    let off = clip_of(None, &rect(30, 30, 40, 40), FillRule::NonZero, 20, 20);
    assert!(off.is_empty());
    // A path that covers the canvas from the left: its coverage reaches
    // the right edge.
    let wide = clip_of(None, &rect(-10, 2, 30, 4), FillRule::NonZero, 20, 20);
    assert_eq!(wide.bounds(), Some((0, 2, 20, 4)));
}

#[test]
fn the_even_odd_rule_clips_its_own_coverage() {
    let mut two = rect(0, 0, 10, 10);
    two.move_to((5 * ONE, 5 * ONE));
    two.line_to((15 * ONE, 5 * ONE));
    two.line_to((15 * ONE, 15 * ONE));
    two.line_to((5 * ONE, 15 * ONE));
    two.close();
    let nonzero = clip_of(None, &two, FillRule::NonZero, 20, 20);
    let even_odd = clip_of(None, &two, FillRule::EvenOdd, 20, 20);
    assert_eq!(nonzero.value(7, 7), 255);
    assert_eq!(even_odd.value(7, 7), 0);
    assert_eq!(even_odd.value(2, 2), 255);
    assert_eq!(even_odd.bounds(), Some((0, 0, 15, 15)));
}

#[test]
fn the_stack_frees_the_masks_above_the_state_below() {
    let mut stack = ClipStack::default();
    let mut scratch = Scratch::default();
    let a = stack.intersect(
        0,
        None,
        &rect(0, 0, 10, 10),
        FillRule::NonZero,
        20,
        20,
        &mut scratch,
    );
    assert_eq!(a, 0);
    // A clip inside q: at index 1, from the one at 0.
    let b = stack.intersect(
        1,
        Some(a),
        &rect(5, 5, 15, 15),
        FillRule::NonZero,
        20,
        20,
        &mut scratch,
    );
    assert_eq!(b, 1);
    assert_eq!(stack.last().unwrap().bounds(), Some((5, 5, 10, 10)));
    // Q: the mask at 1 is free.
    stack.truncate(1);
    assert_eq!(stack.last().unwrap().bounds(), Some((0, 0, 10, 10)));
    // Another clip at the same level replaces the one at 0, from it.
    let c = stack.intersect(
        0,
        Some(a),
        &rect(0, 5, 20, 20),
        FillRule::NonZero,
        20,
        20,
        &mut scratch,
    );
    assert_eq!(c, 0);
    assert_eq!(stack.get(c).bounds(), Some((0, 5, 10, 10)));
    stack.clear();
    assert!(stack.last().is_none());
}
