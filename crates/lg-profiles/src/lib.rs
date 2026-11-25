//! Codec for Logitech HID++ onboard-profile (0x8100) storage.
//!
//! Layout source: libratbag `hidpp20_internal_profile`, checked byte-for-byte
//! against read-only captures of a G502 Proteus Spectrum (format 2, 256-byte
//! sectors) and a G502 LIGHTSPEED ROM profile (format 3, 255-byte sectors).
//!
//! Decoding never discards bytes: a [`Profile`] keeps the full raw sector, and
//! edits patch only their own fields before the CRC is recomputed. This is the
//! basis for the "preserve every byte outside the intended field" rule.

mod button;
mod crc;
mod descriptor;
mod directory;
mod led;
mod profile;

pub use button::{ButtonBinding, MouseButtons, SpecialAction, hid_key_name, hid_key_usage};
pub use crc::crc_ccitt;
pub use descriptor::Descriptor;
pub use directory::{Directory, DirectoryEntry};
pub use led::{Led, LedMode};
pub use profile::{DPI_SLOTS, DpiLimits, EditLimits, Profile, ProfileEdit, diff_offsets};

/// A raw onboard-memory sector with its trailing big-endian CRC-CCITT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sector {
    pub address: u16,
    pub bytes: Vec<u8>,
}

impl Sector {
    pub fn new(address: u16, bytes: Vec<u8>) -> Self {
        Self { address, bytes }
    }

    pub fn stored_crc(&self) -> Option<u16> {
        let n = self.bytes.len();
        (n >= 2).then(|| u16::from_be_bytes([self.bytes[n - 2], self.bytes[n - 1]]))
    }

    pub fn computed_crc(&self) -> Option<u16> {
        let n = self.bytes.len();
        (n >= 2).then(|| crc_ccitt(&self.bytes[..n - 2]))
    }

    pub fn crc_valid(&self) -> bool {
        self.stored_crc().is_some() && self.stored_crc() == self.computed_crc()
    }

    /// Entirely `0xff`: never written.
    pub fn is_erased(&self) -> bool {
        self.bytes.iter().all(|&b| b == 0xff)
    }

    /// Entirely `0x00`: the Proteus Spectrum returns this for allocated but
    /// unused (macro) sectors.
    pub fn is_zeroed(&self) -> bool {
        self.bytes.iter().all(|&b| b == 0)
    }

    /// Factory ROM profiles live at `0x0100 | n`.
    pub fn is_rom(&self) -> bool {
        self.address & 0xff00 == 0x0100
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("sector is {actual} bytes; at least {needed} are required")]
    TooShort { needed: usize, actual: usize },
    #[error("DPI stage {stage} is out of range (profile has {DPI_SLOTS} slots)")]
    NoSuchStage { stage: usize },
    #[error("DPI {dpi} is outside the device range {min}..={max}")]
    DpiOutOfRange { dpi: u16, min: u16, max: u16 },
    #[error("DPI {dpi} is not a multiple of the {step}-DPI step")]
    DpiNotOnStep { dpi: u16, step: u16 },
    #[error("polling rate {hz} Hz is not supported (supported: {supported:?})")]
    UnsupportedReportRate { hz: u16, supported: Vec<u16> },
    #[error("button {index} does not exist (the device has {count})")]
    NoSuchButton { index: usize, count: usize },
    #[error("this kind of binding (macro or unknown) can be kept but not assigned")]
    BindingNotAssignable,
    #[error("the default DPI stage ({}) would be disabled", stage + 1)]
    DefaultStageDisabled { stage: usize },
    #[error("unrecognised action \"{0}\"")]
    UnknownAction(String),
}
