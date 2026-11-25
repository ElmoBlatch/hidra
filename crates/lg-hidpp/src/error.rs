use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("timed out waiting for reply to feature index {feature_index:#04x} function {function}")]
    Timeout { feature_index: u8, function: u8 },
    #[error("HID++ 2.0 error {0}")]
    Hidpp20(Hidpp20Error),
    #[error("HID++ 1.0 error {0}")]
    Hidpp10(Hidpp10Error),
    #[error("device does not expose feature page {0:#06x}")]
    FeatureMissing(u16),
    #[error("malformed reply: {0}")]
    Malformed(String),
    #[error("device supports neither short nor long HID++ reports")]
    NoHidppReports,
    #[error("the mouse is not connected to its receiver: it is switched off, asleep or out of range")]
    NotConnected,
    #[error("refusing to write sector {sector:#06x}: {reason}")]
    WriteRefused { sector: u16, reason: String },
    #[error("writing sector {sector:#06x} failed ({cause}); original contents {}", if *.restored { "were restored and verified" } else { "could NOT be restored" })]
    WriteFailed { sector: u16, cause: String, restored: bool },
}

/// HID++ 2.0 error codes (`ff` feature-index replies).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hidpp20Error(pub u8);

impl fmt::Display for Hidpp20Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            0x00 => "no error",
            0x01 => "unknown",
            0x02 => "invalid argument",
            0x03 => "out of range",
            0x04 => "hardware error",
            0x05 => "Logitech internal",
            0x06 => "invalid feature index",
            0x07 => "invalid function ID",
            0x08 => "busy",
            0x09 => "unsupported",
            _ => "unrecognised",
        };
        write!(f, "{:#04x} ({name})", self.0)
    }
}

/// HID++ 1.0 error codes (`8f` sub-ID replies), e.g. from a receiver whose
/// paired device is asleep or out of range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hidpp10Error(pub u8);

impl fmt::Display for Hidpp10Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            0x01 => "invalid sub-ID",
            0x02 => "invalid address",
            0x03 => "invalid value",
            0x04 => "connection request failed",
            0x05 => "too many devices",
            0x06 => "already exists",
            0x07 => "busy",
            0x08 => "unknown device",
            0x09 => "resource error",
            0x0a => "request unavailable",
            0x0b => "invalid parameter value",
            0x0c => "wrong PIN code",
            _ => "unrecognised",
        };
        write!(f, "{:#04x} ({name})", self.0)
    }
}
