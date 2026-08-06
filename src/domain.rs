use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventPayload {
    ForegroundWindowObserved {
        window_id: u64,
        title: String,
        executable: String,
        executable_path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub observed_at: SystemTime,
    pub payload: EventPayload,
}

impl Event {
    pub fn new_foreground_window_event(
        observed_at: SystemTime,
        window_id: u64,
        title: String,
        executable: String,
        executable_path: Option<PathBuf>,
    ) -> Self {
        Self {
            observed_at,
            payload: EventPayload::ForegroundWindowObserved {
                window_id,
                title,
                executable,
                executable_path,
            },
        }
    }

    pub fn is_foreground_window_event(&self) -> bool {
        match &self.payload {
            EventPayload::ForegroundWindowObserved { .. } => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarBlock {
    pub start: SystemTime,
    pub finish: SystemTime,
    pub observation_count: usize,
    pub executable: String,
    pub description: String,
}

impl CalendarBlock {
    pub fn new(observed_at: SystemTime, executable: String, description: String) -> Self {
        Self {
            start: observed_at,
            finish: observed_at,
            observation_count: 1,
            executable,
            description,
        }
    }
}
