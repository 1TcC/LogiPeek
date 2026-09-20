pub use super::settings::DEFAULT_PRESETS;
use super::{
    settings::{Language, Settings, Theme},
    text::{TextKey, text},
};
use crate::hid::{
    device::{Endpoint, Interface},
    features::{
        battery::{Charging, Level},
        dpi::{self, DpiValues},
    },
    hidpp::Protocol,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationStatus {
    Idle,
    Applying(u16),
    Verified(u16),
    Failed(OperationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationError {
    WorkerBusy,
    Remains(u16),
    NotVerified,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsNotice {
    Saved,
    SaveFailed,
    InvalidPreset,
}

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
    pub battery_level: Option<Level>,
    pub charging: Option<Charging>,
    pub current_dpi: Option<u16>,
    pub supported_dpi: Option<DpiValues>,
    pub presets: [u16; 4],
    pub theme: Theme,
    pub language: Option<Language>,
    pub operation: OperationStatus,
    pub settings_notice: Option<SettingsNotice>,
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
            battery_level: None,
            charging: None,
            current_dpi: None,
            supported_dpi: None,
            presets: Settings::default().presets,
            theme: Theme::System,
            language: None,
            operation: OperationStatus::Idle,
            settings_notice: None,
            target: None,
        }
    }
}

impl AppState {
    pub fn from_scan(interfaces: &[Interface]) -> Self {
        Self::from_scan_with_settings(interfaces, &Settings::default())
    }

    pub fn from_scan_with_settings(interfaces: &[Interface], settings: &Settings) -> Self {
        let targets = targets(interfaces);
        if targets.len() > 1 {
            return Self {
                status: DeviceStatus::MultipleDevices,
                presets: settings.presets,
                theme: settings.theme,
                language: settings.language,
                ..Self::default()
            };
        }
        let Some((interface, endpoint)) = targets.first().copied() else {
            return Self {
                presets: settings.presets,
                theme: settings.theme,
                language: settings.language,
                ..Self::default()
            };
        };
        let battery = endpoint
            .battery
            .as_ref()
            .and_then(|result| result.as_ref().ok());
        let sensor = endpoint
            .dpi
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .and_then(|sensors| (sensors.len() == 1).then(|| sensors[0].clone()));
        Self {
            status: DeviceStatus::Single,
            product: Some(interface.product.clone()),
            battery_percent: battery.and_then(|battery| battery.percentage),
            battery_level: battery.and_then(|battery| battery.level.clone()),
            charging: battery.map(|battery| battery.charging.clone()),
            current_dpi: sensor.as_ref().map(|value| value.current),
            supported_dpi: sensor.map(|value| value.supported),
            presets: settings.presets,
            theme: settings.theme,
            language: settings.language,
            operation: OperationStatus::Idle,
            settings_notice: None,
            target: Some(TargetKey::new(interface, endpoint)),
        }
    }

    pub fn replace_from_scan(&mut self, interfaces: &[Interface]) {
        let settings = self.settings();
        let operation = self.operation.clone();
        let notice = self.settings_notice;
        *self = Self::from_scan_with_settings(interfaces, &settings);
        self.operation = operation;
        self.settings_notice = notice;
    }

    pub fn apply_settings(&mut self, settings: &Settings) {
        self.presets = settings.presets;
        self.theme = settings.theme;
        self.language = settings.language;
    }

    pub fn settings(&self) -> Settings {
        Settings {
            presets: self.presets,
            theme: self.theme,
            language: self.language,
        }
    }

    /// A battery-only scan retains DPI only when it identifies the same sole
    /// endpoint. Any ambiguity or disappearance clears stale device data.
    pub fn apply_battery_scan(&mut self, interfaces: &[Interface]) {
        let settings = self.settings();
        let operation = self.operation.clone();
        let notice = self.settings_notice;
        let targets = targets(interfaces);
        if targets.len() > 1 {
            *self = Self {
                status: DeviceStatus::MultipleDevices,
                presets: settings.presets,
                theme: settings.theme,
                language: settings.language,
                operation,
                settings_notice: notice,
                ..Self::default()
            };
            return;
        }
        let Some((interface, endpoint)) = targets.first().copied() else {
            *self = Self {
                presets: settings.presets,
                theme: settings.theme,
                language: settings.language,
                operation,
                settings_notice: notice,
                ..Self::default()
            };
            return;
        };
        let key = TargetKey::new(interface, endpoint);
        let battery = endpoint
            .battery
            .as_ref()
            .and_then(|result| result.as_ref().ok());
        if self.target.as_ref() != Some(&key) {
            *self = Self {
                status: DeviceStatus::Single,
                product: Some(interface.product.clone()),
                battery_percent: battery.and_then(|battery| battery.percentage),
                battery_level: battery.and_then(|battery| battery.level.clone()),
                charging: battery.map(|battery| battery.charging.clone()),
                presets: settings.presets,
                theme: settings.theme,
                language: settings.language,
                operation,
                settings_notice: notice,
                target: Some(key),
                ..Self::default()
            };
            return;
        }
        self.status = DeviceStatus::Single;
        self.product = Some(interface.product.clone());
        self.battery_percent = battery.and_then(|battery| battery.percentage);
        self.battery_level = battery.and_then(|battery| battery.level.clone());
        self.charging = battery.map(|battery| battery.charging.clone());
    }

    pub fn presets(&self) -> [PresetState; 4] {
        self.presets.map(|value| PresetState {
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
        let language = self.ui_language();
        match self.status {
            DeviceStatus::MultipleDevices => format!(
                "{}: {}",
                text(language, TextKey::Battery),
                text(language, TextKey::MultipleDevices)
            ),
            _ => match (self.battery_percent, &self.battery_level) {
                (Some(value), _) => format!("{}: {value}%", text(language, TextKey::Battery)),
                (None, Some(level)) => format!(
                    "{}: {}",
                    text(language, TextKey::Battery),
                    level_text(language, level)
                ),
                _ => format!(
                    "{}: {}",
                    text(language, TextKey::Battery),
                    text(language, TextKey::Unavailable)
                ),
            },
        }
    }

    pub fn battery_value_text(&self) -> String {
        match (self.battery_percent, &self.battery_level) {
            (Some(value), _) => format!("{value}%"),
            (None, Some(level)) => level_text(self.ui_language(), level).into(),
            _ => "—".into(),
        }
    }

    pub fn dpi_text(&self) -> String {
        let language = self.ui_language();
        match self.status {
            DeviceStatus::MultipleDevices => {
                format!("DPI: {}", text(language, TextKey::MultipleDevices))
            }
            _ => self.current_dpi.map_or_else(
                || format!("DPI: {}", text(language, TextKey::Unavailable)),
                |value| format!("DPI: {value}"),
            ),
        }
    }

    pub fn tooltip(&self) -> String {
        let language = self.ui_language();
        match self.status {
            DeviceStatus::MultipleDevices => {
                format!("LogiPeek - {}", text(language, TextKey::MultipleDevices))
            }
            DeviceStatus::Unavailable => {
                format!("LogiPeek - {}", text(language, TextKey::DeviceUnavailable))
            }
            DeviceStatus::Single => match (self.battery_percent, self.current_dpi) {
                (Some(battery), Some(dpi)) => format!("LogiPeek - {battery}% - {dpi} DPI"),
                (Some(battery), None) => format!("LogiPeek - {battery}%"),
                (None, Some(dpi)) => format!("LogiPeek - {dpi} DPI"),
                (None, None) => {
                    format!("LogiPeek - {}", text(language, TextKey::DeviceUnavailable))
                }
            },
        }
    }

    pub fn connection_text(&self) -> &'static str {
        let language = self.ui_language();
        match self.status {
            DeviceStatus::Unavailable => text(language, TextKey::DeviceUnavailable),
            DeviceStatus::Single => text(language, TextKey::Connected),
            DeviceStatus::MultipleDevices => text(language, TextKey::MultipleDevicesDetected),
        }
    }

    pub fn charging_text(&self) -> &'static str {
        let language = self.ui_language();
        match self.charging {
            Some(Charging::Discharging) => text(language, TextKey::Discharging),
            Some(Charging::Charging | Charging::FinalStage | Charging::Slow) => {
                text(language, TextKey::Charging)
            }
            Some(Charging::Complete) => text(language, TextKey::Full),
            Some(Charging::InvalidBattery | Charging::ThermalError | Charging::Error) => {
                text(language, TextKey::BatteryError)
            }
            Some(Charging::Unknown(_)) | None => text(language, TextKey::StatusUnavailable),
        }
    }

    pub fn operation_text(&self) -> String {
        let language = self.ui_language();
        match &self.operation {
            OperationStatus::Idle => String::new(),
            OperationStatus::Applying(value) => {
                format!("{} {value} DPI...", text(language, TextKey::Applying))
            }
            OperationStatus::Verified(value) => {
                format!("{} {value} DPI", text(language, TextKey::Verified))
            }
            OperationStatus::Failed(OperationError::WorkerBusy) => {
                text(language, TextKey::WorkerBusy).into()
            }
            OperationStatus::Failed(OperationError::Remains(actual)) => match language {
                Language::English => {
                    format!("DPI remains {actual}; requested value was not verified")
                }
                Language::SimplifiedChinese => format!("DPI 仍为 {actual}；请求值未通过验证"),
            },
            OperationStatus::Failed(OperationError::NotVerified) => {
                text(language, TextKey::DpiNotVerified).into()
            }
            OperationStatus::Failed(OperationError::Failed) => {
                text(language, TextKey::DpiFailed).into()
            }
        }
    }

    pub fn settings_notice_text(&self) -> Option<&'static str> {
        let language = self.ui_language();
        self.settings_notice.map(|notice| match notice {
            SettingsNotice::Saved => text(language, TextKey::SettingsSaved),
            SettingsNotice::SaveFailed => text(language, TextKey::SettingsSaveFailed),
            SettingsNotice::InvalidPreset => text(language, TextKey::InvalidPreset),
        })
    }

    pub fn ui_language(&self) -> Language {
        self.language.unwrap_or(Language::English)
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

fn level_text(language: Language, level: &Level) -> &'static str {
    match level {
        Level::Critical => text(language, TextKey::Critical),
        Level::Low => text(language, TextKey::Low),
        Level::Good => text(language, TextKey::Good),
        Level::Full => text(language, TextKey::Full),
        Level::Unknown(_) => text(language, TextKey::Unknown),
    }
}
