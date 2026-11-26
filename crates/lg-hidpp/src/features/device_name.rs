//! Device name and type (0x0005).
use crate::{Device, Result, Transport};

pub const PAGE: u16 = 0x0005;

pub fn name<T: Transport>(dev: &mut Device<T>) -> Result<String> {
    let idx = dev.require(PAGE)?;
    let len = dev.call(idx, 0, &[0, 0, 0])?[0] as usize;
    let mut bytes = Vec::with_capacity(len);
    while bytes.len() < len {
        let r = dev.call(idx, 1, &[bytes.len() as u8, 0, 0])?;
        let take = (len - bytes.len()).min(16);
        bytes.extend_from_slice(&r[..take]);
    }
    Ok(String::from_utf8_lossy(&bytes).trim_end_matches('\0').to_string())
}
