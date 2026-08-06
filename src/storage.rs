mod queries;

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
        let mut statement = self
            .connection
            .prepare(queries::foreground_window_events::SELECT_ALL)?;

        let stored_events = statement
            .query_map([FOREGROUND_WINDOW_EVENT_TYPE], |row| {
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
        create_schema(&connection)?;

        Ok(Self { connection })
    }
}

fn create_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(queries::SCHEMA)?;
    Ok(())
}

fn insert_event(connection: &Connection, event: &Event) -> Result<()> {
    let observed_at = unix_milliseconds(event.observed_at)?;

    connection.execute(
        queries::events::INSERT,
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
                queries::foreground_window_events::INSERT,
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

    use super::*;

    #[test]
    fn saves_and_reads_events() {
        let mut store = EventStore::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let event = Event::new_foreground_window_event(
            UNIX_EPOCH + Duration::from_millis(42_123),
            123,
            "IntelliJ IDEA".to_owned(),
            "idea64.exe".to_owned(),
            Some(PathBuf::from(r"C:\Program Files\JetBrains\idea64.exe")),
        );

        store.save(&event).unwrap();

        assert_eq!(store.all_events().unwrap(), vec![event]);
    }

    #[test]
    fn converts_observed_at_to_unix_milliseconds() {
        let observed_at = UNIX_EPOCH + Duration::from_millis(42_123);

        assert_eq!(unix_milliseconds(observed_at).unwrap(), 42_123);
    }
}
