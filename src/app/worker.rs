use super::state::AppState;
use crate::hid::device::{self, ScanOptions};
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
                let _ = device::set_unique_runtime_dpi(value);
                refresh_all(&state);
            }
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => refresh_battery(&state),
        }
        notify();
    }
}

fn refresh_all(state: &Mutex<AppState>) {
    let next = device::scan(ScanOptions {
        read_battery: true,
        read_dpi: true,
    })
    .map(|interfaces| AppState::from_scan(&interfaces))
    .unwrap_or_default();
    if let Ok(mut current) = state.lock() {
        *current = next;
    }
}

fn refresh_battery(state: &Mutex<AppState>) {
    let Ok(interfaces) = device::scan(ScanOptions {
        read_battery: true,
        read_dpi: false,
    }) else {
        if let Ok(mut current) = state.lock() {
            *current = AppState::default();
        }
        return;
    };
    if let Ok(mut current) = state.lock() {
        current.apply_battery_scan(&interfaces);
    }
}
