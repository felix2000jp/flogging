use std::time::{Duration, UNIX_EPOCH};

use crate::calendar::{CalendarBlock, CalendarInterval, CalendarIntervalBlock};

pub(super) fn build_intervals(
    blocks: &[CalendarBlock],
    interval_duration: Duration,
) -> Vec<CalendarInterval> {
    let interval_seconds = interval_duration.as_secs();
    assert!(interval_seconds > 0, "calendar intervals cannot be empty");

    let mut intervals: Vec<CalendarInterval> = Vec::new();

    for block in blocks {
        if block.start >= block.finish {
            continue;
        }

        let seconds_since_epoch = block
            .start
            .duration_since(UNIX_EPOCH)
            .expect("calendar blocks cannot occur before the Unix epoch")
            .as_secs();
        let aligned_seconds = seconds_since_epoch - (seconds_since_epoch % interval_seconds);
        let mut interval_start = UNIX_EPOCH + Duration::from_secs(aligned_seconds);

        while interval_start < block.finish {
            let interval_finish = interval_start + interval_duration;
            let block_start = block.start.max(interval_start);
            let block_finish = block.finish.min(interval_finish);

            if block_start < block_finish {
                if intervals
                    .last()
                    .is_none_or(|interval| interval.start != interval_start)
                {
                    intervals.push(CalendarInterval::new(
                        interval_start,
                        interval_finish,
                        vec![],
                    ));
                }

                intervals
                    .last_mut()
                    .expect("the interval was created above")
                    .blocks
                    .push(CalendarIntervalBlock::new(
                        block_start,
                        block_finish,
                        block.executable.clone(),
                        block.description.clone(),
                    ));
            }

            interval_start = interval_finish;
        }
    }

    intervals
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::build_intervals;
    use crate::calendar::{CalendarBlock, CalendarInterval, CalendarIntervalBlock};

    const FIVE_MINUTES: Duration = Duration::from_secs(5 * 60);
    const APPLICATION_A: &str = "application-a.exe";
    const APPLICATION_B: &str = "application-b.exe";
    const CONTEXT_A: &str = "Context A";
    const CONTEXT_B: &str = "Context B";

    #[test]
    fn empty_blocks_produce_no_intervals() {
        let intervals = build_intervals(&[], FIVE_MINUTES);

        assert!(intervals.is_empty());
    }

    #[test]
    fn places_a_block_inside_a_clock_aligned_interval() {
        let blocks = vec![block(APPLICATION_A, CONTEXT_A, 60, 120)];

        let intervals = build_intervals(&blocks, FIVE_MINUTES);

        assert_eq!(
            intervals,
            vec![interval(
                0,
                300,
                vec![interval_block(APPLICATION_A, CONTEXT_A, 60, 120)]
            )]
        );
    }

    #[test]
    fn splits_a_block_across_interval_boundaries() {
        let blocks = vec![block(APPLICATION_A, CONTEXT_A, 250, 350)];

        let intervals = build_intervals(&blocks, FIVE_MINUTES);

        assert_eq!(
            intervals,
            vec![
                interval(
                    0,
                    300,
                    vec![interval_block(APPLICATION_A, CONTEXT_A, 250, 300)],
                ),
                interval(
                    300,
                    600,
                    vec![interval_block(APPLICATION_A, CONTEXT_A, 300, 350)],
                ),
            ]
        );
    }

    #[test]
    fn assigns_a_block_on_a_boundary_to_the_following_interval() {
        let blocks = vec![block(APPLICATION_A, CONTEXT_A, 300, 360)];

        let intervals = build_intervals(&blocks, FIVE_MINUTES);

        assert_eq!(
            intervals,
            vec![interval(
                300,
                600,
                vec![interval_block(APPLICATION_A, CONTEXT_A, 300, 360)]
            )]
        );
    }

    #[test]
    fn preserves_each_block_in_an_interval() {
        let blocks = vec![
            block(APPLICATION_A, CONTEXT_A, 0, 120),
            block(APPLICATION_B, CONTEXT_B, 120, 180),
            block(APPLICATION_A, CONTEXT_A, 180, 300),
        ];

        let intervals = build_intervals(&blocks, FIVE_MINUTES);

        assert_eq!(
            intervals,
            vec![interval(
                0,
                300,
                vec![
                    interval_block(APPLICATION_A, CONTEXT_A, 0, 120),
                    interval_block(APPLICATION_B, CONTEXT_B, 120, 180),
                    interval_block(APPLICATION_A, CONTEXT_A, 180, 300),
                ]
            )]
        );
    }

    #[test]
    fn does_not_create_empty_intervals_for_collection_gaps() {
        let blocks = vec![
            block(APPLICATION_A, CONTEXT_A, 60, 120),
            block(APPLICATION_B, CONTEXT_B, 660, 720),
        ];

        let intervals = build_intervals(&blocks, FIVE_MINUTES);

        assert_eq!(
            intervals,
            vec![
                interval(
                    0,
                    300,
                    vec![interval_block(APPLICATION_A, CONTEXT_A, 60, 120)],
                ),
                interval(
                    600,
                    900,
                    vec![interval_block(APPLICATION_B, CONTEXT_B, 660, 720)],
                ),
            ]
        );
    }

    #[test]
    fn ignores_zero_duration_blocks() {
        let blocks = vec![block(APPLICATION_A, CONTEXT_A, 60, 60)];

        let intervals = build_intervals(&blocks, FIVE_MINUTES);

        assert!(intervals.is_empty());
    }

    fn block(
        executable: &str,
        description: &str,
        start_second: u64,
        finish_second: u64,
    ) -> CalendarBlock {
        CalendarBlock {
            start: UNIX_EPOCH + Duration::from_secs(start_second),
            finish: UNIX_EPOCH + Duration::from_secs(finish_second),
            observation_count: 1,
            executable: executable.to_owned(),
            description: description.to_owned(),
        }
    }

    fn interval(
        start_second: u64,
        finish_second: u64,
        blocks: Vec<CalendarIntervalBlock>,
    ) -> CalendarInterval {
        CalendarInterval::new(
            UNIX_EPOCH + Duration::from_secs(start_second),
            UNIX_EPOCH + Duration::from_secs(finish_second),
            blocks,
        )
    }

    fn interval_block(
        executable: &str,
        description: &str,
        start_second: u64,
        finish_second: u64,
    ) -> CalendarIntervalBlock {
        CalendarIntervalBlock::new(
            UNIX_EPOCH + Duration::from_secs(start_second),
            UNIX_EPOCH + Duration::from_secs(finish_second),
            executable.to_owned(),
            description.to_owned(),
        )
    }
}
