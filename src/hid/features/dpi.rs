use crate::hid::hidpp::{self, Error, Exchange, Feature};

pub const FEATURE_IDS: [u16; 2] = [0x2201, 0x2202];
const MAX_SENSOR_COUNT: u8 = 16;

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
    pub current: u16,
    pub default: Option<u16>,
    pub supported: DpiValues,
}

pub fn parse_sensor_count(data: &[u8]) -> Result<u8, Error> {
    let count = data
        .first()
        .copied()
        .ok_or(Error::Malformed("truncated DPI sensor count"))?;
    if count == 0 || count > MAX_SENSOR_COUNT {
        return Err(Error::Malformed("invalid DPI sensor count"));
    }
    Ok(count)
}

pub fn parse_dpi_values(sensor: u8, data: &[u8]) -> Result<DpiValues, Error> {
    if data.len() < 3 {
        return Err(Error::Malformed("truncated DPI list response"));
    }
    if data[0] != sensor {
        return Err(Error::Malformed("DPI sensor index mismatch"));
    }

    let mut entries = Vec::new();
    let mut offset = 1;
    let mut terminated = false;
    while offset + 1 < data.len() {
        let value = u16::from_be_bytes([data[offset], data[offset + 1]]);
        offset += 2;
        if value == 0 {
            terminated = true;
            break;
        }
        entries.push(value);
    }
    // A long HID++ response has one unpaired padding byte after seven u16 entries.
    // Some public tables require a terminator, while compatible implementations
    // also accept a full seven-entry payload without one.
    if terminated && data[offset..].iter().any(|byte| *byte != 0) {
        return Err(Error::Malformed("nonzero data after DPI terminator"));
    }
    if !terminated && (entries.len() != 7 || data.get(offset).copied().unwrap_or(0) != 0) {
        return Err(Error::Malformed("unterminated DPI list"));
    }
    if entries.is_empty() {
        return Err(Error::Malformed("empty DPI list"));
    }

    let sentinels: Vec<_> = entries
        .iter()
        .enumerate()
        .filter(|(_, value)| **value >= 0xe000)
        .collect();
    if sentinels.is_empty() {
        if entries.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(Error::Malformed("invalid discrete DPI list"));
        }
        return Ok(DpiValues::List(entries));
    }
    if entries.len() != 3 || sentinels.len() != 1 || sentinels[0].0 != 1 {
        return Err(Error::Malformed("unexpected DPI range sentinel"));
    }

    let minimum = entries[0];
    let maximum = entries[2];
    let step = entries[1] & 0x1fff;
    if minimum == 0 || minimum > 0xdfff || maximum > 0xdfff || minimum >= maximum || step == 0 {
        return Err(Error::Malformed("invalid DPI range"));
    }
    Ok(DpiValues::Range {
        minimum,
        maximum,
        step,
    })
}

pub fn parse_sensor_dpi(sensor: u8, supported: DpiValues, data: &[u8]) -> Result<SensorDpi, Error> {
    if data.len() < 5 {
        return Err(Error::Malformed("truncated current DPI response"));
    }
    if data[0] != sensor {
        return Err(Error::Malformed("DPI sensor index mismatch"));
    }
    let current = u16::from_be_bytes([data[1], data[2]]);
    let default_raw = u16::from_be_bytes([data[3], data[4]]);
    if current == 0 || current > 0xdfff {
        return Err(Error::Malformed("invalid current DPI"));
    }
    let default = if default_raw == 0 {
        None
    } else if default_raw > 0xdfff {
        return Err(Error::Malformed("invalid default DPI"));
    } else {
        Some(default_raw)
    };
    Ok(SensorDpi {
        sensor,
        current,
        default,
        supported,
    })
}

pub fn read(
    transport: &mut impl Exchange,
    device: u8,
    feature: Feature,
) -> Result<Vec<SensorDpi>, Error> {
    if feature.id != 0x2201 {
        return Err(Error::Malformed("DPI feature not implemented"));
    }
    let count_response = hidpp::read_only_exchange(transport, device, feature.index, 0, &[])
        .map_err(|error| error.in_read("0x2201 sensor count (fn0)"))?;
    let count = parse_sensor_count(&count_response)?;
    let mut sensors = Vec::with_capacity(count as usize);
    for sensor in 0..count {
        let supported_response =
            hidpp::read_only_exchange(transport, device, feature.index, 1, &[sensor])
                .map_err(|error| error.in_read("0x2201 supported DPI (fn1)"))?;
        let supported = parse_dpi_values(sensor, &supported_response)?;
        let current_response =
            hidpp::read_only_exchange(transport, device, feature.index, 2, &[sensor])
                .map_err(|error| error.in_read("0x2201 current DPI (fn2)"))?;
        sensors.push(parse_sensor_dpi(sensor, supported, &current_response)?);
    }
    Ok(sensors)
}
