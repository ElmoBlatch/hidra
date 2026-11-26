//! First-time setup of user profiles on a mouse whose onboard memory has
//! never been written (blank directory, running its factory ROM profile).
//!
//! The factory profile is copied byte-for-byte into user sector 1 (only the
//! CRC is added), then a directory enabling it is written, then the mouse is
//! switched to it. The order matters: a valid directory never points at a
//! profile that is not already in place and verified. Undoing it means
//! writing the directory sector back to its erased state, which
//! `restore_from_backup` does.

use std::path::{Path, PathBuf};

use lg_hidpp::features::onboard_profiles;
use lg_hidpp::features::onboard_profiles_write::{
    replace_sector, restore_sector, set_active_profile,
};
use lg_hidpp::{Device, Transport};
use lg_profiles::{Descriptor, Directory, Profile, Sector, crc_ccitt};

use crate::{EditError, save_backup};

const DIRECTORY: u16 = 0;
const FIRST_PROFILE: u16 = 1;

/// The two sectors that set up user profiles, built and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitPlan {
    sector_size: u16,
    button_count: u8,
    /// Factory profile address the copy was made from.
    pub source: u16,
    erased: Vec<u8>,
    pub profile: Vec<u8>,
    pub directory: Vec<u8>,
}

impl InitPlan {
    pub fn profile(&self) -> Profile {
        Profile::decode(
            Sector::new(FIRST_PROFILE, self.profile.clone()),
            self.button_count,
        )
        .expect("checked in plan")
    }
}

fn dev_err(e: lg_hidpp::Error) -> EditError {
    EditError::Device(e.to_string())
}

fn with_crc(mut bytes: Vec<u8>) -> Vec<u8> {
    let n = bytes.len();
    let crc = crc_ccitt(&bytes[..n - 2]);
    bytes[n - 2..].copy_from_slice(&crc.to_be_bytes());
    bytes
}

/// The directory libratbag writes: one entry `[00, n, enabled, 00]` per
/// profile slot, then `ff ff 00 00`, padded with `ff`, then the CRC.
fn build_directory(slots: u8, enabled: &[u8], sector_size: usize) -> Vec<u8> {
    let mut bytes = vec![0xff; sector_size];
    let mut o = 0;
    for slot in 1..=slots {
        bytes[o..o + 4].copy_from_slice(&[0x00, slot, enabled.contains(&slot) as u8, 0x00]);
        o += 4;
    }
    bytes[o..o + 4].copy_from_slice(&[0xff, 0xff, 0x00, 0x00]);
    with_crc(bytes)
}

/// Check that the device is in the blank state and build the setup. Reads only.
pub fn plan_initialize<T: Transport>(dev: &mut Device<T>) -> Result<InitPlan, EditError> {
    let raw = onboard_profiles::info_raw(dev).map_err(dev_err)?;
    let d = Descriptor::parse(&raw)
        .ok_or_else(|| EditError::Device("short onboard-profiles descriptor".into()))?;
    if !d.layout_supported() {
        return Err(EditError::Invalid(format!(
            "profile format {} with {}-byte sectors is not supported",
            d.profile_format, d.sector_size
        )));
    }
    if d.rom_profile_count == 0 || d.profile_count == 0 {
        return Err(EditError::Invalid(
            "the device has no factory profile to copy or no user profile slot".into(),
        ));
    }
    let size = d.sector_size;
    let directory = onboard_profiles::read_sector(dev, DIRECTORY, size).map_err(dev_err)?;
    let first = onboard_profiles::read_sector(dev, FIRST_PROFILE, size).map_err(dev_err)?;
    let erased = vec![0xff; size as usize];
    if Directory::parse(&Sector::new(DIRECTORY, directory.clone())).is_some() {
        return Err(EditError::Invalid(
            "the device already has user profiles; edit them instead".into(),
        ));
    }
    if directory != erased || first != erased {
        return Err(EditError::Invalid(
            "the directory is invalid but the memory is not blank; this needs a manual look before writing".into(),
        ));
    }

    // Prefer the factory profile the mouse is running now.
    let active = onboard_profiles::active_profile(dev).map_err(dev_err)?;
    let source = if active & 0xff00 == 0x0100
        && (active & 0xff) as u8 <= d.rom_profile_count
        && active & 0xff != 0
    {
        active
    } else {
        0x0101
    };
    let rom = onboard_profiles::read_sector(dev, source, size).map_err(dev_err)?;
    let profile = with_crc(rom.clone());

    // Independent checks on the copy: only the two CRC bytes differ from the
    // factory profile, and it decodes to the same settings.
    let n = size as usize;
    if profile[..n - 2] != rom[..n - 2] {
        return Err(EditError::CheckFailed(
            "the copied profile differs from the factory profile".into(),
        ));
    }
    let (a, b) = (
        Profile::decode(Sector::new(source, rom), d.button_count)
            .map_err(|e| EditError::Invalid(e.to_string()))?,
        Profile::decode(Sector::new(FIRST_PROFILE, profile.clone()), d.button_count)
            .map_err(|e| EditError::Invalid(e.to_string()))?,
    );
    if a.dpi_stages() != b.dpi_stages() || a.buttons() != b.buttons() || !b.sector().crc_valid() {
        return Err(EditError::CheckFailed(
            "the copied profile does not decode like the factory profile".into(),
        ));
    }
    if b.dpi_stages().iter().all(|&dpi| dpi == 0) {
        return Err(EditError::Invalid(
            "the factory profile has no DPI stage; refusing to copy it".into(),
        ));
    }

    let directory = build_directory(d.profile_count, &[FIRST_PROFILE as u8], n);
    let parsed = Directory::parse(&Sector::new(DIRECTORY, directory.clone()))
        .ok_or_else(|| EditError::CheckFailed("the new directory does not parse".into()))?;
    let enabled: Vec<u16> = parsed
        .entries
        .iter()
        .filter(|e| e.enabled)
        .map(|e| e.sector)
        .collect();
    if enabled != [FIRST_PROFILE] || parsed.entries.len() != d.profile_count as usize {
        return Err(EditError::CheckFailed(
            "the new directory does not enable exactly profile 1".into(),
        ));
    }

    Ok(InitPlan {
        sector_size: size,
        button_count: d.button_count,
        source,
        erased,
        profile,
        directory,
    })
}

