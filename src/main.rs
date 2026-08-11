use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Local, TimeZone};
use flogging::calendar;
#[cfg(target_os = "windows")]
use flogging::collectors::windows::WindowsCollector;
use flogging::events::store::EventStore;

const CALENDAR_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

fn main() -> Result<()> {
    let executable_path =
        std::env::current_exe().context("could not locate flogging executable")?;
    let executable_directory = executable_path
        .parent()
        .context("flogging executable does not have a parent directory")?;

    let database_path = executable_directory.join("flogging.db");
    let store = EventStore::build(&database_path)?;

    println!("flogging database: {}", database_path.display());

    #[cfg(target_os = "windows")]
    let collector = WindowsCollector::start(store.clone());

    #[cfg(target_os = "windows")]
    println!("Collecting foreground-window events.");

    #[cfg(not(target_os = "windows"))]
    println!("No event collectors are available on this platform yet.");

    let run_result = (|| -> Result<()> {
        let (exit_sender, exit_receiver) = mpsc::channel();
        let input_worker = thread::spawn(move || {
            let mut input = String::new();
            let result = io::stdin().read_line(&mut input).map(|_| ());
            let _ = exit_sender.send(result);
        });

        println!("Press Enter to stop.");

        loop {
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
            let calendar = calendar::build(date, &events);

            println!();
            println!("Calendar for {}", calendar.date);

            if calendar.blocks.is_empty() {
                println!("No calendar blocks.");
            }

            for block in calendar.blocks {
                let start: DateTime<Local> = block.start.into();
                let finish: DateTime<Local> = block.finish.into();

                println!(
                    "{}-{} | {} | {}",
                    start.format("%H:%M:%S"),
                    finish.format("%H:%M:%S"),
                    block.executable,
                    block.description
                );
            }

            match exit_receiver.recv_timeout(CALENDAR_REFRESH_INTERVAL) {
                Ok(result) => {
                    result?;
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("input thread stopped unexpectedly"));
                }
            }
        }

        input_worker
            .join()
            .map_err(|_| anyhow!("input thread panicked"))?;

        Ok(())
    })();

    #[cfg(target_os = "windows")]
    collector.shutdown()?;

    run_result
}
