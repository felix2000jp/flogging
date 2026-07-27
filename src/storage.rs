use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::domain::{Event, EventPayload};

const FOREGROUND_WINDOW_EVENT_TYPE: &str = "foreground_window_observed";

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
        insert_event(&transaction, event)?;
        transaction.commit()?;

        Ok(())
    }

    pub fn all_events(&self) -> Result<Vec<Event>> {
        let mut statement = self.connection.prepare(
            "
            SELECT
                events.observed_at,
                foreground_window_events.window_id,
                foreground_window_events.executable,
                foreground_window_events.executable_path,
                foreground_window_events.title
            FROM events
            JOIN foreground_window_events
                ON foreground_window_events.event_id = events.id
            WHERE events.event_type = ?1
            ORDER BY events.observed_at, events.id
            ",
        )?;

        let stored_events = statement
            .query_map([FOREGROUND_WINDOW_EVENT_TYPE], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut events = vec![];
        for (observed_at, window_id, executable, executable_path, title) in stored_events {
            events.push(Event {
                observed_at: system_time(observed_at)?,
                payload: EventPayload::ForegroundWindowObserved {
                    window_id: u64::try_from(window_id)?,
                    executable,
                    executable_path: executable_path.map(PathBuf::from),
                    title,
                },
            });
        }

        Ok(events)
    }

    fn initialize(connection: Connection) -> Result<Self> {
        connection.pragma_update(None, "foreign_keys", true)?;
        create_schema(&connection)?;

        Ok(Self { connection })
    }
}

fn create_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY,
            observed_at INTEGER NOT NULL,
            event_type TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS foreground_window_events (
            event_id INTEGER PRIMARY KEY,
            window_id INTEGER NOT NULL,
            executable TEXT,
            executable_path TEXT,
            title TEXT,
            FOREIGN KEY (event_id) REFERENCES events(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS events_observed_at
            ON events(observed_at);

        CREATE INDEX IF NOT EXISTS foreground_window_events_executable
            ON foreground_window_events(executable);
        ",
    )?;

    Ok(())
}

fn insert_event(connection: &Connection, event: &Event) -> Result<()> {
    let observed_at = unix_milliseconds(event.observed_at)?;

    connection.execute(
        "INSERT INTO events (observed_at, event_type) VALUES (?1, ?2)",
        params![observed_at, FOREGROUND_WINDOW_EVENT_TYPE],
    )?;

    let event_id = connection.last_insert_rowid();

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

            connection.execute(
                "
                INSERT INTO foreground_window_events (
                    event_id,
                    window_id,
                    executable,
                    executable_path,
                    title
                )
                VALUES (?1, ?2, ?3, ?4, ?5)
                ",
                params![event_id, window_id, executable, executable_path, title],
            )?;
        }
    }

    Ok(())
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
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    use crate::domain::EventPayload;

    use super::*;

    #[test]
    fn saves_and_reads_events() {
        let mut store = EventStore::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let event = Event {
            observed_at: UNIX_EPOCH + Duration::from_millis(42_123),
            payload: EventPayload::ForegroundWindowObserved {
                window_id: 123,
                executable: Some("idea64.exe".to_owned()),
                executable_path: Some(PathBuf::from(r"C:\Program Files\JetBrains\idea64.exe")),
                title: Some("IntelliJ IDEA".to_owned()),
            },
        };

        store.save(&event).unwrap();

        assert_eq!(store.all_events().unwrap(), vec![event]);
    }

    #[test]
    fn stores_observed_at_as_unix_milliseconds() {
        let mut store = EventStore::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let event = Event {
            observed_at: UNIX_EPOCH + Duration::from_millis(42_123),
            payload: EventPayload::ForegroundWindowObserved {
                window_id: 123,
                executable: None,
                executable_path: None,
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
