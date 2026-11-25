//! The profile editor shown in a sheet: polling rate, DPI stages and button
//! assignments of one onboard profile.
//!
//! The editor owns only form state. It never touches a device: `Save` emits
//! the field edits and the window performs the guarded write.

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, StyledExt as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    radio::Radio,
    select::{Select, SelectEvent, SelectState},
    tag::Tag,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Window, div, rems,
};
use lg_profiles::ProfileEdit;
use lg_snapshot::{DpiRange, ProfileContent};

use crate::edit_form::{self, ActionChoice, FormValues};
use crate::present;

type Choices = Entity<SelectState<Vec<SharedString>>>;

pub enum ProfileEditorEvent {
    /// The user asked to save these edits. Never empty.
    Save(Vec<ProfileEdit>),
    Cancel,
}

pub struct ProfileEditor {
    address: u16,
    original: ProfileContent,
    range: Option<DpiRange>,
    rates: Vec<u16>,
    g502_layout: bool,
    dpi_inputs: Vec<Entity<InputState>>,
    default_stage: usize,
    polling: Choices,
    button_selects: Vec<Choices>,
    button_choices: Vec<Vec<ActionChoice>>,
    saving: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ProfileEditorEvent> for ProfileEditor {}

impl ProfileEditor {
    pub fn new(
        address: u16,
        original: ProfileContent,
        range: Option<DpiRange>,
        rates: Vec<u16>,
        g502_layout: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = Vec::new();
        let form = FormValues::from_profile(&original);

        let dpi_inputs: Vec<Entity<InputState>> = form
            .dpi
            .iter()
            .map(|text| cx.new(|cx| InputState::new(window, cx).placeholder("Off").default_value(text.clone())))
            .collect();
        for input in &dpi_inputs {
            subscriptions.push(cx.subscribe_in(input, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.error = None;
                    cx.notify();
                }
            }));
        }

        // The profile's own rate is always offered, even if the device did not list it.
        let mut rates = rates;
        if !rates.contains(&original.report_rate_hz) {
            rates.push(original.report_rate_hz);
            rates.sort_unstable();
        }
        let rate_labels: Vec<SharedString> = rates.iter().map(|hz| rate_label(*hz)).collect();
        let rate_ix = rates.iter().position(|hz| *hz == original.report_rate_hz).unwrap_or(0);
        let polling = cx.new(|cx| SelectState::new(rate_labels, Some(IndexPath::new(rate_ix)), window, cx));
        subscriptions.push(cx.subscribe_in(&polling, window, Self::on_select_changed));

        let mut button_selects = Vec::new();
        let mut button_choices = Vec::new();
        for binding in &original.bindings {
            let (choices, current) = edit_form::action_choices(*binding);
            let labels: Vec<SharedString> = choices.iter().map(|c| SharedString::from(c.label.clone())).collect();
            let select = cx.new(|cx| SelectState::new(labels, Some(IndexPath::new(current)), window, cx));
            subscriptions.push(cx.subscribe_in(&select, window, Self::on_select_changed));
            button_selects.push(select);
            button_choices.push(choices);
        }

        Self {
            address,
            default_stage: form.default_stage,
            original,
            range,
            rates,
            g502_layout,
            dpi_inputs,
            polling,
            button_selects,
            button_choices,
            saving: false,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn on_select_changed(
        &mut self,
        _: &Choices,
        _: &SelectEvent<Vec<SharedString>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        cx.notify();
    }

    // ---- readers and commands, for the window and tests ----

    pub fn address(&self) -> u16 {
        self.address
    }

    pub fn is_saving(&self) -> bool {
        self.saving
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// What the form holds right now.
    pub fn form_values(&self, cx: &App) -> FormValues {
        let dpi: Vec<String> = self.dpi_inputs.iter().map(|i| i.read(cx).value().to_string()).collect();
        let polling_hz = self
            .polling
            .read(cx)
            .selected_value()
            .and_then(|label| self.rates.iter().copied().find(|hz| rate_label(*hz) == *label))
            .unwrap_or(self.original.report_rate_hz);
        let buttons = self
            .button_selects
            .iter()
            .zip(&self.button_choices)
            .zip(&self.original.bindings)
            .map(|((select, choices), original)| {
                select
                    .read(cx)
                    .selected_value()
                    .and_then(|label| choices.iter().find(|c| c.label == label.as_ref()))
                    .map(|c| c.binding)
                    .unwrap_or(*original)
            })
            .collect();
        FormValues {
            dpi: dpi.try_into().expect("one input per DPI stage"),
            default_stage: self.default_stage,
            polling_hz,
            buttons,
        }
    }

    /// The edits the form amounts to, or why it is invalid.
    pub fn pending_edits(&self, cx: &App) -> Result<Vec<ProfileEdit>, String> {
        edit_form::form_edits(&self.original, &self.form_values(cx), self.range.as_ref())
    }

    pub fn set_dpi_text(&mut self, stage: usize, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = self.dpi_inputs.get(stage) {
            input.update(cx, |state, cx| state.set_value(text.to_string(), window, cx));
            self.error = None;
            cx.notify();
        }
    }

    pub fn set_default_stage(&mut self, stage: usize, cx: &mut Context<Self>) {
        self.default_stage = stage;
        self.error = None;
        cx.notify();
    }

    /// Emit `Save` if the form is valid and holds changes.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        match self.pending_edits(cx) {
            Ok(edits) if !edits.is_empty() => {
                self.saving = true;
                self.error = None;
                cx.emit(ProfileEditorEvent::Save(edits));
                cx.notify();
            }
            Ok(_) => {}
            Err(message) => {
                self.error = Some(message.into());
                cx.notify();
            }
        }
    }

