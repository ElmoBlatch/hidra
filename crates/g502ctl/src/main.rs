//! `g502ctl`: read-only inspector and backup tool for Logitech G502 mice.
//!
//! Nothing in this binary can change device state: `lg-hidpp` exposes only
//! getter functions.

mod report;

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use lg_hidpp::discovery::{self, NodeKind};
use lg_hidpp::features::{self, adjustable_dpi, battery_voltage, device_info, device_name, onboard_profiles, report_rate};
use lg_hidpp::replay::{ReplayTransport, parse_log};
use lg_hidpp::{DEVICE_INDEX_DIRECT, Device, HidrawTransport, Transport};
use lg_profiles::{Descriptor, Directory, Profile, Sector};
use serde::Serialize;

#[derive(Parser)]
#[command(version, about = "Read-only inspector and backup tool for Logitech G502 mice")]
struct Cli {
    /// hidraw node to use (default: every detected Logitech HID++ device).
    #[arg(long, global = true, conflicts_with = "replay")]
    device: Option<PathBuf>,
    /// Answer requests from a `ratbagd --verbose=raw` log instead of hardware.
    #[arg(long, global = true)]
    replay: Option<PathBuf>,
    /// Device name within the replay log (default: first device with traffic).
    #[arg(long, global = true, requires = "replay")]
    segment: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List Logitech HID++ hidraw nodes without opening them.
    List,
    /// Feature table, firmware, DPI, polling rate, battery and profile summary.
    Info,
    /// Decode every onboard profile.
    Profiles,
    /// Save every onboard-memory sector to a directory (raw .bin plus manifest.json).
    Backup {
        #[arg(long, default_value = "backups")]
        out: PathBuf,
    },
    /// Write a shareable report (JSON with replayable traffic) for every Logitech HID++ device.
    Report {
        #[arg(long, default_value = "reports")]
        out: PathBuf,
        /// Also read macro sectors. Macros can contain text you typed.
        #[arg(long)]
        include_macros: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Command::List = cli.command {
        return list();
    }
    if let Command::Report { out, include_macros } = &cli.command {
        if cli.replay.is_some() {
            bail!("report reads live devices; it cannot be combined with --replay");
        }
        let file = report::run(out, *include_macros)?;
        println!("report written to {}", file.display());
        println!("Upload it in a \"Device report\" issue. See docs/device-reports.md for what it contains.");
        return Ok(());
    }
    for (label, mut dev) in open_targets(&cli)? {
        println!("== {label}");
        match &cli.command {
            Command::Info => info(&mut dev)?,
            Command::Profiles => profiles(&mut dev)?,
            Command::Backup { out } => backup(&mut dev, &label, out)?,
            Command::List | Command::Report { .. } => unreachable!(),
        }
        println!();
    }
    Ok(())
}

/// Either a live hidraw node or a replayed log, behind one transport type.
enum AnyTransport {
    Hidraw(HidrawTransport),
    Replay(ReplayTransport),
}

impl Transport for AnyTransport {
    fn write_report(&mut self, r: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Hidraw(t) => t.write_report(r),
            Self::Replay(t) => t.write_report(r),
        }
    }
    fn read_report(&mut self, d: Duration) -> std::io::Result<Option<Vec<u8>>> {
        match self {
            Self::Hidraw(t) => t.read_report(d),
            Self::Replay(t) => t.read_report(d),
        }
    }
    fn supports_short(&self) -> bool {
        match self {
            Self::Hidraw(t) => t.supports_short(),
            Self::Replay(t) => t.supports_short(),
        }
    }
    fn supports_long(&self) -> bool {
        match self {
            Self::Hidraw(t) => t.supports_long(),
            Self::Replay(t) => t.supports_long(),
        }
    }
}

type Dev = Device<AnyTransport>;

