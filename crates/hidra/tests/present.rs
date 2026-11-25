//! Display strings for the replayed captures.

use std::path::PathBuf;

use hidra::present::{
    button_rows, device_title, headline_stats, is_g502_layout, profile_meta, profile_storage, stage_rows, summary_facts,
};
use lg_snapshot::{DeviceEntry, DeviceSnapshot, read_entry, replay_entries};

fn snapshot(log: &str, segment: &str) -> (DeviceEntry, DeviceSnapshot) {
    let path = PathBuf::from(format!("{}/../../captures/ratbagd/{log}", env!("CARGO_MANIFEST_DIR")));
    let entry = replay_entries(&path, Some(segment)).unwrap().remove(0);
    let s = read_entry(&entry).unwrap();
    (entry, s)
}

fn fact<'a>(facts: &'a [hidra::present::Fact], label: &str) -> Option<&'a str> {
    facts.iter().find(|f| f.label == label).map(|f| f.value.as_str())
}

#[test]
fn proteus_summary_hides_missing_battery() {
    let (entry, s) = snapshot("g502-proteus-spectrum-c332.log", "Logitech Gaming Mouse G502");
    let facts = summary_facts(&entry, &s);
    assert_eq!(fact(&facts, "Battery"), None, "no battery feature: hidden, not 'Unavailable'");
    assert_eq!(fact(&facts, "DPI range"), Some("50–12000, step 50"));
    assert_eq!(fact(&facts, "Sensor DPI"), Some("12000"), "default 0 is not shown");
    assert_eq!(fact(&facts, "Supported rates"), Some("125, 250, 500, 1000 Hz"));
    assert_eq!(fact(&facts, "Profile memory"), Some("Format 2, 3 user slots, 3 factory profiles, 11 buttons"));
    assert_eq!(fact(&facts, "HID++ protocol"), Some("4.2"));

    let set = s.onboard.as_ref().unwrap().as_ref().unwrap().profiles.as_ref().unwrap();
    let p = set.active().unwrap();
    let c = p.content.as_ref().unwrap();
    assert_eq!(profile_meta(p, c), "1,000 Hz polling · Stored on the mouse");
    assert_eq!(profile_storage(p, c), "Sector 0x0001, CRC 1c32 OK");
    let stages = stage_rows(c);
    assert_eq!(stages.len(), 5);
    assert_eq!(stages[4].dpi, None, "zero DPI is an unused stage");
    assert_eq!(stages.iter().filter(|r| r.is_default).count(), 1);
    // The sensor runs at stage 4's value; no button is bound to DPI shift
    // here (the sniper is F24), so no stage is marked as the shift stage.
    assert!(stages.iter().all(|r| !r.is_shift));

    // The capture's device name contains "G502", so it gets the G502 layout.
    assert!(is_g502_layout(&entry, c));
    let buttons = button_rows(c, true);
    assert_eq!(buttons.len(), 11);
    assert_eq!((buttons[0].key.as_deref(), buttons[0].name.as_str()), (Some("G1"), "Left click"));
    assert_eq!(buttons[0].action, "Left click");
    assert_eq!((buttons[5].key.as_deref(), buttons[5].action.as_str()), (Some("G6"), "F24"));
    // Only the sniper is remapped on this profile.
    let remapped: Vec<usize> = buttons.iter().enumerate().filter(|(_, b)| b.remapped).map(|(i, _)| i).collect();
    assert_eq!(remapped, [5]);
    assert_eq!(buttons[9].key, None, "wheel tilts carry no G number");

    // Unknown layouts fall back to numbered buttons and never claim a remap.
    let generic = button_rows(c, false);
    assert_eq!(generic[5].name, "Button 6");
    assert!(generic.iter().all(|b| !b.remapped && b.key.is_none()));

    let stats = headline_stats(&s);
    let labels: Vec<&str> = stats.iter().map(|s| s.label).collect();
    assert_eq!(labels, ["Sensor", "Polling", "Active profile"], "wired mouse: no battery");
    assert_eq!(stats[0].value, "12,000");
    assert_eq!(stats[2].value, "Profile 1");
}

