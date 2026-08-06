use crate::calendar::CalendarBlock;
use crate::events::{Event, EventPayload};
use std::time::Duration;

const MINIMUM_BLOCK_DURATION: Duration = Duration::from_secs(5 * 60);

pub fn build_foreground_window_calendar(events: &[Event]) -> Vec<CalendarBlock> {
    let mut foreground_window_events: Vec<&Event> = events
        .iter()
        .filter(|event| event.is_foreground_window_event())
        .collect();

    foreground_window_events.sort_by_key(|event| event.observed_at);

    let mut blocks: Vec<CalendarBlock> = Vec::new();

    for event in foreground_window_events {
        let EventPayload::ForegroundWindowObserved {
            title, executable, ..
        } = &event.payload;

        let Some(last_block) = blocks.last_mut() else {
            blocks.push(CalendarBlock::new(
                event.observed_at,
                executable.clone(),
                title.clone(),
            ));

            continue;
        };

        if last_block.executable.as_str() != executable.as_str()
            || last_block.description.as_str() != title.as_str()
        {
            blocks.push(CalendarBlock::new(
                event.observed_at,
                executable.clone(),
                title.clone(),
            ));

            continue;
        }

        let duration_since_last_block = event
            .observed_at
            .duration_since(last_block.finish)
            .expect("events are processed chronologically");

        if duration_since_last_block >= MINIMUM_BLOCK_DURATION {
            blocks.push(CalendarBlock::new(
                event.observed_at,
                executable.clone(),
                title.clone(),
            ));

            continue;
        }

        last_block.observation_count += 1;
        last_block.finish = event.observed_at;
    }

    blocks.retain(|block| {
        let duration = block
            .finish
            .duration_since(block.start)
            .expect("a calendar block cannot finish before it starts");

        duration >= MINIMUM_BLOCK_DURATION
    });

    blocks.dedup_by(|current, previous| {
        let duration_since_previous = current
            .start
            .duration_since(previous.finish)
            .expect("calendar blocks are ordered chronologically");

        if duration_since_previous >= MINIMUM_BLOCK_DURATION
            || current.executable != previous.executable
            || current.description != previous.description
        {
            return false;
        }

        previous.finish = current.finish;
        previous.observation_count += current.observation_count;

        true
    });

    blocks
}

#[cfg(test)]
mod tests {
    use super::build_foreground_window_calendar;
    use crate::calendar::CalendarBlock;
    use crate::events::Event;
    use std::time::{Duration, UNIX_EPOCH};

    const APPLICATION_A: &str = "application-a.exe";
    const APPLICATION_B: &str = "application-b.exe";
    const CONTEXT_A: &str = "Context A";
    const CONTEXT_B: &str = "Context B";

    #[test]
    fn empty_input_produces_no_blocks() {
        let blocks = build_foreground_window_calendar(&[]);

        assert!(blocks.is_empty());
    }

    #[test]
    fn retains_a_context_at_exactly_five_minutes() {
        let events = observations(APPLICATION_A, CONTEXT_A, 0, 300);

        let blocks = build_foreground_window_calendar(&events);

        assert_eq!(blocks, vec![block(APPLICATION_A, CONTEXT_A, 0, 300, 301)]);
    }

    #[test]
    fn discards_a_context_under_five_minutes() {
        let events = observations(APPLICATION_A, CONTEXT_A, 0, 299);

        let blocks = build_foreground_window_calendar(&events);

        assert!(blocks.is_empty());
    }

    #[test]
    fn retains_a_context_over_five_minutes() {
        let events = observations(APPLICATION_A, CONTEXT_A, 0, 301);

        let blocks = build_foreground_window_calendar(&events);

        assert_eq!(blocks, vec![block(APPLICATION_A, CONTEXT_A, 0, 301, 302)]);
    }

    #[test]
    fn orders_observations_before_building_the_calendar() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 300);
        events.reverse();

        let blocks = build_foreground_window_calendar(&events);

        assert_eq!(blocks, vec![block(APPLICATION_A, CONTEXT_A, 0, 300, 301)]);
    }

    #[test]
    fn merges_matching_contexts_across_a_short_context_switch() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 360);
        events.extend(observations(APPLICATION_B, CONTEXT_B, 361, 421));
        events.extend(observations(APPLICATION_A, CONTEXT_A, 422, 782));

        let blocks = build_foreground_window_calendar(&events);

        assert_eq!(blocks, vec![block(APPLICATION_A, CONTEXT_A, 0, 782, 722)]);
    }

    #[test]
    fn retains_a_qualifying_context_between_matching_contexts() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 360);
        events.extend(observations(APPLICATION_B, CONTEXT_B, 361, 661));
        events.extend(observations(APPLICATION_A, CONTEXT_A, 662, 1_022));

        let blocks = build_foreground_window_calendar(&events);

        assert_eq!(
            blocks,
            vec![
                block(APPLICATION_A, CONTEXT_A, 0, 360, 361),
                block(APPLICATION_B, CONTEXT_B, 361, 661, 301),
                block(APPLICATION_A, CONTEXT_A, 662, 1_022, 361),
            ]
        );
    }

    #[test]
    fn merges_matching_contexts_across_a_short_observation_gap() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 360);
        events.extend(observations(APPLICATION_A, CONTEXT_A, 600, 960));

        let blocks = build_foreground_window_calendar(&events);

        assert_eq!(blocks, vec![block(APPLICATION_A, CONTEXT_A, 0, 960, 722)]);
    }

    #[test]
    fn splits_matching_contexts_at_a_five_minute_observation_gap() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 360);
        events.extend(observations(APPLICATION_A, CONTEXT_A, 660, 1_020));

        let blocks = build_foreground_window_calendar(&events);

        assert_eq!(
            blocks,
            vec![
                block(APPLICATION_A, CONTEXT_A, 0, 360, 361),
                block(APPLICATION_A, CONTEXT_A, 660, 1_020, 361),
            ]
        );
    }

    #[test]
    fn splits_matching_contexts_above_a_five_minute_observation_gap() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 360);
        events.extend(observations(APPLICATION_A, CONTEXT_A, 661, 1_021));

        let blocks = build_foreground_window_calendar(&events);

        assert_eq!(
            blocks,
            vec![
                block(APPLICATION_A, CONTEXT_A, 0, 360, 361),
                block(APPLICATION_A, CONTEXT_A, 661, 1_021, 361),
            ]
        );
    }

    fn observations(
        executable: &str,
        title: &str,
        start_second: u64,
        finish_second: u64,
    ) -> Vec<Event> {
        (start_second..=finish_second)
            .map(|second| {
                Event::new_foreground_window_event(
                    UNIX_EPOCH + Duration::from_secs(second),
                    1,
                    title.to_owned(),
                    executable.to_owned(),
                    None,
                )
            })
            .collect()
    }

    fn block(
        executable: &str,
        description: &str,
        start_second: u64,
        finish_second: u64,
        observation_count: usize,
    ) -> CalendarBlock {
        CalendarBlock {
            start: UNIX_EPOCH + Duration::from_secs(start_second),
            finish: UNIX_EPOCH + Duration::from_secs(finish_second),
            observation_count,
            executable: executable.to_owned(),
            description: description.to_owned(),
        }
    }
}
