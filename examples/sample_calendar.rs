use std::env;

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate, TimeZone};
use flogging::calendar;
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

    let next_date = date
        .succ_opt()
        .context("calendar date has no representable following day")?;

    let start = Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight is valid"))
        .single()
        .with_context(|| {
            format!("cannot build the calendar for {date}: local midnight is missing or ambiguous")
        })?;

    let end = Local
        .from_local_datetime(&next_date.and_hms_opt(0, 0, 0).expect("midnight is valid"))
        .single()
        .with_context(|| {
            format!(
                "cannot build the calendar for {date}: local midnight for the following date \
                 {next_date} is missing or ambiguous"
            )
        })?;

    let store = EventStore::build(database_path)?;
    let events = store.events_between(start.into(), end.into())?;
    let calendar = calendar::build(date, &events);

    for block in calendar.blocks {
        println!("{:#?}", block);
    }

    Ok(())
}
