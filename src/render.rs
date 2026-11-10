//! Interpreting a page's content into an image (docs/adr/0003).

use crate::clip::ClipStack;
use crate::color::{self, ColorSpace, Device};
use crate::content::{self, Op, Operator, Text};
use crate::error::{Error, Result};
use crate::fixed::{Fixed, Matrix, div_round_i64, isqrt, round_shift};
use crate::font::{Font, FontCache, Segment};
use crate::pdf::Object;
use crate::pdf::{Dict, Document, Page, Stream};
use crate::picture::{self, Mapping, Picture, PictureCache};
use crate::raster::{
    Canvas, Cap, DASH_UNIT, FillRule, Frame, Join, Path, PathOf, Point, Scratch, Stroke,
    StrokeScratch, StrokeScratchOf, signed_area, stroke_into, stroke_styled_into,
};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

/// The largest number of pixels of a page (docs/adr/0007).
const MAX_PIXELS: u128 = 1 << 28;

/// The most levels of forms within forms, the page being level 0
/// (docs/adr/0027).
const MAX_FORM_DEPTH: usize = 8;

/// The size of the image of a page: the size of the MediaBox times dpi / 72,
/// rounded up, computed exactly from the Q32.32 values.
pub fn page_size(media_box: &[Fixed; 4], dpi: u32) -> Result<(usize, usize)> {
    let pixels = |lo: Fixed, hi: Fixed| -> Result<u128> {
        let points = u128::try_from(i128::from(hi.raw()) - i128::from(lo.raw()))
            .map_err(|_| Error::overflow())?;
        Ok((points * u128::from(dpi)).div_ceil(72u128 << 32))
    };
    let width = pixels(media_box[0], media_box[2])?;
    let height = pixels(media_box[1], media_box[3])?;
    if width == 0 || height == 0 {
        return Err(Error::new("the page has no area"));
    }
    if width * height > MAX_PIXELS {
        return Err(Error::new(format!(
            "the image of the page is too large: {width} × {height} pixels"
        )));
    }
    Ok((width as usize, height as usize))
}

/// The matrix from the default user space to the device space of the image:
/// `[s 0 0 −s −s·llx s·ury]` with `s = dpi / 72`.
pub fn device_matrix(media_box: &[Fixed; 4], dpi: u32) -> Result<Matrix> {
    let s = Fixed::from_int(i64::from(dpi))?.checked_div(Fixed::from_int(72)?)?;
    Ok(Matrix::new([
        s,
        Fixed::ZERO,
        Fixed::ZERO,
        s.checked_neg()?,
        s.checked_mul(media_box[0])?.checked_neg()?,
        s.checked_mul(media_box[3])?,
    ]))
}

/// A point of user space in device space, in units of 1/256 pixel.
pub fn to_device(ctm: &Matrix, x: Fixed, y: Fixed) -> Result<Point> {
    let (dx, dy) = ctm.apply(x, y)?;
    let unit = |v: Fixed| i32::try_from(v.to_units(8)).map_err(|_| Error::overflow());
    Ok((unit(dx)?, unit(dy)?))
}

/// [`to_device`] of a point that [`fits_in_device`] has shown to fit: the
/// same sum and the same two roundings, without the checks.
pub fn to_device_fitting(m: &Matrix, x: Fixed, y: Fixed) -> Point {
    let (x, y) = (i128::from(x.raw()), i128::from(y.raw()));
    let map = |p: Fixed, q: Fixed, t: Fixed| {
        let sum = i128::from(p.raw()) * x + i128::from(q.raw()) * y + (i128::from(t.raw()) << 32);
        let raw = round_shift(sum, 32) as i64;
        round_shift_i64(raw, 24) as i32
    };
    (map(m.a, m.c, m.e), map(m.b, m.d, m.f))
}

/// [`round_shift`] of an i64.
fn round_shift_i64(v: i64, s: u32) -> i64 {
    let q = v >> s;
    let r = v - (q << s);
    let half = 1i64 << (s - 1);
    if r > half || (r == half && q & 1 == 1) {
        q + 1
    } else {
        q
    }
}

/// Whether [`to_device`] maps every point of `segments` through `m`
/// without an overflow, shown by bounds: with X and Y the largest absolute
/// values of the coordinates, `|a x + c y + e|` is at most `|a| X + |c| Y +
/// |e|`, the rounding to 2^-32 adds at most one unit, and the rounding to
/// 1/256 pixel one more, which must stay below 2^31 (docs/adr/0015). If the
/// bounds do not show it, the points may still fit.
pub fn fits_in_device(m: &Matrix, segments: &[Segment]) -> bool {
    fits_with_extent(m, crate::font::extent(segments))
}

/// [`fits_in_device`] from the largest absolute values of the coordinates.
fn fits_with_extent(m: &Matrix, extent: (u64, u64)) -> bool {
    fits_within(m, extent, i64::from(i32::MAX))
}

/// [`fits_with_extent`] with every unit at most `reach` in size: the
/// points of a glyph of Tr 2 leave room for the margin of its stroke
/// (docs/adr/0022).
fn fits_within(m: &Matrix, (mx, my): (u64, u64), reach: i64) -> bool {
    if reach < 2 {
        return false;
    }
    // The bound of |p x + q y + t| in units of 2^-64.
    let bound = |p: Fixed, q: Fixed, t: Fixed| -> Option<u128> {
        let px = u128::from(p.raw().unsigned_abs()).checked_mul(u128::from(mx))?;
        let qy = u128::from(q.raw().unsigned_abs()).checked_mul(u128::from(my))?;
        px.checked_add(qy)?
            .checked_add(u128::from(t.raw().unsigned_abs()) << 32)
    };
    // raw ≤ S / 2^32 + 1 and units ≤ raw / 2^24 + 1 ≤ reach.
    let limit: u128 = (((reach as u128 - 1) << 24) - 1) << 32;
    [bound(m.a, m.c, m.e), bound(m.b, m.d, m.f)]
        .iter()
        .all(|b| b.is_some_and(|b| b <= limit))
}

/// The margin around the points of a glyph of Tr 2 that holds the polygons
/// of its stroke: the quadrilaterals reach `half` from the segments, the
/// miters at most 10 · half from the vertices, and the rounding a few units
/// (docs/adr/0022).
fn stroke_margin(half: i64) -> i64 {
    16 * half + 16
}

/// Whether a CTM keeps angles: a rotation or a reflection with one scale
/// (docs/adr/0003). The strokes under one are scaled in device space;
/// the others are built in user space (docs/adr/0027).
fn keeps_angles(ctm: &Matrix) -> Result<bool> {
    Ok((ctm.a == ctm.d && ctm.b.checked_neg()? == ctm.c)
        || (ctm.a.checked_neg()? == ctm.d && ctm.b == ctm.c))
}

