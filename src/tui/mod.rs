use std::io;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDate, TimeZone};
use ratatui::DefaultTerminal;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Widget};

use crate::calendar::Calendar;
use crate::events::store::EventStore;

const CALENDAR_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Quit,
    Refresh,
    PreviousDay,
    NextDay,
    Today,
}

pub struct App {
    store: EventStore,
    selected_date: NaiveDate,
    calendar: Calendar,
    refresh_at: Instant,
    should_quit: bool,
}

impl App {
    pub fn new(store: EventStore) -> Result<Self> {
        let selected_date = Local::now().date_naive();

        let mut app = Self {
            store,
            selected_date,
            calendar: Calendar {
                date: selected_date,
                blocks: vec![],
            },
            refresh_at: Instant::now(),
            should_quit: false,
        };

        app.refresh_calendar()?;

        Ok(app)
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| frame.render_widget(&*self, frame.area()))?;

            let today = Local::now().date_naive();
            let wait_duration = if self.selected_date == today {
                self.refresh_at.saturating_duration_since(Instant::now())
            } else {
                CALENDAR_REFRESH_INTERVAL
            };

            if let Some(action) = wait_for_action(wait_duration)? {
                self.handle_action(action)?;
            }

            if self.selected_date == Local::now().date_naive() && Instant::now() >= self.refresh_at
            {
                self.refresh_calendar()?;
            }
        }

        Ok(())
    }

    fn handle_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Refresh => self.refresh_calendar()?,
            Action::PreviousDay => {
                self.selected_date = self
                    .selected_date
                    .pred_opt()
                    .context("calendar date has no representable previous day")?;
                self.refresh_calendar()?;
            }
            Action::NextDay => {
                self.selected_date = self
                    .selected_date
                    .succ_opt()
                    .context("calendar date has no representable following day")?;
                self.refresh_calendar()?;
            }
            Action::Today => {
                self.selected_date = Local::now().date_naive();
                self.refresh_calendar()?;
            }
        }

        Ok(())
    }

    fn refresh_calendar(&mut self) -> Result<()> {
        let next_date = self
            .selected_date
            .succ_opt()
            .context("calendar date has no representable following day")?;

        let start = Local
            .from_local_datetime(
                &self
                    .selected_date
                    .and_hms_opt(0, 0, 0)
                    .expect("midnight is valid"),
            )
            .single()
            .with_context(|| {
                format!(
                    "cannot build the calendar for {}: local midnight is missing or ambiguous",
                    self.selected_date
                )
            })?;

        let end = Local
            .from_local_datetime(&next_date.and_hms_opt(0, 0, 0).expect("midnight is valid"))
            .single()
            .with_context(|| {
                format!(
                    "cannot build the calendar for {}: local midnight for the following date {next_date} is missing or ambiguous",
                    self.selected_date
                )
            })?;

        let events = self.store.events_between(start.into(), end.into())?;
        self.calendar = Calendar::new(self.selected_date, &events);
        self.refresh_at = Instant::now() + CALENDAR_REFRESH_INTERVAL;

        Ok(())
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let [header_area, calendar_area, footer_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(area);

        let header = Paragraph::new(self.calendar.date.format("%A, %d %B %Y").to_string())
            .alignment(Alignment::Center)
            .block(Block::new().borders(Borders::ALL).title(" flogging "));
        header.render(header_area, buffer);

        if self.calendar.blocks.is_empty() {
            let empty_calendar = Paragraph::new("No calendar blocks yet.")
                .alignment(Alignment::Center)
                .block(Block::new().borders(Borders::ALL).title(" Workday "));
            empty_calendar.render(calendar_area, buffer);
        } else {
            let rows = self.calendar.blocks.iter().map(|block| {
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

            table.render(calendar_area, buffer);
        }

        let footer = Paragraph::new("←/→: change day    Space: today    r: refresh    q/Esc: quit")
            .alignment(Alignment::Center)
            .style(Style::default().add_modifier(Modifier::DIM));
        footer.render(footer_area, buffer);
    }
}

fn wait_for_action(timeout: Duration) -> io::Result<Option<Action>> {
    if !event::poll(timeout)? {
        return Ok(None);
    }

    let event = event::read()?;
    let action = action_for_event(event);

    Ok(action)
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
        KeyCode::Left => Some(Action::PreviousDay),
        KeyCode::Right => Some(Action::NextDay),
        KeyCode::Char(' ') => Some(Action::Today),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use chrono::{Local, NaiveDate, TimeZone};
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use ratatui::layout::Rect;
    use ratatui::style::{Modifier, Style};
    use ratatui::widgets::Widget;

    use super::{Action, App, action_for_event};
    use crate::calendar::{Calendar, CalendarBlock};
    use crate::events::store::EventStore;

    #[test]
    fn renders_calendar_blocks() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let app = app(
            date,
            vec![
                CalendarBlock {
                    start: Local
                        .with_ymd_and_hms(2026, 8, 11, 9, 0, 0)
                        .single()
                        .unwrap()
                        .into(),
                    finish: Local
                        .with_ymd_and_hms(2026, 8, 11, 9, 5, 0)
                        .single()
                        .unwrap()
                        .into(),
                    observation_count: 301,
                    executable: "code.exe".to_owned(),
                    description: "MBM-1111".to_owned(),
                },
                CalendarBlock {
                    start: Local
                        .with_ymd_and_hms(2026, 8, 11, 9, 6, 0)
                        .single()
                        .unwrap()
                        .into(),
                    finish: Local
                        .with_ymd_and_hms(2026, 8, 11, 9, 11, 0)
                        .single()
                        .unwrap()
                        .into(),
                    observation_count: 301,
                    executable: "edge.exe".to_owned(),
                    description: "Documentation".to_owned(),
                },
            ],
        );

        let area = Rect::new(0, 0, 80, 10);
        let mut actual = Buffer::empty(area);
        app.render(area, &mut actual);

        let mut expected = Buffer::with_lines([
            "┌ flogging ────────────────────────────────────────────────────────────────────┐",
            "│                            Tuesday, 11 August 2026                           │",
            "└──────────────────────────────────────────────────────────────────────────────┘",
            "┌ Workday ─────────────────────────────────────────────────────────────────────┐",
            "│Start   Finish  Application              Description                          │",
            "│09:00   09:05   code.exe                 MBM-1111                             │",
            "│09:06   09:11   edge.exe                 Documentation                        │",
            "│                                                                              │",
            "└──────────────────────────────────────────────────────────────────────────────┘",
            "          ←/→: change day    Space: today    r: refresh    q/Esc: quit          ",
        ]);
        expected.set_style(
            Rect::new(1, 4, 78, 1),
            Style::new().add_modifier(Modifier::BOLD),
        );
        expected.set_style(
            Rect::new(0, 9, 80, 1),
            Style::new().add_modifier(Modifier::DIM),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn renders_an_empty_calendar() {
        let app = app(NaiveDate::from_ymd_opt(2026, 8, 11).unwrap(), vec![]);

        let area = Rect::new(0, 0, 80, 10);
        let mut actual = Buffer::empty(area);
        app.render(area, &mut actual);

        let mut expected = Buffer::with_lines([
            "┌ flogging ────────────────────────────────────────────────────────────────────┐",
            "│                            Tuesday, 11 August 2026                           │",
            "└──────────────────────────────────────────────────────────────────────────────┘",
            "┌ Workday ─────────────────────────────────────────────────────────────────────┐",
            "│                            No calendar blocks yet.                           │",
            "│                                                                              │",
            "│                                                                              │",
            "│                                                                              │",
            "└──────────────────────────────────────────────────────────────────────────────┘",
            "          ←/→: change day    Space: today    r: refresh    q/Esc: quit          ",
        ]);
        expected.set_style(
            Rect::new(0, 9, 80, 1),
            Style::new().add_modifier(Modifier::DIM),
        );

        assert_eq!(actual, expected);
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

    #[test]
    fn maps_arrow_keys_to_day_navigation_actions() {
        let previous = Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        let next = Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

        assert_eq!(action_for_event(previous), Some(Action::PreviousDay));
        assert_eq!(action_for_event(next), Some(Action::NextDay));
    }

    #[test]
    fn maps_space_to_today_action() {
        let today = Event::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

        assert_eq!(action_for_event(today), Some(Action::Today));
    }

    #[test]
    fn ignores_key_repeats_and_releases() {
        let repeated = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Right,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        ));
        let released = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Right,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));

        assert_eq!(action_for_event(repeated), None);
        assert_eq!(action_for_event(released), None);
    }

    #[test]
    fn previous_day_action_loads_the_previous_date() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let expected_date = date.pred_opt().unwrap();
        let mut app = app(date, vec![]);

        app.handle_action(Action::PreviousDay).unwrap();

        assert_eq!(app.selected_date, expected_date);
        assert_eq!(app.calendar.date, expected_date);
    }

    #[test]
    fn next_day_action_loads_the_next_date() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let expected_date = date.succ_opt().unwrap();
        let mut app = app(date, vec![]);

        app.handle_action(Action::NextDay).unwrap();

        assert_eq!(app.selected_date, expected_date);
        assert_eq!(app.calendar.date, expected_date);
    }

    #[test]
    fn today_action_loads_the_current_date() {
        let before = Local::now().date_naive();
        let mut app = app(before.pred_opt().unwrap(), vec![]);

        app.handle_action(Action::Today).unwrap();

        let after = Local::now().date_naive();
        assert!(app.selected_date == before || app.selected_date == after);
        assert_eq!(app.calendar.date, app.selected_date);
    }

    #[test]
    fn refresh_action_keeps_the_selected_date() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let mut app = app(date, vec![]);

        app.handle_action(Action::Refresh).unwrap();

        assert_eq!(app.selected_date, date);
        assert_eq!(app.calendar.date, date);
    }

    #[test]
    fn quit_action_marks_the_app_for_exit() {
        let mut app = app(NaiveDate::from_ymd_opt(2026, 8, 11).unwrap(), vec![]);

        app.handle_action(Action::Quit).unwrap();

        assert!(app.should_quit);
    }

    fn app(date: NaiveDate, blocks: Vec<CalendarBlock>) -> App {
        App {
            store: EventStore::build(":memory:").unwrap(),
            selected_date: date,
            calendar: Calendar { date, blocks },
            refresh_at: Instant::now(),
            should_quit: false,
        }
    }
}
