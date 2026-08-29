use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, params};

use crate::suggestions::{Suggestion, SuggestionSet};

macro_rules! include_sql {
    ($path:literal) => {
        include_str!(concat!("../../sql/", $path))
    };
}

const SCHEMA: &str = include_sql!("schema.sql");
const SELECT_SUGGESTIONS_BETWEEN: &str = include_sql!("suggestions/select_between.sql");
const UPSERT_SUGGESTION: &str = include_sql!("suggestions/upsert.sql");
const DELETE_SUGGESTIONS_BETWEEN: &str = include_sql!("suggestions/delete_between.sql");
const FIVE_MINUTES_MILLISECONDS: i64 = 5 * 60 * 1_000;
const FIFTEEN_MINUTES_MILLISECONDS: i64 = 15 * 60 * 1_000;

#[derive(Clone)]
pub struct SuggestionStore {
    connection: Arc<Mutex<Connection>>,
}

impl SuggestionStore {
    pub fn build(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        Self::initialize(connection)
    }

    pub fn suggestions_between(&self, start: SystemTime, end: SystemTime) -> Result<SuggestionSet> {
        let start = unix_milliseconds(start)?;
        let end = unix_milliseconds(end)?;
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(SELECT_SUGGESTIONS_BETWEEN)?;

        let stored_suggestions = statement
            .query_map(params![start, end], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut suggestions = SuggestionSet::new(vec![], vec![]);
        for (interval_start, interval_finish, interval_duration, generated_at, jira_issue_key) in
            stored_suggestions
        {
            let suggestion = Suggestion::new(
                system_time(interval_start)?,
                system_time(interval_finish)?,
                system_time(generated_at)?,
                jira_issue_key,
            );

            match interval_duration {
                FIVE_MINUTES_MILLISECONDS => {
                    suggestions.five_minute_suggestions.push(suggestion);
                }
                FIFTEEN_MINUTES_MILLISECONDS => {
                    suggestions.fifteen_minute_suggestions.push(suggestion);
                }
                _ => {
                    return Err(anyhow!(
                        "stored suggestion has an unsupported interval duration"
                    ));
                }
            }
        }

        Ok(suggestions)
    }

    pub fn replace_between(
        &self,
        start: SystemTime,
        end: SystemTime,
        suggestions: &SuggestionSet,
    ) -> Result<()> {
        let start = unix_milliseconds(start)?;
        let end = unix_milliseconds(end)?;

        let mut stored_suggestions = Vec::with_capacity(
            suggestions.five_minute_suggestions.len()
                + suggestions.fifteen_minute_suggestions.len(),
        );
        for (suggestion, interval_duration) in suggestions
            .five_minute_suggestions
            .iter()
            .map(|suggestion| (suggestion, FIVE_MINUTES_MILLISECONDS))
            .chain(
                suggestions
                    .fifteen_minute_suggestions
                    .iter()
                    .map(|suggestion| (suggestion, FIFTEEN_MINUTES_MILLISECONDS)),
            )
        {
            let interval_start = unix_milliseconds(suggestion.interval_start)?;
            let interval_finish = unix_milliseconds(suggestion.interval_finish)?;

            if interval_start < start || interval_start >= end {
                return Err(anyhow!(
                    "suggestion interval starts outside the replacement range"
                ));
            }

            if interval_finish - interval_start != interval_duration {
                return Err(anyhow!(
                    "suggestion boundaries do not match their interval collection"
                ));
            }

            stored_suggestions.push((
                interval_start,
                interval_finish,
                interval_duration,
                unix_milliseconds(suggestion.generated_at)?,
                &suggestion.jira_issue_key,
            ));
        }

        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction()?;

        transaction.execute(DELETE_SUGGESTIONS_BETWEEN, params![start, end])?;

        for (interval_start, interval_finish, interval_duration, generated_at, jira_issue_key) in
            stored_suggestions
        {
            transaction.execute(
                UPSERT_SUGGESTION,
                params![
                    interval_start,
                    interval_finish,
                    interval_duration,
                    generated_at,
                    jira_issue_key,
                ],
            )?;
        }

        transaction.commit()?;

        Ok(())
    }

    fn initialize(connection: Connection) -> Result<Self> {
        connection.execute_batch(SCHEMA)?;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn lock_connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| anyhow!("suggestion store connection lock was poisoned"))
    }
}

