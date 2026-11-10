//! Colors: DeviceGray, DeviceCMYK and Separation (docs/adr/0005), RGB and
//! ICCBased by its number of components (docs/adr/0027).
//!
//! Colors become 8-bit RGB by the conversions of ISO 32000-1 10.3, without
//! ICC profiles.

use crate::error::{Error, Result};
use crate::fixed::{Fixed, round_shift};
use crate::pdf::{Document, Object};

/// A component from 0 to 1 as 0 to 255: clamped, then `round(x × 255)`, ties to even.
pub fn component(v: Fixed) -> u8 {
    let v = v.clamp(Fixed::ZERO, Fixed::ONE);
    round_shift(i128::from(v.raw()) * 255, 32) as u8
}

/// DeviceGray to RGB (10.3.2).
pub fn gray(g: Fixed) -> [u8; 3] {
    let v = component(g);
    [v, v, v]
}

/// DeviceCMYK to RGB (10.3.5): `R = 255 − min(255, C + K)` and so on.
pub fn cmyk(v: [Fixed; 4]) -> [u8; 3] {
    let [c, m, y, k] = v.map(|x| u16::from(component(x)));
    [c, m, y].map(|x| (255 - (x + k).min(255)) as u8)
}

/// RGB to 8-bit RGB: each component as [`component`] rounds it
/// (docs/adr/0027).
pub fn rgb(v: [Fixed; 3]) -> [u8; 3] {
    v.map(component)
}

/// A device color space, or an ICCBased one, which is read as the device
/// space of its number of components (docs/adr/0027).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Gray,
    Rgb,
    Cmyk,
}

impl Device {
    /// The number of components.
    pub fn components(self) -> usize {
        match self {
            Device::Gray => 1,
            Device::Rgb => 3,
            Device::Cmyk => 4,
        }
    }

    /// The RGB of the components, which are as many as [`Device::components`].
    pub fn rgb(self, values: &[Fixed]) -> [u8; 3] {
        match self {
            Device::Gray => gray(values[0]),
            Device::Rgb => rgb([values[0], values[1], values[2]]),
            Device::Cmyk => cmyk([values[0], values[1], values[2], values[3]]),
        }
    }

    /// The RGB of 8-bit components, as many as [`Device::components`]:
    /// the samples of an image (docs/adr/0027). CMYK goes by
    /// `R = 255 − min(255, C + K)` (docs/adr/0005).
    pub fn rgb8(self, v: &[u8]) -> [u8; 3] {
        match self {
            Device::Gray => [v[0]; 3],
            Device::Rgb => [v[0], v[1], v[2]],
            Device::Cmyk => {
                let k = u16::from(v[3]);
                [v[0], v[1], v[2]].map(|c| (255 - (u16::from(c) + k).min(255)) as u8)
            }
        }
    }

    /// The device space of an ICCBased space of `n` components.
    fn of_icc(n: i64) -> Option<Device> {
        match n {
            1 => Some(Device::Gray),
            3 => Some(Device::Rgb),
            4 => Some(Device::Cmyk),
            _ => None,
        }
    }

    /// Reads the color space of an image (docs/adr/0027): the names
    /// `DeviceGray` and `DeviceCMYK`, or `[/ICCBased stream]` by its `N`.
    /// `DeviceRGB` and the other spaces are errors.
    pub fn load(doc: &Document, obj: &Object) -> Result<Device> {
        let space = doc.resolve(obj)?;
        match &space {
            Object::Name(n) if n == b"DeviceGray" => Ok(Device::Gray),
            Object::Name(n) if n == b"DeviceCMYK" => Ok(Device::Cmyk),
            Object::Array(_) => match ColorSpace::load(doc, &space)? {
                ColorSpace::Device(d) => Ok(d),
                ColorSpace::Separation(_) => {
                    Err(Error::new("unsupported ColorSpace Separation of an image"))
                }
            },
            other => Err(Error::new(format!(
                "unsupported ColorSpace {}",
                other.describe()
            ))),
        }
    }
}

/// A Separation color space with a Type 2 function of exponent 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Separation {
    alternate: Device,
    domain: (Fixed, Fixed),
    c0: Vec<Fixed>,
    c1: Vec<Fixed>,
}

impl Separation {
    /// The RGB of a tint: `C0 + t × (C1 − C0)` in the alternate space (7.10.3).
    pub fn rgb(&self, tint: Fixed) -> Result<[u8; 3]> {
        let t = tint.clamp(self.domain.0, self.domain.1);
        let values = self
            .c0
            .iter()
            .zip(&self.c1)
            .map(|(&a, &b)| a.checked_add(t.checked_mul(b.checked_sub(a)?)?))
            .collect::<Result<Vec<_>>>()?;
        Ok(self.alternate.rgb(&values))
    }

    /// Reads `[/Separation name alternate function]` (8.6.6.4).
    pub fn load(doc: &Document, obj: &Object) -> Result<Separation> {
        match ColorSpace::load(doc, obj)? {
            ColorSpace::Separation(s) => Ok(s),
            ColorSpace::Device(_) => Err(Error::new("not a Separation color space")),
        }
    }

