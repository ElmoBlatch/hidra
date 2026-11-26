//! Find Logitech HID++ devices without talking to them: through sysfs on
//! Linux, through the HID device list on Windows.

use std::io;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
#[path = "discovery_linux.rs"]
mod linux;
#[cfg(windows)]
#[path = "discovery_windows.rs"]
pub(crate) mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::hidpp_report_support_for_dev;

/// What a device node is, as far as discovery can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A device that answers HID++ 2.0 at device index 0xff: a wired mouse, or
    /// on Linux the per-device hidraw node that `hid-logitech-dj` creates for
    /// a mouse paired to a receiver.
    Device,
    /// A receiver's own HID++ interface (Unifying / LIGHTSPEED / Bolt).
    Receiver,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidNode {
    /// `/dev/hidrawN` on Linux; on Windows the path of one of the device's
    /// HID++ collections.
    pub dev_path: PathBuf,
    pub vendor: u16,
    pub product: u16,
    pub bus: u16,
    pub name: String,
    pub kind: NodeKind,
    pub short_reports: bool,
    pub long_reports: bool,
    /// Product ID of the receiver this node sits under, for paired devices.
    pub via_receiver: Option<u16>,
    /// The HID++ device index that reaches the device through `dev_path`:
    /// 0xff for the node's own device, or a receiver slot.
    pub device_index: u8,
}

impl HidNode {
    pub fn model_name(&self) -> &'static str {
        model_name(self.product)
    }
}

/// Known receiver product IDs.
pub fn is_receiver_pid(pid: u16) -> bool {
    matches!(
        pid,
        0xc52b | 0xc52f | 0xc532 | 0xc534 | 0xc539 | 0xc53a | 0xc53d | 0xc53f | 0xc541 | 0xc545 | 0xc547 | 0xc548
    )
}

/// Model name for the devices this project targets.
pub fn model_name(pid: u16) -> &'static str {
    match pid {
        0xc332 => "G502 Proteus Spectrum",
        0xc08d | 0x407f => "G502 LIGHTSPEED",
        0xc539 => "LIGHTSPEED receiver",
        _ if is_receiver_pid(pid) => "Logitech receiver",
        _ => "Logitech HID++ device",
    }
}

/// Enumerate every Logitech HID device that carries HID++ short or long
/// reports, without talking to it.
pub fn enumerate() -> io::Result<Vec<HidNode>> {
    #[cfg(target_os = "linux")]
    return linux::enumerate();
    #[cfg(windows)]
    return windows::enumerate();
    #[cfg(not(any(target_os = "linux", windows)))]
    Ok(Vec::new())
}

/// True when mice behind a receiver have to be reached through the
/// receiver's slots: always on Windows, and on Linux when
/// `HIDRA_RECEIVER_SLOTS=1` asks for it (for diagnosis; the kernel's
/// per-mouse nodes are used otherwise).
pub fn uses_receiver_slots() -> bool {
    cfg!(windows) || std::env::var_os("HIDRA_RECEIVER_SLOTS").is_some_and(|v| v == "1")
}

/// Every device that can be asked for its settings: the device nodes from
/// [`enumerate`], and where [`uses_receiver_slots`] says so, each device
/// paired to a receiver. Finding paired devices reads the receiver's pairing
/// registers; a receiver that can't be opened is skipped.
pub fn devices() -> io::Result<Vec<HidNode>> {
    let mut out = Vec::new();
    for node in enumerate()? {
        match node.kind {
            NodeKind::Device => out.push(node),
            NodeKind::Receiver if uses_receiver_slots() => out.extend(paired_nodes(&node)),
            NodeKind::Receiver => {}
        }
    }
    Ok(out)
}

/// A node per device paired to `receiver`.
fn paired_nodes(receiver: &HidNode) -> Vec<HidNode> {
    let Ok(mut transport) = crate::HidTransport::open(&receiver.dev_path) else { return Vec::new() };
    let paired = crate::receiver::paired_devices(&mut transport, receiver.product).unwrap_or_default();
    paired
        .into_iter()
        .map(|p| HidNode {
            product: p.wpid,
            name: format!("{} (slot {})", model_name(p.wpid), p.slot),
            kind: NodeKind::Device,
            via_receiver: Some(receiver.product),
            device_index: p.slot,
            ..receiver.clone()
        })
        .collect()
}

