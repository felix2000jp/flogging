mod interval;

use std::io;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDate, TimeZone};
use ratatui::DefaultTerminal;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, ListState, Paragraph, Row, Table, Widget};

use crate::calendar::{Calendar, CalendarInterval};
use crate::events::store::EventStore;

const CALENDAR_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Quit,
    PreviousDay,
    NextDay,
    Today,
    NextView,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalendarView {
    Occurrences,
    FiveMinuteIntervals,
    FifteenMinuteIntervals,
}

pub struct App {
    store: EventStore,
    selected_date: NaiveDate,
    calendar: Calendar,
    calendar_view: CalendarView,
    occurrence_scroll_offset: usize,
    interval_list_state: ListState,
    refresh_at: Instant,
    should_quit: bool,
}

impl App {
    pub fn new(store: EventStore) -> Result<Self> {
        let selected_date = Local::now().date_naive();

        let mut app = Self {
            store,
            selected_date,
            calendar: Calendar::new(selected_date, &[]),
            calendar_view: CalendarView::Occurrences,
            occurrence_scroll_offset: 0,
            interval_list_state: ListState::default(),
            refresh_at: Instant::now(),
            should_quit: false,
        };

        app.refresh_calendar()?;

        Ok(app)
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| frame.render_widget(&mut *self, frame.area()))?;

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
            Action::PreviousDay => {
                self.selected_date = self
                    .selected_date
                    .pred_opt()
                    .context("calendar date has no representable previous day")?;
                self.refresh_calendar()?;
                self.reset_navigation();
            }
            Action::NextDay => {
                self.selected_date = self
                    .selected_date
                    .succ_opt()
                    .context("calendar date has no representable following day")?;
                self.refresh_calendar()?;
                self.reset_navigation();
            }
            Action::Today => {
                self.selected_date = Local::now().date_naive();
                self.refresh_calendar()?;
                self.reset_navigation();
            }
            Action::NextView => {
                self.calendar_view = match self.calendar_view {
                    CalendarView::Occurrences => CalendarView::FiveMinuteIntervals,
                    CalendarView::FiveMinuteIntervals => CalendarView::FifteenMinuteIntervals,
                    CalendarView::FifteenMinuteIntervals => CalendarView::Occurrences,
                };
                self.reset_navigation();
            }
            Action::ScrollUp => self.move_up(),
            Action::ScrollDown => self.move_down(),
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
        self.clamp_navigation();
        self.refresh_at = Instant::now() + CALENDAR_REFRESH_INTERVAL;

        Ok(())
    }

    fn reset_navigation(&mut self) {
        self.occurrence_scroll_offset = 0;
        self.interval_list_state = ListState::default();

        if self
            .current_intervals()
            .is_some_and(|intervals| !intervals.is_empty())
        {
            self.interval_list_state.select(Some(0));
        }
    }

    fn clamp_navigation(&mut self) {
        match self.calendar_view {
            CalendarView::Occurrences => {
                self.occurrence_scroll_offset = self
                    .occurrence_scroll_offset
                    .min(self.calendar.blocks.len().saturating_sub(1));
            }
            CalendarView::FiveMinuteIntervals | CalendarView::FifteenMinuteIntervals => {
                let interval_count = self
                    .current_intervals()
                    .map_or(0, |intervals| intervals.len());

                if interval_count == 0 {
                    self.interval_list_state.select(None);
                } else {
                    let selected = self
                        .interval_list_state
                        .selected()
                        .unwrap_or(0)
                        .min(interval_count - 1);
                    self.interval_list_state.select(Some(selected));
                }
            }
        }
    }

    fn move_up(&mut self) {
        match self.calendar_view {
            CalendarView::Occurrences => {
                self.occurrence_scroll_offset = self.occurrence_scroll_offset.saturating_sub(1);
            }
            CalendarView::FiveMinuteIntervals | CalendarView::FifteenMinuteIntervals => {
                if let Some(selected) = self.interval_list_state.selected() {
                    self.interval_list_state
                        .select(Some(selected.saturating_sub(1)));
                }
            }
        }
    }

    fn move_down(&mut self) {
        match self.calendar_view {
            CalendarView::Occurrences => {
                if self.occurrence_scroll_offset + 1 < self.calendar.blocks.len() {
                    self.occurrence_scroll_offset += 1;
                }
            }
            CalendarView::FiveMinuteIntervals | CalendarView::FifteenMinuteIntervals => {
                let interval_count = self
                    .current_intervals()
                    .map_or(0, |intervals| intervals.len());

                if let Some(selected) = self.interval_list_state.selected()
                    && selected + 1 < interval_count
                {
                    self.interval_list_state.select(Some(selected + 1));
                }
            }
        }
    }

    fn current_intervals(&self) -> Option<&[CalendarInterval]> {
        match self.calendar_view {
            CalendarView::Occurrences => None,
            CalendarView::FiveMinuteIntervals => Some(&self.calendar.five_minute_intervals),
            CalendarView::FifteenMinuteIntervals => Some(&self.calendar.fifteen_minute_intervals),
        }
    }
}

