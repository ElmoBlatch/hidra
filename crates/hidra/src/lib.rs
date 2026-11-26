//! `hidra`: a readonly desktop viewer for Logitech G502 mice.
//!
//! The library holds the window and its state so integration tests can drive
//! the production view; `main.rs` only parses arguments and opens the window.

mod button_map;
pub mod edit_form;
mod mice_window;
pub mod mouse_art;
pub mod present;
mod profile_detail;
mod profile_editor;
pub mod state;
pub mod theme;

pub use button_map::{ButtonMap, ButtonMapEvent};
pub use mice_window::{
    EditProfile, KEY_CONTEXT, MiceWindow, Quit, Refresh, SelectNextDevice, SelectNextProfile,
    SelectPreviousDevice, SelectPreviousProfile, ShowDeviceInfo, ToggleTheme, init,
};
pub use profile_editor::{ProfileEditor, ProfileEditorEvent};
pub use state::Source;
