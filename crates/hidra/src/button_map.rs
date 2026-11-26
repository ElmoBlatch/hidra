//! The button map: a drawing of the mouse with a marker and a label card per
//! button. Selecting a button opens the list of actions beside the drawing;
//! choosing one records a pending change. Nothing here touches a device: Save
//! emits the pending edits and the window writes them.
//!
//! Keyboard: each button's card is a Tab stop, and Enter or Space opens its
//! action list. In the list, Up, Down, Home and End move, Enter or Space
//! assigns, and Escape closes the list and returns to the card.

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, Context, EventEmitter, FocusHandle, Hsla, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, PathBuilder, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, actions, canvas, div, point, px, rems,
};
use lg_profiles::{ButtonBinding, ProfileEdit};
use lg_snapshot::ProfileContent;

use crate::edit_form::{self, ActionChoice};
use crate::mouse_art::{self, ANCHORS, Fill, SHAPES, Segment, Stroke, VIEW_H, VIEW_W};
use crate::present;

actions!(button_map, [CloseActions, NextAction, PreviousAction, FirstAction, LastAction, ChooseAction]);

/// Key context of the whole map.
const MAP_CONTEXT: &str = "ButtonMap";
/// Key context of the action list.
const ACTIONS_CONTEXT: &str = "ButtonActions";

/// Register the map's key bindings. Call once after `gpui_kit::init`.
pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", CloseActions, Some(MAP_CONTEXT)),
        KeyBinding::new("down", NextAction, Some(ACTIONS_CONTEXT)),
        KeyBinding::new("up", PreviousAction, Some(ACTIONS_CONTEXT)),
        KeyBinding::new("home", FirstAction, Some(ACTIONS_CONTEXT)),
        KeyBinding::new("end", LastAction, Some(ACTIONS_CONTEXT)),
        KeyBinding::new("enter", ChooseAction, Some(ACTIONS_CONTEXT)),
        KeyBinding::new("space", ChooseAction, Some(ACTIONS_CONTEXT)),
    ]);
}

/// One line of the action list: a group title or an assignable action.
enum ListRow {
    Title(&'static str),
    /// The action and its position among the actions alone.
    Choice(usize, ActionChoice),
}

/// The action list for a button whose stored assignment is `stored`.
fn list_rows(stored: ButtonBinding) -> Vec<ListRow> {
    let mut rows = Vec::new();
    let mut position = 0;
    for group in edit_form::action_groups(stored) {
        rows.push(ListRow::Title(group.title));
        for choice in group.choices {
            rows.push(ListRow::Choice(position, choice));
            position += 1;
        }
    }
    rows
}

/// What the button map asks its owner to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ButtonMapEvent {
    /// Write these button assignments to the profile.
    Save(Vec<ProfileEdit>),
}

/// One button as the map shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ButtonInfo {
    /// Short text for the marker: "G6", or "TR" for a wheel tilt.
    marker: SharedString,
    /// "G6 · Sniper", or just the name when the button has no G label.
    caption: SharedString,
    name: SharedString,
}

