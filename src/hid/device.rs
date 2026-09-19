use super::{
    features::{battery, dpi},
    hidpp::{self, Error, Feature, Protocol},
    transport::Transport,
};
use hidapi::HidApi;

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
