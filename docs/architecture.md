# Architecture

LogiPeek is a one-shot native Rust CLI with a narrow HID abstraction. It deliberately does not use a web UI, browser runtime, Electron, WebView, Node.js, or a background service: these would add deployment size, dependencies, and persistent activity without helping a short hardware diagnostic.

```text
CLI → discovery → transport → HID++ protocol → features → battery / DPI reporting
```

`main` parses a single mode, invokes `device::scan`, and renders user-facing results. The library forbids unsafe code.

- `hid::device` enumerates `hidapi` interfaces whose vendor ID is `0x046D`, sanitizes labels, opens only HID++ query candidates, attempts native report-descriptor reconstruction after a successful open, and retains every interface separately.
- `hid::transport` performs serial HID++ exchanges over one opened interface, including Windows zero-padded input handling.
- `hid::hidpp` strictly parses short (`0x10`, 7-byte) and long (`0x11`, 20-byte) protocol packets, classifies errors, probes HID++ versions, and discovers features.
- `hid::features::battery` detects `0x1000`, `0x1001`, and `0x1004`. It reads `0x1004` capabilities/status and retains the `0x1000` version 0 reader as a fallback.
- `hid::features::dpi` detects `0x2201` and `0x2202` and implements the read-only `0x2201` sensor count, supported-values, and current/default queries. It contains no write operation.

## Capability-first discovery

A model database becomes stale, requires ongoing model-specific maintenance, and cannot safely predict feature indices that a device assigns dynamically. LogiPeek instead asks each responding HID++ 2.x endpoint for the features it exposes. This makes partial support explicit and lets new devices be observed without hardcoding dynamic feature indexes. It is still not a compatibility guarantee.

An interface is a query candidate only when its HID usage page is `0xFF00` and usage is `1` or `2`. Usage `1` selects short report ID `0x10`; usage `2` selects long report ID `0x11`.

For each opened candidate, LogiPeek probes `0xFF`, then `1` through `6`. `0xFF` is displayed as a direct-device-or-receiver endpoint; `1–6` are forwarded-slot candidates. The scan does not establish receiver type, receiver family, pairing status, or a paired-device inventory. It does not merge interfaces, PIDs, names, or endpoints into physical devices, so one physical device may appear more than once.

A successful ping identifies HID++ 2.x feature protocol or HID++ 1.x. HID++ 1.x is reported but its register feature set is not used. HID++ 2.x endpoints are queried through the root feature for battery and DPI IDs.

## Request and failure behavior

The transport makes one request at a time and rotates software ID values `1..15` per request. After the write returns, it gives matching replies a 350 ms deadline and consumes at most 128 reports. The 350 ms bound applies to reply waiting, not the entire exchange: the Windows backend write can itself take up to about one second. The transport ignores unrelated notifications and replies, matches device, feature, function, and software ID, and exposes transport, timeout, malformed-response, and protocol errors rather than panicking.

Windows may return a short HID++ input report padded with zero bytes to the collection's maximum input size. `transport::parse_input` accepts only zero padding up to 64 bytes, then passes the exact report slice to the strict `Packet::parse` parser.

Software IDs are not exclusive. Another client can use the interface, and an apparently matching response can be stale or affected by concurrent Logitech software. The bounded exchange cannot prove reply ownership.

## Battery semantics

Feature discovery reports `0x1000`, `0x1001`, and `0x1004`. A discovered `0x1004` is preferred because it can report a percentage, coarse level, charging state, and rechargeable capability; the `0x1000` version 0 reader remains the fallback. Percentage and coarse level remain separate optional values, so no coarse level is converted into an invented percentage. The fourth `0x1004` status byte is retained as an external-power indicator only in raw form because the public material consulted does not define its values. `0x1001` remains detection-only.

For `0x2201`, the reader asks for the sensor count and then reads each sensor's supported DPI representation and current/default DPI. Supported values remain a discrete list or a compact range with a step; the parser does not expand ranges. Device-reported sensor counts are bounded defensively. `0x2202` remains detection-only. No setter, function 3 request, preset, or DPI write API exists.

## Boundaries and future work

There is no runtime network activity, account, telemetry, persistence, service, or background polling. Labels exclude controls and are length-limited; the CLI omits serial numbers and HID paths. On 2026-09-19, Windows testing exercised one USB receiver (`046D:C547`): six interfaces were enumerated, including two `FF00` candidates (usage 1 and 2); enumeration, protocol probes, and dynamic feature detection succeeded. This is a narrow observation, not identification of a mouse model or validation of receiver-family coverage. Bluetooth, direct USB mice, other receiver families, and other connection methods remain unverified.

Future work begins with broader hardware validation and evidence-backed receiver/Bluetooth interpretation. Additional battery formats may be added only after validation. DPI writes, profiles, RGB, macros, remapping, model databases, GUI/tray behavior, startup registration, and updates remain outside this phase.
