//! The body of one profile tab: DPI stages, buttons and lighting.
//!
//! Readonly. Rows are laid out on fixed lanes (identity, value, marker) so the
//! same geometry can carry edit controls later.

use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, StyledExt as _,
    alert::Alert,
    group_box::{GroupBox, GroupBoxVariants as _},
    h_flex,
    tag::Tag,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window, div, relative, rems, rgb,
};
use lg_snapshot::{Integrity, LedSnapshot, ProfileContent, ProfileSnapshot};

use crate::present::{self, ButtonRow, StageRow};

/// Readonly presentation of one decoded profile.
#[derive(IntoElement)]
pub struct ProfileDetail {
    profile: ProfileSnapshot,
    g502_layout: bool,
    /// The sensor's live DPI, when this profile is the active one.
    live_dpi: Option<u16>,
}

impl ProfileDetail {
    pub fn new(profile: ProfileSnapshot) -> Self {
        Self { profile, g502_layout: false, live_dpi: None }
    }

    /// Name buttons with the G502 family's key labels.
    pub fn g502_layout(mut self, g502_layout: bool) -> Self {
        self.g502_layout = g502_layout;
        self
    }

    /// Mark the stage the sensor is currently running at.
    pub fn live_dpi(mut self, live_dpi: Option<u16>) -> Self {
        self.live_dpi = live_dpi;
        self
    }
}

impl RenderOnce for ProfileDetail {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = self.profile;
        let content = match &p.content {
            Ok(c) => c,
            Err(e) => {
                return Alert::error(
                    ("profile-error", p.address as u64),
                    format!("Couldn't read sector {:#06x}: {e}", p.address),
                )
                .title(format!("{} is unreadable", p.title))
                .into_any_element();
            }
        };

        let name: SharedString = match (present::profile_name(content), p.factory) {
            (Some(n), _) => n.to_string().into(),
            (None, true) => p.title.clone().into(),
            (None, false) => "Unnamed profile".into(),
        };
        let muted = cx.theme().muted_foreground;
        let live_dpi = self.live_dpi.filter(|_| p.active);

        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .items_baseline()
                    .gap_3()
                    .child(div().text_lg().font_semibold().child(name))
                    .child(div().text_sm().text_color(muted).child(present::profile_meta(&p, content))),
            )
            .when(matches!(content.integrity, Integrity::CrcMismatch { .. }), |this| {
                this.child(
                    Alert::warning(
                        ("crc-mismatch", p.address as u64),
                        "The stored checksum doesn't match the sector contents. The mouse may ignore this profile.",
                    )
                    .title("Checksum mismatch"),
                )
            })
            .child(
                h_flex()
                    .flex_wrap()
                    .items_start()
                    .gap_4()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(rems(20.))
                            .gap_4()
                            .child(panel("DPI stages", dpi_stages(content, live_dpi, cx)))
                            .child(panel("Lighting", lighting(&content.leds, cx))),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(rems(20.))
                            .child(panel("Buttons", buttons(content, self.g502_layout, cx))),
                    ),
            )
            .into_any_element()
    }
}

/// A titled panel on the raised surface.
fn panel(title: &'static str, body: impl IntoElement) -> impl IntoElement {
    GroupBox::new().fill().title(title).child(body)
}

/// One row of a panel: content on shared lanes, separated by a hairline.
fn row(first: bool, cx: &App) -> gpui_kit::Div {
    h_flex().h_9().gap_3().when(!first, |this| this.border_t_1().border_color(cx.theme().border))
}

fn dpi_stages(c: &ProfileContent, live_dpi: Option<u16>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (muted, track, accent, neutral) = (theme.muted_foreground, theme.muted, theme.chart_1, theme.chart_2);
    v_flex().children(present::stage_rows(c).into_iter().enumerate().map(|(ix, stage)| {
        let StageRow { number, dpi, is_default, is_shift } = stage;
        let in_use = dpi.is_some() && dpi == live_dpi;
        row(ix == 0, cx)
            // Lane 1: stage number.
            .child(div().w_4().flex_shrink_0().text_sm().text_color(muted).child(number.to_string()))
            // Lane 2: bar, scaled to the profile's highest stage.
            .child(div().flex_1().min_w_0().h_1p5().rounded_full().bg(track).when_some(dpi, |this, dpi| {
                this.child(
                    div()
                        .h_full()
                        .rounded_full()
                        .w(relative(present::stage_fraction(dpi, c).max(0.02)))
                        .bg(if in_use { accent } else { neutral }),
                )
            }))
            // Lane 3: value, right-aligned so the numbers compare.
            .child(div().w_16().flex_shrink_0().text_right().text_sm().map(|this| match dpi {
                Some(dpi) => this.font_medium().child(present::grouped(dpi.into())),
                None => this.text_color(muted).child("Unused"),
            }))
            // Lane 4: markers. Fixed width so rows without one stay aligned.
            .child(
                h_flex()
                    .w(rems(9.))
                    .flex_shrink_0()
                    .gap_1()
                    .when(in_use, |this| this.child(Tag::primary().small().child("In use")))
                    .when(is_default, |this| this.child(Tag::secondary().small().child("Default")))
                    .when(is_shift, |this| this.child(Tag::secondary().small().child("Shift"))),
            )
    }))
}

fn buttons(c: &ProfileContent, g502_layout: bool, cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    v_flex().children(present::button_rows(c, g502_layout).into_iter().enumerate().map(|(ix, button)| {
        let ButtonRow { key, name, action, remapped } = button;
        row(ix == 0, cx)
            // Lane 1: key label. The slot stays even when a button has none.
            .child(
                h_flex()
                    .w_10()
                    .flex_shrink_0()
                    .when_some(key, |this, key| this.child(Tag::secondary().small().outline().child(key))),
            )
            // Lane 2: the physical button.
            .child(div().w(rems(9.)).flex_shrink_0().text_sm().truncate().child(name))
            // Lane 3: what it does. Factory assignments stay quiet so a
            // remapped button is the one that reads.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .truncate()
                    .map(|this| if remapped { this.font_semibold() } else { this.text_color(muted) })
                    .child(action),
            )
            // Trailing marker, so the distinction does not rest on weight
            // alone. Nothing follows it, so it needs no reserved lane.
            .when(remapped, |this| this.child(div().flex_shrink_0().child(Tag::primary().small().child("Remapped"))))
    }))
}

fn lighting(leds: &[LedSnapshot], cx: &App) -> impl IntoElement {
    let (muted, border) = (cx.theme().muted_foreground, cx.theme().border);
    v_flex().children(leds.iter().enumerate().map(|(ix, led)| {
        row(ix == 0, cx)
            .child(div().w_10().flex_shrink_0().text_sm().text_color(muted).child(led.zone.clone()))
            // Swatch slot: fixed, so effects without a colour keep the text lane.
            .child(div().size_4().flex_shrink_0().when_some(led.rgb, |this, [r, g, b]| {
                // The colour stored on the device: data, not a theme colour.
                let colour = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
                this.rounded_sm().border_1().border_color(border).bg(rgb(colour))
            }))
            .child(
                div().flex_1().min_w_0().text_sm().font_medium().truncate().child(present::sentence_case(&led.label)),
            )
    }))
}
