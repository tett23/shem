//! Interpreting a page's content into an image (docs/adr/0003).

use crate::color::{self, Device, FillSpace, Separation};
use crate::content::{self, Op, Operator};
use crate::error::{Error, Result};
use crate::fixed::{Fixed, Matrix};
use crate::font::{Font, FontCache, Segment};
use crate::pdf::Object;
use crate::pdf::{Dict, Document, Page};
use crate::raster::{Canvas, FillRule, Path, Point, coverage, signed_area, stroke};

/// The fill and the lines of the box of an image or a form (docs/adr/0006).
const BOX_FILL: [u8; 3] = [192, 192, 192];
const BOX_LINE: [u8; 3] = [96, 96, 96];
use std::collections::BTreeMap;
use std::sync::Arc;

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

/// Half the width of a stroke in device space, in units of 1/256 pixel. The
/// CTM keeps angles; a line width of 0 is one pixel (ISO 32000-1 8.4.3.2).
pub fn stroke_half_width(ctm: &Matrix, width: Fixed) -> Result<i64> {
    let keeps_angles = (ctm.a == ctm.d && ctm.b.checked_neg()? == ctm.c)
        || (ctm.a.checked_neg()? == ctm.d && ctm.b == ctm.c);
    if !keeps_angles {
        return Err(Error::new(
            "unsupported stroke with a CTM that does not keep angles",
        ));
    }
    if width == Fixed::ZERO {
        return Ok(128);
    }
    let scale = ctm.det()?.abs()?.sqrt()?;
    // The width in pixels, in units of 2^-7, is half of it in units of 1/256.
    Ok(width.checked_mul(scale)?.to_units(7).max(1))
}

#[derive(Clone)]
struct GraphicsState {
    ctm: Matrix,
    line_width: Fixed,
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
}

impl Renderer<'_> {
    fn point(&self, x: Fixed, y: Fixed) -> Result<Point> {
        to_device(&self.state.ctm, x, y)
    }

    fn fill(&mut self, path: &Path, rgb: [u8; 3]) {
        if !self.paint {
            return;
        }
        let cov = coverage(
            path,
            FillRule::NonZero,
            self.canvas.width,
            self.canvas.height,
        );
        self.canvas.paint(&cov, rgb);
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
    fn show_text(&mut self, s: &[u8]) -> Result<()> {
        let font = self
            .state
            .font
            .clone()
            .ok_or_else(|| Error::new("Tj without a font"))?;
        if !s.len().is_multiple_of(2) {
            return Err(Error::new("a string of odd length for a two-byte encoding"));
        }
        s.chunks(2).try_for_each(|pair| {
            let cid = u16::from_be_bytes([pair[0], pair[1]]);
            self.show_glyph(&font, cid)
        })
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
        let mut path = Path::new();
        font.outline(cid)?.iter().try_for_each(|s| {
            match *s {
                Segment::Move(x, y) => path.move_to(to_device(&m, x, y)?),
                Segment::Line(x, y) => path.line_to(to_device(&m, x, y)?),
                Segment::Curve([x1, y1, x2, y2, x3, y3]) => path.curve_to(
                    to_device(&m, x1, y1)?,
                    to_device(&m, x2, y2)?,
                    to_device(&m, x3, y3)?,
                ),
                Segment::Close => path.close(),
            }
            Ok::<_, Error>(())
        })?;
        // Each glyph is filled, then stroked in mode 2 (9.3.6).
        let fill = self.state.fill;
        self.fill(&path, fill);
        if self.state.render_mode == 2 {
            // The line width is in user space, so the CTM decides the width.
            let half = stroke_half_width(&self.state.ctm, self.state.line_width)?;
            let rgb = self.state.stroke;
            self.fill(&stroke(&path, half), rgb);
        }
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
        self.fill(&stroke(&cross, self.point_half_width), BOX_LINE);
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
            Operator::CurveTo([x1, y1, x2, y2, x3, y3]) => {
                let (c1, c2, p) = (
                    self.point(*x1, *y1)?,
                    self.point(*x2, *y2)?,
                    self.point(*x3, *y3)?,
                );
                self.path.curve_to(c1, c2, p);
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
                let path = std::mem::take(&mut self.path);
                let rgb = self.state.fill;
                self.fill(&path, rgb);
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
                    .get(name)
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
    render_page_with(doc, page, dpi, &FontCache::new())
}

/// Renders a page with the fonts of a cache shared by the pages of the
/// document (docs/adr/0011).
pub fn render_page_with(
    doc: &Document,
    page: &Page,
    dpi: u32,
    cache: &FontCache,
) -> Result<Canvas> {
    interpret(doc, page, dpi, true, cache)
}

/// Interprets a page without drawing pixels: the content, the resources,
/// the fonts, the glyphs, the colors and the XObjects are read and checked
/// as `render_page` reads them (docs/adr/0002, docs/adr/0007).
pub fn check_page(doc: &Document, page: &Page, dpi: u32) -> Result<()> {
    check_page_with(doc, page, dpi, &FontCache::new())
}

/// Checks a page with the fonts of a cache shared by the pages of the
/// document (docs/adr/0011).
pub fn check_page_with(doc: &Document, page: &Page, dpi: u32, cache: &FontCache) -> Result<()> {
    interpret(doc, page, dpi, false, cache).map(|_| ())
}

fn interpret(
    doc: &Document,
    page: &Page,
    dpi: u32,
    paint: bool,
    cache: &FontCache,
) -> Result<Canvas> {
    let at_page = |e: Error| e.at_page(page.index + 1);
    let ops: Vec<Op> = content::parse(&page.contents).map_err(at_page)?;
    content::check_resources(doc, &page.resources, &ops).map_err(at_page)?;
    let (width, height) = page_size(&page.media_box, dpi).map_err(at_page)?;
    let mut renderer = Renderer {
        doc,
        cache,
        resources: &page.resources,
        canvas: if paint {
            Canvas::new(width, height)
        } else {
            Canvas::new(0, 0)
        },
        paint,
        state: GraphicsState {
            ctm: device_matrix(&page.media_box, dpi).map_err(at_page)?,
            line_width: Fixed::ONE,
            fill: [0, 0, 0],
            fill_space: FillSpace::Device(Device::Gray),
            stroke: [0, 0, 0],
            font: None,
            font_size: Fixed::ZERO,
            h_scale: Fixed::ONE,
            render_mode: 0,
        },
        stack: Vec::new(),
        path: Path::new(),
        text_matrix: Matrix::IDENTITY,
        fonts: BTreeMap::new(),
        point_half_width: Fixed::from_int(i64::from(dpi))
            .and_then(|d| d.checked_div(Fixed::from_int(72)?))
            .map_err(at_page)?
            .to_units(7),
    };
    ops.iter().try_for_each(|op| {
        renderer
            .op(&op.operator)
            .map_err(|e| at_page(e.at_offset(op.offset)))
    })?;
    Ok(renderer.canvas)
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod box_tests;
