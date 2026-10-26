//! Interpreting a page's content into an image (docs/adr/0003).

use crate::color::{self, Device, FillSpace, Separation};
use crate::content::{self, Op, Operator};
use crate::error::{Error, Result};
use crate::fixed::{Fixed, Matrix};
use crate::pdf::{Dict, Document, Page};
use crate::raster::{Canvas, FillRule, Path, Point, coverage};

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
    // Read by the stroke of text (docs/adr/0004).
    #[allow(dead_code)]
    stroke: [u8; 3],
}

struct Renderer<'a> {
    doc: &'a Document,
    resources: &'a Dict,
    canvas: Canvas,
    state: GraphicsState,
    stack: Vec<GraphicsState>,
    path: Path,
}

impl Renderer<'_> {
    fn point(&self, x: Fixed, y: Fixed) -> Result<Point> {
        to_device(&self.state.ctm, x, y)
    }

    fn fill(&mut self, path: &Path, rgb: [u8; 3]) {
        let cov = coverage(
            path,
            FillRule::NonZero,
            self.canvas.width,
            self.canvas.height,
        );
        self.canvas.paint(&cov, rgb);
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
            other => return Err(Error::new(format!("not implemented yet: {other:?}"))),
        }
        Ok(())
    }
}

/// Renders a page at `dpi`. The content is checked before anything is drawn.
pub fn render_page(doc: &Document, page: &Page, dpi: u32) -> Result<Canvas> {
    let at_page = |e: Error| e.at_page(page.index + 1);
    let ops: Vec<Op> = content::parse(&page.contents).map_err(at_page)?;
    content::check_resources(doc, &page.resources, &ops).map_err(at_page)?;
    let (width, height) = page_size(&page.media_box, dpi).map_err(at_page)?;
    let mut renderer = Renderer {
        doc,
        resources: &page.resources,
        canvas: Canvas::new(width, height),
        state: GraphicsState {
            ctm: device_matrix(&page.media_box, dpi).map_err(at_page)?,
            line_width: Fixed::ONE,
            fill: [0, 0, 0],
            fill_space: FillSpace::Device(Device::Gray),
            stroke: [0, 0, 0],
        },
        stack: Vec::new(),
        path: Path::new(),
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
