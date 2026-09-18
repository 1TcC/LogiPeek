use crate::hid::hidpp::{Error, Exchange, Feature};

pub const FEATURE_IDS: [u16; 3] = [0x1000, 0x1001, 0x1004];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    Percentage(u8),
    Critical,
    Low,
    Good,
    Full,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Charging {
    Discharging,
    Charging,
    FinalStage,
    Complete,
    Slow,
    InvalidBattery,
    ThermalError,
    Error,
    Unknown(u8),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Battery {
    pub level: Level,
    pub charging: Charging,
}

/// 0x1000 requires capability flags to distinguish coarse levels from mileage.
pub fn parse_1000(capabilities: &[u8], status: &[u8]) -> Result<Battery, Error> {
    if capabilities.len() < 2 || status.len() < 3 {
        return Err(Error::Malformed("truncated battery data"));
    }
    if !(2..=100).contains(&capabilities[0]) || status[0] > 100 || status[1] > status[0] {
        return Err(Error::Malformed("invalid battery level"));
    }
    let level = match status[0] {
        0 => Level::Unknown,
        value if capabilities[0] >= 10 && capabilities[1] & 2 != 0 => Level::Percentage(value),
        1..=10 => Level::Critical,
        11..=30 => Level::Low,
        31..=80 => Level::Good,
        _ => Level::Full,
    };
    let charging = match status[2] {
        0 => Charging::Discharging,
        1 => Charging::Charging,
        2 => Charging::FinalStage,
        3 => Charging::Complete,
        4 => Charging::Slow,
        5 => Charging::InvalidBattery,
        6 => Charging::ThermalError,
        7 => Charging::Error,
        value => Charging::Unknown(value),
    };
    Ok(Battery { level, charging })
}

pub fn read(transport: &mut impl Exchange, device: u8, feature: Feature) -> Result<Battery, Error> {
    if feature.id != 0x1000 || feature.version != 0 {
        return Err(Error::Malformed("battery feature version not implemented"));
    }
    let capabilities = transport.exchange(device, feature.index, 1, &[])?;
    let status = transport.exchange(device, feature.index, 0, &[])?;
    parse_1000(&capabilities, &status)
}
