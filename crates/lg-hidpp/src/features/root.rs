//! IRoot (0x0000), always at feature index 0.
use crate::{Device, Result, Transport};

pub const PAGE: u16 = 0x0000;

/// `GetFeature(page)`: returns the feature index, or `None` if absent.
pub fn feature_index<T: Transport>(dev: &mut Device<T>, page: u16) -> Result<Option<u8>> {
    let [hi, lo] = page.to_be_bytes();
    let r = dev.call(0x00, 0, &[hi, lo, 0x00])?;
    Ok((r[0] != 0 || page == PAGE).then_some(r[0]))
}

/// `GetProtocolVersion()`: (major, minor). The ping byte is left at zero.
pub fn protocol_version<T: Transport>(dev: &mut Device<T>) -> Result<(u8, u8)> {
    let r = dev.call(0x00, 1, &[0x00, 0x00, 0x00])?;
    Ok((r[0], r[1]))
}
