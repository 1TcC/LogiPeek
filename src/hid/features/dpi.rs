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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetDpiOutcome {
    Verified { current: u16 },
    AcknowledgedMismatch { actual: u16 },
    AcknowledgedUnverified { error: Error },
    TimedOutConfirmed { current: u16 },
    TimedOutDifferent { actual: u16 },
    TimedOutUnverified { error: Error },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetDpiError {
    InvalidFeature,
    InvalidSensor,
    Unsupported {
        requested: u16,
        nearest: Option<u16>,
    },
    Write(Error),
}

impl std::fmt::Display for SetDpiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFeature => f.write_str("DPI write requires feature 0x2201"),
            Self::InvalidSensor => f.write_str("DPI write sensor index is invalid"),
            Self::Unsupported {
                requested,
                nearest: Some(nearest),
            } => write!(
                f,
                "DPI {requested} is unsupported; nearest supported value is {nearest}"
            ),
            Self::Unsupported {
                requested,
                nearest: None,
            } => write!(
                f,
                "DPI {requested} is unsupported; no valid value was reported"
            ),
            Self::Write(error) => write!(f, "DPI write failed: {error}"),
        }
    }
}

impl std::error::Error for SetDpiError {}

pub fn supports_dpi(values: &DpiValues, dpi: u16) -> bool {
    match values {
        DpiValues::List(values) => values.contains(&dpi),
        DpiValues::Range {
            minimum,
            maximum,
            step,
        } => {
            *step != 0
                && minimum <= maximum
                && dpi >= *minimum
                && dpi <= *maximum
                && (dpi - minimum).is_multiple_of(*step)
        }
    }
}

/// Returns the closest value the device reports as writable. Equal-distance
/// ties select the higher DPI so the result is deterministic.
pub fn nearest_supported_dpi(values: &DpiValues, requested: u16) -> Option<u16> {
    match values {
        DpiValues::List(values) => values
            .iter()
            .min_by_key(|value| {
                (
                    u16::abs_diff(**value, requested),
                    std::cmp::Reverse(**value),
                )
            })
            .copied(),
        DpiValues::Range {
            minimum,
            maximum,
            step,
        } => {
            if *step == 0 || minimum > maximum {
                return None;
            }
            if requested <= *minimum {
                return Some(*minimum);
            }
            let last = minimum + ((maximum - minimum) / step) * step;
            if requested >= last {
                return Some(last);
            }
            let offset = requested - minimum;
            let lower = minimum + (offset / step) * step;
            let upper = lower + step;
            if requested - lower < upper - requested {
                Some(lower)
            } else {
                Some(upper)
            }
        }
    }
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

fn read_current(
    transport: &mut impl Exchange,
    device: u8,
    feature: Feature,
    sensor: u8,
    supported: DpiValues,
) -> Result<SensorDpi, Error> {
    let response = hidpp::read_only_exchange(transport, device, feature.index, 2, &[sensor])
        .map_err(|error| error.in_read("0x2201 DPI write read-back (fn2)"))?;
    parse_sensor_dpi(sensor, supported, &response)
}

/// Sends exactly one setSensorDpi request, then verifies it with an independent
/// read-only fn2 request. A write timeout is ambiguous and is never retried.
pub fn set_and_verify(
    transport: &mut impl Exchange,
    device: u8,
    feature: Feature,
    sensor_count: u8,
    sensor: &SensorDpi,
    requested: u16,
) -> Result<SetDpiOutcome, SetDpiError> {
    if feature.id != 0x2201 {
        return Err(SetDpiError::InvalidFeature);
    }
    if sensor_count == 0 || sensor.sensor >= sensor_count {
        return Err(SetDpiError::InvalidSensor);
    }
    if !supports_dpi(&sensor.supported, requested) {
        return Err(SetDpiError::Unsupported {
            requested,
            nearest: nearest_supported_dpi(&sensor.supported, requested),
        });
    }

    let [high, low] = requested.to_be_bytes();
    let write = transport.exchange(device, feature.index, 3, &[sensor.sensor, high, low]);
    let acknowledged = match write {
        Ok(response) => {
            if feature.version > 0
                && response.get(..3) != Some([sensor.sensor, high, low].as_slice())
            {
                return Err(SetDpiError::Write(Error::Malformed(
                    "DPI setter acknowledgement mismatch",
                )));
            }
            true
        }
        Err(Error::Timeout) => false,
        Err(error) => return Err(SetDpiError::Write(error)),
    };

    match read_current(
        transport,
        device,
        feature,
        sensor.sensor,
        sensor.supported.clone(),
    ) {
        Ok(current) if acknowledged && current.current == requested => {
            Ok(SetDpiOutcome::Verified {
                current: current.current,
            })
        }
        Ok(current) if acknowledged => Ok(SetDpiOutcome::AcknowledgedMismatch {
            actual: current.current,
        }),
        Err(error) if acknowledged => Ok(SetDpiOutcome::AcknowledgedUnverified { error }),
        Ok(current) if current.current == requested => Ok(SetDpiOutcome::TimedOutConfirmed {
            current: current.current,
        }),
        Ok(current) => Ok(SetDpiOutcome::TimedOutDifferent {
            actual: current.current,
        }),
        Err(error) => Ok(SetDpiOutcome::TimedOutUnverified { error }),
    }
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
