//! Guarded edits to an existing onboard profile.
//!
//! [`plan_edit`] reads the profile, applies field edits to a copy of its
//! bytes, and checks the result independently: only the bytes belonging to
//! the edited fields (plus the CRC) may differ. [`commit_edit`] backs up the
//! whole device, writes through `replace_sector` (compare-and-swap, read-back,
//! rollback), then re-reads and confirms that nothing else moved.

use std::path::{Path, PathBuf};

use lg_hidpp::features::onboard_profiles_write::replace_sector;
use lg_hidpp::features::{adjustable_dpi, onboard_profiles, report_rate};
use lg_hidpp::{Device, Transport};
use lg_profiles::{Descriptor, Directory, DpiLimits, EditLimits, Profile, ProfileEdit, Sector, diff_offsets};

use crate::{DeviceEntry, DeviceSnapshot, open_entry, read_device, save_backup};

/// Product IDs whose profile layout and write sequence have been verified on
/// real hardware. Everything else stays read-only.
pub const WRITE_VERIFIED_PIDS: &[u16] = &[0xc332, 0x407f];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("editing is not enabled for this device: only models verified on real hardware can be written")]
    NotWriteVerified,
    #[error("{0}")]
    Device(String),
    #[error("{0}")]
    Invalid(String),
    #[error("internal check failed: {0}. Nothing was written")]
    CheckFailed(String),
    #[error("{0}")]
    Backup(String),
    #[error("{0}")]
    Write(String),
    #[error("the write succeeded but verification afterwards failed: {0}. A backup is in {1}")]
    VerifyFailed(String, String),
}

fn dev_err(e: lg_hidpp::Error) -> EditError {
    EditError::Device(e.to_string())
}

/// A checked, byte-level change to one profile sector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPlan {
    pub address: u16,
    sector_size: u16,
    button_count: u8,
    /// Sectors that must not change: the directory and the other profiles.
    other_sectors: Vec<u16>,
    original: Vec<u8>,
    new: Vec<u8>,
    /// Offsets that differ between `original` and `new`.
    pub changed: Vec<usize>,
}

impl EditPlan {
    pub fn is_noop(&self) -> bool {
        self.changed.is_empty()
    }

    pub fn before(&self) -> Profile {
        Profile::decode(Sector::new(self.address, self.original.clone()), self.button_count).expect("validated in plan_edit")
    }

    pub fn after(&self) -> Profile {
        Profile::decode(Sector::new(self.address, self.new.clone()), self.button_count).expect("validated in plan_edit")
    }
}

