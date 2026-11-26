//! The main window: device sidebar beside the selected device's details.
//!
//! `MiceWindow` owns all application state. Device I/O runs on the GPUI
//! background executor; results come back through a weak handle and are
//! applied in `apply_*` methods. Nothing here writes to a device: the only
//! device calls are `Source::discover` and `lg_snapshot::read_entry`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, WindowExt as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    description_list::{DescriptionItem, DescriptionList},
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyTitle},
    h_flex,
    notification::Notification,
    scroll::ScrollableElement as _,
    sidebar::{Sidebar, SidebarMenu, SidebarMenuItem},
    skeleton::Skeleton,
    spinner::Spinner,
    tab::{Tab, TabBar},
    tag::Tag,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyBinding, ParentElement as _, Render, SharedString, Styled as _, Subscription,
    Window, actions, div, rems,
};
use lg_profiles::ProfileEdit;
use lg_snapshot::{DeviceEntry, DeviceSnapshot, EditError, ReadError};

use crate::button_map::{ButtonMap, ButtonMapEvent};
use crate::present::{self, Fact, Stat};
use crate::profile_detail::ProfileDetail;
use crate::profile_editor::{ProfileEditor, ProfileEditorEvent};
use crate::state::{self, DeviceRead, Discovery, Source};

actions!(
    hidra,
    [
        Refresh,
        SelectNextDevice,
        SelectPreviousDevice,
        SelectNextProfile,
        SelectPreviousProfile,
        EditProfile,
        ShowDeviceInfo,
        ToggleTheme,
        Quit
    ]
);

/// Key context of the main window.
pub const KEY_CONTEXT: &str = "MiceWindow";

/// Register the window's key bindings and app-level actions. Call once after
/// `gpui_kit::init`.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-r", Refresh, Some(KEY_CONTEXT)),
        KeyBinding::new("f5", Refresh, Some(KEY_CONTEXT)),
        KeyBinding::new("alt-down", SelectNextDevice, Some(KEY_CONTEXT)),
        KeyBinding::new("alt-up", SelectPreviousDevice, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-tab", SelectNextProfile, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-tab", SelectPreviousProfile, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-e", EditProfile, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-i", ShowDeviceInfo, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-l", ToggleTheme, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-q", Quit, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    crate::button_map::init(cx);
}

pub struct MiceWindow {
    source: Source,
    focus_handle: FocusHandle,
    discovery: Discovery,
    devices: Vec<DeviceEntry>,
    reads: HashMap<String, DeviceRead>,
    /// Devices with a read on the background executor. A device is never read
    /// twice at once: two readers on one hidraw node would see each other's
    /// replies.
    in_flight: HashSet<String>,
    selected: Option<String>,
    /// Profile chosen per device, by sector address.
    chosen_profiles: HashMap<String, u16>,
    /// The open profile editor and the device it edits.
    editor: Option<(String, Entity<ProfileEditor>)>,
    _editor_subscription: Option<Subscription>,
    /// The button map of the shown profile, and what it was built from.
    button_map: Option<(ButtonMapKey, Entity<ButtonMap>)>,
    _button_map_subscription: Option<Subscription>,
    backup_dir: PathBuf,
}

/// What a button map was built from: the device, the profile's address and
/// the identity of the snapshot. A new read or another profile gives a new
/// key, and with it a fresh map.
type ButtonMapKey = (String, u16, usize);

impl MiceWindow {
    pub fn new(source: Source, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let mut this = Self {
            source,
            focus_handle,
            discovery: Discovery::Loading,
            devices: Vec::new(),
            reads: HashMap::new(),
            in_flight: HashSet::new(),
            selected: None,
            chosen_profiles: HashMap::new(),
            editor: None,
            _editor_subscription: None,
            button_map: None,
            _button_map_subscription: None,
            backup_dir: state::default_backup_dir(),
        };
        this.start_discovery(cx);
        this
    }

    // ---- readers, for tests and the shell ----

    pub fn devices(&self) -> &[DeviceEntry] {
        &self.devices
    }

