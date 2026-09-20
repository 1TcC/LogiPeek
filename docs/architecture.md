# Architecture

LogiPeek is a native Rust Windows tray, compact settings window, and one-shot CLI with a narrow HID abstraction. It is read-focused and permits one explicit runtime DPI write path. It deliberately does not use a GUI framework, web UI, browser runtime, Electron, WebView, Node.js, async runtime, database, or background service.

```text
Win32 tray + settings window ─┐
                              ├→ shared AppState → single HID worker → HID++ battery / DPI
one-shot CLI ─────────────────┘          ↑
settings.ini → Settings Store ───────────┘
```

`main` starts tray plus settings-window mode only when no arguments are present; every existing CLI argument still performs one operation and exits. The core library forbids unsafe code. Raw Win32 calls are isolated in the binary-only `app::tray` and `app::window` modules, with documented unsafe blocks and no unsafe HID parsing.

- `app::tray` owns the hidden top-level window, notification icon, popup menu, named single-instance mutex, and blocking Win32 message loop. A small original monochrome mouse icon is generated at startup, avoiding a binary asset. The process detaches its console only after tray initialization succeeds, so CLI output remains intact.
- `app::window` owns one fixed 400 x 580 logical-pixel, Per-Monitor-V2-aware Win32 settings window. It custom-draws the first-run language picker, cards, battery progress, slider, presets, appearance/language choices, and inline status only during `WM_PAINT`. Closing hides it; tray Open shows the same HWND.
- `app::state` is the single hardware and UI truth consumed by tray and window. It stores device status, battery semantics, current/supported DPI, operation status, four presets, theme, optional language, and settings-save notice. Multiple usable targets disable all DPI controls.
- `app::settings` is a tolerant std-only `key=value` parser and serializer. Missing or invalid language remains `None` to select the first-run view; other malformed individual fields fall back, and unknown keys are ignored without preventing startup.
- `app::text` centralizes the small English and Simplified Chinese GUI/tray catalog without an i18n framework. CLI strings deliberately remain English.
- `app::slider` provides pure list/range position conversion. Discrete lists map positions to indices; ranges use the existing nearest-supported rule, including higher-value tie breaking.
- `app::worker` owns the only background thread and a bounded one-command queue. It serializes startup/manual scans, tray DPI writes, and the 60-second battery refresh. `recv_timeout` blocks between work items, and the UI thread blocks in `GetMessageW`; neither loop spins.

- `hid::device` enumerates `hidapi` interfaces whose vendor ID is `0x046D`, sanitizes labels, opens only HID++ query candidates, attempts native report-descriptor reconstruction after a successful open, and retains every interface separately.
- `hid::transport` performs one physical HID++ request and one bounded response wait over an opened interface, including Windows zero-padded input handling. It never retries a request.
- `hid::hidpp` strictly parses short (`0x10`, 7-byte) and long (`0x11`, 20-byte) protocol packets, classifies errors, probes HID++ versions, and discovers features.
- `hid::features::battery` detects `0x1000`, `0x1001`, and `0x1004`. It reads `0x1004` capabilities/status and retains the `0x1000` version 0 reader as a fallback.
- `hid::features::dpi` detects `0x2201` and `0x2202`, implements the `0x2201` sensor count, supported-values, and current/default queries, and exposes the narrowly scoped function 3 runtime setter used only after CLI preflight.

## Tray behavior and concurrency

The tray menu and settings window render locked clones of the same `AppState`. Battery and DPI values therefore come only from completed hardware scans; failed full scans replace them with unavailable state instead of presenting stale data as current. A battery-only refresh retains DPI only when the same sole target is still present. Multiple usable targets produce an explicit multiple-device state and disable every preset.

The worker executes `device::set_unique_runtime_dpi`, the same function used by `--set-dpi`. Every click therefore receives a fresh discovery/preflight, unique endpoint and single-sensor check, supported-value validation, one function 3 attempt, and the existing readback classification. The bounded queue prevents repeated clicks from creating an unbounded series of writes. The worker performs a full rescan after a write and posts a state-update message to the UI thread.

