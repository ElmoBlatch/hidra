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

    /// The wire code, or `None` for an unknown action.
    pub fn code(self) -> Option<u8> {
        use SpecialAction::*;
        Some(match self {
            WheelLeft => 0x01,
            WheelRight => 0x02,
            ResolutionUp => 0x03,
            ResolutionDown => 0x04,
            ResolutionCycle => 0x05,
            ResolutionDefault => 0x06,
            ResolutionShift => 0x07,
            ProfileNext => 0x08,
            ProfilePrevious => 0x09,
            ProfileCycle => 0x0a,
            GShift => 0x0b,
            BatteryLevel => 0x0c,
            Unknown(_) => return None,
        })
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

    /// The 4-byte wire form, for bindings that can be assigned. Macros and
    /// unknown bindings return `None`: they can only be preserved as-is.
    pub fn encode(&self) -> Option<[u8; 4]> {
        Some(match *self {
            Self::Mouse(MouseButtons(mask)) => {
                let [hi, lo] = mask.to_be_bytes();
                [0x80, 0x01, hi, lo]
            }
            Self::Key { modifiers, usage } => [0x80, 0x02, modifiers, usage],
            Self::Consumer(usage) => {
                let [hi, lo] = usage.to_be_bytes();
                [0x80, 0x03, hi, lo]
            }
            // Trailing bytes are zero, as in the factory ROM profiles.
            Self::Special(action) => [0x90, action.code()?, 0x00, 0x00],
            Self::Disabled => [0xff; 4],
            Self::Macro { .. } | Self::Unknown(_) => return None,
        })
    }

    /// Parse the command-line form of an action:
    /// `left`, `right`, `middle`, `back`, `forward`, `dpi-up`, `dpi-down`,
    /// `dpi-cycle`, `dpi-default`, `dpi-shift`, `profile-next`,
    /// `profile-previous`, `profile-cycle`, `g-shift`, `battery`,
    /// `tilt-left`, `tilt-right`, `disabled`, or `key:[mod+...]NAME`
    /// (for example `key:F24`, `key:ctrl+c`, `key:ctrl+shift+F13`).
    pub fn parse(text: &str) -> Result<Self, crate::Error> {
        use SpecialAction::*;
        let lower = text.trim().to_ascii_lowercase();
        let special = |a| Ok(Self::Special(a));
        match lower.as_str() {
            "left" => Ok(Self::Mouse(MouseButtons(0x0001))),
            "right" => Ok(Self::Mouse(MouseButtons(0x0002))),
            "middle" => Ok(Self::Mouse(MouseButtons(0x0004))),
            "back" => Ok(Self::Mouse(MouseButtons(0x0008))),
            "forward" => Ok(Self::Mouse(MouseButtons(0x0010))),
            "dpi-up" => special(ResolutionUp),
            "dpi-down" => special(ResolutionDown),
            "dpi-cycle" => special(ResolutionCycle),
            "dpi-default" => special(ResolutionDefault),
            "dpi-shift" => special(ResolutionShift),
            "profile-next" => special(ProfileNext),
            "profile-previous" => special(ProfilePrevious),
            "profile-cycle" => special(ProfileCycle),
            "g-shift" => special(GShift),
            "battery" => special(BatteryLevel),
            "tilt-left" => special(WheelLeft),
            "tilt-right" => special(WheelRight),
            "disabled" => Ok(Self::Disabled),
            _ => {
                let Some(combo) = lower.strip_prefix("key:") else {
                    return Err(crate::Error::UnknownAction(text.to_string()));
                };
                let mut parts: Vec<&str> = combo.split('+').collect();
                let key = parts.pop().unwrap_or("");
                let mut modifiers = 0u8;
                for m in parts {
                    modifiers |= match m {
                        "ctrl" => 0x01,
                        "shift" => 0x02,
                        "alt" => 0x04,
                        "super" | "meta" | "win" => 0x08,
                        _ => return Err(crate::Error::UnknownAction(text.to_string())),
                    };
                }
                let usage = hid_key_usage(key).ok_or_else(|| crate::Error::UnknownAction(text.to_string()))?;
                Ok(Self::Key { modifiers, usage })
            }
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

/// HID keyboard usage for a key name as produced by [`hid_key_name`]
/// (case-insensitive): letters, digits, F1–F24 and the named keys.
pub fn hid_key_usage(name: &str) -> Option<u8> {
    (0x04..=0x73u8).find(|&u| {
        let n = hid_key_name(u);
        !n.starts_with("usage ") && n.eq_ignore_ascii_case(name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_encode_round_trip() {
        let cases: [(&str, [u8; 4]); 8] = [
            ("left", [0x80, 0x01, 0x00, 0x01]),
            ("forward", [0x80, 0x01, 0x00, 0x10]),
            ("key:F24", [0x80, 0x02, 0x00, 0x73]),
            ("key:ctrl+c", [0x80, 0x02, 0x01, 0x06]),
            ("key:ctrl+shift+f13", [0x80, 0x02, 0x03, 0x68]),
            ("dpi-shift", [0x90, 0x07, 0x00, 0x00]),
            ("profile-cycle", [0x90, 0x0a, 0x00, 0x00]),
            ("disabled", [0xff, 0xff, 0xff, 0xff]),
        ];
        for (text, bytes) in cases {
            let b = ButtonBinding::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(b.encode(), Some(bytes), "{text}");
            assert_eq!(ButtonBinding::decode(bytes), b, "{text} decodes back");
        }
        assert!(ButtonBinding::parse("key:hyper+x").is_err());
        assert!(ButtonBinding::parse("launch-missiles").is_err());
        assert_eq!(ButtonBinding::Macro { sector: 4, offset: 0 }.encode(), None);
    }

    #[test]
    fn device_special_bindings_decode_regardless_of_trailing_bytes() {
        // The Proteus stores `90 04 ff 00`, its second profile `90 07 ff ff`, ROM `90 07 00 00`.
        for tail in [[0xff, 0x00], [0xff, 0xff], [0x00, 0x00]] {
            assert_eq!(
                ButtonBinding::decode([0x90, 0x04, tail[0], tail[1]]),
                ButtonBinding::Special(SpecialAction::ResolutionDown)
            );
        }
    }

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
