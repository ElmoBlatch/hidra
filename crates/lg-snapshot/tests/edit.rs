//! The guarded edit path, end to end, against a simulated mouse seeded with
//! the G502 Proteus Spectrum's recorded onboard memory.

use std::path::PathBuf;

use lg_profiles::{ButtonBinding, ProfileEdit};
use lg_snapshot::{DeviceEntry, DeviceSource, EditError, ProfileContent, edit_entry, open_entry, plan_edit, read_entry, simulated_entry};

fn proteus() -> DeviceEntry {
    let log = PathBuf::from(format!("{}/../../captures/ratbagd/g502-proteus-spectrum-c332.log", env!("CARGO_MANIFEST_DIR")));
    simulated_entry(&log, "Logitech Gaming Mouse G502").expect("simulated device from the capture")
}

fn backups() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hidra-test-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn profile(entry: &DeviceEntry, address: u16) -> ProfileContent {
    let s = read_entry(entry).unwrap();
    let set = s.onboard.unwrap().unwrap().profiles.unwrap();
    set.profiles.into_iter().find(|p| p.address == address).unwrap().content.unwrap()
}

fn sectors(entry: &DeviceEntry) -> std::collections::BTreeMap<u16, Vec<u8>> {
    match &entry.source {
        DeviceSource::Sim(m) => m.lock().unwrap().sectors.clone(),
        _ => unreachable!(),
    }
}

#[test]
fn simulated_device_reads_like_the_capture() {
    let entry = proteus();
    assert!(entry.write_verified());
    let p = profile(&entry, 1);
    assert_eq!(p.dpi_stages, [3350, 5150, 8500, 12000, 0]);
    assert_eq!(p.buttons[5], "key F24");
}

#[test]
fn a_combined_edit_changes_only_its_fields_and_persists() {
    let entry = proteus();
    let before = sectors(&entry);
    let edits = [
        ProfileEdit::SetDpi { stage: 0, dpi: 3200 },
        ProfileEdit::SetDefaultStage(2),
        ProfileEdit::SetReportRate(500),
        ProfileEdit::SetButton { index: 8, binding: ButtonBinding::parse("key:F13").unwrap() },
    ];
    let snapshot = edit_entry(&entry, 1, &edits, &backups()).expect("edit succeeds");

    // The snapshot returned after the write already shows the new values.
    let set = snapshot.onboard.unwrap().unwrap().profiles.unwrap();
    let p = set.profiles.iter().find(|p| p.address == 1).unwrap().content.clone().unwrap();
    assert_eq!(p.dpi_stages, [3200, 5150, 8500, 12000, 0]);
    assert_eq!(p.default_stage, 2);
    assert_eq!(p.report_rate_hz, 500);
    assert_eq!(p.buttons[8], "key F13");
    assert_eq!(p.buttons[5], "key F24", "untouched binding survives");

    // Byte level: only the edited fields and the CRC moved, in sector 1 only.
    let after = sectors(&entry);
    let changed: Vec<usize> = (0..256).filter(|&i| before[&1][i] != after[&1][i]).collect();
    assert_eq!(changed, [0x00, 0x01, 0x03, 0x04, 0x40, 0x41, 0x42, 0x43, 0xfe, 0xff]);
    for s in [0u16, 2, 3] {
        assert_eq!(before[&s], after[&s], "sector {s} untouched");
    }

    // A fresh connection sees the same thing.
    assert_eq!(profile(&entry, 1).report_rate_hz, 500);
}

#[test]
fn invalid_edits_are_refused_before_anything_is_written() {
    let entry = proteus();
    let before = sectors(&entry);
    let dir = backups();
    let refuse = |edits: &[ProfileEdit]| match edit_entry(&entry, 1, edits, &dir) {
        Err(EditError::Invalid(msg)) => msg,
        other => panic!("expected Invalid, got {other:?}"),
    };
    assert!(refuse(&[ProfileEdit::SetDpi { stage: 0, dpi: 3225 }]).contains("step"));
    assert!(refuse(&[ProfileEdit::SetDpi { stage: 0, dpi: 20000 }]).contains("range"));
    assert!(refuse(&[ProfileEdit::SetDpi { stage: 3, dpi: 0 }]).contains("default DPI stage"));
    assert!(refuse(&[ProfileEdit::SetDefaultStage(4)]).contains("default DPI stage"), "stage 5 is unused");
    assert!(refuse(&[ProfileEdit::SetReportRate(333)]).contains("not supported"));
    assert!(refuse(&[ProfileEdit::SetButton { index: 11, binding: ButtonBinding::Disabled }]).contains("does not exist"));
    // Sector 7 is not a profile in the directory.
    assert!(matches!(edit_entry(&entry, 7, &[ProfileEdit::SetReportRate(500)], &dir), Err(EditError::Invalid(_))));
    assert_eq!(sectors(&entry), before);
    match &entry.source {
        DeviceSource::Sim(m) => assert_eq!(m.lock().unwrap().begin_writes, 0),
        _ => unreachable!(),
    }
}

#[test]
fn re_assigning_the_same_action_is_a_noop_that_keeps_the_stored_bytes() {
    let entry = proteus();
    let mut dev = open_entry(&entry).unwrap();
    // The device stores DPI down as `90 04 ff 00`; our encoding would be `90 04 00 00`.
    let plan = plan_edit(&mut dev, 1, &[ProfileEdit::SetButton { index: 6, binding: ButtonBinding::parse("dpi-down").unwrap() }])
        .unwrap();
    assert!(plan.is_noop());
}

#[test]
fn a_failed_write_is_rolled_back_and_reported() {
    let entry = proteus();
    let before = sectors(&entry);
    if let DeviceSource::Sim(m) = &entry.source {
        m.lock().unwrap().faults.fail_write_data_at = Some(3);
    }
    let err = edit_entry(&entry, 1, &[ProfileEdit::SetDpi { stage: 0, dpi: 3200 }], &backups()).unwrap_err();
    match err {
        EditError::Write(msg) => assert!(msg.contains("restored and verified"), "{msg}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(sectors(&entry), before, "the profile is back to its original bytes");
}

#[test]
fn replayed_captures_cannot_be_edited() {
    let log = PathBuf::from(format!("{}/../../captures/ratbagd/g502-proteus-spectrum-c332.log", env!("CARGO_MANIFEST_DIR")));
    let entry = lg_snapshot::replay_entries(&log, Some("Logitech Gaming Mouse G502")).unwrap().remove(0);
    assert!(!entry.write_verified());
    assert_eq!(edit_entry(&entry, 1, &[ProfileEdit::SetReportRate(500)], &backups()), Err(EditError::NotWriteVerified));
}
