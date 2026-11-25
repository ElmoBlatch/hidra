//! A simulated HID++ mouse with onboard profile memory.
//!
//! It answers the requests this crate issues (root, feature set, adjustable
//! DPI, report rate, onboard profiles including the write functions) from
//! in-memory state. Writes land in "flash" chunk by chunk like the real
//! device, so an interrupted write leaves a torn sector. The memory is shared
//! (`Arc<Mutex<_>>`), so it persists across transports: open, write, close,
//! reopen, read.
//!
//! Used by tests and by the viewer's demo mode. It is not a firmware model:
//! it does not interpret profiles, except that the live sensor DPI and
//! polling rate follow the active profile's default stage and report rate.

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::transport::Transport;

const IDX_FEATURE_SET: u8 = 1;
const IDX_DPI: u8 = 2;
const IDX_RATE: u8 = 3;
const IDX_ONBOARD: u8 = 4;
const PAGES: [u16; 4] = [0x0001, 0x2201, 0x8060, 0x8100];

/// Faults to inject, each consumed once.
#[derive(Debug, Default, Clone)]
pub struct SimFaults {
    /// Reject the Nth `WriteData` call (0-based, counted from now).
    pub fail_write_data_at: Option<usize>,
    /// Flip the first byte of the Nth `WriteData` call.
    pub corrupt_write_data_at: Option<usize>,
}

/// The simulated device's persistent state.
#[derive(Debug, Clone)]
pub struct SimMemory {
    /// The 0x8100 `GetInfo` reply.
    pub descriptor: [u8; 16],
    /// Sector contents by address (user sectors and `0x01nn` ROM profiles).
    pub sectors: BTreeMap<u16, Vec<u8>>,
    pub active_profile: u16,
    pub active_stage: u8,
    /// DPI range reported by 0x2201.
    pub dpi_min: u16,
    pub dpi_max: u16,
    pub dpi_step: u16,
    /// 0x8060 bitmask: bit n set means an (n+1) ms report interval.
    pub rate_mask: u8,
    pub faults: SimFaults,
    /// Number of `BeginWrite` calls served.
    pub begin_writes: usize,
    write_data_calls: usize,
    /// (sector, next offset, remaining) of the write in progress.
    cursor: Option<(u16, usize, usize)>,
}

impl SimMemory {
    /// A device with the given descriptor and sectors; DPI range and polling
    /// rates default to the G502 Proteus Spectrum's.
    pub fn new(descriptor: [u8; 16], sectors: BTreeMap<u16, Vec<u8>>, active_profile: u16) -> Self {
        Self {
            descriptor,
            sectors,
            active_profile,
            active_stage: 0,
            dpi_min: 50,
            dpi_max: 12000,
            dpi_step: 50,
            rate_mask: 0x8b,
            faults: SimFaults::default(),
            begin_writes: 0,
            write_data_calls: 0,
            cursor: None,
        }
        .with_default_stage()
    }

    fn with_default_stage(mut self) -> Self {
        if let Some(s) = self.sectors.get(&self.active_profile) {
            self.active_stage = s.get(1).copied().unwrap_or(0);
        }
        self
    }

    pub fn shared(self) -> Arc<Mutex<SimMemory>> {
        Arc::new(Mutex::new(self))
    }

    fn sector_size(&self) -> usize {
        u16::from_be_bytes([self.descriptor[7], self.descriptor[8]]) as usize
    }

    fn live_dpi(&self) -> u16 {
        self.sectors
            .get(&self.active_profile)
            .and_then(|s| {
                let o = 3 + self.active_stage as usize * 2;
                Some(u16::from_le_bytes([*s.get(o)?, *s.get(o + 1)?]))
            })
            .unwrap_or(0)
    }

    fn live_rate_ms(&self) -> u8 {
        self.sectors.get(&self.active_profile).and_then(|s| s.first().copied()).unwrap_or(1)
    }
}

/// A [`Transport`] onto a [`SimMemory`].
pub struct SimMouse {
    memory: Arc<Mutex<SimMemory>>,
    pending: VecDeque<Vec<u8>>,
}

impl SimMouse {
    pub fn new(memory: Arc<Mutex<SimMemory>>) -> Self {
        Self { memory, pending: VecDeque::new() }
    }

    fn reply(&mut self, req: &[u8], payload: &[u8]) {
        let mut out = vec![0u8; 20];
        out[..4].copy_from_slice(&[0x11, req[1], req[2], req[3]]);
        out[4..4 + payload.len()].copy_from_slice(payload);
        self.pending.push_back(out);
    }

    fn error(&mut self, req: &[u8], code: u8) {
        let mut out = vec![0u8; 20];
        out[..6].copy_from_slice(&[0x11, req[1], 0xff, req[2], req[3], code]);
        self.pending.push_back(out);
    }
}

