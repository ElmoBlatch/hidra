/// Onboard LED effect modes, codes per libratbag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedMode {
    Off,
    /// Fixed colour.
    On { rgb: [u8; 3] },
    /// Colour cycle with a period in milliseconds.
    Cycle { period_ms: u16, brightness: u8 },
    /// Breathing in one colour.
    Breathing { rgb: [u8; 3], period_ms: u16, brightness: u8 },
    Unknown(u8),
}

/// An 11-byte LED record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Led {
    pub raw: [u8; 11],
}

impl Led {
    pub fn mode(&self) -> LedMode {
        let r = &self.raw;
        match r[0] {
            0x00 => LedMode::Off,
            0x01 => LedMode::On { rgb: [r[1], r[2], r[3]] },
            0x03 => LedMode::Cycle { period_ms: u16::from_be_bytes([r[6], r[7]]), brightness: r[8] },
            0x0a => LedMode::Breathing {
                rgb: [r[1], r[2], r[3]],
                period_ms: u16::from_be_bytes([r[4], r[5]]),
                brightness: r[7],
            },
            m => LedMode::Unknown(m),
        }
    }

    /// Short description without the colour, for interfaces that show the
    /// colour separately: "solid", "color cycle, 8 s", "breathing, 2.5 s".
    pub fn summary(&self) -> String {
        fn seconds(ms: u16) -> String {
            if ms.is_multiple_of(1000) { format!("{} s", ms / 1000) } else { format!("{:.1} s", f32::from(ms) / 1000.0) }
        }
        match self.mode() {
            LedMode::Off => "off".into(),
            LedMode::On { .. } => "solid".into(),
            LedMode::Cycle { period_ms, .. } => format!("color cycle, {}", seconds(period_ms)),
            LedMode::Breathing { period_ms, .. } => format!("breathing, {}", seconds(period_ms)),
            LedMode::Unknown(m) => format!("unknown effect {m:#04x}"),
        }
    }

    pub fn label(&self) -> String {
        match self.mode() {
            LedMode::Off => "off".into(),
            LedMode::On { rgb } => format!("on #{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]),
            LedMode::Cycle { period_ms, brightness } => format!("cycle {period_ms} ms, intensity {brightness}"),
            LedMode::Breathing { rgb, period_ms, brightness } => format!(
                "breathing #{:02x}{:02x}{:02x} {period_ms} ms, intensity {brightness}",
                rgb[0], rgb[1], rgb[2]
            ),
            LedMode::Unknown(m) => format!("mode {m:#04x}"),
        }
    }
}
