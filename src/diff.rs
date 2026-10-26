//! Diff images: the pixels that differ between two images, in colors
//! (docs/adr/0010). Integer arithmetic only.

use crate::raster::Canvas;

/// After is darker: ink was added.
pub const ADDED: [u8; 3] = [230, 0, 0];
/// After is lighter: ink was removed.
pub const REMOVED: [u8; 3] = [0, 90, 255];
/// The same luminance, another color.
pub const CHANGED: [u8; 3] = [180, 0, 180];
/// In one image only, where the sizes differ.
pub const OUTSIDE: [u8; 3] = [0, 160, 0];
/// The frame around the differences.
pub const FRAME: [u8; 3] = [255, 160, 0];

/// A diff image, the number of pixels that differ and the box around them,
/// `(x0, y0, x1, y1)` with `x1` and `y1` excluded.
pub struct Diff {
    pub image: Canvas,
    pub count: usize,
    pub bbox: (usize, usize, usize, usize),
}

/// `(299 R + 587 G + 114 B + 500) / 1000` (ITU-R BT.601).
pub fn luminance(rgb: [u8; 3]) -> u8 {
    let [r, g, b] = rgb.map(u32::from);
    ((299 * r + 587 * g + 114 * b + 500) / 1000) as u8
}

fn pixel_of(c: &Canvas, x: usize, y: usize) -> Option<[u8; 3]> {
    (x < c.width && y < c.height).then(|| c.pixel(x, y))
}

/// The diff of two images, or `None` if they are the same.
pub fn diff(before: &Canvas, after: &Canvas) -> Option<Diff> {
    let width = before.width.max(after.width);
    let height = before.height.max(after.height);
    // Each pixel: its color and whether it differs.
    let cells: Vec<([u8; 3], bool)> = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .map(
            |(x, y)| match (pixel_of(before, x, y), pixel_of(after, x, y)) {
                (Some(b), Some(a)) if a == b => {
                    let v = 255 - (255 - luminance(a)) / 4;
                    ([v, v, v], false)
                }
                (Some(b), Some(a)) => {
                    let color = match luminance(a).cmp(&luminance(b)) {
                        std::cmp::Ordering::Less => ADDED,
                        std::cmp::Ordering::Greater => REMOVED,
                        std::cmp::Ordering::Equal => CHANGED,
                    };
                    (color, true)
                }
                _ => (OUTSIDE, true),
            },
        )
        .collect();
    let differing: Vec<(usize, usize)> = cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.1)
        .map(|(i, _)| (i % width, i / width))
        .collect();
    let count = differing.len();
    if count == 0 {
        return None;
    }
    let x0 = differing.iter().map(|p| p.0).min().unwrap_or(0);
    let x1 = differing.iter().map(|p| p.0).max().unwrap_or(0) + 1;
    let y0 = differing.iter().map(|p| p.1).min().unwrap_or(0);
    let y1 = differing.iter().map(|p| p.1).max().unwrap_or(0) + 1;
    // The frame: the box widened by 3, without the box widened by 1, over same pixels only.
    let (fx0, fy0) = (x0 as i64 - 3, y0 as i64 - 3);
    let (fx1, fy1) = (x1 as i64 + 3, y1 as i64 + 3);
    let in_frame = |x: i64, y: i64| {
        let outer = (fx0..fx1).contains(&x) && (fy0..fy1).contains(&y);
        let inner = (fx0 + 2..fx1 - 2).contains(&x) && (fy0 + 2..fy1 - 2).contains(&y);
        outer && !inner
    };
    let pixels: Vec<u8> = cells
        .iter()
        .enumerate()
        .flat_map(|(i, &(color, differs))| {
            let (x, y) = ((i % width) as i64, (i / width) as i64);
            if !differs && in_frame(x, y) {
                FRAME
            } else {
                color
            }
        })
        .collect();
    Some(Diff {
        image: Canvas {
            width,
            height,
            pixels,
        },
        count,
        bbox: (x0, y0, x1, y1),
    })
}

#[cfg(test)]
mod tests;
