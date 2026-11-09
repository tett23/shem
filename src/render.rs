//! Interpreting a page's content into an image (docs/adr/0003).

use crate::color::{self, Device, FillSpace, Separation};
use crate::content::{self, Op, Operator, Text};
use crate::error::{Error, Result};
use crate::fixed::{Fixed, Matrix, round_shift};
use crate::font::{Font, FontCache, Segment};
use crate::pdf::Object;
use crate::pdf::{Dict, Document, Page};
use crate::raster::{
    Canvas, DASH_UNIT, FillRule, Frame, Path, Point, Scratch, Stroke, StrokeScratch, signed_area,
    stroke_into, stroke_styled_into,
};

/// The fill and the lines of the box of an image or a form (docs/adr/0006).
const BOX_FILL: [u8; 3] = [192, 192, 192];
const BOX_LINE: [u8; 3] = [96, 96, 96];
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

/// The largest number of pixels of a page (docs/adr/0007).
const MAX_PIXELS: u128 = 1 << 28;

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

/// The scale of a stroke: `sqrt(|det CTM|)`, by which the widths and the
/// dashes of user space go to device space. The CTM keeps angles; another
/// is an error (docs/adr/0003).
fn stroke_scale(ctm: &Matrix) -> Result<Fixed> {
    let keeps_angles = (ctm.a == ctm.d && ctm.b.checked_neg()? == ctm.c)
        || (ctm.a.checked_neg()? == ctm.d && ctm.b == ctm.c);
    if !keeps_angles {
        return Err(Error::new(
            "unsupported stroke with a CTM that does not keep angles",
        ));
    }
    ctm.det()?.abs()?.sqrt()
}

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
    // Round caps (1 J) and the dash array with its phase (d), in user
    // space (docs/adr/0025). The array is shared, so that saving the state
    // allocates nothing.
    round_cap: bool,
    dash: Arc<[Fixed]>,
    dash_phase: Fixed,
    fill: [u8; 3],
    fill_space: FillSpace,
    stroke: [u8; 3],
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
    resources: &'a Dict,
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
    // Half of 1 point in device space, in units of 1/256 pixel.
    point_half_width: i64,
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
}

