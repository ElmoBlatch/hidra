//! IFeatureSet (0x0001).
use crate::{Device, FeatureEntry, FeatureFlags, Result, Transport};

pub const PAGE: u16 = 0x0001;

/// Number of features, excluding the root feature.
pub fn count<T: Transport>(dev: &mut Device<T>, fs_index: u8) -> Result<u8> {
    Ok(dev.call(fs_index, 0, &[0, 0, 0])?[0])
}

pub fn feature_id<T: Transport>(dev: &mut Device<T>, fs_index: u8, index: u8) -> Result<FeatureEntry> {
    let r = dev.call(fs_index, 1, &[index, 0, 0])?;
    Ok(FeatureEntry {
        index,
        page: u16::from_be_bytes([r[0], r[1]]),
        flags: FeatureFlags(r[2]),
        version: r[3],
    })
}