    /// The window reports a failed save: show it and let the user try again.
    pub fn save_failed(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.saving = false;
        self.error = Some(message.into());
        cx.notify();
    }

    // ---- rendering ----

    fn render_dpi(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let hint: SharedString = match &self.range {
            Some(DpiRange::Range { min, max, step }) => format!(
                "{} to {} in steps of {step}. Leave a stage empty to turn it off.",
                present::grouped((*min).into()),
                present::grouped((*max).into())
            )
            .into(),
            _ => "Leave a stage empty to turn it off.".into(),
        };
        v_flex()
            .gap_2()
            .children(self.dpi_inputs.iter().enumerate().map(|(ix, input)| {
                h_flex()
                    .gap_3()
                    .child(div().w_4().flex_shrink_0().text_sm().text_color(muted).child((ix + 1).to_string()))
                    .child(div().w_32().flex_shrink_0().child(
                        Input::new(input)
                            .small()
                            .suffix(div().text_xs().text_color(muted).child("DPI"))
                            .disabled(self.saving),
                    ))
                    .child(
                        Radio::new(("default-stage", ix))
                            .label("Default")
                            .checked(self.default_stage == ix)
                            .disabled(self.saving)
                            .on_click(cx.listener(move |this, _: &bool, _, cx| this.set_default_stage(ix, cx))),
                    )
            }))
            .child(div().text_xs().text_color(muted).child(hint))
    }

    fn render_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let rows = present::button_rows(&self.original, self.g502_layout);
        v_flex().gap_2().children(rows.into_iter().zip(&self.button_selects).enumerate().map(|(ix, (row, select))| {
            h_flex()
                .gap_3()
                .child(
                    h_flex()
                        .w_10()
                        .flex_shrink_0()
                        .when_some(row.key, |this, key| this.child(Tag::secondary().small().outline().child(key))),
                )
                .child(div().w(rems(8.)).flex_shrink_0().text_sm().text_color(muted).truncate().child(row.name.clone()))
                .child(
                    div().flex_1().min_w_0().child(
                        Select::new(select)
                            .id(("button-action", ix))
                            .small()
                            .accessibility_label(format!("Action for {}", row.name))
                            .disabled(self.saving),
                    ),
                )
        }))
    }
}

fn rate_label(hz: u16) -> SharedString {
    format!("{} Hz", present::grouped(hz.into())).into()
}

fn section(title: &'static str, body: impl IntoElement) -> impl IntoElement {
    v_flex().gap_3().child(div().text_sm().font_semibold().child(title)).child(body)
}

impl Render for ProfileEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pending = self.pending_edits(cx);
        let (status, invalid): (SharedString, bool) = match &pending {
            Ok(edits) => (edit_form::change_count_label(edits.len()).into(), false),
            Err(message) => (message.clone().into(), true),
        };
        let can_save = pending.as_ref().is_ok_and(|e| !e.is_empty()) && !self.saving;
        let (muted, danger) = (cx.theme().muted_foreground, cx.theme().danger);

        v_flex()
            .gap_6()
            .child(section(
                "Polling rate",
                div().w_40().child(
                    Select::new(&self.polling)
                        .id("polling-rate")
                        .small()
                        .accessibility_label("Polling rate")
                        .disabled(self.saving),
                ),
            ))
            .child(section("DPI stages", self.render_dpi(cx)))
            .child(section("Buttons", self.render_buttons(cx)))
            .when_some(self.error.clone(), |this, error| {
                this.child(Alert::error("save-error", error).title("Couldn't save"))
            })
            .child(
                h_flex()
                    .justify_between()
                    .gap_3()
                    .pt_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_color(if invalid { danger } else { muted })
                            .child(status),
                    )
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .gap_2()
                            .child(
                                Button::new("cancel-edit")
                                    .label("Cancel")
                                    .disabled(self.saving)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ProfileEditorEvent::Cancel))),
                            )
                            .child(
                                Button::new("save-edit")
                                    .primary()
                                    .label("Save to mouse")
                                    .loading(self.saving)
                                    .disabled(!can_save)
                                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                            ),
                    ),
            )
    }
}
