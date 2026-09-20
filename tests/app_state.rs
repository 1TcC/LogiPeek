use logipeek::app::{
    settings::{Settings, Theme},
    state::{AppState, DEFAULT_PRESETS, DeviceStatus, OperationError, OperationStatus},
};
use logipeek::hid::{
    device::{Endpoint, FeatureResult, Interface},
    features::{
        battery::{Battery, Charging},
        dpi::{DpiValues, SensorDpi},
    },
    hidpp::{Feature, Protocol},
};

fn interface(
    index: i32,
    endpoint_index: u8,
    product: &str,
    dpi: Option<Vec<SensorDpi>>,
    battery: Option<u8>,
) -> Interface {
    Interface {
        number: index as usize,
        vid: 0x046d,
        pid: 0xc539,
        product: product.into(),
        manufacturer: "Logitech".into(),
        interface_number: index,
        usage_page: 0xff00,
        usage: 1,
        candidate: true,
        descriptor_bytes: None,
        open_error: None,
        endpoints: vec![Endpoint {
            index: endpoint_index,
            protocol: Ok(Protocol::Feature { major: 2, minor: 0 }),
            features: vec![FeatureResult {
                id: 0x2201,
                result: Ok(Some(Feature {
                    id: 0x2201,
                    index: 9,
                    flags: 0,
                    version: 3,
                })),
            }],
            battery: battery.map(|percentage| {
                Ok(Battery {
                    feature_id: 0x1004,
                    feature_version: 3,
                    percentage: Some(percentage),
                    level: None,
                    charging: Charging::Discharging,
                    rechargeable: Some(true),
                    external_power_raw: Some(0),
                })
            }),
            dpi: dpi.map(Ok),
        }],
    }
}

fn sensor(current: u16, supported: DpiValues) -> SensorDpi {
    SensorDpi {
        sensor: 0,
        current,
        default: None,
        supported,
    }
}

#[test]
fn presets_are_enabled_and_current_value_is_checked() {
    let state = AppState::from_scan(&[interface(
        1,
        2,
        "Mouse",
        Some(vec![sensor(800, DpiValues::List(vec![400, 800]))]),
        Some(90),
    )]);
    assert_eq!(state.status, DeviceStatus::Single);
    let presets = state.presets();
    assert_eq!(DEFAULT_PRESETS, [400, 800, 1600, 3200]);
    assert_eq!((presets[0].enabled, presets[0].checked), (true, false));
    assert_eq!((presets[1].enabled, presets[1].checked), (true, true));
    assert_eq!((presets[2].enabled, presets[2].checked), (false, false));
}

#[test]
fn tooltip_and_unavailable_state_are_stable() {
    let state = AppState::from_scan(&[]);
    assert_eq!(state.status, DeviceStatus::Unavailable);
    assert_eq!(state.tooltip(), "LogiPeek - Device unavailable");
    assert_eq!(state.battery_text(), "Battery: Unavailable");
    assert_eq!(state.dpi_text(), "DPI: Unavailable");
    let single = AppState::from_scan(&[interface(1, 2, "Mouse", None, Some(77))]);
    assert_eq!(single.tooltip(), "LogiPeek - 77%");
}

#[test]
fn multiple_devices_disable_presets_and_use_safe_text() {
    let interfaces = [
        interface(1, 2, "A", None, Some(80)),
        interface(2, 3, "B", None, Some(70)),
    ];
    let state = AppState::from_scan(&interfaces);
    assert_eq!(state.status, DeviceStatus::MultipleDevices);
    assert!(
        state
            .presets()
            .iter()
            .all(|preset| !preset.enabled && !preset.checked)
    );
    assert_eq!(state.tooltip(), "LogiPeek - Multiple devices");
    assert_eq!(state.battery_text(), "Battery: Multiple devices");
}

#[test]
fn replace_from_scan_clears_stale_state() {
    let mut state = AppState::from_scan(&[interface(
        1,
        2,
        "Mouse",
        Some(vec![sensor(800, DpiValues::List(vec![800]))]),
        Some(90),
    )]);
    state.replace_from_scan(&[]);
    assert_eq!(state.status, DeviceStatus::Unavailable);
    assert!(
        state
            .presets()
            .iter()
            .all(|preset| !preset.enabled && !preset.checked)
    );
    assert_eq!(state.tooltip(), "LogiPeek - Device unavailable");
}

#[test]
fn battery_scan_preserves_dpi_only_for_same_sole_target() {
    let full = interface(
        1,
        2,
        "Mouse",
        Some(vec![sensor(800, DpiValues::List(vec![800]))]),
        Some(90),
    );
    let mut state = AppState::from_scan(&[full]);
    let battery_only = interface(1, 2, "Mouse", None, Some(55));
    state.apply_battery_scan(&[battery_only]);
    assert_eq!(state.current_dpi, Some(800));
    assert_eq!(state.battery_percent, Some(55));
    state.apply_battery_scan(&[interface(2, 3, "Other", None, Some(40))]);
    assert_eq!(state.status, DeviceStatus::Single);
    assert_eq!(state.current_dpi, None);
    assert_eq!(state.battery_percent, Some(40));
}

#[test]
fn custom_presets_share_state_and_unsupported_values_are_disabled() {
    let mut state = AppState::default();
    state.status = DeviceStatus::Single;
    state.current_dpi = Some(850);
    state.supported_dpi = Some(DpiValues::Range {
        minimum: 100,
        maximum: 1600,
        step: 50,
    });
    state.apply_settings(&Settings {
        presets: [450, 850, 1700, 3200],
        theme: Theme::Dark,
        language: None,
    });
    let presets = state.presets();
    assert_eq!(state.theme, Theme::Dark);
    assert!(presets[0].enabled);
    assert!(presets[1].enabled && presets[1].checked);
    assert!(!presets[2].enabled);
    assert!(!presets[3].enabled);
}

#[test]
fn operation_status_never_changes_current_dpi_preview_truth() {
    let mut state = AppState::default();
    state.current_dpi = Some(1300);
    state.operation = OperationStatus::Applying(1350);
    assert_eq!(state.current_dpi, Some(1300));
    assert_eq!(state.operation_text(), "Applying 1350 DPI...");
    state.operation = OperationStatus::Verified(1350);
    assert_eq!(state.operation_text(), "Verified 1350 DPI");
    state.operation = OperationStatus::Failed(OperationError::Failed);
    assert_eq!(
        state.operation_text(),
        "DPI change failed; refresh the device and try again"
    );
}