pub struct ButtonMap {
    address: u16,
    buttons: Vec<ButtonInfo>,
    /// Assignments as stored on the mouse.
    stored: Vec<ButtonBinding>,
    /// Assignments as edited.
    current: Vec<ButtonBinding>,
    selected: Option<usize>,
    /// Why the profile can't be edited, when it can't.
    readonly: Option<SharedString>,
    saving: bool,
    /// Focus of each button's card.
    card_focus: Vec<FocusHandle>,
    /// Focus of the action list.
    list_focus: FocusHandle,
    list_scroll: ScrollHandle,
    /// The action the keyboard is on, counted among the actions alone.
    highlighted: usize,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ButtonMapEvent> for ButtonMap {}

impl ButtonMap {
    /// A map of `content`'s buttons, which must use the G502 layout.
    /// `readonly` names why assignments can't be changed, if they can't.
    pub fn new(
        address: u16,
        content: &ProfileContent,
        readonly: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let buttons: Vec<ButtonInfo> = present::button_rows(content, true)
            .into_iter()
            .enumerate()
            .map(|(ix, row)| {
                let marker = match (&row.key, ix) {
                    (Some(key), _) => key.clone(),
                    (None, 9) => "TR".to_string(),
                    (None, 10) => "TL".to_string(),
                    (None, _) => (ix + 1).to_string(),
                };
                let caption = match &row.key {
                    Some(key) => format!("{key} · {}", row.name),
                    None => row.name.clone(),
                };
                ButtonInfo { marker: marker.into(), caption: caption.into(), name: row.name.into() }
            })
            .collect();
        let card_focus: Vec<FocusHandle> = buttons.iter().map(|_| cx.focus_handle().tab_stop(true)).collect();
        let list_focus = cx.focus_handle().tab_stop(true);
        // Focus decides how markers and the highlighted action are drawn.
        let mut subscriptions = Vec::new();
        for handle in card_focus.iter().chain([&list_focus]) {
            subscriptions.push(cx.on_focus(handle, window, |_, _, cx| cx.notify()));
            subscriptions.push(cx.on_blur(handle, window, |_, _, cx| cx.notify()));
        }
        Self {
            address,
            card_focus,
            list_focus,
            list_scroll: ScrollHandle::new(),
            highlighted: 0,
            _subscriptions: subscriptions,
            buttons,
            stored: content.bindings.clone(),
            current: content.bindings.clone(),
            selected: None,
            readonly,
            saving: false,
        }
    }

    // ---- readers ----

    pub fn address(&self) -> u16 {
        self.address
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn is_saving(&self) -> bool {
        self.saving
    }

    pub fn is_readonly(&self) -> bool {
        self.readonly.is_some()
    }

    /// The assignment shown for button `ix`, including unsaved changes.
    pub fn binding(&self, ix: usize) -> Option<ButtonBinding> {
        self.current.get(ix).copied()
    }

    /// True when button `ix` has an unsaved change.
    pub fn is_changed(&self, ix: usize) -> bool {
        self.current.get(ix) != self.stored.get(ix)
    }

    /// The edits that would be written by Save.
    pub fn pending_edits(&self) -> Vec<ProfileEdit> {
        self.current
            .iter()
            .zip(&self.stored)
            .enumerate()
            .filter(|(_, (new, old))| new != old)
            .map(|(index, (new, _))| ProfileEdit::SetButton { index, binding: *new })
            .collect()
    }

    /// The action the keyboard is on in the open action list.
    pub fn highlighted_action(&self) -> Option<ButtonBinding> {
        self.choices().get(self.highlighted).map(|c| c.binding)
    }

    /// True when button `ix`'s card has keyboard focus.
    pub fn is_button_focused(&self, ix: usize, window: &Window) -> bool {
        self.card_focus.get(ix).is_some_and(|handle| handle.is_focused(window))
    }

    /// True when the action list has keyboard focus.
    pub fn is_action_list_focused(&self, window: &Window) -> bool {
        self.list_focus.is_focused(window)
    }

    /// The actions offered for the selected button, in list order.
    fn choices(&self) -> Vec<ActionChoice> {
        let Some(stored) = self.selected.and_then(|ix| self.stored.get(ix)) else {
            return Vec::new();
        };
        list_rows(*stored)
            .into_iter()
            .filter_map(|row| match row {
                ListRow::Choice(_, choice) => Some(choice),
                ListRow::Title(_) => None,
            })
            .collect()
    }

    // ---- commands ----

    /// Select button `ix`, opening its action list.
    pub fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.current.len() && ix < ANCHORS.len() {
            self.selected = Some(ix);
            let current = self.current[ix];
            let at = self.choices().iter().position(|c| c.binding == current).unwrap_or(0);
            self.highlight(at, cx);
        }
    }

    /// Select button `ix` and move keyboard focus to its action list.
    pub fn open(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.select(ix, cx);
        if self.selected == Some(ix) {
            self.list_focus.focus(window, cx);
        }
    }

