use std::io;
use std::time::Duration;

/// A byte-level HID report channel.
pub trait Transport {
    /// Send one output report (report ID in byte 0).
    fn write_report(&mut self, report: &[u8]) -> io::Result<()>;
    /// Wait up to `timeout` for one input report. `Ok(None)` on timeout.
    fn read_report(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>>;
    /// Whether the device's report descriptor declares short / long HID++ reports.
    fn supports_short(&self) -> bool;
    fn supports_long(&self) -> bool;
}

#[cfg(target_os = "linux")]
#[path = "transport_linux.rs"]
mod imp;
#[cfg(windows)]
#[path = "transport_windows.rs"]
mod imp;
#[cfg(not(any(target_os = "linux", windows)))]
#[path = "transport_unsupported.rs"]
mod imp;

pub use imp::HidTransport;
