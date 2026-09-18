#![forbid(unsafe_code)]
use logipeek::hid::{
    device::{self, Endpoint},
    features::{
        battery::{self, Charging, Level},
        dpi,
    },
    hidpp::Protocol,
};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match args.as_slice() {
        [] => "--help",
        [arg] if ["--help", "-h", "--devices", "--diag", "--battery"].contains(&arg.as_str()) => {
            arg
        }
        _ => {
            eprintln!("Usage: logipeek [--devices | --diag | --battery | --help]");
            return ExitCode::from(2);
        }
    };
    if matches!(mode, "--help" | "-h") {
        println!(
            "LogiPeek - Windows Logitech HID++ diagnostics\n\n--devices  List Logitech candidates and discovered capabilities\n--diag     Show safe interface and protocol diagnostics\n--battery  Read supported 0x1000 battery information\n\nOne-shot queries; no DPI writes, background service, or network requests."
        );
        return ExitCode::SUCCESS;
    }
    println!("LogiPeek\n");
    let interfaces = match device::scan(mode == "--battery") {
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
        }
        if mode == "--diag" {
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
            if mode == "--battery" {
                print_battery(endpoint);
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

fn print_battery(endpoint: &Endpoint) {
    match &endpoint.battery {
        Some(Ok(value)) => {
            match &value.level {
                Level::Percentage(p) => println!("      Battery: {p}% (device-reported mileage)"),
                level => println!("      Battery level: {level:?}"),
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
            println!("      Charging: {status}\n      External power: Unavailable");
        }
        Some(Err(error)) => println!("      Battery reading: Unavailable - {error}"),
        None => println!(
            "      Battery reading: Unavailable (only 0x1000 version 0 reading implemented)"
        ),
    }
}
