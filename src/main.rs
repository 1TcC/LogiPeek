#![deny(unsafe_op_in_unsafe_fn)]
#[cfg(windows)]
#[path = "app/tray.rs"]
mod tray;
use logipeek::hid::{
    device::{self, Endpoint, ScanOptions},
    features::{
        battery::{self, Charging},
        dpi::{self, DpiValues},
    },
    hidpp::{Protocol, READ_ONLY_ATTEMPTS},
    transport::RESPONSE_TIMEOUT,
};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        #[cfg(windows)]
        {
            return match tray::run() {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("Tray startup failed: {error}");
                    ExitCode::FAILURE
                }
            };
        }
        #[cfg(not(windows))]
        {
            eprintln!("Tray mode is available only on Windows.");
            return ExitCode::FAILURE;
        }
    }
    if let [flag, value] = args.as_slice()
        && flag == "--set-dpi"
    {
        let requested = match value.parse::<u16>() {
            Ok(value) => value,
            Err(_) => {
                eprintln!("Invalid DPI value: {value}. Expected an integer from 0 to 65535.");
                return ExitCode::from(2);
            }
        };
        return run_set_dpi(requested);
    }
    let mode = match args.as_slice() {
        [] => "--help",
        [arg]
            if ["--help", "-h", "--devices", "--diag", "--battery", "--dpi"]
                .contains(&arg.as_str()) =>
        {
            arg
        }
        _ => {
            eprintln!(
                "Usage: logipeek [--devices | --diag | --battery | --dpi | --set-dpi <DPI> | --help]"
            );
            return ExitCode::from(2);
        }
    };
    if matches!(mode, "--help" | "-h") {
        println!(
            "LogiPeek - Windows Logitech HID++ diagnostics\n\n(no arguments)   Start the Windows tray\n--devices         List Logitech candidates and discovered capabilities\n--diag            Show safe interface, protocol, battery, and DPI diagnostics\n--battery         Read supported 0x1004 or 0x1000 battery information\n--dpi             Read supported 0x2201 sensor DPI information\n--set-dpi <DPI>   Set runtime DPI on one unique target; rejects unsupported values; writes are never auto-retried\n\nLocal operations only; no profiles, persistence, background service, or network requests."
        );
        return ExitCode::SUCCESS;
    }
    println!("LogiPeek\n");
    if mode == "--diag" {
        println!(
            "Read policy: {} ms per request, up to {} attempts for timeout or HID++ Busy\n",
            RESPONSE_TIMEOUT.as_millis(),
            READ_ONLY_ATTEMPTS
        );
    }
    let interfaces = match device::scan(ScanOptions {
        read_battery: matches!(mode, "--battery" | "--diag"),
        read_dpi: matches!(mode, "--dpi" | "--diag"),
    }) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Device enumeration failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if interfaces.is_empty() {
        println!("No Logitech HID devices detected. Connect or wake your device and retry.");
        return ExitCode::SUCCESS;
    }
    println!("Logitech HID candidates (interfaces may refer to the same physical device):");
    for interface in &interfaces {
        if mode != "--diag" && !interface.candidate {
            continue;
        }
        println!(
            "\n[{}] {}\n    VID: {:04X}  PID: {:04X}",
            interface.number, interface.product, interface.vid, interface.pid
        );
        if mode == "--diag" {
            println!(
                "    Manufacturer: {}\n    Interface number: {}\n    Usage page: {:04X}  Usage: {:04X}\n    HID++ query candidate: {}",
                interface.manufacturer,
                interface.interface_number,
                interface.usage_page,
                interface.usage,
                interface.candidate
            );
            println!(
                "    HID path / serial: Omitted for privacy\n    Query report: {}\n    Receiver family / paired-device inventory: Unavailable",
                if interface.candidate {
                    if interface.usage == 2 {
                        "0x11 (20 bytes; usage-based candidate)"
                    } else {
                        "0x10 (7 bytes; usage-based candidate)"
                    }
                } else {
                    "Unavailable"
                }
            );
            match interface.descriptor_bytes {
                Some(size) => println!(
                    "    Report descriptor: {size} bytes (reconstructed by native HID backend)"
                ),
                None => println!("    Report descriptor: Unavailable"),
            }
        }
        if let Some(error) = &interface.open_error {
            println!("    HID++: Unknown - {error}");
        }
        let mut found = false;
        for endpoint in &interface.endpoints {
            if mode != "--diag" && endpoint.protocol.is_err() {
                continue;
            }
            found |= endpoint.protocol.is_ok();
            println!(
                "    Device index: 0x{:02X} ({})",
                endpoint.index,
                if endpoint.index == 0xff {
                    "direct device or receiver endpoint"
                } else {
                    "forwarded slot candidate; type unverified"
                }
            );
            match &endpoint.protocol {
                Ok(Protocol::Legacy) => {
                    println!("      HID++: 1.x (register features not implemented)")
                }
                Ok(Protocol::Feature { major, minor }) => {
                    println!("      HID++: {major}.{minor} (feature protocol)")
                }
                Err(error) => println!("      HID++: Unknown - {error}"),
            }
            println!(
                "      Battery: {}\n      DPI: {}",
                device::capability(endpoint, &battery::FEATURE_IDS),
                device::capability(endpoint, &dpi::FEATURE_IDS)
            );
            if mode == "--diag" {
                for feature in &endpoint.features {
                    match &feature.result {
                        Ok(Some(value)) => println!(
                            "      Feature 0x{:04X}: index 0x{:02X}, version {}, flags 0x{:02X}",
                            value.id, value.index, value.version, value.flags
                        ),
                        Ok(None) => println!("      Feature 0x{:04X}: Unsupported", feature.id),
                        Err(error) => {
                            println!("      Feature 0x{:04X}: Unavailable - {error}", feature.id)
                        }
                    }
                }
            }
            if mode == "--battery" || (mode == "--diag" && endpoint.battery.is_some()) {
                print_battery(endpoint);
            }
            if mode == "--dpi" || (mode == "--diag" && endpoint.dpi.is_some()) {
                print_dpi(endpoint);
            }
        }
        if interface.candidate && !found && interface.open_error.is_none() {
            println!("    HID++ / Battery / DPI: Unknown (no responding endpoint)");
        }
    }
    if !interfaces.iter().any(|i| i.candidate) {
        println!(
            "\nNo supported HID++ interface candidates. Use --diag to inspect enumerated interfaces."
        );
    }
    println!(
        "\nDetection is not a compatibility guarantee; receiver names do not identify paired mice."
    );
    ExitCode::SUCCESS
}