/// The scale of a stroke: `sqrt(|det CTM|)`, by which the widths and the
/// dashes of user space go to device space. The CTM keeps angles; another
/// is an error (docs/adr/0003).
fn stroke_scale(ctm: &Matrix) -> Result<Fixed> {
    if !keeps_angles(ctm)? {
        return Err(Error::new(
            "unsupported stroke with a CTM that does not keep angles",
        ));
    }
    ctm.det()?.abs()?.sqrt()
}

/// An upper bound of the scale of a CTM, `sqrt(a² + b² + c² + d²)`, as a
/// Q32.32 raw value: the integer square root of the sum, which is at most
/// it, plus 1 (docs/adr/0027). By it, a length of user space is at most
/// that many pixels per unit, whatever its direction.
fn scale_bound(ctm: &Matrix) -> u128 {
    let sq = |v: Fixed| u128::from(v.raw().unsigned_abs()).pow(2);
    let sum = [ctm.a, ctm.b, ctm.c, ctm.d]
        .iter()
        .fold(0u128, |sum, &v| sum.saturating_add(sq(v)));
    isqrt(sum) + 1
}

/// The flatness of the curves of user space under a CTM: the largest
/// second difference, in units of 2^−32, that the bound of the scale maps
/// to 1/16 pixel or less: `2^60 / bound` (docs/adr/0027).
fn user_flatness(bound: u128) -> i64 {
    ((1u128 << 60) / bound).min(i64::MAX as u128) as i64
}

/// The least period of a dash array in user space under a CTM: the one
/// that the bound of the scale maps to 1/16 pixel, so `period · bound ≥
/// 2^60` (docs/adr/0027).
fn user_period_is_long_enough(period: i128, bound: u128) -> bool {
    period >= 0 && (period as u128).saturating_mul(bound) >= 1 << 60
}

/// The largest absolute value of the coordinates of the points of a path
/// of user space that the stroke's arithmetic takes: below 2^62, so that
/// the differences of two coordinates fit in i64 (docs/adr/0027).
const USER_REACH: i64 = (1 << 62) - 1;

/// Half the width of a stroke in device space, in units of 1/256 pixel. The
/// CTM keeps angles; a line width of 0 is one pixel (ISO 32000-1 8.4.3.2).
pub fn stroke_half_width(ctm: &Matrix, width: Fixed) -> Result<i64> {
    half_width(width, stroke_scale(ctm)?)
}

/// [`stroke_half_width`] from the scale of the stroke.
fn half_width(width: Fixed, scale: Fixed) -> Result<i64> {
    if width == Fixed::ZERO {
        return Ok(128);
    }
    // The width in pixels, in units of 2^-7, is half of it in units of 1/256.
    Ok(width.checked_mul(scale)?.to_units(7).max(1))
}

/// A length of a dash array or its phase in device space, in units of
/// 2^−16 device units, rounded to the nearest, ties to even
/// (docs/adr/0025).
fn dash_length(value: Fixed, scale: Fixed) -> Result<i64> {
    Ok(value.checked_mul(scale)?.to_units(8 + 16))
}

/// The least period of a dash array in device space: 1/16 pixel
/// (docs/adr/0025).
const MIN_DASH_PERIOD: i128 = 16 * DASH_UNIT as i128;

#[derive(Clone)]
struct GraphicsState {
    ctm: Matrix,
    line_width: Fixed,
    // The caps (J), the joins (j) with the miter limit (M), and the dash
    // array with its phase (d), in user space (docs/adr/0025, 0027). The
    // array is shared, so that saving the state allocates nothing.
    cap: Cap,
    line_join: i64,
    miter_limit: Fixed,
    dash: Arc<[Fixed]>,
    dash_phase: Fixed,
    fill: [u8; 3],
    fill_space: ColorSpace,
    stroke: [u8; 3],
    stroke_space: ColorSpace,
    // The index of the clip's mask in the workspace's stack, or none
    // (docs/adr/0027).
    clip: Option<usize>,
    // The text state (ISO 32000-1 9.3): the font and its size, the
    // horizontal scaling (Tz / 100) and the rendering mode.
    font: Option<Arc<Font>>,
    font_size: Fixed,
    h_scale: Fixed,
    render_mode: i64,
}

struct Renderer<'a> {
    doc: &'a Document,
    cache: &'a FontCache,
    pictures: &'a PictureCache,
    // The resources of the page, or of the form being drawn
    // (docs/adr/0027).
    resources: Cow<'a, Dict>,
    canvas: Canvas,
    // The size of the image, and its frame, beyond which a curve is one
    // segment (docs/adr/0022).
    width: usize,
    height: usize,
    frame: Frame,
    state: GraphicsState,
    stack: Vec<GraphicsState>,
    path: Path,
    // The text matrix, set by BT and Tm.
    text_matrix: Matrix,
    // The fonts of the resources, read once per page.
    fonts: BTreeMap<Vec<u8>, Arc<Font>>,
    // How many forms the content being drawn is within (docs/adr/0027).
    form_depth: usize,
    // Whether pixels are drawn; without it, the page is only interpreted.
    paint: bool,
    // The buffers that the glyphs, the strokes and the fills of the page
    // reuse (docs/adr/0013).
    glyph: Path,
    outline: Path,
    scratch: Scratch,
    segments: Vec<Segment>,
    // The disc and the dash of the strokes of the page's paths, and the
    // dash array in device space (docs/adr/0025).
    strokes: StrokeScratch,
    dash: Vec<i64>,
    // The page's path in user space, built beside the device path under
    // a CTM that does not keep angles, the outline of its stroke, and the
    // disc and the dash of those strokes (docs/adr/0027).
    user_path: PathOf<i64>,
    user_outline: PathOf<i64>,
    user_strokes: StrokeScratchOf<i64>,
    // The masks of the clips, and the rule of a W or W* waiting for the
    // operator that ends the path (docs/adr/0027).
    clips: ClipStack,
    pending_clip: Option<FillRule>,
    // The parallelogram of an image or the BBox of a form (docs/adr/0027).
    quad: Path,
}

