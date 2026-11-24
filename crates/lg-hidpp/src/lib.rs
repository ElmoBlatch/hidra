//! Read-only access to Logitech HID++ 2.0 devices over Linux hidraw.
//!
//! This crate deliberately exposes **no write path**. Every public feature
//! function issues a getter. Request dispatch ([`Device::call`]) is
//! crate-private so that callers cannot send arbitrary functions such as
//! onboard-profile `BeginWrite`/`WriteData`. Writes will be added later as a
//! separate, explicitly gated API once read-back verification exists.

pub mod discovery;
mod device;
mod error;
pub mod features;
pub mod record;
pub mod replay;
mod report;
mod transport;

pub use device::{Device, FeatureEntry, FeatureFlags};
pub use error::{Error, Hidpp10Error, Hidpp20Error, Result};
pub use report::{Incoming, Request, SOFTWARE_ID};
pub use transport::{HidrawTransport, Transport};

/// Device index for a directly connected (wired, or DJ child hidraw) device.
pub const DEVICE_INDEX_DIRECT: u8 = 0xff;

/// Logitech USB vendor ID.
pub const LOGITECH_VID: u16 = 0x046d;
