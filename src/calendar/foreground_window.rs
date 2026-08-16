use std::time::Duration;

use crate::calendar::CalendarBlock;
use crate::events::{Event, EventPayload};

const MAXIMUM_CONTINUOUS_OBSERVATION_GAP: Duration = Duration::from_secs(5);

pub(super) fn build_foreground_window_blocks(events: &[Event]) -> Vec<CalendarBlock> {
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

        let duration_since_last_observation = event
            .observed_at
            .duration_since(last_block.finish)
            .expect("events are processed chronologically");

        if last_block.executable.as_str() == executable.as_str()
            && last_block.description.as_str() == title.as_str()
            && duration_since_last_observation <= MAXIMUM_CONTINUOUS_OBSERVATION_GAP
        {
            last_block.observation_count += 1;
            last_block.finish = event.observed_at;
            continue;
        }

        if duration_since_last_observation <= MAXIMUM_CONTINUOUS_OBSERVATION_GAP {
            last_block.finish = event.observed_at;
        }

        blocks.push(CalendarBlock::new(
            event.observed_at,
            executable.clone(),
            title.clone(),
        ));
    }

    blocks
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::build_foreground_window_blocks;
    use crate::calendar::CalendarBlock;
    use crate::events::Event;

    const APPLICATION_A: &str = "application-a.exe";
    const APPLICATION_B: &str = "application-b.exe";
    const CONTEXT_A: &str = "Context A";
    const CONTEXT_B: &str = "Context B";

    #[test]
    fn empty_input_produces_no_blocks() {
        let blocks = build_foreground_window_blocks(&[]);

        assert!(blocks.is_empty());
    }

    #[test]
    fn builds_a_block_from_a_single_observation() {
        let events = observations(APPLICATION_A, CONTEXT_A, 0, 0);

        let blocks = build_foreground_window_blocks(&events);

        assert_eq!(blocks, vec![block(APPLICATION_A, CONTEXT_A, 0, 0, 1)]);
    }

    #[test]
    fn combines_consecutive_matching_observations() {
        let events = observations(APPLICATION_A, CONTEXT_A, 0, 2);

        let blocks = build_foreground_window_blocks(&events);

        assert_eq!(blocks, vec![block(APPLICATION_A, CONTEXT_A, 0, 2, 3)]);
    }

    #[test]
    fn extends_an_occurrence_until_the_next_observation() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 0);
        events.extend(observations(APPLICATION_B, CONTEXT_B, 1, 2));

        let blocks = build_foreground_window_blocks(&events);

        assert_eq!(
            blocks,
            vec![
                block(APPLICATION_A, CONTEXT_A, 0, 1, 1),
                block(APPLICATION_B, CONTEXT_B, 1, 2, 2),
            ]
        );
    }

    #[test]
    fn retains_short_occurrences() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 2);
        events.extend(observations(APPLICATION_B, CONTEXT_B, 3, 4));

        let blocks = build_foreground_window_blocks(&events);

        assert_eq!(
            blocks,
            vec![
                block(APPLICATION_A, CONTEXT_A, 0, 3, 3),
                block(APPLICATION_B, CONTEXT_B, 3, 4, 2),
            ]
        );
    }

    #[test]
    fn orders_observations_before_building_the_calendar() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 2);
        events.reverse();

        let blocks = build_foreground_window_blocks(&events);

        assert_eq!(blocks, vec![block(APPLICATION_A, CONTEXT_A, 0, 2, 3)]);
    }

    #[test]
    fn starts_a_new_occurrence_when_the_title_changes() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 1);
        events.extend(observations(APPLICATION_A, CONTEXT_B, 2, 3));

        let blocks = build_foreground_window_blocks(&events);

        assert_eq!(
            blocks,
            vec![
                block(APPLICATION_A, CONTEXT_A, 0, 2, 2),
                block(APPLICATION_A, CONTEXT_B, 2, 3, 2),
            ]
        );
    }

    #[test]
    fn starts_a_new_occurrence_when_the_executable_changes() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 1);
        events.extend(observations(APPLICATION_B, CONTEXT_A, 2, 3));

        let blocks = build_foreground_window_blocks(&events);

        assert_eq!(
            blocks,
            vec![
                block(APPLICATION_A, CONTEXT_A, 0, 2, 2),
                block(APPLICATION_B, CONTEXT_A, 2, 3, 2),
            ]
        );
    }

    #[test]
    fn keeps_matching_contexts_as_separate_occurrences_across_a_context_switch() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 1);
        events.extend(observations(APPLICATION_B, CONTEXT_B, 2, 3));
        events.extend(observations(APPLICATION_A, CONTEXT_A, 4, 5));

        let blocks = build_foreground_window_blocks(&events);

        assert_eq!(
            blocks,
            vec![
                block(APPLICATION_A, CONTEXT_A, 0, 2, 2),
                block(APPLICATION_B, CONTEXT_B, 2, 4, 2),
                block(APPLICATION_A, CONTEXT_A, 4, 5, 2),
            ]
        );
    }

    #[test]
    fn combines_matching_observations_at_the_maximum_continuous_gap() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 1);
        events.extend(observations(APPLICATION_A, CONTEXT_A, 6, 7));

        let blocks = build_foreground_window_blocks(&events);

        assert_eq!(blocks, vec![block(APPLICATION_A, CONTEXT_A, 0, 7, 4)]);
    }

    #[test]
    fn splits_matching_contexts_after_collection_stops() {
        let mut events = observations(APPLICATION_A, CONTEXT_A, 0, 1);
        events.extend(observations(APPLICATION_A, CONTEXT_A, 7, 8));

        let blocks = build_foreground_window_blocks(&events);

        assert_eq!(
            blocks,
            vec![
                block(APPLICATION_A, CONTEXT_A, 0, 1, 2),
                block(APPLICATION_A, CONTEXT_A, 7, 8, 2),
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
