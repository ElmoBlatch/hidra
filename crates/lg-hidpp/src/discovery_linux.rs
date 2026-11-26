//! Linux discovery: `/sys/class/hidraw`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{HidNode, NodeKind, is_receiver_pid, report_ids};
use crate::LOGITECH_VID;

/// Enumerate every Logitech hidraw node that declares HID++ short or long
/// reports in its report descriptor.
pub(super) fn enumerate() -> io::Result<Vec<HidNode>> {
    let mut out = Vec::new();
    let dir = match fs::read_dir("/sys/class/hidraw") {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let sys = entry.path();
        let Ok(uevent) = fs::read_to_string(sys.join("device/uevent")) else { continue };
        let Some((bus, vendor, product)) = parse_hid_id(&uevent) else { continue };
        if vendor != LOGITECH_VID {
            continue;
        }
        let Ok(desc) = fs::read(sys.join("device/report_descriptor")) else { continue };
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
        out.push(HidNode {
            dev_path: PathBuf::from("/dev").join(&name),
            vendor,
            product,
            bus,
            name: hid_name,
            kind: if is_receiver_pid(product) { NodeKind::Receiver } else { NodeKind::Device },
            short_reports,
            long_reports,
            via_receiver,
            device_index: crate::DEVICE_INDEX_DIRECT,
        });
    }
    out.sort_by(|a, b| a.dev_path.cmp(&b.dev_path));
    Ok(out)
}

/// HID++ short/long support for an opened `/dev/hidrawN` path.
pub(crate) fn hidpp_report_support_for_dev(dev: &Path) -> Option<(bool, bool)> {
    let name = dev.file_name()?;
    let desc = fs::read(Path::new("/sys/class/hidraw").join(name).join("device/report_descriptor")).ok()?;
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
    let (vid, pid) = (u16::from_str_radix(vid, 16).ok()?, u16::from_str_radix(pid, 16).ok()?);
    (vid == LOGITECH_VID && is_receiver_pid(pid)).then_some(pid)
}

/// Parse `HID_ID=0003:0000046D:0000C332`.
fn parse_hid_id(uevent: &str) -> Option<(u16, u16, u16)> {
    let id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
    let mut it = id.split(':').map(|p| u32::from_str_radix(p, 16).ok());
    Some((it.next()?? as u16, it.next()?? as u16, it.next()?? as u16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hid_id() {
        assert_eq!(parse_hid_id("DRIVER=x\nHID_ID=0003:0000046D:0000C332\n"), Some((3, 0x046d, 0xc332)));
    }
}
