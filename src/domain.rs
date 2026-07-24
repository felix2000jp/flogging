use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub observed_at: SystemTime,
    pub payload: EventPayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventPayload {
    ForegroundWindowObserved {
        window_id: u64,
        title: Option<String>,
    },
}
