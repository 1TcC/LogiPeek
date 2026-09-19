use super::{
    features::{battery, dpi},
    hidpp::{self, Error, Feature, Protocol},
    transport::Transport,
};
use hidapi::HidApi;
use std::fmt;

#[derive(Debug)]
pub struct FeatureResult {
    pub id: u16,
    pub result: Result<Option<Feature>, Error>,
}
#[derive(Debug)]
pub struct Endpoint {
    pub index: u8,
    pub protocol: Result<Protocol, Error>,
    pub features: Vec<FeatureResult>,
    pub battery: Option<Result<battery::Battery, Error>>,
    pub dpi: Option<Result<Vec<dpi::SensorDpi>, Error>>,
}
#[derive(Debug)]
pub struct Interface {
    pub number: usize,
    pub vid: u16,
    pub pid: u16,
    pub product: String,
    pub manufacturer: String,
    pub interface_number: i32,
    pub usage_page: u16,
    pub usage: u16,
    pub candidate: bool,
    pub descriptor_bytes: Option<usize>,
    pub open_error: Option<Error>,
    pub endpoints: Vec<Endpoint>,
}

/// Retain endpoint identity within its HID interface. Never merge devices by PID/name.
/// Slots are bounded probes, not claims about pairing or receiver type.
pub const PROBE_INDICES: [u8; 7] = [0xff, 1, 2, 3, 4, 5, 6];