    /// Reads the items of `[/Separation name alternate function]`.
    fn load_items(doc: &Document, items: &[Object]) -> Result<Separation> {
        let [_, _, alternate, function] = items else {
            return Err(Error::new("malformed Separation color space"));
        };
        let alternate = match doc.resolve(alternate)?.as_name() {
            Some(b"DeviceGray") => Device::Gray,
            Some(b"DeviceCMYK") => Device::Cmyk,
            _ => {
                return Err(Error::new(format!(
                    "unsupported alternate color space {}",
                    doc.resolve(alternate)?.describe()
                )));
            }
        };
        let function = doc
            .resolve(function)?
            .as_dict()
            .cloned()
            .ok_or_else(|| Error::new("the tint transform is not a dictionary"))?;
        match doc.get(&function, "FunctionType")? {
            Some(Object::Int(2)) => {}
            Some(other) => {
                return Err(Error::new(format!(
                    "unsupported FunctionType {}",
                    other.describe()
                )));
            }
            None => return Err(Error::new("the tint transform has no FunctionType")),
        }
        let exponent = doc
            .get(&function, "N")?
            .ok_or_else(|| Error::new("the tint transform has no N"))?;
        if doc.number(&exponent)? != Fixed::ONE {
            return Err(Error::new(format!("unsupported N {}", exponent.describe())));
        }
        let numbers = |key: &str, default: Fixed| -> Result<Vec<Fixed>> {
            match doc.get(&function, key)? {
                None => Ok(vec![default]),
                Some(Object::Array(a)) => a.iter().map(|v| doc.number(v)).collect(),
                Some(other) => Err(Error::new(format!(
                    "{key} is not an array: {}",
                    other.describe()
                ))),
            }
        };
        let c0 = numbers("C0", Fixed::ZERO)?;
        let c1 = numbers("C1", Fixed::ONE)?;
        let n = alternate.components();
        if c0.len() != n || c1.len() != n {
            return Err(Error::new(
                "C0 or C1 does not have the components of the alternate color space",
            ));
        }
        let domain = match numbers("Domain", Fixed::ZERO)?.as_slice() {
            [lo, hi] => (*lo, *hi),
            _ => {
                return Err(Error::new(
                    "the tint transform has no Domain of two numbers",
                ));
            }
        };
        Ok(Separation {
            alternate,
            domain,
            c0,
            c1,
        })
    }
}

/// A color space of the fill or the stroke, as shem reads it: a device
/// space (DeviceGray, DeviceCMYK, or an ICCBased space by its number of
/// components) or a Separation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    Device(Device),
    Separation(Separation),
}

impl ColorSpace {
    /// Reads a color space of the resources: `[/ICCBased stream]` by the
    /// `N` of the stream (docs/adr/0027), or `[/Separation …]`
    /// (docs/adr/0005).
    pub fn load(doc: &Document, obj: &Object) -> Result<ColorSpace> {
        let space = doc.resolve(obj)?;
        let items = match &space {
            Object::Array(a) => a.as_slice(),
            other => {
                return Err(Error::new(format!(
                    "unsupported ColorSpace {}",
                    other.describe()
                )));
            }
        };
        let family = items.first().and_then(Object::as_name).unwrap_or_default();
        match family {
            b"Separation" => Separation::load_items(doc, items).map(ColorSpace::Separation),
            b"ICCBased" => {
                let [_, stream] = items else {
                    return Err(Error::new("malformed ICCBased color space"));
                };
                let dict = match doc.resolve(stream)? {
                    Object::Stream(s) => s.dict,
                    _ => return Err(Error::new("malformed ICCBased color space")),
                };
                let n = doc
                    .get(&dict, "N")?
                    .ok_or_else(|| Error::new("an ICCBased color space without N"))?;
                let device = n.as_int().and_then(Device::of_icc).ok_or_else(|| {
                    Error::new(format!("unsupported ICCBased with N {}", n.describe()))
                })?;
                Ok(ColorSpace::Device(device))
            }
            _ => Err(Error::new(format!(
                "unsupported ColorSpace {}",
                String::from_utf8_lossy(family)
            ))),
        }
    }

    /// The color after the space is selected: black (ISO 32000-1
    /// 8.6.3), or the tint 1 of a Separation (8.6.6.4).
    pub fn initial(&self) -> Result<[u8; 3]> {
        match self {
            ColorSpace::Device(_) => Ok([0, 0, 0]),
            ColorSpace::Separation(s) => s.rgb(Fixed::ONE),
        }
    }

    /// The RGB of the components of `sc` or `SC` (`op`), which must be as
    /// many as the device space has (docs/adr/0027).
    pub fn components(&self, op: &str, values: &[Fixed]) -> Result<[u8; 3]> {
        match self {
            ColorSpace::Device(d) if d.components() == values.len() => Ok(d.rgb(values)),
            ColorSpace::Device(d) => Err(Error::new(format!(
                "{op} with {} components for a color space of {}",
                values.len(),
                d.components()
            ))),
            ColorSpace::Separation(_) => Err(Error::new(format!(
                "unsupported {op} with a Separation color space"
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
