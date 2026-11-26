use std::io;
use std::time::Duration;

use lg_hidpp::replay::ReplayTransport;
use lg_hidpp::sim::SimMouse;
use lg_hidpp::{HidTransport, Transport};

/// Either a live hidraw node or a replayed capture, behind one transport type.
pub enum AnyTransport {
    Hidraw(HidTransport),
    Replay(ReplayTransport),
    Sim(SimMouse),
}

impl Transport for AnyTransport {
    fn write_report(&mut self, r: &[u8]) -> io::Result<()> {
        match self {
            Self::Hidraw(t) => t.write_report(r),
            Self::Replay(t) => t.write_report(r),
            Self::Sim(t) => t.write_report(r),
        }
    }
    fn read_report(&mut self, d: Duration) -> io::Result<Option<Vec<u8>>> {
        match self {
            Self::Hidraw(t) => t.read_report(d),
            Self::Replay(t) => t.read_report(d),
            Self::Sim(t) => t.read_report(d),
        }
    }
    fn supports_short(&self) -> bool {
        match self {
            Self::Hidraw(t) => t.supports_short(),
            Self::Replay(t) => t.supports_short(),
            Self::Sim(t) => t.supports_short(),
        }
    }
    fn supports_long(&self) -> bool {
        match self {
            Self::Hidraw(t) => t.supports_long(),
            Self::Replay(t) => t.supports_long(),
            Self::Sim(t) => t.supports_long(),
        }
    }
}