impl Renderer<'_> {
    fn point(&self, x: Fixed, y: Fixed) -> Result<Point> {
        to_device(&self.state.ctm, x, y)
    }

    fn fill(&mut self, path: &Path, rgb: [u8; 3]) {
        if !self.paint {
            return;
        }
        self.canvas
            .fill(path, FillRule::NonZero, rgb, &mut self.scratch);
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
                    round_cap: self.state.round_cap,
                    dash: &dash,
                    phase,
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

    /// The font of a name of the resources, read at its first use.
    fn font(&mut self, name: &[u8]) -> Result<Arc<Font>> {
        if let Some(f) = self.fonts.get(name) {
            return Ok(f.clone());
        }
        let fonts = self
            .doc
            .get_dict(self.resources, "Font")?
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

    /// Draws an image or a form as a gray box with a cross (docs/adr/0006).
    /// The contents of the XObject are not read.
    fn placeholder(&mut self, name: &[u8]) -> Result<()> {
        let xobjects = self
            .doc
            .get_dict(self.resources, "XObject")?
            .unwrap_or_default();
        let obj = xobjects
            .get(name)
            .ok_or_else(|| Error::new("no such XObject in the resources"))?;
        let dict = self
            .doc
            .resolve(obj)?
            .as_dict()
            .cloned()
            .ok_or_else(|| Error::new("an XObject is not a stream"))?;
        let (rect, matrix) = match self
            .doc
            .get(&dict, "Subtype")?
            .as_ref()
            .and_then(Object::as_name)
        {
            // The unit square of image space (ISO 32000-1 8.9.4)
            Some(b"Image") => (
                [Fixed::ZERO, Fixed::ZERO, Fixed::ONE, Fixed::ONE],
                Matrix::IDENTITY,
            ),
            // The BBox mapped by the Matrix of the form (8.10.1)
            Some(b"Form") => {
                let bbox = self
                    .doc
                    .get(&dict, "BBox")?
                    .ok_or_else(|| Error::new("a form without a BBox"))?;
                let matrix = match self.doc.get(&dict, "Matrix")? {
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
                (self.doc.rectangle(&bbox)?, matrix)
            }
            _ => {
                let subtype = self
                    .doc
                    .get(&dict, "Subtype")?
                    .map_or("none".into(), |o| o.describe());
                return Err(Error::new(format!("unsupported XObject Subtype {subtype}")));
            }
        };
        let m = matrix.concat(&self.state.ctm)?;
        let [x0, y0, x1, y1] = rect;
        let corners = [
            to_device(&m, x0, y0)?,
            to_device(&m, x1, y0)?,
            to_device(&m, x1, y1)?,
            to_device(&m, x0, y1)?,
        ];
        if signed_area(&corners) == 0 {
            return Ok(());
        }
        let mut box_path = Path::new();
        box_path.move_to(corners[0]);
        corners[1..].iter().for_each(|&c| box_path.line_to(c));
        box_path.close();
        self.fill(&box_path, BOX_FILL);
        let mut cross = Path::new();
        cross.move_to(corners[0]);
        cross.line_to(corners[2]);
        cross.move_to(corners[1]);
        cross.line_to(corners[3]);
        self.fill_stroke(&cross, self.point_half_width, BOX_LINE);
        Ok(())
    }

    fn op(&mut self, op: &Operator) -> Result<()> {
        match op {
            Operator::Save => self.stack.push(self.state.clone()),
            Operator::Restore => {
                self.state = self.stack.pop().ok_or_else(|| Error::new("Q without q"))?;
            }
            Operator::Concat(m) => self.state.ctm = m.concat(&self.state.ctm)?,
            Operator::LineWidth(w) => self.state.line_width = *w,
            Operator::MoveTo(x, y) => {
                let p = self.point(*x, *y)?;
                self.path.move_to(p);
            }
            Operator::LineTo(x, y) => {
                let p = self.point(*x, *y)?;
                self.path.line_to(p);
            }
            Operator::CurveTo([x1, y1, x2, y2, x3, y3]) => {
                let (c1, c2, p) = (
                    self.point(*x1, *y1)?,
                    self.point(*x2, *y2)?,
                    self.point(*x3, *y3)?,
                );
                let frame = self.curve_frame();
                self.path.curve_to_within(c1, c2, p, &frame);
            }
            Operator::ClosePath => self.path.close(),
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
            }
            Operator::Fill => {
                let mut path = std::mem::take(&mut self.path);
                let rgb = self.state.fill;
                self.fill(&path, rgb);
                path.clear();
                self.path = path;
            }
            Operator::Stroke => {
                let mut path = std::mem::take(&mut self.path);
                let result = self.stroke_path(&path);
                path.clear();
                self.path = path;
                result?;
            }
            Operator::LineCap(cap) => self.state.round_cap = *cap == 1,
            Operator::Dash(array, phase) => {
                self.state.dash = array.clone();
                self.state.dash_phase = *phase;
            }
            Operator::FillCmyk(v) => {
                self.state.fill_space = FillSpace::Device(Device::Cmyk);
                self.state.fill = color::cmyk(*v);
            }
            Operator::StrokeCmyk(v) => self.state.stroke = color::cmyk(*v),
            Operator::FillSpace(name) => {
                let spaces = self
                    .doc
                    .get_dict(self.resources, "ColorSpace")?
                    .unwrap_or_default();
                let space = spaces
                    .get(name.as_ref())
                    .ok_or_else(|| Error::new("no such color space in the resources"))?;
                let separation = Separation::load(self.doc, space)?;
                // The initial color of a Separation is the tint 1 (8.6.6.4).
                self.state.fill = separation.rgb(Fixed::ONE)?;
                self.state.fill_space = FillSpace::Separation(separation);
            }
            Operator::FillColor(tint) => match &self.state.fill_space {
                FillSpace::Separation(s) => self.state.fill = s.rgb(*tint)?,
                FillSpace::Device(_) => {
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
            Operator::Do(name) => self.placeholder(name)?,
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
    interpret(doc, page, dpi, true, cache, &mut ws)?;
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
    interpret(doc, page, dpi, false, cache, &mut Workspace::default())
}

/// The buffers that a thread keeps between the pages it checks and renders
/// (docs/adr/0014): the operators, the stack of graphics states, the paths,
/// the cells and pixels of filling, and the image.
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
    cache: &FontCache,
    ws: &mut Workspace,
) -> Result<()> {
    let at_page = |e: Error| e.at_page(page.index + 1);
    let mut ops: Vec<Op> = recycle(std::mem::take(&mut ws.ops));
    let result = interpret_ops((doc, dpi, cache), page, paint, ws, &mut ops, true).map_err(at_page);
    ws.ops = recycle(ops);
    result
}

/// What every page of a rendering shares: the document, the resolution and
/// the cache of the fonts.
type Context<'d> = (&'d Document, u32, &'d FontCache);

fn interpret_ops<'c>(
    (doc, dpi, cache): Context,
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
        resources: &page.resources,
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
        state: GraphicsState {
            ctm: device_matrix(&page.media_box, dpi)?,
            line_width: Fixed::ONE,
            round_cap: false,
            dash: Arc::from([]),
            dash_phase: Fixed::ZERO,
            fill: [0, 0, 0],
            fill_space: FillSpace::Device(Device::Gray),
            stroke: [0, 0, 0],
            font: None,
            font_size: Fixed::ZERO,
            h_scale: Fixed::ONE,
            render_mode: 0,
        },
        stack: std::mem::take(&mut ws.stack),
        path: std::mem::take(&mut ws.path),
        text_matrix: Matrix::IDENTITY,
        fonts: BTreeMap::new(),
        point_half_width: Fixed::from_int(i64::from(dpi))
            .and_then(|d| d.checked_div(Fixed::from_int(72)?))?
            .to_units(7),
    };
    renderer.stack.clear();
    renderer.path.clear();
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
            return interpret(self.doc, self.page(page)?, self.dpi, false, &self.fonts, ws);
        };
        let p: &'a Page = pages.get(page).ok_or_else(|| self.no_page(page))?;
        let mut ops = Vec::new();
        interpret_ops(
            (self.doc, self.dpi, &self.fonts),
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
                    (self.doc, self.dpi, &self.fonts),
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
        interpret(self.doc, self.page(page)?, self.dpi, true, &self.fonts, ws)?;
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
mod box_tests;

#[cfg(test)]
mod stroke_tests;

#[cfg(test)]
mod session_tests;
