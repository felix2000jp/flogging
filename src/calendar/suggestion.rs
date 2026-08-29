use crate::calendar::CalendarInterval;
use crate::suggestions::Suggestion;

pub(super) fn enrich_intervals(
    mut intervals: Vec<CalendarInterval>,
    suggestions: &[Suggestion],
) -> Vec<CalendarInterval> {
    for interval in &mut intervals {
        interval.suggestion = suggestions
            .iter()
            .find(|suggestion| {
                suggestion.interval_start == interval.start
                    && suggestion.interval_finish == interval.finish
            })
            .cloned();
    }

    intervals
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::enrich_intervals;
    use crate::calendar::CalendarInterval;
    use crate::suggestions::Suggestion;

    #[test]
    fn empty_intervals_produce_no_enriched_intervals() {
        let suggestion = suggestion(0, 300, Some("MBFS-1234"));

        let intervals = enrich_intervals(vec![], &[suggestion]);

        assert!(intervals.is_empty());
    }

    #[test]
    fn matching_suggestion_enriches_the_interval() {
        let suggestion = suggestion(0, 300, Some("MBFS-1234"));

        let intervals = enrich_intervals(vec![interval(0, 300)], std::slice::from_ref(&suggestion));

        assert_eq!(intervals[0].suggestion, Some(suggestion));
    }

    #[test]
    fn analyzed_interval_without_a_match_is_still_enriched() {
        let suggestion = suggestion(0, 300, None);

        let intervals = enrich_intervals(vec![interval(0, 300)], std::slice::from_ref(&suggestion));

        assert_eq!(intervals[0].suggestion, Some(suggestion));
    }

    #[test]
    fn suggestion_must_match_both_interval_boundaries() {
        let different_finish = suggestion(0, 900, Some("MBFS-1234"));
        let different_start = suggestion(300, 600, Some("MBFS-5678"));

        let intervals =
            enrich_intervals(vec![interval(0, 300)], &[different_finish, different_start]);

        assert!(intervals[0].suggestion.is_none());
    }

    #[test]
    fn intervals_without_a_matching_suggestion_remain_unenriched() {
        let suggestion = suggestion(0, 300, Some("MBFS-1234"));

        let intervals = enrich_intervals(
            vec![interval(0, 300), interval(300, 600)],
            std::slice::from_ref(&suggestion),
        );

        assert_eq!(intervals[0].suggestion, Some(suggestion));
        assert!(intervals[1].suggestion.is_none());
    }

    fn interval(start: u64, finish: u64) -> CalendarInterval {
        CalendarInterval::new(
            UNIX_EPOCH + Duration::from_secs(start),
            UNIX_EPOCH + Duration::from_secs(finish),
            vec![],
        )
    }

    fn suggestion(start: u64, finish: u64, jira_issue_key: Option<&str>) -> Suggestion {
        Suggestion::new(
            UNIX_EPOCH + Duration::from_secs(start),
            UNIX_EPOCH + Duration::from_secs(finish),
            UNIX_EPOCH + Duration::from_secs(1_000),
            jira_issue_key.map(str::to_owned),
        )
    }
}
