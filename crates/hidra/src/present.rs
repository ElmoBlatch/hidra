//! Turn a [`DeviceSnapshot`] into display strings.
//!
//! Pure functions with no GPUI dependency, so the wording and the rules for
//! hiding unsupported fields are tested directly against replayed captures.

use lg_snapshot::{
    DeviceEntry, DeviceSnapshot, Integrity, OnboardSummary, ProfileContent, ProfileSnapshot,
    Reading,
};

/// One labelled value in the device summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

impl Fact {
    fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

const UNAVAILABLE: &str = "Unavailable";

/// Push a fact for an optional reading: hidden when the feature is missing,
/// "Unavailable" when the read failed.
fn push_reading<T>(
    facts: &mut Vec<Fact>,
    label: &str,
    r: &Reading<T>,
    show: impl FnOnce(&T) -> String,
) {
    match r {
        None => {}
        Some(Ok(v)) => facts.push(Fact::new(label, show(v))),
        Some(Err(_)) => facts.push(Fact::new(label, UNAVAILABLE)),
    }
}

/// Upper-case the first character: decoder labels are lower case ("mouse left").
pub fn sentence_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The device title: its reported name, else the model name from discovery.
pub fn device_title(entry: &DeviceEntry, snapshot: Option<&DeviceSnapshot>) -> String {
    snapshot
        .and_then(|s| s.name.as_ref()?.as_ref().ok().cloned())
        .unwrap_or_else(|| entry.model.clone())
}

/// Facts for the device summary, in display order.
pub fn summary_facts(entry: &DeviceEntry, s: &DeviceSnapshot) -> Vec<Fact> {
    let mut facts = Vec::new();
    facts.push(Fact::new("Model", entry.model.clone()));
    facts.push(Fact::new("Connection", entry.connection.description()));
    facts.push(Fact::new("Device node", entry.location()));
    for fw in &s.firmware {
        let version = fw.version.trim();
        facts.push(Fact::new(
            format!("{} firmware", sentence_case(&fw.kind)),
            version,
        ));
    }
    push_reading(&mut facts, "Sensor DPI", &s.sensor_dpi, |d| {
        if d.default == 0 {
            d.current.to_string()
        } else {
            format!("{} (default {})", d.current, d.default)
        }
    });
    push_reading(&mut facts, "DPI range", &s.dpi_range, |r| r.label());
    push_reading(&mut facts, "Polling rate", &s.polling_rate_hz, |hz| {
        format!("{hz} Hz")
    });
    push_reading(&mut facts, "Supported rates", &s.polling_rates_hz, |v| {
        format!(
            "{} Hz",
            v.iter().map(u16::to_string).collect::<Vec<_>>().join(", ")
        )
    });
    push_reading(&mut facts, "Battery", &s.battery_mv, |&(mv, charging)| {
        if charging {
            format!("{mv} mV, charging")
        } else {
            format!("{mv} mV")
        }
    });
    match &s.onboard {
        None => {}
        Some(Err(_)) => facts.push(Fact::new("Onboard profiles", UNAVAILABLE)),
        Some(Ok(o)) => {
            push_reading(&mut facts, "Onboard mode", &o.mode, |m| m.label());
            facts.push(Fact::new("Profile memory", profile_memory(o)));
        }
    }
    facts.push(Fact::new(
        "HID++ protocol",
        format!("{}.{}", s.protocol.0, s.protocol.1),
    ));
    facts
}

/// Where each profile is stored, for the device info sheet.
pub fn storage_facts(s: &DeviceSnapshot) -> Vec<Fact> {
    let Some(Ok(o)) = &s.onboard else {
        return Vec::new();
    };
    let Ok(set) = &o.profiles else {
        return Vec::new();
    };
    set.profiles
        .iter()
        .filter_map(|p| {
            p.content
                .as_ref()
                .ok()
                .map(|c| Fact::new(p.title.clone(), profile_storage(p, c)))
        })
        .collect()
}

fn profile_memory(o: &OnboardSummary) -> String {
    let plural = |n: u8, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    format!(
        "Format {}, {}, {}, {}",
        o.profile_format,
        plural(o.profile_count, "user slot", "user slots"),
        plural(o.rom_profile_count, "factory profile", "factory profiles"),
        plural(o.button_count, "button", "buttons"),
    )
}

/// Secondary line under a profile's name: polling rate and where it is stored.
pub fn profile_meta(p: &ProfileSnapshot, c: &ProfileContent) -> String {
    let mut parts = vec![format!("{} Hz polling", grouped(c.report_rate_hz.into()))];
    parts.push(match c.integrity {
        Integrity::Rom => "Built into the mouse".to_string(),
        Integrity::CrcOk(_) => "Stored on the mouse".to_string(),
        Integrity::CrcMismatch { .. } => "Checksum mismatch".to_string(),
    });
    if !p.enabled {
        parts.push("Disabled".to_string());
    }
    parts.join(" · ")
}

/// Storage details for the device info sheet.
pub fn profile_storage(p: &ProfileSnapshot, c: &ProfileContent) -> String {
    let crc = match c.integrity {
        Integrity::Rom => "ROM".to_string(),
        Integrity::CrcOk(crc) => format!("CRC {crc:04x} OK"),
        Integrity::CrcMismatch { .. } => "CRC mismatch".to_string(),
    };
    format!("Sector {:#06x}, {crc}", p.address)
}

/// One row of the DPI stage table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRow {
    /// 1-based stage number, as printed on Logitech's software.
    pub number: usize,
    /// `None` for an unused stage.
    pub dpi: Option<u16>,
    pub is_default: bool,
    /// The stage the DPI-shift (sniper) button drops to, when one is bound.
    pub is_shift: bool,
}

