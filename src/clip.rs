//! The clip of the graphics state: a coverage from 0 to 255 per pixel,
//! held within the bounding box of its pixels above 0 (docs/adr/0027).
//!
//! `W` and `W*` make the clip the product of the current one and the
//! coverage of a path, computed as a fill is. A fill, a stroke or an image
//! multiplies its opacity by the clip of each pixel, with
//! [`mul_alpha`]. The masks of a page are kept in a [`ClipStack`] of the
//! thread's workspace, so that clipping allocates nothing per path
//! (docs/adr/0013).

use crate::raster::{FillRule, Path, Scratch, grid_bounds, visit_rows};

/// `div_round(a × b, 255)`: the product of two opacities or coverages. The
/// quotient is never halfway, since 255 is odd, so the rounding is
/// `(a × b + 127) / 255`.
pub fn mul_alpha(a: u8, b: u8) -> u8 {
    ((u32::from(a) * u32::from(b) + 127) / 255) as u8
}

/// The coverage of a clip within its bounding box; 0 elsewhere. An empty
/// box clips everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clip {
    x0: usize,
    y0: usize,
    width: usize,
    height: usize,
    /// `width × height` values, row by row.
    data: Vec<u8>,
}

impl Clip {
    /// The bounding box of the pixels above 0 as `(x0, y0, x1, y1)`, the
    /// ends excluded, or `None` if no pixel is above 0.
    pub fn bounds(&self) -> Option<(usize, usize, usize, usize)> {
        (self.width > 0 && self.height > 0).then_some((
            self.x0,
            self.y0,
            self.x0 + self.width,
            self.y0 + self.height,
        ))
    }

    pub fn is_empty(&self) -> bool {
        self.bounds().is_none()
    }

    /// The coverage of the clip at a pixel.
    pub fn value(&self, x: usize, y: usize) -> u8 {
        if x < self.x0 || y < self.y0 || x - self.x0 >= self.width || y - self.y0 >= self.height {
            return 0;
        }
        self.data[(y - self.y0) * self.width + (x - self.x0)]
    }

    /// The pixels `x0..x1` of row `y` that are within the bounding box, and
    /// their coverages; none if the row or the range is outside it.
    pub fn span(&self, y: usize, x0: usize, x1: usize) -> Option<(std::ops::Range<usize>, &[u8])> {
        let (bx0, by0, bx1, by1) = self.bounds()?;
        if y < by0 || y >= by1 {
            return None;
        }
        let (lo, hi) = (x0.max(bx0), x1.min(bx1));
        if lo >= hi {
            return None;
        }
        let row = (y - self.y0) * self.width;
        Some((lo..hi, &self.data[row + lo - bx0..row + hi - bx0]))
    }

    /// Makes `self` the product of `previous` (no clip if `None`) and the
    /// coverage of `path` under `rule` on a `width` × `height` canvas,
    /// `div_round(c × p, 255)` at each pixel, within the bounding box of
    /// the pixels above 0 (docs/adr/0027). The buffer of `self` is reused.
    pub fn intersect_into(
        &mut self,
        previous: Option<&Clip>,
        path: &Path,
        rule: FillRule,
        width: usize,
        height: usize,
        scratch: &mut Scratch,
    ) {
        // The pixels the path can cover: its rows, and the columns from
        // its first to the right edge of the canvas; within the previous
        // clip.
        let rect = grid_bounds(path, width, height).and_then(|(row0, row1, col0, _)| {
            let (x0, y0, x1, y1) = (
                col0.max(0) as usize,
                row0 as usize,
                width,
                row1 as usize + 1,
            );
            match previous {
                None => Some((x0, y0, x1, y1)),
                Some(p) => {
                    let (px0, py0, px1, py1) = p.bounds()?;
                    let (x0, y0, x1, y1) = (x0.max(px0), y0.max(py0), x1.min(px1), y1.min(py1));
                    (x0 < x1 && y0 < y1).then_some((x0, y0, x1, y1))
                }
            }
        });
        let Some((x0, y0, x1, y1)) = rect else {
            self.set_empty();
            return;
        };
        (self.x0, self.y0, self.width, self.height) = (x0, y0, x1 - x0, y1 - y0);
        self.data.clear();
        self.data.resize(self.width * self.height, 0);
        let (w, data) = (self.width, &mut self.data);
        // The bounding box of the pixels above 0, as they are written.
        let mut tight = (usize::MAX, usize::MAX, 0usize, 0usize);
        visit_rows(path, rule, width, height, scratch, |y, rx0, alphas| {
            if y < y0 || y >= y1 {
                return;
            }
            let (lo, hi) = (rx0.max(x0), (rx0 + alphas.len()).min(x1));
            if lo >= hi {
                return;
            }
            let row = &mut data[(y - y0) * w..][..w];
            let alphas = &alphas[lo - rx0..hi - rx0];
            let values = row[lo - x0..hi - x0].iter_mut().zip(alphas);
            match previous {
                None => values.for_each(|(v, &a)| *v = a),
                Some(p) => {
                    let (_, clip) = p.span(y, lo, hi).expect("the row is within the clip");
                    values
                        .zip(clip)
                        .for_each(|((v, &a), &c)| *v = mul_alpha(a, c));
                }
            }
            row[lo - x0..hi - x0]
                .iter()
                .zip(lo..)
                .filter(|&(&v, _)| v > 0)
                .for_each(|(_, x)| {
                    tight = (
                        tight.0.min(x),
                        tight.1.min(y),
                        tight.2.max(x + 1),
                        tight.3.max(y + 1),
                    );
                });
        });
        self.shrink_to(tight);
    }

