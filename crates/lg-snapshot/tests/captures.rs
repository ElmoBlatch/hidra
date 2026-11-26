//! Snapshot the two recorded mice through the replay transport.

use std::path::PathBuf;

use lg_snapshot::{
    Connection, DeviceEntry, DpiRange, Integrity, ReadError, read_entry, replay_entries,
};

fn capture(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/../../captures/ratbagd/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

fn entry(log: &str, segment: &str) -> DeviceEntry {
    let mut entries = replay_entries(&capture(log), Some(segment)).expect("replay entries");
    assert_eq!(entries.len(), 1);
    entries.remove(0)
}

#[test]
fn proteus_spectrum_snapshot() {
    let e = entry(
        "g502-proteus-spectrum-c332.log",
        "Logitech Gaming Mouse G502",
    );
    assert_eq!(e.connection, Connection::Replay);
    assert_eq!(e.id, "replay:Logitech Gaming Mouse G502");
    let s = read_entry(&e).expect("read");
    assert_eq!(s.protocol, (4, 2));
    assert_eq!(
        s.dpi_range,
        Some(Ok(DpiRange::Range {
            min: 50,
            max: 12000,
            step: 50
        }))
    );
    assert_eq!(s.sensor_dpi.clone().unwrap().unwrap().current, 12000);
    assert_eq!(s.polling_rates_hz, Some(Ok(vec![125, 250, 500, 1000])));
    assert_eq!(
        s.battery_mv, None,
        "wired Proteus has no battery feature, so it is hidden"
    );

    let onboard = s.onboard.expect("onboard feature").expect("onboard read");
    assert_eq!(
        (
            onboard.profile_format,
            onboard.profile_count,
            onboard.button_count
        ),
        (2, 3, 11)
    );
    assert_eq!(onboard.active_address, Some(0x0001));
    let set = onboard.profiles.expect("profiles");
    assert!(!set.factory);
    assert_eq!(
        set.profiles.iter().map(|p| p.address).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(
        set.profiles
            .iter()
            .map(|p| p.title.as_str())
            .collect::<Vec<_>>(),
        ["Profile 1", "Profile 2", "Profile 3"]
    );
    let active = set.active().expect("one active profile");
    assert_eq!(active.address, 1);
    assert_eq!(set.profiles.iter().filter(|p| p.active).count(), 1);

    let c = active.content.as_ref().expect("decoded");
    assert_eq!(c.name.as_deref(), Some("Default"));
    assert_eq!(c.integrity, Integrity::CrcOk(0x1c32));
    assert_eq!(c.dpi_stages, [3350, 5150, 8500, 12000, 0]);
    assert_eq!(c.report_rate_hz, 1000);
    assert_eq!(c.buttons.len(), 11);
    assert_eq!(c.buttons[5], "key F24");
    assert_eq!(
        c.leds.iter().map(|l| l.zone.as_str()).collect::<Vec<_>>(),
        ["Logo", "Side"]
    );

    let p3 = set.profiles[2].content.as_ref().unwrap();
    assert_eq!(p3.report_rate_hz, 500);
    assert_eq!(p3.dpi_stages, [2400, 0, 0, 0, 0]);
}

#[test]
fn lightspeed_snapshot_uses_factory_profile() {
    let e = entry("both-mice-lightspeed-407f.log", "Logitech G502");
    let s = read_entry(&e).expect("read");
    let onboard = s.onboard.expect("onboard feature").expect("onboard read");
    assert_eq!(onboard.profile_format, 3);
    assert_eq!(onboard.active_address, Some(0x0101));
    let set = onboard.profiles.expect("profiles");
    assert!(set.factory, "blank directory means factory ROM profiles");
    assert_eq!(set.profiles[0].title, "Factory profile 1");
    let active = set.active().expect("ROM profile 1 is active");
    assert_eq!(active.address, 0x0101);
    assert!(active.factory);
    let c = active.content.as_ref().expect("decoded");
    assert_eq!(c.integrity, Integrity::Rom);
    assert_eq!(c.dpi_stages, [400, 800, 1600, 3200, 6400]);
    assert_eq!(c.default_stage, 1);
    assert_eq!(c.name, None);
}

#[test]
fn unknown_segment_is_an_error() {
    let err = replay_entries(
        &capture("both-mice-lightspeed-407f.log"),
        Some("No such mouse"),
    )
    .unwrap_err();
    assert!(matches!(err, ReadError::Other(_)));
}

#[test]
fn missing_capture_is_an_error() {
    assert!(replay_entries(&capture("does-not-exist.log"), None).is_err());
}