impl Renderer<'_> {
    fn point(&self, x: Fixed, y: Fixed) -> Result<Point> {
        to_device(&self.state.ctm, x, y)
    }

    fn fill(&mut self, path: &Path, rgb: [u8; 3]) {
        self.fill_with(path, FillRule::NonZero, rgb);
    }

    /// Fills a path under a rule with a color, within the clip if there is
    /// one (docs/adr/0027).
    fn fill_with(&mut self, path: &Path, rule: FillRule, rgb: [u8; 3]) {
        if !self.paint {
            return;
        }
        match self.state.clip {
            None => self.canvas.fill(path, rule, rgb, &mut self.scratch),
            Some(i) => {
                self.canvas
                    .fill_within(path, rule, rgb, &mut self.scratch, self.clips.get(i))
            }
        }
    }

    /// Whether the user path is built beside the device path: the CTM
    /// does not keep angles, so a stroke would be built in user space
    /// (docs/adr/0027).
    fn builds_user_path(&self) -> Result<bool> {
        Ok(!keeps_angles(&self.state.ctm)?)
    }

    /// Narrows the clip to the path under the rule (docs/adr/0027): the
    /// new mask goes right after the mask of the state below, whose masks
    /// the state keeps; the masks above it are free.
    fn clip_to(&mut self, path: &Path, rule: FillRule) {
        if !self.paint {
            return;
        }
        let keep = self.stack.last().and_then(|s| s.clip).map_or(0, |i| i + 1);
        let index = self.clips.intersect(
            keep,
            self.state.clip,
            path,
            rule,
            self.width,
            self.height,
            &mut self.scratch,
        );
        self.state.clip = Some(index);
    }

    /// Fills the stroke of a path, `half` units on each side.
    fn fill_stroke(&mut self, path: &Path, half: i64, rgb: [u8; 3]) {
        if !self.paint {
            return;
        }
        let mut outline = std::mem::take(&mut self.outline);
        outline.clear();
        stroke_into(path, half, &mut outline);
        self.fill(&outline, rgb);
        self.outline = outline;
    }

    /// The frame of the flattening of a curve of the page's path
    /// (docs/adr/0025): with a dash array, none, since the dashes are
    /// measured along the flattened curve; with a solid line and a CTM
    /// that keeps angles, the margin of the stroke; otherwise the canvas
    /// alone, since a stroke would be an error and a fill does not depend
    /// on the margin (docs/adr/0022).
    fn curve_frame(&self) -> Frame {
        if !self.state.dash.is_empty() {
            return Frame::NONE;
        }
        match stroke_half_width(&self.state.ctm, self.state.line_width) {
            Ok(half) => Frame::around(self.width, self.height, stroke_margin(half)),
            Err(_) => self.frame,
        }
    }

    /// Strokes the page's path (`S`) with the line color, after the checks
    /// that find its errors without drawing (docs/adr/0025): the CTM keeps
    /// angles, the period of the dash array is at least 1/16 pixel, and
    /// the points with the margin of the stroke are within i32.
    fn stroke_path(&mut self, path: &Path) -> Result<()> {
        let scale = stroke_scale(&self.state.ctm)?;
        let half = half_width(self.state.line_width, scale)?;
        let mut dash = std::mem::take(&mut self.dash);
        dash.clear();
        let lengths = self
            .state
            .dash
            .iter()
            .try_for_each(|&v| dash_length(v, scale).map(|l| dash.push(l)));
        let result = lengths.and_then(|()| {
            // An odd array is its double (docs/adr/0025).
            let doubled = if dash.len() % 2 == 1 { 2 } else { 1 };
            let period: i128 = dash.iter().map(|&l| i128::from(l)).sum::<i128>() * doubled;
            if !dash.is_empty() && period < MIN_DASH_PERIOD {
                return Err(Error::new("dash pattern shorter than 1/16 pixel"));
            }
            let phase = if dash.is_empty() {
                0
            } else {
                (i128::from(dash_length(self.state.dash_phase, scale)?) % period) as i64
            };
            let margin = stroke_margin(half);
            let reach = i64::from(i32::MAX) - margin;
            let beyond = |v: i32| i64::from(v).abs() > reach;
            if path.points().iter().any(|&(x, y)| beyond(x) || beyond(y)) {
                return Err(Error::new("stroke outside the range of coordinates"));
            }
            if self.paint {
                let style = Stroke {
                    half,
                    cap: self.state.cap,
                    join: self.join(),
                    dash: &dash,
                    phase,
                    flatness: crate::raster::FLATNESS,
                };
                let frame = Frame::around(self.width, self.height, margin);
                let mut outline = std::mem::take(&mut self.outline);
                outline.clear();
                stroke_styled_into(path, &style, &frame, &mut self.strokes, &mut outline);
                let rgb = self.state.stroke;
                self.fill(&outline, rgb);
                self.outline = outline;
            }
            Ok(())
        });
        self.dash = dash;
        result
    }

    /// The join of the strokes of the state: `j` with the limit `M`
    /// (docs/adr/0027).
    fn join(&self) -> Join {
        match self.state.line_join {
            1 => Join::Round,
            2 => Join::Bevel,
            _ => Join::Miter(self.state.miter_limit.raw()),
        }
    }

    /// Strokes the page's path under a CTM that does not keep angles
    /// (docs/adr/0027): the outline is built in user space, in units of
    /// 2^−32, with the widths and the dashes as they are, and its points
    /// are mapped to the device as the points of a path are. The checks
    /// find the errors without drawing: a width of 0, a dash period too
    /// short for the bound of the scale, and points beyond the device.
    fn stroke_user_path(&mut self) -> Result<()> {
        if self.state.line_width == Fixed::ZERO {
            return Err(Error::new(
                "zero-width stroke with a CTM that does not keep angles",
            ));
        }
        let ctm = self.state.ctm;
        let bound = scale_bound(&ctm);
        let half = div_round_i64(self.state.line_width.raw(), 2).max(1);
        let mut dash = std::mem::take(&mut self.dash);
        dash.clear();
        dash.extend(self.state.dash.iter().map(|v| v.raw()));
        let result = (|| {
            let doubled = if dash.len() % 2 == 1 { 2 } else { 1 };
            let period: i128 = dash.iter().map(|&l| i128::from(l)).sum::<i128>() * doubled;
            if !dash.is_empty() && !user_period_is_long_enough(period, bound) {
                return Err(Error::new("dash pattern shorter than 1/16 pixel"));
            }
            let phase = if dash.is_empty() {
                0
            } else {
                (i128::from(self.state.dash_phase.raw()) % period) as i64
            };
            let beyond = |v: i64| v.abs() > USER_REACH;
            if self
                .user_path
                .points()
                .iter()
                .any(|&(x, y)| beyond(x) || beyond(y))
            {
                return Err(Error::new("stroke outside the range of coordinates"));
            }
            let style = Stroke {
                half,
                cap: self.state.cap,
                join: self.join(),
                dash: &dash,
                phase,
                flatness: user_flatness(bound),
            };
            let mut user_outline = std::mem::take(&mut self.user_outline);
            user_outline.clear();
            stroke_styled_into(
                &self.user_path,
                &style,
                &Frame::NONE,
                &mut self.user_strokes,
                &mut user_outline,
            );
            // The points are mapped without the checks when the bounds
            // show that they fit (docs/adr/0015).
            let extent = user_outline
                .points()
                .iter()
                .fold((0, 0), |(mx, my), &(x, y)| {
                    (mx.max(x.unsigned_abs()), my.max(y.unsigned_abs()))
                });
            let fits = fits_with_extent(&ctm, extent);
            let mut outline = std::mem::take(&mut self.outline);
            outline.clear();
            let mapped = user_outline.subpaths().try_for_each(|s| {
                s.points.iter().enumerate().try_for_each(|(i, &(x, y))| {
                    let (x, y) = (Fixed::from_raw(x), Fixed::from_raw(y));
                    let p = if fits {
                        to_device_fitting(&ctm, x, y)
                    } else {
                        to_device(&ctm, x, y)
                            .map_err(|_| Error::new("stroke outside the range of coordinates"))?
                    };
                    if i == 0 {
                        outline.move_to(p);
                    } else {
                        outline.line_to(p);
                    }
                    Ok::<_, Error>(())
                })?;
                outline.close();
                Ok(())
            });
            if mapped.is_ok() && self.paint {
                let rgb = self.state.stroke;
                self.fill(&outline, rgb);
            }
            self.outline = outline;
            self.user_outline = user_outline;
            mapped
        })();
        self.dash = dash;
        result
    }

    /// Strokes the page's path (`S`, `B`, `B*`) in the way of its CTM.
    fn stroke_current_path(&mut self) -> Result<()> {
        if self.builds_user_path()? {
            return self.stroke_user_path();
        }
        let path = std::mem::take(&mut self.path);
        let result = self.stroke_path(&path);
        self.path = path;
        result
    }

    /// Ends the page's path with a painting operator: fills it under the
    /// rule, strokes it, narrows the clip to it if a `W` or `W*` came
    /// before, and empties it (docs/adr/0027).
    fn end_path(&mut self, fill: Option<FillRule>, stroke: bool) -> Result<()> {
        let mut path = std::mem::take(&mut self.path);
        if let Some(rule) = fill {
            let rgb = self.state.fill;
            self.fill_with(&path, rule, rgb);
        }
        self.path = std::mem::take(&mut path);
        let stroked = if stroke {
            self.stroke_current_path()
        } else {
            Ok(())
        };
        if let Some(rule) = self.pending_clip.take()
            && stroked.is_ok()
        {
            let path = std::mem::take(&mut self.path);
            self.clip_to(&path, rule);
            self.path = path;
        }
        self.path.clear();
        self.user_path.clear();
        stroked
    }

    /// The font of a name of the resources, read at its first use.
    fn font(&mut self, name: &[u8]) -> Result<Arc<Font>> {
        if let Some(f) = self.fonts.get(name) {
            return Ok(f.clone());
        }
        let fonts = self
            .doc
            .get_dict(&self.resources, "Font")?
            .unwrap_or_default();
        let obj = fonts
            .get(name)
            .ok_or_else(|| Error::new("no such font in the resources"))?;
        let font = self.cache.get(self.doc, obj)?;
        self.fonts.insert(name.to_vec(), font.clone());
        Ok(font)
    }

    /// Shows a string: each two bytes are a CID (Identity-H and Identity-V).
    fn show_text(&mut self, s: &Text) -> Result<()> {
        let font = self
            .state
            .font
            .clone()
            .ok_or_else(|| Error::new("Tj without a font"))?;
        if !s.bytes().count().is_multiple_of(2) {
            return Err(Error::new("a string of odd length for a two-byte encoding"));
        }
        let mut bytes = s.bytes();
        std::iter::from_fn(|| Some(u16::from_be_bytes([bytes.next()?, bytes.next()?])))
            .try_for_each(|cid| self.show_glyph(&font, cid))
    }

    fn show_glyph(&mut self, font: &Font, cid: u16) -> Result<()> {
        let size = self.state.font_size;
        let thousandth = |v: Fixed| v.checked_div(Fixed::from_int(1000)?);
        // Trm = [Tfs·Th 0 0 Tfs 0 0] × Tm × CTM (ISO 32000-1 9.4.4)
        let scale = Matrix::new([
            size.checked_mul(self.state.h_scale)?,
            Fixed::ZERO,
            Fixed::ZERO,
            size,
            Fixed::ZERO,
            Fixed::ZERO,
        ]);
        let trm = scale.concat(&self.text_matrix)?.concat(&self.state.ctm)?;
        // In vertical writing, the glyph is placed back by its position vector (9.7.4.3).
        let (shift, advance) = if font.is_vertical() {
            let (w1, vx, vy) = font.vertical_metrics(cid)?;
            let shift = Matrix::translation(
                thousandth(vx)?.checked_neg()?,
                thousandth(vy)?.checked_neg()?,
            );
            (
                shift,
                Matrix::translation(Fixed::ZERO, thousandth(w1)?.checked_mul(size)?),
            )
        } else {
            let tx = thousandth(font.width(cid))?
                .checked_mul(size)?
                .checked_mul(self.state.h_scale)?;
            (Matrix::IDENTITY, Matrix::translation(tx, Fixed::ZERO))
        };
        let m = font.glyph_matrix()?.concat(&shift)?.concat(&trm)?;
        let mut path = std::mem::take(&mut self.glyph);
        path.clear();
        // Without painting, the points are only checked: making the path,
        // which cannot fail, is left to the rendering (docs/adr/0014).
        let paint = self.paint;
        let (outline, extent) = font.outline_and_extent(cid, &mut self.segments)?;
        // When the bounds show that every point fits, the check need not
        // map them, and the rendering maps them without the checks
        // (docs/adr/0015).
        let fits = fits_with_extent(&m, extent);
        // In mode 2, the glyph is stroked, and the points with the margin
        // of the stroke around them must stay within the device
        // (docs/adr/0022). An error of the width is reported after the
        // points, as before.
        let half = (self.state.render_mode == 2)
            .then(|| stroke_half_width(&self.state.ctm, self.state.line_width));
        let margin = match half {
            Some(Ok(half)) => stroke_margin(half),
            _ => 0,
        };
        let reach = i64::from(i32::MAX) - margin;
        let within = margin == 0 || fits_within(&m, extent, reach);
        let frame = Frame::around(self.width, self.height, margin);
        if paint || !fits || !within {
            let point = |x: Fixed, y: Fixed| {
                let p = if fits {
                    to_device_fitting(&m, x, y)
                } else {
                    to_device(&m, x, y)?
                };
                if !within && (i64::from(p.0).abs() > reach || i64::from(p.1).abs() > reach) {
                    return Err(Error::new("glyph stroke outside the range of coordinates"));
                }
                Ok(p)
            };
            outline.iter().try_for_each(|s| {
                match *s {
                    Segment::Move(x, y) => {
                        let p = point(x, y)?;
                        if paint {
                            path.move_to(p);
                        }
                    }
                    Segment::Line(x, y) => {
                        let p = point(x, y)?;
                        if paint {
                            path.line_to(p);
                        }
                    }
                    Segment::Curve([x1, y1, x2, y2, x3, y3]) => {
                        let (c1, c2, p) = (point(x1, y1)?, point(x2, y2)?, point(x3, y3)?);
                        if paint {
                            path.curve_to_within(c1, c2, p, &frame);
                        }
                    }
                    Segment::Close => path.close(),
                }
                Ok::<_, Error>(())
            })?;
        }
        // Each glyph is filled, then stroked in mode 2 (9.3.6).
        let fill = self.state.fill;
        self.fill(&path, fill);
        let stroked = match half {
            // The line width is in user space, so the CTM decides the width.
            Some(Ok(half)) => {
                let rgb = self.state.stroke;
                self.fill_stroke(&path, half, rgb);
                Ok(())
            }
            Some(Err(e)) => Err(e),
            None => Ok(()),
        };
        self.glyph = path;
        stroked?;
        self.text_matrix = advance.concat(&self.text_matrix)?;
        Ok(())
    }

    /// Draws the XObject of a name of the resources: an image or a form
    /// (docs/adr/0027). The errors of the XObject name its object.
    fn xobject(&mut self, name: &[u8]) -> Result<()> {
        // The entry is a reference, cloned without an allocation; the
        // dictionary of the resources is borrowed when it is direct, so
        // that placing an image allocates nothing (docs/adr/0013).
        let entry = match self.resources.get(b"XObject".as_slice()) {
            Some(Object::Dict(d)) => d.get(name).cloned(),
            _ => self
                .doc
                .get_dict(&self.resources, "XObject")?
                .unwrap_or_default()
                .get(name)
                .cloned(),
        };
        let entry = entry.ok_or_else(|| Error::new("no such XObject in the resources"))?;
        let num = match &entry {
            Object::Ref(r) => Some(r.num),
            _ => None,
        };
        let placed = match num.and_then(|n| self.pictures.cached(n)) {
            Some(picture) => self.place(&picture),
            None => self.load_xobject(num, &entry),
        };
        placed.map_err(|e| match num {
            Some(n) => e.at_object(n),
            None => e,
        })
    }

    /// Reads the XObject of `entry` and draws it.
    fn load_xobject(&mut self, num: Option<u32>, entry: &Object) -> Result<()> {
        let stream = match self.doc.resolve(entry)? {
            Object::Stream(s) => s,
            _ => return Err(Error::new("an XObject is not a stream")),
        };
        match self
            .doc
            .get(&stream.dict, "Subtype")?
            .as_ref()
            .and_then(Object::as_name)
        {
            Some(b"Image") => {
                let doc = self.doc;
                let picture = match num {
                    Some(n) => self.pictures.get(n, || picture::load(doc, &stream))?,
                    None => Arc::new(picture::load(doc, &stream)?),
                };
                self.place(&picture)
            }
            Some(b"Form") => self.form(&stream),
            _ => {
                let subtype = self
                    .doc
                    .get(&stream.dict, "Subtype")?
                    .map_or("none".into(), |o| o.describe());
                Err(Error::new(format!("unsupported XObject Subtype {subtype}")))
            }
        }
    }

    /// The path of the rectangle `[x0 y0 x1 y1]` under the CTM, in the
    /// buffer of the quadrilateral, and its corners.
    fn quad_of(&mut self, [x0, y0, x1, y1]: [Fixed; 4]) -> Result<[Point; 4]> {
        let corners = [
            self.point(x0, y0)?,
            self.point(x1, y0)?,
            self.point(x1, y1)?,
            self.point(x0, y1)?,
        ];
        self.quad.clear();
        self.quad.move_to(corners[0]);
        corners[1..].iter().for_each(|&c| self.quad.line_to(c));
        self.quad.close();
        Ok(corners)
    }

    /// Draws a decoded image: the unit square of image space under the
    /// CTM (ISO 32000-1 8.9.4), filled with the interpolated samples
    /// (docs/adr/0027). A singular CTM draws nothing.
    fn place(&mut self, picture: &Picture) -> Result<()> {
        let corners = self.quad_of([Fixed::ZERO, Fixed::ZERO, Fixed::ONE, Fixed::ONE])?;
        if signed_area(&corners) == 0 {
            return Ok(());
        }
        let mask = picture.mask.as_ref().map(|m| (m.width, m.height));
        let Some(mapping) = Mapping::new(&self.state.ctm, picture.width, picture.height, mask)?
        else {
            return Ok(());
        };
        if !self.paint {
            return Ok(());
        }
        let quad = std::mem::take(&mut self.quad);
        let clip = self.state.clip.map(|i| self.clips.get(i));
        picture::draw(
            &mut self.canvas,
            &quad,
            &mapping,
            picture,
            clip,
            &mut self.scratch,
        );
        self.quad = quad;
        Ok(())
    }

    /// Draws a form: `q`, its `Matrix` on the CTM, the clip of its `BBox`,
    /// its content with its `Resources`, `Q` (ISO 32000-1 8.10.1,
    /// docs/adr/0027).
    fn form(&mut self, stream: &Stream) -> Result<()> {
        let dict = &stream.dict;
        ["Group", "Ref", "OC"]
            .iter()
            .try_for_each(|key| match dict.get(key.as_bytes()) {
                Some(v) => Err(Error::new(format!("unsupported {key} {}", v.describe()))),
                None => Ok(()),
            })?;
        if let Some(t) = self.doc.get(dict, "FormType")?
            && t != Object::Int(1)
        {
            return Err(Error::new(format!("unsupported FormType {}", t.describe())));
        }
        let bbox = self
            .doc
            .get(dict, "BBox")?
            .ok_or_else(|| Error::new("a form without a BBox"))?;
        let rect = self.doc.rectangle(&bbox)?;
        let matrix = match self.doc.get(dict, "Matrix")? {
            None => Matrix::IDENTITY,
            Some(Object::Array(a)) if a.len() == 6 => {
                let v = a
                    .iter()
                    .map(|o| self.doc.number(o))
                    .collect::<Result<Vec<_>>>()?;
                Matrix::new([v[0], v[1], v[2], v[3], v[4], v[5]])
            }
            Some(other) => {
                return Err(Error::new(format!("malformed Matrix {}", other.describe())));
            }
        };
        let resources = self
            .doc
            .get_dict(dict, "Resources")?
            .ok_or_else(|| Error::new("a form without Resources"))?;
        if self.form_depth >= MAX_FORM_DEPTH {
            return Err(Error::new(format!(
                "forms nested deeper than {MAX_FORM_DEPTH}"
            )));
        }
        let content = self.doc.stream_data(stream)?;
        let ops = content::parse(&content)?;
        content::check_resources(self.doc, &resources, &ops)?;
        // q
        self.stack.push(self.state.clone());
        let depth = self.stack.len();
        let result = (|| {
            self.state.ctm = matrix.concat(&self.state.ctm)?;
            self.quad_of(rect)?;
            let quad = std::mem::take(&mut self.quad);
            self.clip_to(&quad, FillRule::NonZero);
            self.quad = quad;
            let saved = std::mem::replace(&mut self.resources, Cow::Owned(resources));
            let fonts = std::mem::take(&mut self.fonts);
            self.form_depth += 1;
            let drawn = ops.iter().try_for_each(|op| {
                if matches!(op.operator, Operator::BeginText) {
                    return Err(Error::new("text in a form").at_offset(op.offset));
                }
                self.op(&op.operator).map_err(|e| e.at_offset(op.offset))
            });
            self.form_depth -= 1;
            self.fonts = fonts;
            self.resources = saved;
            drawn
        })();
        // Q, whatever the content left on the stack
        self.stack.truncate(depth);
        self.state = self.stack.pop().expect("the state saved before the form");
        self.clips.truncate(self.state.clip.map_or(0, |i| i + 1));
        result
    }

    /// The color space of a name of the resources (`cs`, `CS`).
    fn color_space(&self, name: &[u8]) -> Result<ColorSpace> {
        let spaces = self
            .doc
            .get_dict(&self.resources, "ColorSpace")?
            .unwrap_or_default();
        let space = spaces
            .get(name)
            .ok_or_else(|| Error::new("no such color space in the resources"))?;
        ColorSpace::load(self.doc, space)
    }

    /// The point of user space as the user path holds it (docs/adr/0027).
    fn user_point(x: Fixed, y: Fixed) -> (i64, i64) {
        (x.raw(), y.raw())
    }

    fn op(&mut self, op: &Operator) -> Result<()> {
        match op {
            Operator::Save => self.stack.push(self.state.clone()),
            Operator::Restore => {
                self.state = self.stack.pop().ok_or_else(|| Error::new("Q without q"))?;
                // The masks above the restored state's are free (docs/adr/0027).
                self.clips.truncate(self.state.clip.map_or(0, |i| i + 1));
            }
            Operator::Concat(m) => self.state.ctm = m.concat(&self.state.ctm)?,
            Operator::LineWidth(w) => self.state.line_width = *w,
            Operator::MoveTo(x, y) => {
                let p = self.point(*x, *y)?;
                self.path.move_to(p);
                if self.builds_user_path()? {
                    self.user_path.move_to(Self::user_point(*x, *y));
                }
            }
            Operator::LineTo(x, y) => {
                let p = self.point(*x, *y)?;
                self.path.line_to(p);
                if self.builds_user_path()? {
                    self.user_path.line_to(Self::user_point(*x, *y));
                }
            }
            Operator::CurveTo([x1, y1, x2, y2, x3, y3]) => {
                let (c1, c2, p) = (
                    self.point(*x1, *y1)?,
                    self.point(*x2, *y2)?,
                    self.point(*x3, *y3)?,
                );
                let frame = self.curve_frame();
                self.path.curve_to_within(c1, c2, p, &frame);
                if self.builds_user_path()? {
                    let flatness = user_flatness(scale_bound(&self.state.ctm));
                    self.user_path.curve_to_flat(
                        Self::user_point(*x1, *y1),
                        Self::user_point(*x2, *y2),
                        Self::user_point(*x3, *y3),
                        flatness,
                    );
                }
            }
            Operator::ClosePath => {
                self.path.close();
                self.user_path.close();
            }
            Operator::Rect([x, y, w, h]) => {
                // x y m, x+w y l, x+w y+h l, x y+h l, h (ISO 32000-1 8.5.2.1)
                let (x1, y1) = (x.checked_add(*w)?, y.checked_add(*h)?);
                let corners = [
                    self.point(*x, *y)?,
                    self.point(x1, *y)?,
                    self.point(x1, y1)?,
                    self.point(*x, y1)?,
                ];
                self.path.move_to(corners[0]);
                corners[1..].iter().for_each(|&c| self.path.line_to(c));
                self.path.close();
                if self.builds_user_path()? {
                    let corners = [(*x, *y), (x1, *y), (x1, y1), (*x, y1)]
                        .map(|(x, y)| Self::user_point(x, y));
                    self.user_path.move_to(corners[0]);
                    corners[1..].iter().for_each(|&c| self.user_path.line_to(c));
                    self.user_path.close();
                }
            }
            Operator::Fill => self.end_path(Some(FillRule::NonZero), false)?,
            Operator::FillEvenOdd => self.end_path(Some(FillRule::EvenOdd), false)?,
            Operator::Stroke => self.end_path(None, true)?,
            Operator::FillStroke => self.end_path(Some(FillRule::NonZero), true)?,
            Operator::FillStrokeEvenOdd => self.end_path(Some(FillRule::EvenOdd), true)?,
            Operator::EndPath => self.end_path(None, false)?,
            Operator::Clip => self.pending_clip = Some(FillRule::NonZero),
            Operator::ClipEvenOdd => self.pending_clip = Some(FillRule::EvenOdd),
            Operator::LineCap(cap) => {
                self.state.cap = match cap {
                    1 => Cap::Round,
                    2 => Cap::Square,
                    _ => Cap::Butt,
                }
            }
            Operator::LineJoin(join) => self.state.line_join = *join,
            Operator::MiterLimit(limit) => self.state.miter_limit = *limit,
            Operator::Dash(array, phase) => {
                self.state.dash = array.clone();
                self.state.dash_phase = *phase;
            }
            Operator::FillCmyk(v) => {
                self.state.fill_space = ColorSpace::Device(Device::Cmyk);
                self.state.fill = color::cmyk(*v);
            }
            Operator::StrokeCmyk(v) => {
                self.state.stroke_space = ColorSpace::Device(Device::Cmyk);
                self.state.stroke = color::cmyk(*v);
            }
            Operator::FillSpace(name) => {
                let space = self.color_space(name)?;
                self.state.fill = space.initial()?;
                self.state.fill_space = space;
            }
            Operator::StrokeSpace(name) => {
                let space = self.color_space(name)?;
                self.state.stroke = space.initial()?;
                self.state.stroke_space = space;
            }
            Operator::FillComponents(c) => {
                self.state.fill = self.state.fill_space.components("sc", c.as_slice())?;
            }
            Operator::StrokeComponents(c) => {
                self.state.stroke = self.state.stroke_space.components("SC", c.as_slice())?;
            }
            Operator::FillColor(tint) => match &self.state.fill_space {
                ColorSpace::Separation(s) => self.state.fill = s.rgb(*tint)?,
                ColorSpace::Device(_) => {
                    return Err(Error::new(
                        "unsupported scn with a color space other than Separation",
                    ));
                }
            },
            Operator::BeginMarked | Operator::EndMarked => {}
            Operator::BeginText => self.text_matrix = Matrix::IDENTITY,
            Operator::EndText => {}
            Operator::Font(name, size) => {
                self.state.font = Some(self.font(name)?);
                self.state.font_size = *size;
            }
            Operator::HScale(v) => self.state.h_scale = v.checked_div(Fixed::from_int(100)?)?,
            Operator::RenderMode(m) => self.state.render_mode = *m,
            Operator::TextMatrix(m) => self.text_matrix = *m,
            Operator::ShowText(s) => self.show_text(s)?,
            Operator::Do(name) => self.xobject(name)?,
        }
        Ok(())
    }
}

