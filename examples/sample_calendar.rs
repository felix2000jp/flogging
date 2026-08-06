use std::env;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use flogging::engine::FloggingEngine;
use flogging::events::store::EventStore;

fn main() -> Result<()> {
    let database_path = env::args()
        .nth(1)
        .context("usage: cargo run --example sample_calendar -- <database-path> <YYYY-MM-DD>")?;
    let date = env::args()
        .nth(2)
        .context("usage: cargo run --example sample_calendar -- <database-path> <YYYY-MM-DD>")?
        .parse::<NaiveDate>()
        .context("calendar date must use the YYYY-MM-DD format")?;

    let store = EventStore::open(database_path)?;
    let engine = FloggingEngine::new(store);

    let foreground_window_calendar = engine.calendar_for(date)?;

    for block in foreground_window_calendar {
        println!("{:#?}", block);
    }

    Ok(())
}
