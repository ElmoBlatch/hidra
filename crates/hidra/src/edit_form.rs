//! The profile editor's form logic, free of GPUI: the list of assignable
//! actions, and turning form values into field edits.

use lg_profiles::{ButtonBinding, DPI_SLOTS, ProfileEdit};
use lg_snapshot::{DpiRange, ProfileContent};

use crate::present;

/// One entry of a button's action list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionChoice {
    pub label: String,
    pub binding: ButtonBinding,
}

/// Actions offered for every button, in menu order.
fn standard_actions() -> Vec<ActionChoice> {
    let mut texts = vec![
        "left",
        "right",
        "middle",
        "back",
        "forward",
        "dpi-up",
        "dpi-down",
        "dpi-cycle",
        "dpi-shift",
        "dpi-default",
        "profile-cycle",
        "tilt-left",
        "tilt-right",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    texts.extend((13..=24).map(|n| format!("key:F{n}")));
    texts.push("disabled".to_string());
    texts
        .iter()
        .map(|t| {
            let binding = ButtonBinding::parse(t).expect("standard action parses");
            ActionChoice { label: present::action_label(&binding.label()), binding }
        })
        .collect()
}

/// The action list for one button: the standard actions, preceded by the
/// button's current binding when that is something else (another key, a
/// macro), so it can be kept. Returns the list and the current entry's index.
pub fn action_choices(current: ButtonBinding) -> (Vec<ActionChoice>, usize) {
    let mut choices = standard_actions();
    match choices.iter().position(|c| c.binding == current) {
        Some(ix) => (choices, ix),
        None => {
            choices.insert(0, ActionChoice { label: present::action_label(&current.label()), binding: current });
            (choices, 0)
        }
    }
}

/// What the form currently holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormValues {
    /// DPI text per stage; empty or "0" turns the stage off.
    pub dpi: [String; DPI_SLOTS],
    /// 0-based.
    pub default_stage: usize,
    pub polling_hz: u16,
    pub buttons: Vec<ButtonBinding>,
}

impl FormValues {
    /// The form as it looks before any change.
    pub fn from_profile(c: &ProfileContent) -> Self {
        Self {
            dpi: c.dpi_stages.map(|d| if d == 0 { String::new() } else { d.to_string() }),
            default_stage: c.default_stage as usize,
            polling_hz: c.report_rate_hz,
            buttons: c.bindings.clone(),
        }
    }
}

/// Turn the form into the edits that differ from `original`, or a message
/// naming the first invalid value.
pub fn form_edits(original: &ProfileContent, form: &FormValues, range: Option<&DpiRange>) -> Result<Vec<ProfileEdit>, String> {
    let mut edits = Vec::new();
    let mut stages = [0u16; DPI_SLOTS];
    for (ix, text) in form.dpi.iter().enumerate() {
        let text = text.trim();
        let dpi: u16 = if text.is_empty() {
            0
        } else {
            text.replace(',', "").parse().map_err(|_| format!("Stage {}: \"{text}\" isn't a DPI value.", ix + 1))?
        };
        if dpi != 0 {
            match range {
                Some(DpiRange::Range { min, max, step }) => {
                    if dpi < *min || dpi > *max {
                        return Err(format!(
                            "Stage {}: use a value from {} to {}.",
                            ix + 1,
                            present::grouped((*min).into()),
                            present::grouped((*max).into())
                        ));
                    }
                    if *step > 0 && !(dpi - min).is_multiple_of(*step) {
                        return Err(format!("Stage {}: use a multiple of {step}.", ix + 1));
                    }
                }
                Some(DpiRange::Discrete(values)) if !values.contains(&dpi) => {
                    return Err(format!("Stage {}: this sensor doesn't support {dpi} DPI.", ix + 1));
                }
                _ => {}
            }
        }
        stages[ix] = dpi;
        if dpi != original.dpi_stages[ix] {
            edits.push(ProfileEdit::SetDpi { stage: ix, dpi });
        }
    }
    if stages.get(form.default_stage).is_none_or(|&d| d == 0) {
        return Err("The default stage needs a DPI value.".to_string());
    }
    if form.default_stage != original.default_stage as usize {
        edits.push(ProfileEdit::SetDefaultStage(form.default_stage as u8));
    }
    if form.polling_hz != original.report_rate_hz {
        edits.push(ProfileEdit::SetReportRate(form.polling_hz));
    }
    for (ix, (new, old)) in form.buttons.iter().zip(&original.bindings).enumerate() {
        if new != old {
            edits.push(ProfileEdit::SetButton { index: ix, binding: *new });
        }
    }
    Ok(edits)
}

/// "1 change" / "3 changes".
pub fn change_count_label(n: usize) -> String {
    match n {
        0 => "No changes".to_string(),
        1 => "1 change".to_string(),
        n => format!("{n} changes"),
    }
}
