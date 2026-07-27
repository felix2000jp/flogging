use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::SystemTime;

use anyhow::{Result, anyhow};
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};
use windows::core::PWSTR;

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

fn poll_and_store_foreground_window_events(mut store: EventStore, stop_receiver: Receiver<()>) {
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
    let process_id = process_id(window);
    let executable_path = process_id.and_then(process_executable_path);
    let executable = executable_path
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned());

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

    let executable = executable?;
    let title = title?;

    Some(Event {
        observed_at,
        payload: EventPayload::ForegroundWindowObserved {
            window_id,
            executable,
            executable_path,
            title,
        },
    })
}

fn process_id(window: HWND) -> Option<u32> {
    let mut process_id = 0;

    // SAFETY: `window` came from GetForegroundWindow and `process_id` points to
    // valid writable memory for the duration of this call. If the window has
    // disappeared, Windows returns zero and the observation remains partial.
    let thread_id = unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };

    (thread_id != 0 && process_id != 0).then_some(process_id)
}

fn process_executable_path(process_id: u32) -> Option<PathBuf> {
    const MAX_WINDOWS_PATH_LENGTH: usize = 32_768;

    let mut buffer = vec![0_u16; MAX_WINDOWS_PATH_LENGTH];
    let mut length = buffer.len() as u32;

    // SAFETY: `process_id` was supplied by Windows for the foreground window.
    // We request read-only, limited query access and handle access failures by
    // returning no executable path.
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;

    // SAFETY: `process` is a valid handle returned by OpenProcess. `buffer` is
    // writable for `length` UTF-16 elements, and `length` remains valid for the
    // duration of the call.
    let query_result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR::from_raw(buffer.as_mut_ptr()),
            &mut length,
        )
    };

    // SAFETY: `process` is owned by this function and has not been closed yet.
    // Closing it here releases the operating-system resource on every result.
    let _ = unsafe { CloseHandle(process) };

    query_result.ok()?;

    Some(PathBuf::from(OsString::from_wide(
        &buffer[..length as usize],
    )))
}
