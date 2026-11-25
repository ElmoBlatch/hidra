//! `init-profiles` and `restore`.

use std::path::Path;

use anyhow::{Context, Result, bail};
use lg_hidpp::features::{adjustable_dpi, onboard_profiles};
use lg_snapshot::{DeviceEntry, commit_initialize, discover, open_entry, plan_initialize, restore_from_backup};

/// Models on which first-time profile setup has been verified on hardware.
/// G502 LIGHTSPEED (0x407f): run on 2026-11-25, firmware MPM 17.00.B0008.
const INIT_VERIFIED_PIDS: &[u16] = &[0x407f];

fn pick(device: Option<&Path>, filter: impl Fn(&DeviceEntry) -> bool, none: &str) -> Result<DeviceEntry> {
    let mut entries: Vec<DeviceEntry> = discover()?
        .into_iter()
        .filter(|e| device.is_none_or(|d| d.display().to_string() == e.id))
        .filter(|e| filter(e))
        .collect();
    match entries.len() {
        1 => Ok(entries.remove(0)),
        0 => bail!("{none}"),
        _ => bail!("more than one matching device; pick one with --device"),
    }
}

pub fn init_profiles(device: Option<&Path>, yes: bool, allow_unverified: bool, backup_dir: &Path) -> Result<()> {
    let entry = pick(
        device,
        |e| INIT_VERIFIED_PIDS.contains(&e.product) || (allow_unverified && device.is_some()),
        "no supported device found. First-time profile setup has not been verified on real hardware for any \
         model yet",
    )?;
    let label = format!("{} ({})", entry.model, entry.location());
    let mut dev = open_entry(&entry)?;
    let plan = plan_initialize(&mut dev)?;
    let p = plan.profile();
    println!("== {label}");
    println!("  the user memory is blank; the mouse runs factory profile {:#06x}", plan.source);
    println!("  this copies that factory profile into user profile 1 and enables it:");
    println!("    DPI stages  {:?} (default stage {})", p.dpi_stages(), p.default_stage() + 1);
    println!("    polling     {} Hz", p.report_rate_hz());
    for (i, b) in p.buttons().iter().enumerate() {
        println!("    button {:<2}   {}", i + 1, b.label());
    }
    println!("  sectors written: 0x0001 (profile), then 0x0000 (directory)");
    if !yes {
        println!("  dry run: nothing was written. Add --yes to write.");
        return Ok(());
    }
    let sensor_before = adjustable_dpi::sensor_dpi(&mut dev, 0).ok().map(|s| s.current);
    let outcome = commit_initialize(&mut dev, &plan, &label, backup_dir)?;
    let sensor_after = adjustable_dpi::sensor_dpi(&mut dev, 0).ok().map(|s| s.current);
    println!("  backup saved to {}", outcome.backup.display());
    println!("  written and verified; active profile {:#06x} -> {:#06x}", outcome.active_before, outcome.active_after);
    println!("  onboard mode {:?}, live sensor DPI {sensor_before:?} -> {sensor_after:?}", onboard_profiles::mode(&mut dev).ok());
    println!("  to undo: hidractl restore --backup {} --yes", outcome.backup.display());
    Ok(())
}

pub fn restore(device: Option<&Path>, backup: &Path, yes: bool) -> Result<()> {
    let manifest = std::fs::read_to_string(backup.join("manifest.json"))
        .with_context(|| format!("{} is not a backup directory (no manifest.json)", backup.display()))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest)?;
    // With several devices connected, the backup's own descriptor picks the target.
    let wanted = manifest["descriptor_hex"].as_str().unwrap_or("").to_string();
    let entry = pick(
        device,
        |e| {
            open_entry(e).ok().and_then(|mut d| onboard_profiles::info_raw(&mut d).ok()).is_some_and(|raw| {
                raw.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ") == wanted
            })
        },
        "no connected device has the onboard-memory layout recorded in this backup",
    )?;
    println!("== {} ({})", entry.model, entry.location());
    println!("  backup of \"{}\"", manifest["device"].as_str().unwrap_or("?"));
    if !yes {
        println!("  dry run: this would write the backup's user sectors back to the mouse. Add --yes to write.");
        return Ok(());
    }
    let mut dev = open_entry(&entry)?;
    let rewritten = restore_from_backup(&mut dev, backup)?;
    println!("  restored and verified; {rewritten} sectors rewritten");
    println!("  active profile now {:?}", onboard_profiles::active_profile(&mut dev).ok().map(|a| format!("{a:#06x}")));
    Ok(())
}
