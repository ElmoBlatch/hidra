use lg_hidpp::features::{
    adjustable_dpi, battery_voltage, device_info, device_name, onboard_profiles, report_rate,
};
use lg_hidpp::{Device, Transport};
use lg_profiles::{DPI_SLOTS, Descriptor, Directory, LedMode, Profile, Sector};

/// The outcome of one optional read.
///
/// `None` (feature not present) means "hide it"; `Some(Err(_))` means the
/// device has the feature but the read failed, which is worth showing.
pub type Reading<T> = Option<Result<T, String>>;

fn reading<T>(r: lg_hidpp::Result<T>) -> Reading<T> {
    match r {
        Ok(v) => Some(Ok(v)),
        Err(lg_hidpp::Error::FeatureMissing(_)) => None,
        Err(e) => Some(Err(e.to_string())),
    }
}

/// Everything the viewer shows about one device, as owned plain data.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DeviceSnapshot {
    /// HID++ protocol (major, minor).
    pub protocol: (u8, u8),
    pub name: Reading<String>,
    pub firmware: Vec<Firmware>,
    pub sensor_dpi: Reading<SensorDpi>,
    pub dpi_range: Reading<DpiRange>,
    pub polling_rate_hz: Reading<u16>,
    pub polling_rates_hz: Reading<Vec<u16>>,
    pub battery_mv: Reading<(u16, bool)>,
    /// `None` when the device has no onboard-profiles feature.
    pub onboard: Option<Result<OnboardSummary, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Firmware {
    /// "application", "bootloader", …
    pub kind: String,
    /// e.g. `U1 03.02.B0012`.
    pub version: String,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SensorDpi {
    pub current: u16,
    /// 0 when the device does not report one.
    pub default: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DpiRange {
    Range { min: u16, max: u16, step: u16 },
    Discrete(Vec<u16>),
}

impl DpiRange {
    pub fn label(&self) -> String {
        match self {
            Self::Range { min, max, step } => format!("{min}–{max}, step {step}"),
            Self::Discrete(v) => v.iter().map(u16::to_string).collect::<Vec<_>>().join(", "),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardMode {
    Onboard,
    Host,
    Other(u8),
}

impl OnboardMode {
    pub fn label(self) -> String {
        match self {
            Self::Onboard => "Onboard".into(),
            Self::Host => "Host".into(),
            Self::Other(m) => format!("Mode {m}"),
        }
    }
}

/// Onboard-profile memory: layout and decoded profiles.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OnboardSummary {
    pub profile_format: u8,
    pub profile_count: u8,
    pub rom_profile_count: u8,
    pub button_count: u8,
    pub sector_count: u8,
    pub sector_size: u16,
    pub mode: Reading<OnboardMode>,
    /// Address reported by `GetActiveProfile`.
    pub active_address: Option<u16>,
    /// `Err` when the layout is unsupported or the directory could not be read.
    pub profiles: Result<ProfileSet, String>,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ProfileSet {
    /// True when the user directory is blank or invalid and the device runs
    /// its factory (ROM) profiles.
    pub factory: bool,
    pub profiles: Vec<ProfileSnapshot>,
}

impl ProfileSet {
    pub fn active(&self) -> Option<&ProfileSnapshot> {
        self.profiles.iter().find(|p| p.active)
    }
}

/// CRC state of a profile sector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrity {
    /// Factory ROM sector; no CRC to check.
    Rom,
    CrcOk(u16),
    CrcMismatch {
        stored: Option<u16>,
        computed: Option<u16>,
    },
}

/// One decoded profile.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ProfileSnapshot {
    /// Sector address; the stable identity of this profile on the device.
    pub address: u16,
    /// Display title: "Profile 1", or "Factory profile 1".
    pub title: String,
    pub factory: bool,
    pub enabled: bool,
    pub active: bool,
    /// The decoded content, or why the sector could not be read or decoded.
    pub content: Result<ProfileContent, String>,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ProfileContent {
    pub name: Option<String>,
    pub integrity: Integrity,
    pub report_rate_hz: u16,
    /// 0 marks an unused stage.
    pub dpi_stages: [u16; DPI_SLOTS],
    pub default_stage: u8,
    pub shift_stage: u8,
    /// One label per button, in device order.
    pub buttons: Vec<String>,
    /// The decoded binding behind each label.
    pub bindings: Vec<lg_profiles::ButtonBinding>,
    pub leds: Vec<LedSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LedSnapshot {
    /// "Logo" or "Side".
    pub zone: String,
    pub label: String,
    /// The effect colour, for effects that have one.
    pub rgb: Option<[u8; 3]>,
}

/// Read everything from an opened device. Never fails as a whole: each part
/// records its own error so the rest can still be shown.
pub fn read_device<T: Transport>(dev: &mut Device<T>) -> DeviceSnapshot {
    let firmware = match device_info::device_info(dev) {
        Ok(info) => (0..info.entity_count)
            .filter_map(|e| device_info::firmware_info(dev, e).ok())
            .map(|f| Firmware {
                kind: f.kind_name().to_string(),
                version: f.version_string(),
                active: f.active,
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    DeviceSnapshot {
        protocol: dev.protocol_version(),
        name: reading(device_name::name(dev)),
        firmware,
        sensor_dpi: reading(adjustable_dpi::sensor_dpi(dev, 0).map(|d| SensorDpi {
            current: d.current,
            default: d.default,
        })),
        dpi_range: reading(adjustable_dpi::dpi_list(dev, 0).map(|l| match l {
            adjustable_dpi::DpiList::Range { min, max, step } => DpiRange::Range { min, max, step },
            adjustable_dpi::DpiList::Discrete(v) => DpiRange::Discrete(v),
        })),
        polling_rate_hz: reading(report_rate::current_rate_hz(dev)),
        polling_rates_hz: reading(report_rate::supported_rates_hz(dev)),
        battery_mv: reading(battery_voltage::voltage(dev).map(|b| (b.millivolts, b.charging()))),
        onboard: read_onboard(dev),
    }
}

fn read_onboard<T: Transport>(dev: &mut Device<T>) -> Option<Result<OnboardSummary, String>> {
    let raw = match onboard_profiles::info_raw(dev) {
        Ok(raw) => raw,
        Err(lg_hidpp::Error::FeatureMissing(_)) => return None,
        Err(e) => return Some(Err(e.to_string())),
    };
    let Some(d) = Descriptor::parse(&raw) else {
        return Some(Err("short onboard-profiles descriptor".into()));
    };
    let mode = reading(onboard_profiles::mode(dev).map(|m| match m {
        1 => OnboardMode::Onboard,
        2 => OnboardMode::Host,
        m => OnboardMode::Other(m),
    }));
    let active_address = onboard_profiles::active_profile(dev).ok();
    let profiles = if d.layout_supported() {
        read_profiles(dev, &d, active_address)
    } else {
        Err(format!(
            "profile format {} with {}-byte sectors is not supported by this decoder",
            d.profile_format, d.sector_size
        ))
    };
    Some(Ok(OnboardSummary {
        profile_format: d.profile_format,
        profile_count: d.profile_count,
        rom_profile_count: d.rom_profile_count,
        button_count: d.button_count,
        sector_count: d.sector_count,
        sector_size: d.sector_size,
        mode,
        active_address,
        profiles,
    }))
}

fn read_profiles<T: Transport>(
    dev: &mut Device<T>,
    d: &Descriptor,
    active: Option<u16>,
) -> Result<ProfileSet, String> {
    let dir_sector = Sector::new(
        0,
        onboard_profiles::read_sector(dev, 0, d.sector_size)
            .map_err(|e| format!("reading directory: {e}"))?,
    );
    // (address, title, enabled)
    let (factory, targets): (bool, Vec<(u16, String, bool)>) = match Directory::parse(&dir_sector) {
        Some(dir) => (
            false,
            dir.entries
                .iter()
                .map(|e| (e.sector, format!("Profile {}", e.profile + 1), e.enabled))
                .collect(),
        ),
        None => (
            true,
            (1..=d.rom_profile_count as u16)
                .map(|n| (0x0100 | n, format!("Factory profile {n}"), true))
                .collect(),
        ),
    };
    let profiles = targets
        .into_iter()
        .map(|(address, title, enabled)| ProfileSnapshot {
            address,
            title,
            factory,
            enabled,
            active: active == Some(address),
            content: read_profile(dev, address, d),
        })
        .collect();
    Ok(ProfileSet { factory, profiles })
}

fn read_profile<T: Transport>(
    dev: &mut Device<T>,
    address: u16,
    d: &Descriptor,
) -> Result<ProfileContent, String> {
    let bytes =
        onboard_profiles::read_sector(dev, address, d.sector_size).map_err(|e| e.to_string())?;
    let sector = Sector::new(address, bytes);
    let integrity = if sector.is_rom() {
        Integrity::Rom
    } else if sector.crc_valid() {
        Integrity::CrcOk(sector.stored_crc().unwrap_or(0))
    } else {
        Integrity::CrcMismatch {
            stored: sector.stored_crc(),
            computed: sector.computed_crc(),
        }
    };
    let p = Profile::decode(sector, d.button_count).map_err(|e| e.to_string())?;
    let leds = ["Logo", "Side"]
        .into_iter()
        .zip(p.leds())
        .map(|(zone, led)| LedSnapshot {
            zone: zone.to_string(),
            label: led.summary(),
            rgb: match led.mode() {
                LedMode::On { rgb } | LedMode::Breathing { rgb, .. } => Some(rgb),
                _ => None,
            },
        })
        .collect();
    Ok(ProfileContent {
        name: p.name(),
        integrity,
        report_rate_hz: p.report_rate_hz(),
        dpi_stages: p.dpi_stages(),
        default_stage: p.default_stage(),
        shift_stage: p.switched_stage(),
        buttons: p.buttons().iter().map(|b| b.label()).collect(),
        bindings: p.buttons(),
        leds,
    })
}
