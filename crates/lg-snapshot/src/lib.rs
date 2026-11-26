//! Shared device operations for the `hidractl` CLI and the `hidra` viewer:
//! find devices, read one into a plain [`DeviceSnapshot`], back up its onboard
//! memory, and apply guarded profile edits.
//!
//! Reading: [`discover`] / [`replay_entries`], then [`read_entry`].
//! Writing: [`plan_edit`] builds a byte-level change from field edits and
//! checks it; [`commit_edit`] backs the device up and writes through
//! `lg_hidpp`'s compare-and-swap `replace_sector`. [`edit_entry`] does both
//! for a discovered device.
//!
//! Everything blocks (hidraw polls can take up to about a second per
//! request), so a UI must call these off its main thread.

mod backup;
mod edit;
mod init;
mod snapshot;
mod source;
mod transport;

pub use backup::save_backup;
pub use edit::{EditError, EditPlan, WRITE_VERIFIED_PIDS, commit_edit, edit_entry, plan_edit};
pub use init::{InitOutcome, InitPlan, commit_initialize, plan_initialize, restore_from_backup};
pub use snapshot::{
    DeviceSnapshot, DpiRange, Firmware, Integrity, LedSnapshot, OnboardMode, OnboardSummary,
    ProfileContent, ProfileSet, ProfileSnapshot, Reading, SensorDpi, read_device,
};
pub use source::{
    Connection, DeviceEntry, DeviceSource, ReadError, discover, open_entry, read_entry,
    replay_entries, simulated_entry, simulated_entry_from_log,
};
pub use transport::AnyTransport;

/// Timeout used for replayed captures: long enough for the replay transport,
/// short enough that requests missing from the capture fail quickly.
pub const REPLAY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100);
