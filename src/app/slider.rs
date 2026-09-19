use crate::hid::features::dpi::{self, DpiValues};

pub fn value_at(values: &DpiValues, position: i32, width: i32) -> Option<u16> {
    if width <= 0 {
        return first(values);
    }
    let position = position.clamp(0, width) as u64;
    let width = width as u64;
    match values {
        DpiValues::List(values) => {
            let last = values.len().checked_sub(1)? as u64;
            let index = ((position * last * 2 + width) / (width * 2)) as usize;
            values.get(index).copied()
        }
        DpiValues::Range {
            minimum, maximum, ..
        } => {
            let span = u64::from(maximum.saturating_sub(*minimum));
            let requested = u64::from(*minimum) + (position * span * 2 + width) / (width * 2);
            dpi::nearest_supported_dpi(values, requested.min(u64::from(u16::MAX)) as u16)
        }
    }
}

pub fn position_of(values: &DpiValues, value: u16, width: i32) -> Option<i32> {
    if width < 0 {
        return None;
    }
    match values {
        DpiValues::List(values) => {
            let index = values.iter().position(|candidate| *candidate == value)?;
            let last = values.len().checked_sub(1)?;
            if last == 0 {
                Some(0)
            } else {
                Some(((index as i64 * i64::from(width) + last as i64 / 2) / last as i64) as i32)
            }
        }
        DpiValues::Range {
            minimum,
            maximum,
            step,
        } if *step > 0
            && minimum < maximum
            && value >= *minimum
            && value <= *maximum
            && (value - minimum).is_multiple_of(*step) =>
        {
            let numerator = i64::from(value - minimum) * i64::from(width);
            Some(
                ((numerator + i64::from(maximum - minimum) / 2) / i64::from(maximum - minimum))
                    as i32,
            )
        }
        _ => None,
    }
}

pub fn endpoints(values: &DpiValues) -> Option<(u16, u16)> {
    match values {
        DpiValues::List(values) => Some((*values.first()?, *values.last()?)),
        DpiValues::Range {
            minimum,
            maximum,
            step,
        } if *step > 0 && minimum <= maximum => {
            let last = minimum + ((maximum - minimum) / step) * step;
            Some((*minimum, last))
        }
        _ => None,
    }
}

fn first(values: &DpiValues) -> Option<u16> {
    endpoints(values).map(|(minimum, _)| minimum)
}
