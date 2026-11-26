//! First-time profile setup against a simulated mouse seeded with the G502
//! LIGHTSPEED's recorded memory: blank user sectors, one factory profile,
//! 255-byte sectors.

use std::collections::BTreeMap;
use std::path::PathBuf;

use lg_profiles::ProfileEdit;
use lg_snapshot::{
    DeviceEntry, DeviceSource, EditError, commit_initialize, edit_entry, open_entry,
    plan_initialize, read_entry, restore_from_backup, simulated_entry,
};

fn lightspeed() -> DeviceEntry {
    let log = PathBuf::from(format!(
        "{}/../../captures/ratbagd/both-mice-lightspeed-407f.log",
        env!("CARGO_MANIFEST_DIR")
    ));
    simulated_entry(&log, "Logitech G502").expect("simulated LIGHTSPEED")
}

fn backups(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hidra-init-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn memory(entry: &DeviceEntry) -> (BTreeMap<u16, Vec<u8>>, u16) {
    match &entry.source {
        DeviceSource::Sim(m) => {
            let m = m.lock().unwrap();
            (m.sectors.clone(), m.active_profile)
        }
        _ => unreachable!(),
    }
}

#[test]
fn setup_copies_the_factory_profile_then_edits_and_restore_work() {
    let entry = lightspeed();
    let (blank, active) = memory(&entry);
    assert_eq!(active, 0x0101);
    assert!(blank[&0].iter().all(|&b| b == 0xff) && blank[&0].len() == 255);

    let dir = backups("happy");
    let mut dev = open_entry(&entry).unwrap();
    let plan = plan_initialize(&mut dev).unwrap();
    assert_eq!(plan.source, 0x0101);
    assert_eq!(plan.profile().dpi_stages(), [400, 800, 1600, 3200, 6400]);
    // Directory: five slots, the first enabled, libratbag's terminator.
    assert_eq!(
        &plan.directory[..24],
        &[
            0, 1, 1, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 5, 0, 0, 0xff, 0xff, 0, 0
        ]
    );
    let outcome = commit_initialize(&mut dev, &plan, "sim", &dir).unwrap();
    assert_eq!(
        (outcome.active_before, outcome.active_after),
        (0x0101, 0x0001)
    );
    drop(dev);

    // The user profile is the factory profile plus a CRC; nothing else moved.
    let (after, active) = memory(&entry);
    assert_eq!(active, 0x0001);
    assert_eq!(after[&1][..253], blank[&0x0101][..253]);
    for (address, bytes) in &blank {
        if ![0u16, 1].contains(address) {
            assert_eq!(&after[address], bytes, "sector {address:#06x} untouched");
        }
    }

    // It now reads as an ordinary device with one user profile, and edits work.
    let s = read_entry(&entry).unwrap();
    let set = s.onboard.unwrap().unwrap().profiles.unwrap();
    assert!(!set.factory);
    let enabled: Vec<_> = set.profiles.iter().filter(|p| p.enabled).collect();
    assert_eq!(enabled.len(), 1);
    assert!(enabled[0].active);
    let edited = edit_entry(
        &entry,
        1,
        &[ProfileEdit::SetDpi {
            stage: 1,
            dpi: 1000,
        }],
        &dir,
    )
    .unwrap();
    let c = edited.onboard.unwrap().unwrap().profiles.unwrap().profiles[0]
        .content
        .clone()
        .unwrap();
    assert_eq!(c.dpi_stages, [400, 1000, 1600, 3200, 6400]);

    // Setting it up twice is refused.
    let mut dev = open_entry(&entry).unwrap();
    assert!(matches!(
        plan_initialize(&mut dev),
        Err(EditError::Invalid(_))
    ));

    // Restoring the pre-setup backup returns the memory to blank and the
    // mouse to its factory profile.
    let rewritten = restore_from_backup(&mut dev, &outcome.backup).unwrap();
    assert_eq!(rewritten, 2);
    let (restored, active) = memory(&entry);
    assert_eq!(restored, blank);
    assert_eq!(active, 0x0101);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_directory_write_leaves_the_memory_blank() {
    let entry = lightspeed();
    let (blank, _) = memory(&entry);
    if let DeviceSource::Sim(m) = &entry.source {
        // 16 WriteData calls for the profile, then fail inside the directory write.
        m.lock().unwrap().faults.fail_write_data_at = Some(16 + 4);
    }
    let dir = backups("fail");
    let mut dev = open_entry(&entry).unwrap();
    let plan = plan_initialize(&mut dev).unwrap();
    assert!(matches!(
        commit_initialize(&mut dev, &plan, "sim", &dir),
        Err(EditError::Write(_))
    ));
    let (after, active) = memory(&entry);
    assert_eq!(after, blank);
    assert_eq!(active, 0x0101);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_device_with_user_profiles_is_not_set_up_again() {
    let log = PathBuf::from(format!(
        "{}/../../captures/ratbagd/g502-proteus-spectrum-c332.log",
        env!("CARGO_MANIFEST_DIR")
    ));
    let entry = simulated_entry(&log, "Logitech Gaming Mouse G502").unwrap();
    let mut dev = open_entry(&entry).unwrap();
    match plan_initialize(&mut dev) {
        Err(EditError::Invalid(m)) => assert!(m.contains("already has user profiles")),
        other => panic!("unexpected {other:?}"),
    }
}