/// Renders a page at `dpi`. The content is checked before anything is drawn.
pub fn render_page(doc: &Document, page: &Page, dpi: u32) -> Result<Canvas> {
    Session::single(doc, page, dpi).render(0)
}

/// Renders a page with the fonts of a cache shared by the pages of the
/// document (docs/adr/0011).
pub fn render_page_with(
    doc: &Document,
    page: &Page,
    dpi: u32,
    cache: &FontCache,
) -> Result<Canvas> {
    let mut ws = Workspace::default();
    let pictures = PictureCache::new();
    interpret(doc, page, dpi, true, (cache, &pictures), &mut ws)?;
    Ok(ws.canvas)
}

/// Interprets a page without drawing pixels: the content, the resources,
/// the fonts, the glyphs, the colors and the XObjects are read and checked
/// as `render_page` reads them (docs/adr/0002, docs/adr/0007).
pub fn check_page(doc: &Document, page: &Page, dpi: u32) -> Result<()> {
    Session::single(doc, page, dpi).check(0)
}

/// Checks a page with the fonts of a cache shared by the pages of the
/// document (docs/adr/0011).
pub fn check_page_with(doc: &Document, page: &Page, dpi: u32, cache: &FontCache) -> Result<()> {
    let pictures = PictureCache::new();
    interpret(
        doc,
        page,
        dpi,
        false,
        (cache, &pictures),
        &mut Workspace::default(),
    )
}

