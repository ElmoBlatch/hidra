//! Typed HID++ 2.0 feature access. Everything is a getter except
//! [`onboard_profiles_write`], the one guarded write path.

pub mod adjustable_dpi;
pub mod battery_voltage;
pub mod device_info;
pub mod device_name;
pub mod feature_set;
pub mod onboard_profiles;
pub mod onboard_profiles_write;
pub mod report_rate;
pub mod root;

/// Human-readable name for a feature page, where known.
pub fn page_name(page: u16) -> &'static str {
    match page {
        0x0000 => "Root",
        0x0001 => "Feature set",
        0x0002 => "Feature info",
        0x0003 => "Device information / firmware",
        0x0005 => "Device name and type",
        0x0007 => "Device friendly name",
        0x0020 => "Configuration change",
        0x00c2 => "DFU control (signed)",
        0x00c3 => "DFU control (secure)",
        0x00d0 => "DFU",
        0x1000 => "Battery status",
        0x1001 => "Battery voltage",
        0x1004 => "Unified battery",
        0x1300 => "LED software control",
        0x1802 => "Device reset",
        0x1803 => "GPIO access",
        0x1806 => "Configurable device properties",
        0x1811 => "OOB state",
        0x1830 => "Power modes",
        0x1890 => "RF test",
        0x18a1 => "LED test",
        0x18b1 => "Monitor mode",
        0x1d4b => "Wireless device status",
        0x1df3 => "Equad DJ debug info",
        0x1e00 => "Hidden features enable",
        0x1e22 => "SPI direct access",
        0x1eb0 => "TDE access",
        0x1f1f => "Firmware properties",
        0x2201 => "Adjustable DPI",
        0x8060 => "Adjustable report rate",
        0x8070 => "Color LED effects",
        0x8071 => "RGB effects",
        0x8090 => "Mode status",
        0x8100 => "Onboard profiles",
        0x8110 => "Mouse button spy",
        _ => "Unknown",
    }
}
