//! `hidractl report`: collect a shareable, replayable device report.
//!
//! The report is one JSON file holding the discovery list, a decoded summary
//! per device, and the raw HID++ traffic in ratbagd log format, so it can be
//! dropped into `captures/` and replayed by the test suite.
//!
//! Read-only like everything else. Privacy: the unit ID from feature 0x0003
//! is zeroed at record time, and macro sectors are skipped unless
//! `--include-macros` is passed (macros can contain typed text).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use lg_hidpp::discovery;
use lg_hidpp::features::{
    adjustable_dpi, battery_voltage, device_info, device_name, onboard_profiles, page_name, report_rate,
};
use lg_hidpp::record::{Recorder, RecordingTransport};
use lg_hidpp::{DEVICE_INDEX_DIRECT, Device, HidTransport, Transport};
use lg_profiles::{Descriptor, Directory, Sector};
use serde::Serialize;

pub const FORMAT: &str = "hidra-report/1";

#[derive(Serialize)]
struct Report {
    format: &'static str,
    tool: String,
    created_unix: u64,
    os: &'static str,
    kernel: Option<String>,
    privacy: Privacy,
    nodes: Vec<NodeRecord>,
    devices: Vec<DeviceRecord>,
    /// Raw HID++ traffic for all devices, in `ratbagd --verbose=raw` format.
    log: String,
}

#[derive(Serialize)]
struct Privacy {
    unit_id_redacted: bool,
    macro_sectors_included: bool,
    note: &'static str,
}

#[derive(Serialize)]
struct NodeRecord {
    hidraw: String,
    vid: String,
    pid: String,
    bus: String,
    name: String,
    kind: String,
    via_receiver: Option<String>,
    short_reports: bool,
    long_reports: bool,
}

#[derive(Serialize, Default)]
struct DeviceRecord {
    hidraw: String,
    pid: String,
    hid_name: String,
    error: Option<String>,
    protocol: Option<String>,
    name: Option<String>,
    features: Vec<FeatureRecord>,
    firmware: Vec<String>,
    sensor_dpi: Option<String>,
    report_rate: Option<String>,
    battery_mv: Option<u16>,
    onboard_descriptor_hex: Option<String>,
    onboard_layout_supported: Option<bool>,
    onboard_mode: Option<u8>,
    active_profile: Option<String>,
    user_directory_valid: Option<bool>,
    sectors_read: Vec<String>,
    sector_errors: Vec<String>,
}

