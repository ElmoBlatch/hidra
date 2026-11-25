use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lg_hidpp::discovery::{self, NodeKind};
use lg_hidpp::features::onboard_profiles;
use lg_hidpp::replay::{ReplayTransport, parse_log};
use lg_hidpp::sim::{SimMemory, SimMouse};
use lg_hidpp::{DEVICE_INDEX_DIRECT, Device, HidrawTransport};

use crate::snapshot::{DeviceSnapshot, read_device};
use crate::{AnyTransport, REPLAY_TIMEOUT};

/// How a device reaches the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    /// Wired USB.
    Wired,
    /// Paired to a wireless receiver with this product ID.
    Receiver(u16),
    /// Served from a recorded capture rather than hardware.
    Replay,
    /// A simulated mouse (demo mode and tests).
    Simulated,
}

impl Connection {
    /// Short human description, e.g. "Wired" or "Via receiver".
    pub fn label(&self) -> &'static str {
        match self {
            Self::Wired => "Wired",
            Self::Receiver(_) => "Via receiver",
            Self::Replay => "Replay capture",
            Self::Simulated => "Simulated",
        }
    }

    /// Longer description naming the receiver, e.g. "Via LIGHTSPEED receiver".
    pub fn description(&self) -> String {
        match self {
            Self::Receiver(pid) => format!("Via {}", discovery::model_name(*pid)),
            other => other.label().to_string(),
        }
    }
}

/// Where to read a device from.
#[derive(Debug, Clone)]
pub enum DeviceSource {
    Hidraw(PathBuf),
    Replay { log: PathBuf, segment: String },
    /// A simulated mouse whose memory persists for the life of the entry.
    Sim(Arc<Mutex<SimMemory>>),
}

impl PartialEq for DeviceSource {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Hidraw(a), Self::Hidraw(b)) => a == b,
            (Self::Replay { log: a, segment: sa }, Self::Replay { log: b, segment: sb }) => a == b && sa == sb,
            (Self::Sim(a), Self::Sim(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for DeviceSource {}

/// One detected device, before it has been opened.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeviceEntry {
    /// Stable identity for the lifetime of the connection: the hidraw path, or
    /// `replay:<segment>`.
    pub id: String,
    pub model: String,
    pub connection: Connection,
    pub source: DeviceSource,
    /// USB/HID++ product ID; 0 for replayed captures.
    pub product: u16,
}

impl DeviceEntry {
    /// True when profile edits are enabled for this device: live hardware of
    /// a model whose write path has been verified.
    pub fn write_verified(&self) -> bool {
        match self.source {
            DeviceSource::Hidraw(_) => crate::WRITE_VERIFIED_PIDS.contains(&self.product),
            DeviceSource::Sim(_) => true,
            DeviceSource::Replay { .. } => false,
        }
    }

    /// Human-readable location: `/dev/hidraw7`, or the capture file name.
    pub fn location(&self) -> String {
        match &self.source {
            DeviceSource::Hidraw(p) => p.display().to_string(),
            DeviceSource::Replay { log, .. } => {
                log.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| log.display().to_string())
            }
            DeviceSource::Sim(_) => "simulated".to_string(),
        }
    }
}

/// Why a device could not be opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    /// The hidraw node exists but this user may not open it.
    #[error("permission denied opening {path}")]
    PermissionDenied { path: String },
    /// The node opened but the device did not answer (asleep, out of range, or
    /// switched off).
    #[error("{0}")]
    Unreachable(String),
    /// Anything else: missing node, bad capture file, …
    #[error("{0}")]
    Other(String),
}

/// Find every Logitech HID++ device node (receivers themselves are skipped).
pub fn discover() -> io::Result<Vec<DeviceEntry>> {
    Ok(discovery::enumerate()?
        .into_iter()
        .filter(|n| n.kind == NodeKind::Device)
        .map(|n| DeviceEntry {
            id: n.dev_path.display().to_string(),
            product: n.product,
            model: n.model_name().to_string(),
            connection: match n.via_receiver {
                Some(pid) => Connection::Receiver(pid),
                None => Connection::Wired,
            },
            source: DeviceSource::Hidraw(n.dev_path),
        })
        .collect())
}

