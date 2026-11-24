//! Read a Logitech G502 into a plain, owned snapshot.
//!
//! This crate is the shared "read everything" layer for the `hidractl` CLI and
//! the `hidra` desktop viewer. It is read-only by construction: it only
//! calls the getter functions that `lg-hidpp` exposes, and `lg-hidpp` has no
//! write path.
//!
//! The entry points are [`discover`] / [`replay_entries`] to find devices and
//! [`read_entry`] to open one and read a [`DeviceSnapshot`]. All of them block
//! (hidraw polls can take up to about a second per request), so a UI must call
//! them off its main thread.

mod snapshot;
mod source;
mod transport;

pub use snapshot::{
    DeviceSnapshot, DpiRange, Firmware, Integrity, LedSnapshot, OnboardMode, OnboardSummary, ProfileContent, ProfileSet,
    ProfileSnapshot, Reading, SensorDpi, read_device,
};
pub use source::{Connection, DeviceEntry, DeviceSource, ReadError, discover, open_entry, read_entry, replay_entries};
pub use transport::AnyTransport;

/// Timeout used for replayed captures: long enough for the replay transport,
/// short enough that requests missing from the capture fail quickly.
pub const REPLAY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100);
