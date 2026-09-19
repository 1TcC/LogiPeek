use super::state::{AppState, OperationStatus};
use crate::hid::{
    device::{self, ScanOptions},
    features::dpi::SetDpiOutcome,
};
use std::{
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

pub const BATTERY_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    RefreshAll,
    SetDpi(u16),
    Shutdown,
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
    refresh_all(&state);
    notify();
    loop {
        match receiver.recv_timeout(BATTERY_REFRESH_INTERVAL) {
            Ok(Command::RefreshAll) => refresh_all(&state),
            Ok(Command::SetDpi(value)) => {
                let result = device::set_unique_runtime_dpi(value);
                refresh_all(&state);
                if let Ok(mut current) = state.lock() {
                    current.operation = match result {
                        Ok(report) => match report.outcome {
                            SetDpiOutcome::Verified { current }
                            | SetDpiOutcome::TimedOutConfirmed { current } => {
                                OperationStatus::Verified(current)
                            }
                            SetDpiOutcome::AcknowledgedMismatch { actual }
                            | SetDpiOutcome::TimedOutDifferent { actual } => {
                                OperationStatus::Failed(format!(
                                    "DPI remains {actual}; requested value was not verified"
                                ))
                            }
                            SetDpiOutcome::AcknowledgedUnverified { .. }
                            | SetDpiOutcome::TimedOutUnverified { .. } => {
                                OperationStatus::Failed("DPI change could not be verified".into())
                            }
                        },
                        Err(_) => OperationStatus::Failed(
                            "DPI change failed; refresh the device and try again".into(),
                        ),
                    };
                }
            }
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => refresh_battery(&state),
        }
        notify();
    }
}

fn refresh_all(state: &Mutex<AppState>) {
    let result = device::scan(ScanOptions {
        read_battery: true,
        read_dpi: true,
    });
    if let Ok(mut current) = state.lock() {
        match result {
            Ok(interfaces) => current.replace_from_scan(&interfaces),
            Err(_) => {
                let settings = current.settings();
                let mut replacement = AppState::default();
                replacement.apply_settings(&settings);
                *current = replacement;
            }
        }
    }
}

fn refresh_battery(state: &Mutex<AppState>) {
    let Ok(interfaces) = device::scan(ScanOptions {
        read_battery: true,
        read_dpi: false,
    }) else {
        if let Ok(mut current) = state.lock() {
            let settings = current.settings();
            let operation = current.operation.clone();
            let notice = current.settings_notice.clone();
            let mut replacement = AppState::default();
            replacement.apply_settings(&settings);
            replacement.operation = operation;
            replacement.settings_notice = notice;
            *current = replacement;
        }
        return;
    };
    if let Ok(mut current) = state.lock() {
        current.apply_battery_scan(&interfaces);
    }
}
