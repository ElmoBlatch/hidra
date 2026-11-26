//! The application theme: "Graphite", dark by default with a light variant.
//!
//! Colors live in `themes/graphite.json`, the one place raw color values are
//! allowed. Views read semantic tokens from `cx.theme()`.

use gpui_kit::App;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode, ThemeRegistry};

const GRAPHITE: &str = include_str!("../themes/graphite.json");
const DARK: &str = "Graphite Dark";
const LIGHT: &str = "Graphite Light";

/// Register the Graphite themes and switch to the dark one. Call once after
/// `gpui_kit::init`.
pub fn init(cx: &mut App) {
    if let Err(e) = ThemeRegistry::global_mut(cx).load_themes_from_str(GRAPHITE) {
        // The file is compiled in, so this is a programming error; keep the
        // default theme rather than refusing to start.
        eprintln!("hidra: couldn't load the Graphite theme: {e}");
        return;
    }
    let themes = ThemeRegistry::global(cx).themes();
    let (Some(dark), Some(light)) = (themes.get(DARK).cloned(), themes.get(LIGHT).cloned()) else {
        return;
    };
    Theme::update(cx, |theme| {
        theme.light_theme = light;
        theme.apply_config(&dark);
    });
}

/// Switch between the dark and light Graphite themes.
pub fn toggle(cx: &mut App) {
    let next = if cx.theme().is_dark() { ThemeMode::Light } else { ThemeMode::Dark };
    Theme::change(next, None, cx);
}