    /// Close the action list.
    pub fn deselect(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        cx.notify();
    }

    /// Close the action list and return keyboard focus to the button's card.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.selected.and_then(|ix| self.card_focus.get(ix)) {
            handle.focus(window, cx);
        }
        self.deselect(cx);
    }

    /// Put the keyboard on action `at` of the open list and bring it into view.
    fn highlight(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(stored) = self.selected.and_then(|ix| self.stored.get(ix)) else {
            return;
        };
        let rows = list_rows(*stored);
        let count = rows.iter().filter(|row| matches!(row, ListRow::Choice(..))).count();
        self.highlighted = at.min(count.saturating_sub(1));
        // The first action is shown together with its group title.
        let row = match self.highlighted {
            0 => 0,
            at => rows.iter().position(|row| matches!(row, ListRow::Choice(ix, _) if *ix == at)).unwrap_or(0),
        };
        self.list_scroll.scroll_to_item(row);
        cx.notify();
    }

    fn on_close(&mut self, _: &CloseActions, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_some() {
            self.close(window, cx);
        } else {
            cx.propagate();
        }
    }

    fn on_next(&mut self, _: &NextAction, _: &mut Window, cx: &mut Context<Self>) {
        self.highlight(self.highlighted + 1, cx);
    }

    fn on_previous(&mut self, _: &PreviousAction, _: &mut Window, cx: &mut Context<Self>) {
        self.highlight(self.highlighted.saturating_sub(1), cx);
    }

    fn on_first(&mut self, _: &FirstAction, _: &mut Window, cx: &mut Context<Self>) {
        self.highlight(0, cx);
    }

    fn on_last(&mut self, _: &LastAction, _: &mut Window, cx: &mut Context<Self>) {
        self.highlight(usize::MAX, cx);
    }

    fn on_choose(&mut self, _: &ChooseAction, _: &mut Window, cx: &mut Context<Self>) {
        if let (Some(ix), Some(binding)) = (self.selected, self.highlighted_action()) {
            self.assign(ix, binding, cx);
        }
    }

    /// Assign `binding` to button `ix` as an unsaved change.
    pub fn assign(&mut self, ix: usize, binding: ButtonBinding, cx: &mut Context<Self>) {
        if self.readonly.is_some() || self.saving {
            return;
        }
        if let Some(slot) = self.current.get_mut(ix) {
            *slot = binding;
            cx.notify();
        }
    }

    /// Drop every unsaved change.
    pub fn discard(&mut self, cx: &mut Context<Self>) {
        if !self.saving {
            self.current = self.stored.clone();
            cx.notify();
        }
    }

    /// Ask the owner to write the unsaved changes.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let edits = self.pending_edits();
        if edits.is_empty() || self.saving || self.readonly.is_some() {
            return;
        }
        self.saving = true;
        cx.emit(ButtonMapEvent::Save(edits));
        cx.notify();
    }

    /// The save didn't happen; keep the changes so it can be tried again.
    pub fn save_failed(&mut self, cx: &mut Context<Self>) {
        self.saving = false;
        cx.notify();
    }

    // ---- rendering ----

    fn action_text(&self, ix: usize) -> SharedString {
        self.current.get(ix).map(|b| present::action_label(&b.label())).unwrap_or_default().into()
    }

    fn render_stage(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let palette = Palette {
            body: theme.muted,
            shell: theme.secondary,
            recess: theme.background,
            key: theme.accent,
            outline: theme.muted_foreground.opacity(0.55),
            seam: theme.border,
            accent: theme.primary,
            selected_fill: theme.primary.opacity(0.25),
            changed: theme.warning,
        };
        let selected = self.selected;
        let changed: Vec<bool> = (0..self.current.len()).map(|ix| self.is_changed(ix)).collect();
        let count = self.buttons.len().min(ANCHORS.len());

        let drawing = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| paint_mouse(bounds, &palette, selected, &changed, count, window),
        )
        .absolute()
        .size_full();

        let mut stage = div().relative().flex_none().w(units(VIEW_W)).h(units(VIEW_H)).child(drawing);

        for (ix, (anchor, info)) in ANCHORS.iter().zip(&self.buttons).enumerate() {
            let is_selected = selected == Some(ix);
            let is_changed = self.is_changed(ix);
            let is_focused = self.is_button_focused(ix, window);
            let theme = cx.theme();

            let card = h_flex()
                .id(("button-card", ix))
                .track_focus(&self.card_focus[ix])
                .focus_visible(|s| s.border_color(theme.ring).bg(theme.list_hover))
                .absolute()
                .left(units(anchor.card_x()))
                .top(units(anchor.card_y - mouse_art::CARD_H / 2.))
                .w(units(mouse_art::CARD_W))
                .h(units(mouse_art::CARD_H))
                .px_2p5()
                .gap_2()
                .rounded(theme.radius)
                .border_1()
                .cursor_pointer()
                .map(|this| {
                    if is_selected {
                        this.border_color(theme.primary).bg(theme.list_active)
                    } else {
                        this.border_color(theme.border).bg(theme.popover).hover(|s| s.bg(theme.list_hover))
                    }
                })
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().font_semibold().truncate().child(self.action_text(ix)))
                        .child(
                            div().text_xs().text_color(theme.muted_foreground).truncate().child(info.caption.clone()),
                        ),
                )
                .when(is_changed, |this| this.child(div().flex_none().size_2().rounded_full().bg(theme.warning)))
                .on_click(cx.listener(move |this, _, window, cx| this.open(ix, window, cx)));

            let marker = h_flex()
                .id(("button-marker", ix))
                .absolute()
                .left(units(anchor.marker.0 - mouse_art::MARKER / 2.))
                .top(units(anchor.marker.1 - mouse_art::MARKER / 2.))
                .size(units(mouse_art::MARKER))
                .justify_center()
                .rounded_full()
                .border_1()
                .cursor_pointer()
                .text_size(rems(0.5625))
                .font_bold()
                .map(|this| {
                    if is_selected {
                        this.bg(theme.primary).border_color(theme.primary).text_color(theme.primary_foreground)
                    } else {
                        this.bg(theme.popover)
                            .border_color(if is_focused {
                                theme.ring
                            } else if is_changed {
                                theme.warning
                            } else {
                                theme.muted_foreground
                            })
                            .hover(|s| s.border_color(theme.primary))
                    }
                })
                .child(info.marker.clone())
                .on_click(cx.listener(move |this, _, window, cx| this.open(ix, window, cx)));

            stage = stage.child(card).child(marker);
        }
        stage
    }

    fn render_inspector(&self, ix: usize, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let info = &self.buttons[ix];
        let current = self.current[ix];
        let stored = self.stored[ix];
        let readonly = self.readonly.clone();
        let locked = readonly.is_some() || self.saving;
        let list_focused = self.is_action_list_focused(window);
        let rows: Vec<AnyElement> = list_rows(stored)
            .into_iter()
            .map(|row| match row {
                ListRow::Title(title) => self.render_title(title, cx),
                ListRow::Choice(at, choice) => {
                    let on_keyboard = list_focused && at == self.highlighted;
                    self.render_choice(ix, at, choice, current, locked, on_keyboard, cx)
                }
            })
            .collect();
        let theme = cx.theme();

        v_flex()
            .id("button-inspector")
            .flex_none()
            .w(rems(17.))
            .h_full()
            .border_l_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .flex_none()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .child(
                        h_flex()
                            .flex_none()
                            .size_9()
                            .justify_center()
                            .rounded_full()
                            .bg(theme.primary)
                            .text_color(theme.primary_foreground)
                            .text_sm()
                            .font_bold()
                            .child(info.marker.clone()),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().font_semibold().truncate().child(info.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(SharedString::from(format!("Does: {}", self.action_text(ix)))),
                            ),
                    )
                    .child(
                        Button::new("close-inspector")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip("Close (Esc)")
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    ),
            )
            .when_some(readonly, |this, reason| {
                this.child(div().px_4().pb_2().text_xs().text_color(theme.muted_foreground).child(reason))
            })
            .child(
                v_flex()
                    .id("button-actions")
                    .key_context(ACTIONS_CONTEXT)
                    .track_focus(&self.list_focus)
                    .track_scroll(&self.list_scroll)
                    .on_action(cx.listener(Self::on_next))
                    .on_action(cx.listener(Self::on_previous))
                    .on_action(cx.listener(Self::on_first))
                    .on_action(cx.listener(Self::on_last))
                    .on_action(cx.listener(Self::on_choose))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .pb_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .children(rows),
            )
            .when(current != stored && !locked, |this| {
                this.child(
                    h_flex().flex_none().justify_end().px_4().py_2().border_t_1().border_color(theme.border).child(
                        Button::new("revert-button")
                            .outline()
                            .small()
                            .label("Undo change")
                            .on_click(cx.listener(move |this, _, _, cx| this.assign(ix, stored, cx))),
                    ),
                )
            })
    }

    fn render_title(&self, title: &'static str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_none()
            .px_2()
            .pt_3()
            .pb_1()
            .text_xs()
            .font_semibold()
            .text_color(cx.theme().muted_foreground)
            .child(title)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_choice(
        &self,
        ix: usize,
        at: usize,
        choice: ActionChoice,
        current: ButtonBinding,
        locked: bool,
        on_keyboard: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let is_current = choice.binding == current;
        let binding = choice.binding;
        h_flex()
            .id(("button-action", at))
            .flex_none()
            .h_8()
            .px_2()
            .gap_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(if on_keyboard { theme.ring } else { theme.ring.opacity(0.) })
            .text_sm()
            .map(|this| {
                if is_current {
                    this.bg(theme.list_active).font_semibold()
                } else if locked {
                    this.text_color(theme.muted_foreground)
                } else {
                    this.cursor_pointer()
                        .when(on_keyboard, |s| s.bg(theme.list_hover))
                        .hover(|s| s.bg(theme.list_hover))
                }
            })
            .child(div().flex_1().min_w_0().truncate().child(SharedString::from(choice.label)))
            .when(is_current, |this| this.child(Icon::new(IconName::Check).small().text_color(theme.primary)))
            .when(!locked, |this| {
                this.on_click(cx.listener(move |this, _, _, cx| {
                    this.highlighted = at;
                    this.assign(ix, binding, cx);
                }))
            })
            .into_any_element()
    }
}

