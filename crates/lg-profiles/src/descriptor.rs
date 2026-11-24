/// Decoded `0x8100 GetInfo` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Descriptor {
    pub memory_model: u8,
    pub profile_format: u8,
    pub macro_format: u8,
    /// Writable user-profile slots.
    pub profile_count: u8,
    /// Factory ROM ("out of box") profiles.
    pub rom_profile_count: u8,
    pub button_count: u8,
    pub sector_count: u8,
    pub sector_size: u16,
    pub mechanical_layout: u8,
    pub various_info: u8,
}

impl Descriptor {
    pub fn parse(info: &[u8]) -> Option<Self> {
        if info.len() < 11 {
            return None;
        }
        Some(Self {
            memory_model: info[0],
            profile_format: info[1],
            macro_format: info[2],
            profile_count: info[3],
            rom_profile_count: info[4],
            button_count: info[5],
            sector_count: info[6],
            sector_size: u16::from_be_bytes([info[7], info[8]]),
            mechanical_layout: info[9],
            various_info: info[10],
        })
    }

    /// Profile formats this codec's layout is known to apply to.
    pub fn layout_supported(&self) -> bool {
        (1..=5).contains(&self.profile_format) && self.sector_size >= 252
    }

    /// Mechanical layout bit 1: the device has a G-shift / second-function mode.
    pub fn has_g_shift(&self) -> bool {
        self.mechanical_layout & 0x02 != 0
    }

    /// Mechanical layout bit 3: the device has a DPI-shift (sniper) button.
    pub fn has_dpi_shift(&self) -> bool {
        self.mechanical_layout & 0x08 != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proteus_spectrum() {
        let d = Descriptor::parse(&[0x01, 0x02, 0x01, 0x03, 0x03, 0x0b, 0x10, 0x01, 0x00, 0x0a, 0x01]).unwrap();
        assert_eq!((d.profile_format, d.profile_count, d.rom_profile_count), (2, 3, 3));
        assert_eq!((d.button_count, d.sector_count, d.sector_size), (11, 16, 256));
        assert!(d.layout_supported());
    }

    #[test]
    fn lightspeed() {
        let d = Descriptor::parse(&[0x01, 0x03, 0x01, 0x05, 0x01, 0x0b, 0x10, 0x00, 0xff, 0x0a, 0x04]).unwrap();
        assert_eq!((d.profile_format, d.profile_count, d.rom_profile_count), (3, 5, 1));
        assert_eq!(d.sector_size, 255);
    }
}
