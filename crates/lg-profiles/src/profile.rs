use std::ops::Range;

use crate::{ButtonBinding, Error, Led, Sector, crc_ccitt};

pub const DPI_SLOTS: usize = 5;
const MAX_BUTTONS: usize = 16;

// Field offsets (libratbag `hidpp20_internal_profile`).
const REPORT_RATE: usize = 0;
const DEFAULT_DPI: usize = 1;
const SWITCHED_DPI: usize = 2;
const DPI: Range<usize> = 3..13;
const BUTTONS: usize = 32;
const ALT_BUTTONS: usize = 96;
const NAME: Range<usize> = 160..208;
const LEDS: usize = 208;
const ALT_LEDS: usize = 230;
const LED_LEN: usize = 11;
/// The layout's highest fixed field ends here; the CRC sits at `len - 2`.
const MIN_LEN: usize = ALT_LEDS + 2 * LED_LEN + 2;

/// One onboard profile, decoded from (and backed by) its raw sector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    sector: Sector,
    button_count: usize,
}

/// A field-level change. Applying one rewrites only that field's bytes and
/// the trailing CRC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileEdit {
    /// Set a DPI stage. `dpi == 0` disables the slot.
    SetDpi { stage: usize, dpi: u16 },
    SetDefaultStage(u8),
    SetSwitchedStage(u8),
    /// Polling rate in Hz. Stored as the report interval in milliseconds.
    SetReportRate(u16),
    /// Assign a button (primary layer). Macros and unknown bindings cannot be
    /// assigned, only preserved.
    SetButton { index: usize, binding: ButtonBinding },
}

impl ProfileEdit {
    /// The sector bytes this edit is allowed to change (the CRC is extra).
    pub fn offsets(&self) -> Vec<usize> {
        match *self {
            Self::SetDpi { stage, .. } => vec![DPI.start + stage * 2, DPI.start + stage * 2 + 1],
            Self::SetDefaultStage(_) => vec![DEFAULT_DPI],
            Self::SetSwitchedStage(_) => vec![SWITCHED_DPI],
            Self::SetReportRate(_) => vec![REPORT_RATE],
            Self::SetButton { index, .. } => (BUTTONS + index * 4..BUTTONS + index * 4 + 4).collect(),
        }
    }
}

/// What the device allows, for validating edits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditLimits {
    pub dpi: Option<DpiLimits>,
    /// Supported polling rates in Hz; empty means "not known, do not allow".
    pub report_rates_hz: Vec<u16>,
}

/// Device DPI limits from feature 0x2201.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DpiLimits {
    pub min: u16,
    pub max: u16,
    pub step: u16,
}

impl Profile {
    pub fn decode(sector: Sector, button_count: u8) -> Result<Self, Error> {
        if sector.bytes.len() < MIN_LEN {
            return Err(Error::TooShort { needed: MIN_LEN, actual: sector.bytes.len() });
        }
        Ok(Self { sector, button_count: (button_count as usize).min(MAX_BUTTONS) })
    }

    pub fn sector(&self) -> &Sector {
        &self.sector
    }

    pub fn bytes(&self) -> &[u8] {
        &self.sector.bytes
    }

    fn b(&self) -> &[u8] {
        &self.sector.bytes
    }

    pub fn report_rate_hz(&self) -> u16 {
        match self.b()[REPORT_RATE] {
            0 | 0xff => 0,
            ms => 1000 / ms as u16,
        }
    }

    pub fn default_stage(&self) -> u8 {
        self.b()[DEFAULT_DPI]
    }

    pub fn switched_stage(&self) -> u8 {
        self.b()[SWITCHED_DPI]
    }

    /// The five DPI stages; 0 means the slot is disabled.
    pub fn dpi_stages(&self) -> [u16; DPI_SLOTS] {
        let mut out = [0u16; DPI_SLOTS];
        for (i, c) in self.b()[DPI].as_chunks::<2>().0.iter().enumerate() {
            let v = u16::from_le_bytes([c[0], c[1]]);
            out[i] = if v == 0xffff { 0 } else { v };
        }
        out
    }

    pub fn buttons(&self) -> Vec<ButtonBinding> {
        self.bindings_at(BUTTONS)
    }

    /// G-shift (second-function) bindings.
    pub fn alt_buttons(&self) -> Vec<ButtonBinding> {
        self.bindings_at(ALT_BUTTONS)
    }

    fn bindings_at(&self, base: usize) -> Vec<ButtonBinding> {
        (0..self.button_count)
            .map(|i| {
                let o = base + i * 4;
                ButtonBinding::decode(self.b()[o..o + 4].try_into().expect("4 bytes"))
            })
            .collect()
    }