pub fn stage_rows(c: &ProfileContent) -> Vec<StageRow> {
    let shift = uses_dpi_shift(c);
    c.dpi_stages
        .iter()
        .enumerate()
        .map(|(ix, &dpi)| StageRow {
            number: ix + 1,
            dpi: (dpi != 0).then_some(dpi),
            is_default: ix == c.default_stage as usize,
            is_shift: shift && ix == c.shift_stage as usize,
        })
        .collect()
}

/// One row of the button list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonRow {
    /// Logitech's key label ("G6"), where the model's numbering is known.
    pub key: Option<String>,
    /// The physical button: "Sniper", "Wheel tilt left", or "Button 7".
    pub name: String,
    pub action: String,
    /// The action differs from what this button does out of the box.
    pub remapped: bool,
}

/// Physical button names for the G502 family (Proteus Spectrum, LIGHTSPEED),
/// in onboard-profile order. G1–G9 follow Logitech's own labels. G7 and G8
/// sit along the left click's outer edge and G9 behind the wheel. Which of
/// G7 and G8 is the front one is taken from their factory roles (DPI down and
/// DPI up) and has not been checked by pressing them. The wheel tilts' G
/// numbers are not confirmed, so those stay unnumbered.
/// (key label, physical name, factory actions as decoded labels). The
/// factory actions come from the mice's own ROM profiles; G9 is profile cycle
/// on the Proteus Spectrum and battery level on the LIGHTSPEED.
const G502_BUTTONS: [(Option<&str>, &str, &[&str]); 11] = [
    (Some("G1"), "Left click", &["mouse left"]),
    (Some("G2"), "Right click", &["mouse right"]),
    (Some("G3"), "Wheel click", &["mouse middle"]),
    (Some("G4"), "Back", &["mouse back"]),
    (Some("G5"), "Forward", &["mouse forward"]),
    (Some("G6"), "Sniper", &["DPI shift (sniper)"]),
    (Some("G7"), "Top button, rear", &["DPI down"]),
    (Some("G8"), "Top button, front", &["DPI up"]),
    (
        Some("G9"),
        "Behind the wheel",
        &["cycle profile", "show battery level"],
    ),
    (None, "Wheel tilt right", &["wheel tilt right"]),
    (None, "Wheel tilt left", &["wheel tilt left"]),
];

