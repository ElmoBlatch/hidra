use std::time::{Duration, Instant};

use crate::error::{Error, Hidpp10Error, Hidpp20Error, Result};
use crate::features::{feature_set, root};
use crate::report::{Incoming, Request, SOFTWARE_ID};
use crate::transport::Transport;

const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1000);
const WAKE_RETRIES: u32 = 2;
const WAKE_DELAY: Duration = Duration::from_millis(300);

/// Feature type flags from `IFeatureSet.GetFeatureID`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FeatureFlags(pub u8);

impl FeatureFlags {
    pub fn obsolete(self) -> bool {
        self.0 & 0x80 != 0
    }
    pub fn hidden(self) -> bool {
        self.0 & 0x40 != 0
    }
    pub fn engineering(self) -> bool {
        self.0 & 0x20 != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureEntry {
    pub index: u8,
    pub page: u16,
    pub flags: FeatureFlags,
    pub version: u8,
}

/// An opened HID++ 2.0 device with its discovered feature table.
pub struct Device<T: Transport> {
    transport: T,
    device_index: u8,
    protocol: (u8, u8),
    features: Vec<FeatureEntry>,
    timeout: Duration,
}

impl<T: Transport> Device<T> {
    /// Ping the device, then enumerate its features through IFeatureSet.
    pub fn open(transport: T, device_index: u8) -> Result<Self> {
        Self::open_with_timeout(transport, device_index, DEFAULT_TIMEOUT)
    }

    pub fn open_with_timeout(transport: T, device_index: u8, timeout: Duration) -> Result<Self> {
        if !transport.supports_short() && !transport.supports_long() {
            return Err(Error::NoHidppReports);
        }
        let mut dev = Self { transport, device_index, protocol: (0, 0), features: Vec::new(), timeout };
        // A sleeping wireless mouse can drop the first request while the
        // receiver wakes it, so retry the initial ping a few times.
        let mut attempt = 0;
        dev.protocol = loop {
            match root::protocol_version(&mut dev) {
                Err(Error::Timeout { .. }) if attempt < WAKE_RETRIES => attempt += 1,
                // A receiver answers for a paired mouse that is not connected
                // right now with a HID++ 1.0 error; give it a moment to wake.
                Err(Error::Hidpp10(_)) if attempt < WAKE_RETRIES => {
                    attempt += 1;
                    std::thread::sleep(WAKE_DELAY);
                }
                Err(Error::Hidpp10(_)) => break Err(Error::NotConnected)?,
                other => break other?,
            }
        };
        let mut features = vec![FeatureEntry { index: 0, page: root::PAGE, flags: FeatureFlags(0), version: 0 }];
        if let Some(fs_index) = root::feature_index(&mut dev, feature_set::PAGE)? {
            let count = feature_set::count(&mut dev, fs_index)?;
            for i in 1..=count {
                features.push(feature_set::feature_id(&mut dev, fs_index, i)?);
            }
        }
        dev.features = features;
        Ok(dev)
    }

    pub fn device_index(&self) -> u8 {
        self.device_index
    }

    /// HID++ protocol version (major, minor).
    pub fn protocol_version(&self) -> (u8, u8) {
        self.protocol
    }

    pub fn features(&self) -> &[FeatureEntry] {
        &self.features
    }

    pub fn feature(&self, page: u16) -> Option<FeatureEntry> {
        self.features.iter().copied().find(|f| f.page == page)
    }

    pub(crate) fn require(&self, page: u16) -> Result<u8> {
        self.feature(page).map(|f| f.index).ok_or(Error::FeatureMissing(page))
    }

    pub(crate) fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    pub fn into_transport(self) -> T {
        self.transport
    }

    /// Send one request and wait for its matching reply payload.
    ///
    /// Crate-private on purpose: only typed, read-only feature getters may
    /// issue requests.
    pub(crate) fn call(&mut self, feature_index: u8, function: u8, params: &[u8]) -> Result<Vec<u8>> {
        let req = Request { device_index: self.device_index, feature_index, function, params: params.to_vec() };
        let bytes = req
            .encode(self.transport.supports_short(), self.transport.supports_long())
            .ok_or_else(|| Error::Malformed(format!("{} params do not fit any supported report", params.len())))?;
        self.transport.write_report(&bytes)?;

        let func_swid = (function << 4) | SOFTWARE_ID;
        let deadline = Instant::now() + self.timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let Some(buf) = self.transport.read_report(remaining)? else {
                return Err(Error::Timeout { feature_index, function });
            };
            let index_ok = |idx: u8| idx == self.device_index || self.device_index == crate::DEVICE_INDEX_DIRECT;
            match Incoming::parse(&buf) {
                Incoming::Message { device_index, feature_index: fi, function: f, software_id, payload }
                    if index_ok(device_index) && fi == feature_index && f == function && software_id == SOFTWARE_ID =>
                {
                    let mut payload = payload;
                    payload.resize(16, 0);
                    return Ok(payload);
                }
                Incoming::Error20 { device_index, feature_index: fi, function: f, software_id, code }
                    if index_ok(device_index) && fi == feature_index && f == function && software_id == SOFTWARE_ID =>
                {
                    return Err(Error::Hidpp20(Hidpp20Error(code)));
                }
                Incoming::Error10 { device_index, sub_id, address, code }
                    if index_ok(device_index) && sub_id == feature_index && address == func_swid =>
                {
                    return Err(Error::Hidpp10(Hidpp10Error(code)));
                }
                _ => {
                    if Instant::now() >= deadline {
                        return Err(Error::Timeout { feature_index, function });
                    }
                }
            }
        }
    }
}