#[derive(Debug, Clone, Copy, Default)]
pub struct ScanOptions {
    pub read_battery: bool,
    pub read_dpi: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpiSetReport {
    pub interface_number: usize,
    pub vid: u16,
    pub pid: u16,
    pub product: String,
    pub device_index: u8,
    pub feature_version: u8,
    pub previous: u16,
    pub requested: u16,
    pub outcome: dpi::SetDpiOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DpiSetTargetError {
    Enumeration(Error),
    Preflight {
        interface_number: usize,
        device_index: u8,
        error: Error,
    },
    MultipleSensors {
        interface_number: usize,
        device_index: u8,
        count: usize,
    },
    UnsupportedFeature,
    NoWritableTarget,
    MultipleWritableTargets(usize),
    Set(dpi::SetDpiError),
}

impl fmt::Display for DpiSetTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enumeration(error) => write!(f, "Device enumeration failed: {error}"),
            Self::Preflight {
                interface_number,
                device_index,
                error,
            } => write!(
                f,
                "DPI preflight failed on interface {interface_number}, device 0x{device_index:02X}: {error}; no write was sent"
            ),
            Self::MultipleSensors {
                interface_number,
                device_index,
                count,
            } => write!(
                f,
                "Interface {interface_number}, device 0x{device_index:02X} reports {count} sensors; this phase refuses multi-sensor writes"
            ),
            Self::UnsupportedFeature => {
                f.write_str("Reachable HID++ endpoints do not expose adjustable DPI feature 0x2201")
            }
            Self::NoWritableTarget => {
                f.write_str("No currently writable 0x2201 endpoint passed the fresh DPI preflight")
            }
            Self::MultipleWritableTargets(count) => write!(
                f,
                "Found {count} currently writable 0x2201 endpoints; refusing to choose a target"
            ),
            Self::Set(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for DpiSetTargetError {}

struct PreparedDpiTarget {
    transport: Transport,
    interface_number: usize,
    vid: u16,
    pid: u16,
    product: String,
    device_index: u8,
    feature: Feature,
    sensor: dpi::SensorDpi,
}

/// Performs a fresh capability preflight across all candidates and sends a
/// write only when exactly one currently usable 0x2201 endpoint is found.
pub fn set_unique_runtime_dpi(requested: u16) -> Result<DpiSetReport, DpiSetTargetError> {
    let api = HidApi::new().map_err(|_| DpiSetTargetError::Enumeration(Error::Io))?;
    let mut targets = Vec::new();
    let mut feature_endpoints = 0usize;
    let mut unsupported_endpoints = 0usize;

    for (offset, info) in api
        .device_list()
        .filter(|device| device.vendor_id() == 0x046d)
        .enumerate()
    {
        if info.usage_page() != 0xff00 || !matches!(info.usage(), 1 | 2) {
            continue;
        }
        let interface_number = offset + 1;
        let Ok(handle) = info.open_device(&api) else {
            continue;
        };
        let mut transport = Transport::new(handle, info.usage() == 2);
        let mut endpoint_target = None;

        for device_index in PROBE_INDICES {
            if !matches!(
                hidpp::probe(&mut transport, device_index),
                Ok(Protocol::Feature { .. })
            ) {
                continue;
            }
            feature_endpoints += 1;
            let feature =
                hidpp::discover(&mut transport, device_index, 0x2201).map_err(|error| {
                    DpiSetTargetError::Preflight {
                        interface_number,
                        device_index,
                        error: error.in_read("0x2201 fresh feature discovery"),
                    }
                })?;
            let Some(feature) = feature else {
                unsupported_endpoints += 1;
                continue;
            };
            let sensors = dpi::read(&mut transport, device_index, feature).map_err(|error| {
                DpiSetTargetError::Preflight {
                    interface_number,
                    device_index,
                    error,
                }
            })?;
            if sensors.len() != 1 {
                return Err(DpiSetTargetError::MultipleSensors {
                    interface_number,
                    device_index,
                    count: sensors.len(),
                });
            }
            if endpoint_target.is_some() {
                return Err(DpiSetTargetError::MultipleWritableTargets(2));
            }
            endpoint_target = Some((device_index, feature, sensors[0].clone()));
        }

        if let Some((device_index, feature, sensor)) = endpoint_target {
            targets.push(PreparedDpiTarget {
                transport,
                interface_number,
                vid: info.vendor_id(),
                pid: info.product_id(),
                product: safe_label(info.product_string().unwrap_or("Unavailable")),
                device_index,
                feature,
                sensor,
            });
        }
    }

    if targets.is_empty() {
        if feature_endpoints > 0 && unsupported_endpoints == feature_endpoints {
            return Err(DpiSetTargetError::UnsupportedFeature);
        }
        return Err(DpiSetTargetError::NoWritableTarget);
    }
    if targets.len() != 1 {
        return Err(DpiSetTargetError::MultipleWritableTargets(targets.len()));
    }
    let mut target = targets.remove(0);
    let previous = target.sensor.current;
    let outcome = dpi::set_and_verify(
        &mut target.transport,
        target.device_index,
        target.feature,
        1,
        &target.sensor,
        requested,
    )
    .map_err(DpiSetTargetError::Set)?;
    Ok(DpiSetReport {
        interface_number: target.interface_number,
        vid: target.vid,
        pid: target.pid,
        product: target.product,
        device_index: target.device_index,
        feature_version: target.feature.version,
        previous,
        requested,
        outcome,
    })
}

pub fn scan(options: ScanOptions) -> Result<Vec<Interface>, Error> {
    let api = HidApi::new().map_err(|_| Error::Io)?;
    let mut interfaces = Vec::new();
    for info in api.device_list().filter(|d| d.vendor_id() == 0x046d) {
        let candidate = info.usage_page() == 0xff00 && matches!(info.usage(), 1 | 2);
        let mut interface = Interface {
            number: interfaces.len() + 1,
            vid: info.vendor_id(),
            pid: info.product_id(),
            product: safe_label(info.product_string().unwrap_or("Unavailable")),
            manufacturer: safe_label(info.manufacturer_string().unwrap_or("Unavailable")),
            interface_number: info.interface_number(),
            usage_page: info.usage_page(),
            usage: info.usage(),
            candidate,
            descriptor_bytes: None,
            open_error: None,
            endpoints: Vec::new(),
        };
        if candidate {
            match info.open_device(&api) {
                Err(_) => interface.open_error = Some(Error::Io),
                Ok(handle) => {
                    let mut descriptor = [0u8; 4096];
                    interface.descriptor_bytes = handle
                        .get_report_descriptor(&mut descriptor)
                        .ok()
                        .filter(|n| *n > 0 && *n < descriptor.len());
                    let mut transport = Transport::new(handle, info.usage() == 2);
                    for index in PROBE_INDICES {
                        let protocol = hidpp::probe(&mut transport, index);
                        let mut endpoint = Endpoint {
                            index,
                            protocol,
                            features: Vec::new(),
                            battery: None,
                            dpi: None,
                        };
                        if matches!(endpoint.protocol, Ok(Protocol::Feature { .. })) {
                            for id in battery::FEATURE_IDS.into_iter().chain(dpi::FEATURE_IDS) {
                                endpoint.features.push(FeatureResult {
                                    id,
                                    result: hidpp::discover(&mut transport, index, id),
                                });
                            }
                            if options.read_battery {
                                let feature = endpoint
                                    .features
                                    .iter()
                                    .find_map(|f| match &f.result {
                                        Ok(Some(feature)) if feature.id == 0x1004 => Some(*feature),
                                        _ => None,
                                    })
                                    .or_else(|| {
                                        endpoint.features.iter().find_map(|f| match &f.result {
                                            Ok(Some(feature))
                                                if feature.id == 0x1000 && feature.version == 0 =>
                                            {
                                                Some(*feature)
                                            }
                                            _ => None,
                                        })
                                    });
                                if let Some(feature) = feature {
                                    endpoint.battery =
                                        Some(battery::read(&mut transport, index, feature));
                                }
                            }
                            if options.read_dpi {
                                let feature =
                                    endpoint.features.iter().find_map(|f| match &f.result {
                                        Ok(Some(feature)) if feature.id == 0x2201 => Some(*feature),
                                        _ => None,
                                    });
                                if let Some(feature) = feature {
                                    endpoint.dpi = Some(dpi::read(&mut transport, index, feature));
                                }
                            }
                        }
                        interface.endpoints.push(endpoint);
                    }
                }
            }
        }
        interfaces.push(interface);
    }
    Ok(interfaces)
}

/// HID metadata is untrusted; avoid control/terminal escape injection and huge output.
pub fn safe_label(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).take(96).collect()
}

pub fn capability(endpoint: &Endpoint, ids: &[u16]) -> &'static str {
    if ids.iter().any(|id| {
        endpoint
            .features
            .iter()
            .any(|f| f.id == *id && matches!(f.result, Ok(Some(_))))
    }) {
        return "Supported";
    }
    if ids.iter().all(|id| {
        endpoint
            .features
            .iter()
            .any(|f| f.id == *id && matches!(f.result, Ok(None)))
    }) {
        return "Unsupported (queried features)";
    }
    "Unknown"
}
