//! `g502-config`: a readonly desktop viewer for Logitech G502 mice.
//!
//! The library holds the window and its state so integration tests can drive
//! the production view; `main.rs` only parses arguments and opens the window.

mod mice_window;
pub mod present;
mod profile_detail;
pub mod state;
pub mod theme;

pub use mice_window::{
    KEY_CONTEXT, MiceWindow, Quit, Refresh, SelectNextDevice, SelectNextProfile, SelectPreviousDevice,
    SelectPreviousProfile, ShowDeviceInfo, ToggleTheme, init,
};
pub use state::Source;
