//! Application state that does not depend on GPUI: where devices come from,
//! the per-device read lifecycle, and selection rules.

use std::path::PathBuf;
use std::sync::Arc;

use lg_snapshot::{DeviceEntry, DeviceSnapshot, ProfileSet, ReadError};

/// Where the window reads devices from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Live hidraw nodes found through sysfs.
    Hardware,
    /// A `ratbagd --verbose=raw` capture; every segment, or one by name.
    Replay { log: PathBuf, segment: Option<String> },
    /// A fixed set of devices, e.g. simulated mice for demo mode and tests.
    Fixed(Vec<DeviceEntry>),
}

impl Source {
    /// A simulated G502 Proteus Spectrum seeded from the bundled capture.
    /// Edits work and are kept in memory only.
    pub fn demo() -> Result<Self, String> {
        const CAPTURE: &str = include_str!("../../../captures/ratbagd/g502-proteus-spectrum-c332.log");
        let mut entry =
            lg_snapshot::simulated_entry_from_log(CAPTURE, "Logitech Gaming Mouse G502").map_err(|e| e.to_string())?;
        entry.model = "G502 Proteus Spectrum".to_string();
        Ok(Self::Fixed(vec![entry]))
    }

    /// Find devices. Blocking (sysfs or file I/O).
    pub fn discover(&self) -> Result<Vec<DeviceEntry>, String> {
        match self {
            Self::Hardware => lg_snapshot::discover().map_err(|e| format!("Couldn't list hidraw devices: {e}")),
            Self::Fixed(entries) => Ok(entries.clone()),
            Self::Replay { log, segment } => {
                lg_snapshot::replay_entries(log, segment.as_deref()).map_err(|e| e.to_string())
            }
        }
    }
}

/// Where automatic pre-write backups go: `$XDG_DATA_HOME/hidra/backups`,
/// falling back to `~/.local/share/hidra/backups`.
pub fn default_backup_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("hidra").join("backups")
}

/// Discovery lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discovery {
    Loading,
    Ready,
    Failed(String),
}

/// One device's read lifecycle. A refresh keeps the previous snapshot visible
/// until the new one arrives.
#[derive(Debug, Clone)]
pub enum DeviceRead {
    Reading { previous: Option<Arc<DeviceSnapshot>> },
    Ready(Arc<DeviceSnapshot>),
    Failed(ReadError),
}

impl DeviceRead {
    /// The snapshot to display, if any.
    pub fn snapshot(&self) -> Option<&Arc<DeviceSnapshot>> {
        match self {
            Self::Reading { previous } => previous.as_ref(),
            Self::Ready(s) => Some(s),
            Self::Failed(_) => None,
        }
    }

    pub fn is_reading(&self) -> bool {
        matches!(self, Self::Reading { .. })
    }
}

/// Keep the current selection if that device is still present, else select
/// the first device.
pub fn reconcile_selection(current: Option<&str>, devices: &[DeviceEntry]) -> Option<String> {
    current
        .filter(|id| devices.iter().any(|d| d.id == *id))
        .map(str::to_string)
        .or_else(|| devices.first().map(|d| d.id.clone()))
}

/// Move the selection by `delta` places, clamped to the ends.
pub fn step<T: PartialEq + Clone>(items: &[T], current: Option<&T>, delta: isize) -> Option<T> {
    if items.is_empty() {
        return None;
    }
    let ix = current.and_then(|c| items.iter().position(|i| i == c));
    let next = match ix {
        Some(ix) => (ix as isize + delta).clamp(0, items.len() as isize - 1) as usize,
        None => 0,
    };
    Some(items[next].clone())
}

/// The profile shown when the user has not chosen one: the active profile,
/// else the first.
pub fn default_profile(set: &ProfileSet) -> Option<u16> {
    set.active().or(set.profiles.first()).map(|p| p.address)
}

/// The profile to show: the user's choice if it still exists, else the default.
pub fn shown_profile(set: &ProfileSet, chosen: Option<u16>) -> Option<u16> {
    chosen.filter(|a| set.profiles.iter().any(|p| p.address == *a)).or_else(|| default_profile(set))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_clamps_and_starts_at_first() {
        let items = ["a", "b", "c"];
        assert_eq!(step(&items, None, 1), Some("a"));
        assert_eq!(step(&items, Some(&"a"), 1), Some("b"));
        assert_eq!(step(&items, Some(&"c"), 1), Some("c"));
        assert_eq!(step(&items, Some(&"a"), -1), Some("a"));
        assert_eq!(step::<&str>(&[], None, 1), None);
    }
}