/// The buffers that a thread keeps between the pages it checks and renders
/// (docs/adr/0014): the operators, the stack of graphics states, the paths
/// of device space and of user space, the cells and pixels of filling, the
/// masks of the clips (docs/adr/0027), and the image.
#[derive(Default)]
pub struct Workspace {
    ops: Vec<Op<'static>>,
    stack: Vec<GraphicsState>,
    scratch: Scratch,
    glyph: Path,
    outline: Path,
    path: Path,
    canvas: Canvas,
    segments: Vec<Segment>,
    strokes: StrokeScratch,
    dash: Vec<i64>,
    user_path: PathOf<i64>,
    user_outline: PathOf<i64>,
    user_strokes: StrokeScratchOf<i64>,
    /// The masks of the clips of the last page (docs/adr/0027).
    pub(crate) clips: ClipStack,
    quad: Path,
}

/// An empty `Vec` of operators of another content, in the allocation of
/// `ops` when the standard library reuses it.
fn recycle<'b>(mut ops: Vec<Op<'_>>) -> Vec<Op<'b>> {
    ops.clear();
    ops.into_iter().map(|_| unreachable!()).collect()
}

fn interpret(
    doc: &Document,
    page: &Page,
    dpi: u32,
    paint: bool,
    caches: Caches,
    ws: &mut Workspace,
) -> Result<()> {
    let at_page = |e: Error| e.at_page(page.index + 1);
    let mut ops: Vec<Op> = recycle(std::mem::take(&mut ws.ops));
    let result =
        interpret_ops((doc, dpi, caches), page, paint, ws, &mut ops, true).map_err(at_page);
    ws.ops = recycle(ops);
    result
}

