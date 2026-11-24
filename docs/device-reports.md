# Sending a device report

Support for a Logitech mouse or keyboard starts with real data from that
device. A device report is one JSON file containing what your device says
about itself over HID++ 2.0, plus the raw traffic, which we replay in our test
suite. Collecting it **only reads** from the device; nothing on it is changed.

## 1. Get the tool

Either download `g502ctl-x86_64-linux` from the latest GitHub release
(`chmod +x` it and use it in place of `./target/release/g502ctl` below), or
build it. Building needs Rust (<https://rustup.rs>) and Linux.

```sh
git clone https://github.com/ElmoBlatch/hidra.git
cd hidra
cargo build --release -p g502ctl
```

## 2. Plug in the device

Wired, through its receiver, or over Bluetooth all work. If you can, send one
report per connection type. Wireless devices fall asleep: move the mouse or
press a key just before running the report.

## 3. Collect the report

Reading hidraw devices needs root, so the simplest way is a one-off `sudo`:

```sh
sudo ./target/release/g502ctl report --out reports
```

(If you plan to use the tool regularly, install the udev rule instead; see the
README. Then you can drop `sudo`.)

It prints one line per device it reads and the path of the report, e.g.
`reports/g502ctl-report-c332-1795517600.json`.

Optional: `--include-macros` adds the onboard sectors that hold macros. Only
use it if you know your macros contain nothing private: a macro can contain
text you typed.

## 4. Check it (optional)

```sh
./target/release/g502ctl info        # what the tool can read
./target/release/g502ctl profiles    # decoded onboard profiles
```

If something looks wrong (wrong DPI, buttons that do not match what the device
really does), say so in the issue; that is the most useful information of all.

## 5. Upload

Open a [Device report issue](https://github.com/ElmoBlatch/hidra/issues/new?template=device-report.yml)
and drag the JSON file into the
"Report file" box.

## What the report contains

- The list of Logitech HID nodes on your system: vendor/product IDs, bus,
  the device name the kernel reports, and whether it sits behind a receiver.
- Per device: HID++ version, feature list, name, firmware versions, DPI range
  and current DPI, polling rate, battery voltage, and the onboard-profile
  descriptor.
- Onboard-profile storage: the profile directory, your user profiles, and the
  factory profiles. Profile names you gave are included as stored.
- The raw HID++ requests and replies used to read all of the above.
- Your kernel version.

## What it leaves out

- The device unit ID (a per-unit serial number) is zeroed before it is written.
- Macro sectors, unless you pass `--include-macros`.
- Your hostname, user name, and anything not read from the device.

The file is plain JSON; read it before uploading if you want to be sure.