    /// UTF-16LE profile name, or `None` when unset (erased).
    pub fn name(&self) -> Option<String> {
        let units: Vec<u16> = self.b()[NAME]
            .as_chunks::<2>().0.iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0 && u != 0xffff)
            .collect();
        (!units.is_empty()).then(|| String::from_utf16_lossy(&units))
    }

    pub fn leds(&self) -> [Led; 2] {
        [self.led_at(LEDS), self.led_at(LEDS + LED_LEN)]
    }

    pub fn alt_leds(&self) -> [Led; 2] {
        [self.led_at(ALT_LEDS), self.led_at(ALT_LEDS + LED_LEN)]
    }

    fn led_at(&self, o: usize) -> Led {
        Led { raw: self.b()[o..o + LED_LEN].try_into().expect("11 bytes") }
    }

    /// Return a copy with `edit` applied and the CRC recomputed.
    /// Return a copy with one DPI-related `edit` applied and the CRC recomputed.
    pub fn apply(&self, edit: ProfileEdit, limits: Option<DpiLimits>) -> Result<Self, Error> {
        self.apply_all(&[edit], &EditLimits { dpi: limits, report_rates_hz: Vec::new() })
    }

    /// Return a copy with every edit applied, in order, and the CRC
    /// recomputed. The result is validated as a whole: the default stage must
    /// be an enabled one.
    pub fn apply_all(&self, edits: &[ProfileEdit], limits: &EditLimits) -> Result<Self, Error> {
        let mut bytes = self.sector.bytes.clone();
        for edit in edits {
            match *edit {
                ProfileEdit::SetDpi { stage, dpi } => {
                    if stage >= DPI_SLOTS {
                        return Err(Error::NoSuchStage { stage });
                    }
                    if dpi != 0
                        && let Some(l) = limits.dpi
                    {
                        if dpi < l.min || dpi > l.max {
                            return Err(Error::DpiOutOfRange { dpi, min: l.min, max: l.max });
                        }
                        if l.step > 0 && (dpi - l.min) % l.step != 0 {
                            return Err(Error::DpiNotOnStep { dpi, step: l.step });
                        }
                    }
                    let o = DPI.start + stage * 2;
                    bytes[o..o + 2].copy_from_slice(&dpi.to_le_bytes());
                }
                ProfileEdit::SetDefaultStage(s) => {
                    if s as usize >= DPI_SLOTS {
                        return Err(Error::NoSuchStage { stage: s as usize });
                    }
                    bytes[DEFAULT_DPI] = s;
                }
                ProfileEdit::SetSwitchedStage(s) => {
                    if s as usize >= DPI_SLOTS {
                        return Err(Error::NoSuchStage { stage: s as usize });
                    }
                    bytes[SWITCHED_DPI] = s;
                }
                ProfileEdit::SetReportRate(hz) => {
                    if hz == 0 || !limits.report_rates_hz.contains(&hz) {
                        return Err(Error::UnsupportedReportRate { hz, supported: limits.report_rates_hz.clone() });
                    }
                    bytes[REPORT_RATE] = (1000 / hz) as u8;
                }
                ProfileEdit::SetButton { index, binding } => {
                    if index >= self.button_count {
                        return Err(Error::NoSuchButton { index, count: self.button_count });
                    }
                    let o = BUTTONS + index * 4;
                    // Already that action: keep the stored bytes exactly (the
                    // device's own encodings differ in their unused bytes).
                    let current: [u8; 4] = bytes[o..o + 4].try_into().expect("4 bytes");
                    if ButtonBinding::decode(current) != binding {
                        let encoded = binding.encode().ok_or(Error::BindingNotAssignable)?;
                        bytes[o..o + 4].copy_from_slice(&encoded);
                    }
                }
            }
        }
        let n = bytes.len();
        let crc = crc_ccitt(&bytes[..n - 2]);
        bytes[n - 2..].copy_from_slice(&crc.to_be_bytes());
        let edited = Self { sector: Sector::new(self.sector.address, bytes), button_count: self.button_count };
        let default = edited.default_stage() as usize;
        if edited.dpi_stages().get(default).is_none_or(|&dpi| dpi == 0) {
            return Err(Error::DefaultStageDisabled { stage: default });
        }
        Ok(edited)
    }
}

/// Byte offsets at which two sectors differ.
pub fn diff_offsets(a: &[u8], b: &[u8]) -> Vec<usize> {
    let n = a.len().max(b.len());
    (0..n).filter(|&i| a.get(i) != b.get(i)).collect()
}
