//! UI integration tests: the production window, fed from replayed captures.

use std::path::PathBuf;
use std::time::Duration;

use hidra::state::{DeviceRead, Discovery};
use hidra::{MiceWindow, Source};
use gpui_kit::test::{TestAppContextExt as _, TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, MouseButton, Entity, TestAppContext, WindowBounds, WindowOptions, px, size,
};

fn capture(name: &str) -> PathBuf {
    PathBuf::from(format!("{}/../../captures/ratbagd/{name}", env!("CARGO_MANIFEST_DIR")))
}

fn open(cx: &mut TestAppContext, source: Source) -> (AnyWindowHandle, Entity<MiceWindow>) {
    cx.update(gpui_kit::init);
    cx.update(hidra::theme::init);
    cx.update(hidra::init);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1120.), px(1400.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| MiceWindow::new(source, window, cx)),
        )
        .expect("open test window")
    })
}

/// Wait until discovery and every device read have finished.
async fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<MiceWindow>) {
    for _ in 0..200 {
        cx.run_until_parked();
        if !cx.update(|cx| view.read(cx).is_busy()) {
            break;
        }
        cx.background_executor.timer(Duration::from_millis(10)).await;
    }
    assert!(!cx.update(|cx| view.read(cx).is_busy()), "reads did not finish");
    cx.wait_for(handle, Duration::from_secs(1), |_, _| true).await;
}

#[gpui_kit::test]
async fn proteus_replay_shows_profiles_and_switches_tabs(cx: &mut TestAppContext) {
    let source = Source::Replay {
        log: capture("g502-proteus-spectrum-c332.log"),
        segment: Some("Logitech Gaming Mouse G502".into()),
    };
    let (handle, view) = open(cx, source);
    settle(cx, handle, &view).await;

    cx.update(|cx| {
        let v = view.read(cx);
        assert_eq!(v.discovery(), &Discovery::Ready);
        assert_eq!(v.devices().len(), 1);
        let id = v.selected_device().expect("first device selected").to_string();
        assert!(matches!(v.device_read(&id), Some(DeviceRead::Ready(_))));
        // The active profile (sector 1) is shown first.
        assert_eq!(v.shown_profile(), Some(0x0001));
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // Clicking the third tab shows profile 3 (sector 3).
        let tab = window.find_by_label("Profile 3");
        assert!(tab.visible());
        assert_eq!(window.find_by_label("Profile 1").selected(), Some(true));
        let at = tab.bounds().center();
        window.pointer_move(at, None, cx);
        window.pointer_down(at, MouseButton::Left, cx);
        window.pointer_up(at, MouseButton::Left, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(view.read(cx).shown_profile(), Some(0x0003)));

    // Keyboard: Ctrl+Shift+Tab steps back to profile 2.
    cx.update_window(handle, |_, window, cx| window.press("ctrl-shift-tab", cx)).unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(view.read(cx).shown_profile(), Some(0x0002)));
}

#[gpui_kit::test]
async fn lightspeed_replay_selects_factory_profile(cx: &mut TestAppContext) {
    let source = Source::Replay { log: capture("both-mice-lightspeed-407f.log"), segment: Some("Logitech G502".into()) };
    let (handle, view) = open(cx, source);
    settle(cx, handle, &view).await;
    cx.update(|cx| assert_eq!(view.read(cx).shown_profile(), Some(0x0101)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find_by_label("Factory profile 1").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
async fn device_info_sheet_opens_from_the_keyboard_and_escape_closes_it(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let source = Source::Replay {
        log: capture("g502-proteus-spectrum-c332.log"),
        segment: Some("Logitech Gaming Mouse G502".into()),
    };
    let (handle, view) = open(cx, source);
    settle(cx, handle, &view).await;

    let facts = cx.update(|cx| view.read(cx).device_info_facts()).expect("facts once the device is read");
    assert!(facts.iter().any(|f| f.label == "HID++ protocol" && f.value == "4.2"));
    assert!(facts.iter().any(|f| f.label == "Profile 1" && f.value == "Sector 0x0001, CRC 1c32 OK"));

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!window.has_active_sheet(cx));
        window.press("ctrl-i", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_sheet(cx), "Ctrl+I opens the device info sheet");
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!window.has_active_sheet(cx), "Escape dismisses it");
    })
    .unwrap();
}

#[gpui_kit::test]
async fn theme_starts_dark_and_toggles(cx: &mut TestAppContext) {
    use gpui_kit::component::ActiveTheme as _;
    let source = Source::Replay {
        log: capture("g502-proteus-spectrum-c332.log"),
        segment: Some("Logitech Gaming Mouse G502".into()),
    };
    let (handle, view) = open(cx, source);
    settle(cx, handle, &view).await;
    cx.update(|cx| {
        assert!(cx.theme().is_dark(), "Graphite Dark is the default");
        assert_eq!(cx.theme().theme_name().as_ref(), "Graphite Dark");
    });
    cx.update_window(handle, |_, window, cx| window.press("ctrl-shift-l", cx)).unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!cx.theme().is_dark());
        assert_eq!(cx.theme().theme_name().as_ref(), "Graphite Light");
    });
}

