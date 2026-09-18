# HID++ behavior in LogiPeek

This page documents what the current code sends and accepts. It is not a claim that every Logitech device, receiver, or Bluetooth interface behaves the same way. A narrow Windows USB receiver observation is recorded below; other connection methods remain unverified.

## Hardware observation

The following evidence was collected on **2026-09-19** on Windows. It records one observation and is not a mouse-model identification, a paired-device inventory, or a receiver-family compatibility claim.

| Item | Observed result |
| --- | --- |
| USB receiver | Logitech VID `046D`, PID `C547` |
| Enumeration | 6 interfaces total: two `0xFF00` query candidates (usage 1 and usage 2), plus four other enumerated interfaces |
| Native descriptor query | Both candidate interfaces opened successfully; each reconstructed report descriptor was 36 bytes |
| Usage 1 candidate | Endpoint `0xFF` identified HID++ 1.x (Legacy) |
| Usage 2 candidate | Slot `0x01` identified HID++ 4.2 |
| Dynamic features at slot `0x01` | `0x1004`: index `0x06`, version 3, flags 0; `0x2201`: index `0x0A`, version 2, flags 0 |
| Not found at slot `0x01` | `0x1000`, `0x1001`, and `0x2202` |
| Battery command | Completed normally; this device exposes `0x1004`, not the implemented `0x1000` v0 reader, so no real battery level was read |
| DPI | Dynamic detection verified; no DPI query or write was performed |
| Validation | Enumeration, protocol probing, and dynamic feature detection verified for this setup only |

## Sources

Protocol interpretation is informed by Logitech’s public HID++ documentation: [cpg-docs HID++ 2.0](https://github.com/Logitech/cpg-docs/tree/master/hidpp20), especially [0x0000 IRoot](https://github.com/Logitech/cpg-docs/blob/master/hidpp20/features/0x0000-IRoot.rst), and the public mirror of Logitech’s [HID++ 2.0 specification draft](https://lekensteyn.nl/files/logitech/logitech_hidpp_2.0_specification_draft_2012-06-04.pdf) for feature `0x1000`. This project does not copy GPL implementation code.

## Candidate interfaces and reports

Only Logitech vendor ID `0x046D` is enumerated. An interface is a HID++ query candidate when its usage page is `0xFF00` and usage is `1` or `2`. The implementation uses report ID `0x10` with 7 bytes for usage `1`, or report ID `0x11` with 20 bytes for usage `2`. This heuristic does not identify a receiver family or prove a device is paired.

A request is report ID, device index, feature index, function/software-ID byte, parameters, then zero padding to the selected report length. Functions fit in four bits. Each request rotates software ID through `1..15`.

Protocol packets themselves are exact-length: `Packet::parse` accepts only 7-byte `0x10` or 20-byte `0x11` slices. Windows can deliver a short report zero-padded to the collection's maximum input size. `transport::parse_input` first accepts only zero padding, never nonzero trailing data, with an upper input length of 64 bytes; it then gives `Packet::parse` the exact report slice.

Replies are accepted only when a parsed report has the selected device and matches the requested feature, function, and software ID. HID++ error markers `0x8F` (short legacy reports only) and `0xFF` are protocol errors only when their embedded feature/function identifiers match the request. Unrelated traffic is ignored. A malformed report that otherwise matches the request is an error.

## Endpoint probing and root feature

The program probes endpoint indices `0xFF`, `1`, `2`, `3`, `4`, `5`, and `6` with the HID++ root ping (feature `0`, function `1`, parameters `00 00 A5`). A HID++ 1.x error response with error code `1` identifies Legacy; a valid reply with major version at least 2 and echoed `A5` identifies HID++ 2.x.

For HID++ 2.x, feature discovery calls IRoot function `0` with a big-endian feature ID. A returned feature index of zero means unsupported except for feature ID zero itself. LogiPeek queries battery IDs `0x1000`, `0x1001`, `0x1004` and DPI IDs `0x2201`, `0x2202`.

Endpoint `0xFF` may mean a direct device or a receiver endpoint. Indices `1–6` are forwarded-slot candidates only. The scan neither lists paired devices nor verifies that a slot represents a mouse, receiver, or distinct physical device.

## Battery and DPI scope

When `0x1000` is found at version 0, LogiPeek calls function `1` for capabilities and function `0` for status. It requires at least two capability bytes and three status bytes. Exact percentage output depends on capability flags; without a percentage/mileage indication it uses a coarse level. Level zero is Unknown. Other `0x1000` versions, plus `0x1001` and `0x1004`, are not read.

`0x2201` and `0x2202` are capability detection only. There is no DPI read or write request.

## Timing and uncertainty

Each exchange is serial. Once the write completes, a matching reply has a 350 ms deadline and at most 128 reports are processed. The Windows backend write can itself take up to about one second, so 350 ms is not a bound on the entire exchange. Software ID rotation is not exclusive: input can be stale and other software can concurrently send HID++ traffic. Disconnects, sleep, permissions, malformed data, unsupported features, and timeouts become explicit errors. The hardware observation above validates one Windows USB receiver setup only; Bluetooth, direct USB mice, other receiver families, and other connection methods remain unverified.