/// The caches of the fonts and of the decoded images (docs/adr/0027).
type Caches<'d> = (&'d FontCache, &'d PictureCache);

/// What every page of a rendering shares: the document, the resolution and
/// the caches.
type Context<'d> = (&'d Document, u32, Caches<'d>);

fn interpret_ops<'c>(
    (doc, dpi, (cache, pictures)): Context,
    page: &'c Page,
    paint: bool,
    ws: &mut Workspace,
    ops: &mut Vec<Op<'c>>,
    parse: bool,
) -> Result<()> {
    // Operators parsed by the check of the page are given as they are.
    if parse {
        content::parse_into(&page.contents, ops)?;
    }
    content::check_resources(doc, &page.resources, ops)?;
    let (width, height) = page_size(&page.media_box, dpi)?;
    let mut canvas = std::mem::take(&mut ws.canvas);
    if paint {
        canvas.reset(width, height);
    }
    let mut renderer = Renderer {
        doc,
        cache,
        pictures,
        resources: Cow::Borrowed(&page.resources),
        canvas,
        width,
        height,
        frame: Frame::around(width, height, 0),
        paint,
        glyph: std::mem::take(&mut ws.glyph),
        outline: std::mem::take(&mut ws.outline),
        scratch: std::mem::take(&mut ws.scratch),
        segments: std::mem::take(&mut ws.segments),
        strokes: std::mem::take(&mut ws.strokes),
        dash: std::mem::take(&mut ws.dash),
        user_path: std::mem::take(&mut ws.user_path),
        user_outline: std::mem::take(&mut ws.user_outline),
        user_strokes: std::mem::take(&mut ws.user_strokes),
        clips: std::mem::take(&mut ws.clips),
        pending_clip: None,
        quad: std::mem::take(&mut ws.quad),
        state: GraphicsState {
            ctm: device_matrix(&page.media_box, dpi)?,
            line_width: Fixed::ONE,
            cap: Cap::Butt,
            line_join: 0,
            miter_limit: Fixed::from_int(10)?,
            dash: Arc::from([]),
            dash_phase: Fixed::ZERO,
            fill: [0, 0, 0],
            fill_space: ColorSpace::Device(Device::Gray),
            stroke: [0, 0, 0],
            stroke_space: ColorSpace::Device(Device::Gray),
            clip: None,
            font: None,
            font_size: Fixed::ZERO,
            h_scale: Fixed::ONE,
            render_mode: 0,
        },
        stack: std::mem::take(&mut ws.stack),
        path: std::mem::take(&mut ws.path),
        text_matrix: Matrix::IDENTITY,
        fonts: BTreeMap::new(),
        form_depth: 0,
    };
    renderer.stack.clear();
    renderer.path.clear();
    renderer.user_path.clear();
    renderer.clips.clear();
    let result = ops.iter().try_for_each(|op| {
        renderer
            .op(&op.operator)
            .map_err(|e| e.at_offset(op.offset))
    });
    ws.canvas = renderer.canvas;
    ws.glyph = renderer.glyph;
    ws.outline = renderer.outline;
    ws.scratch = renderer.scratch;
    ws.stack = renderer.stack;
    ws.path = renderer.path;
    ws.segments = renderer.segments;
    ws.strokes = renderer.strokes;
    ws.dash = renderer.dash;
    ws.user_path = renderer.user_path;
    ws.user_outline = renderer.user_outline;
    ws.user_strokes = renderer.user_strokes;
    ws.clips = renderer.clips;
    ws.quad = renderer.quad;
    result
}

