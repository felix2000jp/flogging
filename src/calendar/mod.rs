mod foreground_window;
mod interval;

use std::time::{Duration, SystemTime};

use chrono::NaiveDate;

use crate::events::Event;
use crate::suggestions::Suggestion;
use foreground_window::build_foreground_window_blocks;
use interval::build_intervals;

const FIVE_MINUTES: Duration = Duration::from_secs(5 * 60);
const FIFTEEN_MINUTES: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    pub date: NaiveDate,
    pub blocks: Vec<CalendarBlock>,
    pub five_minute_intervals: Vec<CalendarInterval>,
    pub fifteen_minute_intervals: Vec<CalendarInterval>,
}

impl Calendar {
    pub fn new(date: NaiveDate, events: &[Event], suggestions: &[Suggestion]) -> Self {
        let blocks = build_foreground_window_blocks(events);
        let mut five_minute_intervals = build_intervals(&blocks, FIVE_MINUTES);
        let mut fifteen_minute_intervals = build_intervals(&blocks, FIFTEEN_MINUTES);

        for interval in five_minute_intervals
            .iter_mut()
            .chain(&mut fifteen_minute_intervals)
        {
            interval.suggestion = suggestions
                .iter()
                .find(|suggestion| {
                    suggestion.interval_start == interval.start
                        && suggestion.interval_finish == interval.finish
                })
                .cloned();
        }

        Self {
            date,
            blocks,
            five_minute_intervals,
            fifteen_minute_intervals,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarInterval {
    pub start: SystemTime,
    pub finish: SystemTime,
    pub contexts: Vec<CalendarIntervalContext>,
    pub suggestion: Option<Suggestion>,
}

impl CalendarInterval {
    pub fn new(
        start: SystemTime,
        finish: SystemTime,
        contexts: Vec<CalendarIntervalContext>,
    ) -> Self {
        Self {
            start,
            finish,
            contexts,
            suggestion: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarIntervalContext {
    pub duration: Duration,
    pub executable: String,
    pub description: String,
}

impl CalendarIntervalContext {
    pub fn new(duration: Duration, executable: String, description: String) -> Self {
        Self {
            duration,
            executable,
            description,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use chrono::NaiveDate;

    use super::{Calendar, CalendarBlock, CalendarInterval, CalendarIntervalContext};
    use crate::events::Event;
    use crate::suggestions::Suggestion;

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

        let calendar = Calendar::new(date, &events, &[]);

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
        assert_eq!(
            calendar.five_minute_intervals,
            vec![CalendarInterval::new(
                UNIX_EPOCH,
                UNIX_EPOCH + Duration::from_secs(300),
                vec![CalendarIntervalContext::new(
                    Duration::from_secs(300),
                    "application-a.exe".to_owned(),
                    "Context A".to_owned(),
                )],
            )]
        );
        assert_eq!(
            calendar.fifteen_minute_intervals,
            vec![CalendarInterval::new(
                UNIX_EPOCH,
                UNIX_EPOCH + Duration::from_secs(900),
                vec![CalendarIntervalContext::new(
                    Duration::from_secs(300),
                    "application-a.exe".to_owned(),
                    "Context A".to_owned(),
                )],
            )]
        );
    }

    #[test]
    fn enriches_matching_intervals_with_suggestions() {
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
        let five_minute_suggestion = Suggestion::new(
            UNIX_EPOCH,
            UNIX_EPOCH + Duration::from_secs(300),
            UNIX_EPOCH + Duration::from_secs(1_000),
            Some("MBFS-1234".to_owned()),
        );
        let fifteen_minute_suggestion = Suggestion::new(
            UNIX_EPOCH,
            UNIX_EPOCH + Duration::from_secs(900),
            UNIX_EPOCH + Duration::from_secs(1_000),
            None,
        );

        let calendar = Calendar::new(
            date,
            &events,
            &[
                five_minute_suggestion.clone(),
                fifteen_minute_suggestion.clone(),
            ],
        );

        assert_eq!(
            calendar.five_minute_intervals[0].suggestion,
            Some(five_minute_suggestion)
        );
        assert_eq!(
            calendar.fifteen_minute_intervals[0].suggestion,
            Some(fifteen_minute_suggestion)
        );
    }

    #[test]
    fn ignores_suggestions_that_do_not_match_a_calendar_interval() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let suggestion = Suggestion::new(
            UNIX_EPOCH,
            UNIX_EPOCH + Duration::from_secs(300),
            UNIX_EPOCH + Duration::from_secs(1_000),
            Some("MBFS-1234".to_owned()),
        );

        let calendar = Calendar::new(date, &[], &[suggestion]);

        assert!(calendar.five_minute_intervals.is_empty());
        assert!(calendar.fifteen_minute_intervals.is_empty());
    }
}
