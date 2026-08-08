use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::events::{Event, EventPayload};

macro_rules! include_sql {
    ($path:literal) => {
        include_str!(concat!("../../sql/", $path))
    };
}

const FOREGROUND_WINDOW_EVENT_TYPE: &str = "foreground_window_observed";

const SCHEMA: &str = include_sql!("schema.sql");
const INSERT_EVENT: &str = include_sql!("events/insert.sql");
const INSERT_FOREGROUND_WINDOW_EVENT: &str = include_sql!("foreground_window_events/insert.sql");
const SELECT_FOREGROUND_WINDOW_EVENTS_BETWEEN: &str =
    include_sql!("foreground_window_events/select_between.sql");

pub struct EventStore {
    connection: Connection,
}

impl EventStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        Self::initialize(connection)
    }

    pub fn save(&mut self, event: &Event) -> Result<()> {
        let transaction = self.connection.transaction()?;

        let observed_at = unix_milliseconds(event.observed_at)?;

        transaction.execute(
            INSERT_EVENT,
            params![observed_at, FOREGROUND_WINDOW_EVENT_TYPE],
        )?;

        let event_id = transaction.last_insert_rowid();

        match &event.payload {
            EventPayload::ForegroundWindowObserved {
                window_id,
                executable,
                executable_path,
                title,
            } => {
                let window_id = i64::try_from(*window_id)?;
                let executable_path = executable_path
                    .as_deref()
                    .map(|path| path.to_string_lossy());

                transaction.execute(
                    INSERT_FOREGROUND_WINDOW_EVENT,
                    params![event_id, window_id, executable, executable_path, title],
                )?;
            }
        }

        transaction.commit()?;

        Ok(())
    }

    pub fn events_between(&self, start: SystemTime, end: SystemTime) -> Result<Vec<Event>> {
        let start = unix_milliseconds(start)?;
        let end = unix_milliseconds(end)?;
        let mut statement = self
            .connection
            .prepare(SELECT_FOREGROUND_WINDOW_EVENTS_BETWEEN)?;

        let stored_events = statement
            .query_map(params![FOREGROUND_WINDOW_EVENT_TYPE, start, end], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut events = vec![];
        for (observed_at, window_id, executable, executable_path, title) in stored_events {
            events.push(Event::new_foreground_window_event(
                system_time(observed_at)?,
                u64::try_from(window_id)?,
                title,
                executable,
                executable_path.map(PathBuf::from),
            ));
        }

        Ok(events)
    }

    fn initialize(connection: Connection) -> Result<Self> {
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.execute_batch(SCHEMA)?;

        Ok(Self { connection })
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
        .context("stored observed_at is outside the supported SystemTime range")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::EventStore;
    use crate::events::Event;

    const EXECUTABLE: &str = "application.exe";

    #[test]
    fn events_between_returns_no_events_from_an_empty_store() {
        let store = EventStore::open(":memory:").unwrap();

        let events = store.events_between(time(1_000), time(2_000)).unwrap();

        assert!(events.is_empty());
    }

    #[test]
    fn save_round_trips_a_foreground_window_event() {
        let mut store = EventStore::open(":memory:").unwrap();
        let event = event(
            1_500,
            42,
            "Project - application",
            Some(Path::new("applications/application.exe")),
        );

        store.save(&event).unwrap();

        let events = store.events_between(time(1_000), time(2_000)).unwrap();
        assert_eq!(events, vec![event]);
    }

    #[test]
    fn save_round_trips_a_missing_executable_path() {
        let mut store = EventStore::open(":memory:").unwrap();
        let event = event(1_500, 42, "Project - application", None);

        store.save(&event).unwrap();

        let events = store.events_between(time(1_000), time(2_000)).unwrap();
        assert_eq!(events, vec![event]);
    }

    #[test]
    fn events_between_uses_a_half_open_time_range() {
        let mut store = EventStore::open(":memory:").unwrap();
        let before_start = event(999, 1, "before start", None);
        let at_start = event(1_000, 2, "at start", None);
        let inside = event(1_500, 3, "inside", None);
        let before_end = event(1_999, 4, "before end", None);
        let at_end = event(2_000, 5, "at end", None);
        let after_end = event(2_001, 6, "after end", None);

        for event in [
            &before_start,
            &at_start,
            &inside,
            &before_end,
            &at_end,
            &after_end,
        ] {
            store.save(event).unwrap();
        }

        let events = store.events_between(time(1_000), time(2_000)).unwrap();

        assert_eq!(events, vec![at_start, inside, before_end]);
    }

    #[test]
    fn events_between_orders_by_time_and_then_insertion_order() {
        let mut store = EventStore::open(":memory:").unwrap();
        let first_at_same_time = event(2_000, 1, "first at same time", None);
        let latest = event(3_000, 2, "latest", None);
        let second_at_same_time = event(2_000, 3, "second at same time", None);
        let earliest = event(1_000, 4, "earliest", None);

        for event in [
            &first_at_same_time,
            &latest,
            &second_at_same_time,
            &earliest,
        ] {
            store.save(event).unwrap();
        }

        let events = store.events_between(time(0), time(4_000)).unwrap();

        assert_eq!(
            events,
            vec![earliest, first_at_same_time, second_at_same_time, latest]
        );
    }

    fn time(milliseconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(milliseconds)
    }

    fn event(
        observed_at_milliseconds: u64,
        window_id: u64,
        title: &str,
        executable_path: Option<&Path>,
    ) -> Event {
        Event::new_foreground_window_event(
            time(observed_at_milliseconds),
            window_id,
            title.to_owned(),
            EXECUTABLE.to_owned(),
            executable_path.map(PathBuf::from),
        )
    }
}
