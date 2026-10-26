use super::*;
use crate::raster::Canvas;

const WHITE: [u8; 3] = [255, 255, 255];

fn solid(width: usize, height: usize, rgb: [u8; 3]) -> Canvas {
    Canvas {
        width,
        height,
        pixels: rgb.repeat(width * height),
    }
}

fn set(c: &mut Canvas, x: usize, y: usize, rgb: [u8; 3]) {
    let i = (y * c.width + x) * 3;
    c.pixels[i..i + 3].copy_from_slice(&rgb);
}

#[test]
fn the_same_images_have_no_diff() {
    assert!(diff(&solid(5, 4, WHITE), &solid(5, 4, WHITE)).is_none());
}

#[test]
fn a_darker_pixel_is_red_and_framed() {
    let before = solid(20, 20, WHITE);
    let mut after = before.clone();
    set(&mut after, 10, 10, [0, 0, 0]);
    let d = diff(&before, &after).unwrap();
    assert_eq!((d.count, d.bbox), (1, (10, 10, 11, 11)));
    assert_eq!(d.image.pixel(10, 10), ADDED);
    // the same pixels are the faded after image: white stays white
    assert_eq!(d.image.pixel(0, 0), WHITE);
    assert_eq!(d.image.pixel(10, 11), WHITE);
    // the frame: 2 pixels wide, from 3 to 2 pixels outside the box
    assert_eq!(d.image.pixel(7, 10), FRAME);
    assert_eq!(d.image.pixel(8, 10), FRAME);
    assert_eq!(d.image.pixel(9, 10), WHITE);
    assert_eq!(d.image.pixel(6, 10), WHITE);
    assert_eq!(d.image.pixel(13, 13), FRAME);
    assert_eq!(d.image.pixel(14, 14), WHITE);
}

#[test]
fn a_lighter_pixel_is_blue() {
    let before = solid(5, 5, [0, 0, 0]);
    let mut after = before.clone();
    set(&mut after, 2, 2, WHITE);
    let d = diff(&before, &after).unwrap();
    assert_eq!(d.image.pixel(2, 2), REMOVED);
    // (0, 0) is on the frame; (1, 1), inside the frame, is black faded to 255 - 255 / 4 = 192
    assert_eq!(d.image.pixel(0, 0), FRAME);
    assert_eq!(d.image.pixel(1, 1), [192, 192, 192]);
}

#[test]
fn a_change_of_color_with_the_same_luminance_is_purple() {
    // find two colors of the same luminance
    let gray = [100, 100, 100];
    let other = (0..=255u8)
        .flat_map(|r| (0..=255u8).map(move |g| [r, g, 0]))
        .find(|&c| luminance(c) == luminance(gray))
        .unwrap();
    let before = solid(3, 3, gray);
    let mut after = before.clone();
    set(&mut after, 1, 1, other);
    assert_eq!(diff(&before, &after).unwrap().image.pixel(1, 1), CHANGED);
}

#[test]
fn faded_pixels_follow_the_formula() {
    let before = solid(12, 12, [100, 150, 200]);
    let mut after = before.clone();
    set(&mut after, 11, 11, WHITE);
    let d = diff(&before, &after).unwrap();
    let l = luminance([100, 150, 200]);
    let v = 255 - (255 - l) / 4;
    assert_eq!(d.image.pixel(0, 0), [v, v, v]);
}

#[test]
fn images_of_different_sizes() {
    let before = solid(4, 3, WHITE);
    let after = solid(6, 3, WHITE);
    let d = diff(&before, &after).unwrap();
    assert_eq!((d.image.width, d.image.height), (6, 3));
    assert_eq!(d.count, 6);
    assert_eq!(d.bbox, (4, 0, 6, 3));
    assert_eq!(d.image.pixel(4, 0), OUTSIDE);
    assert_eq!(d.image.pixel(5, 2), OUTSIDE);
    assert_eq!(d.image.pixel(3, 0), WHITE);
}

#[test]
fn the_frame_is_cut_at_the_edges_and_spares_the_differences() {
    let before = solid(4, 4, WHITE);
    let mut after = before.clone();
    set(&mut after, 0, 0, [0, 0, 0]);
    set(&mut after, 3, 3, [0, 0, 0]);
    let d = diff(&before, &after).unwrap();
    assert_eq!(d.bbox, (0, 0, 4, 4));
    assert_eq!(d.image.pixel(0, 0), ADDED);
    assert_eq!(d.image.pixel(3, 3), ADDED);
    // the frame is outside the image, so no pixel is orange
    assert!(d.image.pixels.chunks(3).all(|p| p != FRAME));
}

#[test]
fn luminance_formula() {
    assert_eq!(luminance([0, 0, 0]), 0);
    assert_eq!(luminance([255, 255, 255]), 255);
    // (299 * 255 + 500) / 1000 = 76
    assert_eq!(luminance([255, 0, 0]), 76);
}