    pub fn selected_device(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    pub fn device_read(&self, id: &str) -> Option<&DeviceRead> {
        self.reads.get(id)
    }

    /// True while discovery or any device read is running.
    pub fn is_busy(&self) -> bool {
        self.discovery == Discovery::Loading || !self.in_flight.is_empty()
    }

    /// The sector address of the profile shown for the selected device.
    pub fn shown_profile(&self) -> Option<u16> {
        let id = self.selected.as_ref()?;
        let set = self.profile_set(id)?;
        state::shown_profile(set, self.chosen_profiles.get(id).copied())
    }

    fn profile_set(&self, id: &str) -> Option<&lg_snapshot::ProfileSet> {
        let snapshot = self.reads.get(id)?.snapshot()?;
        snapshot
            .onboard
            .as_ref()?
            .as_ref()
            .ok()?
            .profiles
            .as_ref()
            .ok()
    }

    /// The open profile editor, if any.
    pub fn editor(&self) -> Option<&Entity<ProfileEditor>> {
        self.editor.as_ref().map(|(_, e)| e)
    }

    /// Where pre-write backups are saved.
    pub fn set_backup_dir(&mut self, dir: PathBuf) {
        self.backup_dir = dir;
    }

    /// The button map of the shown profile, when it has one.
    pub fn button_map(&self) -> Option<&Entity<ButtonMap>> {
        self.button_map.as_ref().map(|(_, map)| map)
    }

    /// What the button map should show: its key, the profile content and why
    /// the profile can't be edited, if it can't. `None` when the shown profile
    /// has no G502 button layout to draw.
    fn button_map_target(
        &self,
    ) -> Option<(
        ButtonMapKey,
        lg_snapshot::ProfileContent,
        Option<SharedString>,
    )> {
        let id = self.selected.as_ref()?;
        let entry = self.devices.iter().find(|d| &d.id == id)?;
        let snapshot = self.reads.get(id)?.snapshot()?;
        let set = self.profile_set(id)?;
        let shown = state::shown_profile(set, self.chosen_profiles.get(id).copied())?;
        let profile = set.profiles.iter().find(|p| p.address == shown)?;
        let content = profile.content.as_ref().ok()?;
        if !present::is_g502_layout(entry, content) {
            return None;
        }
        let readonly = if !entry.write_verified() {
            Some("Editing isn't verified for this mouse yet.".into())
        } else if profile.factory {
            Some("Factory profiles can't be edited.".into())
        } else {
            None
        };
        let key = (id.clone(), shown, Arc::as_ptr(snapshot) as usize);
        Some((key, content.clone(), readonly))
    }

    /// Keep the button map in step with the shown profile. Unsaved changes
    /// belong to one profile as it was read, so they go when either changes.
    fn sync_button_map(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.button_map_target();
        if target.as_ref().map(|(key, ..)| key) == self.button_map.as_ref().map(|(key, _)| key) {
            return;
        }
        match target {
            Some((key, content, readonly)) => {
                let map = cx.new(|cx| ButtonMap::new(key.1, &content, readonly, window, cx));
                self._button_map_subscription =
                    Some(cx.subscribe_in(&map, window, Self::on_button_map_event));
                self.button_map = Some((key, map));
            }
            None => {
                self.button_map = None;
                self._button_map_subscription = None;
            }
        }
    }

    fn on_button_map_event(
        &mut self,
        map: &Entity<ButtonMap>,
        event: &ButtonMapEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ButtonMapEvent::Save(edits) = event;
        let Some((key, _)) = &self.button_map else {
            return;
        };
        let (id, address) = (key.0.clone(), key.1);
        let Some(entry) = self.devices.iter().find(|d| d.id == id).cloned() else {
            return;
        };
        if !self.in_flight.insert(id.clone()) {
            map.update(cx, |map, cx| map.save_failed(cx));
            window.push_notification(
                Notification::warning("The mouse is being read. Try again in a moment.")
                    .title("Not saved"),
                cx,
            );
            return;
        }
        let (edits, backup_dir) = (edits.clone(), self.backup_dir.clone());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    lg_snapshot::edit_entry(&entry, address, &edits, &backup_dir)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.apply_save(id, result, window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// True when the shown profile can be edited: a write-verified device, a
    /// user profile that decoded, and no read in progress.
    pub fn can_edit_shown_profile(&self) -> bool {
        self.editable_profile().is_some()
    }

    fn editable_profile(
        &self,
    ) -> Option<(&DeviceEntry, &DeviceSnapshot, &lg_snapshot::ProfileSnapshot)> {
        let id = self.selected.as_ref()?;
        let entry = self.devices.iter().find(|d| &d.id == id)?;
        if !entry.write_verified() || self.in_flight.contains(id) {
            return None;
        }
        let snapshot = self.reads.get(id)?.snapshot()?;
        let set = self.profile_set(id)?;
        let shown = state::shown_profile(set, self.chosen_profiles.get(id).copied())?;
        let profile = set.profiles.iter().find(|p| p.address == shown)?;
        (!profile.factory && profile.content.is_ok()).then_some((entry, snapshot.as_ref(), profile))
    }

    // ---- commands ----

    /// Open the editor for the shown profile.
    pub fn edit_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((entry, snapshot, profile)) = self.editable_profile() else {
            return;
        };
        let Ok(content) = profile.content.clone() else {
            return;
        };
        let (id, address, title) = (entry.id.clone(), profile.address, profile.title.clone());
        let range = snapshot
            .dpi_range
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .cloned();
        let rates = snapshot
            .polling_rates_hz
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .cloned()
            .unwrap_or_default();
        let g502_layout = present::is_g502_layout(entry, &content);

        let editor = cx
            .new(|cx| ProfileEditor::new(address, content, range, rates, g502_layout, window, cx));
        self._editor_subscription = Some(cx.subscribe_in(&editor, window, Self::on_editor_event));
        self.editor = Some((id, editor.clone()));
        window.open_sheet(cx, move |sheet, _, _| {
            sheet
                .title(format!("Edit {title}"))
                .size(rems(32.))
                .child(editor.clone())
        });
        cx.notify();
    }

    fn on_editor_event(
        &mut self,
        _: &Entity<ProfileEditor>,
        event: &ProfileEditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ProfileEditorEvent::Cancel => self.close_editor(window, cx),
            ProfileEditorEvent::Save(edits) => self.save_edits(edits.clone(), window, cx),
        }
    }

    fn close_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor = None;
        self._editor_subscription = None;
        window.close_sheet(cx);
        cx.notify();
    }

