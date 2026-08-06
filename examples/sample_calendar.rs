use std::env;

use anyhow::{Context, Result};
use flogging::engine::build_foreground_window_calendar;
use flogging::events::store::EventStore;

fn main() -> Result<()> {
    let database_path = env::args()
        .nth(1)
        .context("usage: cargo run --example sample_calendar -- <database-path>")?;

    let store = EventStore::open(database_path)?;
    let events = store.all_events()?;

    let foreground_window_calendar = build_foreground_window_calendar(&events);

    for block in foreground_window_calendar {
        println!("{:#?}", block);
    }

    Ok(())
}
