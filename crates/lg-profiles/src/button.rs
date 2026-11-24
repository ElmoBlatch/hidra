use std::fmt;

/// Mouse-button bitmask used by HID mouse bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseButtons(pub u16);

impl fmt::Display for MouseButtons {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = ["left", "right", "middle", "back", "forward"];
        let mut first = true;
        for bit in 0..16 {
            if self.0 & (1 << bit) != 0 {
                if !first {
                    f.write_str("+")?;
                }
                first = false;
                match names.get(bit) {
                    Some(n) => f.write_str(n)?,
                    None => write!(f, "button{}", bit + 1)?,
                }
            }
        }
        if first {
            f.write_str("none")?;
        }
        Ok(())
    }
}

/// Special (firmware-handled) actions, codes per libratbag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialAction {
    WheelLeft,
    WheelRight,
    ResolutionUp,
    ResolutionDown,
    ResolutionCycle,
    ResolutionDefault,
    ResolutionShift,
    ProfileNext,
    ProfilePrevious,
    ProfileCycle,
    GShift,
    BatteryLevel,
    Unknown(u8),
}

impl SpecialAction {
    pub fn from_code(code: u8) -> Self {
        use SpecialAction::*;
        match code {
            0x01 => WheelLeft,
            0x02 => WheelRight,
            0x03 => ResolutionUp,
            0x04 => ResolutionDown,
            0x05 => ResolutionCycle,
            0x06 => ResolutionDefault,
            0x07 => ResolutionShift,
            0x08 => ProfileNext,
            0x09 => ProfilePrevious,
            0x0a => ProfileCycle,
            0x0b => GShift,
            0x0c => BatteryLevel,
            other => Unknown(other),
        }
    }

    pub fn label(self) -> String {
        use SpecialAction::*;
        match self {
            WheelLeft => "wheel tilt left".into(),
            WheelRight => "wheel tilt right".into(),
            ResolutionUp => "DPI up".into(),
            ResolutionDown => "DPI down".into(),
            ResolutionCycle => "DPI cycle".into(),
            ResolutionDefault => "DPI default".into(),
            ResolutionShift => "DPI shift (sniper)".into(),
            ProfileNext => "next profile".into(),
            ProfilePrevious => "previous profile".into(),
            ProfileCycle => "cycle profile".into(),
            GShift => "G-shift".into(),
            BatteryLevel => "show battery level".into(),
            Unknown(c) => format!("special {c:#04x}"),
        }
    }
}

/// A 4-byte onboard button binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonBinding {
    /// `80 01 hi lo`: mouse-button bitmask.
    Mouse(MouseButtons),
    /// `80 02 mods key`: keyboard key (HID usage) with modifier bitmask.
    Key { modifiers: u8, usage: u8 },
    /// `80 03 hi lo`: consumer-control usage.
    Consumer(u16),
    /// `90 code ..`: firmware special action.
    Special(SpecialAction),
    /// `00 sector 00 offset`: macro stored elsewhere in onboard memory.
    Macro { sector: u8, offset: u8 },
    /// `ff ff ff ff`: unassigned.
    Disabled,
    Unknown([u8; 4]),
}

impl ButtonBinding {
    pub fn decode(b: [u8; 4]) -> Self {
        match b {
            [0xff, ..] => Self::Disabled,
            [0x80, 0x01, hi, lo] => Self::Mouse(MouseButtons(u16::from_be_bytes([hi, lo]))),
            [0x80, 0x02, modifiers, usage] => Self::Key { modifiers, usage },
            [0x80, 0x03, hi, lo] => Self::Consumer(u16::from_be_bytes([hi, lo])),
            [0x90, code, ..] => Self::Special(SpecialAction::from_code(code)),
            [0x00, sector, _, offset] => Self::Macro { sector, offset },
            other => Self::Unknown(other),
        }
    }

    pub fn label(&self) -> String {
        match *self {
            Self::Mouse(m) => format!("mouse {m}"),
            Self::Key { modifiers, usage } => {
                let mut s = String::new();
                for (bit, name) in ["Ctrl", "Shift", "Alt", "Super", "RCtrl", "RShift", "RAlt", "RSuper"]
                    .iter()
                    .enumerate()
                {
                    if modifiers & (1 << bit) != 0 {
                        s.push_str(name);
                        s.push('+');
                    }
                }
                s.push_str(&hid_key_name(usage));
                format!("key {s}")
            }
            Self::Consumer(u) => format!("consumer {u:#06x}"),
            Self::Special(a) => a.label(),
            Self::Macro { sector, offset } => format!("macro @ sector {sector} offset {offset:#04x}"),
            Self::Disabled => "disabled".into(),
            Self::Unknown(b) => format!("unknown {:02x} {:02x} {:02x} {:02x}", b[0], b[1], b[2], b[3]),
        }
    }
}

/// Name for a HID keyboard usage (page 0x07).
pub fn hid_key_name(usage: u8) -> String {
    match usage {
        0x04..=0x1d => ((b'A' + usage - 0x04) as char).to_string(),
        0x1e..=0x26 => ((b'1' + usage - 0x1e) as char).to_string(),
        0x27 => "0".into(),
        0x28 => "Enter".into(),
        0x29 => "Esc".into(),
        0x2a => "Backspace".into(),
        0x2b => "Tab".into(),
        0x2c => "Space".into(),
        0x3a..=0x45 => format!("F{}", usage - 0x3a + 1),
        0x68..=0x73 => format!("F{}", usage - 0x68 + 13),
        0x4f => "Right".into(),
        0x50 => "Left".into(),
        0x51 => "Down".into(),
        0x52 => "Up".into(),
        _ => format!("usage {usage:#04x}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f24_is_keyboard_usage() {
        let b = ButtonBinding::decode([0x80, 0x02, 0x00, 0x73]);
        assert_eq!(b, ButtonBinding::Key { modifiers: 0, usage: 0x73 });
        assert_eq!(b.label(), "key F24");
    }

    #[test]
    fn ctrl_c() {
        assert_eq!(ButtonBinding::decode([0x80, 0x02, 0x01, 0x06]).label(), "key Ctrl+C");
    }
}
