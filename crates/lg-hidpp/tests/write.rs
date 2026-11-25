//! The guarded write path against a simulated mouse with onboard memory.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lg_hidpp::features::onboard_profiles::read_sector;
use lg_hidpp::features::onboard_profiles_write::replace_sector;
use lg_hidpp::sim::{SimMemory, SimMouse};
use lg_hidpp::{DEVICE_INDEX_DIRECT, Device, Error};

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

/// Four 256-byte sectors with distinct contents and valid CRCs.
fn memory() -> Arc<Mutex<SimMemory>> {
    let descriptor = [0x01, 0x02, 0x01, 0x03, 0x03, 0x0b, 0x10, 0x01, 0x00, 0x0a, 0x01, 0, 0, 0, 0, 0];
    let sectors: BTreeMap<u16, Vec<u8>> =
        (0..4u16).map(|i| (i, with_crc(vec![(i as u8).wrapping_mul(17).wrapping_add(1); 256]))).collect();
    SimMemory::new(descriptor, sectors, 1).shared()
}

fn open(memory: &Arc<Mutex<SimMemory>>) -> Device<SimMouse> {
    Device::open_with_timeout(SimMouse::new(memory.clone()), DEVICE_INDEX_DIRECT, Duration::from_millis(50)).unwrap()
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
    let mem = memory();
    let mut dev = open(&mem);
    let before: Vec<Vec<u8>> = (0..4).map(|s| read_sector(&mut dev, s, 256).unwrap()).collect();
    let new = edited(&before[1]);

    replace_sector(&mut dev, 1, &before[1], &new).unwrap();

    assert_eq!(read_sector(&mut dev, 1, 256).unwrap(), new);
    for s in [0u16, 2, 3] {
        assert_eq!(read_sector(&mut dev, s, 256).unwrap(), before[s as usize], "sector {s} untouched");
    }
    assert_eq!(mem.lock().unwrap().begin_writes, 1);
}

#[test]
fn identical_contents_send_nothing() {
    let mem = memory();
    let mut dev = open(&mem);
    let current = read_sector(&mut dev, 1, 256).unwrap();
    replace_sector(&mut dev, 1, &current, &current).unwrap();
    assert_eq!(mem.lock().unwrap().begin_writes, 0);
}

#[test]
fn stale_expectation_is_refused_before_any_write() {
    let mem = memory();
    let mut dev = open(&mem);
    let current = read_sector(&mut dev, 1, 256).unwrap();
    let mut stale = current.clone();
    stale[40] ^= 1;
    let err = replace_sector(&mut dev, 1, &stale, &edited(&stale)).unwrap_err();
    assert!(matches!(err, Error::WriteRefused { .. }), "{err}");
    let m = mem.lock().unwrap();
    assert_eq!(m.begin_writes, 0);
    assert_eq!(m.sectors[&1], current);
}

#[test]
fn bad_crc_wrong_size_and_rom_addresses_are_refused() {
    let mem = memory();
    let mut dev = open(&mem);
    let current = read_sector(&mut dev, 1, 256).unwrap();
    let mut bad_crc = edited(&current);
    bad_crc[255] ^= 1;
    assert!(matches!(replace_sector(&mut dev, 1, &current, &bad_crc), Err(Error::WriteRefused { .. })));
    assert!(matches!(replace_sector(&mut dev, 1, &current, &current[..240]), Err(Error::WriteRefused { .. })));
    assert!(matches!(replace_sector(&mut dev, 0x0101, &current, &edited(&current)), Err(Error::WriteRefused { .. })));
    assert_eq!(mem.lock().unwrap().begin_writes, 0);
}

#[test]
fn a_write_that_fails_midway_is_rolled_back() {
    let mem = memory();
    mem.lock().unwrap().faults.fail_write_data_at = Some(5);
    let mut dev = open(&mem);
    let current = read_sector(&mut dev, 1, 256).unwrap();

    let err = replace_sector(&mut dev, 1, &current, &edited(&current)).unwrap_err();
    assert!(matches!(err, Error::WriteFailed { restored: true, .. }), "{err}");
    assert_eq!(read_sector(&mut dev, 1, 256).unwrap(), current, "original contents are back");
}

#[test]
fn a_write_that_lands_wrong_is_caught_by_read_back_and_rolled_back() {
    let mem = memory();
    mem.lock().unwrap().faults.corrupt_write_data_at = Some(9);
    let mut dev = open(&mem);
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

#[test]
fn writes_persist_across_connections() {
    let mem = memory();
    let mut dev = open(&mem);
    let current = read_sector(&mut dev, 1, 256).unwrap();
    let new = edited(&current);
    replace_sector(&mut dev, 1, &current, &new).unwrap();
    drop(dev);
    let mut again = open(&mem);
    assert_eq!(read_sector(&mut again, 1, 256).unwrap(), new);
}
