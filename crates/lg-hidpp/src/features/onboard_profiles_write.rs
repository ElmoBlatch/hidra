//! The guarded write path for onboard-profile sectors (0x8100).
//!
//! Wire sequence, per LGS 9.04.49 `Feature8100OnboardProfiles` and libratbag:
//! function 6 `BeginWrite(sector, offset, count)`, function 7 `WriteData`
//! with 16 bytes per call, function 8 `EndWrite`.
//!
//! [`replace_sector`] never writes blind:
//! 1. the new contents must be the right length and carry a valid CRC;
//! 2. the sector is read first and must equal `expected_current`, so a write
//!    built from stale data is refused before anything is sent;
//! 3. after writing, the sector is read back and must equal `new`;
//! 4. on any error or mismatch after writing has begun, `expected_current` is
//!    written back and verified, and the caller is told whether that worked.

use std::time::Duration;

use super::onboard_profiles::{PAGE, read_sector};
use crate::{Device, Error, Result, Transport};

/// Flash writes can take longer than ordinary requests.
const WRITE_TIMEOUT: Duration = Duration::from_secs(3);
const ROLLBACK_ATTEMPTS: usize = 3;

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

fn crc_valid(sector: &[u8]) -> bool {
    let n = sector.len();
    n >= 2 && crc_ccitt(&sector[..n - 2]) == u16::from_be_bytes([sector[n - 2], sector[n - 1]])
}

/// Send one sector: BeginWrite, WriteData x N, EndWrite. No checks.
fn send_sector<T: Transport>(dev: &mut Device<T>, sector: u16, data: &[u8]) -> Result<()> {
    let idx = dev.require(PAGE)?;
    let [s_hi, s_lo] = sector.to_be_bytes();
    let [c_hi, c_lo] = (data.len() as u16).to_be_bytes();
    let previous = dev.timeout();
    dev.set_timeout(previous.max(WRITE_TIMEOUT));
    let result = (|| {
        dev.call(idx, 6, &[s_hi, s_lo, 0, 0, c_hi, c_lo])?;
        for chunk in data.chunks(16) {
            // The last chunk of a sector whose size is not a multiple of 16 is
            // zero-padded to 16 bytes, as LGS does.
            let mut block = [0u8; 16];
            block[..chunk.len()].copy_from_slice(chunk);
            dev.call(idx, 7, &block)?;
        }
        dev.call(idx, 8, &[0, 0, 0])?;
        Ok(())
    })();
    dev.set_timeout(previous);
    result
}

/// Put `original` back and confirm it by reading. True on success.
fn roll_back<T: Transport>(dev: &mut Device<T>, sector: u16, original: &[u8]) -> bool {
    let size = original.len() as u16;
    (0..ROLLBACK_ATTEMPTS).any(|_| {
        send_sector(dev, sector, original).is_ok() && read_sector(dev, sector, size).is_ok_and(|now| now == original)
    })
}

/// Replace a user sector's contents, compare-and-swap, with read-back
/// verification and rollback. See the module documentation for the contract.
///
/// `sector` must be a user sector (not a `0x01nn` ROM address).
pub fn replace_sector<T: Transport>(
    dev: &mut Device<T>,
    sector: u16,
    expected_current: &[u8],
    new: &[u8],
) -> Result<()> {
    let refuse = |reason: &str| Err(Error::WriteRefused { sector, reason: reason.to_string() });
    if sector & 0xff00 != 0 {
        return refuse("not a user sector");
    }
    if new.len() != expected_current.len() || new.len() < 16 || new.len() > u16::MAX as usize {
        return refuse("new contents are not the same size as the sector");
    }
    if !crc_valid(new) {
        return refuse("new contents do not carry a valid CRC");
    }
    let size = new.len() as u16;
    let current = read_sector(dev, sector, size)?;
    if current != expected_current {
        return refuse("the device's contents differ from what the change was based on; read it again");
    }
    if current == new {
        return Ok(());
    }

    let outcome = send_sector(dev, sector, new).map_err(|e| e.to_string()).and_then(|()| {
        match read_sector(dev, sector, size) {
            Ok(now) if now == new => Ok(()),
            Ok(_) => Err("read-back does not match what was written".to_string()),
            Err(e) => Err(format!("read-back failed: {e}")),
        }
    });
    match outcome {
        Ok(()) => Ok(()),
        Err(cause) => {
            let restored = roll_back(dev, sector, expected_current);
            Err(Error::WriteFailed { sector, cause, restored })
        }
    }
}

/// Write `bytes` to a user sector exactly as given, then confirm by reading
/// back. No compare-and-swap and no CRC requirement: this is the recovery
/// path for putting a backup back, including sectors that were erased
/// (all `0xff`, which never carries a valid CRC).
pub fn restore_sector<T: Transport>(dev: &mut Device<T>, sector: u16, bytes: &[u8]) -> Result<()> {
    let refuse = |reason: &str| Err(Error::WriteRefused { sector, reason: reason.to_string() });
    if sector & 0xff00 != 0 {
        return refuse("not a user sector");
    }
    if bytes.len() < 16 || bytes.len() > u16::MAX as usize {
        return refuse("not a whole sector");
    }
    let size = bytes.len() as u16;
    if read_sector(dev, sector, size)? == bytes {
        return Ok(());
    }
    let ok = (0..ROLLBACK_ATTEMPTS)
        .any(|_| send_sector(dev, sector, bytes).is_ok() && read_sector(dev, sector, size).is_ok_and(|now| now == bytes));
    if ok {
        Ok(())
    } else {
        Err(Error::WriteFailed { sector, cause: "restore did not read back correctly".into(), restored: false })
    }
}

/// Make a profile the active one (function 3). `address` is what
/// `GetActiveProfile` reports: a user profile's sector, or `0x0100 | n` for a
/// factory profile. Not stored in flash: it changes which profile the mouse
/// is running, like pressing the profile button.
pub fn set_active_profile<T: Transport>(dev: &mut Device<T>, address: u16) -> Result<()> {
    let idx = dev.require(PAGE)?;
    let [hi, lo] = address.to_be_bytes();
    dev.call(idx, 3, &[hi, lo, 0])?;
    Ok(())
}
