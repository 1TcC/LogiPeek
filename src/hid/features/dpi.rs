/// Phase one detects these feature IDs only. No DPI write operation exists.
pub const FEATURE_IDS: [u16; 2] = [0x2201, 0x2202];

/// Future reads must populate these values from device-reported sensor data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DpiValues {
    List(Vec<u16>),
    Range {
        minimum: u16,
        maximum: u16,
        step: u16,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensorDpi {
    pub sensor: u8,
    pub current: Option<u16>,
    pub default: Option<u16>,
    pub supported: DpiValues,
}
