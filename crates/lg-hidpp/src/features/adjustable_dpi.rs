//! Adjustable DPI (0x2201). Live sensor resolution, independent of the DPI
//! stages stored in onboard profiles.
use crate::{Device, Result, Transport};

pub const PAGE: u16 = 0x2201;

pub fn sensor_count<T: Transport>(dev: &mut Device<T>) -> Result<u8> {
    let idx = dev.require(PAGE)?;
    Ok(dev.call(idx, 0, &[0, 0, 0])?[0])
}

/// Supported DPI values, as either a discrete list or ranges with a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DpiList {
    Discrete(Vec<u16>),
    Range { min: u16, max: u16, step: u16 },
}

/// `GetSensorDpiList(sensor)`. Values are big-endian u16; a value with the top
/// three bits set (`0xE000 | step`) means "range from the previous value to
/// the next value in `step` increments".
pub fn dpi_list<T: Transport>(dev: &mut Device<T>, sensor: u8) -> Result<DpiList> {
    let idx = dev.require(PAGE)?;
    let r = dev.call(idx, 1, &[sensor, 0, 0])?;
    Ok(parse_dpi_list(&r[1..]))
}

pub(crate) fn parse_dpi_list(words: &[u8]) -> DpiList {
    let vals: Vec<u16> = words
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .take_while(|&v| v != 0)
        .collect();
    if let Some(pos) = vals.iter().position(|v| v & 0xe000 == 0xe000)
        && pos > 0
        && pos + 1 < vals.len()
    {
        return DpiList::Range {
            min: vals[pos - 1],
            max: vals[pos + 1],
            step: vals[pos] & 0x1fff,
        };
    }
    DpiList::Discrete(vals)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SensorDpi {
    pub current: u16,
    pub default: u16,
}

pub fn sensor_dpi<T: Transport>(dev: &mut Device<T>, sensor: u8) -> Result<SensorDpi> {
    let idx = dev.require(PAGE)?;
    let r = dev.call(idx, 2, &[sensor, 0, 0])?;
    Ok(SensorDpi {
        current: u16::from_be_bytes([r[1], r[2]]),
        default: u16::from_be_bytes([r[3], r[4]]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_list() {
        // 50..12000 step 50, as reported by the Proteus Spectrum.
        let bytes = [0x00, 0x32, 0xe0, 0x32, 0x2e, 0xe0, 0, 0];
        assert_eq!(
            parse_dpi_list(&bytes),
            DpiList::Range {
                min: 50,
                max: 12000,
                step: 50
            }
        );
    }
}