impl Render for ButtonMap {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let stage = self.render_stage(window, cx).into_any_element();
        let inspector = self.selected.map(|ix| self.render_inspector(ix, window, cx).into_any_element());
        let theme = cx.theme();
        let changes = self.pending_edits().len();
        let hint: SharedString = match (&self.readonly, self.selected) {
            (Some(reason), _) => reason.clone(),
            (None, Some(_)) => "Choose what this button does. Esc closes the list.".into(),
            (None, None) => "Select a button to change what it does.".into(),
        };

        v_flex()
            .id("button-map")
            .key_context(MAP_CONTEXT)
            .on_action(cx.listener(Self::on_close))
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .child(
                h_flex()
                    .h_11()
                    .px_4()
                    .gap_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_sm().font_semibold().child("Buttons"))
                    .child(div().flex_1().min_w_0().truncate().text_sm().text_color(theme.muted_foreground).child(hint))
                    .when(changes > 0, |this| {
                        this.child(
                            h_flex().gap_2().child(div().size_2().rounded_full().bg(theme.warning)).child(
                                div()
                                    .text_sm()
                                    .text_color(theme.warning)
                                    .child(SharedString::from(edit_form::change_count_label(changes))),
                            ),
                        )
                        .child(
                            Button::new("discard-buttons")
                                .ghost()
                                .small()
                                .label("Discard")
                                .disabled(self.saving)
                                .on_click(cx.listener(|this, _, _, cx| this.discard(cx))),
                        )
                        .child(
                            Button::new("save-buttons")
                                .primary()
                                .small()
                                .label("Save to mouse")
                                .loading(self.saving)
                                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                        )
                    }),
            )
            .child(
                h_flex()
                    .h(rems(23.))
                    .items_stretch()
                    .child(
                        h_flex()
                            .id("button-stage")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_x_scroll()
                            .child(div().flex_1())
                            .child(stage)
                            .child(div().flex_1()),
                    )
                    .when_some(inspector, |this, inspector| this.child(inspector)),
            )
    }
}

