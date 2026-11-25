# Logitech G502 Proteus Spectrum on Linux: RE notes

## Goal

Understand how to edit the Logitech G502 Proteus Spectrum (`046d:c332`) DPI/profile settings on Linux while preserving the existing F24 push-to-talk button mapping. No device settings were changed during this investigation.

## Current status (2026-11-24)

- Rust workspace in place (`crates/lg-hidpp`, `crates/lg-profiles`, `crates/hidractl`); read-only only. `cargo test` replays both captures and reproduces every captured sector CRC.
- **Correction:** profile 0 button 5 is bytes `80 02 00 73`, a plain HID keyboard binding (usage 0x73 = F24), not a macro. ratbagctl presents it as a macro. Button 5 is the DPI-shift (sniper) position; profile 1 still has `90 07` (DPI shift) there.
- **Hypothesis for libratbag #1798:** without `INDEX_OFFSET`, libratbag treats sector 2 (logical profile 1) as active, so an "active profile" DPI edit would land in the wrong profile. That would explain both reported symptoms (cursor speed unchanged, buttons apparently changed). PR #1799 is the fix for exactly this ID. To be confirmed by the first verified write.
- **Live read-only verification (2026-11-24, `hidractl` over hidraw with the udev rule):** both mice read without root. Live profile bytes match the ratbagd captures, and two consecutive full backups were byte-identical. Backups: `backups/` (gitignored) and a copy in `~/archive/logitech-backups/`.
  - Proteus Spectrum: name "Tunable RGB Gaming Mouse G502", firmware application `U1 03.02.B0012`, bootloader `BOT 14.00.B0007`. Sectors 0–3 have valid CRCs; sectors 4–15 read as all zeros (unused macro space, no macros stored); ROM profiles 0x0101–0x0103 read fine.
  - G502 LIGHTSPEED: firmware application `MPM 17.00.B0008`, bootloader `BOT 92.00.B0008`, plus a third entity `00.00.B00a9`; all 16 user sectors erased; ROM profile 0x0101 active. The first ping after idle can time out while the receiver wakes the mouse; `Device::open` now retries it.
- **First verified write (2026-11-24, G502 Proteus Spectrum, firmware U1 03.02.B0012):** `hidractl set-dpi` changed profile 1 stage 1 from 3350 to 3200 with `BeginWrite(sector, 0, 256)`, sixteen 16-byte `WriteData` calls and `EndWrite`, in onboard mode, without touching the directory. Read-back matched the intended bytes exactly (only offsets 0x03, 0x04 and the CRC changed; CRC became `a2af`), the F24 binding, the directory and the other profiles were unchanged, and the active profile stayed at address 1. Writing 3350 back returned all 19 sectors to byte-identical with the original backup (CRC `1c32`). The device accepts a byte-patched sector write; no directory rewrite or mode switch is needed.
- libratbag, by contrast, re-encodes each profile from its own model and rewrites every enabled profile plus the directory on commit (`hidpp20_onboard_profiles_commit`), which together with the missing `INDEX_OFFSET` is a plausible source of the button changes reported in #1798.
- **Edited stage is used by the device (2026-11-24):** after stage 1 was written as 3200 (and left that way), stepping down to it with the DPI-down button put the sensor at 3200 (`0x2201` read-back), with no replug or profile switch. The firmware reads stage values from the rewritten sector when a stage is selected.
- **In-use stage and live polling (2026-11-25):** editing the DPI stage the mouse is currently on changes the sensor immediately (`0x2201` read 11950 right after stage 4 was written as 11950, and 12000 after it was restored), and a polling-rate edit on the active profile is reported by `0x8060` at once. No profile switch or separate "apply" request is needed.
- **Other fields verified by write, read-back and restore (2026-11-25):** a button binding (one byte for a keyboard usage change), the default stage (offset 0x01) and the polling rate (offset 0x00). After the series the device was byte-identical to the backup taken before it.
- **Directory format for a first save** (libratbag `write_dict`): one `[00, n, enabled, 00]` entry per profile slot, then `ff ff 00 00`, `ff` padding, CRC. `SetActiveProfile` (function 3) takes the same 16-bit address `GetActiveProfile` reports. `hidractl init-profiles` builds this for a blank device from a copy of its factory profile; it passes on a simulated LIGHTSPEED and ran on the real one on 2026-11-25 (see below).
- **LIGHTSPEED first save, verified on hardware 2026-11-25** (firmware `MPM 17.00.B0008`). Starting from blank user memory running ROM profile `0x0101`: wrote a copy of the ROM profile to sector `0x0001` (CRC `3e23`), wrote the directory to sector `0x0000`, then `SetActiveProfile(0x0001)`. All three read back correctly, the mouse stayed in onboard mode and the live sensor DPI (6400, stage 5) did not change. A following DPI stage edit (stage 1, 400 → 450 → 400) changed only bytes `0x03`, `0xfd`, `0xfe`, confirming the 255-byte sector with its CRC in the last two bytes; the sector returned to CRC `3e23`. The pre-setup backup is the way back to factory-blank memory (`hidractl restore`).

