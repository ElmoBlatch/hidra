//! HID++ report framing.
//!
//! Short report: `10 dev feat (fn<<4|swid) p0 p1 p2` (7 bytes).
//! Long report:  `11 dev feat (fn<<4|swid) p0..p15` (20 bytes).

pub const SHORT_REPORT_ID: u8 = 0x10;
pub const LONG_REPORT_ID: u8 = 0x11;
pub const SHORT_REPORT_LEN: usize = 7;
pub const LONG_REPORT_LEN: usize = 20;
const HIDPP20_ERROR_FEATURE: u8 = 0xff;
const HIDPP10_ERROR_SUB_ID: u8 = 0x8f;

/// Software ID placed in the low nibble of byte 3. Non-zero so that replies to
/// us are distinguishable from device notifications (SWID 0). libratbag uses
/// 0x8; using a different value avoids confusing a concurrently running
/// ratbagd.
pub const SOFTWARE_ID: u8 = 0x0b;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub device_index: u8,
    pub feature_index: u8,
    pub function: u8,
    pub params: Vec<u8>,
}

impl Request {
    /// Encode as a short report when the parameters fit and short reports are
    /// supported, otherwise as a long report.
    pub fn encode(&self, short_ok: bool, long_ok: bool) -> Option<Vec<u8>> {
        debug_assert!(self.function < 0x10);
        let use_short = short_ok && (self.params.len() <= 3 || !long_ok);
        let (id, len) = if use_short {
            if self.params.len() > 3 {
                return None;
            }
            (SHORT_REPORT_ID, SHORT_REPORT_LEN)
        } else if long_ok {
            if self.params.len() > 16 {
                return None;
            }
            (LONG_REPORT_ID, LONG_REPORT_LEN)
        } else {
            return None;
        };
        let mut out = vec![0u8; len];
        out[0] = id;
        out[1] = self.device_index;
        out[2] = self.feature_index;
        out[3] = (self.function << 4) | SOFTWARE_ID;
        out[4..4 + self.params.len()].copy_from_slice(&self.params);
        Some(out)
    }
}

/// A decoded incoming report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// A HID++ 2.0 reply or notification. `payload` is 3 or 16 bytes.
    Message {
        device_index: u8,
        feature_index: u8,
        function: u8,
        software_id: u8,
        payload: Vec<u8>,
    },
    /// HID++ 2.0 error reply: `11 dev ff feat fn|swid code`.
    Error20 {
        device_index: u8,
        feature_index: u8,
        function: u8,
        software_id: u8,
        code: u8,
    },
    /// HID++ 1.0 error reply: `10 dev 8f sub_id address code`.
    Error10 {
        device_index: u8,
        sub_id: u8,
        address: u8,
        code: u8,
    },
    /// Anything else (mouse input reports, DJ reports, ...).
    Other,
}

impl Incoming {
    pub fn parse(buf: &[u8]) -> Self {
        let expected = match buf.first() {
            Some(&SHORT_REPORT_ID) => SHORT_REPORT_LEN,
            Some(&LONG_REPORT_ID) => LONG_REPORT_LEN,
            _ => return Incoming::Other,
        };
        if buf.len() < expected {
            return Incoming::Other;
        }
        let device_index = buf[1];
        if buf[0] == SHORT_REPORT_ID && buf[2] == HIDPP10_ERROR_SUB_ID {
            return Incoming::Error10 {
                device_index,
                sub_id: buf[3],
                address: buf[4],
                code: buf[5],
            };
        }
        if buf[2] == HIDPP20_ERROR_FEATURE {
            return Incoming::Error20 {
                device_index,
                feature_index: buf[3],
                function: buf[4] >> 4,
                software_id: buf[4] & 0x0f,
                code: buf[5],
            };
        }
        Incoming::Message {
            device_index,
            feature_index: buf[2],
            function: buf[3] >> 4,
            software_id: buf[3] & 0x0f,
            payload: buf[4..expected].to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_request_encoding() {
        let req = Request { device_index: 0xff, feature_index: 0x00, function: 1, params: vec![] };
        assert_eq!(req.encode(true, true).unwrap(), vec![0x10, 0xff, 0x00, 0x1b, 0, 0, 0]);
    }

    #[test]
    fn long_request_when_params_exceed_short() {
        let req = Request { device_index: 0xff, feature_index: 0x0e, function: 5, params: vec![0, 1, 0, 0x10] };
        let enc = req.encode(true, true).unwrap();
        assert_eq!(enc.len(), LONG_REPORT_LEN);
        assert_eq!(&enc[..8], &[0x11, 0xff, 0x0e, 0x5b, 0, 1, 0, 0x10]);
    }

    #[test]
    fn long_only_device_gets_long_report() {
        let req = Request { device_index: 1, feature_index: 0, function: 0, params: vec![0x81, 0x00] };
        assert_eq!(req.encode(false, true).unwrap()[0], LONG_REPORT_ID);
    }

    #[test]
    fn parse_errors_and_noise() {
        let e20 = [0x11, 0xff, 0xff, 0x0e, 0x5b, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            Incoming::parse(&e20),
            Incoming::Error20 { device_index: 0xff, feature_index: 0x0e, function: 5, software_id: 0xb, code: 2 }
        );
        let e10 = [0x10, 0x01, 0x8f, 0x00, 0x1b, 0x09, 0x00];
        assert_eq!(
            Incoming::parse(&e10),
            Incoming::Error10 { device_index: 1, sub_id: 0, address: 0x1b, code: 9 }
        );
        // Mouse input report (report ID 2) interleaved with HID++ traffic.
        assert_eq!(Incoming::parse(&[0x02, 0, 0, 0, 0, 1, 0, 0, 0]), Incoming::Other);
    }
}