impl Transport for SimMouse {
    fn write_report(&mut self, req: &[u8]) -> io::Result<()> {
        if req.len() < 4 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "short report"));
        }
        let (feature, function) = (req[2], req[3] >> 4);
        let p = |i: usize| req.get(4 + i).copied().unwrap_or(0);
        let be16 = |i: usize| u16::from_be_bytes([p(i), p(i + 1)]);
        let memory = self.memory.clone();
        let mut m = memory.lock().expect("sim memory lock");
        match (feature, function) {
            (0, 0) => {
                let index = PAGES.iter().position(|&pg| pg == be16(0)).map(|i| i as u8 + 1).unwrap_or(0);
                self.reply(req, &[index, 0, 0]);
            }
            (0, 1) => self.reply(req, &[4, 2, p(2)]),
            (IDX_FEATURE_SET, 0) => self.reply(req, &[PAGES.len() as u8]),
            (IDX_FEATURE_SET, 1) => match PAGES.get((p(0) as usize).wrapping_sub(1)) {
                Some(page) => {
                    let [hi, lo] = page.to_be_bytes();
                    self.reply(req, &[hi, lo, 0, 0]);
                }
                None => self.error(req, 0x02),
            },
            (IDX_DPI, 0) => self.reply(req, &[1]),
            (IDX_DPI, 1) => {
                let (min, max) = (m.dpi_min.to_be_bytes(), m.dpi_max.to_be_bytes());
                let step = (0xe000 | m.dpi_step).to_be_bytes();
                self.reply(req, &[0, min[0], min[1], step[0], step[1], max[0], max[1]]);
            }
            (IDX_DPI, 2) => {
                let dpi = m.live_dpi().to_be_bytes();
                self.reply(req, &[0, dpi[0], dpi[1], 0, 0]);
            }
            (IDX_RATE, 0) => self.reply(req, &[m.rate_mask]),
            (IDX_RATE, 1) => self.reply(req, &[m.live_rate_ms()]),
            (IDX_ONBOARD, 0) => {
                let d = m.descriptor;
                self.reply(req, &d);
            }
            (IDX_ONBOARD, 2) => self.reply(req, &[1]),
            (IDX_ONBOARD, 4) => {
                let a = m.active_profile.to_be_bytes();
                self.reply(req, &a);
            }
            (IDX_ONBOARD, 0xb) => self.reply(req, &[m.active_stage]),
            (IDX_ONBOARD, 5) => {
                let (sector, offset) = (be16(0), be16(2) as usize);
                match m.sectors.get(&sector) {
                    Some(s) if offset + 16 <= s.len() => {
                        let block = s[offset..offset + 16].to_vec();
                        self.reply(req, &block);
                    }
                    _ => self.error(req, 0x02),
                }
            }
            (IDX_ONBOARD, 6) => {
                m.begin_writes += 1;
                let (sector, offset, count) = (be16(0), be16(2) as usize, be16(4) as usize);
                let size = m.sector_size();
                if sector & 0xff00 == 0 && offset + count <= size {
                    m.sectors.entry(sector).or_insert_with(|| vec![0xff; size]);
                    m.cursor = Some((sector, offset, count));
                    self.reply(req, &[]);
                } else {
                    self.error(req, 0x02);
                }
            }
            (IDX_ONBOARD, 7) => {
                let call = m.write_data_calls;
                m.write_data_calls += 1;
                if m.faults.fail_write_data_at == Some(call) {
                    m.faults.fail_write_data_at = None;
                    m.cursor = None;
                    self.error(req, 0x04);
                    return Ok(());
                }
                let Some((sector, offset, remaining)) = m.cursor else {
                    self.error(req, 0x05);
                    return Ok(());
                };
                let n = remaining.min(16);
                let mut block: Vec<u8> = (0..n).map(p).collect();
                if m.faults.corrupt_write_data_at == Some(call) {
                    m.faults.corrupt_write_data_at = None;
                    block[0] ^= 0xff;
                }
                if let Some(s) = m.sectors.get_mut(&sector) {
                    s[offset..offset + n].copy_from_slice(&block);
                }
                m.cursor = Some((sector, offset + n, remaining - n));
                self.reply(req, &[]);
            }
            (IDX_ONBOARD, 8) => {
                m.cursor = None;
                self.reply(req, &[]);
            }
            _ => self.error(req, 0x07),
        }
        Ok(())
    }

    fn read_report(&mut self, _: Duration) -> io::Result<Option<Vec<u8>>> {
        Ok(self.pending.pop_front())
    }

    fn supports_short(&self) -> bool {
        true
    }

    fn supports_long(&self) -> bool {
        true
    }
}