The notification icon is re-added after Explorer broadcasts `TaskbarCreated`. Exit hides/destroys the settings window, removes the icon, stops and joins the worker while its notification HWND remains valid, destroys the tray window and icon, unregisters both classes, and releases the named mutex. A second no-argument process exits normally; CLI processes do not acquire this mutex.

The only direct Windows dependency is `windows-sys`. Phase 5 adds only DWM, Storage/FileSystem, HiDPI, Input/IME, and KeyboardAndMouse feature groups to the existing Foundation, GDI, Security, Console, LibraryLoader, Threading, Shell, and WindowsAndMessaging set. The process disables IME only for its ASCII-numeric custom preset editor; this avoids third-party input-method injection without changing system configuration.

## Settings window and slider commit

All layout constants are 96-DPI logical units and are scaled with the window DPI. `WM_DPICHANGED` copies and applies the suggested rectangle, updates scaling, and invalidates the window. Light and Dark use small fixed palettes; System follows Win32 system window colors and theme-change messages. The DWM dark-title attribute is optional and failure is harmless.

Slider capture starts on mouse down. Mouse movement updates only a `preview` value derived from `DpiValues`; it never sends HID traffic. Mouse release ends capture and can enqueue exactly one `SetDpi` command when preview differs from the last hardware current value. Capture loss cancels the preview. The worker performs the existing fresh preflight, at-most-once function 3 request, readback, and full refresh before publishing Verified or Failed state.

Four custom numeric edit slots accept ASCII digits, Backspace, Tab, and Enter/Save Settings. Values are positive `u16` preferences; unsupported values may be saved but their tray/window buttons are disabled for the current device. There is no continuous render timer or animation loop.

## Language selection and settings storage

The runtime path is `%LOCALAPPDATA%\LogiPeek\settings.ini`. Fields are `preset1` through `preset4`, `theme=system|light|dark`, and `language=en|zh-CN`. `language=None` is the only onboarding state: the main window renders the language picker instead of normal settings. Choosing a language saves it immediately, switches the window and tray catalog, and then reveals the main UI. Saving writes, flushes, and syncs `settings.tmp`, then uses `MoveFileExW` with replace-existing and write-through flags for same-volume atomic replacement. A read or write failure leaves the application usable; write failure is reported inline. No registry, database, serializer crate, or network access is involved.

## Capability-first discovery

A model database becomes stale, requires ongoing model-specific maintenance, and cannot safely predict feature indices that a device assigns dynamically. LogiPeek instead asks each responding HID++ 2.x endpoint for the features it exposes. This makes partial support explicit and lets new devices be observed without hardcoding dynamic feature indexes. It is still not a compatibility guarantee.

An interface is a query candidate only when its HID usage page is `0xFF00` and usage is `1` or `2`. Usage `1` selects short report ID `0x10`; usage `2` selects long report ID `0x11`.

For each opened candidate, LogiPeek probes `0xFF`, then `1` through `6`. `0xFF` is displayed as a direct-device-or-receiver endpoint; `1–6` are forwarded-slot candidates. The scan does not establish receiver type, receiver family, pairing status, or a paired-device inventory. It does not merge interfaces, PIDs, names, or endpoints into physical devices, so one physical device may appear more than once.

A successful ping identifies HID++ 2.x feature protocol or HID++ 1.x. HID++ 1.x is reported but its register feature set is not used. HID++ 2.x endpoints are queried through the root feature for battery and DPI IDs.

## Request and failure behavior

The transport makes one request at a time and rotates software ID values `1..15` per request. After the write returns, it gives matching replies a 750 ms deadline and consumes at most 128 reports. The 750 ms bound applies to reply waiting, not the entire exchange: the Windows backend write can itself take up to about one second. The transport ignores unrelated notifications and replies, matches device, feature, function, and software ID, and exposes transport, timeout, malformed-response, and protocol errors rather than panicking.