/// Entries for a `ratbagd --verbose=raw` capture: the named segment, or every
/// segment with HID++ traffic when `segment` is `None`.
pub fn replay_entries(log: &Path, segment: Option<&str>) -> Result<Vec<DeviceEntry>, ReadError> {
    let text = std::fs::read_to_string(log).map_err(|e| ReadError::Other(format!("reading {}: {e}", log.display())))?;
    let entries: Vec<DeviceEntry> = parse_log(&text)
        .into_iter()
        .filter(|s| segment.is_none_or(|name| s.name == name))
        .map(|s| DeviceEntry {
            id: format!("replay:{}", s.name),
            product: 0,
            model: s.name.clone(),
            connection: Connection::Replay,
            source: DeviceSource::Replay { log: log.to_path_buf(), segment: s.name },
        })
        .collect();
    if entries.is_empty() {
        return Err(ReadError::Other(match segment {
            Some(name) => format!("no segment named \"{name}\" with HID++ traffic in {}", log.display()),
            None => format!("no device segment with HID++ traffic in {}", log.display()),
        }));
    }
    Ok(entries)
}

/// A simulated mouse seeded with the onboard memory recorded in a capture:
/// its descriptor, directory and profile sectors. Edits to it persist for the
/// life of the returned entry, so the whole edit path can be exercised
/// without hardware.
pub fn simulated_entry(log: &Path, segment: &str) -> Result<DeviceEntry, ReadError> {
    let replay = replay_entries(log, Some(segment))?.remove(0);
    let mut dev = open_entry(&replay)?;
    let other = |e: lg_hidpp::Error| ReadError::Other(format!("reading the capture: {e}"));
    let descriptor = onboard_profiles::info_raw(&mut dev).map_err(other)?;
    let d = lg_profiles::Descriptor::parse(&descriptor)
        .ok_or_else(|| ReadError::Other("the capture has no onboard-profiles descriptor".into()))?;
    let active = onboard_profiles::active_profile(&mut dev).map_err(other)?;
    let mut sectors = std::collections::BTreeMap::new();
    let directory = onboard_profiles::read_sector(&mut dev, 0, d.sector_size).map_err(other)?;
    let profile_sectors: Vec<u16> = lg_profiles::Directory::parse(&lg_profiles::Sector::new(0, directory.clone()))
        .map(|dir| dir.entries.iter().map(|e| e.sector).collect())
        .unwrap_or_default();
    sectors.insert(0, directory);
    for s in profile_sectors {
        sectors.insert(s, onboard_profiles::read_sector(&mut dev, s, d.sector_size).map_err(other)?);
    }
    // The capture holds only the directory and profiles. Fill the rest the way
    // the Proteus Spectrum reports unused memory (zeros), so a full backup
    // can read every address; the factory profiles are placeholders.
    let rom = (1..=d.rom_profile_count as u16).map(|n| 0x0100 | n);
    for address in (0..d.sector_count as u16).chain(rom) {
        sectors.entry(address).or_insert_with(|| vec![0u8; d.sector_size as usize]);
    }
    Ok(DeviceEntry {
        id: format!("sim:{segment}"),
        product: 0,
        model: replay.model,
        connection: Connection::Simulated,
        source: DeviceSource::Sim(SimMemory::new(descriptor, sectors, active).shared()),
    })
}

/// Open a device and enumerate its features. Blocking.
pub fn open_entry(entry: &DeviceEntry) -> Result<Device<AnyTransport>, ReadError> {
    match &entry.source {
        DeviceSource::Hidraw(path) => {
            let transport = HidrawTransport::open(path).map_err(|e| match e.kind() {
                ErrorKind::PermissionDenied => ReadError::PermissionDenied { path: path.display().to_string() },
                _ => ReadError::Other(format!("opening {}: {e}", path.display())),
            })?;
            Device::open(AnyTransport::Hidraw(transport), DEVICE_INDEX_DIRECT)
                .map_err(|e| ReadError::Unreachable(e.to_string()))
        }
        DeviceSource::Sim(memory) => {
            Device::open_with_timeout(AnyTransport::Sim(SimMouse::new(memory.clone())), DEVICE_INDEX_DIRECT, REPLAY_TIMEOUT)
                .map_err(|e| ReadError::Unreachable(e.to_string()))
        }
        DeviceSource::Replay { log, segment } => {
            let text =
                std::fs::read_to_string(log).map_err(|e| ReadError::Other(format!("reading {}: {e}", log.display())))?;
            let seg = parse_log(&text)
                .into_iter()
                .find(|s| &s.name == segment)
                .ok_or_else(|| ReadError::Other(format!("no segment named \"{segment}\" in {}", log.display())))?;
            Device::open_with_timeout(AnyTransport::Replay(ReplayTransport::new(seg)), DEVICE_INDEX_DIRECT, REPLAY_TIMEOUT)
                .map_err(|e| ReadError::Unreachable(e.to_string()))
        }
    }
}

/// Open a device and read everything the viewer shows. Blocking: expect one
/// to several seconds on hardware.
pub fn read_entry(entry: &DeviceEntry) -> Result<DeviceSnapshot, ReadError> {
    let mut dev = open_entry(entry)?;
    Ok(read_device(&mut dev))
}
