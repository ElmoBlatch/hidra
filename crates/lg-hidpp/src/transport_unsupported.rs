//! Placeholder for platforms without a HID backend: opening always fails.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::Transport;

pub struct HidTransport {
    path: PathBuf,
}

impl HidTransport {
    pub fn open(_path: impl AsRef<Path>) -> io::Result<Self> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "hidra talks to devices on Linux and Windows only"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Transport for HidTransport {
    fn write_report(&mut self, _: &[u8]) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn read_report(&mut self, _: Duration) -> io::Result<Option<Vec<u8>>> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn supports_short(&self) -> bool {
        false
    }

    fn supports_long(&self) -> bool {
        false
    }
}
