//! Windows discovery: the HID device list.
//!
//! Each HID++ collection is its own device path; the collections of one
//! interface are reported as one node. A receiver is listed as a receiver:
//! mice paired to it are not found yet, because Windows has no per-mouse node
//! for them and finding them means asking the receiver.

use std::ffi::CString;
use std::io;
use std::path::PathBuf;

use hidapi::{BusType, HidApi};

use super::{HidNode, NodeKind, hidpp_usage, is_receiver_pid};
use crate::LOGITECH_VID;

pub(crate) use super::windows_group_key as group_key;

/// One HID++ top-level collection.
pub(crate) struct Collection {
    pub path: CString,
    /// Shared by the collections of one interface.
    pub key: String,
    pub product: u16,
    pub bus: u16,
    pub name: String,
    pub short: bool,
    pub long: bool,
}

pub(crate) fn io_error(error: hidapi::HidError) -> io::Error {
    io::Error::other(error.to_string())
}

pub(crate) fn api() -> io::Result<HidApi> {
    HidApi::new().map_err(io_error)
}

/// Every Logitech collection that carries HID++ reports, in path order.
pub(crate) fn collections(api: &HidApi) -> Vec<Collection> {
    let mut out: Vec<Collection> = api
        .device_list()
        .filter(|info| info.vendor_id() == LOGITECH_VID)
        .filter_map(|info| {
            let (short, long) = hidpp_usage(info.usage_page(), info.usage());
            (short || long).then(|| Collection {
                path: info.path().to_owned(),
                key: group_key(&info.path().to_string_lossy()),
                product: info.product_id(),
                // The numbers Linux uses, so reports read the same on both.
                bus: match info.bus_type() {
                    BusType::Usb => 0x03,
                    BusType::Bluetooth => 0x05,
                    _ => 0,
                },
                name: info.product_string().unwrap_or("").to_string(),
                short,
                long,
            })
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

pub(super) fn enumerate() -> io::Result<Vec<HidNode>> {
    let api = api()?;
    let mut nodes: Vec<(String, HidNode)> = Vec::new();
    for c in collections(&api) {
        match nodes.iter_mut().find(|(key, _)| *key == c.key) {
            Some((_, node)) => {
                node.short_reports |= c.short;
                node.long_reports |= c.long;
            }
            None => nodes.push((
                c.key,
                HidNode {
                    dev_path: PathBuf::from(c.path.to_string_lossy().into_owned()),
                    vendor: LOGITECH_VID,
                    product: c.product,
                    bus: c.bus,
                    name: c.name,
                    kind: if is_receiver_pid(c.product) { NodeKind::Receiver } else { NodeKind::Device },
                    short_reports: c.short,
                    long_reports: c.long,
                    via_receiver: None,
                    device_index: crate::DEVICE_INDEX_DIRECT,
                },
            )),
        }
    }
    Ok(nodes.into_iter().map(|(_, node)| node).collect())
}
