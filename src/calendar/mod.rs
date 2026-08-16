mod foreground_window;

use std::time::SystemTime;

use chrono::NaiveDate;

use crate::events::Event;
use foreground_window::build_foreground_window_blocks;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    pub date: NaiveDate,
    pub blocks: Vec<CalendarBlock>,
}

impl Calendar {
    pub fn new(date: NaiveDate, events: &[Event]) -> Self {
        let blocks = build_foreground_window_blocks(events);

        Self { date, blocks }
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use chrono::NaiveDate;

    use super::{Calendar, CalendarBlock};
    use crate::events::Event;

    #[test]
    fn builds_a_calendar_for_the_requested_date() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let events = (0..=300)
            .map(|second| {
                Event::new_foreground_window_event(
                    UNIX_EPOCH + Duration::from_secs(second),
                    1,
                    "Context A".to_owned(),
                    "application-a.exe".to_owned(),
                    None,
                )
            })
            .collect::<Vec<_>>();

        let calendar = Calendar::new(date, &events);

        assert_eq!(calendar.date, date);
        assert_eq!(
            calendar.blocks,
            vec![CalendarBlock {
                start: UNIX_EPOCH,
                finish: UNIX_EPOCH + Duration::from_secs(300),
                observation_count: 301,
                executable: "application-a.exe".to_owned(),
                description: "Context A".to_owned(),
            }]
        );
    }
}