/// A document to render at one resolution, with the cache of its fonts
/// shared by its pages, the check and the rendering, and the threads
/// (docs/adr/0011, docs/adr/0012). Pages are numbered from 0, as
/// [`Page::index`].
pub struct Session<'a> {
    doc: &'a Document,
    pages: Pages<'a>,
    dpi: u32,
    fonts: FontCache,
    /// The decoded images, shared by the pages, the check and the
    /// rendering (docs/adr/0027).
    pictures: PictureCache,
    /// The operators of each page that the check parsed, until the page is
    /// rendered; only for pages borrowed by [`Session::from_pages`], whose
    /// contents outlive the session (docs/adr/0015).
    parsed: Vec<Mutex<Option<Vec<Op<'a>>>>>,
}

/// The pages of a session: its own, or borrowed.
enum Pages<'a> {
    Owned(Vec<Page>),
    Borrowed(&'a [Page]),
}

impl Pages<'_> {
    fn as_slice(&self) -> &[Page] {
        match self {
            Pages::Owned(v) => v,
            Pages::Borrowed(s) => s,
        }
    }
}

/// An error of [`Session::render_pages`].
#[derive(Debug)]
pub enum PageError<E> {
    /// An error of the rendering; the error names its page.
    Render(Error),
    /// The error that `consume` returned for a page.
    Consume(usize, E),
}