    /// Write the editor's edits through the guarded edit path, off the UI
    /// thread, then show the device as re-read after the write.
    fn save_edits(&mut self, edits: Vec<ProfileEdit>, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, editor)) = self.editor.clone() else {
            return;
        };
        let Some(entry) = self.devices.iter().find(|d| d.id == id).cloned() else {
            return;
        };
        if !self.in_flight.insert(id.clone()) {
            editor.update(cx, |e, cx| {
                e.save_failed("The mouse is being read. Try again in a moment.", cx)
            });
            return;
        }
        let address = editor.read(cx).address();
        let backup_dir = self.backup_dir.clone();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    lg_snapshot::edit_entry(&entry, address, &edits, &backup_dir)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.apply_save(id, result, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn apply_save(
        &mut self,
        id: String,
        result: Result<DeviceSnapshot, EditError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.in_flight.remove(&id);
        match result {
            Ok(snapshot) => {
                self.reads.insert(id, DeviceRead::Ready(Arc::new(snapshot)));
                if self.editor.is_some() {
                    self.close_editor(window, cx);
                }
                window.push_notification(Notification::success("Saved to the mouse"), cx);
            }
            Err(e) => {
                if let Some((_, map)) = &self.button_map {
                    map.update(cx, |map, cx| map.save_failed(cx));
                }
                if let Some((_, editor)) = &self.editor {
                    editor.update(cx, |editor, cx| editor.save_failed(e.to_string(), cx));
                } else {
                    window.push_notification(
                        Notification::error(e.to_string()).title("Couldn't save"),
                        cx,
                    );
                }
            }
        }
        cx.notify();
    }

    /// Re-run discovery, then re-read every device.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.discovery != Discovery::Loading {
            self.start_discovery(cx);
        }
    }

    fn start_discovery(&mut self, cx: &mut Context<Self>) {
        self.discovery = Discovery::Loading;
        cx.notify();
        let source = self.source.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { source.discover() }).await;
            this.update(cx, |this, cx| this.apply_discovery(result, cx))
                .ok();
        })
        .detach();
    }

    fn apply_discovery(
        &mut self,
        result: Result<Vec<DeviceEntry>, String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(devices) => {
                self.discovery = Discovery::Ready;
                self.reads
                    .retain(|id, _| devices.iter().any(|d| &d.id == id));
                self.selected = state::reconcile_selection(self.selected.as_deref(), &devices);
                self.devices = devices;
                for entry in self.devices.clone() {
                    self.start_read(entry, cx);
                }
            }
            Err(e) => self.discovery = Discovery::Failed(e),
        }
        cx.notify();
    }

    fn start_read(&mut self, entry: DeviceEntry, cx: &mut Context<Self>) {
        if !self.in_flight.insert(entry.id.clone()) {
            return;
        }
        let previous = self
            .reads
            .get(&entry.id)
            .and_then(|r| r.snapshot().cloned());
        self.reads
            .insert(entry.id.clone(), DeviceRead::Reading { previous });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let id = entry.id.clone();
            let result = cx
                .background_spawn(async move { lg_snapshot::read_entry(&entry) })
                .await;
            this.update(cx, |this, cx| this.apply_read(id, result, cx))
                .ok();
        })
        .detach();
    }

    fn apply_read(
        &mut self,
        id: String,
        result: Result<DeviceSnapshot, ReadError>,
        cx: &mut Context<Self>,
    ) {
        self.in_flight.remove(&id);
        // The device may have disappeared in a refresh while it was being read.
        if self.devices.iter().any(|d| d.id == id) {
            let read = match result {
                Ok(s) => DeviceRead::Ready(Arc::new(s)),
                Err(e) => DeviceRead::Failed(e),
            };
            self.reads.insert(id, read);
        }
        cx.notify();
    }

    fn retry(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(entry) = self.devices.iter().find(|d| d.id == id).cloned() {
            self.start_read(entry, cx);
        }
    }

    pub fn select_device(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.devices.iter().any(|d| d.id == id) && self.selected.as_deref() != Some(id) {
            self.selected = Some(id.to_string());
            cx.notify();
        }
    }

    pub fn select_profile(&mut self, address: u16, cx: &mut Context<Self>) {
        if let Some(id) = self.selected.clone() {
            self.chosen_profiles.insert(id, address);
            cx.notify();
        }
    }

    fn step_device(&mut self, delta: isize, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.devices.iter().map(|d| d.id.clone()).collect();
        if let Some(id) = state::step(&ids, self.selected.as_ref(), delta) {
            self.select_device(&id, cx);
        }
    }

    fn step_profile(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let Some(set) = self.profile_set(&id) else {
            return;
        };
        let addresses: Vec<u16> = set.profiles.iter().map(|p| p.address).collect();
        let current = self.shown_profile();
        if let Some(next) = state::step(&addresses, current.as_ref(), delta) {
            self.select_profile(next, cx);
        }
    }

    /// Facts for the selected device's info sheet, once it has been read.
    pub fn device_info_facts(&self) -> Option<Vec<Fact>> {
        let entry = self
            .selected
            .as_ref()
            .and_then(|id| self.devices.iter().find(|d| &d.id == id))?;
        let snapshot = self.reads.get(&entry.id)?.snapshot()?;
        let mut facts = present::summary_facts(entry, snapshot);
        facts.extend(present::storage_facts(snapshot));
        Some(facts)
    }

    /// Open the sheet with the selected device's technical details.
    pub fn show_device_info(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(facts) = self.device_info_facts() else {
            return;
        };
        window.open_sheet(cx, move |sheet, _, _| {
            sheet.title("Device info").size(rems(28.)).child(
                DescriptionList::horizontal()
                    .small()
                    .columns(1)
                    .label_width(rems(9.))
                    .children(
                        facts
                            .iter()
                            .map(|f| DescriptionItem::new(f.label.clone()).value(f.value.clone())),
                    ),
            )
        });
    }

    fn on_edit_profile(&mut self, _: &EditProfile, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_profile(window, cx);
    }
    fn on_show_device_info(
        &mut self,
        _: &ShowDeviceInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_device_info(window, cx);
    }
    fn on_toggle_theme(&mut self, _: &ToggleTheme, _: &mut Window, cx: &mut Context<Self>) {
        crate::theme::toggle(cx);
    }
    fn on_refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.refresh(cx);
    }
    fn on_next_device(&mut self, _: &SelectNextDevice, _: &mut Window, cx: &mut Context<Self>) {
        self.step_device(1, cx);
    }
    fn on_previous_device(
        &mut self,
        _: &SelectPreviousDevice,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_device(-1, cx);
    }
    fn on_next_profile(&mut self, _: &SelectNextProfile, _: &mut Window, cx: &mut Context<Self>) {
        self.step_profile(1, cx);
    }
    fn on_previous_profile(
        &mut self,
        _: &SelectPreviousProfile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_profile(-1, cx);
    }

    // ---- rendering ----

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.devices.iter().map(|d| {
            let id = d.id.clone();
            let connection: SharedString = d.connection.label().into();
            SidebarMenuItem::new(d.model.clone())
                .accessibility_label(format!("{}, {}", d.model, d.connection.label()))
                .active(self.selected.as_deref() == Some(d.id.as_str()))
                .suffix(move |_, cx| {
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(connection.clone())
                })
                .on_click(cx.listener(move |this, _, _, cx| this.select_device(&id, cx)))
        });
        Sidebar::new("devices")
            .collapsible(false)
            .w(rems(16.))
            .header(
                h_flex()
                    .w_full()
                    .justify_between()
                    .pl_2()
                    .child(div().text_sm().font_semibold().child("Devices"))
                    .child(
                        Button::new("refresh")
                            .ghost()
                            .small()
                            .icon(IconName::RefreshCw)
                            .accessibility_label("Refresh")
                            .loading(self.is_busy())
                            .tooltip_with_action("Refresh", &Refresh, Some(KEY_CONTEXT))
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .child(SidebarMenu::new().children(items))
            .footer(
                h_flex()
                    .w_full()
                    .justify_between()
                    .pl_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Read-only"),
                    )
                    .child(
                        Button::new("toggle-theme")
                            .ghost()
                            .small()
                            .icon(if cx.theme().is_dark() {
                                IconName::Sun
                            } else {
                                IconName::Moon
                            })
                            .accessibility_label(if cx.theme().is_dark() {
                                "Switch to light theme"
                            } else {
                                "Switch to dark theme"
                            })
                            .tooltip_with_action(
                                if cx.theme().is_dark() {
                                    "Light theme"
                                } else {
                                    "Dark theme"
                                },
                                &ToggleTheme,
                                Some(KEY_CONTEXT),
                            )
                            .on_click(cx.listener(|_, _, _, cx| crate::theme::toggle(cx))),
                    ),
            )
    }

    fn render_main(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let main = v_flex().id("main").flex_1().min_w_0().h_full();
        let selected = self
            .selected
            .as_ref()
            .and_then(|id| self.devices.iter().find(|d| &d.id == id));
        match (selected, &self.discovery) {
            (Some(entry), _) => main.child(self.render_device(entry, cx)).into_any_element(),
            (None, Discovery::Loading) => main
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(cx.theme().muted_foreground)
                .child(Spinner::new())
                .child(div().text_sm().child("Looking for mice…"))
                .into_any_element(),
            (None, Discovery::Failed(e)) => main
                .p_6()
                .child(Alert::error("discovery-error", e.clone()).title("Couldn't look for mice"))
                .into_any_element(),
            (None, Discovery::Ready) => main
                .p_6()
                .child(
                    Empty::new()
                        .header(
                            EmptyHeader::new()
                                .title(EmptyTitle::new().child("No mice found"))
                                .description(EmptyDescription::new().child(
                                    "Connect a G502 by USB or through its LIGHTSPEED receiver, then refresh.",
                                )),
                        )
                        .child(
                            Button::new("empty-refresh")
                                .outline()
                                .icon(IconName::RefreshCw)
                                .label("Refresh")
                                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                        ),
                )
                .into_any_element(),
        }
    }

    fn render_device(&self, entry: &DeviceEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let read = self.reads.get(&entry.id);
        let snapshot = read.and_then(|r| r.snapshot());
        let reading = read.is_some_and(DeviceRead::is_reading);
        let muted = cx.theme().muted_foreground;

        let header =
            h_flex()
                .flex_shrink_0()
                .justify_between()
                .gap_4()
                .px_6()
                .pt_5()
                .pb_4()
                .child(
                    v_flex()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_xl()
                                .font_semibold()
                                .truncate()
                                .child(present::device_title(entry, snapshot.map(|s| s.as_ref()))),
                        )
                        .child(div().text_sm().text_color(muted).truncate().child(format!(
                            "{} · {}",
                            entry.model,
                            entry.connection.description()
                        ))),
                )
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_3()
                        .when(reading && snapshot.is_some(), |this| {
                            this.child(
                                h_flex()
                                    .gap_2()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(Spinner::new().small())
                                    .child("Reading…"),
                            )
                        })
                        .child(
                            Button::new("device-info")
                                .outline()
                                .small()
                                .icon(IconName::Info)
                                .label("Device info…")
                                .disabled(snapshot.is_none())
                                .tooltip_with_action(
                                    "Firmware, protocol and storage",
                                    &ShowDeviceInfo,
                                    Some(KEY_CONTEXT),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.show_device_info(window, cx)
                                })),
                        ),
                );

        let stats = snapshot
            .map(|s| present::headline_stats(s))
            .unwrap_or_default();
        let body = match (read, snapshot) {
            (_, Some(s)) => self.render_profiles(entry, s, cx).into_any_element(),
            (Some(DeviceRead::Failed(e)), None) => {
                self.render_read_error(entry, e, cx).into_any_element()
            }
            _ => render_loading().into_any_element(),
        };

        v_flex()
            .size_full()
            .child(header)
            .when(!stats.is_empty(), |this| this.child(stat_strip(stats, cx)))
            .child(
                div()
                    .id(SharedString::from(format!("device-body-{}", entry.id)))
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .overflow_y_scrollbar()
                    .child(div().p_6().child(body)),
            )
    }

    fn render_profiles(
        &self,
        entry: &DeviceEntry,
        s: &DeviceSnapshot,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let section = v_flex().gap_4();
        let onboard = match &s.onboard {
            None => {
                return section
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child("This mouse has no onboard profile memory."),
                    )
                    .into_any_element();
            }
            Some(Err(e)) => {
                return section
                    .child(
                        Alert::error("onboard-error", e.clone())
                            .title("Couldn't read onboard profiles"),
                    )
                    .into_any_element();
            }
            Some(Ok(o)) => o,
        };
        let set = match &onboard.profiles {
            Ok(set) => set,
            Err(e) => {
                return section
                    .child(
                        Alert::warning("profiles-error", e.clone())
                            .title("Couldn't decode profiles"),
                    )
                    .into_any_element();
            }
        };
        let can_edit = self.can_edit_shown_profile();
        let edit_unavailable = if !entry.write_verified() {
            "Editing isn't verified for this mouse yet"
        } else if set.factory {
            "Factory profiles can't be edited"
        } else {
            "The mouse is being read"
        };
        let shown = state::shown_profile(set, self.chosen_profiles.get(&entry.id).copied());
        let shown_ix = set
            .profiles
            .iter()
            .position(|p| Some(p.address) == shown)
            .unwrap_or(0);
        let addresses: Vec<u16> = set.profiles.iter().map(|p| p.address).collect();

        let tabs = TabBar::new(SharedString::from(format!("profiles-{}", entry.id)))
            .underline()
            .selected_index(shown_ix)
            .children(set.profiles.iter().map(|p| {
                Tab::new().label(p.title.clone()).when(p.active, |tab| {
                    tab.suffix(Tag::secondary().small().child("Active"))
                })
            }))
            .on_click(cx.listener(move |this, ix: &usize, _, cx| {
                if let Some(&address) = addresses.get(*ix) {
                    this.select_profile(address, cx);
                }
            }));

        section
            .when(set.factory, |this| {
                this.child(div().text_sm().text_color(muted).child(
                    "No user profiles are stored on this mouse, so it runs its factory profile.",
                ))
            })
            .child(
                h_flex()
                    .items_end()
                    .gap_4()
                    .child(div().flex_1().min_w_0().child(tabs))
                    .child(
                        Button::new("edit-profile")
                            .small()
                            .label("Edit…")
                            .disabled(!can_edit)
                            .map(|button| {
                                if can_edit {
                                    button.tooltip_with_action(
                                        "Edit this profile",
                                        &EditProfile,
                                        Some(KEY_CONTEXT),
                                    )
                                } else {
                                    button.tooltip(edit_unavailable)
                                }
                            })
                            .on_click(
                                cx.listener(|this, _, window, cx| this.edit_profile(window, cx)),
                            ),
                    ),
            )
            .when_some(set.profiles.get(shown_ix).cloned(), |this, p| {
                let g502_layout = p
                    .content
                    .as_ref()
                    .is_ok_and(|c| present::is_g502_layout(entry, c));
                let live_dpi = s
                    .sensor_dpi
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .map(|d| d.current);
                let map = self
                    .button_map
                    .as_ref()
                    .filter(|(key, _)| key.1 == p.address)
                    .map(|(_, map)| map.clone());
                this.child(
                    ProfileDetail::new(p)
                        .g502_layout(g502_layout)
                        .live_dpi(live_dpi)
                        .button_map(map),
                )
            })
            .into_any_element()
    }

    fn render_read_error(
        &self,
        entry: &DeviceEntry,
        e: &ReadError,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = entry.id.clone();
        let alert = match e {
            ReadError::PermissionDenied { path } => Alert::error(
                "read-error",
                "Install packaging/70-hidra.rules so your user can read the mouse, then reconnect it.",
            )
            .title(format!("Permission denied for {path}")),
            ReadError::Unreachable(detail) => Alert::warning(
                "read-error",
                format!("The mouse may be asleep or out of range. Move it to wake it, then try again. ({detail})"),
            )
            .title(format!("{} isn't responding", entry.model)),
            ReadError::Other(detail) => {
                Alert::error("read-error", detail.clone()).title(format!("Couldn't read {}", entry.model))
            }
        };
        v_flex().gap_4().items_start().child(alert).child(
            Button::new("retry")
                .outline()
                .label("Try again")
                .loading(self.in_flight.contains(&entry.id))
                .on_click(cx.listener(move |this, _, _, cx| this.retry(&id, cx))),
        )
    }
}

