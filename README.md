# hidra

Linux tooling for Logitech mice (starting with the G502 family) that speaks
HID++ 2.0 directly over hidraw: a
desktop viewer, a command-line inspector, and a device-report collector.

![hidra showing a profile's DPI stages, buttons and lighting](docs/screenshot-dark.png)

**Status.** Reading works on any HID++ 2.0 device. Writing is deliberately
narrow: `hidractl set-dpi` can change the DPI stages of an existing onboard
profile on the G502 Proteus Spectrum, the one model where the write has been
verified on hardware. The desktop viewer is read-only for now. Protocol notes
are in [RESEARCH.md](RESEARCH.md).

Every write is compare-and-swap: it is refused unless the mouse still holds
the bytes the change was based on, it changes only the bytes of the field
being edited plus the checksum, it is verified by reading the sector back, and
the original contents are restored if anything goes wrong. A full backup is
taken first.

## Tested devices

| Device | USB ID | Connection | Read | Write |
| --- | --- | --- | --- | --- |
| G502 Proteus Spectrum | `046d:c332` | USB cable | Verified (firmware `U1 03.02.B0012`): info, three onboard profiles, factory profiles, full backup | DPI stages verified: changed, read back, restored byte-for-byte |
| G502 LIGHTSPEED | `046d:407f` | LIGHTSPEED receiver `046d:c539` | Verified (firmware `MPM 17.00.B0008`): info, battery, factory profile, full backup | Not enabled |

`hidractl info` and `hidractl report` work on any Logitech device that speaks
HID++ 2.0. Onboard-profile decoding covers profile formats 1–5 and has only
been checked on the two mice above.

**Have a different Logitech mouse or keyboard?** Send a device report so it can
be supported: [docs/device-reports.md](docs/device-reports.md). Collecting one
only reads from the device.

## Install

Needs Rust and Linux.

```sh
cargo build --release

# Let your user read the mice without root:
sudo install -m644 packaging/70-hidra.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=hidraw
```

The udev rule covers the two tested mice. For other devices, run `hidractl`
with `sudo`, or add the product ID to the rule.

## Desktop viewer

```sh
cargo run --release -p hidra            # dark theme
cargo run --release -p hidra -- --light
```

Shows each mouse's sensor DPI, polling rate, battery and active profile, and
for every onboard profile its DPI stages, button assignments (remapped buttons
are marked) and lighting. Firmware and storage details are under
**Device info…**.

Keys: `Ctrl+R` refresh, `Alt+Up/Down` switch device, `Ctrl+Tab` switch
profile, `Ctrl+I` device info, `Ctrl+Shift+L` light/dark, `Ctrl+Q` quit.

## Command line

```sh
hidractl list                    # Logitech HID++ nodes
hidractl info                    # features, firmware, DPI, polling rate, battery
hidractl profiles                # decoded onboard profiles
hidractl backup --out backups    # every onboard-memory sector, raw
hidractl report --out reports    # shareable device report (see docs/device-reports.md)

# Change a DPI stage (G502 Proteus Spectrum). Shows the change; add --yes to write.
hidractl set-dpi --profile 1 --stage 1 --dpi 3200

# Without hardware, from a capture or a report file:
hidractl --replay captures/ratbagd/g502-proteus-spectrum-c332.log profiles
```

## Layout

| Path | What |
| --- | --- |
| `crates/lg-hidpp` | HID++ framing, hidraw transport, sysfs discovery, typed feature getters, the guarded sector write, traffic recorder and replay transport |
| `crates/lg-profiles` | Onboard-profile codec: descriptor, directory, CRC, profile fields, button bindings, lighting, field-level edits |
| `crates/lg-snapshot` | Reads everything about a device into plain data for the viewer |
| `crates/hidractl` | Command-line tool |
| `crates/hidra` | Desktop viewer ([GPUI Kit](https://gpui-kit.com)) |
| `captures/ratbagd` | Recorded traffic from both tested mice; the test suite replays it |
| `re/ghidra-scripts` | Ghidra scripts used to study Logitech Gaming Software |
| `packaging/70-hidra.rules` | udev rule |

`cargo test` replays the captures through the whole stack, UI included, so it
needs no hardware.

## License

MIT or Apache-2.0, at your option. Not affiliated with Logitech.
