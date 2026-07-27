use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub observed_at: SystemTime,
    pub payload: EventPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventPayload {
    ForegroundWindowObserved {
        window_id: u64,
        executable: Option<String>,
        executable_path: Option<PathBuf>,
        title: Option<String>,
    },
}
