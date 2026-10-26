//! Colors: DeviceGray, DeviceCMYK and Separation (docs/adr/0005).
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

/// A device color space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Gray,
    Cmyk,
}

impl Device {
    fn components(self) -> usize {
        match self {
            Device::Gray => 1,
            Device::Cmyk => 4,
        }
    }

    fn rgb(self, values: &[Fixed]) -> [u8; 3] {
        match self {
            Device::Gray => gray(values[0]),
            Device::Cmyk => cmyk([values[0], values[1], values[2], values[3]]),
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
        let space = doc.resolve(obj)?;
        let items = match &space {
            Object::Array(a) => a.clone(),
            other => {
                return Err(Error::new(format!(
                    "unsupported ColorSpace {}",
                    other.describe()
                )));
            }
        };
        let family = items.first().and_then(Object::as_name).unwrap_or_default();
        if family != b"Separation" {
            return Err(Error::new(format!(
                "unsupported ColorSpace {}",
                String::from_utf8_lossy(family)
            )));
        }
        let [_, _, alternate, function] = items.as_slice() else {
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

/// The color space of the fill, as v0.1 reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FillSpace {
    Device(Device),
    Separation(Separation),
}

#[cfg(test)]
mod tests;