## Findings

- The mouse is visible to `libratbag`/`ratbagd` 0.18 as `Logitech Gaming Mouse G502`, with 3 onboard profiles. The installed `ratbagctl` can read its profiles; Piper is not installed in this environment.
- The current profile 0 readout includes four enabled DPI stages (3350, 5150, 8500, 12000) and a disabled 50 DPI slot. Button 5 is mapped to `KEY_F24`. This was observed from `ratbagctl`; it does not establish whether writes through libratbag are reliable.
- The supplied G HUB installer (`lghub_installer.exe`) is G HUB 2026.7.4691.0 (SHA-256 `883399cfc1d45a38bc70d584dc4349fe031cf2402f8b17dc78812b3bb674e06c`). It is a bootstrap/setup package. It was inspected statically and not executed.
- The G502 Proteus Spectrum is a better match for Logitech Gaming Software (LGS). Logitech's support listing for the exact mouse documents LGS as its legacy software; LGS 8.78.129 release notes specifically added G502 Proteus Spectrum support. The official download page's old 8.78.129 link is now unavailable. An official LGS 9.04.49 x64 installer was therefore inspected statically (SHA-256 `6db9f6442d46fbde0953f8bfbc36bd714e5e31a62f927563594cafd60724d3e0`). The installer was not run or installed.
- The extracted LGS `G502RGB` device manifest matches VID/PID `046d:c332`. It describes 1000 Hz and a four-stage DPI table of 1200, 2400, 3200, and 6400; these are software manifest defaults, not a readback from this mouse. The manifest also labels the device as supporting onboard profiles. Button IDs 6/7/8 are assigned DPI shift/down/up functions in its host-mode metadata.
- LGS `LCore.exe` imports Windows HID and WinUSB APIs (`HidD_GetFeature`, `HidD_SetFeature`, `WinUsb_WritePipe`, etc.) and contains strings related to onboard profile save/activation. These are static indicators that it supports device communication and onboard profile UI. They do not show which API path writes this specific device's DPI data or prove the write sequence.
- A libratbag report filed Jan. 7, 2026 describes the same G502 Proteus Spectrum USB ID and ratbagd 0.18: `ratbagctl` reports changed DPI after a write, but physical cursor speed does not change. This is a closely matching reported defect, not a reproduction on this machine. The report also describes button mappings unexpectedly changing after DPI edits.
- The protocol is Logitech HID++ 2.0. libratbag identifies onboard profiles as feature page `0x8100`; the adjustable live-DPI feature is `0x2201`. Those are distinct operations. The existing `F24` mapping is visible in the readback, but no edit was attempted.
- A read-only query on 2026-11-24 initially showed profile 1 as active, but raw HID++ plus the profile directory corrects that interpretation: the device reports active page 1, and page/sector 1 belongs to logical profile 0. The installed `/usr/share/libratbag/logitech-g502-proteus-spectrum.device` lacks the `INDEX_OFFSET` quirk, so ratbagctl marks profile 1 active one slot too far forward. Upstream merged `INDEX_OFFSET` for this exact G502 Proteus Spectrum in PR #1799; its current device file includes the quirk. Thus the current active profile is profile 0, which contains the F24 mapping. Physical indicator not checked.

