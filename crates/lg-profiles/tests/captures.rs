//! Replay recorded ratbagd traffic through the real protocol stack and check
//! the decoded profiles against what was observed on the hardware.

use lg_hidpp::features::{adjustable_dpi, onboard_profiles, report_rate};
use lg_hidpp::replay::{ReplayTransport, parse_log};
use lg_hidpp::{DEVICE_INDEX_DIRECT, Device};
use lg_profiles::{
    ButtonBinding, Descriptor, Directory, DpiLimits, Profile, ProfileEdit, Sector, diff_offsets,
};
use std::time::Duration;

fn capture(name: &str) -> String {
    let path = format!(
        "{}/../../captures/ratbagd/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn open(log: &str, segment_name: &str) -> Device<ReplayTransport> {
    let seg = parse_log(&capture(log))
        .into_iter()
        .find(|s| s.name == segment_name)
        .unwrap_or_else(|| panic!("no segment named {segment_name}"));
    Device::open_with_timeout(
        ReplayTransport::new(seg),
        DEVICE_INDEX_DIRECT,
        Duration::from_millis(100),
    )
    .expect("open")
}

#[test]
fn proteus_spectrum_full_decode() {
    let mut dev = open(
        "g502-proteus-spectrum-c332.log",
        "Logitech Gaming Mouse G502",
    );
    assert_eq!(dev.protocol_version(), (4, 2));
    assert_eq!(dev.features().len(), 20, "root + 19 from IFeatureSet");
    assert_eq!(dev.feature(0x8100).unwrap().index, 0x0e);
    assert_eq!(dev.feature(0x2201).unwrap().index, 0x0a);

    assert_eq!(
        adjustable_dpi::dpi_list(&mut dev, 0).unwrap(),
        adjustable_dpi::DpiList::Range {
            min: 50,
            max: 12000,
            step: 50
        }
    );
    assert_eq!(
        adjustable_dpi::sensor_dpi(&mut dev, 0).unwrap().current,
        12000
    );
    assert_eq!(
        report_rate::supported_rates_hz(&mut dev).unwrap(),
        vec![125, 250, 500, 1000]
    );

    let desc = Descriptor::parse(&onboard_profiles::info_raw(&mut dev).unwrap()).unwrap();
    assert_eq!(
        (desc.profile_count, desc.sector_size, desc.button_count),
        (3, 256, 11)
    );

    let active = onboard_profiles::active_profile(&mut dev).unwrap();
    assert_eq!(active, 0x0001);

    let dir_sector = Sector::new(
        0,
        onboard_profiles::read_sector(&mut dev, 0, desc.sector_size).unwrap(),
    );
    assert_eq!(dir_sector.stored_crc(), Some(0xda84));
    assert!(dir_sector.crc_valid());
    let dir = Directory::parse(&dir_sector).unwrap();
    assert_eq!(
        dir.entries.iter().map(|e| e.sector).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(dir.entries.iter().all(|e| e.enabled));
    // Active address 1 is sector 1, which is logical profile 0 (the INDEX_OFFSET issue).
    assert_eq!(dir.profile_for_address(active), Some(0));

    let expected: [([u16; 5], u16, u16); 3] = [
        ([3350, 5150, 8500, 12000, 0], 1000, 0x1c32),
        ([400, 800, 1600, 2400, 0], 1000, 0x59d7),
        ([2400, 0, 0, 0, 0], 500, 0x78f5),
    ];
    for (entry, (dpi, rate, crc)) in dir.entries.iter().zip(expected) {
        let bytes =
            onboard_profiles::read_sector(&mut dev, entry.sector, desc.sector_size).unwrap();
        let sector = Sector::new(entry.sector, bytes);
        assert!(sector.crc_valid(), "sector {} CRC", entry.sector);
        assert_eq!(sector.stored_crc(), Some(crc));
        let p = Profile::decode(sector, desc.button_count).unwrap();
        assert_eq!(p.dpi_stages(), dpi, "profile {}", entry.profile);
        assert_eq!(p.report_rate_hz(), rate, "profile {}", entry.profile);
    }

    let p0 = Profile::decode(
        Sector::new(
            1,
            onboard_profiles::read_sector(&mut dev, 1, desc.sector_size).unwrap(),
        ),
        desc.button_count,
    )
    .unwrap();
    assert_eq!(p0.name().as_deref(), Some("Default"));
    assert_eq!(
        p0.buttons()[5],
        ButtonBinding::Key {
            modifiers: 0,
            usage: 0x73
        },
        "F24 on button 5"
    );
}

#[test]
fn proteus_dpi_edit_touches_only_its_field_and_crc() {
    let mut dev = open(
        "g502-proteus-spectrum-c332.log",
        "Logitech Gaming Mouse G502",
    );
    let original = Profile::decode(
        Sector::new(1, onboard_profiles::read_sector(&mut dev, 1, 256).unwrap()),
        11,
    )
    .unwrap();
    let limits = Some(DpiLimits {
        min: 50,
        max: 12000,
        step: 50,
    });

    // Re-applying the existing value must reproduce the captured bytes exactly,
    // including the device-computed CRC.
    let same = original
        .apply(
            ProfileEdit::SetDpi {
                stage: 0,
                dpi: 3350,
            },
            limits,
        )
        .unwrap();
    assert_eq!(same.bytes(), original.bytes());

    let edited = original
        .apply(
            ProfileEdit::SetDpi {
                stage: 0,
                dpi: 3200,
            },
            limits,
        )
        .unwrap();
    assert_eq!(edited.dpi_stages(), [3200, 5150, 8500, 12000, 0]);
    assert!(edited.sector().crc_valid());
    let changed = diff_offsets(original.bytes(), edited.bytes());
    assert!(
        changed.iter().all(|&o| o == 3 || o == 4 || o >= 254),
        "changed offsets {changed:?}"
    );
    assert_eq!(edited.buttons(), original.buttons(), "button map preserved");

    assert!(
        original
            .apply(
                ProfileEdit::SetDpi {
                    stage: 0,
                    dpi: 3225
                },
                limits
            )
            .is_err()
    );
    assert!(
        original
            .apply(
                ProfileEdit::SetDpi {
                    stage: 0,
                    dpi: 13000
                },
                limits
            )
            .is_err()
    );
    assert!(
        original
            .apply(ProfileEdit::SetDpi { stage: 5, dpi: 800 }, limits)
            .is_err()
    );
}

#[test]
fn lightspeed_blank_directory_and_rom_profile() {
    let mut dev = open("both-mice-lightspeed-407f.log", "Logitech G502");
    assert_eq!(dev.feature(0x8100).unwrap().index, 0x09);
    let desc = Descriptor::parse(&onboard_profiles::info_raw(&mut dev).unwrap()).unwrap();
    assert_eq!(
        (desc.profile_format, desc.profile_count, desc.sector_size),
        (3, 5, 255)
    );
    assert_eq!(onboard_profiles::active_profile(&mut dev).unwrap(), 0x0101);

    let dir = Sector::new(
        0,
        onboard_profiles::read_sector(&mut dev, 0, desc.sector_size).unwrap(),
    );
    assert!(dir.is_erased());
    assert!(
        Directory::parse(&dir).is_none(),
        "blank directory means ROM fallback"
    );

    let rom = Sector::new(
        0x0101,
        onboard_profiles::read_sector(&mut dev, 0x0101, desc.sector_size).unwrap(),
    );
    assert!(rom.is_rom());
    let p = Profile::decode(rom, desc.button_count).unwrap();
    assert_eq!(p.dpi_stages(), [400, 800, 1600, 3200, 6400]);
    assert_eq!(p.default_stage(), 1);
    assert_eq!(p.report_rate_hz(), 1000);
    assert_eq!(p.name(), None);
}

/// A recording of a session replays to the same data, and configured
/// redactions are applied before anything is logged.
#[test]
fn recorded_session_replays_and_redacts() {
    use lg_hidpp::record::{Recorder, RecordingTransport};

    let seg = parse_log(&capture("g502-proteus-spectrum-c332.log"))
        .into_iter()
        .find(|s| s.name == "Logitech Gaming Mouse G502")
        .unwrap();
    let recorder = Recorder::new_shared();
    recorder
        .borrow_mut()
        .begin_device("Recorded mouse", "/dev/hidrawN");
    let transport = RecordingTransport::new(ReplayTransport::new(seg), recorder.clone());
    let mut dev =
        Device::open_with_timeout(transport, DEVICE_INDEX_DIRECT, Duration::from_millis(100))
            .unwrap();
    let idx = dev.feature(0x8100).unwrap().index;
    // Redact the first two payload bytes of every ReadData reply (function 5).
    recorder.borrow_mut().redact(idx, 5, 0..2);
    let original = onboard_profiles::read_sector(&mut dev, 1, 256).unwrap();
    assert_eq!(
        &original[..2],
        &[0x01, 0x03],
        "the live read is not altered by redaction"
    );

    let text = recorder.borrow().text();
    let seg = parse_log(&text)
        .into_iter()
        .find(|s| s.name == "Recorded mouse")
        .expect("recording parses back");
    assert_eq!(seg.hidraw.as_deref(), Some("/dev/hidrawN"));
    let mut replayed = Device::open_with_timeout(
        ReplayTransport::new(seg),
        DEVICE_INDEX_DIRECT,
        Duration::from_millis(100),
    )
    .unwrap();
    assert_eq!(replayed.features().len(), 20);
    let again = onboard_profiles::read_sector(&mut replayed, 1, 256).unwrap();
    for block in (0..256).step_by(16) {
        assert_eq!(
            &again[block..block + 2],
            &[0, 0],
            "redacted bytes at {block:#x}"
        );
        assert_eq!(
            &again[block + 2..block + 16],
            &original[block + 2..block + 16]
        );
    }
}
