//! `set-dpi`: change one DPI stage of one onboard profile.
//!
//! The change is built by patching the two bytes of that stage in the sector
//! as read from the device, recomputing the CRC, and handing both versions to
//! `replace_sector`, which only writes if the device still holds the bytes the
//! patch was based on, verifies by read-back, and rolls back on failure.

use std::path::Path;

use anyhow::{Context, Result, bail};
use lg_hidpp::discovery::{self, NodeKind};
use lg_hidpp::features::onboard_profiles_write::replace_sector;
use lg_hidpp::features::{adjustable_dpi, onboard_profiles};
use lg_hidpp::{DEVICE_INDEX_DIRECT, Device, HidrawTransport, Transport};
use lg_profiles::{DPI_SLOTS, Directory, DpiLimits, Profile, ProfileEdit, Sector, diff_offsets};

/// Product IDs whose profile layout and write sequence have been verified on
/// real hardware. Everything else stays read-only.
const WRITE_VERIFIED_PIDS: &[u16] = &[0xc332];

/// Offset of the first DPI word in a profile sector.
const DPI_OFFSET: usize = 3;

pub fn run(device: Option<&Path>, profile: u8, stage: u8, dpi: u16, yes: bool, backup_dir: &Path) -> Result<()> {
    if profile == 0 {
        bail!("profiles are numbered from 1");
    }
    if stage == 0 || stage as usize > DPI_SLOTS {
        bail!("stage must be 1 to {DPI_SLOTS}");
    }
    let nodes: Vec<_> = discovery::enumerate()?
        .into_iter()
        .filter(|n| n.kind == NodeKind::Device && WRITE_VERIFIED_PIDS.contains(&n.product))
        .filter(|n| device.is_none_or(|d| d == n.dev_path))
        .collect();
    let node = match nodes.as_slice() {
        [one] => one,
        [] => bail!(
            "no write-verified device found. Writes are enabled only for models checked on real hardware \
             (currently the G502 Proteus Spectrum, 046d:c332)"
        ),
        _ => bail!("more than one matching device; pick one with --device"),
    };
    let label = format!("{} ({})", node.model_name(), node.dev_path.display());
    let transport =
        HidrawTransport::open(&node.dev_path).with_context(|| format!("opening {}", node.dev_path.display()))?;
    let mut dev = Device::open(transport, DEVICE_INDEX_DIRECT)?;

    let d = crate::read_descriptor(&mut dev)?;
    let dir_bytes = onboard_profiles::read_sector(&mut dev, 0, d.sector_size)?;
    let dir = Directory::parse(&Sector::new(0, dir_bytes))
        .context("the user profile directory is blank or invalid; this command only edits existing profiles")?;
    let entry = dir
        .entries
        .get(profile as usize - 1)
        .with_context(|| format!("the device has {} profiles", dir.entries.len()))?;

    let original_bytes = onboard_profiles::read_sector(&mut dev, entry.sector, d.sector_size)?;
    let original_sector = Sector::new(entry.sector, original_bytes.clone());
    if !original_sector.crc_valid() {
        bail!("sector {:#06x} has an invalid CRC; refusing to edit it", entry.sector);
    }
    let original = Profile::decode(original_sector, d.button_count)?;

    let limits = match adjustable_dpi::dpi_list(&mut dev, 0)? {
        adjustable_dpi::DpiList::Range { min, max, step } => DpiLimits { min, max, step },
        adjustable_dpi::DpiList::Discrete(_) => bail!("this sensor reports a discrete DPI list, which is not supported yet"),
    };
    let stage_ix = stage as usize - 1;
    let edited = original.apply(ProfileEdit::SetDpi { stage: stage_ix, dpi }, Some(limits))?;

    // Independent check: nothing outside this stage's two bytes and the CRC may differ.
    let n = original.bytes().len();
    let allowed = [DPI_OFFSET + stage_ix * 2, DPI_OFFSET + stage_ix * 2 + 1, n - 2, n - 1];
    let changed = diff_offsets(original.bytes(), edited.bytes());
    if let Some(bad) = changed.iter().find(|o| !allowed.contains(o)) {
        bail!("internal error: the edit would change byte {bad:#04x}, outside the DPI field and CRC");
    }
    if edited.buttons() != original.buttons() {
        bail!("internal error: the edit would change button bindings");
    }

    println!("== {label}");
    println!("  profile {profile} (sector {:#06x}), stage {stage}", entry.sector);
    println!("  DPI stages now   {:?}", original.dpi_stages());
    println!("  DPI stages after {:?}", edited.dpi_stages());
    println!("  bytes that change: {}", changed.iter().map(|o| format!("{o:#04x}")).collect::<Vec<_>>().join(" "));
    if changed.is_empty() {
        println!("  nothing to do: the stage already has that value");
        return Ok(());
    }
    if !yes {
        println!("  dry run: nothing was written. Add --yes to write.");
        return Ok(());
    }

    crate::backup(&mut dev, &label, backup_dir)?;
    let others_before = read_others(&mut dev, &dir, entry.sector, d.sector_size)?;
    let sensor_before = adjustable_dpi::sensor_dpi(&mut dev, 0).ok().map(|s| s.current);

    replace_sector(&mut dev, entry.sector, original.bytes(), edited.bytes())?;

    // Everything below re-reads from the device; nothing is assumed.
    let after_bytes = onboard_profiles::read_sector(&mut dev, entry.sector, d.sector_size)?;
    let after = Profile::decode(Sector::new(entry.sector, after_bytes.clone()), d.button_count)?;
    let others_after = read_others(&mut dev, &dir, entry.sector, d.sector_size)?;
    let sensor_after = adjustable_dpi::sensor_dpi(&mut dev, 0).ok().map(|s| s.current);

    println!("  written and verified by read-back");
    println!("  DPI stages read back {:?}", after.dpi_stages());
    println!("  sector matches intended bytes: {}", after_bytes == edited.bytes());
    println!("  CRC valid: {}", after.sector().crc_valid());
    println!("  button bindings unchanged: {}", after.buttons() == original.buttons());
    println!("  directory and other profiles unchanged: {}", others_before == others_after);
    println!("  live sensor DPI before {sensor_before:?}, after {sensor_after:?}");
    if after_bytes != edited.bytes() || others_before != others_after {
        bail!("verification failed after writing; see the backup in {}", backup_dir.display());
    }
    Ok(())
}

/// The directory sector and every profile sector except `skip`.
fn read_others<T: Transport>(dev: &mut Device<T>, dir: &Directory, skip: u16, size: u16) -> Result<Vec<Vec<u8>>> {
    let mut out = vec![onboard_profiles::read_sector(dev, 0, size)?];
    for e in dir.entries.iter().filter(|e| e.sector != skip) {
        out.push(onboard_profiles::read_sector(dev, e.sector, size)?);
    }
    Ok(out)
}
