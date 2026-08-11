mod calendar;
mod collectors;
mod events;
mod tui;

use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use crate::collectors::windows::WindowsCollector;
use crate::events::store::EventStore;
use crate::tui::{Action, Tui};
use anyhow::{Context, Result};
use chrono::{Local, TimeZone};

const CALENDAR_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

fn main() -> Result<()> {
    let executable_path =
        std::env::current_exe().context("could not locate flogging executable")?;
    let executable_directory = executable_path
        .parent()
        .context("flogging executable does not have a parent directory")?;

    let database_path = executable_directory.join("flogging.db");
    let store = EventStore::build(&database_path)?;

    #[cfg(target_os = "windows")]
    let _collector = WindowsCollector::start(store.clone());

    let mut tui = Tui::start()?;
    let mut calendar = None;
    let mut refresh_at = Instant::now();

    loop {
        if Instant::now() >= refresh_at {
            let date = Local::now().date_naive();
            let next_date = date
                .succ_opt()
                .context("calendar date has no representable following day")?;

            let start = Local
                .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight is valid"))
                .single()
                .with_context(|| {
                    format!(
                        "cannot build the calendar for {date}: local midnight is missing or \
                         ambiguous"
                    )
                })?;

            let end = Local
                .from_local_datetime(&next_date.and_hms_opt(0, 0, 0).expect("midnight is valid"))
                .single()
                .with_context(|| {
                    format!(
                        "cannot build the calendar for {date}: local midnight for the following \
                         date {next_date} is missing or ambiguous"
                    )
                })?;

            let events = store.events_between(start.into(), end.into())?;
            calendar = Some(calendar::build(date, &events));
            refresh_at = Instant::now() + CALENDAR_REFRESH_INTERVAL;
        }

        tui.draw(
            calendar
                .as_ref()
                .expect("calendar is built before the TUI is drawn"),
        )?;

        let wait_duration = refresh_at.saturating_duration_since(Instant::now());

        match tui.wait_for_action(wait_duration)? {
            Some(Action::Quit) => break,
            Some(Action::Refresh) => refresh_at = Instant::now(),
            None => {}
        }
    }

    Ok(())
}
