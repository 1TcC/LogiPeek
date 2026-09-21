use crate::hid::features::battery::Charging;

pub const HYSTERESIS_PERCENT: u8 = 5;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BatteryAlertState {
    device: Option<String>,
    previous: Option<u8>,
    armed: bool,
}

impl BatteryAlertState {
    pub fn evaluate(
        &mut self,
        device: Option<&str>,
        enabled: bool,
        threshold: u8,
        percentage: Option<u8>,
        charging: Option<&Charging>,
    ) -> bool {
        if self.device.as_deref() != device {
            self.device = device.map(str::to_owned);
            self.previous = None;
            self.armed = true;
        }

        let Some(percentage) = percentage else {
            self.previous = None;
            return false;
        };
        if percentage >= threshold.saturating_add(HYSTERESIS_PERCENT) {
            self.armed = true;
        }
        let discharging = matches!(charging, Some(Charging::Discharging));
        if !discharging {
            self.previous = None;
            return false;
        }
        let crossed =
            self.previous.is_none_or(|previous| previous > threshold) && percentage <= threshold;
        let notify = enabled && self.armed && crossed;
        if notify {
            self.armed = false;
        }
        self.previous = Some(percentage);
        notify
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluate(
        state: &mut BatteryAlertState,
        enabled: bool,
        percentage: Option<u8>,
        charging: Charging,
    ) -> bool {
        state.evaluate(Some("device"), enabled, 20, percentage, Some(&charging))
    }

    #[test]
    fn crossing_notifies_once_and_hysteresis_rearms() {
        let mut state = BatteryAlertState::default();
        assert!(!evaluate(&mut state, true, Some(21), Charging::Discharging));
        assert!(evaluate(&mut state, true, Some(20), Charging::Discharging));
        assert!(!evaluate(&mut state, true, Some(19), Charging::Discharging));
        assert!(!evaluate(&mut state, true, Some(24), Charging::Discharging));
        assert!(!evaluate(&mut state, true, Some(25), Charging::Discharging));
        assert!(evaluate(&mut state, true, Some(20), Charging::Discharging));
    }

    #[test]
    fn charging_disabled_and_missing_percentage_never_notify() {
        for charging in [
            Charging::Charging,
            Charging::FinalStage,
            Charging::Complete,
            Charging::Slow,
        ] {
            let mut state = BatteryAlertState::default();
            assert!(!evaluate(&mut state, true, Some(20), charging));
        }
        let mut state = BatteryAlertState::default();
        assert!(!evaluate(
            &mut state,
            false,
            Some(20),
            Charging::Discharging
        ));
        assert!(!evaluate(&mut state, true, None, Charging::Discharging));
    }
}