`hidpp::read_only_exchange` is an explicit recovery layer for idempotent reads. It permits two attempts and retries only Timeout, HID++ 1.x Busy `0x07`, or HID++ 2.x Busy `0x08`. Every attempt calls the transport separately and therefore gets a new nonzero software ID. The DPI setter bypasses this helper and calls the generic exchange exactly once, so an ambiguous timeout, Busy response, I/O failure, malformed response, or acknowledgement mismatch can never resend the write.

Windows may return a short HID++ input report padded with zero bytes to the collection's maximum input size. `transport::parse_input` accepts only zero padding up to 64 bytes, then passes the exact report slice to the strict `Packet::parse` parser.

Software IDs are not exclusive. Another client can use the interface, and an apparently matching response can be stale or affected by concurrent Logitech software. The bounded exchange cannot prove reply ownership.

## Battery semantics

Feature discovery reports `0x1000`, `0x1001`, and `0x1004`. A discovered `0x1004` is preferred because it can report a percentage, coarse level, charging state, and rechargeable capability; the `0x1000` version 0 reader remains the fallback. Percentage and coarse level remain separate optional values, so no coarse level is converted into an invented percentage. The fourth `0x1004` status byte is retained as an external-power indicator only in raw form because the public material consulted does not define its values. `0x1001` remains detection-only.

## DPI semantics and write boundary

For `0x2201`, the reader asks for the sensor count and then reads each sensor's supported DPI representation and current/default DPI. Supported values remain a discrete list or a compact range with a step; the parser does not expand ranges. Device-reported sensor counts are bounded defensively. `0x2202` remains detection-only.

`--set-dpi <DPI>` starts a fresh discovery and preflight instead of reusing earlier CLI output or a persistent cache. The write is allowed only when that scan finds exactly one responding endpoint with `0x2201` and its function 0 reports exactly one sensor. This intentionally rejects ambiguous multi-endpoint and multi-sensor configurations rather than guessing a target.

The requested DPI must be exact. For a discrete list it must equal one listed value. For a range it must lie between the inclusive bounds and satisfy `(value - minimum) % step == 0`. The command never rounds or clamps. Rejected values include a nearest supported suggestion; if two values are equally distant, the higher value wins. Suggestion calculation does not authorize a write.

After validation, function 3 receives `[sensor index, DPI MSB, DPI LSB]` through one direct generic exchange. Version 1 and later responses must echo the same sensor index and big-endian DPI; version 0 is allowed to acknowledge without echoing those parameters. A function 2 readback follows a valid acknowledgement or setter Timeout and may use the read-only retry policy, but no readback result can trigger another write. A valid acknowledgement plus a matching readback is verified; a different or failed readback is acknowledged but unverified. After setter Timeout, a matching readback confirms the currently observed value, while a different or unavailable readback remains ambiguous. Non-timeout protocol or I/O errors, malformed responses, and acknowledgement echo mismatches return immediately without readback. None of these paths repeats function 3.

## Boundaries and future work

There is no runtime network activity, account, telemetry, service, or busy polling. Persistence is limited to four preset values and appearance in the local settings file. The tray's only periodic work is the 60-second battery refresh. Function 3 changes only the active runtime DPI exposed by `0x2201`; presets are preferences rather than profiles, onboard settings, or startup configuration. Labels exclude controls and are length-limited; the CLI omits serial numbers and HID paths. On 2026-09-19, Windows testing exercised one USB receiver (`046D:C547`): six interfaces were enumerated, including two `FF00` candidates (usage 1 and 2); enumeration, protocol probes, and dynamic feature detection succeeded. This is a narrow observation, not identification of a mouse model or validation of receiver-family coverage. Bluetooth, direct USB mice, other receiver families, and other connection methods remain unverified.

Future work begins with broader hardware validation, evidence-backed receiver/Bluetooth interpretation, and incremental accessibility improvements. Device selection, profiles, `0x2202` writes, RGB, macros, remapping, model databases, startup registration, notifications, and updates remain outside this phase.