fn run_set_dpi(requested: u16) -> ExitCode {
    println!("LogiPeek\n\nRunning fresh DPI preflight for requested value {requested}...");
    let report = match device::set_unique_runtime_dpi(requested) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("DPI change refused: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "Target: interface {}, {:04X}:{:04X} {}, device 0x{:02X}, 0x2201 v{}\nPrevious DPI: {}\nRequested DPI: {}",
        report.interface_number,
        report.vid,
        report.pid,
        report.product,
        report.device_index,
        report.feature_version,
        report.previous,
        report.requested
    );
    match report.outcome {
        dpi::SetDpiOutcome::Verified { current } => {
            println!("Result: write acknowledged and read-back verified {current} DPI");
            ExitCode::SUCCESS
        }
        dpi::SetDpiOutcome::AcknowledgedMismatch { actual } => {
            eprintln!(
                "Result: write acknowledged, but read-back reported {actual} DPI; write was not retried"
            );
            ExitCode::FAILURE
        }
        dpi::SetDpiOutcome::AcknowledgedUnverified { error } => {
            eprintln!(
                "Result: write acknowledged, but read-back was unavailable ({error}); write was not retried"
            );
            ExitCode::FAILURE
        }
        dpi::SetDpiOutcome::TimedOutConfirmed { current } => {
            println!(
                "Result: setter acknowledgement timed out, but safe read-back confirms {current} DPI; write was not retried"
            );
            ExitCode::SUCCESS
        }
        dpi::SetDpiOutcome::TimedOutDifferent { actual } => {
            eprintln!(
                "Result: setter acknowledgement timed out; safe read-back reports {actual} DPI; write was not retried"
            );
            ExitCode::FAILURE
        }
        dpi::SetDpiOutcome::TimedOutUnverified { error } => {
            eprintln!(
                "Result: setter acknowledgement timed out and read-back was unavailable ({error}); outcome is unknown and the write was not retried"
            );
            ExitCode::FAILURE
        }
    }
}

fn print_battery(endpoint: &Endpoint) {
    match &endpoint.battery {
        Some(Ok(value)) => {
            println!(
                "      Battery feature: 0x{:04X} v{}",
                value.feature_id, value.feature_version
            );
            if let Some(percentage) = value.percentage {
                println!("      Battery: {percentage}%");
            }
            match &value.level {
                Some(level) => println!("      Battery level: {level:?}"),
                None if value.percentage.is_none() => println!("      Battery level: Unavailable"),
                None => {}
            }
            if let Some(rechargeable) = value.rechargeable {
                println!(
                    "      Rechargeable: {}",
                    if rechargeable { "Yes" } else { "No" }
                );
            }
            let status = match value.charging {
                Charging::Discharging => "No (discharging)",
                Charging::Charging => "Yes",
                Charging::FinalStage => "Yes (final stage)",
                Charging::Complete => "Complete",
                Charging::Slow => "Yes (slow)",
                Charging::InvalidBattery => "Invalid battery",
                Charging::ThermalError => "Thermal error",
                Charging::Error => "Charging error",
                Charging::Unknown(_) => "Unknown",
            };
            println!("      Charging: {status}");
            match value.external_power_raw {
                Some(raw) => println!(
                    "      External power: Unknown (raw indicator 0x{raw:02X}; public value semantics unavailable)"
                ),
                None => println!("      External power: Unavailable"),
            }
        }
        Some(Err(error)) => println!("      Battery reading: Unavailable - {error}"),
        None => println!("      Battery reading: Unsupported or no implemented reader"),
    }
}

fn print_dpi(endpoint: &Endpoint) {
    match &endpoint.dpi {
        Some(Ok(sensors)) => {
            if let Some(feature) =
                endpoint
                    .features
                    .iter()
                    .find_map(|result| match &result.result {
                        Ok(Some(feature)) if feature.id == 0x2201 => Some(feature),
                        _ => None,
                    })
            {
                println!("      DPI feature: 0x2201 v{}", feature.version);
            }
            println!("      Sensor count: {}", sensors.len());
            for sensor in sensors {
                println!("      Sensor {}", sensor.sensor);
                println!("        Current DPI: {}", sensor.current);
                match sensor.default {
                    Some(value) => println!("        Default DPI: {value}"),
                    None => println!("        Default DPI: Unavailable"),
                }
                match &sensor.supported {
                    DpiValues::List(values) => {
                        let values = values
                            .iter()
                            .map(u16::to_string)
                            .collect::<Vec<_>>()
                            .join(", ");
                        println!("        Supported DPI: {values}");
                    }
                    DpiValues::Range {
                        minimum,
                        maximum,
                        step,
                    } => {
                        println!("        Supported DPI: {minimum}-{maximum}");
                        println!("        Step: {step}");
                    }
                }
            }
        }
        Some(Err(error)) => println!("      DPI reading: Unavailable - {error}"),
        None => println!("      DPI reading: Unsupported or no implemented reader"),
    }
}
