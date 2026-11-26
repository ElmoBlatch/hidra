//! `set` / `set-dpi`: change fields of an existing onboard profile through
//! the guarded edit path in `lg-snapshot`.

use std::path::Path;

use anyhow::{Context, Result, bail};
use lg_hidpp::features::{adjustable_dpi, onboard_profiles, report_rate};
use lg_profiles::{ButtonBinding, DPI_SLOTS, Directory, Profile, ProfileEdit, Sector};
use lg_snapshot::{commit_edit, discover, open_entry, plan_edit};

/// What the user asked to change, before it is resolved against the device.
#[derive(Debug, Default)]
pub struct Request {
    /// `(stage 1-based, dpi)`.
    pub dpi: Vec<(u8, u16)>,
    /// 1-based.
    pub default_stage: Option<u8>,
    pub polling_hz: Option<u16>,
    /// `(button name, action text)`.
    pub buttons: Vec<(String, String)>,
}

/// Parse `KEY=VALUE`.
pub fn split_pair(text: &str) -> Result<(String, String)> {
    let (k, v) = text
        .split_once('=')
        .with_context(|| format!("expected NAME=VALUE, got \"{text}\""))?;
    Ok((k.trim().to_string(), v.trim().to_string()))
}

/// Button index from `G1`..`G16`, a 1-based number, or `tilt-right` /
/// `tilt-left` (G502 layout).
fn button_index(name: &str) -> Result<usize> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "tilt-right" => return Ok(9),
        "tilt-left" => return Ok(10),
        _ => {}
    }
    let digits = lower.strip_prefix('g').unwrap_or(&lower);
    match digits.parse::<usize>() {
        Ok(n) if n >= 1 => Ok(n - 1),
        _ => bail!(
            "unknown button \"{name}\" (use G1..G9, tilt-left, tilt-right, or a button number)"
        ),
    }
}

fn summarize(p: &Profile) -> Vec<String> {
    let stages: Vec<String> = p
        .dpi_stages()
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let tag = if i as u8 == p.default_stage() {
                "*"
            } else {
                ""
            };
            if v == 0 {
                format!("-{tag}")
            } else {
                format!("{v}{tag}")
            }
        })
        .collect();
    let mut lines = vec![
        format!("DPI stages  {}", stages.join(" / ")),
        format!("polling     {} Hz", p.report_rate_hz()),
    ];
    lines.extend(
        p.buttons()
            .iter()
            .enumerate()
            .map(|(i, b)| format!("button {:<2}   {}", i + 1, b.label())),
    );
    lines
}

pub fn run(
    device: Option<&Path>,
    profile: u8,
    request: Request,
    yes: bool,
    backup_dir: &Path,
) -> Result<()> {
    if profile == 0 {
        bail!("profiles are numbered from 1");
    }
    let mut edits = Vec::new();
    for &(stage, dpi) in &request.dpi {
        if stage == 0 || stage as usize > DPI_SLOTS {
            bail!("stage must be 1 to {DPI_SLOTS}");
        }
        edits.push(ProfileEdit::SetDpi {
            stage: stage as usize - 1,
            dpi,
        });
    }
    if let Some(stage) = request.default_stage {
        if stage == 0 || stage as usize > DPI_SLOTS {
            bail!("default stage must be 1 to {DPI_SLOTS}");
        }
        edits.push(ProfileEdit::SetDefaultStage(stage - 1));
    }
    if let Some(hz) = request.polling_hz {
        edits.push(ProfileEdit::SetReportRate(hz));
    }
    for (name, action) in &request.buttons {
        edits.push(ProfileEdit::SetButton {
            index: button_index(name)?,
            binding: ButtonBinding::parse(action)?,
        });
    }
    if edits.is_empty() {
        bail!(
            "nothing to change: give at least one of --dpi, --default-stage, --polling, --button"
        );
    }

    let entries: Vec<_> = discover()?
        .into_iter()
        .filter(|e| e.write_verified())
        .filter(|e| device.is_none_or(|d| d.display().to_string() == e.id))
        .collect();
    let entry = match entries.as_slice() {
        [one] => one,
        [] => bail!(
            "no write-verified device found. Editing is enabled only for models checked on real hardware \
             (currently the G502 Proteus Spectrum, 046d:c332)"
        ),
        _ => bail!("more than one matching device; pick one with --device"),
    };
    let label = format!("{} ({})", entry.model, entry.location());
    let mut dev = open_entry(entry)?;

    // Profile number -> sector, through the directory.
    let raw = onboard_profiles::info_raw(&mut dev)?;
    let d = lg_profiles::Descriptor::parse(&raw).context("short onboard-profiles descriptor")?;
    let dir = Directory::parse(&Sector::new(
        0,
        onboard_profiles::read_sector(&mut dev, 0, d.sector_size)?,
    ))
    .context(
        "the user profile directory is blank or invalid; only existing profiles can be edited",
    )?;
    let address = dir
        .entries
        .get(profile as usize - 1)
        .with_context(|| format!("the device has {} profiles", dir.entries.len()))?
        .sector;

    let plan = plan_edit(&mut dev, address, &edits)?;
    println!("== {label}");
    println!("  profile {profile} (sector {address:#06x})");
    let (before, after) = (summarize(&plan.before()), summarize(&plan.after()));
    for (b, a) in before.iter().zip(&after) {
        if b == a {
            println!("    {b}");
        } else {
            println!("  - {b}");
            println!("  + {a}");
        }
    }
    println!(
        "  bytes that change: {}",
        plan.changed
            .iter()
            .map(|o| format!("{o:#04x}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    if plan.is_noop() {
        println!("  nothing to do: the profile already has those values");
        return Ok(());
    }
    if !yes {
        println!("  dry run: nothing was written. Add --yes to write.");
        return Ok(());
    }

    let sensor_before = adjustable_dpi::sensor_dpi(&mut dev, 0)
        .ok()
        .map(|s| s.current);
    let rate_before = report_rate::current_rate_hz(&mut dev).ok();
    let backup = commit_edit(&mut dev, &plan, &label, backup_dir)?;
    let sensor_after = adjustable_dpi::sensor_dpi(&mut dev, 0)
        .ok()
        .map(|s| s.current);
    let rate_after = report_rate::current_rate_hz(&mut dev).ok();

    if let Some(dir) = backup {
        println!("  backup saved to {}", dir.display());
    }
    println!("  written; read-back matches, directory and other profiles unchanged");
    println!(
        "  live sensor DPI {sensor_before:?} -> {sensor_after:?}, live polling {rate_before:?} -> {rate_after:?} Hz"
    );
    Ok(())
}
