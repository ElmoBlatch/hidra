//! Device information / firmware version (0x0003).
use crate::{Device, Result, Transport};

pub const PAGE: u16 = 0x0003;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub entity_count: u8,
    pub unit_id: [u8; 4],
    pub transport: u16,
    pub model_id: [u8; 6],
}

pub fn device_info<T: Transport>(dev: &mut Device<T>) -> Result<DeviceInfo> {
    let idx = dev.require(PAGE)?;
    let r = dev.call(idx, 0, &[0, 0, 0])?;
    Ok(DeviceInfo {
        entity_count: r[0],
        unit_id: [r[1], r[2], r[3], r[4]],
        transport: u16::from_be_bytes([r[5], r[6]]),
        model_id: [r[7], r[8], r[9], r[10], r[11], r[12]],
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareInfo {
    pub entity: u8,
    /// 0 main application, 1 bootloader, 2 hardware, 3+ other.
    pub kind: u8,
    pub prefix: String,
    /// BCD-encoded firmware number, revision and build.
    pub number: u8,
    pub revision: u8,
    pub build: u16,
    pub active: bool,
    pub transport_pid: u16,
}

impl FirmwareInfo {
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            0 => "application",
            1 => "bootloader",
            2 => "hardware",
            _ => "other",
        }
    }

    /// Logitech's usual rendering, e.g. `U1 12.03.B0011`.
    pub fn version_string(&self) -> String {
        format!("{} {:02x}.{:02x}.B{:04x}", self.prefix, self.number, self.revision, self.build)
    }
}

pub fn firmware_info<T: Transport>(dev: &mut Device<T>, entity: u8) -> Result<FirmwareInfo> {
    let idx = dev.require(PAGE)?;
    let r = dev.call(idx, 1, &[entity, 0, 0])?;
    let prefix: String = r[1..4].iter().filter(|b| b.is_ascii_graphic()).map(|&b| b as char).collect();
    Ok(FirmwareInfo {
        entity,
        kind: r[0] & 0x0f,
        prefix,
        number: r[4],
        revision: r[5],
        build: u16::from_be_bytes([r[6], r[7]]),
        active: r[8] & 0x01 != 0,
        transport_pid: u16::from_be_bytes([r[9], r[10]]),
    })
}
