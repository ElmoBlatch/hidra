use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

/// A Linux `/dev/hidrawN` node.
pub struct HidrawTransport {
    file: File,
    path: PathBuf,
    short: bool,
    long: bool,
}

impl HidrawTransport {
    /// Open a hidraw node. Report support is read from its sysfs report
    /// descriptor.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)?;
        let (short, long) =
            crate::discovery::hidpp_report_support_for_dev(&path).unwrap_or((true, true));
        Ok(Self {
            file,
            path,
            short,
            long,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Transport for HidrawTransport {
    fn write_report(&mut self, report: &[u8]) -> io::Result<()> {
        let n = self.file.write(report)?;
        if n != report.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "short hidraw write",
            ));
        }
        Ok(())
    }

    fn read_report(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let mut pfd = libc::pollfd {
                fd: self.file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ms = remaining.as_millis().min(i32::MAX as u128) as i32;
            // SAFETY: pfd is a valid pollfd for the lifetime of the call.
            let rc = unsafe { libc::poll(&mut pfd, 1, ms) };
            if rc < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            if rc == 0 {
                return Ok(None);
            }
            if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "hidraw device went away",
                ));
            }
            let mut buf = [0u8; 64];
            match self.file.read(&mut buf) {
                Ok(n) => return Ok(Some(buf[..n].to_vec())),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Ok(None);
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn supports_short(&self) -> bool {
        self.short
    }

    fn supports_long(&self) -> bool {
        self.long
    }
}
