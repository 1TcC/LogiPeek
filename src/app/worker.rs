use super::state::{AppState, DeviceStatus, OperationError, OperationStatus};
use crate::hid::device::{self, ScanOptions};
use std::{
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

pub const BATTERY_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    RefreshAll,
    SelectDevice(String),
    SetDpi(u16),
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DpiWriteRoute {
    Fast,
    FullPreflight,
    Blocked,
}

fn dpi_write_route(status: DeviceStatus, has_target: bool) -> DpiWriteRoute {
    match (status, has_target) {
        (DeviceStatus::MultipleDevices, _) => DpiWriteRoute::Blocked,
        (DeviceStatus::Single, true) => DpiWriteRoute::Fast,
        _ => DpiWriteRoute::FullPreflight,
    }
}

pub struct Worker {
    sender: mpsc::SyncSender<Command>,
    join: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn spawn(
        state: Arc<Mutex<AppState>>,
        notify: impl Fn() + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        // One queued action is enough: it serializes HID traffic and prevents
        // repeated menu clicks from building an unbounded write backlog.
        let (sender, receiver) = mpsc::sync_channel(1);
        let join = thread::Builder::new()
            .name("logipeek-hid".into())
            .spawn(move || run(receiver, state, notify))?;
        Ok(Self {
            sender,
            join: Some(join),
        })
    }

    pub fn send(&self, command: Command) -> bool {
        self.sender.try_send(command).is_ok()
    }

    pub fn shutdown(mut self) {
        let _ = self.sender.send(Command::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn run(receiver: mpsc::Receiver<Command>, state: Arc<Mutex<AppState>>, notify: impl Fn()) {
    let mut dpi_target = refresh_all(&state, None);
    notify();
    loop {
        match receiver.recv_timeout(BATTERY_REFRESH_INTERVAL) {
            Ok(Command::RefreshAll) => dpi_target = refresh_all(&state, None),
            Ok(Command::SelectDevice(device)) => {
                dpi_target = refresh_all(&state, Some(&device));
            }
            Ok(Command::SetDpi(value)) => {
                let status = state
                    .lock()
                    .map_or(DeviceStatus::Unavailable, |current| current.status);
                match dpi_write_route(status, dpi_target.is_some()) {
                    DpiWriteRoute::Fast => {}
                    DpiWriteRoute::FullPreflight => dpi_target = refresh_all(&state, None),
                    DpiWriteRoute::Blocked => dpi_target = None,
                }
                let result = dpi_target
                    .as_mut()
                    .map(|target| device::set_validated_runtime_dpi(target, value));
                if let Ok(mut current) = state.lock() {
                    match result {
                        Some(Ok(result)) => {
                            current.apply_dpi_outcome(&result.report.outcome);
                            if !result.target_valid {
                                dpi_target = None;
                            }
                        }
                        Some(Err(device::FastDpiError::Unsupported)) => {
                            current.operation = OperationStatus::Failed(OperationError::Failed)
                        }
                        Some(Err(device::FastDpiError::Invalidated)) | None => {
                            dpi_target = None;
                            current.operation = OperationStatus::Failed(OperationError::Failed)
                        }
                    }
                }
            }
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !refresh_battery(&state, dpi_target.as_ref()) {
                    dpi_target = None;
                }
            }
        }
        notify();
    }
}

fn refresh_all(
    state: &Mutex<AppState>,
    selected_device: Option<&str>,
) -> Option<device::ValidatedDpiTarget> {
    let result = device::scan(ScanOptions {
        read_battery: true,
        read_dpi: true,
    });
    if let Ok(mut current) = state.lock() {
        if let Some(selected_device) = selected_device {
            current.begin_device_switch(selected_device.to_owned());
        }
        match &result {
            Ok(interfaces) => current.replace_from_scan(interfaces),
            Err(_) => {
                let settings = current.settings();
                let mut replacement = AppState::default();
                replacement.apply_settings(&settings);
                *current = replacement;
            }
        }
        if selected_device.is_some() {
            current.operation = OperationStatus::Idle;
        }
    }
    let selected = state
        .lock()
        .ok()
        .and_then(|current| current.selected_device.clone());
    let target = result
        .as_ref()
        .ok()
        .and_then(|interfaces| device::validated_dpi_target(interfaces, selected.as_deref()));
    target.filter(|_| {
        state
            .lock()
            .is_ok_and(|current| current.status == DeviceStatus::Single)
    })
}

fn refresh_battery(state: &Mutex<AppState>, target: Option<&device::ValidatedDpiTarget>) -> bool {
    let Ok(interfaces) = device::scan(ScanOptions {
        read_battery: true,
        read_dpi: false,
    }) else {
        if let Ok(mut current) = state.lock() {
            let settings = current.settings();
            let operation = current.operation.clone();
            let notice = current.settings_notice;
            let mut replacement = AppState::default();
            replacement.apply_settings(&settings);
            replacement.operation = operation;
            replacement.settings_notice = notice;
            *current = replacement;
        }
        return false;
    };
    let target_matches =
        target.is_some_and(|target| device::validated_target_matches_scan(target, &interfaces));
    if let Ok(mut current) = state.lock() {
        current.apply_battery_scan(&interfaces);
        target_matches && current.status == DeviceStatus::Single
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_target_uses_full_preflight_and_valid_target_uses_fast_path() {
        assert_eq!(
            dpi_write_route(DeviceStatus::Single, false),
            DpiWriteRoute::FullPreflight
        );
        assert_eq!(
            dpi_write_route(DeviceStatus::Single, true),
            DpiWriteRoute::Fast
        );
        assert_eq!(
            dpi_write_route(DeviceStatus::Unavailable, false),
            DpiWriteRoute::FullPreflight
        );
    }

    #[test]
    fn multiple_devices_never_use_fast_path() {
        assert_eq!(
            dpi_write_route(DeviceStatus::MultipleDevices, true),
            DpiWriteRoute::Blocked
        );
        assert_eq!(
            dpi_write_route(DeviceStatus::MultipleDevices, false),
            DpiWriteRoute::Blocked
        );
    }
}
