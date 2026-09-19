# LogiPeek

English | [简体中文](README.zh-CN.md)

## What is LogiPeek?

LogiPeek is a small, unofficial Windows command-line tool that inspects Logitech HID++ interfaces and reports the battery and DPI capabilities they expose. It runs locally as a one-shot diagnostic.

## Motivation

It is for people who want a lightweight way to inspect mouse battery and DPI capabilities without installing a complete device suite. It is not a full replacement for Logitech G HUB: it has no profiles, remapping, RGB, macros, tray application, background service, or configuration writes.

## Status and goals

This is an early read-only foundation. It discovers Logitech HID interfaces, identifies selected HID++ responses, and reports only information that the device returns. Its goals are a small Rust implementation, explicit failure reporting, privacy-safe diagnostics, and no background activity. One receiver setup was exercised on Windows on 2026-09-19; see the [hardware observation](docs/hidpp.md#baseline-hardware-observation). It does not claim model support or broad receiver compatibility. A real `0x1004` battery read succeeded on that setup; a complete real-device DPI read remains unverified because the forwarded slot was intermittent.

Designed to support as many Logitech mice as practical through HID++ capability discovery. The 25 protocol tests, `cargo fmt --check`, `cargo check`, `cargo clippy --all-targets --all-features -- -D warnings`, and release build passed on 2026-09-19.

## Current features

- Enumerates Logitech USB HID interfaces and prints safe labels, IDs, interface metadata, and a reconstructed report-descriptor length when the native backend provides one.
- Treats only usage page `0xFF00`, usage `1` or `2` as HID++ query candidates.
- Probes endpoint `0xFF` and slots `1` through `6`; these are bounded candidates, not a receiver or paired-device inventory.
- Recognizes HID++ 2.x feature protocol and HID++ 1.x only; HID++ 1.x register features are not implemented.
- Detects battery feature IDs `0x1000`, `0x1001`, and `0x1004`. It reads Unified Battery `0x1004` when available and retains `0x1000` version 0 as a fallback.
- Reports read-only battery percentage, coarse level, charging state, feature version, and the raw external-power indicator when the selected feature provides them. Unknown protocol values remain explicitly unknown.
- Detects DPI feature IDs `0x2201` and `0x2202`. It reads sensor count, current DPI, optional default DPI, and supported values or ranges from `0x2201`; it never writes DPI or scans unknown functions.
- Provides `--devices`, `--diag`, `--battery`, and `--dpi` as one-shot commands.

## Compatibility and limits

LogiPeek can see several interfaces for one physical device and deliberately does not deduplicate them. An `0xFF` endpoint can be a direct-device or receiver endpoint; a responding `1–6` slot is only a forwarded-slot candidate. Neither establishes receiver family, pairing, or a complete paired-device inventory.

A Windows USB receiver observation verified enumeration, protocol probing, dynamic feature detection, and one `0x1004 v3` battery sample for one `046D:C547` setup. The device reported 43% and discharging; the unresolved external-power indicator was raw `0x00`. Other attempts timed out, and no complete DPI value was captured, so connection stability and real-device DPI parsing remain unverified. This did not identify the mouse model or establish support for every receiver. Bluetooth, direct USB mice, and other receiver families or connection methods remain unverified.

HID++ is shared transport traffic. Replies can be stale or belong to another application; rotating software IDs reduces collisions but does not make them exclusive. Close Logitech software and retry if diagnostics are inconsistent.

## Build

Install the Rust stable MSVC toolchain and the Windows SDK/Build Tools. From a Developer PowerShell, or another shell where the MSVC linker is available:

```powershell
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Run the CLI during development:

```powershell
cargo run -- --devices
cargo run -- --diag
cargo run -- --battery
cargo run -- --dpi
```

The project uses `hidapi` with its `windows-native` backend. No runtime network access is required.

## CLI

```text
logipeek --devices   # candidate interfaces and discovered capabilities
logipeek --diag      # safe interface/protocol detail
logipeek --battery   # read supported 0x1004 battery data, with 0x1000 v0 fallback
logipeek --dpi       # read supported 0x2201 DPI data
logipeek --help
```

`--diag` omits HID paths and serial numbers. Timeouts, busy interfaces, malformed replies, and unsupported features are expected outcomes on some hardware and are shown explicitly.

## Privacy and performance

LogiPeek has no accounts, telemetry, analytics, uploads, cloud calls, background service, or polling loop. Requests are serial and bounded. After a write completes, the reply deadline is 350 ms and at most 128 incoming reports are processed. A Windows backend write itself can take up to about one second, so 350 ms is not an entire-exchange limit. Diagnostics omit serial numbers and full HID paths.

## Roadmap

- Broaden validation beyond the observed Windows USB receiver setup to direct and Bluetooth devices.
- Improve device/receiver interpretation only where protocol evidence supports it.
- Add carefully validated reads for more battery formats and DPI data.
- Keep DPI writes, presets, profiles, RGB, macros, remapping, GUI/tray behavior, background services, startup registration, and updates outside the current read-only phase.

## Contributing

Please report the command used, sanitized `--diag` output, Windows version, and exact observed behavior. Do not include serial numbers, full HID paths, or personal data. Contributions must preserve the local-only design, avoid GPL source copying, and document hardware evidence separately from implementation.

## License

MIT. See [LICENSE](LICENSE).

## Disclaimer

LogiPeek is an unofficial open-source project and is not affiliated with, endorsed by, or sponsored by Logitech. “Logitech” and HID++ are used only to identify relevant protocols and hardware.