fn unix_milliseconds(time: SystemTime) -> Result<i64> {
    let milliseconds = time.duration_since(UNIX_EPOCH)?.as_millis();
    Ok(i64::try_from(milliseconds)?)
}

fn system_time(unix_milliseconds: i64) -> Result<SystemTime> {
    let milliseconds = u64::try_from(unix_milliseconds)?;

    UNIX_EPOCH
        .checked_add(Duration::from_millis(milliseconds))
        .context("stored suggestion time is outside the supported SystemTime range")
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::SuggestionStore;
    use crate::suggestions::{Suggestion, SuggestionSet};

    #[test]
    fn suggestions_between_returns_no_suggestions_from_an_empty_store() {
        let store = SuggestionStore::build(":memory:").unwrap();

        let suggestions = store.suggestions_between(time(1_000), time(2_000)).unwrap();

        assert_eq!(suggestions, SuggestionSet::new(vec![], vec![]));
    }

    #[test]
    fn replace_between_round_trips_both_suggestion_sets() {
        let store = SuggestionStore::build(":memory:").unwrap();
        let five_minute = suggestion(1_000, 1_300, 2_000, Some("MBFS-1234"));
        let fifteen_minute = suggestion(1_000, 1_900, 2_000, Some("MBFS-5678"));
        let expected = SuggestionSet::new(vec![five_minute], vec![fifteen_minute]);

        store
            .replace_between(time(1_000), time(2_000), &expected)
            .unwrap();

        let suggestions = store.suggestions_between(time(1_000), time(2_000)).unwrap();
        assert_eq!(suggestions, expected);
    }

    #[test]
    fn replace_between_round_trips_an_analyzed_interval_without_a_matching_issue() {
        let store = SuggestionStore::build(":memory:").unwrap();
        let expected = SuggestionSet::new(vec![suggestion(1_000, 1_300, 2_000, None)], vec![]);

        store
            .replace_between(time(1_000), time(2_000), &expected)
            .unwrap();

        let suggestions = store.suggestions_between(time(1_000), time(2_000)).unwrap();
        assert_eq!(suggestions, expected);
    }

    #[test]
    fn replace_between_replaces_the_previous_analysis() {
        let store = SuggestionStore::build(":memory:").unwrap();
        let original = SuggestionSet::new(
            vec![
                suggestion(1_000, 1_300, 2_000, Some("MBFS-1234")),
                suggestion(1_300, 1_600, 2_000, Some("MBFS-2222")),
            ],
            vec![],
        );
        let replacement = SuggestionSet::new(
            vec![suggestion(1_000, 1_300, 3_000, Some("MBFS-5678"))],
            vec![],
        );

        store
            .replace_between(time(1_000), time(2_000), &original)
            .unwrap();
        store
            .replace_between(time(1_000), time(2_000), &replacement)
            .unwrap();

        let suggestions = store.suggestions_between(time(1_000), time(2_000)).unwrap();
        assert_eq!(suggestions, replacement);
    }

    #[test]
    fn replace_between_clears_the_previous_analysis_when_given_no_suggestions() {
        let store = SuggestionStore::build(":memory:").unwrap();
        let original = SuggestionSet::new(
            vec![suggestion(1_000, 1_300, 2_000, Some("MBFS-1234"))],
            vec![],
        );

        store
            .replace_between(time(1_000), time(2_000), &original)
            .unwrap();
        store
            .replace_between(
                time(1_000),
                time(2_000),
                &SuggestionSet::new(vec![], vec![]),
            )
            .unwrap();

        let suggestions = store.suggestions_between(time(1_000), time(2_000)).unwrap();
        assert_eq!(suggestions, SuggestionSet::new(vec![], vec![]));
    }

    #[test]
    fn replace_between_does_not_change_suggestions_outside_the_range() {
        let store = SuggestionStore::build(":memory:").unwrap();
        let outside = SuggestionSet::new(
            vec![
                suggestion(500, 800, 2_000, Some("BEFORE-1")),
                suggestion(2_000, 2_300, 2_000, Some("END-1")),
            ],
            vec![],
        );
        let inside = SuggestionSet::new(
            vec![
                suggestion(1_000, 1_300, 2_000, Some("START-5")),
                suggestion(1_500, 1_800, 2_000, Some("INSIDE-1")),
            ],
            vec![suggestion(1_000, 1_900, 2_000, Some("START-15"))],
        );

        store
            .replace_between(time(0), time(3_000), &outside)
            .unwrap();
        store
            .replace_between(time(1_000), time(2_000), &inside)
            .unwrap();

        let suggestions = store.suggestions_between(time(0), time(3_000)).unwrap();

        assert_eq!(
            suggestions,
            SuggestionSet::new(
                vec![
                    suggestion(500, 800, 2_000, Some("BEFORE-1")),
                    suggestion(1_000, 1_300, 2_000, Some("START-5")),
                    suggestion(1_500, 1_800, 2_000, Some("INSIDE-1")),
                    suggestion(2_000, 2_300, 2_000, Some("END-1")),
                ],
                vec![suggestion(1_000, 1_900, 2_000, Some("START-15"))],
            )
        );
    }

    #[test]
    fn replace_between_rejects_suggestions_outside_the_range_without_changing_the_store() {
        let store = SuggestionStore::build(":memory:").unwrap();
        let original = SuggestionSet::new(
            vec![suggestion(1_000, 1_300, 2_000, Some("MBFS-1234"))],
            vec![],
        );
        let outside = SuggestionSet::new(
            vec![suggestion(2_000, 2_300, 3_000, Some("MBFS-5678"))],
            vec![],
        );

        store
            .replace_between(time(1_000), time(2_000), &original)
            .unwrap();

        let result = store.replace_between(time(1_000), time(2_000), &outside);

        assert!(result.is_err());
        let suggestions = store.suggestions_between(time(1_000), time(2_000)).unwrap();
        assert_eq!(suggestions, original);
    }

    #[test]
    fn replace_between_rejects_suggestions_in_the_wrong_interval_set() {
        let store = SuggestionStore::build(":memory:").unwrap();
        let original = SuggestionSet::new(
            vec![suggestion(1_000, 1_300, 2_000, Some("MBFS-1234"))],
            vec![],
        );
        let wrong_set = SuggestionSet::new(
            vec![],
            vec![suggestion(1_300, 1_600, 3_000, Some("MBFS-5678"))],
        );

        store
            .replace_between(time(1_000), time(2_000), &original)
            .unwrap();

        let result = store.replace_between(time(1_000), time(2_000), &wrong_set);

        assert!(result.is_err());
        let suggestions = store.suggestions_between(time(1_000), time(2_000)).unwrap();
        assert_eq!(suggestions, original);
    }

    #[test]
    fn cloned_stores_share_the_same_in_memory_database() {
        let store = SuggestionStore::build(":memory:").unwrap();
        let cloned_store = store.clone();
        let expected = SuggestionSet::new(
            vec![suggestion(1_000, 1_300, 2_000, Some("MBFS-1234"))],
            vec![],
        );

        cloned_store
            .replace_between(time(1_000), time(2_000), &expected)
            .unwrap();

        let suggestions = store.suggestions_between(time(1_000), time(2_000)).unwrap();
        assert_eq!(suggestions, expected);
    }

    fn time(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn suggestion(
        interval_start: u64,
        interval_finish: u64,
        generated_at: u64,
        jira_issue_key: Option<&str>,
    ) -> Suggestion {
        Suggestion::new(
            time(interval_start),
            time(interval_finish),
            time(generated_at),
            jira_issue_key.map(str::to_owned),
        )
    }
}