fn open_targets(cli: &Cli) -> Result<Vec<(String, Dev)>> {
    if let Some(log) = &cli.replay {
        let mut text = std::fs::read_to_string(log).with_context(|| format!("reading {}", log.display()))?;
        // A `g502ctl report` file carries its traffic log in the "log" field.
        if text.trim_start().starts_with('{') {
            let report: serde_json::Value = serde_json::from_str(&text).context("parsing report JSON")?;
            text = report["log"].as_str().context("report has no \"log\" field")?.to_string();
        }
        let segments = parse_log(&text);
        let seg = match &cli.segment {
            Some(name) => segments.into_iter().find(|s| &s.name == name),
            None => segments.into_iter().next(),
        }
        .context("no matching device segment with HID++ traffic in the log")?;
        let label = format!("replay: {} ({})", seg.name, seg.hidraw.as_deref().unwrap_or("?"));
        let dev = Device::open_with_timeout(
            AnyTransport::Replay(ReplayTransport::new(seg)),
            DEVICE_INDEX_DIRECT,
            Duration::from_millis(50),
        )?;
        return Ok(vec![(label, dev)]);
    }

    let paths: Vec<(String, PathBuf)> = match &cli.device {
        Some(p) => vec![(p.display().to_string(), p.clone())],
        None => discovery::enumerate()?
            .into_iter()
            .filter(|n| n.kind == NodeKind::Device)
            .map(|n| (format!("{} ({})", n.model_name(), n.dev_path.display()), n.dev_path))
            .collect(),
    };
    if paths.is_empty() {
        bail!("no Logitech HID++ devices found");
    }
    let mut out = Vec::new();
    for (label, path) in paths {
        let transport = match HidrawTransport::open(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                bail!(
                    "permission denied opening {}. Install packaging/70-g502ctl.rules (see README) or run as root",
                    path.display()
                )
            }
            Err(e) => return Err(e).with_context(|| format!("opening {}", path.display())),
        };
        match Device::open(AnyTransport::Hidraw(transport), DEVICE_INDEX_DIRECT) {
            Ok(dev) => out.push((label, dev)),
            Err(e) => eprintln!("skipping {label}: {e} (asleep or out of range?)"),
        }
    }
    Ok(out)
}

fn list() -> Result<()> {
    let nodes = discovery::enumerate()?;
    if nodes.is_empty() {
        println!("no Logitech HID++ hidraw nodes found");
    }
    for n in nodes {
        let via = n.via_receiver.map(|p| format!(" via receiver {p:04x}")).unwrap_or_default();
        println!(
            "{}  {:04x}:{:04x}  {:<24} {:?}{via}  \"{}\"",
            n.dev_path.display(),
            n.vendor,
            n.product,
            n.model_name(),
            n.kind,
            n.name
        );
    }
    Ok(())
}

/// Print an optional field; a missing feature or timeout is not fatal.
fn opt<T>(label: &str, r: lg_hidpp::Result<T>, show: impl FnOnce(T) -> String) {
    match r {
        Ok(v) => println!("  {label:<20} {}", show(v)),
        Err(lg_hidpp::Error::FeatureMissing(_)) => {}
        Err(e) => println!("  {label:<20} unavailable ({e})"),
    }
}

fn info(dev: &mut Dev) -> Result<()> {
    let (maj, min) = dev.protocol_version();
    println!("  {:<20} {maj}.{min}", "HID++ protocol");
    opt("name", device_name::name(dev), |n| n);
    opt("model", device_info::device_info(dev), |i| {
        format!("model id {:02x?}, {} firmware entities", i.model_id, i.entity_count)
    });
    if let Ok(i) = device_info::device_info(dev) {
        for e in 0..i.entity_count {
            opt(&format!("firmware[{e}]"), device_info::firmware_info(dev, e), |f| {
                format!("{} {}{}", f.kind_name(), f.version_string(), if f.active { " (active)" } else { "" })
            });
        }
    }
    opt("sensor DPI", adjustable_dpi::sensor_dpi(dev, 0), |d| format!("{} (default {})", d.current, d.default));
    opt("DPI range", adjustable_dpi::dpi_list(dev, 0), |l| match l {
        adjustable_dpi::DpiList::Range { min, max, step } => format!("{min}..={max} step {step}"),
        adjustable_dpi::DpiList::Discrete(v) => format!("{v:?}"),
    });
    opt("polling rate", report_rate::current_rate_hz(dev), |hz| format!("{hz} Hz"));
    opt("polling rates", report_rate::supported_rates_hz(dev), |v| format!("{v:?} Hz"));
    opt("battery", battery_voltage::voltage(dev), |b| {
        format!("{} mV{}", b.millivolts, if b.charging() { " (charging)" } else { "" })
    });
    if let Ok(raw) = onboard_profiles::info_raw(dev)
        && let Some(d) = Descriptor::parse(&raw)
    {
        println!(
            "  {:<20} format {}, {} user + {} ROM profiles, {} buttons, {}x{}-byte sectors",
            "onboard profiles", d.profile_format, d.profile_count, d.rom_profile_count, d.button_count,
            d.sector_count, d.sector_size
        );
        opt("onboard mode", onboard_profiles::mode(dev), |m| match m {
            1 => "onboard".into(),
            2 => "host".into(),
            m => format!("{m}"),
        });
        opt("active profile", onboard_profiles::active_profile(dev), |a| format!("address {a:#06x}"));
    }
    println!("  features:");
    for f in dev.features() {
        let mut flags = String::new();
        if f.flags.hidden() {
            flags.push_str(" hidden");
        }
        if f.flags.engineering() {
            flags.push_str(" engineering");
        }
        if f.flags.obsolete() {
            flags.push_str(" obsolete");
        }
        println!("    [{:#04x}] {:#06x} v{} {}{flags}", f.index, f.page, f.version, features::page_name(f.page));
    }
    Ok(())
}