/// A length in the drawing's units, scaled with the font size.
fn units(view_units: f32) -> gpui_kit::Rems {
    rems(view_units / 16.)
}

/// Colours of the drawing, taken from the theme.
#[derive(Clone, Copy)]
struct Palette {
    body: Hsla,
    shell: Hsla,
    recess: Hsla,
    key: Hsla,
    outline: Hsla,
    seam: Hsla,
    accent: Hsla,
    selected_fill: Hsla,
    changed: Hsla,
}

fn paint_mouse(
    bounds: Bounds<Pixels>,
    palette: &Palette,
    selected: Option<usize>,
    changed: &[bool],
    count: usize,
    window: &mut Window,
) {
    let scale = bounds.size.width / px(VIEW_W);
    let at = |x: f32, y: f32| point(bounds.origin.x + px(x * scale), bounds.origin.y + px(y * scale));
    let build = |segments: &[Segment], mut builder: PathBuilder| {
        for segment in segments {
            match *segment {
                Segment::Move(x, y) => builder.move_to(at(x, y)),
                Segment::Line(x, y) => builder.line_to(at(x, y)),
                Segment::Cubic(ax, ay, bx, by, x, y) => builder.cubic_bezier_to(at(x, y), at(ax, ay), at(bx, by)),
                Segment::Close => builder.close(),
            }
        }
        builder.build().ok()
    };

    for shape in SHAPES {
        let Some(segments) = mouse_art::parse_path(shape.d) else {
            continue;
        };
        let is_selected = shape.button.is_some() && shape.button == selected;
        let is_changed = shape.button.is_some_and(|ix| changed.get(ix).copied().unwrap_or(false));

        let fill = match shape.fill {
            Fill::None => None,
            Fill::Body => Some(palette.body),
            Fill::Shell => Some(palette.shell),
            Fill::Recess => Some(palette.recess),
            Fill::Key => Some(if is_selected { palette.selected_fill } else { palette.key }),
        };
        if let Some(color) = fill
            && let Some(path) = build(&segments, PathBuilder::fill())
        {
            window.paint_path(path, color);
        }

        let (stroke, width) = if is_selected {
            (palette.accent, shape.width.max(2.))
        } else if is_changed {
            (palette.changed, shape.width.max(1.5))
        } else {
            let color = match shape.stroke {
                Stroke::Outline => palette.outline,
                Stroke::Seam => palette.seam,
                Stroke::Accent => palette.accent,
            };
            (color, shape.width)
        };
        if let Some(path) = build(&segments, PathBuilder::stroke(px(width * scale))) {
            window.paint_path(path, stroke);
        }
    }

    for (ix, anchor) in ANCHORS.iter().enumerate().take(count) {
        let (x0, y0) = anchor.line_start();
        // Stop at the marker's edge so the line doesn't show through it.
        let (dx, dy) = (anchor.marker.0 - x0, anchor.marker.1 - y0);
        let length = (dx * dx + dy * dy).sqrt().max(1.);
        let reach = (length - mouse_art::MARKER / 2.) / length;
        let segments = [Segment::Move(x0, y0), Segment::Line(x0 + dx * reach, y0 + dy * reach)];
        let is_selected = selected == Some(ix);
        let (color, width) = if is_selected { (palette.accent, 2.) } else { (palette.outline, 1.) };
        if let Some(path) = build(&segments, PathBuilder::stroke(px(width * scale))) {
            window.paint_path(path, color);
        }
    }
}
