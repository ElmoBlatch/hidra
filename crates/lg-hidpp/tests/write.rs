//! The guarded write path against a simulated mouse with onboard memory.

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use lg_hidpp::features::onboard_profiles::read_sector;
use lg_hidpp::features::onboard_profiles_write::replace_sector;
use lg_hidpp::{DEVICE_INDEX_DIRECT, Device, Error, Transport};

const SECTOR: usize = 256;
const ONBOARD_INDEX: u8 = 2;

fn crc_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

fn with_crc(mut sector: Vec<u8>) -> Vec<u8> {
    let n = sector.len();
    let crc = crc_ccitt(&sector[..n - 2]);
    sector[n - 2..].copy_from_slice(&crc.to_be_bytes());
    sector
}

/// A mouse with four 256-byte sectors. Writes go straight to "flash" chunk by
/// chunk, like the real thing, so an interrupted write leaves a torn sector.
struct FakeMouse {
    sectors: Vec<Vec<u8>>,
    pending: VecDeque<Vec<u8>>,
    /// (sector, next offset, remaining) of the write in progress.
    cursor: Option<(usize, usize, usize)>,
    /// Fail the Nth WriteData call (0-based, counted across the session) once.
    fail_write_data_at: Option<usize>,
    /// Silently corrupt one byte of the Nth WriteData call, once.
    corrupt_write_data_at: Option<usize>,
    write_data_calls: usize,
    begin_calls: usize,
}

impl FakeMouse {
    fn new() -> Self {
        let sectors = (0..4u8).map(|i| with_crc(vec![i.wrapping_mul(17).wrapping_add(1); SECTOR])).collect();
        Self {
            sectors,
            pending: VecDeque::new(),
            cursor: None,
            fail_write_data_at: None,
            corrupt_write_data_at: None,
            write_data_calls: 0,
            begin_calls: 0,
        }
    }

    fn reply(&mut self, req: &[u8], payload: &[u8]) {
        let mut out = vec![0u8; 20];
        out[0] = 0x11;
        out[1] = req[1];
        out[2] = req[2];
        out[3] = req[3];
        out[4..4 + payload.len()].copy_from_slice(payload);
        self.pending.push_back(out);
    }

    fn error(&mut self, req: &[u8], code: u8) {
        let mut out = vec![0u8; 20];
        out[..6].copy_from_slice(&[0x11, req[1], 0xff, req[2], req[3], code]);
        self.pending.push_back(out);
    }
}

