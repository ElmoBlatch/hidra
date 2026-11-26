//! Battery voltage (0x1001).
use crate::{Device, Result, Transport};

pub const PAGE: u16 = 0x1001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryVoltage {
    pub millivolts: u16,
    pub flags: u8,
}

impl BatteryVoltage {
    pub fn charging(self) -> bool {
        self.flags & 0x80 != 0
    }
}

pub fn voltage<T: Transport>(dev: &mut Device<T>) -> Result<BatteryVoltage> {
    let idx = dev.require(PAGE)?;
    let r = dev.call(idx, 0, &[0, 0, 0])?;
    Ok(BatteryVoltage {
        millivolts: u16::from_be_bytes([r[0], r[1]]),
        flags: r[2],
    })
}
