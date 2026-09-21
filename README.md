# LogiPeek

English | [简体中文](README.zh-CN.md)

## What is LogiPeek?

LogiPeek is a lightweight native Windows tray application with a compact settings window and CLI diagnostics for Logitech HID++ battery and DPI capabilities. It runs entirely on the local machine.

## Motivation

It is for people who want a lightweight way to view mouse battery state and control runtime DPI without installing a complete device suite. It is not a full replacement for Logitech G HUB: it has no profiles, remapping, RGB, macros, background service, cloud account, or onboard-memory editor.

## Status and goals

With no arguments LogiPeek starts its native notification-area process and opens a compact settings window. The internal `--startup` launch enters tray-only mode; public CLI arguments still perform one operation and exit. GUI/tray writes use an in-memory validated target with fresh fast revalidation, while CLI writes keep the complete unique-target preflight. One receiver setup was exercised on Windows on 2026-09-19; see the [hardware observation](docs/hidpp.md#baseline-hardware-observation).

Designed to support as many Logitech mice as practical through HID++ capability discovery. The automated suite contains 34 protocol tests plus state, settings, and slider tests.

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
- Starts a native Win32 tray when run without arguments. Its menu shows current battery/DPI state, Refresh, Exit, and fixed 400/800/1600/3200 DPI choices. Unsupported choices stay visible but disabled, and the current choice is checked when it matches.
- Refreshes battery state every 60 seconds on one blocking HID worker. DPI is read at startup, after a tray write, and on manual Refresh. A named mutex prevents duplicate tray instances without blocking CLI commands.
- Shows a compact 400 x 640 logical-pixel, Per-Monitor-DPI-aware native Win32 settings window with battery state, current DPI, a capability-driven slider, presets, device selection, startup and low-battery controls, inline status, and Refresh.
- Supports both discrete DPI lists and stepped ranges. Dragging updates only a pending preview; releasing commits at most one write through the same safe setter and returns to the hardware value after failure.
- Lets users edit four preset slots, choose System, Light, or Dark appearance, and switch the window and tray live between English and Simplified Chinese. Presets unsupported by the current device remain saved but disabled.
- Shows an in-window language picker on first run and after upgrade from settings without a valid `language` field; the normal settings page appears after the choice is saved.
- Stores tolerant, human-readable settings in `%LOCALAPPDATA%\LogiPeek\settings.ini` using a synced temporary file and atomic same-volume replacement. The optional Start with Windows switch manages only LogiPeek's current-user Run value; no service, scheduled task, or database is used.
- Selects the only writable DPI device automatically. When multiple writable devices are present, it requires an explicit choice, persists only a derived opaque ID, and keeps Battery/DPI bound to that same endpoint.
- Can show localized low-battery tray balloons for real percentages while discharging. Notifications default to 20%, fire once per crossing, and rearm only after a 5% recovery.
- Closing the settings window hides it to the tray. `Open LogiPeek` or tray activation shows the same window again; Tray Exit performs clean shutdown.

## Compatibility and limits

LogiPeek can see several interfaces for one physical device and deliberately does not deduplicate them. An `0xFF` endpoint can be a direct-device or receiver endpoint; a responding `1–6` slot is only a forwarded-slot candidate. Neither establishes receiver family, pairing, or a complete paired-device inventory.

A Windows USB receiver observation verified enumeration, protocol probing, dynamic feature detection, `0x1004 v3` battery reads, and one complete `0x2201 v2` DPI read for one `046D:C547` setup. Once slot `0x01` was online, five consecutive battery runs succeeded at 42%, Good, rechargeable, and discharging; the unresolved external-power indicator was raw `0x00`. The DPI response reported one sensor, current DPI 1300, default DPI 800, and a 100–25600 range in steps of 50. Other runs still timed out during slot probing, so endpoint availability remains intermittent. This did not identify the mouse model or establish support for every receiver. Bluetooth, direct USB mice, and other receiver families or connection methods remain unverified.

Runtime `0x2201` DPI writing was physically verified on the same setup while `logi_lamparray_service` was running. A fresh read reported 1300 DPI; one at-most-once function 3 request changed it to the adjacent supported value 1350, and both the immediate safe readback and an independent `--dpi` process confirmed 1350. A second at-most-once request restored 1300, again confirmed immediately and independently. Both setter acknowledgements timed out, so the confirmation came from readback rather than an ACK; neither command retried function 3. This observation does not establish compatibility with all Logitech devices.

The tray command path was also exercised on this setup. Startup reported 41% battery and 1300 DPI; the 1600 preset changed the physical runtime value and an independent CLI process read back 1600. The original 1300 value was then restored and independently confirmed. Refresh, single-instance behavior, and clean Exit were exercised as well. Menu rendering was driven programmatically for this test rather than by a manual pointer click.

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

Run `cargo run` with no arguments to start the tray and settings window.

The four presets, appearance choice, GUI language, startup intent, battery-alert preference, threshold, and optional opaque device selection are saved locally. DPI changes remain runtime-only device values and are not written to onboard profiles. CLI output remains English.

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

LogiPeek has no accounts, telemetry, analytics, uploads, cloud calls, runtime network requests, or background service. The Win32 UI repaints only on state/input changes. The tray uses the message wait plus one blocking worker queue; it does not busy-poll. The worker performs a battery refresh every 60 seconds and serializes all HID access. The DPI setter always makes one physical write attempt per submitted command. Diagnostics omit serial numbers and full HID paths.

## Roadmap

- Broaden validation beyond the observed Windows USB receiver setup to direct and Bluetooth devices.
- Improve device/receiver interpretation only where protocol evidence supports it.
- Add carefully validated reads for more battery formats and DPI data.
- Broaden native-window accessibility and multi-device hardware coverage. Profiles, `0x2202` writes, RGB, macros, remapping, background services, and automatic updates remain outside the current phase. LogiPeek performs no version polling; users obtain releases from this repository.

## Contributing

Please report the command used, sanitized `--diag` output, Windows version, and exact observed behavior. Do not include serial numbers, full HID paths, or personal data. Contributions must preserve the local-only design, avoid GPL source copying, and document hardware evidence separately from implementation.

## License

MIT. See [LICENSE](LICENSE).

## Disclaimer

LogiPeek is an unofficial open-source project and is not affiliated with, endorsed by, or sponsored by Logitech. “Logitech” and HID++ are used only to identify relevant protocols and hardware.