## Sources

- [Logitech G502 Proteus Spectrum support page](https://support.logi.com/hc/en-nz/articles/360024636953-G502-Proteus-Spectrum-RGB-Tunable-Gaming-Mouse)
- [Logitech LGS support/download page](https://support.logi.com/hc/en-ch/articles/360025130234-Logitech-Gaming-Software)
- [Official LGS 9.04.49 x64 installer](https://download01.logi.com/web/ftp/pub/techsupport/gaming/LGS_9.04.49_x64_Logitech.exe)
- [libratbag issue #1798: G502 Proteus Spectrum settings written incorrectly](https://github.com/libratbag/libratbag/issues/1798)
- [libratbag HID++ 2.0 protocol declarations](https://github.com/libratbag/libratbag/blob/master/src/hidpp20.h)
- [libratbag HID++ 2.0 profile parser and sector writer](https://github.com/libratbag/libratbag/blob/master/src/hidpp20.c)
- [libratbag HID++ 2.0 driver](https://github.com/libratbag/libratbag/blob/master/src/driver-hidpp20.c)
- [libratbag G502 Proteus Spectrum device entry](https://github.com/libratbag/libratbag/blob/master/data/devices/logitech-g502-proteus-spectrum.device)
- [libratbag PR #1799: active-profile index offset fix](https://github.com/libratbag/libratbag/pull/1799)
- [Logitech's HID++ protocol documentation](https://github.com/Logitech/cpg-docs/tree/master/hidpp20)
- [Logitech firmware update instructions](https://support.logi.com/hc/en-au/articles/6526066717591-How-do-I-update-my-mouse-firmware)
- [OpenMouse HID++ onboard-profile notes](https://github.com/OpenMouse-Project/mouse-protocol/blob/main/docs/logitech-onboard-profiles.md) (community-derived format notes; not hardware validation for this specific mouse)

## Next useful RE step

Read-only profile data and sector CRCs now match the libratbag profile layout. Next, compare the LGS and libratbag write implementations at the command/field level and determine why libratbag writes were reported unreliable on this exact USB ID. Do not send writes until we can preserve and verify every byte outside the intended DPI fields.

## Ghidra availability

Ghidra 12.2 is installed at `/opt/ghidra`. The REA service detects Java 21 and rejects this version of Ghidra, but direct headless analysis works with `/usr/lib/jvm/java-27-openjdk` selected explicitly. Helper scripts are in `re/ghidra-scripts/`.

## Ghidra pass (LGS 9.04.49 `LCore.exe`)

Ghidra 12.2 auto-analysis completed successfully when launched directly with Java 27 and a temporary user directory. The scripts are in `re/ghidra-scripts/`; the Ghidra project and decompiled output are not published (they contain Logitech code).

Observed in decompiled code:

- `FUN_1406d9150` calls `HidD_SetFeature`. It refuses a payload larger than the device's reported feature-report length, zero-pads shorter payloads to that length, copies the caller's bytes at offset zero, then sends the padded report.
- `FUN_1406d9240` creates a buffer of the same reported length, writes a caller-supplied report ID into byte 0, and calls `HidD_GetFeature`. `FUN_1406d9320` also calls `HidD_GetFeature` on a caller-provided buffer.
- The HidD routines sit in a virtual dispatch table (`.rdata` around VA `0x140d3b158`), which explains why ordinary direct-call xrefs do not connect them to profile handlers.
- The WinUSB route is separate: `FUN_14043a400` zero-fills a transfer buffer, copies the provided bytes, then calls `WinUsb_WritePipe`; it accepts normal completion or `ERROR_IO_PENDING`. Its immediate callers include short fixed payloads, so this route is not yet identified as the G502 profile writer.
- Profile-oriented code references `cFeature8100OnboardProfile(s)` and adjustable-DPI code references `cFeature2201AdjustableDpi`. For example, `FUN_1402fe230` copies selected fields from a profile-feature object into an internal record, including five 16-bit entries at offsets `+3` through `+0x0c` and several one-byte fields. The field meanings have not yet been confirmed, and this adapter function is not itself proof of the wire layout.

The profile/UI adapters, generic HID transport, and `0x8100` operation layer have been found. Pass 3 below connects feature dispatch to the generic HID++ report handler. The live profile-sector format is decoded below; actual write acceptance on this G502 remains unverified.

## Ghidra pass 2: HID++ 0x8100 operation layer

The `Feature8100OnboardProfiles` implementation in `LCore.exe` reveals operation selectors passed into its generic feature dispatcher. This is direct static evidence from the LGS binary; it does not yet prove that every G502 firmware revision supports each operation identically.

| Selector | LGS method | Observed request/response handling |
| --- | --- | --- |
| `0` | `GetOnboardProfilesInfo` | Empty request; expects at least 10 response bytes, parses bytes 0–6, a 16-bit value at 7–8, and two flag fields from byte 9. |
| `1` | `SetOnboardMode` | One-byte request. |
| `2` | `IsOnboardMode` | Empty request; reads byte 0 of response. |
| `3` | `SetActiveProfile` | One 16-bit request value. |
| `4` | `GetActiveProfile` | Empty request; reads a 16-bit response value. |
| `5` | `ReadData` | Two 16-bit request values (sector and address arguments); expects at least 16 response bytes and returns them. |
| `6` | `BeginWrite` | Three 16-bit request values. |
| `7` | `WriteData` | Request data must be at most 16 bytes; LGS pads shorter requests to exactly 16 bytes. On success it reads a 16-bit result from the response. |
| `8` | `EndWrite` | Empty request. |
| `10` | `GetCRC` | One 16-bit request value. |
| `11` / `12` | `GetActiveProfileResolution` / `SetActiveProfileResolution` | Getter returns byte 0; setter sends one byte. |

This gives a concrete Linux implementation target: preserve the existing button/profile data, read the profile sectors, change only the desired DPI bytes, and reproduce the observed begin/write/end operations. At this stage the sector format and CRC have been decoded and checked against readback. The device's acceptance of a write and the reason for the reported libratbag failure remain unverified for `046d:c332`.

## Ghidra pass 3: HID++ report dispatch

`FUN_1409bc0a0` is LGS's generic device read handler. It ignores reports shorter than seven bytes, identifies HID++ 2.0 reports, reads the feature index and function/SWID nibbles from the report, and dispatches notifications or replies to the matching feature / pending command. A pending-command mismatch falls through to feature-response decoding; unknown feature indexes are ignored. This ties the 0x8100 methods to the common HID++ 2.0 response path at the software layer.

This is static analysis of LGS 9.04.49; it does not by itself reveal the mouse's live feature index or profile data. The read-only raw capture below supplies those replies and validates the CRC convention. Whether this firmware accepts the profile write sequence remains unverified.

## G502 raw HID++ capture (read-only)

Captured ratbagd 0.18 startup traffic with `ratbagd --verbose=raw`; the log is `captures/ratbagd/g502-proteus-spectrum-c332.log`. The daemon discovered onboard-profiles feature page `0x8100` at feature index `0x0e`. Its GetActiveProfile reply contains `[0x00, 0x01]`, which is the 1-based user-profile page/sector. The directory maps that page to logical profile 0. The installed ratbag data file lacks the `INDEX_OFFSET` quirk and therefore mislabels it as logical profile 1. Upstream PR #1799 added this quirk for the exact G502 Proteus Spectrum model and its current device file includes it. GetOnboardProfilesInfo reports three profiles, matching `ratbagctl info`.

The log includes successful ReadData replies for profile storage. The data bytes match the live profile values: profile 0's record has 3350, 5150, 8500, and 12000 DPI and button 5 F24; profile 1 has 400, 800, 1600, and 2400 DPI; profile 2 begins at 2400 DPI and has Ctrl+C / Ctrl+V macros on buttons 6 and 7 per `ratbagctl info`. Profile 0 is active according to the 1-based page response and directory mapping, despite installed ratbagctl showing profile 1 active. The capture ended during later feature traffic after the profile sectors had been read.

The profile descriptor decodes as memory model `0x01`, profile format `0x02`, macro format `0x01`, three user profiles, eleven buttons, sixteen 256-byte sectors, and three ROM profiles. Directory sector 0 maps profile indexes 0, 1, and 2 to sectors 1, 2, and 3; all three directory entries are enabled. All four sectors (directory plus three profiles) were fully captured.

The live profile record format matches libratbag's 256-byte `hidpp20_internal_profile`: byte 0 is report-rate divisor, bytes 1 and 2 are DPI indexes, and five little-endian 16-bit DPI values start at byte offset 3. The captured records match `ratbagctl` exactly: sector 1/profile 0 has `[3350, 5150, 8500, 12000, 0]`, sector 2/profile 1 has `[400, 800, 1600, 2400, 0]`, and sector 3/profile 2 has `[2400, 0, 0, 0, 0]`. Profile 0's UTF-16LE name is `Default`; its button 5 remains the F24 macro in the live readback.

The final two bytes of each 256-byte sector are a big-endian CRC-CCITT over the first 254 bytes. Computing CRC-CCITT with initial value `0xffff` reproduces the captured values: directory sector 0 `0xda84`, profile sector 1 `0x1c32`, sector 2 `0x59d7`, and sector 3 `0x78f5`. This agrees with libratbag's source, which validates/stores the CRC at `sector_size - 2` and reads each profile's five DPI words from offset 3.

The startup traffic issued feature discovery, profile-info, active-profile, and sector-read requests. No `BeginWrite` (function 6), `WriteData` (function 7), or `EndWrite` (function 8) requests appear in the capture. No profile settings were changed. The payload and checksum format are now grounded in the live device; write acceptance and the reported libratbag write defect remain untested, so do not send profile writes yet.

Libratbag's current open-source parser independently documents the same layout: profile directory entries point to sectors `1..N`, profiles are 256 bytes, the five DPI values are little-endian words at offsets `3..12`, and the final two bytes store big-endian CRC-CCITT over the preceding 254 bytes. Its writer starts a sector transaction with sector/address/count, sends each 16-byte chunk, then ends the transaction. The matching readback CRCs above verify the parser's CRC convention against this mouse. This documents a Linux write implementation, but does not prove that a DPI-only sector write is safe on this unit or explain the reported G502 write defect.

## Device capability inventory for a Rust implementation

Raw root feature discovery sees HID++ protocol v4.2 and 20 feature pages. Features exposed in ratbagd's model for this mouse:

| Capability | Page / live value | Notes |
| --- | --- | --- |
| Adjustable DPI | `0x2201`, feature index `0x0a`; min 50, max 12000, step 50 | One sensor; live sensor DPI and per-profile DPI stages are separate data paths. |
| Polling rate | `0x8060`, feature index `0x0d`; 125, 250, 500, 1000 Hz | The captured unit is at 1000 Hz in profile 0 and 1, 500 Hz in profile 2. |
| Onboard profiles | `0x8100`, feature index `0x0e`; 3 user profiles, 11 buttons | 5 DPI slots/profile, profile-local button assignments/macros, names, rates, and LEDs. |
| RGB effects | `0x8070`, feature index `0x02`; 2 zones, 4 effects | Profiles currently use cycle, breathing, or on; LEDs are also stored in profile sectors. |
| LED software control | `0x1300`, feature index `0x05` | Exposed alongside the RGB effect page. |
| Button spy | `0x8110`, feature index `0x0f` | Reports button events; it is not the onboard button-map editor. |

The profile assignment data and F24 macro are inside `0x8100` profile records, while DPI capability and direct sensor DPI are exposed separately through `0x2201`. The current user goal is therefore a G502-specific configurator with a G HUB-like editor, not full G HUB feature parity for every Logitech device. Logitech's support page lists LGS for this G502 generation; G HUB is the successor for newer devices.

Local implementation environment: Rust/Cargo 1.99, GTK 4.24, hidapi 0.15, ratbagd 0.18. Piper is not installed. (At the time of writing, the workspace held only this note; see Current status.)

## Firmware version and update research

The captured feature table includes Device Information / Firmware Version `0x0003` at index `0x03`. LGS's `LGHppFrc.dll` contains `Feature0003FirmwareInfo` methods for firmware-entity count, per-entity firmware version/build, PID, and model ID. HID++ source documentation describes read-only `GetEntityCount` and `GetFwInfo` calls on this feature. A version readout is therefore a realistic local feature; the live query has not yet been issued.

The G HUB installer contains the hostname `updates.ghub.logitechg.com`, but static strings alone do not establish that this is a device-firmware catalog or identify a read-only update endpoint/schema. Logitech's public firmware-update instructions direct users through supported software and list current firmware changes for selected newer devices; they do not document a public device-update API. Treat a firmware-version display as feasible and an online “check for update” as an optional follow-up after the endpoint and model coverage are verified. No firmware download or update has been attempted.

## Second target: G502 LIGHTSPEED

The requested second model is the original G502 LIGHTSPEED (not G502 X LIGHTSPEED). Its product identifiers differ by connection path: Logitech lists `046d:c08d` for the wired USB mouse and `046d:c539` for the LIGHTSPEED receiver; Linux exposes the paired wireless mouse through HID++ device ID `046d:407f`. Logitech lists 25,600 max DPI, onboard memory, an RGB G logo and DPI/power indicator, and 11 programmable buttons. Its support page says it supports up to five DPI settings and five onboard profiles, with report rate, performance/endurance modes, and tracking behavior configurable in G HUB. This materially expands the profile model from the three profiles observed on the Proteus Spectrum.

Current upstream and installed libratbag name this family “G502 Hero Wireless” and match `046d:407f` plus `046d:c08d`; the device entry selects the generic HID++ 2.0 driver and `INDEX_OFFSET`. The receiver PID `c539` is not the profile device ID. The early `ratbagctl list` query only showed the Proteus; after a raw daemon discovery and service restart, ratbagctl sees both mice. The wireless mouse appears as `raging-hutia`, `usb:046d:407f:0`, with 11 buttons, two LEDs, and five profiles.

For a combined tool, keep the Rust HID++ transport and profile editor shared, but use per-device capability discovery and model-specific profile codecs/quirks. Reuse of the Proteus Spectrum's exact 256-byte profile layout on the LIGHTSPEED must be verified from a read-only capture before writing. The wireless target also needs explicit receiver routing/slot discovery and a connection-state UX. First implementation scope should include both the c08d cable path and the c539 receiver path if feasible, with the wireless path validated on the actual laptop before any writes. The LIGHTSPEED's extra performance/endurance/tracking options should be exposed only when its live feature discovery confirms them; do not silently treat them as the wired G502's features.

Additional sources:

- [Logitech G502 LIGHTSPEED technical specifications](https://support.logi.com/hc/en-us/articles/360023303914-G502-LIGHTSPEED-Wireless-Gaming-Mouse-Technical-Specifications)
- [Logitech G502 LIGHTSPEED onboard profile and DPI indicators](https://support.logi.com/hc/en-us/articles/360023192654-G502-LIGHTSPEED-DPI-Onboard-Profile-Indicators)
- [libratbag G502 Hero Wireless device entry](https://github.com/libratbag/libratbag/blob/master/data/devices/logitech-g502-hero-wireless.device)
- [libratbag issue #1150: G502 LIGHTSPEED receiver/device detection](https://github.com/libratbag/libratbag/issues/1150)

## UI framework preference

The user prefers [Longbridge GPUI Kit](https://github.com/longbridge/gpui-kit) over GTK. Its current documentation describes `gpui-kit` as a Rust desktop framework supporting Linux, macOS, and Windows; it provides styled components on top of GPUI and publishes a single `gpui-kit` application dependency. The website currently documents v0.11.0. Plan the app UI around GPUI Kit, using its styled component layer unless implementation work uncovers a blocking Linux build/runtime issue. The HID++ device layer remains independent of UI framework.

## Both mice connected on this PC (read-only capture)

On 2026-11-24, `lsusb` sees the Proteus Spectrum as `046d:c332` and the LIGHTSPEED receiver as `046d:c539`. Sysfs exposes paired mouse HID++ device `046d:407f` under the receiver at `/dev/hidraw15` (receiver interface 2). Raw `ratbagd --verbose=raw` capture: `captures/ratbagd/both-mice-lightspeed-407f.log` (69,632 bytes). After restoring the service, `systemctl is-active ratbagd` reports `active` and `ratbagctl list` shows both mice. No device configuration was changed.

The LIGHTSPEED identifies as HID++ v4.2 with 30 features. Relevant pages include Device Info `0x0003` at feature index `0x02`, Wireless Device Status `0x1d4b` at `0x04`, Battery Voltage `0x1001` at `0x06`, RGB/color effects `0x8070` at `0x07`, LED software control `0x1300` at `0x08`, onboard profiles `0x8100` at `0x09`, button spy `0x8110` at `0x0a`, report rate `0x8060` at `0x0b`, and adjustable DPI `0x2201` at `0x0c`. The capture reports 3750 mV, 1000 Hz current polling with four supported rates (125/250/500/1000 Hz), and sensor DPI 6400/current, 800/default, 100–25600 range, step 50. The `0x8070` feature reports two LED zones (Logo and Side), four effects each; no `0x8071` page appears in its feature table.

The `0x8100` descriptor is `01 03 01 05 01 0b 10 00 ff 0a 04`: memory model 1, profile format 3, macro format 1, five profile slots, 11 buttons, 16 sectors, sector size 255 bytes, and vendor-specific descriptor fields `0x0a,0x04`. The device reports onboard mode 1 and active profile address `01 01`, i.e. ROM profile 1. User sector 0 is erased (`ff` throughout) and therefore has no valid user-profile directory CRC; this is the blank/uninitialized-directory fallback to factory ROM profiles described in PeriHub's protocol notes, rather than evidence of user-data corruption. The raw ROM profile 1 read at sector `0x0101` begins `01 01 00 90 01 20 03 40 06 80 0c 00 19`, which decodes to 400/800/1600/3200/6400 DPI stages with default stage index 1; its button slots encode standard left/right/middle/back/forward plus DPI/profile special actions. The final two returned ROM bytes are `ff ff`; applying the user-sector CRC calculation produces `3e23`, so CRC handling for factory ROM profiles remains unresolved and must not be treated like a writable user sector. `ratbagctl` currently marks all five profiles disabled because it does not present the blank-directory ROM fallback as enabled profiles; treat that readout as a driver/UI limitation.

This mouse reports the same profile format 3 and 255-byte sectors used by G502 X Wireless implementations. Community reverse-engineering notes report that formats 1–5 share the classic profile layout while 255-byte devices require the CRC at offsets 253–254 and an overlapping final read block. This is a strong decoder starting point, but before enabling writes the exact 255-byte profile and factory-list behavior must be decoded and validated against the full capture. The raw log contains no onboard-profile BeginWrite (`function 6`), WriteData (`7`), or EndWrite (`8`) requests for either mouse; traffic to `0x8100` is discovery and ReadData only. The profile initialization path for a blank directory is still an implementation-specific concern: a safe first save needs to preserve/copy the factory profiles and publish a valid directory in the correct order.

Additional references:

- [PeriHub onboard profile protocol notes: ROM fallback and 255-byte sectors](https://gh.poscat.moe/hex-dump/perihub/docs/protocols/logitech-onboard-profiles.md)
- [Rust `omm-rust`: direct HID++ device access and onboard editor](https://github.com/rareinator/omm-rust) (only one tested mouse; macros, rate, names, and lighting are limitations)
