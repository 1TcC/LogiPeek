# LogiPeek

English | [简体中文](README.zh-CN.md)

## What is LogiPeek?

LogiPeek is a small, unofficial Windows command-line tool that inspects Logitech HID++ interfaces and reports the battery and DPI capabilities they expose. It runs locally as a one-shot diagnostic.

## Motivation

It is for people who want a lightweight way to inspect mouse battery and DPI capabilities without installing a complete device suite. It is not a full replacement for Logitech G HUB: it has no profiles, remapping, RGB, macros, tray application, background service, or persistent configuration. Its only configuration command is an explicit, one-shot runtime DPI change.

## Status and goals

This is an early, read-focused foundation with one narrowly scoped write: `--set-dpi <DPI>` changes the active runtime DPI through `0x2201` function 3 after a fresh safety preflight. It discovers Logitech HID interfaces, identifies selected HID++ responses, and reports only information that the device returns. Its goals are a small Rust implementation, explicit failure reporting, privacy-safe diagnostics, and no background activity. One receiver setup was exercised on Windows on 2026-09-19; see the [hardware observation](docs/hidpp.md#baseline-hardware-observation). It does not claim model support or broad receiver compatibility. Real `0x1004` battery and `0x2201` DPI reads succeeded on that setup, although forwarded-slot availability remained intermittent.

Designed to support as many Logitech mice as practical through HID++ capability discovery. The 34 protocol tests, `cargo fmt --check`, `cargo check`, `cargo clippy --all-targets --all-features -- -D warnings`, and release build passed on 2026-09-19.

## Current features

- Enumerates Logitech USB HID interfaces and prints safe labels, IDs, interface metadata, and a reconstructed report-descriptor length when the native backend provides one.
- Treats only usage page `0xFF00`, usage `1` or `2` as HID++ query candidates.
- Probes endpoint `0xFF` and slots `1` through `6`; these are bounded candidates, not a receiver or paired-device inventory.
- Recognizes HID++ 2.x feature protocol and HID++ 1.x only; HID++ 1.x register features are not implemented.
- Detects battery feature IDs `0x1000`, `0x1001`, and `0x1004`. It reads Unified Battery `0x1004` when available and retains `0x1000` version 0 as a fallback.
- Reports read-only battery percentage, coarse level, charging state, feature version, and the raw external-power indicator when the selected feature provides them. Unknown protocol values remain explicitly unknown.
- Detects DPI feature IDs `0x2201` and `0x2202`. It reads sensor count, current DPI, optional default DPI, and supported values or ranges from `0x2201`.
- Provides `--set-dpi <DPI>` for a validated runtime-only `0x2201` function 3 change. A fresh preflight must find exactly one `0x2201` endpoint and exactly one sensor. The requested value must exactly match a discrete list entry or an aligned range step; invalid values are rejected with the nearest supported suggestion, choosing the higher value on an equal-distance tie.
- Gives each physical request a 750 ms response window. Explicitly read-only requests get at most two attempts, and retry only after a timeout or the protocol-specific HID++ Busy error. Generic exchanges never retry automatically.
- Provides `--devices`, `--diag`, `--battery`, `--dpi`, and `--set-dpi <DPI>` as one-shot commands.

## Compatibility and limits

LogiPeek can see several interfaces for one physical device and deliberately does not deduplicate them. An `0xFF` endpoint can be a direct-device or receiver endpoint; a responding `1–6` slot is only a forwarded-slot candidate. Neither establishes receiver family, pairing, or a complete paired-device inventory.

A Windows USB receiver observation verified enumeration, protocol probing, dynamic feature detection, `0x1004 v3` battery reads, and one complete `0x2201 v2` DPI read for one `046D:C547` setup. Once slot `0x01` was online, five consecutive battery runs succeeded at 42%, Good, rechargeable, and discharging; the unresolved external-power indicator was raw `0x00`. The DPI response reported one sensor, current DPI 1300, default DPI 800, and a 100–25600 range in steps of 50. Other runs still timed out during slot probing, so endpoint availability remains intermittent. This did not identify the mouse model or establish support for every receiver. Bluetooth, direct USB mice, and other receiver families or connection methods remain unverified.

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
cargo run -- --set-dpi 1600
```

The project uses `hidapi` with its `windows-native` backend. No runtime network access is required.

## CLI

```text
logipeek --devices   # candidate interfaces and discovered capabilities
logipeek --diag      # safe interface/protocol detail
logipeek --battery   # read supported 0x1004 battery data, with 0x1000 v0 fallback
logipeek --dpi       # read supported 0x2201 DPI data
logipeek --set-dpi 1600  # validate and change the active runtime DPI
logipeek --help
```

`--diag` omits HID paths and serial numbers. Timeouts, busy interfaces, malformed replies, and unsupported features are expected outcomes on some hardware and are shown explicitly.

`--set-dpi` never rounds or clamps. It sends function 3 exactly once through the generic exchange path, with no write retry after Timeout, Busy, I/O failure, malformed response, or any other result. Version 1 and later must echo the sensor index and big-endian DPI value in the acknowledgement; version 0 does not echo these parameters. A safe function 2 readback follows a valid acknowledgement or setter Timeout. A matching readback verifies the observed runtime value. An acknowledged request with a failed or different readback remains unverified; after Timeout, a matching readback confirms the currently observed value while a different or unavailable readback remains ambiguous. Non-timeout protocol or I/O errors, malformed responses, and wrong echoes return immediately without readback. LogiPeek never repeats the write to resolve uncertainty.

## Privacy and performance

LogiPeek has no accounts, telemetry, analytics, uploads, cloud calls, background service, or polling loop. Requests are serial and bounded. After a request is sent, the reply deadline is 750 ms and at most 128 incoming reports are processed. Explicit read-only operations may make one retry after Timeout or HID++ Busy; malformed data, I/O failures, and other protocol errors are returned immediately. The DPI setter always makes one physical write attempt. A Windows backend write itself can take up to about one second, so 750 ms is not an entire-exchange limit. Diagnostics omit serial numbers and full HID paths.

## Roadmap

- Broaden validation beyond the observed Windows USB receiver setup to direct and Bluetooth devices.
- Improve device/receiver interpretation only where protocol evidence supports it.
- Add carefully validated reads for more battery formats and DPI data.
- Keep DPI presets, persistence, profiles, `0x2202` writes, RGB, macros, remapping, GUI/tray behavior, background services, startup registration, and updates outside the current phase. The authorized `0x2201` runtime DPI command does not expand this scope.

## Contributing

Please report the command used, sanitized `--diag` output, Windows version, and exact observed behavior. Do not include serial numbers, full HID paths, or personal data. Contributions must preserve the local-only design, avoid GPL source copying, and document hardware evidence separately from implementation.

## License

MIT. See [LICENSE](LICENSE).

## Disclaimer

LogiPeek is an unofficial open-source project and is not affiliated with, endorsed by, or sponsored by Logitech. “Logitech” and HID++ are used only to identify relevant protocols and hardware.