impl Widget for &mut App {
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

        match self.calendar_view {
            CalendarView::Occurrences => {
                if self.calendar.blocks.is_empty() {
                    Paragraph::new("No occurrences yet.")
                        .alignment(Alignment::Center)
                        .block(Block::new().borders(Borders::ALL).title(" Occurrences "))
                        .render(calendar_area, buffer);
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
                        Constraint::Length(13),
                        Constraint::Length(9),
                        Constraint::Length(20),
                        Constraint::Fill(1),
                    ];
                    let table = Table::new(rows.skip(self.occurrence_scroll_offset), widths)
                        .header(header)
                        .column_spacing(1)
                        .block(Block::new().borders(Borders::ALL).title(" Occurrences "));

                    table.render(calendar_area, buffer);
                }
            }
            CalendarView::FiveMinuteIntervals => interval::render(
                "5-minute intervals",
                &self.calendar.five_minute_intervals,
                &mut self.interval_list_state,
                calendar_area,
                buffer,
            ),
            CalendarView::FifteenMinuteIntervals => interval::render(
                "15-minute intervals",
                &self.calendar.fifteen_minute_intervals,
                &mut self.interval_list_state,
                calendar_area,
                buffer,
            ),
        }

        let navigation = match self.calendar_view {
            CalendarView::Occurrences => "↑/↓: scroll",
            CalendarView::FiveMinuteIntervals | CalendarView::FifteenMinuteIntervals => {
                "↑/↓: interval"
            }
        };
        let footer = Paragraph::new(format!(
            "Tab: view    {navigation}    ←/→: day    Space: today    Esc: quit"
        ))
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
        KeyCode::Esc => Some(Action::Quit),
        KeyCode::Left => Some(Action::PreviousDay),
        KeyCode::Right => Some(Action::NextDay),
        KeyCode::Char(' ') => Some(Action::Today),
        KeyCode::Tab => Some(Action::NextView),
        KeyCode::Up => Some(Action::ScrollUp),
        KeyCode::Down => Some(Action::ScrollDown),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration as StdDuration, Instant, UNIX_EPOCH};

    use chrono::{Duration, Local, NaiveDate, TimeZone};
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::widgets::{ListState, Widget};

    use super::{Action, App, CalendarView, action_for_event};
    use crate::calendar::{Calendar, CalendarBlock, CalendarInterval, CalendarIntervalContext};
    use crate::events::store::EventStore;

    #[test]
    fn renders_calendar_blocks() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let mut app = app(
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
            vec![],
            vec![],
        );
        let area = Rect::new(0, 0, 80, 10);
        let mut actual = Buffer::empty(area);
        (&mut app).render(area, &mut actual);

        let mut expected = Buffer::with_lines([
            "┌ flogging ────────────────────────────────────────────────────────────────────┐",
            "│                            Tuesday, 11 August 2026                           │",
            "└──────────────────────────────────────────────────────────────────────────────┘",
            "┌ Occurrences ─────────────────────────────────────────────────────────────────┐",
            "│Start         Finish    Application          Description                      │",
            "│09:00         09:05     code.exe             MBM-1111                         │",
            "│09:06         09:11     edge.exe             Documentation                    │",
            "│                                                                              │",
            "└──────────────────────────────────────────────────────────────────────────────┘",
            "        Tab: view    ↑/↓: scroll    ←/→: day    Space: today    Esc: quit       ",
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
    fn renders_five_minute_intervals() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let mut app = app(
            date,
            vec![],
            vec![CalendarInterval::new(
                Local
                    .with_ymd_and_hms(2026, 8, 11, 9, 0, 0)
                    .single()
                    .unwrap()
                    .into(),
                Local
                    .with_ymd_and_hms(2026, 8, 11, 9, 5, 0)
                    .single()
                    .unwrap()
                    .into(),
                vec![
                    CalendarIntervalContext::new(
                        StdDuration::from_secs(3 * 60),
                        "code.exe".to_owned(),
                        "MBM-1111".to_owned(),
                    ),
                    CalendarIntervalContext::new(
                        StdDuration::from_secs(2 * 60),
                        "edge.exe".to_owned(),
                        "Documentation".to_owned(),
                    ),
                ],
            )],
            vec![],
        );
        app.calendar_view = CalendarView::FiveMinuteIntervals;

        let area = Rect::new(0, 0, 80, 10);
        let mut actual = Buffer::empty(area);
        (&mut app).render(area, &mut actual);

        let mut expected = Buffer::with_lines([
            "┌ flogging ────────────────────────────────────────────────────────────────────┐",
            "│                            Tuesday, 11 August 2026                           │",
            "└──────────────────────────────────────────────────────────────────────────────┘",
            "┌ 5-minute intervals ──────────────────────────────────────────────────────────┐",
            "│› 09:00–09:05  ▕━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━▏│",
            "│    ├─ 03:00  code.exe · MBM-1111                                             │",
            "│    └─ 02:00  edge.exe · Documentation                                        │",
            "│                                                                              │",
            "└──────────────────────────────────────────────────────────────────────────────┘",
            "       Tab: view    ↑/↓: interval    ←/→: day    Space: today    Esc: quit      ",
        ]);
        expected.set_style(
            Rect::new(3, 4, 11, 1),
            Style::new().add_modifier(Modifier::BOLD),
        );
        expected.set_style(
            Rect::new(17, 4, 36, 1),
            Style::new().fg(Color::Rgb(122, 162, 247)),
        );
        expected.set_style(
            Rect::new(53, 4, 25, 1),
            Style::new().fg(Color::Rgb(115, 218, 202)),
        );
        expected.set_style(
            Rect::new(3, 5, 5, 1),
            Style::new().add_modifier(Modifier::DIM),
        );
        expected.set_style(
            Rect::new(8, 5, 5, 1),
            Style::new()
                .fg(Color::Rgb(122, 162, 247))
                .add_modifier(Modifier::BOLD),
        );
        expected.set_style(
            Rect::new(3, 6, 5, 1),
            Style::new().add_modifier(Modifier::DIM),
        );
        expected.set_style(
            Rect::new(8, 6, 5, 1),
            Style::new()
                .fg(Color::Rgb(115, 218, 202))
                .add_modifier(Modifier::BOLD),
        );
        expected.set_style(
            Rect::new(0, 9, 80, 1),
            Style::new().add_modifier(Modifier::DIM),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn renders_an_empty_default_view() {
        let mut app = app(
            NaiveDate::from_ymd_opt(2026, 8, 11).unwrap(),
            vec![],
            vec![],
            vec![],
        );

        let area = Rect::new(0, 0, 80, 10);
        let mut actual = Buffer::empty(area);
        (&mut app).render(area, &mut actual);

        let mut expected = Buffer::with_lines([
            "┌ flogging ────────────────────────────────────────────────────────────────────┐",
            "│                            Tuesday, 11 August 2026                           │",
            "└──────────────────────────────────────────────────────────────────────────────┘",
            "┌ Occurrences ─────────────────────────────────────────────────────────────────┐",
            "│                              No occurrences yet.                             │",
            "│                                                                              │",
            "│                                                                              │",
            "│                                                                              │",
            "└──────────────────────────────────────────────────────────────────────────────┘",
            "        Tab: view    ↑/↓: scroll    ←/→: day    Space: today    Esc: quit       ",
        ]);
        expected.set_style(
            Rect::new(0, 9, 80, 1),
            Style::new().add_modifier(Modifier::DIM),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn maps_escape_to_quit_action() {
        let escape = Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(action_for_event(escape), Some(Action::Quit));
    }

    #[test]
    fn does_not_map_removed_shortcuts() {
        let q = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        let r = Event::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        assert_eq!(action_for_event(q), None);
        assert_eq!(action_for_event(r), None);
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
    fn maps_tab_to_next_view_action() {
        let next_view = Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(action_for_event(next_view), Some(Action::NextView));
    }

    #[test]
    fn maps_vertical_arrows_to_scroll_actions() {
        let up = Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        assert_eq!(action_for_event(up), Some(Action::ScrollUp));
        assert_eq!(action_for_event(down), Some(Action::ScrollDown));
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
        let mut app = empty_app(date);
        app.occurrence_scroll_offset = 2;

        app.handle_action(Action::PreviousDay).unwrap();

        assert_eq!(app.selected_date, expected_date);
        assert_eq!(app.calendar.date, expected_date);
        assert_eq!(app.occurrence_scroll_offset, 0);
    }

    #[test]
    fn next_day_action_loads_the_next_date() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let expected_date = date.succ_opt().unwrap();
        let mut app = empty_app(date);
        app.occurrence_scroll_offset = 2;

        app.handle_action(Action::NextDay).unwrap();

        assert_eq!(app.selected_date, expected_date);
        assert_eq!(app.calendar.date, expected_date);
        assert_eq!(app.occurrence_scroll_offset, 0);
    }

    #[test]
    fn today_action_loads_the_current_date() {
        let before = Local::now().date_naive();
        let mut app = empty_app(before.pred_opt().unwrap());
        app.occurrence_scroll_offset = 2;

        app.handle_action(Action::Today).unwrap();

        let after = Local::now().date_naive();
        assert!(app.selected_date == before || app.selected_date == after);
        assert_eq!(app.calendar.date, app.selected_date);
        assert_eq!(app.occurrence_scroll_offset, 0);
    }

    #[test]
    fn next_view_action_cycles_calendar_views() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let mut app = empty_app(date);
        app.occurrence_scroll_offset = 2;

        app.handle_action(Action::NextView).unwrap();
        assert_eq!(app.calendar_view, CalendarView::FiveMinuteIntervals);
        assert_eq!(app.occurrence_scroll_offset, 0);

        app.handle_action(Action::NextView).unwrap();
        assert_eq!(app.calendar_view, CalendarView::FifteenMinuteIntervals);

        app.handle_action(Action::NextView).unwrap();
        assert_eq!(app.calendar_view, CalendarView::Occurrences);
    }

    #[test]
    fn entering_an_interval_view_selects_the_first_interval() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let mut app = app(
            date,
            vec![],
            vec![CalendarInterval::new(
                UNIX_EPOCH,
                UNIX_EPOCH + StdDuration::from_secs(300),
                vec![],
            )],
            vec![],
        );

        app.handle_action(Action::NextView).unwrap();

        assert_eq!(app.interval_list_state.selected(), Some(0));
    }

    #[test]
    fn occurrence_scrolling_stays_within_bounds() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let mut app = app(
            date,
            vec![
                CalendarBlock::new(UNIX_EPOCH, "a.exe".to_owned(), "A".to_owned()),
                CalendarBlock::new(UNIX_EPOCH, "b.exe".to_owned(), "B".to_owned()),
            ],
            vec![],
            vec![],
        );

        app.handle_action(Action::ScrollDown).unwrap();
        assert_eq!(app.occurrence_scroll_offset, 1);

        app.handle_action(Action::ScrollDown).unwrap();
        assert_eq!(app.occurrence_scroll_offset, 1);

        app.handle_action(Action::ScrollUp).unwrap();
        assert_eq!(app.occurrence_scroll_offset, 0);

        app.handle_action(Action::ScrollUp).unwrap();
        assert_eq!(app.occurrence_scroll_offset, 0);
    }

    #[test]
    fn interval_selection_stays_within_bounds() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let interval_start = Local
            .with_ymd_and_hms(2026, 8, 11, 9, 0, 0)
            .single()
            .unwrap();
        let mut app = app(
            date,
            vec![],
            vec![
                CalendarInterval::new(
                    interval_start.into(),
                    (interval_start + Duration::minutes(5)).into(),
                    vec![CalendarIntervalContext::new(
                        StdDuration::from_secs(5 * 60),
                        "code.exe".to_owned(),
                        "Context A".to_owned(),
                    )],
                ),
                CalendarInterval::new(
                    (interval_start + Duration::minutes(5)).into(),
                    (interval_start + Duration::minutes(10)).into(),
                    vec![CalendarIntervalContext::new(
                        StdDuration::from_secs(5 * 60),
                        "edge.exe".to_owned(),
                        "Context B".to_owned(),
                    )],
                ),
            ],
            vec![],
        );
        app.calendar_view = CalendarView::FiveMinuteIntervals;
        app.reset_navigation();

        app.handle_action(Action::ScrollDown).unwrap();
        assert_eq!(app.interval_list_state.selected(), Some(1));

        app.handle_action(Action::ScrollDown).unwrap();
        assert_eq!(app.interval_list_state.selected(), Some(1));

        app.handle_action(Action::ScrollUp).unwrap();
        assert_eq!(app.interval_list_state.selected(), Some(0));

        app.handle_action(Action::ScrollUp).unwrap();
        assert_eq!(app.interval_list_state.selected(), Some(0));
    }

    #[test]
    fn refreshing_clears_selection_when_no_intervals_remain() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let interval_start = Local
            .with_ymd_and_hms(2026, 8, 11, 9, 0, 0)
            .single()
            .unwrap();
        let mut app = app(
            date,
            vec![],
            vec![CalendarInterval::new(
                interval_start.into(),
                (interval_start + Duration::minutes(5)).into(),
                vec![
                    CalendarIntervalContext::new(
                        StdDuration::from_secs(2 * 60),
                        "code.exe".to_owned(),
                        "Context A".to_owned(),
                    ),
                    CalendarIntervalContext::new(
                        StdDuration::from_secs(3 * 60),
                        "edge.exe".to_owned(),
                        "Context B".to_owned(),
                    ),
                ],
            )],
            vec![],
        );
        app.calendar_view = CalendarView::FiveMinuteIntervals;
        app.interval_list_state.select(Some(0));

        app.refresh_calendar().unwrap();

        assert!(app.calendar.five_minute_intervals.is_empty());
        assert_eq!(app.interval_list_state.selected(), None);
    }

    #[test]
    fn quit_action_marks_the_app_for_exit() {
        let mut app = empty_app(NaiveDate::from_ymd_opt(2026, 8, 11).unwrap());

        app.handle_action(Action::Quit).unwrap();

        assert!(app.should_quit);
    }

    fn empty_app(date: NaiveDate) -> App {
        app(date, vec![], vec![], vec![])
    }

    fn app(
        date: NaiveDate,
        blocks: Vec<CalendarBlock>,
        five_minute_intervals: Vec<CalendarInterval>,
        fifteen_minute_intervals: Vec<CalendarInterval>,
    ) -> App {
        App {
            store: EventStore::build(":memory:").unwrap(),
            selected_date: date,
            calendar: Calendar {
                date,
                blocks,
                five_minute_intervals,
                fifteen_minute_intervals,
            },
            calendar_view: CalendarView::Occurrences,
            occurrence_scroll_offset: 0,
            interval_list_state: ListState::default(),
            refresh_at: Instant::now(),
            should_quit: false,
        }
    }
}
