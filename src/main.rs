use std::io;
use std::path::Path;

use anyhow::{Context, Result};

#[cfg(target_os = "windows")]
use flogging::collectors::windows::WindowsCollector;
#[cfg(target_os = "windows")]
use flogging::events::store::EventStore;

fn main() -> Result<()> {
    let executable_path =
        std::env::current_exe().context("could not locate flogging executable")?;
    let executable_directory = executable_path
        .parent()
        .context("flogging executable does not have a parent directory")?;
    let database_path = executable_directory.join("flogging.db");

    #[cfg(target_os = "windows")]
    let collector = {
        let store = EventStore::build(&database_path)?;
        WindowsCollector::start(store)
    };

    let run_result = run_application(&database_path);

    #[cfg(target_os = "windows")]
    collector.shutdown()?;

    run_result
}

fn run_application(database_path: &Path) -> Result<()> {
    println!("flogging database: {}", database_path.display());

    #[cfg(target_os = "windows")]
    println!("Collecting foreground-window events.");

    #[cfg(not(target_os = "windows"))]
    println!("No event collectors are available on this platform yet.");

    println!("Press Enter to stop.");

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(())
}
