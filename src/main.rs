mod agents;
mod calendar;
mod collectors;
mod events;
pub mod suggestions;
mod tui;

use std::io;

#[cfg(target_os = "windows")]
use crate::collectors::windows::WindowsCollector;
use crate::{
    agents::SuggestionAgent, events::store::EventStore, suggestions::store::SuggestionStore,
    tui::App,
};
use anyhow::{Context, Result};
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;

fn main() -> Result<()> {
    let executable_path =
        std::env::current_exe().context("could not locate flogging executable")?;
    let executable_directory = executable_path
        .parent()
        .context("flogging executable does not have a parent directory")?;

    let database_path = executable_directory.join("flogging.db");
    let event_store = EventStore::build(&database_path)?;
    let suggestion_store = SuggestionStore::build(&database_path)?;
    
    let suggestion_agent = SuggestionAgent::new();

    #[cfg(target_os = "windows")]
    let _collector = WindowsCollector::start(event_store.clone());

    let mut app = App::new(
        event_store.clone(),
        suggestion_store.clone(),
        suggestion_agent,
    )?;

    ratatui::run(|terminal| {
        let _mouse_capture = MouseCapture::enable().context("could not enable mouse controls")?;
        app.run(terminal)
    })
}

struct MouseCapture;

impl MouseCapture {
    fn enable() -> io::Result<Self> {
        execute!(io::stdout(), EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for MouseCapture {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
}