#[gpui_kit::test]
async fn missing_capture_shows_discovery_error(cx: &mut TestAppContext) {
    let source = Source::Replay { log: capture("does-not-exist.log"), segment: None };
    let (handle, view) = open(cx, source);
    settle(cx, handle, &view).await;
    cx.update(|cx| {
        assert!(matches!(view.read(cx).discovery(), Discovery::Failed(_)));
        assert!(view.read(cx).devices().is_empty());
    });
}

#[gpui_kit::test]
async fn sidebar_click_and_keyboard_switch_devices(cx: &mut TestAppContext) {
    // Without a segment name, every device in the capture is listed.
    let source = Source::Replay { log: capture("both-mice-lightspeed-407f.log"), segment: None };
    let (handle, view) = open(cx, source);
    settle(cx, handle, &view).await;
    let ids: Vec<String> = cx.update(|cx| view.read(cx).devices().iter().map(|d| d.id.clone()).collect());
    assert!(ids.len() >= 2, "capture holds both mice: {ids:?}");
    cx.update(|cx| assert_eq!(view.read(cx).selected_device(), Some(ids[0].as_str())));

    // Sidebar rows are keyed "<menu>-<row>" by the Sidebar component.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("0-1", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(view.read(cx).selected_device(), Some(ids[1].as_str())));

    cx.update_window(handle, |_, window, cx| window.press("alt-up", cx)).unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(view.read(cx).selected_device(), Some(ids[0].as_str())));

    // Every device was read (none failed) from the capture.
    cx.update(|cx| {
        for id in &ids {
            assert!(matches!(view.read(cx).device_read(id), Some(DeviceRead::Ready(_))), "{id}");
        }
    });
}

/// The viewer's whole edit flow against a simulated mouse: open the editor
/// from the keyboard, change values, save, and see the device re-read.
#[gpui_kit::test]
async fn editing_a_profile_saves_to_the_simulated_mouse(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    use lg_snapshot::DeviceSource;

    let source = Source::demo().unwrap();
    let Source::Fixed(entries) = &source else { unreachable!() };
    let DeviceSource::Sim(memory) = entries[0].source.clone() else { unreachable!() };
    let backups = std::env::temp_dir().join(format!("hidra-ui-test-{}", std::process::id()));
    let (handle, view) = open(cx, source);
    cx.update(|cx| view.update(cx, |v, _| v.set_backup_dir(backups.clone())));
    settle(cx, handle, &view).await;
    cx.update(|cx| assert!(view.read(cx).can_edit_shown_profile()));

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("ctrl-e", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let editor = cx.update(|cx| view.read(cx).editor().cloned()).expect("Ctrl+E opens the editor");
    cx.update_window(handle, |_, window, cx| assert!(window.has_active_sheet(cx))).unwrap();
    cx.update(|cx| assert_eq!(editor.read(cx).pending_edits(cx), Ok(vec![]), "nothing changed yet"));

    // An off-step value is reported and nothing is written.
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |e, cx| {
            e.set_dpi_text(0, "3225", window, cx);
            e.save(cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(editor.read(cx).error().is_some_and(|m| m.contains("multiple of 50")));
        assert!(!editor.read(cx).is_saving());
    });
    assert_eq!(memory.lock().unwrap().begin_writes, 0);

    // A valid change saves, closes the editor and shows the new value.
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |e, cx| {
            e.set_dpi_text(0, "3200", window, cx);
            e.set_default_stage(2, cx);
            e.save(cx);
        });
    })
    .unwrap();
    settle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!window.has_active_sheet(cx), "the editor closes after a successful save");
    })
    .unwrap();
    cx.update(|cx| {
        let v = view.read(cx);
        assert!(v.editor().is_none());
        let id = v.selected_device().unwrap().to_string();
        let Some(DeviceRead::Ready(s)) = v.device_read(&id) else { panic!("device is shown as read") };
        let set = s.onboard.as_ref().unwrap().as_ref().unwrap().profiles.as_ref().unwrap();
        let c = set.profiles[0].content.as_ref().unwrap();
        assert_eq!(c.dpi_stages, [3200, 5150, 8500, 12000, 0]);
        assert_eq!(c.default_stage, 2);
        assert_eq!(c.buttons[5], "key F24");
    });
    let m = memory.lock().unwrap();
    assert_eq!(m.begin_writes, 1, "one sector write");
    assert_eq!(&m.sectors[&1][3..5], &3200u16.to_le_bytes());
    drop(m);
    let _ = std::fs::remove_dir_all(&backups);
}

