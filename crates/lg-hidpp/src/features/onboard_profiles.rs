//! Onboard profiles (0x8100), read operations only.
//!
//! Function numbers follow LGS 9.04.49 `Feature8100OnboardProfiles` and
//! libratbag: 0 GetInfo, 2 GetMode, 4 GetActiveProfile, 5 ReadData,
//! 0xB GetActiveResolutionIndex. Write functions (1, 3, 6, 7, 8, 0xC) are
//! intentionally not implemented here.
use crate::{Device, Error, Result, Transport};

pub const PAGE: u16 = 0x8100;

/// Raw 16-byte `GetInfo` reply; decode with `lg_profiles::Descriptor`.
pub fn info_raw<T: Transport>(dev: &mut Device<T>) -> Result<[u8; 16]> {
    let idx = dev.require(PAGE)?;
    let r = dev.call(idx, 0, &[0, 0, 0])?;
    Ok(r[..16].try_into().expect("payload is 16 bytes"))
}

/// Onboard mode: 1 = onboard (device-stored profiles), 2 = host mode.
pub fn mode<T: Transport>(dev: &mut Device<T>) -> Result<u8> {
    let idx = dev.require(PAGE)?;
    Ok(dev.call(idx, 2, &[0, 0, 0])?[0])
}

/// Active profile address. User profiles report their directory sector
/// (1-based); factory ROM profiles report `0x0100 | n`.
pub fn active_profile<T: Transport>(dev: &mut Device<T>) -> Result<u16> {
    let idx = dev.require(PAGE)?;
    let r = dev.call(idx, 4, &[0, 0, 0])?;
    Ok(u16::from_be_bytes([r[0], r[1]]))
}

/// Index of the active DPI stage within the active profile.
pub fn active_resolution_index<T: Transport>(dev: &mut Device<T>) -> Result<u8> {
    let idx = dev.require(PAGE)?;
    Ok(dev.call(idx, 0xb, &[0, 0, 0])?[0])
}

/// Offsets of the 16-byte reads that cover a sector. Reads must not run past
/// the sector end, so for sizes that are not a multiple of 16 the final read
/// overlaps the previous one (e.g. 255-byte sectors end with a read at 0xEF).
pub fn read_offsets(sector_size: u16) -> Vec<u16> {
    let size = sector_size as usize;
    let mut offsets: Vec<u16> = (0..size)
        .step_by(16)
        .filter(|o| o + 16 <= size)
        .map(|o| o as u16)
        .collect();
    if !size.is_multiple_of(16) && size >= 16 {
        offsets.push((size - 16) as u16);
    }
    offsets
}

/// Read one full sector by issuing `ReadData` for each 16-byte block.
pub fn read_sector<T: Transport>(
    dev: &mut Device<T>,
    sector: u16,
    sector_size: u16,
) -> Result<Vec<u8>> {
    if sector_size < 16 {
        return Err(Error::Malformed(format!(
            "sector size {sector_size} is smaller than one read"
        )));
    }
    let idx = dev.require(PAGE)?;
    let mut out = vec![0u8; sector_size as usize];
    let [s_hi, s_lo] = sector.to_be_bytes();
    for off in read_offsets(sector_size) {
        let [a_hi, a_lo] = off.to_be_bytes();
        let r = dev.call(idx, 5, &[s_hi, s_lo, a_hi, a_lo])?;
        out[off as usize..off as usize + 16].copy_from_slice(&r[..16]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::read_offsets;

    #[test]
    fn offsets_256() {
        let o = read_offsets(256);
        assert_eq!(o.len(), 16);
        assert_eq!(*o.last().unwrap(), 0xf0);
    }

    #[test]
    fn offsets_255_overlap_final_block() {
        let o = read_offsets(255);
        assert_eq!(&o[o.len() - 2..], &[0xe0, 0xef]);
    }
}
