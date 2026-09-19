use crate::hid::hidpp::{self, Error, Exchange, Feature};

pub const FEATURE_IDS: [u16; 3] = [0x1000, 0x1001, 0x1004];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    Critical,
    Low,
    Good,
    Full,
    Unknown(u8),
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
    pub feature_id: u16,
    pub feature_version: u8,
    pub percentage: Option<u8>,
    pub level: Option<Level>,
    pub charging: Charging,
    pub rechargeable: Option<bool>,
    /// Public material identifies this byte but does not define its values.
    pub external_power_raw: Option<u8>,
}

/// 0x1000 requires capability flags to distinguish coarse levels from mileage.
pub fn parse_1000(capabilities: &[u8], status: &[u8]) -> Result<Battery, Error> {
    if capabilities.len() < 2 || status.len() < 3 {
        return Err(Error::Malformed("truncated battery data"));
    }
    if !(2..=100).contains(&capabilities[0]) || status[0] > 100 || status[1] > status[0] {
        return Err(Error::Malformed("invalid battery level"));
    }
    let percentage_supported = capabilities[0] >= 10 && capabilities[1] & 2 != 0;
    let percentage = (percentage_supported && status[0] != 0).then_some(status[0]);
    let level = if percentage_supported || status[0] == 0 {
        None
    } else {
        Some(match status[0] {
            1..=10 => Level::Critical,
            11..=30 => Level::Low,
            31..=80 => Level::Good,
            _ => Level::Full,
        })
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
    Ok(Battery {
        feature_id: 0x1000,
        feature_version: 0,
        percentage,
        level,
        charging,
        rechargeable: None,
        external_power_raw: None,
    })
}

/// Parse the documented 0x1004 capabilities and status responses.
pub fn parse_1004(
    feature_version: u8,
    capabilities: &[u8],
    status: &[u8],
) -> Result<Battery, Error> {
    if capabilities.len() < 2 || status.len() < 4 {
        return Err(Error::Malformed("truncated unified battery data"));
    }

    let percentage_supported = capabilities[1] & 0x02 != 0;
    let percentage = if percentage_supported {
        if status[0] > 100 {
            return Err(Error::Malformed("invalid battery percentage"));
        }
        Some(status[0])
    } else {
        None
    };
    let reported_level = status[1];
    let level_supported = reported_level != 0 && capabilities[0] & reported_level != 0;
    let level = match reported_level {
        0 => None,
        1 if level_supported => Some(Level::Critical),
        2 if level_supported => Some(Level::Low),
        4 if level_supported => Some(Level::Good),
        8 if level_supported => Some(Level::Full),
        value => Some(Level::Unknown(value)),
    };
    let charging = match status[2] {
        0 => Charging::Discharging,
        1 => Charging::Charging,
        2 => Charging::Slow,
        3 => Charging::Complete,
        4 => Charging::Error,
        value => Charging::Unknown(value),
    };

    Ok(Battery {
        feature_id: 0x1004,
        feature_version,
        percentage,
        level,
        charging,
        rechargeable: Some(capabilities[1] & 0x01 != 0),
        external_power_raw: Some(status[3]),
    })
}

pub fn read(transport: &mut impl Exchange, device: u8, feature: Feature) -> Result<Battery, Error> {
    match (feature.id, feature.version) {
        (0x1000, 0) => {
            let capabilities = hidpp::read_only_exchange(transport, device, feature.index, 1, &[])
                .map_err(|error| error.in_read("0x1000 capabilities (fn1)"))?;
            let status = hidpp::read_only_exchange(transport, device, feature.index, 0, &[])
                .map_err(|error| error.in_read("0x1000 status (fn0)"))?;
            parse_1000(&capabilities, &status)
        }
        (0x1004, version) => {
            let capabilities = hidpp::read_only_exchange(transport, device, feature.index, 0, &[])
                .map_err(|error| error.in_read("0x1004 capabilities (fn0)"))?;
            let status = hidpp::read_only_exchange(transport, device, feature.index, 1, &[])
                .map_err(|error| error.in_read("0x1004 status (fn1)"))?;
            parse_1004(version, &capabilities, &status)
        }
        _ => Err(Error::Malformed("battery feature version not implemented")),
    }
}
