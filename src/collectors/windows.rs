use std::time::SystemTime;

use crate::domain::{Event, EventPayload};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
};

pub fn collect_foreground_window_event() -> Option<Event> {
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