impl<'a> Session<'a> {
    pub fn new(doc: &'a Document, dpi: u32) -> Result<Session<'a>> {
        Session::with_jobs(doc, dpi, NonZeroUsize::MIN)
    }

    /// [`Session::new`], with the contents of the pages decoded on at most
    /// `jobs` threads ([`Document::pages_with`]).
    pub fn with_jobs(doc: &'a Document, dpi: u32, jobs: NonZeroUsize) -> Result<Session<'a>> {
        Ok(Session {
            doc,
            pages: Pages::Owned(doc.pages_with(jobs)?),
            dpi,
            fonts: FontCache::new(),
            pictures: PictureCache::new(),
            parsed: Vec::new(),
        })
    }

    /// A session of pages read before ([`Document::pages_with`]), which it
    /// borrows. The check of a page keeps the operators it parsed, and the
    /// rendering of the page uses them instead of parsing the content again
    /// (docs/adr/0015). The results are those of [`Session::new`].
    pub fn from_pages(doc: &'a Document, pages: &'a [Page], dpi: u32) -> Session<'a> {
        Session {
            doc,
            pages: Pages::Borrowed(pages),
            dpi,
            fonts: FontCache::new(),
            pictures: PictureCache::new(),
            parsed: pages.iter().map(|_| Mutex::new(None)).collect(),
        }
    }

    /// A session of one page, which is its page 0.
    fn single(doc: &'a Document, page: &Page, dpi: u32) -> Session<'a> {
        Session {
            doc,
            pages: Pages::Owned(vec![page.clone()]),
            dpi,
            fonts: FontCache::new(),
            pictures: PictureCache::new(),
            parsed: Vec::new(),
        }
    }

    pub fn page_count(&self) -> usize {
        self.pages.as_slice().len()
    }

    fn no_page(&self, page: usize) -> Error {
        Error::new(format!(
            "page {} does not exist: the document has {} pages",
            page + 1,
            self.pages.as_slice().len()
        ))
    }

    fn page(&self, page: usize) -> Result<&Page> {
        self.pages
            .as_slice()
            .get(page)
            .ok_or_else(|| self.no_page(page))
    }

    /// Checks one page, as [`check_page`].
    pub fn check(&self, page: usize) -> Result<()> {
        check_page_with(self.doc, self.page(page)?, self.dpi, &self.fonts)
    }

    /// Renders one page, as [`render_page`].
    pub fn render(&self, page: usize) -> Result<Canvas> {
        render_page_with(self.doc, self.page(page)?, self.dpi, &self.fonts)
    }

    /// Checks one page in the buffers of `ws`.
    pub fn check_with(&self, ws: &mut Workspace, page: usize) -> Result<()> {
        let Pages::Borrowed(pages) = self.pages else {
            return interpret(
                self.doc,
                self.page(page)?,
                self.dpi,
                false,
                (&self.fonts, &self.pictures),
                ws,
            );
        };
        let p: &'a Page = pages.get(page).ok_or_else(|| self.no_page(page))?;
        let mut ops = Vec::new();
        interpret_ops(
            (self.doc, self.dpi, (&self.fonts, &self.pictures)),
            p,
            false,
            ws,
            &mut ops,
            true,
        )
        .map_err(|e| e.at_page(p.index + 1))?;
        let mut slot = self.parsed[page].lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(ops);
        }
        Ok(())
    }

    /// Renders one page in the buffers of `ws`; the image stays in `ws`
    /// until the next page.
    pub fn render_with<'w>(&self, ws: &'w mut Workspace, page: usize) -> Result<&'w Canvas> {
        if let Pages::Borrowed(pages) = self.pages {
            let p: &'a Page = pages.get(page).ok_or_else(|| self.no_page(page))?;
            let taken = self.parsed[page]
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            if let Some(mut ops) = taken {
                interpret_ops(
                    (self.doc, self.dpi, (&self.fonts, &self.pictures)),
                    p,
                    true,
                    ws,
                    &mut ops,
                    false,
                )
                .map_err(|e| e.at_page(p.index + 1))?;
                return Ok(&ws.canvas);
            }
        }
        interpret(
            self.doc,
            self.page(page)?,
            self.dpi,
            true,
            (&self.fonts, &self.pictures),
            ws,
        )?;
        Ok(&ws.canvas)
    }

    /// Checks the pages on at most `jobs` threads, and returns the error of
    /// the first page of `pages` that fails, whatever the threads do
    /// ([`crate::parallel::run`]).
    pub fn check_pages(&self, pages: &[usize], jobs: NonZeroUsize) -> Result<()> {
        pages.iter().try_for_each(|&p| self.page(p).map(|_| ()))?;
        crate::parallel::run_with(pages.len(), jobs, Workspace::default, |ws, k| {
            self.check_with(ws, pages[k])
        })
        .map_err(|(_, e)| e)
    }

    /// Renders the pages on at most `jobs` threads and gives each image to
    /// `consume` on the thread that rendered it, once per page and in any
    /// order. After an error, no new page is started; the error returned is
    /// the one of the first page of `pages` that failed, whatever the threads
    /// do ([`crate::parallel::run`]).
    pub fn render_pages<E, F>(
        &self,
        pages: &[usize],
        jobs: NonZeroUsize,
        consume: F,
    ) -> std::result::Result<(), PageError<E>>
    where
        E: Send,
        F: Fn(usize, &Canvas) -> std::result::Result<(), E> + Sync,
    {
        pages
            .iter()
            .try_for_each(|&p| self.page(p).map(|_| ()))
            .map_err(PageError::Render)?;
        crate::parallel::run_with(pages.len(), jobs, Workspace::default, |ws, k| {
            let canvas = self.render_with(ws, pages[k]).map_err(PageError::Render)?;
            consume(pages[k], canvas).map_err(|e| PageError::Consume(pages[k], e))
        })
        .map_err(|(_, e)| e)
    }
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
pub(crate) mod image_tests;

#[cfg(test)]
mod form_tests;

#[cfg(test)]
mod stroke_tests;

#[cfg(test)]
mod session_tests;

#[cfg(test)]
mod join_tests;

#[cfg(test)]
mod affine_tests;

#[cfg(test)]
mod clip_tests;
