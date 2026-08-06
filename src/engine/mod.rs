mod foreground_window_calendar;

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate, TimeZone};

use crate::calendar::CalendarBlock;
use crate::events::store::EventStore;
use foreground_window_calendar::build_foreground_window_calendar;

pub struct FloggingEngine {
    event_store: EventStore,
}

impl FloggingEngine {
    pub fn new(event_store: EventStore) -> Self {
        Self { event_store }
    }

    pub fn calendar_for(&self, date: NaiveDate) -> Result<Vec<CalendarBlock>> {
        let next_date = date
            .succ_opt()
            .context("calendar date has no representable following day")?;

        let start = Local
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight is valid"))
            .single()
            .with_context(|| {
                format!(
                    "cannot build the calendar for {date}: local midnight is missing or ambiguous"
                )
            })?;

        let end = Local
            .from_local_datetime(&next_date.and_hms_opt(0, 0, 0).expect("midnight is valid"))
            .single()
            .with_context(|| {
                format!(
                    "cannot build the calendar for {date}: local midnight for the following date {next_date} is missing or ambiguous"
                )
            })?;

        let events = self.event_store.events_between(start.into(), end.into())?;
        let foreground_window_calendar = build_foreground_window_calendar(&events);

        Ok(foreground_window_calendar)
    }
}

#[cfg(test)]
mod calendar_for {
    use std::time::{Duration, SystemTime};

    use chrono::{Local, NaiveDate, TimeZone};

    use super::FloggingEngine;
    use crate::calendar::CalendarBlock;
    use crate::events::Event;
    use crate::events::store::EventStore;

    const APPLICATION_A: &str = "application-a.exe";
    const APPLICATION_B: &str = "application-b.exe";
    const CONTEXT_A: &str = "Context A";
    const CONTEXT_B: &str = "Context B";

    #[test]
    fn only_uses_events_from_the_requested_local_date() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let previous_date_start = local_time(2026, 1, 14, 23, 50, 0);
        let requested_date_start = local_time(2026, 1, 15, 12, 0, 0);
        let following_date_start = local_time(2026, 1, 16, 0, 0, 0);
        let mut store = EventStore::open(":memory:").unwrap();

        save_context(&mut store, previous_date_start, APPLICATION_B, CONTEXT_B);
        save_context(&mut store, requested_date_start, APPLICATION_A, CONTEXT_A);
        save_context(&mut store, following_date_start, APPLICATION_B, CONTEXT_B);

        let engine = FloggingEngine::new(store);

        let blocks = engine.calendar_for(date).unwrap();

        assert_eq!(
            blocks,
            vec![CalendarBlock {
                start: requested_date_start,
                finish: requested_date_start + Duration::from_secs(300),
                observation_count: 3,
                executable: APPLICATION_A.to_owned(),
                description: CONTEXT_A.to_owned(),
            }]
        );
    }

    fn local_time(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> SystemTime {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .unwrap()
            .into()
    }

    fn save_context(store: &mut EventStore, start: SystemTime, executable: &str, title: &str) {
        for seconds in [0, 299, 300] {
            store
                .save(&Event::new_foreground_window_event(
                    start + Duration::from_secs(seconds),
                    1,
                    title.to_owned(),
                    executable.to_owned(),
                    None,
                ))
                .unwrap();
        }
    }
}