#[derive(Serialize)]
struct FeatureRecord {
    index: String,
    page: String,
    version: u8,
    flags: String,
    name: &'static str,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// Collect a report for every Logitech HID++ device and write it under `out`.
pub fn run(out: &Path, include_macros: bool) -> Result<PathBuf> {
    let nodes = discovery::enumerate()?;
    let targets = discovery::devices()?;
    let recorder = Recorder::new_shared();
    let mut devices = Vec::new();

    for n in &targets {
        let path = match n.device_index {
            DEVICE_INDEX_DIRECT => n.dev_path.display().to_string(),
            slot => format!("{}#{slot}", n.dev_path.display()),
        };
        eprintln!("reading {} ({path}) ...", n.model_name());
        recorder.borrow_mut().begin_device(&n.name, &path);
        let mut rec = DeviceRecord {
            hidraw: path.clone(),
            pid: format!("{:04x}", n.product),
            hid_name: n.name.clone(),
            ..Default::default()
        };
        match HidTransport::open(&n.dev_path) {
            Err(e) => rec.error = Some(format!("open: {e}")),
            Ok(t) => {
                let transport = RecordingTransport::new(t, recorder.clone());
                match Device::open_with_timeout(transport, n.device_index, Duration::from_millis(1500)) {
                    Err(e) => rec.error = Some(format!("HID++ 2.0 open: {e}")),
                    Ok(mut dev) => {
                        if let Some(f) = dev.feature(device_info::PAGE) {
                            // GetDeviceInfo payload bytes 1..5 are the unit ID.
                            recorder.borrow_mut().redact(f.index, 0, 1..5);
                        }
                        collect(&mut dev, &mut rec, include_macros);
                    }
                }
            }
        }
        if let Some(e) = &rec.error {
            recorder.borrow_mut().note(e);
        }
        devices.push(rec);
    }

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let report = Report {
        format: FORMAT,
        tool: format!("hidractl {}", env!("CARGO_PKG_VERSION")),
        created_unix: now,
        os: std::env::consts::OS,
        kernel: std::fs::read_to_string("/proc/sys/kernel/osrelease").ok().map(|s| s.trim().to_string()),
        privacy: Privacy {
            unit_id_redacted: true,
            macro_sectors_included: include_macros,
            note: "Profile names you typed yourself are included as stored on the device.",
        },
        nodes: nodes
            .iter()
            .map(|n| NodeRecord {
                hidraw: n.dev_path.display().to_string(),
                vid: format!("{:04x}", n.vendor),
                pid: format!("{:04x}", n.product),
                bus: format!("{:04x}", n.bus),
                name: n.name.clone(),
                kind: format!("{:?}", n.kind),
                via_receiver: n.via_receiver.map(|p| format!("{p:04x}")),
                short_reports: n.short_reports,
                long_reports: n.long_reports,
            })
            .collect(),
        devices,
        log: recorder.borrow().text(),
    };

    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let pids: Vec<&str> = report.devices.iter().map(|d| d.pid.as_str()).collect();
    let file = out.join(format!("hidra-report-{}-{now}.json", if pids.is_empty() { "none".into() } else { pids.join("-") }));
    std::fs::write(&file, serde_json::to_string_pretty(&report)?)?;
    Ok(file)
}

fn collect<T: Transport>(dev: &mut Device<T>, rec: &mut DeviceRecord, include_macros: bool) {
    let (maj, min) = dev.protocol_version();
    rec.protocol = Some(format!("{maj}.{min}"));
    rec.features = dev
        .features()
        .iter()
        .map(|f| FeatureRecord {
            index: format!("{:#04x}", f.index),
            page: format!("{:#06x}", f.page),
            version: f.version,
            flags: format!("{:#04x}", f.flags.0),
            name: page_name(f.page),
        })
        .collect();
    rec.name = device_name::name(dev).ok();
    if let Ok(info) = device_info::device_info(dev) {
        for e in 0..info.entity_count {
            if let Ok(f) = device_info::firmware_info(dev, e) {
                rec.firmware.push(format!("{} {}", f.kind_name(), f.version_string()));
            }
        }
    }
    if let Ok(count) = adjustable_dpi::sensor_count(dev) {
        for s in 0..count {
            let list = adjustable_dpi::dpi_list(dev, s).ok();
            let cur = adjustable_dpi::sensor_dpi(dev, s).ok();
            rec.sensor_dpi = Some(format!("sensor {s}: current {:?}, list {:?}", cur.map(|c| c.current), list));
        }
    }
    let rates = report_rate::supported_rates_hz(dev).ok();
    let cur = report_rate::current_rate_hz(dev).ok();
    if rates.is_some() || cur.is_some() {
        rec.report_rate = Some(format!("current {cur:?} Hz, supported {rates:?}"));
    }
    rec.battery_mv = battery_voltage::voltage(dev).ok().map(|b| b.millivolts);

    let Ok(raw) = onboard_profiles::info_raw(dev) else { return };
    rec.onboard_descriptor_hex = Some(hex(&raw));
    let Some(d) = Descriptor::parse(&raw) else { return };
    rec.onboard_layout_supported = Some(d.layout_supported());
    rec.onboard_mode = onboard_profiles::mode(dev).ok();
    rec.active_profile = onboard_profiles::active_profile(dev).ok().map(|a| format!("{a:#06x}"));
    let _ = onboard_profiles::active_resolution_index(dev);

    let mut addresses: Vec<u16> = vec![0];
    match onboard_profiles::read_sector(dev, 0, d.sector_size) {
        Ok(bytes) => {
            let dir = Directory::parse(&Sector::new(0, bytes));
            rec.user_directory_valid = Some(dir.is_some());
            if let Some(dir) = dir {
                addresses.extend(dir.entries.iter().map(|e| e.sector));
            }
            rec.sectors_read.push("0x0000".into());
        }
        Err(e) => rec.sector_errors.push(format!("0x0000: {e}")),
    }
    addresses.extend((1..=d.rom_profile_count as u16).map(|n| 0x0100 | n));
    if include_macros {
        addresses.extend(1..d.sector_count as u16);
    }
    addresses.sort_unstable();
    addresses.dedup();
    for addr in addresses.into_iter().filter(|&a| a != 0) {
        match onboard_profiles::read_sector(dev, addr, d.sector_size) {
            Ok(_) => rec.sectors_read.push(format!("{addr:#06x}")),
            Err(e) => rec.sector_errors.push(format!("{addr:#06x}: {e}")),
        }
    }
}