/// The headline values under the device title, on one baseline.
fn stat_strip(stats: Vec<Stat>, cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .flex_shrink_0()
        .flex_wrap()
        .gap_x_10()
        .gap_y_3()
        .px_6()
        .pb_5()
        .children(stats.into_iter().map(|stat| {
            v_flex()
                .gap_1()
                .child(div().text_xs().text_color(muted).child(stat.label))
                .child(
                    h_flex()
                        .items_baseline()
                        .gap_1()
                        .child(div().text_2xl().font_semibold().child(stat.value))
                        .when_some(stat.unit, |this, unit| {
                            this.child(div().text_sm().text_color(muted).child(unit))
                        }),
                )
        }))
}

/// Placeholder in the shape of the loaded view while the first read runs.
fn render_loading() -> impl IntoElement {
    v_flex()
        .id("loading")
        .gap_8()
        .child(
            v_flex()
                .gap_2()
                .child(Skeleton::new().h_5().w_24())
                .child(Skeleton::new().h(rems(10.)).w_full()),
        )
        .child(
            v_flex()
                .gap_2()
                .child(Skeleton::new().h_5().w_24())
                .child(Skeleton::new().h_8().w(rems(20.)))
                .child(Skeleton::new().h(rems(16.)).w_full()),
        )
}

impl Focusable for MiceWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MiceWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_button_map(window, cx);
        h_flex()
            .id("mice-window")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_refresh))
            .on_action(cx.listener(Self::on_next_device))
            .on_action(cx.listener(Self::on_previous_device))
            .on_action(cx.listener(Self::on_next_profile))
            .on_action(cx.listener(Self::on_previous_profile))
            .on_action(cx.listener(Self::on_edit_profile))
            .on_action(cx.listener(Self::on_show_device_info))
            .on_action(cx.listener(Self::on_toggle_theme))
            .items_stretch()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_sidebar(cx))
            .child(self.render_main(cx))
    }
}