fn read_descriptor(dev: &mut Dev) -> Result<Descriptor> {
    let raw = onboard_profiles::info_raw(dev).context("device has no onboard-profiles feature")?;
    let d = Descriptor::parse(&raw).context("short onboard-profiles descriptor")?;
    if !d.layout_supported() {
        bail!("profile format {} / sector size {} is not supported by this decoder", d.profile_format, d.sector_size);
    }
    Ok(d)
}

fn profiles(dev: &mut Dev) -> Result<()> {
    let d = read_descriptor(dev)?;
    let active = onboard_profiles::active_profile(dev).ok();
    let dir_sector = Sector::new(0, onboard_profiles::read_sector(dev, 0, d.sector_size)?);
    let targets: Vec<(String, u16)> = match Directory::parse(&dir_sector) {
        Some(dir) => dir
            .entries
            .iter()
            .map(|e| (format!("profile {}{}", e.profile, if e.enabled { "" } else { " (disabled)" }), e.sector))
            .collect(),
        None => {
            println!("  user directory is blank or invalid: device runs its factory ROM profiles");
            (1..=d.rom_profile_count as u16).map(|n| (format!("ROM profile {n}"), 0x0100 | n)).collect()
        }
    };
    for (label, addr) in targets {
        let sector = Sector::new(addr, onboard_profiles::read_sector(dev, addr, d.sector_size)?);
        let crc = if sector.is_rom() {
            "ROM, CRC not checked".to_string()
        } else if sector.crc_valid() {
            format!("CRC {:04x} ok", sector.stored_crc().unwrap_or(0))
        } else {
            format!("CRC MISMATCH stored {:04x?} computed {:04x?}", sector.stored_crc(), sector.computed_crc())
        };
        let p = Profile::decode(sector, d.button_count)?;
        let marker = if active == Some(addr) { "  << active" } else { "" };
        println!("  {label} @ sector {addr:#06x} ({crc}){marker}");
        println!("    name          {}", p.name().unwrap_or_else(|| "(unnamed)".into()));
        println!("    polling       {} Hz", p.report_rate_hz());
        let stages: Vec<String> = p
            .dpi_stages()
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let tag = if i as u8 == p.default_stage() { "*" } else { "" };
                if v == 0 { format!("-{tag}") } else { format!("{v}{tag}") }
            })
            .collect();
        println!("    DPI stages    {}   (* default; shift stage {})", stages.join(" / "), p.switched_stage());
        for (i, b) in p.buttons().iter().enumerate() {
            println!("    button {i:<2}     {}", b.label());
        }
        let [l0, l1] = p.leds();
        println!("    LED logo      {}", l0.label());
        println!("    LED side      {}", l1.label());
    }
    Ok(())
}

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

fn backup(dev: &mut Dev, label: &str, out: &Path) -> Result<()> {
    let raw = onboard_profiles::info_raw(dev)?;
    let d = read_descriptor(dev)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let slug: String = label.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let dir = out.join(format!("{now}-{}", slug.trim_matches('-')));
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let mut addresses: Vec<u16> = (0..d.sector_count as u16).collect();
    addresses.extend((1..=d.rom_profile_count as u16).map(|n| 0x0100 | n));
    let mut records = Vec::new();
    for addr in addresses {
        match onboard_profiles::read_sector(dev, addr, d.sector_size) {
            Ok(bytes) => {
                let s = Sector::new(addr, bytes);
                let file = format!("sector-{addr:04x}.bin");
                std::fs::write(dir.join(&file), &s.bytes)?;
                records.push(SectorRecord {
                    address: format!("{addr:#06x}"),
                    file: Some(file),
                    crc_valid: (!s.is_rom() && !s.is_erased() && !s.is_zeroed()).then(|| s.crc_valid()),
                    erased: s.is_erased(),
                    zeroed: s.is_zeroed(),
                    error: None,
                });
            }
            Err(e) => records.push(SectorRecord {
                address: format!("{addr:#06x}"),
                file: None,
                crc_valid: None,
                erased: false,
                zeroed: false,
                error: Some(e.to_string()),
            }),
        }
    }
    let (maj, min) = dev.protocol_version();
    let manifest = Manifest {
        tool: format!("g502ctl {}", env!("CARGO_PKG_VERSION")),
        created_unix: now,
        device: label.to_string(),
        protocol: format!("{maj}.{min}"),
        descriptor_hex: raw.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "),
        active_profile: onboard_profiles::active_profile(dev).ok(),
        sectors: records,
    };
    std::fs::write(dir.join("manifest.json"), serde_json::to_string_pretty(&manifest)?)?;
    let ok = manifest.sectors.iter().filter(|s| s.file.is_some()).count();
    println!("  saved {ok}/{} sectors to {}", manifest.sectors.len(), dir.display());
    for s in manifest.sectors.iter().filter(|s| s.error.is_some()) {
        println!("    {}: {}", s.address, s.error.as_deref().unwrap_or(""));
    }
    Ok(())
}
