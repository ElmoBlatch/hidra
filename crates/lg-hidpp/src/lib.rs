//! Access to Logitech HID++ 2.0 devices over Linux hidraw.
//!
//! Almost everything here is read-only. Request dispatch ([`Device::call`])
//! is crate-private, so callers cannot send arbitrary functions. The single
//! write path is [`features::onboard_profiles_write::replace_sector`], which
//! is compare-and-swap: it refuses unless the device still holds the bytes
//! the caller expects, verifies the result by reading it back, and puts the
//! original contents back if anything goes wrong.

mod device;
pub mod discovery;
mod error;
pub mod features;
pub mod record;
pub mod replay;
mod report;
pub mod sim;
mod transport;

pub use device::{Device, FeatureEntry, FeatureFlags};
pub use error::{Error, Hidpp10Error, Hidpp20Error, Result};
pub use report::{Incoming, Request, SOFTWARE_ID};
pub use transport::{HidrawTransport, Transport};

/// Device index for a directly connected (wired, or DJ child hidraw) device.
pub const DEVICE_INDEX_DIRECT: u8 = 0xff;

/// Logitech USB vendor ID.
pub const LOGITECH_VID: u16 = 0x046d;