#[test]
fn lightspeed_summary_and_factory_profile() {
    let (entry, s) = snapshot("both-mice-lightspeed-407f.log", "Logitech G502");
    assert!(!device_title(&entry, Some(&s)).is_empty());
    let set = s.onboard.as_ref().unwrap().as_ref().unwrap().profiles.as_ref().unwrap();
    let p = set.active().unwrap();
    let c = p.content.as_ref().unwrap();
    assert_eq!(profile_meta(p, c), "1,000 Hz polling · Built into the mouse");
    // Factory assignments, including battery level on G9, are not remaps.
    assert!(button_rows(c, true).iter().all(|b| !b.remapped));
    let stats = headline_stats(&s);
    let battery = stats.iter().find(|s| s.label == "Battery").expect("wireless mouse reports battery");
    assert_eq!((battery.value.as_str(), battery.unit.as_deref()), ("3.75", Some("V")));
    let stages = stage_rows(c);
    assert!(stages[1].is_default);
    assert!(stages[0].is_shift, "the sniper button drops to stage 1");
    assert_eq!(stages.iter().filter_map(|r| r.dpi).collect::<Vec<_>>(), [400, 800, 1600, 3200, 6400]);
}

#[test]
fn form_edits_only_reports_what_changed_and_validates() {
    use hidra::edit_form::{FormValues, action_choices, change_count_label, form_edits};
    use lg_profiles::{ButtonBinding, ProfileEdit};

    let (_, s) = snapshot("g502-proteus-spectrum-c332.log", "Logitech Gaming Mouse G502");
    let set = s.onboard.as_ref().unwrap().as_ref().unwrap().profiles.as_ref().unwrap();
    let c = set.active().unwrap().content.as_ref().unwrap();
    let range = s.dpi_range.as_ref().unwrap().as_ref().unwrap();

    let unchanged = FormValues::from_profile(c);
    assert_eq!(unchanged.dpi[4], "", "an unused stage shows as empty");
    assert_eq!(form_edits(c, &unchanged, Some(range)), Ok(vec![]));

    let mut form = unchanged.clone();
    form.dpi[0] = "3,200".into();
    form.polling_hz = 500;
    form.buttons[8] = ButtonBinding::parse("key:F13").unwrap();
    assert_eq!(
        form_edits(c, &form, Some(range)),
        Ok(vec![
            ProfileEdit::SetDpi { stage: 0, dpi: 3200 },
            ProfileEdit::SetReportRate(500),
            ProfileEdit::SetButton { index: 8, binding: ButtonBinding::parse("key:F13").unwrap() },
        ])
    );

    let invalid = |f: &dyn Fn(&mut FormValues)| {
        let mut form = unchanged.clone();
        f(&mut form);
        form_edits(c, &form, Some(range)).unwrap_err()
    };
    assert!(invalid(&|f| f.dpi[1] = "abc".into()).contains("Stage 2"));
    assert!(invalid(&|f| f.dpi[1] = "5125".into()).contains("multiple of 50"));
    assert!(invalid(&|f| f.dpi[1] = "20000".into()).contains("50 to 12,000"));
    assert!(invalid(&|f| f.dpi[3] = String::new()).contains("default stage"));
    assert!(invalid(&|f| f.default_stage = 4).contains("default stage"));

    // F24 is a standard action; Ctrl+C is not, so it is offered first as "keep".
    let (choices, current) = action_choices(c.bindings[5]);
    assert_eq!(choices[current].label, "F24");
    let ctrl_c = ButtonBinding::parse("key:ctrl+c").unwrap();
    let (choices, current) = action_choices(ctrl_c);
    assert_eq!((current, choices[0].binding), (0, ctrl_c));

    assert_eq!(change_count_label(0), "No changes");
    assert_eq!(change_count_label(1), "1 change");
    assert_eq!(change_count_label(3), "3 changes");
}