#[gpui_kit::test]
async fn replayed_captures_offer_no_editing(cx: &mut TestAppContext) {
    let source = Source::Replay {
        log: capture("g502-proteus-spectrum-c332.log"),
        segment: Some("Logitech Gaming Mouse G502".into()),
    };
    let (handle, view) = open(cx, source);
    settle(cx, handle, &view).await;
    cx.update(|cx| assert!(!view.read(cx).can_edit_shown_profile()));
    cx.update_window(handle, |_, window, cx| window.press("ctrl-e", cx)).unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert!(view.read(cx).editor().is_none()));
}

/// The button map against a simulated mouse: select a button, choose an
/// action, save, and see the device re-read with the new assignment.
#[gpui_kit::test]
async fn button_map_assigns_an_action_and_saves_it(cx: &mut TestAppContext) {
    use lg_profiles::{ButtonBinding, ProfileEdit};
    use lg_snapshot::DeviceSource;

    let source = Source::demo().unwrap();
    let Source::Fixed(entries) = &source else { unreachable!() };
    let DeviceSource::Sim(memory) = entries[0].source.clone() else { unreachable!() };
    let backups = std::env::temp_dir().join(format!("hidra-map-test-{}", std::process::id()));
    let (handle, view) = open(cx, source);
    cx.update(|cx| view.update(cx, |v, _| v.set_backup_dir(backups.clone())));
    settle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();

    let map = cx.update(|cx| view.read(cx).button_map().cloned()).expect("an editable G502 profile has a button map");
    let f13 = ButtonBinding::parse("key:F13").unwrap();
    cx.update(|cx| {
        let m = map.read(cx);
        assert!(!m.is_readonly());
        assert_eq!(m.selected(), None);
        assert!(m.pending_edits().is_empty());
    });

    // Selecting a button and choosing an action is an unsaved change; nothing
    // is written yet.
    cx.update(|cx| {
        map.update(cx, |m, cx| {
            m.select(6, cx);
            m.assign(6, f13, cx);
        })
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();
    cx.update(|cx| {
        let m = map.read(cx);
        assert_eq!(m.selected(), Some(6));
        assert!(m.is_changed(6) && !m.is_changed(5));
        assert_eq!(m.pending_edits(), vec![ProfileEdit::SetButton { index: 6, binding: f13 }]);
    });
    assert_eq!(memory.lock().unwrap().begin_writes, 0);

    // Discard drops it; assigning again and saving writes one sector.
    cx.update(|cx| map.update(cx, |m, cx| m.discard(cx)));
    cx.update(|cx| assert!(map.read(cx).pending_edits().is_empty()));
    cx.update(|cx| {
        map.update(cx, |m, cx| {
            m.assign(6, f13, cx);
            m.save(cx);
        })
    });
    settle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();

    cx.update(|cx| {
        let v = view.read(cx);
        let id = v.selected_device().unwrap().to_string();
        let Some(DeviceRead::Ready(s)) = v.device_read(&id) else { panic!("device is shown as read") };
        let set = s.onboard.as_ref().unwrap().as_ref().unwrap().profiles.as_ref().unwrap();
        assert_eq!(set.profiles[0].content.as_ref().unwrap().buttons[6], "key F13");
        // The re-read device gets a fresh map with nothing pending.
        let fresh = v.button_map().expect("the map follows the new read");
        assert_ne!(fresh.entity_id(), map.entity_id());
        assert!(fresh.read(cx).pending_edits().is_empty());
        assert_eq!(fresh.read(cx).binding(6), Some(f13));
    });
    assert_eq!(memory.lock().unwrap().begin_writes, 1, "one sector write");
    let _ = std::fs::remove_dir_all(&backups);
}

/// A replayed capture shows the map but accepts no changes.
#[gpui_kit::test]
async fn button_map_is_readonly_for_replayed_captures(cx: &mut TestAppContext) {
    use lg_profiles::ButtonBinding;

    let source = Source::Replay { log: capture("g502-proteus-spectrum-c332.log"), segment: None };
    let (handle, view) = open(cx, source);
    settle(cx, handle, &view).await;
    cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();

    let map = cx.update(|cx| view.read(cx).button_map().cloned()).expect("the capture's profile has a button map");
    cx.update(|cx| {
        map.update(cx, |m, cx| {
            assert!(m.is_readonly());
            let before = m.binding(5);
            m.select(5, cx);
            m.assign(5, ButtonBinding::parse("key:F13").unwrap(), cx);
            m.save(cx);
            assert_eq!(m.binding(5), before);
            assert!(m.pending_edits().is_empty() && !m.is_saving());
        })
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();
}
