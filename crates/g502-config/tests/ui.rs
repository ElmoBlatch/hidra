//! UI integration tests: the production window, fed from replayed captures.

use std::path::PathBuf;
use std::time::Duration;

use g502_config::state::{DeviceRead, Discovery};
use g502_config::{MiceWindow, Source};
use gpui_kit::test::{TestAppContextExt as _, TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, MouseButton, Entity, TestAppContext, WindowBounds, WindowOptions, px, size,
};

fn capture(name: &str) -> PathBuf {
    PathBuf::from(format!("{}/../../captures/ratbagd/{name}", env!("CARGO_MANIFEST_DIR")))
}

fn open(cx: &mut TestAppContext, source: Source) -> (AnyWindowHandle, Entity<MiceWindow>) {
    cx.update(gpui_kit::init);
    cx.update(g502_config::theme::init);
    cx.update(g502_config::init);
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
