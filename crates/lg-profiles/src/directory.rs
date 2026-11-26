use crate::Sector;

/// One entry of the user-profile directory in sector 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryEntry {
    /// Logical profile number (position in the directory).
    pub profile: u8,
    /// Sector holding the profile (1-based for user profiles).
    pub sector: u16,
    pub enabled: bool,
}

/// The profile directory: 4-byte entries `[sector_hi, sector_lo, enabled, 0]`
/// terminated by `ff ff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    pub entries: Vec<DirectoryEntry>,
}

impl Directory {
    /// Parse sector 0. Returns `None` if the sector is erased or its CRC is
    /// invalid, in which case the device falls back to its ROM profiles.
    pub fn parse(sector: &Sector) -> Option<Self> {
        if sector.is_erased() || !sector.crc_valid() {
            return None;
        }
        let body = &sector.bytes[..sector.bytes.len() - 2];
        let mut entries = Vec::new();
        for (i, e) in body.as_chunks::<4>().0.iter().enumerate() {
            let s = u16::from_be_bytes([e[0], e[1]]);
            if s == 0xffff {
                break;
            }
            entries.push(DirectoryEntry { profile: i as u8, sector: s, enabled: e[2] != 0 });
        }
        Some(Self { entries })
    }

    /// Map an `0x8100 GetActiveProfile` address to a logical profile number.
    /// The address is the profile's sector, not its directory position; this
    /// is the off-by-one that libratbag's `INDEX_OFFSET` quirk corrects.
    pub fn profile_for_address(&self, address: u16) -> Option<u8> {
        self.entries.iter().find(|e| e.sector == address).map(|e| e.profile)
    }
}
