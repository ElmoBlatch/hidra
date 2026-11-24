//! A [`Transport`] that answers requests from a recorded `ratbagd --verbose=raw`
//! log. It lets the protocol and profile layers be exercised against real
//! device traffic without touching hardware.
//!
//! Requests are matched on feature index, function and parameters; report
//! length, device index and software ID are ignored, and the software ID in
//! replayed replies is rewritten to ours. A request that does not appear in the
//! log times out, exactly like a device that never answers.

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use crate::report::{LONG_REPORT_ID, SHORT_REPORT_ID, SOFTWARE_ID};
use crate::transport::Transport;

#[derive(Debug, Clone)]
struct Exchange {
    key: Key,
    replies: Vec<Vec<u8>>,
    used: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    feature_index: u8,
    function: u8,
    params: [u8; 16],
}

impl Key {
    fn from_request(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 4 || !matches!(bytes[0], SHORT_REPORT_ID | LONG_REPORT_ID) {
            return None;
        }
        let mut params = [0u8; 16];
        let p = &bytes[4..];
        params[..p.len().min(16)].copy_from_slice(&p[..p.len().min(16)]);
        Some(Self { feature_index: bytes[2], function: bytes[3] >> 4, params })
    }
}

/// One device's traffic from a log.
#[derive(Debug, Clone)]
pub struct ReplaySegment {
    pub name: String,
    pub hidraw: Option<String>,
    exchanges: Vec<Exchange>,
}

impl ReplaySegment {
    pub fn exchange_count(&self) -> usize {
        self.exchanges.len()
    }
}

/// Split a ratbagd raw log into per-device segments that contain HID++ traffic.
pub fn parse_log(text: &str) -> Vec<ReplaySegment> {
    let mut segments: Vec<ReplaySegment> = Vec::new();
    let mut current: Option<ReplaySegment> = None;
    for line in text.lines() {
        if let Some(rest) = line.split("New device: ").nth(1) {
            if let Some(seg) = current.take().filter(|s| !s.exchanges.is_empty()) {
                segments.push(seg);
            }
            current = Some(ReplaySegment { name: rest.trim().to_string(), hidraw: None, exchanges: Vec::new() });
            continue;
        }
        let Some(seg) = current.as_mut() else { continue };
        if let Some(rest) = line.split("is device '").nth(1) {
            if seg.hidraw.is_none() {
                seg.hidraw = rest.split('\'').next().map(str::to_string);
            }
            continue;
        }
        let (is_write, hex) = if let Some(h) = line.split("hidpp write:").nth(1) {
            (true, h)
        } else if let Some(h) = line.split("hidpp read:").nth(1) {
            (false, h)
        } else {
            continue;
        };
        let Ok(bytes) = hex.split_whitespace().map(|b| u8::from_str_radix(b, 16)).collect::<Result<Vec<u8>, _>>()
        else {
            continue;
        };
        if is_write {
            if let Some(key) = Key::from_request(&bytes) {
                seg.exchanges.push(Exchange { key, replies: Vec::new(), used: false });
            }
        } else if let Some(ex) = seg.exchanges.last_mut() {
            ex.replies.push(bytes);
        }
    }
    if let Some(seg) = current.filter(|s| !s.exchanges.is_empty()) {
        segments.push(seg);
    }
    segments
}

pub struct ReplayTransport {
    exchanges: Vec<Exchange>,
    pending: VecDeque<Vec<u8>>,
    short: bool,
    long: bool,
}

impl ReplayTransport {
    pub fn new(segment: ReplaySegment) -> Self {
        Self { exchanges: segment.exchanges, pending: VecDeque::new(), short: true, long: true }
    }

    /// Number of recorded requests that the replay has served at least once.
    pub fn used_exchanges(&self) -> usize {
        self.exchanges.iter().filter(|e| e.used).count()
    }

    fn rewrite_swid(mut reply: Vec<u8>) -> Vec<u8> {
        if reply.len() >= 5 && matches!(reply[0], SHORT_REPORT_ID | LONG_REPORT_ID) {
            let swid_byte = if reply[2] == 0xff || (reply[0] == SHORT_REPORT_ID && reply[2] == 0x8f) { 4 } else { 3 };
            if reply[swid_byte] & 0x0f != 0 {
                reply[swid_byte] = (reply[swid_byte] & 0xf0) | SOFTWARE_ID;
            }
        }
        reply
    }
}

impl Transport for ReplayTransport {
    fn write_report(&mut self, report: &[u8]) -> io::Result<()> {
        let key = Key::from_request(report)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a HID++ request"))?;
        let pos = self
            .exchanges
            .iter()
            .position(|e| !e.used && e.key == key)
            .or_else(|| self.exchanges.iter().rposition(|e| e.key == key));
        if let Some(pos) = pos {
            let ex = &mut self.exchanges[pos];
            ex.used = true;
            self.pending.extend(ex.replies.iter().cloned().map(Self::rewrite_swid));
        }
        Ok(())
    }

    fn read_report(&mut self, _timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        Ok(self.pending.pop_front())
    }

    fn supports_short(&self) -> bool {
        self.short
    }

    fn supports_long(&self) -> bool {
        self.long
    }
}