/// Build and check the change for `edits` on the user profile stored at
/// sector `address`. Reads only.
pub fn plan_edit<T: Transport>(dev: &mut Device<T>, address: u16, edits: &[ProfileEdit]) -> Result<EditPlan, EditError> {
    let raw = onboard_profiles::info_raw(dev).map_err(dev_err)?;
    let d = Descriptor::parse(&raw).ok_or_else(|| EditError::Device("short onboard-profiles descriptor".into()))?;
    if !d.layout_supported() {
        return Err(EditError::Invalid(format!(
            "profile format {} with {}-byte sectors is not supported",
            d.profile_format, d.sector_size
        )));
    }
    let dir_bytes = onboard_profiles::read_sector(dev, 0, d.sector_size).map_err(dev_err)?;
    let dir = Directory::parse(&Sector::new(0, dir_bytes)).ok_or_else(|| {
        EditError::Invalid("the user profile directory is blank or invalid; only existing profiles can be edited".into())
    })?;
    if !dir.entries.iter().any(|e| e.sector == address) {
        return Err(EditError::Invalid(format!("sector {address:#06x} is not a profile in the directory")));
    }
    let original_bytes = onboard_profiles::read_sector(dev, address, d.sector_size).map_err(dev_err)?;
    let sector = Sector::new(address, original_bytes.clone());
    if !sector.crc_valid() {
        return Err(EditError::Invalid(format!("sector {address:#06x} has an invalid CRC; refusing to edit it")));
    }
    let original = Profile::decode(sector, d.button_count).map_err(|e| EditError::Invalid(e.to_string()))?;

    let limits = EditLimits {
        dpi: match adjustable_dpi::dpi_list(dev, 0) {
            Ok(adjustable_dpi::DpiList::Range { min, max, step }) => Some(DpiLimits { min, max, step }),
            Ok(adjustable_dpi::DpiList::Discrete(_)) | Err(_) => None,
        },
        report_rates_hz: report_rate::supported_rates_hz(dev).unwrap_or_default(),
    };
    if limits.dpi.is_none() && edits.iter().any(|e| matches!(e, ProfileEdit::SetDpi { .. })) {
        return Err(EditError::Invalid("the sensor's DPI range could not be read, so DPI edits are refused".into()));
    }
    let edited = original.apply_all(edits, &limits).map_err(|e| EditError::Invalid(e.to_string()))?;

    // Independent check: nothing outside the edited fields and the CRC may differ.
    let n = original.bytes().len();
    let mut allowed: Vec<usize> = edits.iter().flat_map(ProfileEdit::offsets).collect();
    allowed.extend([n - 2, n - 1]);
    let changed = diff_offsets(original.bytes(), edited.bytes());
    if let Some(bad) = changed.iter().find(|o| !allowed.contains(o)) {
        return Err(EditError::CheckFailed(format!("the edit would change byte {bad:#04x}, outside the edited fields")));
    }
    let touched_buttons: Vec<usize> = edits
        .iter()
        .filter_map(|e| match e {
            ProfileEdit::SetButton { index, .. } => Some(*index),
            _ => None,
        })
        .collect();
    for (ix, (a, b)) in original.buttons().iter().zip(edited.buttons()).enumerate() {
        if *a != b && !touched_buttons.contains(&ix) {
            return Err(EditError::CheckFailed(format!("the edit would change button {ix}")));
        }
    }
    if edited.alt_buttons() != original.alt_buttons() || edited.name() != original.name() || edited.leds() != original.leds()
    {
        return Err(EditError::CheckFailed("the edit would change G-shift bindings, the name or lighting".into()));
    }

    Ok(EditPlan {
        address,
        sector_size: d.sector_size,
        button_count: d.button_count,
        other_sectors: std::iter::once(0).chain(dir.entries.iter().map(|e| e.sector).filter(|s| *s != address)).collect(),
        original: original_bytes,
        new: edited.bytes().to_vec(),
        changed,
    })
}

fn read_others<T: Transport>(dev: &mut Device<T>, plan: &EditPlan) -> Result<Vec<Vec<u8>>, EditError> {
    plan.other_sectors.iter().map(|&s| onboard_profiles::read_sector(dev, s, plan.sector_size).map_err(dev_err)).collect()
}

/// Back up the device, write the planned change, and verify the outcome.
/// Returns the backup directory. A no-op plan writes nothing and returns
/// `None`.
pub fn commit_edit<T: Transport>(
    dev: &mut Device<T>,
    plan: &EditPlan,
    label: &str,
    backup_dir: &Path,
) -> Result<Option<PathBuf>, EditError> {
    if plan.is_noop() {
        return Ok(None);
    }
    let (backup, failed) = save_backup(dev, label, backup_dir).map_err(EditError::Backup)?;
    if failed > 0 {
        return Err(EditError::Backup(format!(
            "{failed} sectors could not be backed up to {}; nothing was written",
            backup.display()
        )));
    }
    let others_before = read_others(dev, plan)?;

    replace_sector(dev, plan.address, &plan.original, &plan.new).map_err(|e| EditError::Write(e.to_string()))?;

    let verify_failed = |what: &str| EditError::VerifyFailed(what.to_string(), backup.display().to_string());
    let now = onboard_profiles::read_sector(dev, plan.address, plan.sector_size).map_err(dev_err)?;
    if now != plan.new {
        return Err(verify_failed("the profile does not hold the intended bytes"));
    }
    if read_others(dev, plan)? != others_before {
        return Err(verify_failed("the directory or another profile changed"));
    }
    Ok(Some(backup))
}

/// Plan and commit `edits` on a discovered device, then read it again.
/// Refuses devices that are not write-verified. Blocking.
pub fn edit_entry(
    entry: &DeviceEntry,
    address: u16,
    edits: &[ProfileEdit],
    backup_dir: &Path,
) -> Result<DeviceSnapshot, EditError> {
    if !entry.write_verified() {
        return Err(EditError::NotWriteVerified);
    }
    let mut dev = open_entry(entry).map_err(|e| EditError::Device(e.to_string()))?;
    let plan = plan_edit(&mut dev, address, edits)?;
    commit_edit(&mut dev, &plan, &format!("{} {}", entry.model, entry.location()), backup_dir)?;
    Ok(read_device(&mut dev))
}
