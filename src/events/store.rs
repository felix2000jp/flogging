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
const SELECT_ALL_FOREGROUND_WINDOW_EVENTS: &str =
    include_sql!("foreground_window_events/select_all.sql");

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
            .prepare(SELECT_ALL_FOREGROUND_WINDOW_EVENTS)?;

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
    connection.execute_batch(SCHEMA)?;
    Ok(())
}

fn insert_event(connection: &Connection, event: &Event) -> Result<()> {
    let observed_at = unix_milliseconds(event.observed_at)?;

    connection.execute(
        INSERT_EVENT,
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
                INSERT_FOREGROUND_WINDOW_EVENT,
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