    fn set_empty(&mut self) {
        (self.x0, self.y0, self.width, self.height) = (0, 0, 0, 0);
        self.data.clear();
    }

    /// Keeps the pixels of the box `(x0, y0, x1, y1)`, which is within the
    /// current one, moving the rows in place.
    fn shrink_to(&mut self, (x0, y0, x1, y1): (usize, usize, usize, usize)) {
        if x0 >= x1 || y0 >= y1 {
            self.set_empty();
            return;
        }
        let (old_w, old_x0, old_y0) = (self.width, self.x0, self.y0);
        let (w, h) = (x1 - x0, y1 - y0);
        if (x0, y0, w, h) == (old_x0, old_y0, self.width, self.height) {
            return;
        }
        // Each row moves to an index at most its own: the rows before it
        // only get shorter, so the copies never pass unread pixels.
        (0..h).for_each(|i| {
            let from = (y0 - old_y0 + i) * old_w + (x0 - old_x0);
            let to = i * w;
            self.data.copy_within(from..from + w, to);
        });
        self.data.truncate(w * h);
        (self.x0, self.y0, self.width, self.height) = (x0, y0, w, h);
    }
}

/// The masks of the clips of a page's graphics states (docs/adr/0027). A
/// state holds the index of its mask; the masks are made in order, and a
/// state's index is at least those of the states below it on the stack,
/// so that the masks after the one of the state below are free when a
/// state gets a new clip or is popped. The buffers of the masks are kept
/// for the next masks and the next pages (docs/adr/0013).
#[derive(Clone, Debug, Default)]
pub struct ClipStack {
    masks: Vec<Clip>,
    /// The masks in use: `masks[..live]`.
    live: usize,
    /// The mask being made, swapped into place when it is done.
    next: Clip,
}

impl ClipStack {
    /// Forgets the masks, for a new page.
    pub fn clear(&mut self) {
        self.live = 0;
    }

    pub fn get(&self, index: usize) -> &Clip {
        &self.masks[index]
    }

    /// The mask made last, if any is in use.
    pub fn last(&self) -> Option<&Clip> {
        self.live.checked_sub(1).map(|i| &self.masks[i])
    }

    /// Frees the masks from `keep` on, the ones that no state holds.
    pub fn truncate(&mut self, keep: usize) {
        self.live = self.live.min(keep);
    }

    /// Makes the mask of the product of `previous` and the path, as
    /// [`Clip::intersect_into`], at index `at`, freeing the masks from
    /// `at` on, and gives its index.
    #[allow(clippy::too_many_arguments)]
    pub fn intersect(
        &mut self,
        at: usize,
        previous: Option<usize>,
        path: &Path,
        rule: FillRule,
        width: usize,
        height: usize,
        scratch: &mut Scratch,
    ) -> usize {
        let mut next = std::mem::take(&mut self.next);
        next.intersect_into(
            previous.map(|i| &self.masks[i]),
            path,
            rule,
            width,
            height,
            scratch,
        );
        if self.masks.len() <= at {
            self.masks.resize_with(at + 1, Clip::default);
        }
        self.next = std::mem::replace(&mut self.masks[at], next);
        self.live = at + 1;
        at
    }
}

#[cfg(test)]
mod tests;
