//! Find Logitech HID++ hidraw nodes through sysfs, without opening them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::LOGITECH_VID;

/// What a hidraw node is, as far as discovery can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A device that answers HID++ 2.0 at device index 0xff: a wired mouse, or
    /// the per-device hidraw node that `hid-logitech-dj` creates for a mouse
    /// paired to a receiver.
    Device,
    /// A receiver's own HID++ interface (Unifying / LIGHTSPEED / Bolt).
    Receiver,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidrawNode {
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
}

impl HidrawNode {
    pub fn model_name(&self) -> &'static str {
        model_name(self.product)
    }
}

/// Known receiver product IDs.
pub fn is_receiver_pid(pid: u16) -> bool {
    matches!(
        pid,
        0xc52b
            | 0xc52f
            | 0xc532
            | 0xc534
            | 0xc539
            | 0xc53a
            | 0xc53d
            | 0xc53f
            | 0xc541
            | 0xc545
            | 0xc547
            | 0xc548
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

/// Enumerate every Logitech hidraw node that declares HID++ short or long
/// reports in its report descriptor.
pub fn enumerate() -> io::Result<Vec<HidrawNode>> {
    let mut out = Vec::new();
    let dir = match fs::read_dir("/sys/class/hidraw") {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let sys = entry.path();
        let Ok(uevent) = fs::read_to_string(sys.join("device/uevent")) else {
            continue;
        };
        let Some((bus, vendor, product)) = parse_hid_id(&uevent) else {
            continue;
        };
        if vendor != LOGITECH_VID {
            continue;
        }
        let Ok(desc) = fs::read(sys.join("device/report_descriptor")) else {
            continue;
        };
        let ids = report_ids(&desc);
        let (short_reports, long_reports) = (ids.contains(&0x10), ids.contains(&0x11));
        if !short_reports && !long_reports {
            continue;
        }
        let hid_name = uevent
            .lines()
            .find_map(|l| l.strip_prefix("HID_NAME="))
            .unwrap_or("")
            .to_string();
        let via_receiver = receiver_parent(&sys);
        out.push(HidrawNode {
            dev_path: PathBuf::from("/dev").join(&name),
            vendor,
            product,
            bus,
            name: hid_name,
            kind: if is_receiver_pid(product) {
                NodeKind::Receiver
            } else {
                NodeKind::Device
            },
            short_reports,
            long_reports,
            via_receiver,
        });
    }
    out.sort_by(|a, b| a.dev_path.cmp(&b.dev_path));
    Ok(out)
}

/// HID++ short/long support for an opened `/dev/hidrawN` path.
pub(crate) fn hidpp_report_support_for_dev(dev: &Path) -> Option<(bool, bool)> {
    let name = dev.file_name()?;
    let desc = fs::read(
        Path::new("/sys/class/hidraw")
            .join(name)
            .join("device/report_descriptor"),
    )
    .ok()?;
    let ids = report_ids(&desc);
    Some((ids.contains(&0x10), ids.contains(&0x11)))
}

fn receiver_parent(sys: &Path) -> Option<u16> {
    // A DJ child's HID device directory sits inside the receiver's HID device
    // directory: .../0003:046D:C539.000A/0003:046D:407F.000B/hidraw/hidraw15
    let real = fs::canonicalize(sys.join("device")).ok()?;
    let parent = real.parent()?.file_name()?.to_string_lossy().to_string();
    let mut parts = parent.split(['.', ':']);
    let (_bus, vid, pid) = (parts.next()?, parts.next()?, parts.next()?);
    let (vid, pid) = (
        u16::from_str_radix(vid, 16).ok()?,
        u16::from_str_radix(pid, 16).ok()?,
    );
    (vid == LOGITECH_VID && is_receiver_pid(pid)).then_some(pid)
}

/// Parse `HID_ID=0003:0000046D:0000C332`.
fn parse_hid_id(uevent: &str) -> Option<(u16, u16, u16)> {
    let id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
    let mut it = id.split(':').map(|p| u32::from_str_radix(p, 16).ok());
    Some((it.next()?? as u16, it.next()?? as u16, it.next()?? as u16))
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
        if prefix & 0xfc == 0x84
            && size >= 1
            && let Some(&id) = desc.get(i + 1)
        {
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
    fn hid_id() {
        assert_eq!(
            parse_hid_id("DRIVER=x\nHID_ID=0003:0000046D:0000C332\n"),
            Some((3, 0x046d, 0xc332))
        );
    }

    #[test]
    fn descriptor_report_ids() {
        // Vendor usage page 0xff00, report ID 0x10, then report ID 0x11.
        let desc = [
            0x06, 0x00, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x10, 0x95, 0x06, 0x85, 0x11, 0xc0,
        ];
        assert_eq!(report_ids(&desc), vec![0x10, 0x11]);
    }
}
