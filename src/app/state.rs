use crate::hid::{
    device::{Endpoint, Interface},
    features::dpi::{self, DpiValues},
    hidpp::Protocol,
};

pub const DEFAULT_PRESETS: [u16; 4] = [400, 800, 1600, 3200];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStatus {
    Unavailable,
    Single,
    MultipleDevices,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetState {
    pub dpi: u16,
    pub enabled: bool,
    pub checked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub status: DeviceStatus,
    pub product: Option<String>,
    pub battery_percent: Option<u8>,
    pub current_dpi: Option<u16>,
    pub supported_dpi: Option<DpiValues>,
    target: Option<TargetKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetKey {
    vid: u16,
    pid: u16,
    interface_number: i32,
    device_index: u8,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            status: DeviceStatus::Unavailable,
            product: None,
            battery_percent: None,
            current_dpi: None,
            supported_dpi: None,
            target: None,
        }
    }
}

impl AppState {
    pub fn from_scan(interfaces: &[Interface]) -> Self {
        let targets = targets(interfaces);
        if targets.len() > 1 {
            return Self {
                status: DeviceStatus::MultipleDevices,
                ..Self::default()
            };
        }
        let Some((interface, endpoint)) = targets.first().copied() else {
            return Self::default();
        };
        let sensor = endpoint
            .dpi
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .and_then(|sensors| (sensors.len() == 1).then(|| sensors[0].clone()));
        Self {
            status: DeviceStatus::Single,
            product: Some(interface.product.clone()),
            battery_percent: battery_percent(endpoint),
            current_dpi: sensor.as_ref().map(|value| value.current),
            supported_dpi: sensor.map(|value| value.supported),
            target: Some(TargetKey::new(interface, endpoint)),
        }
    }

    pub fn replace_from_scan(&mut self, interfaces: &[Interface]) {
        *self = Self::from_scan(interfaces);
    }

    /// A battery-only scan retains DPI only when it identifies the same sole
    /// endpoint. Any ambiguity or disappearance clears stale device data.
    pub fn apply_battery_scan(&mut self, interfaces: &[Interface]) {
        let targets = targets(interfaces);
        if targets.len() > 1 {
            *self = Self {
                status: DeviceStatus::MultipleDevices,
                ..Self::default()
            };
            return;
        }
        let Some((interface, endpoint)) = targets.first().copied() else {
            *self = Self::default();
            return;
        };
        let key = TargetKey::new(interface, endpoint);
        if self.target.as_ref() != Some(&key) {
            *self = Self {
                status: DeviceStatus::Single,
                product: Some(interface.product.clone()),
                battery_percent: battery_percent(endpoint),
                target: Some(key),
                ..Self::default()
            };
            return;
        }
        self.status = DeviceStatus::Single;
        self.product = Some(interface.product.clone());
        self.battery_percent = battery_percent(endpoint);
    }

    pub fn presets(&self) -> [PresetState; 4] {
        DEFAULT_PRESETS.map(|value| PresetState {
            dpi: value,
            enabled: self.status == DeviceStatus::Single
                && self
                    .supported_dpi
                    .as_ref()
                    .is_some_and(|supported| dpi::supports_dpi(supported, value)),
            checked: self.status == DeviceStatus::Single && self.current_dpi == Some(value),
        })
    }

    pub fn battery_text(&self) -> String {
        match self.status {
            DeviceStatus::MultipleDevices => "Battery: Multiple devices".into(),
            _ => self.battery_percent.map_or_else(
                || "Battery: Unavailable".into(),
                |value| format!("Battery: {value}%"),
            ),
        }
    }

    pub fn dpi_text(&self) -> String {
        match self.status {
            DeviceStatus::MultipleDevices => "DPI: Multiple devices".into(),
            _ => self.current_dpi.map_or_else(
                || "DPI: Unavailable".into(),
                |value| format!("DPI: {value}"),
            ),
        }
    }

    pub fn tooltip(&self) -> String {
        match self.status {
            DeviceStatus::MultipleDevices => "LogiPeek - Multiple devices".into(),
            DeviceStatus::Unavailable => "LogiPeek - Device unavailable".into(),
            DeviceStatus::Single => match (self.battery_percent, self.current_dpi) {
                (Some(battery), Some(dpi)) => format!("LogiPeek - {battery}% - {dpi} DPI"),
                (Some(battery), None) => format!("LogiPeek - {battery}%"),
                (None, Some(dpi)) => format!("LogiPeek - {dpi} DPI"),
                (None, None) => "LogiPeek - Device unavailable".into(),
            },
        }
    }
}

impl TargetKey {
    fn new(interface: &Interface, endpoint: &Endpoint) -> Self {
        Self {
            vid: interface.vid,
            pid: interface.pid,
            interface_number: interface.interface_number,
            device_index: endpoint.index,
        }
    }
}

fn targets(interfaces: &[Interface]) -> Vec<(&Interface, &Endpoint)> {
    interfaces
        .iter()
        .flat_map(|interface| {
            interface
                .endpoints
                .iter()
                .map(move |endpoint| (interface, endpoint))
        })
        .filter(|(_, endpoint)| {
            matches!(endpoint.protocol, Ok(Protocol::Feature { .. }))
                && endpoint
                    .features
                    .iter()
                    .any(|feature| matches!(feature.result, Ok(Some(_))))
        })
        .collect()
}

fn battery_percent(endpoint: &Endpoint) -> Option<u8> {
    endpoint
        .battery
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|battery| battery.percentage)
}
