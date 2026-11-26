//! Read-only queries to a Logitech wireless receiver (HID++ 1.0).
//!
//! A receiver keeps a slot per paired device. The device in slot N is reached
//! by sending HID++ 2.0 requests to the receiver with device index N. Linux
//! hides this behind a node per paired device; Windows does not, so there the
//! slots have to be asked for.

use std::time::{Duration, Instant};

use crate::report::{LONG_REPORT_ID, LONG_REPORT_LEN, SHORT_REPORT_ID, SHORT_REPORT_LEN};
use crate::{DEVICE_INDEX_DIRECT, Result, Transport};

/// Highest slot number a receiver can have.
pub const MAX_SLOTS: u8 = 6;

const GET_LONG_REGISTER: u8 = 0x83;
const ERROR_SUB_ID: u8 = 0x8f;
/// Receiver information register (0x2b5).
const RECEIVER_INFO: u8 = 0xb5;
const REPLY_TIMEOUT: Duration = Duration::from_millis(300);

/// A device paired to a receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairedDevice {
    /// 1-based slot; also the device index to address it with.
    pub slot: u8,
    /// Wireless product ID.
    pub wpid: u16,
}

/// Bolt receivers keep pairing information at a different sub-address and
/// byte order than Unifying and LIGHTSPEED ones. Taken from Solaar; not run
/// against a Bolt receiver here.
fn is_bolt(receiver_pid: u16) -> bool {
    receiver_pid == 0xc548
}

/// The pairing-information request for `slot`: a short HID++ 1.0 "get long
/// register" report.
fn pairing_request(receiver_pid: u16, slot: u8) -> [u8; SHORT_REPORT_LEN] {
    let base = if is_bolt(receiver_pid) { 0x50 } else { 0x20 };
    let sub_address = if is_bolt(receiver_pid) { base + slot } else { base + slot - 1 };
    [SHORT_REPORT_ID, DEVICE_INDEX_DIRECT, GET_LONG_REGISTER, RECEIVER_INFO, sub_address, 0, 0]
}

/// What a report means as the answer to `request`: `Some(Some(wpid))` for
/// pairing information, `Some(None)` when the receiver says the slot is
/// empty, `None` when the report is something else.
fn pairing_reply(receiver_pid: u16, request: &[u8; SHORT_REPORT_LEN], report: &[u8]) -> Option<Option<u16>> {
    let sub_address = request[4];
    match report {
        [LONG_REPORT_ID, DEVICE_INDEX_DIRECT, GET_LONG_REGISTER, RECEIVER_INFO, sub, data @ ..]
            if *sub == sub_address && report.len() >= LONG_REPORT_LEN =>
        {
            let wpid = if is_bolt(receiver_pid) {
                u16::from_le_bytes([data[1], data[2]])
            } else {
                u16::from_be_bytes([data[2], data[3]])
            };
            Some((wpid != 0).then_some(wpid))
        }
        [SHORT_REPORT_ID, DEVICE_INDEX_DIRECT, ERROR_SUB_ID, GET_LONG_REGISTER, RECEIVER_INFO, ..] => Some(None),
        _ => None,
    }
}

/// The devices paired to the receiver behind `transport`, whether or not they
/// are switched on. Reads registers only.
pub fn paired_devices<T: Transport>(transport: &mut T, receiver_pid: u16) -> Result<Vec<PairedDevice>> {
    let mut paired = Vec::new();
    if !transport.supports_short() {
        return Ok(paired);
    }
    for slot in 1..=MAX_SLOTS {
        let request = pairing_request(receiver_pid, slot);
        transport.write_report(&request)?;
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let Some(report) = transport.read_report(remaining)? else { break };
            if let Some(answer) = pairing_reply(receiver_pid, &request, &report) {
                if let Some(wpid) = answer {
                    paired.push(PairedDevice { slot, wpid });
                }
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
        }
    }
    Ok(paired)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unifying_and_lightspeed_pairing_information() {
        let request = pairing_request(0xc539, 1);
        assert_eq!(request, [0x10, 0xff, 0x83, 0xb5, 0x20, 0, 0]);
        let mut reply = [0u8; 20];
        reply[..9].copy_from_slice(&[0x11, 0xff, 0x83, 0xb5, 0x20, 0x07, 0x01, 0x40, 0x7f]);
        assert_eq!(pairing_reply(0xc539, &request, &reply), Some(Some(0x407f)));
        // The answer for another slot, and mouse movement, are not ours.
        reply[4] = 0x21;
        assert_eq!(pairing_reply(0xc539, &request, &reply), None);
        assert_eq!(pairing_reply(0xc539, &request, &[0x01, 0, 0, 0]), None);
        // "Invalid address" for an empty slot.
        assert_eq!(pairing_reply(0xc539, &request, &[0x10, 0xff, 0x8f, 0x83, 0xb5, 0x02, 0]), Some(None));
    }

    #[test]
    fn bolt_pairing_information() {
        let request = pairing_request(0xc548, 2);
        assert_eq!(request[4], 0x52);
        let mut reply = [0u8; 20];
        reply[..8].copy_from_slice(&[0x11, 0xff, 0x83, 0xb5, 0x52, 0x00, 0x23, 0xb0]);
        assert_eq!(pairing_reply(0xc548, &request, &reply), Some(Some(0xb023)));
    }
}
