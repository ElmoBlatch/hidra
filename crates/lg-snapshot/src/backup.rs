//! Save every onboard-memory sector to disk.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use lg_hidpp::features::onboard_profiles;
use lg_hidpp::{Device, Transport};
use lg_profiles::{Descriptor, Sector};
use serde::Serialize;

#[derive(Serialize)]
struct Manifest {
    tool: String,
    created_unix: u64,
    device: String,
    protocol: String,
    descriptor_hex: String,
    active_profile: Option<u16>,
    sectors: Vec<SectorRecord>,
}

#[derive(Serialize)]
struct SectorRecord {
    address: String,
    file: Option<String>,
    crc_valid: Option<bool>,
    erased: bool,
    zeroed: bool,
    error: Option<String>,
}

/// Read every user sector and factory profile and write them, with a
/// manifest, into a new timestamped directory under `out`. Returns that
/// directory and how many sectors could not be read.
pub fn save_backup<T: Transport>(
    dev: &mut Device<T>,
    label: &str,
    out: &Path,
) -> Result<(PathBuf, usize), String> {
    let raw = onboard_profiles::info_raw(dev).map_err(|e| e.to_string())?;
    let d = Descriptor::parse(&raw).ok_or("short onboard-profiles descriptor")?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let slug: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let dir = out.join(format!("{now}-{}", slug.trim_matches('-')));
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;

    let mut addresses: Vec<u16> = (0..d.sector_count as u16).collect();
    addresses.extend((1..=d.rom_profile_count as u16).map(|n| 0x0100 | n));
    let mut records = Vec::new();
    for addr in addresses {
        let address = format!("{addr:#06x}");
        match onboard_profiles::read_sector(dev, addr, d.sector_size) {
            Ok(bytes) => {
                let s = Sector::new(addr, bytes);
                let file = format!("sector-{addr:04x}.bin");
                std::fs::write(dir.join(&file), &s.bytes)
                    .map_err(|e| format!("writing {file}: {e}"))?;
                records.push(SectorRecord {
                    address,
                    file: Some(file),
                    crc_valid: (!s.is_rom() && !s.is_erased() && !s.is_zeroed())
                        .then(|| s.crc_valid()),
                    erased: s.is_erased(),
                    zeroed: s.is_zeroed(),
                    error: None,
                });
            }
            Err(e) => records.push(SectorRecord {
                address,
                file: None,
                crc_valid: None,
                erased: false,
                zeroed: false,
                error: Some(e.to_string()),
            }),
        }
    }
    let failed = records.iter().filter(|r| r.file.is_none()).count();
    let (maj, min) = dev.protocol_version();
    let manifest = Manifest {
        tool: format!("hidra {}", env!("CARGO_PKG_VERSION")),
        created_unix: now,
        device: label.to_string(),
        protocol: format!("{maj}.{min}"),
        descriptor_hex: raw
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
        active_profile: onboard_profiles::active_profile(dev).ok(),
        sectors: records,
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("manifest.json"), json)
        .map_err(|e| format!("writing manifest: {e}"))?;
    Ok((dir, failed))
}