/// Wording for a decoded action label.
pub fn action_label(raw: &str) -> String {
    match raw {
        "mouse left" => "Left click".into(),
        "mouse right" => "Right click".into(),
        "mouse middle" => "Middle click".into(),
        "mouse back" => "Back".into(),
        "mouse forward" => "Forward".into(),
        "DPI shift (sniper)" => "DPI shift".into(),
        "cycle profile" => "Next profile".into(),
        "show battery level" => "Show battery level".into(),
        other => match other.strip_prefix("key ") {
            Some(key) => key.to_string(),
            None => sentence_case(other),
        },
    }
}

/// True when the device uses the G502 button layout.
pub fn is_g502_layout(entry: &DeviceEntry, c: &ProfileContent) -> bool {
    entry.model.contains("G502") && c.buttons.len() == G502_BUTTONS.len()
}

/// Button rows in device order. `g502_layout` selects the G502 names;
/// other devices get "Button N".
pub fn button_rows(c: &ProfileContent, g502_layout: bool) -> Vec<ButtonRow> {
    c.buttons
        .iter()
        .enumerate()
        .map(|(ix, action)| {
            let (key, name, remapped) = match G502_BUTTONS.get(ix).filter(|_| g502_layout) {
                Some((key, name, factory)) => (
                    key.map(str::to_string),
                    name.to_string(),
                    !factory.contains(&action.as_str()),
                ),
                None => (None, format!("Button {}", ix + 1), false),
            };
            ButtonRow {
                key,
                name,
                action: action_label(action),
                remapped,
            }
        })
        .collect()
}

/// A number with thousands separators: `12000` -> `12,000`.
pub fn grouped(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// One headline value in the strip under the device title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub label: &'static str,
    pub value: String,
    /// Unit or qualifier shown after the value, e.g. "DPI" or "Hz".
    pub unit: Option<String>,
}

/// Headline values, in display order. Missing features are left out.
pub fn headline_stats(s: &DeviceSnapshot) -> Vec<Stat> {
    let mut stats = Vec::new();
    if let Some(Ok(d)) = &s.sensor_dpi {
        stats.push(Stat {
            label: "Sensor",
            value: grouped(d.current.into()),
            unit: Some("DPI".into()),
        });
    }
    if let Some(Ok(hz)) = &s.polling_rate_hz {
        stats.push(Stat {
            label: "Polling",
            value: grouped((*hz).into()),
            unit: Some("Hz".into()),
        });
    }
    if let Some(Ok((mv, charging))) = &s.battery_mv {
        let volts = format!("{}.{:02}", mv / 1000, (mv % 1000) / 10);
        stats.push(Stat {
            label: "Battery",
            value: volts,
            unit: Some(if *charging {
                "V, charging".into()
            } else {
                "V".into()
            }),
        });
    }
    if let Some(Ok(o)) = &s.onboard
        && let Ok(set) = &o.profiles
    {
        let value = match set.active() {
            Some(p) => p.title.clone(),
            None => "None".into(),
        };
        stats.push(Stat {
            label: "Active profile",
            value,
            unit: None,
        });
    }
    stats
}

/// Bar length for a DPI stage as a fraction of the profile's highest stage.
pub fn stage_fraction(dpi: u16, c: &ProfileContent) -> f32 {
    let max = c.dpi_stages.iter().copied().max().unwrap_or(0);
    if max == 0 {
        0.0
    } else {
        f32::from(dpi) / f32::from(max)
    }
}

/// True when a button in this profile is bound to DPI shift, so the shift
/// stage means something.
pub fn uses_dpi_shift(c: &ProfileContent) -> bool {
    c.buttons.iter().any(|b| b.contains("DPI shift"))
}

/// Profile title for a tab: the directory title.
pub fn profile_name(c: &ProfileContent) -> Option<&str> {
    c.name.as_deref().map(str::trim).filter(|n| !n.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_inserts_separators() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(800), "800");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(25600), "25,600");
        assert_eq!(grouped(1234567), "1,234,567");
    }

    #[test]
    fn sentence_case_handles_empty_and_ascii() {
        assert_eq!(sentence_case(""), "");
        assert_eq!(sentence_case("mouse left"), "Mouse left");
        assert_eq!(sentence_case("DPI up"), "DPI up");
    }
}
