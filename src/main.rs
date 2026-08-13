mod calendar;
mod collectors;
mod events;
mod tui;

#[cfg(target_os = "windows")]
use crate::collectors::windows::WindowsCollector;
use crate::{events::store::EventStore, tui::App};
use anyhow::{Context, Result};

fn main() -> Result<()> {
    let executable_path =
        std::env::current_exe().context("could not locate flogging executable")?;
    let executable_directory = executable_path
        .parent()
        .context("flogging executable does not have a parent directory")?;

    let database_path = executable_directory.join("flogging.db");
    let store = EventStore::build(&database_path)?;
    let mut app = App::new(store.clone())?;

    #[cfg(target_os = "windows")]
    let _collector = WindowsCollector::start(store.clone());

    ratatui::run(|terminal| app.run(terminal))
}
