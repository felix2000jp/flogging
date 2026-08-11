use std::io;
use std::time::Duration;

use chrono::{DateTime, Local};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::{DefaultTerminal, Frame};

use crate::calendar::Calendar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Refresh,
}

pub struct Tui {
    terminal: DefaultTerminal,
}

impl Tui {
    pub fn start() -> io::Result<Self> {
        Ok(Self {
            terminal: ratatui::try_init()?,
        })
    }

    pub fn draw(&mut self, calendar: &Calendar) -> io::Result<()> {
        self.terminal.draw(|frame| render(frame, calendar))?;
        Ok(())
    }

    pub fn wait_for_action(&self, timeout: Duration) -> io::Result<Option<Action>> {
        if !event::poll(timeout)? {
            return Ok(None);
        }

        Ok(action_for_event(event::read()?))
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

fn render(frame: &mut Frame, calendar: &Calendar) {
    let [header_area, calendar_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let header = Paragraph::new(calendar.date.format("%A, %d %B %Y").to_string())
        .alignment(Alignment::Center)
        .block(Block::new().borders(Borders::ALL).title(" flogging "));
    frame.render_widget(header, header_area);

    if calendar.blocks.is_empty() {
        let empty_calendar = Paragraph::new("No calendar blocks yet.")
            .alignment(Alignment::Center)
            .block(Block::new().borders(Borders::ALL).title(" Workday "));
        frame.render_widget(empty_calendar, calendar_area);
    } else {
        let rows = calendar.blocks.iter().map(|block| {
            let start: DateTime<Local> = block.start.into();
            let finish: DateTime<Local> = block.finish.into();

            Row::new([
                Cell::from(start.format("%H:%M").to_string()),
                Cell::from(finish.format("%H:%M").to_string()),
                Cell::from(block.executable.as_str()),
                Cell::from(block.description.as_str()),
            ])
        });

        let header = Row::new(["Start", "Finish", "Application", "Description"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let widths = [
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(24),
            Constraint::Fill(1),
        ];
        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .block(Block::new().borders(Borders::ALL).title(" Workday "));

        frame.render_widget(table, calendar_area);
    }

    let footer = Paragraph::new("q / Esc: quit    r: refresh")
        .alignment(Alignment::Center)
        .style(Style::default().add_modifier(Modifier::DIM));
    frame.render_widget(footer, footer_area);
}

fn action_for_event(event: Event) -> Option<Action> {
    let Event::Key(key) = event else {
        return None;
    };

    if key.kind != KeyEventKind::Press {
        return None;
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        KeyCode::Char('r') => Some(Action::Refresh),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use chrono::NaiveDate;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    use super::{Action, action_for_event, render};
    use crate::calendar::{Calendar, CalendarBlock};

    #[test]
    fn renders_calendar_blocks() {
        let calendar = Calendar {
            date: NaiveDate::from_ymd_opt(2026, 8, 11).unwrap(),
            blocks: vec![CalendarBlock {
                start: UNIX_EPOCH,
                finish: UNIX_EPOCH + Duration::from_secs(300),
                observation_count: 301,
                executable: "code.exe".to_owned(),
                description: "MBM-1111".to_owned(),
            }],
        };
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(frame, &calendar)).unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("code.exe"));
        assert!(rendered.contains("MBM-1111"));
    }

    #[test]
    fn maps_quit_keys_to_quit_action() {
        let q = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        let escape = Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(action_for_event(q), Some(Action::Quit));
        assert_eq!(action_for_event(escape), Some(Action::Quit));
    }

    #[test]
    fn maps_r_to_refresh_action() {
        let refresh = Event::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        assert_eq!(action_for_event(refresh), Some(Action::Refresh));
    }
}
