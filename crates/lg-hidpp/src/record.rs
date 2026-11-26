//! A [`Transport`] wrapper that records traffic in the same text format as
//! `ratbagd --verbose=raw`, so recordings can be replayed by
//! [`crate::replay`] and become test fixtures.
//!
//! Redaction happens at record time: replies to a configured feature function
//! have chosen payload bytes zeroed before they are logged (for example the
//! unit ID returned by feature 0x0003 function 0).

use std::cell::RefCell;
use std::io;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use crate::report::{Incoming, LONG_REPORT_ID, SHORT_REPORT_ID};
use crate::transport::Transport;

#[derive(Debug, Clone)]
struct Redaction {
    feature_index: u8,
    function: u8,
    /// Byte range within the reply payload (payload byte 0 = report byte 4).
    payload: Range<usize>,
}

/// Shared recording state. Keep a clone to add redactions and read the log
/// while the transport itself is owned by a `Device`.
#[derive(Debug, Default)]
pub struct Recorder {
    lines: Vec<String>,
    redactions: Vec<Redaction>,
}

impl Recorder {
    pub fn new_shared() -> Rc<RefCell<Recorder>> {
        Rc::new(RefCell::new(Recorder::default()))
    }

    /// Write a section header that [`crate::replay::parse_log`] uses to split devices.
    pub fn begin_device(&mut self, name: &str, hidraw: &str) {
        self.lines.push(format!("ratbag debug: New device: {name}"));
        self.lines
            .push(format!("ratbag debug: {name} is device '{hidraw}'."));
    }

    pub fn note(&mut self, text: &str) {
        self.lines.push(format!("note: {text}"));
    }

    /// Zero `payload` bytes of every recorded reply to `feature_index`/`function`.
    pub fn redact(&mut self, feature_index: u8, function: u8, payload: Range<usize>) {
        self.redactions.push(Redaction {
            feature_index,
            function,
            payload,
        });
    }

    pub fn text(&self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        s
    }

    fn record(&mut self, dir: &str, bytes: &[u8]) {
        let mut bytes = bytes.to_vec();
        if dir == "read"
            && let Incoming::Message {
                feature_index,
                function,
                ..
            } = Incoming::parse(&bytes)
            && matches!(bytes[0], SHORT_REPORT_ID | LONG_REPORT_ID)
        {
            for r in self
                .redactions
                .iter()
                .filter(|r| r.feature_index == feature_index && r.function == function)
            {
                for i in r.payload.clone() {
                    if let Some(b) = bytes.get_mut(4 + i) {
                        *b = 0;
                    }
                }
            }
        }
        let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let pad = if dir == "read" { "   " } else { "  " };
        self.lines
            .push(format!("ratbag raw: hidpp {dir}:{pad}{}", hex.join(" ")));
    }
}

pub struct RecordingTransport<T: Transport> {
    inner: T,
    recorder: Rc<RefCell<Recorder>>,
}

impl<T: Transport> RecordingTransport<T> {
    pub fn new(inner: T, recorder: Rc<RefCell<Recorder>>) -> Self {
        Self { inner, recorder }
    }
}

impl<T: Transport> Transport for RecordingTransport<T> {
    fn write_report(&mut self, report: &[u8]) -> io::Result<()> {
        self.recorder.borrow_mut().record("write", report);
        self.inner.write_report(report)
    }

    fn read_report(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let r = self.inner.read_report(timeout)?;
        if let Some(bytes) = &r {
            self.recorder.borrow_mut().record("read", bytes);
        }
        Ok(r)
    }

    fn supports_short(&self) -> bool {
        self.inner.supports_short()
    }

    fn supports_long(&self) -> bool {
        self.inner.supports_long()
    }
}
