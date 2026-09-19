# HID++ behavior in LogiPeek

This page documents what the current code sends and accepts. It is not a claim that every Logitech device, receiver, or Bluetooth interface behaves the same way. A narrow Windows USB receiver observation is recorded below; other connection methods remain unverified.

## Baseline hardware observation

The following evidence was collected on **2026-09-19** on Windows before the phase-two battery and DPI readers were added. It records one observation and is not a mouse-model identification, a paired-device inventory, or a receiver-family compatibility claim. It does not claim any concrete battery or DPI value.

| Item | Observed result |
| --- | --- |
| USB receiver | Logitech VID `046D`, PID `C547` |
| Enumeration | 6 interfaces total: two `0xFF00` query candidates (usage 1 and usage 2), plus four other enumerated interfaces |
| Native descriptor query | Both candidate interfaces opened successfully; each reconstructed report descriptor was 36 bytes |
| Usage 1 candidate | Endpoint `0xFF` identified HID++ 1.x (Legacy) |
| Usage 2 candidate | Slot `0x01` identified HID++ 4.2 |
| Dynamic features at slot `0x01` | `0x1004`: index `0x06`, version 3, flags 0; `0x2201`: index `0x0A`, version 2, flags 0 |
| Not found at slot `0x01` | `0x1000`, `0x1001`, and `0x2202` |
| Phase-one battery command | Completed normally; the device exposed `0x1004`, which was detection-only at that time, so no battery value was recorded |
| Phase-one DPI command | Dynamic detection verified; no DPI query or write was performed at that time |
| Validation | Enumeration, protocol probing, and dynamic feature detection verified for this setup only |

Phase-two read-only validation on the same date produced one successful `0x1004 v3` sample at slot `0x01`: 43%, Discharging, and raw external-power indicator `0x00`. Other battery attempts and all DPI attempts in this run timed out while probing the forwarded slot. The implementation and parser tests passed, but a complete physical-device `0x2201` value remains unverified. No write function was sent.

## Evidence and sources

The implementation keeps four kinds of evidence separate:

- **Official public material.** Logitech’s [cpg-docs HID++ 2.0](https://github.com/Logitech/cpg-docs/tree/master/hidpp20) defines the packet model and dynamic feature discovery, including [0x0000 IRoot](https://github.com/Logitech/cpg-docs/blob/master/hidpp20/features/0x0000-IRoot.rst). Public mirrors of Logitech’s [HID++ 1.x receiver specification](https://lekensteyn.nl/files/logitech/logitech_hidpp10_specification_for_Unifying_Receivers.pdf) and [HID++ 2.0 specification draft](https://lekensteyn.nl/files/logitech/logitech_hidpp_2.0_specification_draft_2012-06-04.pdf) document the error codes and `0x1000`. The public mirror of Logitech’s [`0x2201` Adjustable DPI feature](https://lekensteyn.nl/files/logitech/x2201_adjustabledpi.html) documents sensor count, the DPI list/range encoding, current/default DPI, and the separate write function.
- **Permissively licensed implementation cross-check.** The [`0x1004` Unified Battery](https://openlogi.org/hidpp/features/x1004-unified-battery) and [`0x2201` Adjustable DPI](https://openlogi.org/hidpp/features/x2201-adjustable-dpi) references from OpenLogi were used to cross-check request functions and response layout. OpenLogi is MIT/Apache-2.0 and its HID++ crate is 0BSD. LogiPeek implements its own parsers and transport behavior.
- **Real hardware observation.** The table above records the receiver interfaces, endpoint results, protocol 4.2 response, and dynamically returned feature versions seen on one Windows setup. Feature indices are never copied from the observation; they are resolved with IRoot on every run.
- **Still uncertain.** Public material identifies byte 3 of the `0x1004` status response as an external-power-source indicator but does not define its value meanings. LogiPeek therefore preserves it only as a raw byte and does not claim that it means connected or disconnected. Publicly mirrored `0x2201` documentation covers versions 0 and 1, while the observed device reports version 2; LogiPeek confines version 2 handling to the documented base functions and layouts, including function 3, and does not infer new version-2 fields. Bluetooth, direct USB mice, other receivers, and additional devices remain unverified until separately observed.

The Linux HID++ driver was consulted only as a behavior cross-check for `0x1004` and transport behavior; no GPL implementation code is copied into this MIT project.

## Candidate interfaces and reports

Only Logitech vendor ID `0x046D` is enumerated. An interface is a HID++ query candidate when its usage page is `0xFF00` and usage is `1` or `2`. The implementation uses report ID `0x10` with 7 bytes for usage `1`, or report ID `0x11` with 20 bytes for usage `2`. This heuristic does not identify a receiver family or prove a device is paired.

A request is report ID, device index, feature index, function/software-ID byte, parameters, then zero padding to the selected report length. Functions fit in four bits. Each request rotates software ID through `1..15`.

Protocol packets themselves are exact-length: `Packet::parse` accepts only 7-byte `0x10` or 20-byte `0x11` slices. Windows can deliver a short report zero-padded to the collection's maximum input size. `transport::parse_input` first accepts only zero padding, never nonzero trailing data, with an upper input length of 64 bytes; it then gives `Packet::parse` the exact report slice.

Replies are accepted only when a parsed report has the selected device and matches the requested feature, function, and software ID. HID++ error markers `0x8F` (short legacy reports only) and `0xFF` are protocol errors only when their embedded feature/function identifiers match the request. Unrelated traffic is ignored. A malformed report that otherwise matches the request is an error.

## Endpoint probing and root feature

The program probes endpoint indices `0xFF`, `1`, `2`, `3`, `4`, `5`, and `6` with the HID++ root ping (feature `0`, function `1`, parameters `00 00 A5`). A HID++ 1.x error response with error code `1` identifies Legacy; a valid reply with major version at least 2 and echoed `A5` identifies HID++ 2.x.

For HID++ 2.x, feature discovery calls IRoot function `0` with a big-endian feature ID. A returned feature index of zero means unsupported except for feature ID zero itself. LogiPeek queries battery IDs `0x1000`, `0x1001`, `0x1004` and DPI IDs `0x2201`, `0x2202`.

Endpoint `0xFF` may mean a direct device or a receiver endpoint. Indices `1–6` are forwarded-slot candidates only. The scan neither lists paired devices nor verifies that a slot represents a mouse, receiver, or distinct physical device.

## Battery reads

Battery feature discovery still checks `0x1000`, `0x1001`, and `0x1004`. The read path supports `0x1004` and retains the `0x1000` version-0 fallback; `0x1001` remains detection-only.

For `0x1004`, LogiPeek calls function `0` (`getBatteryCapabilities`) and then function `1` (`getBatteryInfo`). The capability response supplies the supported coarse-level bits, rechargeable flag, and percentage-support flag. The information response supplies a percentage byte, coarse level, charging state, and the unresolved external-power byte. A percentage is exposed only when the capability bit says it is supported, and values above 100 are rejected. Coarse levels use the documented values Critical `1`, Low `2`, Good `4`, and Full `8`; unexpected values remain explicit unknown values. Charging states `0` through `4` map to Discharging, Charging, Slow, Complete, and Error; other values remain explicit unknown values. The dynamically discovered feature version is retained in the result.

The parser requires at least two capability bytes and four information bytes. It never converts a coarse level into a made-up percentage. The external-power byte is preserved as raw diagnostic data because its public semantics are incomplete.

For `0x1000` version 0, LogiPeek continues to call function `1` for capabilities and function `0` for status. It requires at least two capability bytes and three status bytes. Exact percentage output depends on capability flags; without a percentage/mileage indication it reports a coarse level. Other `0x1000` versions are not read.

## DPI reads and runtime write

The read path and narrowly scoped runtime setter support `0x2201`; `0x2202` remains detection-only. LogiPeek uses only these documented `0x2201` functions:

| Function | Request purpose | Parsed result |
| --- | --- | --- |
| `0` | `getSensorCount` | Number of motion sensors |
| `1` | `getSensorDpiList(sensorIdx)` | Per-sensor supported discrete values or a range |
| `2` | `getSensorDpi(sensorIdx)` | Echoed sensor index, current DPI, and optional default DPI |
| `3` | `setSensorDpi(sensorIdx, dpi)` | One runtime DPI change after unique-target and value validation |

Sensor count must be between 1 and 16, which bounds device-controlled work. Each sensor index from zero to count minus one is queried separately, and both function `1` and function `2` must echo the requested index.

Function `1` values are big-endian `u16`. Zero terminates the list. Values `1..=0xDFFF` are explicit DPI values. Values with the top three bits set are range markers; the low 13 bits are the step. A range must be exactly `minimum, marker, maximum`, with a nonzero step and increasing bounds. `0xE000` is rejected because it encodes a zero step. Discrete lists must be strictly increasing. Nonzero data after a terminator, malformed sentinels, truncated replies, and unterminated partial lists are errors. The parser tolerates a full seven-entry long response without a terminator because public references differ on whether the final slot must be reserved for zero padding.

Function `2` accepts current and default DPI only in `1..=0xDFFF`; a zero default is represented as unavailable because feature version 0 reports no default. Current/default values are not invented from the supported range.

The public `0x2201` document covers feature versions 0 and 1. The observed device reports version 2, so LogiPeek uses only the same base function numbers and payload layouts and does not invent version-2 fields. This compatibility assumption is kept separate from hardware evidence.

### `--set-dpi` safety preflight

`--set-dpi <DPI>` performs a fresh scan for every invocation. It does not reuse a previous `--devices` or `--dpi` result and has no global cache. Before any write, the scan must produce exactly one responding endpoint that exposes `0x2201`, and function 0 on that endpoint must report exactly one sensor. Zero or multiple matching endpoints, or any sensor count other than one, abort without sending function 3.

The command reads the supported representation before writing. A discrete list authorizes only exact members. A range authorizes only values within its inclusive bounds for which `(requested - minimum) % step == 0`. Values are never rounded or clamped. When the value is unsupported, LogiPeek suggests the nearest valid value; an equal-distance tie resolves upward. This suggestion is informational and the rejected invocation sends no write.

### Function 3 request and acknowledgement

The function 3 payload is exactly three meaningful bytes: byte 0 is sensor index `0`; bytes 1 and 2 are the requested DPI as a big-endian `u16`. It is sent through `Exchange::exchange`, never `read_only_exchange`. There is exactly one physical function 3 attempt, including after Timeout, HID++ Busy, I/O failure, a malformed reply, or an acknowledgement mismatch.

For feature version 1 and later, the public specification says the response echoes the sensor index and DPI in the same three-byte layout; LogiPeek requires that echo to match. Version 0 does not echo these parameters, so a strictly matched successful protocol response is its acknowledgement. No other response parameter is interpreted.

After a valid acknowledgement or setter Timeout, a function 2 readback is safe because it cannot repeat or change the DPI. Readback can use the bounded read-only retry policy. Non-timeout protocol or I/O errors, malformed responses, and wrong echoes return immediately without readback. The outcomes remain explicit:

- a valid acknowledgement followed by the requested readback is verified success;
- an explicit HID++ protocol error returns immediately as a rejected write;
- a valid acknowledgement followed by a different value or failed readback means the write was acknowledged but the final state is not verified;
- after setter Timeout, a matching readback confirms that the requested value is currently observed, while a different or unavailable readback leaves the outcome ambiguous;
- an I/O failure, malformed response, or wrong echo is reported immediately and does not cause a speculative readback.

None of these outcomes triggers a second function 3 request. The change is runtime-only: LogiPeek does not persist a preset, alter an onboard profile, write `0x2202`, scan unknown functions, or promise that the value survives reconnection, profile changes, device reset, or another application's changes.

## Timing and uncertainty

### Implementation policy

Each generic exchange remains one physical request followed by one bounded wait; it never resends automatically. Once the write completes, a matching reply has a 750 ms deadline and at most 128 reports are processed. This replaces the original 350 ms policy, which provided less margin than mature implementations and the observed receiver path warranted. The Windows backend write can itself take up to about one second, so 750 ms is not a bound on the entire exchange.

Only explicitly read-only calls use `read_only_exchange`. They make at most two attempts and retry only Timeout, HID++ 1.x Busy `0x07`, or HID++ 2.x Busy `0x08`. I/O errors, malformed responses, unsupported/invalid arguments, out-of-range errors, invalid feature/function errors, and every other protocol code return immediately. Battery GETs, DPI GETs, root feature discovery, protocol ping, and the post-write DPI readback use this helper. The DPI setter calls generic exchange directly once and never inherits read retry behavior.

Every physical attempt rotates to a new nonzero software ID. Matching remains strict on device index, feature index, function, and software ID. A late reply for the first attempt is ignored while the retry waits for its own software ID. Input is not drained, so notifications and unrelated application traffic are ignored through matching rather than destructively discarded.

These timeout and retry values are LogiPeek implementation policy, not requirements of the HID++ specification. The MIT-licensed [libratbag HID++ transport](https://github.com/libratbag/libratbag/blob/master/src/hidpp-generic.c) uses a one-second poll, while the permissively licensed [OpenLogi channel](https://github.com/AprilNEA/OpenLogi/blob/master/crates/openlogi-hidpp/src/channel.rs) uses a longer default request budget. LogiPeek keeps a smaller bound because it probes multiple possible receiver slots in a one-shot command. The HID++ 1.x and 2.x Busy codes were cross-checked against the public protocol material cited above; the Linux driver was used only as an additional behavior comparison.

### Phase 2.5 hardware observation

With the 750 ms window and two read-only attempts, `--devices` reached slot `0x01`, identified HID++ 4.2, and rediscovered `0x1004 v3` and `0x2201 v2`. An initial group of five independent `--battery` runs timed out during slot probing before any battery function was called. After the slot became reachable again, five consecutive `--battery` runs succeeded and consistently reported 42%, Good, rechargeable, Discharging, and raw external-power indicator `0x00`.

One complete `--dpi` run succeeded: sensor count 1; sensor 0 current DPI 1300; default DPI 800; supported range 100–25600; step 50. Two later DPI runs and a following device probe timed out before fn0 because slot `0x01` was no longer responding. The successful sample validates the implemented fn0/fn1/fn2 sequence and parsers on this device. The failed samples show that forwarded-endpoint availability remains intermittent and is distinct from a DPI read-stage error.

These Phase 2.5 observations were read-only. They do not by themselves claim that function 3 has been physically verified on the observed version-2 device; any Phase 3 write observation must be recorded separately with its acknowledgement and readback outcome.

A `logi_lamparray_service` process was present, but no G HUB application process was found. The service could not be stopped without administrator access, so this run did not establish whether it contributes concurrent HID++ traffic. No security setting or service configuration was changed.

### Phase 3 hardware observation

On 2026-09-19, the final Phase 3 read-only preflight again enumerated the `046D:C547` receiver. Usage 1 exposed only the HID++ 1.x endpoint at `0xFF`; usage 2 had no responding endpoint, so the fresh `--dpi` command could not obtain a current value, supported representation, or step. The required preflight therefore did not authorize `--set-dpi`: no function 3 request was sent, no physical DPI change was claimed, and no restore was necessary. `logi_lamparray_service` remained present throughout this observation and was not stopped, killed, or reconfigured.

Software ID rotation is not exclusive: input can be stale and other software can concurrently send HID++ traffic. IDs are four bits and repeat after 15 requests, which is a protocol-space limitation. Disconnects, sleep, permissions, malformed data, unsupported features, and timeouts become explicit errors. The hardware observation above validates one Windows USB receiver setup only; Bluetooth, direct USB mice, other receiver families, and other connection methods remain unverified.
