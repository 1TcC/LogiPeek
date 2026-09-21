use super::{
    features::{battery, dpi},
    hidpp::{self, Error, Feature, Protocol},
    transport::Transport,
};
use hidapi::HidApi;
use std::{ffi::CString, fmt};

#[derive(Debug)]
pub struct FeatureResult {
    pub id: u16,
    pub result: Result<Option<Feature>, Error>,
}
#[derive(Debug)]
pub struct Endpoint {
    pub index: u8,
    pub opaque_id: String,
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
struct DpiTargetIdentity {
    device_id: String,
    interface_number: usize,
    hid_interface_number: i32,
    vid: u16,
    pid: u16,
    usage_page: u16,
    usage: u16,
    product: String,
    device_index: u8,
    feature: Feature,
    sensor: dpi::SensorDpi,
}

#[derive(Clone)]
pub struct ValidatedDpiTarget {
    path: CString,
    identity: DpiTargetIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastDpiError {
    Unsupported,
    Invalidated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FastDpiSetResult {
    pub report: DpiSetReport,
    pub target_valid: bool,
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

fn dpi_identities(interfaces: &[Interface]) -> Option<Vec<DpiTargetIdentity>> {
    let mut targets = Vec::new();

    for interface in interfaces {
        for endpoint in &interface.endpoints {
            if !matches!(endpoint.protocol, Ok(Protocol::Feature { .. })) {
                continue;
            }
            let feature_result = endpoint.features.iter().find(|item| item.id == 0x2201)?;
            let feature = match &feature_result.result {
                Ok(Some(feature)) => *feature,
                Ok(None) => continue,
                Err(_) => return None,
            };
            let sensors = match &endpoint.dpi {
                Some(Ok(sensors)) if sensors.len() == 1 => sensors,
                _ => return None,
            };
            targets.push(DpiTargetIdentity {
                device_id: endpoint.opaque_id.clone(),
                interface_number: interface.number,
                hid_interface_number: interface.interface_number,
                vid: interface.vid,
                pid: interface.pid,
                usage_page: interface.usage_page,
                usage: interface.usage,
                product: interface.product.clone(),
                device_index: endpoint.index,
                feature,
                sensor: sensors[0].clone(),
            });
        }
    }
    Some(targets)
}

fn info_matches_identity(info: &hidapi::DeviceInfo, identity: &DpiTargetIdentity) -> bool {
    info.vendor_id() == identity.vid
        && info.product_id() == identity.pid
        && info.interface_number() == identity.hid_interface_number
        && info.usage_page() == identity.usage_page
        && info.usage() == identity.usage
        && safe_label(info.product_string().unwrap_or("Unavailable")) == identity.product
}

/// Creates an in-memory fast-path target only from a complete, unique DPI scan.
/// The HID path is deliberately private and is never persisted or displayed.
pub fn validated_dpi_target(
    interfaces: &[Interface],
    selected_device: Option<&str>,
) -> Option<ValidatedDpiTarget> {
    let identity = select_dpi_identity(dpi_identities(interfaces)?, selected_device)?;
    let api = HidApi::new().ok()?;
    let mut matches = api.device_list().filter(|info| {
        info_matches_identity(info, &identity)
            && opaque_device_id(
                info.path().to_bytes(),
                info.vendor_id(),
                info.product_id(),
                info.interface_number(),
                info.usage_page(),
                info.usage(),
                identity.device_index,
            ) == identity.device_id
    });
    let info = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(ValidatedDpiTarget {
        path: info.path().to_owned(),
        identity,
    })
}

fn select_dpi_identity(
    mut identities: Vec<DpiTargetIdentity>,
    selected_device: Option<&str>,
) -> Option<DpiTargetIdentity> {
    if identities.len() == 1 {
        return Some(identities.remove(0));
    }
    let selected = selected_device?;
    let index = identities
        .iter()
        .position(|identity| identity.device_id == selected)?;
    if identities
        .iter()
        .filter(|identity| identity.device_id == selected)
        .count()
        != 1
    {
        return None;
    }
    Some(identities.remove(index))
}

/// A battery-only scan can preserve the target only when the unique 0x2201
/// endpoint still has the same public HID and HID++ identity.
pub fn validated_target_matches_scan(
    target: &ValidatedDpiTarget,
    interfaces: &[Interface],
) -> bool {
    let mut matched = false;
    for interface in interfaces {
        for endpoint in &interface.endpoints {
            if !matches!(endpoint.protocol, Ok(Protocol::Feature { .. })) {
                continue;
            }
            let Some(feature_result) = endpoint.features.iter().find(|item| item.id == 0x2201)
            else {
                return false;
            };
            let feature = match &feature_result.result {
                Ok(Some(feature)) => *feature,
                Ok(None) => continue,
                Err(_) => return false,
            };
            if endpoint.opaque_id == target.identity.device_id {
                if matched
                    || interface.interface_number != target.identity.hid_interface_number
                    || interface.vid != target.identity.vid
                    || interface.pid != target.identity.pid
                    || interface.usage_page != target.identity.usage_page
                    || interface.usage != target.identity.usage
                    || interface.product != target.identity.product
                    || endpoint.index != target.identity.device_index
                    || feature != target.identity.feature
                {
                    return false;
                }
                matched = true;
            }
        }
    }
    matched
}

impl ValidatedDpiTarget {
    pub fn device_id(&self) -> &str {
        &self.identity.device_id
    }
}

fn revalidation_matches(
    identity: &DpiTargetIdentity,
    feature: Feature,
    sensors: &[dpi::SensorDpi],
) -> bool {
    feature == identity.feature
        && sensors.len() == 1
        && sensors[0].sensor == identity.sensor.sensor
        && sensors[0].supported == identity.sensor.supported
}

fn outcome_keeps_target(outcome: &dpi::SetDpiOutcome) -> bool {
    matches!(
        outcome,
        dpi::SetDpiOutcome::Verified { .. } | dpi::SetDpiOutcome::TimedOutConfirmed { .. }
    )
}

/// Reopens and revalidates one previously unique target. Every identity and
/// capability check occurs before the single possible fn3 request.
pub fn set_validated_runtime_dpi(
    target: &mut ValidatedDpiTarget,
    requested: u16,
) -> Result<FastDpiSetResult, FastDpiError> {
    if !dpi::supports_dpi(&target.identity.sensor.supported, requested) {
        return Err(FastDpiError::Unsupported);
    }

    let api = HidApi::new().map_err(|_| FastDpiError::Invalidated)?;
    let mut matches = api.device_list().filter(|info| {
        info.path() == target.path.as_c_str() && info_matches_identity(info, &target.identity)
    });
    let info = matches.next().ok_or(FastDpiError::Invalidated)?;
    if matches.next().is_some() {
        return Err(FastDpiError::Invalidated);
    }
    let handle = info
        .open_device(&api)
        .map_err(|_| FastDpiError::Invalidated)?;
    let mut transport = Transport::new(handle, info.usage() == 2);
    if !matches!(
        hidpp::probe(&mut transport, target.identity.device_index),
        Ok(Protocol::Feature { .. })
    ) {
        return Err(FastDpiError::Invalidated);
    }
    let feature = hidpp::discover(&mut transport, target.identity.device_index, 0x2201)
        .map_err(|_| FastDpiError::Invalidated)?
        .ok_or(FastDpiError::Invalidated)?;
    let sensors = dpi::read(&mut transport, target.identity.device_index, feature)
        .map_err(|_| FastDpiError::Invalidated)?;
    if !revalidation_matches(&target.identity, feature, &sensors) {
        return Err(FastDpiError::Invalidated);
    }

    let previous = sensors[0].current;
    let outcome = dpi::set_and_verify(
        &mut transport,
        target.identity.device_index,
        feature,
        1,
        &sensors[0],
        requested,
    )
    .map_err(|_| FastDpiError::Invalidated)?;
    let target_valid = outcome_keeps_target(&outcome);
    if let dpi::SetDpiOutcome::Verified { current }
    | dpi::SetDpiOutcome::TimedOutConfirmed { current } = &outcome
    {
        target.identity.sensor.current = *current;
    }
    Ok(FastDpiSetResult {
        report: DpiSetReport {
            interface_number: target.identity.interface_number,
            vid: target.identity.vid,
            pid: target.identity.pid,
            product: target.identity.product.clone(),
            device_index: target.identity.device_index,
            feature_version: feature.version,
            previous,
            requested,
            outcome,
        },
        target_valid,
    })
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
                            opaque_id: opaque_device_id(
                                info.path().to_bytes(),
                                info.vendor_id(),
                                info.product_id(),
                                info.interface_number(),
                                info.usage_page(),
                                info.usage(),
                                index,
                            ),
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

fn opaque_device_id(
    path: &[u8],
    vid: u16,
    pid: u16,
    interface_number: i32,
    usage_page: u16,
    usage: u16,
    device_index: u8,
) -> String {
    fn hash(seed: u64, chunks: &[&[u8]]) -> u64 {
        let mut value = seed;
        for chunk in chunks {
            for byte in *chunk {
                value ^= u64::from(*byte);
                value = value.wrapping_mul(0x100000001b3);
            }
            value ^= 0xff;
            value = value.wrapping_mul(0x100000001b3);
        }
        value
    }
    let vid = vid.to_le_bytes();
    let pid = pid.to_le_bytes();
    let interface_number = interface_number.to_le_bytes();
    let usage_page = usage_page.to_le_bytes();
    let usage = usage.to_le_bytes();
    let index = [device_index];
    let chunks = [
        path,
        vid.as_slice(),
        pid.as_slice(),
        interface_number.as_slice(),
        usage_page.as_slice(),
        usage.as_slice(),
        index.as_slice(),
    ];
    format!(
        "{:016x}{:016x}",
        hash(0xcbf29ce484222325, &chunks),
        hash(0x84222325cbf29ce4, &chunks)
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dpi_feature() -> Feature {
        Feature {
            id: 0x2201,
            index: 9,
            flags: 0,
            version: 2,
        }
    }

    fn sensor() -> dpi::SensorDpi {
        dpi::SensorDpi {
            sensor: 0,
            current: 1300,
            default: Some(800),
            supported: dpi::DpiValues::Range {
                minimum: 100,
                maximum: 25_600,
                step: 50,
            },
        }
    }

    fn interface(device_index: u8, feature: Result<Option<Feature>, Error>) -> Interface {
        let dpi = feature
            .as_ref()
            .ok()
            .and_then(|value| *value)
            .map(|_| Ok(vec![sensor()]));
        Interface {
            number: 1,
            vid: 0x046d,
            pid: 0xc547,
            product: "Test Mouse".into(),
            manufacturer: "Logitech".into(),
            interface_number: 2,
            usage_page: 0xff00,
            usage: 2,
            candidate: true,
            descriptor_bytes: Some(64),
            open_error: None,
            endpoints: vec![Endpoint {
                index: device_index,
                opaque_id: format!("device-{device_index}"),
                protocol: Ok(Protocol::Feature { major: 4, minor: 5 }),
                features: vec![FeatureResult {
                    id: 0x2201,
                    result: feature,
                }],
                battery: None,
                dpi,
            }],
        }
    }

    fn identity() -> DpiTargetIdentity {
        dpi_identities(&[interface(1, Ok(Some(dpi_feature())))])
            .expect("valid topology")
            .remove(0)
    }

    #[test]
    fn full_scan_requires_exactly_one_complete_dpi_target() {
        assert_eq!(dpi_identities(&[]), Some(Vec::new()));
        assert_eq!(
            dpi_identities(&[interface(1, Ok(Some(dpi_feature())))])
                .expect("valid topology")
                .len(),
            1
        );
        assert_eq!(
            dpi_identities(&[
                interface(1, Ok(Some(dpi_feature()))),
                interface(2, Ok(Some(dpi_feature()))),
            ])
            .expect("valid topology")
            .len(),
            2
        );
        assert!(dpi_identities(&[interface(1, Err(Error::Timeout))]).is_none());
    }

    #[test]
    fn identity_or_capability_change_fails_before_write() {
        let identity = identity();
        let mut wrong_feature = dpi_feature();
        wrong_feature.version += 1;
        assert!(!revalidation_matches(&identity, wrong_feature, &[sensor()]));

        let mut changed_sensor = sensor();
        changed_sensor.supported = dpi::DpiValues::List(vec![400, 800, 1600]);
        assert!(!revalidation_matches(
            &identity,
            dpi_feature(),
            &[changed_sensor]
        ));
        assert!(revalidation_matches(&identity, dpi_feature(), &[sensor()]));
    }

    #[test]
    fn topology_mismatch_invalidates_cached_target() {
        let identity = identity();
        let target = ValidatedDpiTarget {
            path: CString::new("test").unwrap(),
            identity,
        };
        assert!(validated_target_matches_scan(
            &target,
            &[interface(1, Ok(Some(dpi_feature())))]
        ));
        assert!(!validated_target_matches_scan(
            &target,
            &[interface(2, Ok(Some(dpi_feature())))]
        ));
        assert!(!validated_target_matches_scan(
            &target,
            &[interface(1, Err(Error::Timeout))]
        ));
    }

    #[test]
    fn only_independently_confirmed_outcomes_keep_target() {
        assert!(outcome_keeps_target(&dpi::SetDpiOutcome::Verified {
            current: 1350
        }));
        assert!(outcome_keeps_target(
            &dpi::SetDpiOutcome::TimedOutConfirmed { current: 1350 }
        ));
        assert!(!outcome_keeps_target(
            &dpi::SetDpiOutcome::AcknowledgedMismatch { actual: 1300 }
        ));
        assert!(!outcome_keeps_target(
            &dpi::SetDpiOutcome::TimedOutDifferent { actual: 1300 }
        ));
        assert!(!outcome_keeps_target(
            &dpi::SetDpiOutcome::AcknowledgedUnverified {
                error: Error::Timeout
            }
        ));
        assert!(!outcome_keeps_target(
            &dpi::SetDpiOutcome::TimedOutUnverified {
                error: Error::Timeout
            }
        ));
    }

    #[test]
    fn cached_capability_rejects_unsupported_dpi() {
        let identity = identity();
        assert!(dpi::supports_dpi(&identity.sensor.supported, 1350));
        assert!(!dpi::supports_dpi(&identity.sensor.supported, 1325));
    }

    #[test]
    fn opaque_identity_is_stable_and_separates_slots() {
        let first = opaque_device_id(b"path", 0x046d, 0xc547, 2, 0xff00, 2, 1);
        assert_eq!(first.len(), 32);
        assert_eq!(
            first,
            opaque_device_id(b"path", 0x046d, 0xc547, 2, 0xff00, 2, 1)
        );
        assert_ne!(
            first,
            opaque_device_id(b"path", 0x046d, 0xc547, 2, 0xff00, 2, 2)
        );
        assert_ne!(
            first,
            opaque_device_id(b"other", 0x046d, 0xc547, 2, 0xff00, 2, 1)
        );
    }

    #[test]
    fn multiple_targets_require_one_explicit_matching_identity() {
        let identities = dpi_identities(&[
            interface(1, Ok(Some(dpi_feature()))),
            interface(2, Ok(Some(dpi_feature()))),
        ])
        .expect("valid topology");
        assert!(select_dpi_identity(identities.clone(), None).is_none());
        assert!(select_dpi_identity(identities.clone(), Some("missing")).is_none());
        let selected =
            select_dpi_identity(identities, Some("device-2")).expect("explicit unique selection");
        assert_eq!(selected.device_id, "device-2");
    }
}
