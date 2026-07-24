use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::domain::{Event, EventPayload};

pub struct EventStore {
    connection: Connection,
}

impl EventStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        Self::initialize(connection)
    }

    pub fn save(&self, event: &Event) -> Result<()> {
        let observed_at = unix_milliseconds(event.observed_at)?;
        let payload = serde_json::to_string(&event.payload)?;

        self.connection.execute(
            "INSERT INTO events (observed_at, payload) VALUES (?1, ?2)",
            params![observed_at, payload],
        )?;

        Ok(())
    }

    pub fn all_events(&self) -> Result<Vec<Event>> {
        let mut statement = self.connection.prepare(
            "SELECT observed_at, payload
             FROM events
             ORDER BY observed_at, id",
        )?;

        let stored_events = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut events = vec![];
        for (observed_at, payload) in stored_events {
            events.push(Event {
                observed_at: system_time(observed_at)?,
                payload: serde_json::from_str::<EventPayload>(&payload)?,
            });
        }

        Ok(events)
    }

    fn initialize(connection: Connection) -> Result<Self> {
        connection.execute(
            "
            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY,
                observed_at INTEGER NOT NULL,
                payload TEXT NOT NULL
            )
            ",
            [],
        )?;

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
    use std::time::{Duration, UNIX_EPOCH};

    use crate::domain::EventPayload;

    use super::*;

    #[test]
    fn saves_and_reads_events() {
        let store = EventStore::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let event = Event {
            observed_at: UNIX_EPOCH + Duration::from_millis(42_123),
            payload: EventPayload::ForegroundWindowObserved {
                window_id: 123,
                title: Some("IntelliJ IDEA".to_owned()),
            },
        };

        store.save(&event).unwrap();

        assert_eq!(store.all_events().unwrap(), vec![event]);
    }

    #[test]
    fn stores_observed_at_as_unix_milliseconds() {
        let store = EventStore::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let event = Event {
            observed_at: UNIX_EPOCH + Duration::from_millis(42_123),
            payload: EventPayload::ForegroundWindowObserved {
                window_id: 123,
                title: None,
            },
        };

        store.save(&event).unwrap();

        let observed_at = store
            .connection
            .query_row("SELECT observed_at FROM events", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();

        assert_eq!(observed_at, 42_123);
    }
}