/// What [`commit_initialize`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitOutcome {
    pub backup: PathBuf,
    /// Active profile address before and after.
    pub active_before: u16,
    pub active_after: u16,
}

/// Back up, write the profile, write the directory, switch to the profile.
/// If the directory write or the switch fails, the directory is put back to
/// its erased state so the mouse keeps running its factory profile.
pub fn commit_initialize<T: Transport>(
    dev: &mut Device<T>,
    plan: &InitPlan,
    label: &str,
    backup_dir: &Path,
) -> Result<InitOutcome, EditError> {
    let (backup, failed) = save_backup(dev, label, backup_dir).map_err(EditError::Backup)?;
    if failed > 0 {
        return Err(EditError::Backup(format!(
            "{failed} sectors could not be backed up; nothing was written"
        )));
    }
    let active_before = onboard_profiles::active_profile(dev).map_err(dev_err)?;
    let write = |e: lg_hidpp::Error| EditError::Write(e.to_string());

    replace_sector(dev, FIRST_PROFILE, &plan.erased, &plan.profile).map_err(write)?;
    if let Err(e) = replace_sector(dev, DIRECTORY, &plan.erased, &plan.directory) {
        // replace_sector already restored the directory; also clear the
        // now-unreferenced profile so the device is exactly as before.
        let _ = restore_sector(dev, FIRST_PROFILE, &plan.erased);
        return Err(write(e));
    }
    let switched = set_active_profile(dev, FIRST_PROFILE)
        .and_then(|()| onboard_profiles::active_profile(dev))
        .map_err(|e| e.to_string())
        .and_then(|now| {
            if now == FIRST_PROFILE {
                Ok(now)
            } else {
                Err(format!("the mouse reports profile {now:#06x}"))
            }
        });
    match switched {
        Ok(active_after) => Ok(InitOutcome {
            backup,
            active_before,
            active_after,
        }),
        Err(cause) => {
            let undone = restore_sector(dev, DIRECTORY, &plan.erased).is_ok()
                && restore_sector(dev, FIRST_PROFILE, &plan.erased).is_ok();
            Err(EditError::Write(format!(
                "the profiles were written but the mouse would not switch to them ({cause}); the memory {}",
                if undone {
                    "was put back to blank"
                } else {
                    "could NOT be put back; restore from the backup"
                }
            )))
        }
    }
}

/// Put every user sector back from a backup directory written by
/// `save_backup`, then switch to the profile the backup recorded as active
/// when it is a factory profile. Returns how many sectors were rewritten.
///
/// The backup must come from a device with the same onboard-memory
/// descriptor.
pub fn restore_from_backup<T: Transport>(
    dev: &mut Device<T>,
    backup: &Path,
) -> Result<usize, EditError> {
    let invalid = |m: String| EditError::Invalid(m);
    let manifest = std::fs::read_to_string(backup.join("manifest.json"))
        .map_err(|e| invalid(format!("reading {}/manifest.json: {e}", backup.display())))?;
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest).map_err(|e| invalid(format!("manifest: {e}")))?;
    let raw = onboard_profiles::info_raw(dev).map_err(dev_err)?;
    let descriptor = raw
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    if manifest["descriptor_hex"].as_str() != Some(descriptor.as_str()) {
        return Err(invalid(
            "this backup is from a device with a different onboard-memory layout".into(),
        ));
    }
    let d = Descriptor::parse(&raw)
        .ok_or_else(|| EditError::Device("short onboard-profiles descriptor".into()))?;

    // Load everything before writing anything.
    let mut sectors = Vec::new();
    for address in 0..d.sector_count as u16 {
        let file = backup.join(format!("sector-{address:04x}.bin"));
        let bytes = std::fs::read(&file)
            .map_err(|e| invalid(format!("reading {}: {e}", file.display())))?;
        if bytes.len() != d.sector_size as usize {
            return Err(invalid(format!(
                "{} is {} bytes, expected {}",
                file.display(),
                bytes.len(),
                d.sector_size
            )));
        }
        sectors.push((address, bytes));
    }
    // Profiles and data first, the directory last, so the directory never
    // points at sectors that are not yet in place.
    let mut rewritten = 0;
    for (address, bytes) in sectors
        .iter()
        .filter(|(a, _)| *a != DIRECTORY)
        .chain(sectors.iter().filter(|(a, _)| *a == DIRECTORY))
    {
        let before =
            onboard_profiles::read_sector(dev, *address, d.sector_size).map_err(dev_err)?;
        if &before != bytes {
            restore_sector(dev, *address, bytes).map_err(|e| EditError::Write(e.to_string()))?;
            rewritten += 1;
        }
    }
    if let Some(active) = manifest["active_profile"].as_u64().map(|a| a as u16)
        && active & 0xff00 == 0x0100
    {
        // Best effort: a blank directory makes the mouse fall back to ROM by itself.
        let _ = set_active_profile(dev, active);
    }
    Ok(rewritten)
}
