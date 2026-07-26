use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::SystemTime;

use anyhow::{Result, anyhow};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
};

use crate::domain::{Event, EventPayload};
use crate::storage::EventStore;

pub struct WindowsCollector {
    stop_sender: Sender<()>,
    worker: JoinHandle<()>,
}

impl WindowsCollector {
    pub fn start(store: EventStore) -> Self {
        let (stop_sender, stop_receiver) = mpsc::channel();
        let worker =
            thread::spawn(move || poll_and_store_foreground_window_events(store, stop_receiver));

        Self {
            stop_sender,
            worker,
        }
    }

    pub fn shutdown(self) -> Result<()> {
        let _ = self.stop_sender.send(());

        self.worker
            .join()
            .map_err(|_| anyhow!("Windows collector thread panicked"))
    }
}

fn poll_and_store_foreground_window_events(store: EventStore, stop_receiver: Receiver<()>) {
    loop {
        if let Some(event) = collect_foreground_window_event()
            && let Err(error) = store.save(&event)
        {
            eprintln!("Could not save foreground-window event: {error:#}");
        }

        match stop_receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn collect_foreground_window_event() -> Option<Event> {
    let observed_at = SystemTime::now();

    // SAFETY: GetForegroundWindow takes no arguments and returns a borrowed
    // window handle. We only inspect the handle; we do not own or release it.
    let window = unsafe { GetForegroundWindow() };

    if window.is_invalid() {
        return None;
    }

    let window_id = window.0.addr() as u64;

    // SAFETY: `window` came directly from GetForegroundWindow and is used only
    // for the duration of this call. Windows may invalidate it at any time, in
    // which case the API returns zero and we keep the observation without a title.
    let title_length = unsafe { GetWindowTextLengthW(window) };

    let title = if title_length > 0 {
        let mut buffer = vec![0; title_length as usize + 1];

        // SAFETY: `buffer` is a valid writable UTF-16 buffer. The windows crate
        // passes its length to Win32, preventing GetWindowTextW from writing
        // beyond the allocation.
        let copied_length = unsafe { GetWindowTextW(window, &mut buffer) };

        (copied_length > 0).then(|| String::from_utf16_lossy(&buffer[..copied_length as usize]))
    } else {
        None
    };

    Some(Event {
        observed_at,
        payload: EventPayload::ForegroundWindowObserved { window_id, title },
    })
}