impl Transport for FakeMouse {
    fn write_report(&mut self, req: &[u8]) -> io::Result<()> {
        let (feature, function) = (req[2], req[3] >> 4);
        let p = |i: usize| req.get(4 + i).copied().unwrap_or(0);
        let be16 = |i: usize| u16::from_be_bytes([p(i), p(i + 1)]) as usize;
        match (feature, function) {
            // Root: GetFeature, GetProtocolVersion.
            (0, 0) => {
                let index = match be16(0) {
                    0x0001 => 1,
                    0x8100 => ONBOARD_INDEX,
                    _ => 0,
                };
                self.reply(req, &[index, 0, 0]);
            }
            (0, 1) => self.reply(req, &[4, 2, p(2)]),
            // Feature set: count, feature ID.
            (1, 0) => self.reply(req, &[2]),
            (1, 1) => {
                let page: u16 = if p(0) == 1 { 0x0001 } else { 0x8100 };
                let [hi, lo] = page.to_be_bytes();
                self.reply(req, &[hi, lo, 0, 0]);
            }
            // Onboard profiles: ReadData, BeginWrite, WriteData, EndWrite.
            (ONBOARD_INDEX, 5) => {
                let (sector, offset) = (be16(0), be16(2));
                match self.sectors.get(sector) {
                    Some(s) if offset + 16 <= s.len() => {
                        let block = s[offset..offset + 16].to_vec();
                        self.reply(req, &block);
                    }
                    _ => self.error(req, 0x02),
                }
            }
            (ONBOARD_INDEX, 6) => {
                self.begin_calls += 1;
                let (sector, offset, count) = (be16(0), be16(2), be16(4));
                if sector < self.sectors.len() && offset + count <= SECTOR {
                    self.cursor = Some((sector, offset, count));
                    self.reply(req, &[]);
                } else {
                    self.error(req, 0x02);
                }
            }
            (ONBOARD_INDEX, 7) => {
                let call = self.write_data_calls;
                self.write_data_calls += 1;
                if self.fail_write_data_at == Some(call) {
                    self.fail_write_data_at = None;
                    self.cursor = None;
                    self.error(req, 0x04);
                    return Ok(());
                }
                let Some((sector, offset, remaining)) = self.cursor else {
                    self.error(req, 0x05);
                    return Ok(());
                };
                let n = remaining.min(16);
                let mut block = req[4..4 + n].to_vec();
                if self.corrupt_write_data_at == Some(call) {
                    self.corrupt_write_data_at = None;
                    block[0] ^= 0xff;
                }
                self.sectors[sector][offset..offset + n].copy_from_slice(&block);
                self.cursor = Some((sector, offset + n, remaining - n));
                self.reply(req, &[]);
            }
            (ONBOARD_INDEX, 8) => {
                self.cursor = None;
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

fn open(mouse: FakeMouse) -> Device<FakeMouse> {
    Device::open_with_timeout(mouse, DEVICE_INDEX_DIRECT, Duration::from_millis(50)).unwrap()
}

/// A valid replacement for sector 1: change two bytes, fix the CRC.
fn edited(original: &[u8]) -> Vec<u8> {
    let mut new = original.to_vec();
    new[3] = 0x80;
    new[4] = 0x0c;
    with_crc(new)
}

#[test]
fn replace_writes_and_only_touches_the_target_sector() {
    let mut dev = open(FakeMouse::new());
    let before: Vec<Vec<u8>> = (0..4).map(|s| read_sector(&mut dev, s, 256).unwrap()).collect();
    let new = edited(&before[1]);

    replace_sector(&mut dev, 1, &before[1], &new).unwrap();

    assert_eq!(read_sector(&mut dev, 1, 256).unwrap(), new);
    for s in [0u16, 2, 3] {
        assert_eq!(read_sector(&mut dev, s, 256).unwrap(), before[s as usize], "sector {s} untouched");
    }
    assert_eq!(dev.into_transport().begin_calls, 1);
}

#[test]
fn identical_contents_send_nothing() {
    let mut dev = open(FakeMouse::new());
    let current = read_sector(&mut dev, 1, 256).unwrap();
    replace_sector(&mut dev, 1, &current, &current).unwrap();
    assert_eq!(dev.into_transport().begin_calls, 0);
}

#[test]
fn stale_expectation_is_refused_before_any_write() {
    let mut dev = open(FakeMouse::new());
    let current = read_sector(&mut dev, 1, 256).unwrap();
    let mut stale = current.clone();
    stale[40] ^= 1;
    let err = replace_sector(&mut dev, 1, &stale, &edited(&stale)).unwrap_err();
    assert!(matches!(err, Error::WriteRefused { .. }), "{err}");
    let mouse = dev.into_transport();
    assert_eq!(mouse.begin_calls, 0);
    assert_eq!(mouse.sectors[1], current);
}

#[test]
fn bad_crc_wrong_size_and_rom_addresses_are_refused() {
    let mut dev = open(FakeMouse::new());
    let current = read_sector(&mut dev, 1, 256).unwrap();
    let mut bad_crc = edited(&current);
    bad_crc[255] ^= 1;
    assert!(matches!(replace_sector(&mut dev, 1, &current, &bad_crc), Err(Error::WriteRefused { .. })));
    assert!(matches!(replace_sector(&mut dev, 1, &current, &current[..240]), Err(Error::WriteRefused { .. })));
    assert!(matches!(replace_sector(&mut dev, 0x0101, &current, &edited(&current)), Err(Error::WriteRefused { .. })));
    assert_eq!(dev.into_transport().begin_calls, 0);
}

#[test]
fn a_write_that_fails_midway_is_rolled_back() {
    let mut mouse = FakeMouse::new();
    mouse.fail_write_data_at = Some(5);
    let mut dev = open(mouse);
    let current = read_sector(&mut dev, 1, 256).unwrap();

    let err = replace_sector(&mut dev, 1, &current, &edited(&current)).unwrap_err();
    assert!(matches!(err, Error::WriteFailed { restored: true, .. }), "{err}");
    assert_eq!(read_sector(&mut dev, 1, 256).unwrap(), current, "original contents are back");
}

#[test]
fn a_write_that_lands_wrong_is_caught_by_read_back_and_rolled_back() {
    let mut mouse = FakeMouse::new();
    mouse.corrupt_write_data_at = Some(9);
    let mut dev = open(mouse);
    let current = read_sector(&mut dev, 1, 256).unwrap();

    let err = replace_sector(&mut dev, 1, &current, &edited(&current)).unwrap_err();
    match err {
        Error::WriteFailed { restored, cause, .. } => {
            assert!(restored);
            assert!(cause.contains("read-back"), "{cause}");
        }
        other => panic!("unexpected {other}"),
    }
    assert_eq!(read_sector(&mut dev, 1, 256).unwrap(), current);
}