/// Which HID++ reports a Windows top-level collection carries, from its usage
/// page and usage: (short, long).
///
/// Logitech declares HID++ either on vendor page 0xff00 with usage 1 for the
/// short report and usage 2 for the long one, or on page 0xff43 with the
/// report kinds as bits of the usage's low byte (1 short, 2 long).
#[cfg(any(windows, test))]
pub(crate) fn hidpp_usage(usage_page: u16, usage: u16) -> (bool, bool) {
    match usage_page {
        0xff00 => (usage == 1, usage == 2),
        0xff43 => (usage & 0x01 != 0, usage & 0x02 != 0),
        _ => (false, false),
    }
}

/// What the collections of one Windows HID interface have in common, taken
/// from a device path such as
/// `\\?\hid#vid_046d&pid_c332&mi_01&col02#8&1a2b3c4d&0&0001#{4d1e55b2-…}`:
/// the hardware ID without its `&colNN` part, and the instance ID without its
/// last, per-collection segment.
#[cfg(any(windows, test))]
pub(crate) fn windows_group_key(path: &str) -> String {
    let path = path.to_ascii_lowercase();
    let mut parts = path.split('#');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(_), Some(hardware), Some(instance)) if hardware.contains("&col") => {
            let hardware = hardware.split("&col").next().unwrap_or(hardware);
            let instance = instance.rsplit_once('&').map_or(instance, |(shared, _)| shared);
            format!("{hardware}#{instance}")
        }
        _ => path,
    }
}

/// Collect Report ID items (global tag 8) from a HID report descriptor.
pub fn report_ids(desc: &[u8]) -> Vec<u8> {
    let mut ids = Vec::new();
    let mut i = 0;
    while i < desc.len() {
        let prefix = desc[i];
        if prefix == 0xfe {
            // Long item: fe, size, tag, data...
            let size = *desc.get(i + 1).unwrap_or(&0) as usize;
            i += 3 + size;
            continue;
        }
        let size = match prefix & 0x03 {
            3 => 4,
            n => n as usize,
        };
        if prefix & 0xfc == 0x84 && size >= 1
            && let Some(&id) = desc.get(i + 1) {
                ids.push(id);
            }
        i += 1 + size;
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_collections_of_one_interface_share_a_key() {
        let short = r"\\?\HID#VID_046D&PID_C332&MI_01&Col01#8&1a2b3c4d&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        let long = r"\\?\hid#vid_046d&pid_c332&mi_01&col02#8&1a2b3c4d&0&0001#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        let other_mouse = r"\\?\hid#vid_046d&pid_c332&mi_01&col02#8&99999999&0&0001#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        let other_interface = r"\\?\hid#vid_046d&pid_c332&mi_00#8&2b3c4d5e&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        assert_eq!(windows_group_key(short), windows_group_key(long));
        assert_eq!(windows_group_key(short), "vid_046d&pid_c332&mi_01#8&1a2b3c4d&0");
        assert_ne!(windows_group_key(long), windows_group_key(other_mouse));
        assert_ne!(windows_group_key(long), windows_group_key(other_interface));
    }

    #[test]
    fn hidpp_usages() {
        assert_eq!(hidpp_usage(0xff00, 1), (true, false));
        assert_eq!(hidpp_usage(0xff00, 2), (false, true));
        assert_eq!(hidpp_usage(0xff43, 0x0202), (false, true));
        assert_eq!(hidpp_usage(0x0001, 2), (false, false), "a generic-desktop mouse collection");
    }

    #[test]
    fn descriptor_report_ids() {
        // Vendor usage page 0xff00, report ID 0x10, then report ID 0x11.
        let desc = [0x06, 0x00, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x10, 0x95, 0x06, 0x85, 0x11, 0xc0];
        assert_eq!(report_ids(&desc), vec![0x10, 0x11]);
    }
}
