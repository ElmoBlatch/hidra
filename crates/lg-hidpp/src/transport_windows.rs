//! The Windows transport.
//!
//! Windows gives each top-level collection of a HID interface its own device
//! path, so HID++ short reports (ID 0x10) and long reports (ID 0x11) travel on
//! different handles. A report is written to the handle whose collection
//! declares its ID, and replies are read from both.
//!
//! Not yet run on hardware: see RESEARCH.md, "Windows".

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hidapi::HidDevice;

use super::Transport;
use crate::discovery::windows::{self as win, Collection};

/// How long one handle is polled before the other gets its turn.
const SLICE_MS: i32 = 2;

pub struct HidTransport {
    path: PathBuf,
    short: Option<HidDevice>,
    long: Option<HidDevice>,
    /// True when some open collection declares the short report.
    declares_short: bool,
    /// Which handle is polled first on the next read.
    long_first: bool,
}

impl HidTransport {
    /// Open the HID++ collections of the device that `path` belongs to.
    /// `path` is any one of its collection paths, as discovery reports it.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let key = win::group_key(&path.to_string_lossy());
        let api = win::api()?;
        let mut short = None;
        let mut long = None;
        let mut declares_short = false;
        for Collection { path: collection, key: its_key, short: is_short, long: is_long, .. } in win::collections(&api) {
            if its_key != key {
                continue;
            }
            let device = api.open_path(&collection).map_err(win::io_error)?;
            declares_short |= is_short;
            // One collection can declare both reports; then one handle serves both.
            match (is_short, is_long) {
                (true, true) | (false, true) if long.is_none() => long = Some(device),
                (true, false) if short.is_none() => short = Some(device),
                _ => {}
            }
        }
        if short.is_none() && long.is_none() {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("no HID++ collection at {}", path.display())));
        }
        Ok(Self { path, short, long, declares_short, long_first: false })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Transport for HidTransport {
    fn write_report(&mut self, report: &[u8]) -> io::Result<()> {
        let device = match report.first() {
            Some(0x10) => self.short.as_ref().or(self.long.as_ref()),
            _ => self.long.as_ref().or(self.short.as_ref()),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no HID++ collection open"))?;
        let written = device.write(report).map_err(win::io_error)?;
        if written < report.len() {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "short HID write"));
        }
        Ok(())
    }

    fn read_report(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 64];
        loop {
            let order = if self.long_first { [&self.long, &self.short] } else { [&self.short, &self.long] };
            for device in order.into_iter().flatten() {
                let n = device.read_timeout(&mut buf, SLICE_MS).map_err(win::io_error)?;
                if n > 0 {
                    self.long_first = !self.long_first;
                    return Ok(Some(buf[..n].to_vec()));
                }
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }

    fn supports_short(&self) -> bool {
        self.declares_short
    }

    fn supports_long(&self) -> bool {
        self.long.is_some()
    }
}
