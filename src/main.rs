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
    let mut selected_date = Local::now().date_naive();
    let mut calendar = None;
    let mut refresh_at = Instant::now();

    loop {
        let today = Local::now().date_naive();

        if calendar.is_none() || selected_date == today && Instant::now() >= refresh_at {
            let next_date = selected_date
                .succ_opt()
                .context("calendar date has no representable following day")?;

            let start = Local
                .from_local_datetime(
                    &selected_date
                        .and_hms_opt(0, 0, 0)
                        .expect("midnight is valid"),
                )
                .single()
                .with_context(|| {
                    format!(
                        "cannot build the calendar for {selected_date}: local midnight is missing \
                         or ambiguous"
                    )
                })?;

            let end = Local
                .from_local_datetime(&next_date.and_hms_opt(0, 0, 0).expect("midnight is valid"))
                .single()
                .with_context(|| {
                    format!(
                        "cannot build the calendar for {selected_date}: local midnight for the \
                         following date {next_date} is missing or ambiguous"
                    )
                })?;

            let events = store.events_between(start.into(), end.into())?;
            calendar = Some(calendar::build(selected_date, &events));
            refresh_at = Instant::now() + CALENDAR_REFRESH_INTERVAL;
        }

        tui.draw(
            calendar
                .as_ref()
                .expect("calendar is built before the TUI is drawn"),
        )?;

        let wait_duration = if selected_date == today {
            refresh_at.saturating_duration_since(Instant::now())
        } else {
            CALENDAR_REFRESH_INTERVAL
        };

        match tui.wait_for_action(wait_duration)? {
            Some(Action::Quit) => break,
            Some(Action::Refresh) => calendar = None,
            Some(Action::PreviousDay) => {
                selected_date = selected_date
                    .pred_opt()
                    .context("calendar date has no representable previous day")?;
                calendar = None;
            }
            Some(Action::NextDay) => {
                selected_date = selected_date
                    .succ_opt()
                    .context("calendar date has no representable following day")?;
                calendar = None;
            }
            Some(Action::Today) => {
                selected_date = Local::now().date_naive();
                calendar = None;
            }
            None => {}
        }
    }

    Ok(())
}
