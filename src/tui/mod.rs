mod interval;
mod theme;

use std::io;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate, TimeZone};
use ratatui::DefaultTerminal;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, ListState, Paragraph, Widget};

use crate::agents::{AgentInterval, AgentIntervalContext, AgentRequest, SuggestionAgent};
use crate::calendar::{Calendar, CalendarInterval};
use crate::events::store::EventStore;
use crate::suggestions::{SuggestionSet, store::SuggestionStore};

const CALENDAR_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const AGENT_RESULT_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub struct App {
    event_store: EventStore,
    suggestion_store: SuggestionStore,
    suggestion_agent: SuggestionAgent,
    suggestion_job: SuggestionJob,
    selected_date: NaiveDate,
    calendar: Calendar,
    calendar_view: CalendarView,
    focus: Focus,
    interval_list_state: ListState,
    interval_context_offset: usize,
    interval_pane_area: Rect,
    details_pane_area: Rect,
    refresh_at: Instant,
    should_quit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Focus {
    Intervals,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Quit,
    PreviousDay,
    NextDay,
    Today,
    ToggleView,
    ToggleFocus,
    MoveUp,
    MoveDown,
    Suggest,
    MouseScrollUp { column: u16, row: u16 },
    MouseScrollDown { column: u16, row: u16 },
    MouseClick { column: u16, row: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SuggestionJob {
    Idle,
    Running { date: NaiveDate },
    Failed { date: NaiveDate, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalendarView {
    FiveMinuteIntervals,
    FifteenMinuteIntervals,
}

impl App {
    pub fn new(
        event_store: EventStore,
        suggestion_store: SuggestionStore,
        suggestion_agent: SuggestionAgent,
    ) -> Result<Self> {
        let selected_date = Local::now().date_naive();
        let mut app = Self {
            event_store,
            suggestion_store,
            suggestion_agent,
            suggestion_job: SuggestionJob::Idle,
            selected_date,
            calendar: Calendar::new(selected_date, &[], &SuggestionSet::new(vec![], vec![])),
            calendar_view: CalendarView::FiveMinuteIntervals,
            focus: Focus::Intervals,
            interval_list_state: ListState::default(),
            interval_context_offset: 0,
            interval_pane_area: Rect::default(),
            details_pane_area: Rect::default(),
            refresh_at: Instant::now(),
            should_quit: false,
        };

        app.refresh_calendar()?;

        Ok(app)
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.should_quit {
            self.finish_suggestion_job()?;
            terminal.draw(|frame| frame.render_widget(&mut *self, frame.area()))?;

            let today = Local::now().date_naive();
            let mut wait_duration = if self.selected_date == today {
                self.refresh_at.saturating_duration_since(Instant::now())
            } else {
                CALENDAR_REFRESH_INTERVAL
            };
            if self.suggestion_agent.is_running() {
                wait_duration = wait_duration.min(AGENT_RESULT_POLL_INTERVAL);
            }

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
            Action::ToggleView => {
                self.calendar_view = match self.calendar_view {
                    CalendarView::FiveMinuteIntervals => CalendarView::FifteenMinuteIntervals,
                    CalendarView::FifteenMinuteIntervals => CalendarView::FiveMinuteIntervals,
                };
                self.reset_navigation();
            }
            Action::ToggleFocus => {
                self.focus = match self.focus {
                    Focus::Intervals => Focus::Details,
                    Focus::Details => Focus::Intervals,
                };
            }
            Action::MoveUp => self.move_up(),
            Action::MoveDown => self.move_down(),
            Action::Suggest => self.start_suggestion_job()?,
            Action::MouseScrollUp { column, row } => {
                if self.focus_at(column, row) {
                    self.move_up();
                }
            }
            Action::MouseScrollDown { column, row } => {
                if self.focus_at(column, row) {
                    self.move_down();
                }
            }
            Action::MouseClick { column, row } => self.handle_mouse_click(column, row),
        }

        Ok(())
    }

    fn refresh_calendar(&mut self) -> Result<()> {
        let (start, end) = local_date_bounds(self.selected_date)?;

        let events = self.event_store.events_between(start, end)?;
        let suggestions = self.suggestion_store.suggestions_between(start, end)?;
        self.calendar = Calendar::new(self.selected_date, &events, &suggestions);
        self.clamp_navigation();
        self.refresh_at = Instant::now() + CALENDAR_REFRESH_INTERVAL;

        Ok(())
    }

    fn start_suggestion_job(&mut self) -> Result<()> {
        if self.suggestion_agent.is_running() {
            return Ok(());
        }

        self.refresh_calendar()?;
        let (range_start, range_finish) = local_date_bounds(self.selected_date)?;
        let five_minute_intervals = self
            .calendar
            .five_minute_intervals
            .iter()
            .map(agent_interval)
            .collect();
        let fifteen_minute_intervals = self
            .calendar
            .fifteen_minute_intervals
            .iter()
            .map(agent_interval)
            .collect();
        let request = AgentRequest::new(
            self.selected_date,
            range_start,
            range_finish,
            five_minute_intervals,
            fifteen_minute_intervals,
        );

        self.suggestion_agent.start(request)?;
        self.suggestion_job = SuggestionJob::Running {
            date: self.selected_date,
        };

        Ok(())
    }

    fn finish_suggestion_job(&mut self) -> Result<()> {
        let Some(result) = self.suggestion_agent.try_finish() else {
            return Ok(());
        };

        match result {
            Ok(result) => {
                if let Err(error) = self.suggestion_store.replace_between(
                    result.range_start,
                    result.range_finish,
                    &result.suggestions,
                ) {
                    self.suggestion_job = SuggestionJob::Failed {
                        date: result.date,
                        message: format!("Could not save suggestions: {error:#}"),
                    };
                    return Ok(());
                }
                self.suggestion_job = SuggestionJob::Idle;

                if result.date == self.selected_date {
                    self.refresh_calendar()?;
                }
            }
            Err(error) => {
                let date = match self.suggestion_job {
                    SuggestionJob::Running { date } => date,
                    _ => self.selected_date,
                };
                self.suggestion_job = SuggestionJob::Failed {
                    date,
                    message: format!("{error:#}"),
                };
            }
        }

        Ok(())
    }

    fn reset_navigation(&mut self) {
        self.focus = Focus::Intervals;
        self.interval_list_state = ListState::default();
        self.interval_context_offset = 0;

        if !self.current_intervals().is_empty() {
            self.interval_list_state.select(Some(0));
        }
    }

    fn clamp_navigation(&mut self) {
        let interval_count = self.current_intervals().len();

        if interval_count == 0 {
            self.interval_list_state.select(None);
            *self.interval_list_state.offset_mut() = 0;
            self.interval_context_offset = 0;
            return;
        }

        let selected = self
            .interval_list_state
            .selected()
            .unwrap_or(0)
            .min(interval_count - 1);
        self.interval_list_state.select(Some(selected));
        let context_count = self.current_intervals()[selected].contexts.len();
        self.interval_context_offset = self
            .interval_context_offset
            .min(context_count.saturating_sub(1));
    }

    fn move_up(&mut self) {
        match self.focus {
            Focus::Intervals => {
                if let Some(selected) = self.interval_list_state.selected() {
                    let previous = selected.saturating_sub(1);

                    if previous != selected {
                        self.interval_list_state.select(Some(previous));
                        self.interval_context_offset = 0;
                    }
                }
            }
            Focus::Details => {
                self.interval_context_offset = self.interval_context_offset.saturating_sub(1);
            }
        }
    }

    fn move_down(&mut self) {
        match self.focus {
            Focus::Intervals => {
                let interval_count = self.current_intervals().len();

                if let Some(selected) = self.interval_list_state.selected()
                    && selected + 1 < interval_count
                {
                    self.interval_list_state.select(Some(selected + 1));
                    self.interval_context_offset = 0;
                }
            }
            Focus::Details => {
                let context_count = self
                    .selected_interval()
                    .map_or(0, |interval| interval.contexts.len());

                if self.interval_context_offset + 1 < context_count {
                    self.interval_context_offset += 1;
                }
            }
        }
    }

    fn handle_mouse_click(&mut self, column: u16, row: u16) {
        if contains(self.details_pane_area, column, row) {
            self.focus = Focus::Details;
            return;
        }

        if !contains(self.interval_pane_area, column, row) {
            return;
        }

        self.focus = Focus::Intervals;
        let first_row = self
            .interval_pane_area
            .y
            .saturating_add(1 + interval::INTERVAL_TOP_PADDING);
        let last_row = self.interval_pane_area.bottom().saturating_sub(1);

        if row < first_row || row >= last_row {
            return;
        }

        let clicked = self.interval_list_state.offset()
            + usize::from(row - first_row) / interval::INTERVAL_ITEM_HEIGHT;
        if clicked < self.current_intervals().len() {
            self.interval_list_state.select(Some(clicked));
            self.interval_context_offset = 0;
        }
    }

    fn focus_at(&mut self, column: u16, row: u16) -> bool {
        if contains(self.interval_pane_area, column, row) {
            self.focus = Focus::Intervals;
            true
        } else if contains(self.details_pane_area, column, row) {
            self.focus = Focus::Details;
            true
        } else {
            false
        }
    }

    fn selected_interval(&self) -> Option<&CalendarInterval> {
        self.interval_list_state
            .selected()
            .and_then(|selected| self.current_intervals().get(selected))
    }

    fn current_intervals(&self) -> &[CalendarInterval] {
        match self.calendar_view {
            CalendarView::FiveMinuteIntervals => &self.calendar.five_minute_intervals,
            CalendarView::FifteenMinuteIntervals => &self.calendar.fifteen_minute_intervals,
        }
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Block::new()
            .style(Style::new().bg(theme::BACKGROUND).fg(theme::PRIMARY_TEXT))
            .render(area, buffer);

        if area.width < 50 || area.height < 16 {
            Paragraph::new("Terminal too small — resize to at least 50 × 16")
                .alignment(Alignment::Center)
                .style(Style::new().bg(theme::BACKGROUND).fg(theme::SECONDARY_TEXT))
                .render(area, buffer);
            self.interval_pane_area = Rect::default();
            self.details_pane_area = Rect::default();
            return;
        }

        let [header_area, calendar_area, footer_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(area);

        Paragraph::new(self.calendar.date.format("%A, %d %B %Y").to_string())
            .alignment(Alignment::Center)
            .style(
                Style::new()
                    .bg(theme::PANEL_BACKGROUND)
                    .fg(theme::PRIMARY_TEXT),
            )
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_style(Style::new().fg(theme::INACTIVE_BORDER))
                    .style(Style::new().bg(theme::PANEL_BACKGROUND))
                    .title(Span::styled(
                        " flogging ",
                        Style::new().fg(theme::FOCUS).add_modifier(Modifier::BOLD),
                    )),
            )
            .render(header_area, buffer);

        let (title, intervals) = match self.calendar_view {
            CalendarView::FiveMinuteIntervals => (
                "5-minute intervals",
                &self.calendar.five_minute_intervals[..],
            ),
            CalendarView::FifteenMinuteIntervals => (
                "15-minute intervals",
                &self.calendar.fifteen_minute_intervals[..],
            ),
        };
        let suggestion_status = match &self.suggestion_job {
            SuggestionJob::Running { date } if *date == self.calendar.date => {
                interval::SuggestionStatus::Running
            }
            SuggestionJob::Failed { date, message } if *date == self.calendar.date => {
                interval::SuggestionStatus::Failed(message)
            }
            _ => interval::SuggestionStatus::Idle,
        };
        let panes = interval::render(
            interval::IntervalView::new(title, intervals, suggestion_status),
            &mut self.interval_list_state,
            self.interval_context_offset,
            self.focus,
            calendar_area,
            buffer,
        );
        self.interval_pane_area = panes.intervals;
        self.details_pane_area = panes.details;

        let footer = if area.width >= 80 {
            Line::from(vec![
                key("Tab"),
                hint(": focus  "),
                key("↑/↓"),
                hint(": move  "),
                key("V"),
                hint(": 5m/15m  "),
                key("←/→"),
                hint(": day  "),
                key("Space"),
                hint(": today  "),
                key("S"),
                hint(": suggest  "),
                key("Esc"),
                hint(": quit"),
            ])
        } else {
            Line::from(vec![
                key("Tab"),
                hint(" focus  "),
                key("↑/↓"),
                hint(" move  "),
                key("V"),
                hint(" view  "),
                key("←/→"),
                hint(" day  "),
                key("S"),
                hint(" suggest  "),
                key("Esc"),
                hint(" quit"),
            ])
        }
        .alignment(Alignment::Center);
        Paragraph::new(footer)
            .style(Style::new().bg(theme::BACKGROUND))
            .render(footer_area, buffer);
    }
}

fn key(label: &'static str) -> Span<'static> {
    Span::styled(
        label,
        Style::new().fg(theme::FOCUS).add_modifier(Modifier::BOLD),
    )
}

fn hint(label: &'static str) -> Span<'static> {
    Span::styled(label, Style::new().fg(theme::DIM_TEXT))
}

fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
}

fn local_date_bounds(date: NaiveDate) -> Result<(SystemTime, SystemTime)> {
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
                "cannot build the calendar for {date}: local midnight for the following date {next_date} is missing or ambiguous"
            )
        })?;

    Ok((start.into(), end.into()))
}

fn agent_interval(interval: &CalendarInterval) -> AgentInterval {
    AgentInterval::new(
        interval.start,
        interval.finish,
        interval
            .contexts
            .iter()
            .map(|context| {
                AgentIntervalContext::new(
                    context.duration,
                    context.executable.clone(),
                    context.description.clone(),
                )
            })
            .collect(),
    )
}

fn wait_for_action(timeout: Duration) -> io::Result<Option<Action>> {
    if !event::poll(timeout)? {
        return Ok(None);
    }

    Ok(action_for_event(event::read()?))
}

fn action_for_event(event: Event) -> Option<Action> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
            KeyCode::Esc => Some(Action::Quit),
            KeyCode::Left => Some(Action::PreviousDay),
            KeyCode::Right => Some(Action::NextDay),
            KeyCode::Char(' ') => Some(Action::Today),
            KeyCode::Char('v' | 'V') => Some(Action::ToggleView),
            KeyCode::Char('s' | 'S') => Some(Action::Suggest),
            KeyCode::Tab => Some(Action::ToggleFocus),
            KeyCode::Up => Some(Action::MoveUp),
            KeyCode::Down => Some(Action::MoveDown),
            _ => None,
        },
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => Some(Action::MouseScrollUp {
                column: mouse.column,
                row: mouse.row,
            }),
            MouseEventKind::ScrollDown => Some(Action::MouseScrollDown {
                column: mouse.column,
                row: mouse.row,
            }),
            MouseEventKind::Down(MouseButton::Left) => Some(Action::MouseClick {
                column: mouse.column,
                row: mouse.row,
            }),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration as StdDuration, Instant, SystemTime, UNIX_EPOCH};

    use chrono::{Local, NaiveDate, TimeZone};
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };
    use ratatui::layout::Rect;
    use ratatui::widgets::{ListState, Widget};

    use super::{Action, App, CalendarView, Focus, SuggestionJob, action_for_event};
    use crate::agents::SuggestionAgent;
    use crate::calendar::{Calendar, CalendarInterval, CalendarIntervalContext};
    use crate::events::Event as ActivityEvent;
    use crate::events::store::EventStore;
    use crate::suggestions::store::SuggestionStore;
    use crate::suggestions::{Suggestion, SuggestionSet};
    use crate::tui::theme;

    #[test]
    fn five_minute_intervals_are_the_default_view() {
        let app = empty_app(NaiveDate::from_ymd_opt(2026, 8, 11).unwrap());

        assert_eq!(app.calendar_view, CalendarView::FiveMinuteIntervals);
        assert_eq!(app.focus, Focus::Intervals);
    }

    #[test]
    fn renders_the_tokyo_night_theme_and_separate_panes() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let mut app = app(
            date,
            vec![interval(vec![
                context(180, "code.exe", "MBFSNL-11923"),
                context(120, "edge.exe", "Pull request"),
            ])],
            vec![],
        );
        app.reset_navigation();
        let area = Rect::new(0, 0, 80, 24);
        let mut buffer = Buffer::empty(area);

        (&mut app).render(area, &mut buffer);
        let rendered = rendered_text(&buffer);

        assert!(rendered.contains("5-minute intervals · focused"));
        assert!(rendered.contains("Details ·"));
        assert!(rendered.contains("code.exe"));
        assert!(rendered.contains("MBFSNL-11923"));
        assert_eq!(buffer[(0, 23)].bg, theme::BACKGROUND);
        assert_eq!(
            buffer[(app.interval_pane_area.x, app.interval_pane_area.y)].fg,
            theme::FOCUS
        );
        assert_eq!(
            buffer[(app.details_pane_area.x, app.details_pane_area.y)].fg,
            theme::INACTIVE_BORDER
        );
    }

    #[test]
    fn empty_calendar_still_renders_the_interval_and_detail_panes() {
        let mut app = empty_app(NaiveDate::from_ymd_opt(2026, 8, 11).unwrap());
        let area = Rect::new(0, 0, 80, 24);
        let mut buffer = Buffer::empty(area);

        (&mut app).render(area, &mut buffer);
        let rendered = rendered_text(&buffer);

        assert!(rendered.contains("No 5-minute intervals yet."));
        assert!(rendered.contains("Select an interval to see its details."));
    }

    #[test]
    fn terminal_below_the_minimum_size_shows_a_clear_message() {
        let mut app = empty_app(NaiveDate::from_ymd_opt(2026, 8, 11).unwrap());
        let area = Rect::new(0, 0, 49, 15);
        let mut buffer = Buffer::empty(area);

        (&mut app).render(area, &mut buffer);

        assert!(rendered_text(&buffer).contains("resize to at least 50 × 16"));
        assert_eq!(app.interval_pane_area, Rect::default());
        assert_eq!(app.details_pane_area, Rect::default());
    }

    #[test]
    fn maps_keyboard_controls() {
        assert_eq!(key_action(KeyCode::Esc), Some(Action::Quit));
        assert_eq!(key_action(KeyCode::Left), Some(Action::PreviousDay));
        assert_eq!(key_action(KeyCode::Right), Some(Action::NextDay));
        assert_eq!(key_action(KeyCode::Char(' ')), Some(Action::Today));
        assert_eq!(key_action(KeyCode::Char('v')), Some(Action::ToggleView));
        assert_eq!(key_action(KeyCode::Char('V')), Some(Action::ToggleView));
        assert_eq!(key_action(KeyCode::Char('s')), Some(Action::Suggest));
        assert_eq!(key_action(KeyCode::Char('S')), Some(Action::Suggest));
        assert_eq!(key_action(KeyCode::Tab), Some(Action::ToggleFocus));
        assert_eq!(key_action(KeyCode::Up), Some(Action::MoveUp));
        assert_eq!(key_action(KeyCode::Down), Some(Action::MoveDown));
    }

    #[test]
    fn maps_mouse_controls() {
        assert_eq!(
            mouse_action(MouseEventKind::ScrollUp, 12, 8),
            Some(Action::MouseScrollUp { column: 12, row: 8 })
        );
        assert_eq!(
            mouse_action(MouseEventKind::ScrollDown, 13, 9),
            Some(Action::MouseScrollDown { column: 13, row: 9 })
        );
        assert_eq!(
            mouse_action(MouseEventKind::Down(MouseButton::Left), 14, 10),
            Some(Action::MouseClick {
                column: 14,
                row: 10
            })
        );
    }

    #[test]
    fn ignores_unmapped_keys_and_non_press_key_events() {
        assert_eq!(key_action(KeyCode::Char('q')), None);
        assert_eq!(key_action(KeyCode::Char('r')), None);

        let repeated = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Right,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        ));
        assert_eq!(action_for_event(repeated), None);
    }

    #[test]
    fn tab_switches_which_pane_the_arrows_control() {
        let mut app = app(
            NaiveDate::from_ymd_opt(2026, 8, 11).unwrap(),
            vec![interval(vec![
                context(100, "a.exe", "A"),
                context(100, "b.exe", "B"),
                context(100, "c.exe", "C"),
            ])],
            vec![],
        );
        app.reset_navigation();

        app.handle_action(Action::ToggleFocus).unwrap();
        app.handle_action(Action::MoveDown).unwrap();

        assert_eq!(app.focus, Focus::Details);
        assert_eq!(app.interval_list_state.selected(), Some(0));
        assert_eq!(app.interval_context_offset, 1);
    }

    #[test]
    fn interval_navigation_changes_selection_and_resets_detail_scrolling() {
        let mut app = app(
            NaiveDate::from_ymd_opt(2026, 8, 11).unwrap(),
            vec![
                interval(vec![context(300, "a.exe", "A")]),
                interval(vec![context(300, "b.exe", "B")]),
            ],
            vec![],
        );
        app.reset_navigation();
        app.interval_context_offset = 1;

        app.handle_action(Action::MoveDown).unwrap();
        app.handle_action(Action::MoveDown).unwrap();

        assert_eq!(app.interval_list_state.selected(), Some(1));
        assert_eq!(app.interval_context_offset, 0);

        app.handle_action(Action::MoveUp).unwrap();
        app.handle_action(Action::MoveUp).unwrap();
        assert_eq!(app.interval_list_state.selected(), Some(0));
    }

    #[test]
    fn toggling_view_switches_between_five_and_fifteen_minutes() {
        let mut app = empty_app(NaiveDate::from_ymd_opt(2026, 8, 11).unwrap());

        app.handle_action(Action::ToggleView).unwrap();
        assert_eq!(app.calendar_view, CalendarView::FifteenMinuteIntervals);

        app.handle_action(Action::ToggleView).unwrap();
        assert_eq!(app.calendar_view, CalendarView::FiveMinuteIntervals);
    }

    #[test]
    fn mouse_wheel_focuses_the_hovered_pane_and_uses_arrow_behavior() {
        let mut app = app(
            NaiveDate::from_ymd_opt(2026, 8, 11).unwrap(),
            vec![interval(vec![
                context(100, "a.exe", "A"),
                context(100, "b.exe", "B"),
                context(100, "c.exe", "C"),
            ])],
            vec![],
        );
        app.reset_navigation();
        render_app(&mut app);

        app.handle_action(Action::MouseScrollDown {
            column: app.details_pane_area.x + 1,
            row: app.details_pane_area.y + 1,
        })
        .unwrap();

        assert_eq!(app.focus, Focus::Details);
        assert_eq!(app.interval_context_offset, 1);
    }

    #[test]
    fn clicking_an_interval_focuses_and_selects_it() {
        let mut app = app(
            NaiveDate::from_ymd_opt(2026, 8, 11).unwrap(),
            vec![interval(vec![]), interval(vec![])],
            vec![],
        );
        app.reset_navigation();
        render_app(&mut app);

        app.handle_action(Action::MouseClick {
            column: app.interval_pane_area.x + 2,
            row: app.interval_pane_area.y + 4,
        })
        .unwrap();

        assert_eq!(app.focus, Focus::Intervals);
        assert_eq!(app.interval_list_state.selected(), Some(1));
    }

    #[test]
    fn day_navigation_rebuilds_the_requested_calendar() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let mut app = empty_app(date);

        app.handle_action(Action::PreviousDay).unwrap();
        assert_eq!(app.calendar.date, date.pred_opt().unwrap());

        app.handle_action(Action::NextDay).unwrap();
        assert_eq!(app.calendar.date, date);
    }

    #[test]
    fn calendar_refresh_loads_both_sets_of_stored_suggestions() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let event_store = EventStore::build(":memory:").unwrap();
        let suggestion_store = SuggestionStore::build(":memory:").unwrap();
        save_activity(&event_store, date);
        let five_minute_suggestion = suggestion(date, 5, "MBFS-1234");
        let fifteen_minute_suggestion = suggestion(date, 15, "MBFS-5678");
        let (day_start, day_end) = day_bounds(date);
        suggestion_store
            .replace_between(
                day_start,
                day_end,
                &SuggestionSet::new(
                    vec![five_minute_suggestion.clone()],
                    vec![fifteen_minute_suggestion.clone()],
                ),
            )
            .unwrap();
        let mut app = app_with_stores(date, event_store, suggestion_store, vec![], vec![]);

        app.refresh_calendar().unwrap();
        app.refresh_calendar().unwrap();

        assert_eq!(
            app.calendar.five_minute_intervals[0].suggestion,
            Some(five_minute_suggestion)
        );
        assert_eq!(
            app.calendar.fifteen_minute_intervals[0].suggestion,
            Some(fifteen_minute_suggestion)
        );
    }

    #[test]
    fn day_navigation_loads_suggestions_for_the_new_date() {
        let first_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let second_date = first_date.succ_opt().unwrap();
        let event_store = EventStore::build(":memory:").unwrap();
        let suggestion_store = SuggestionStore::build(":memory:").unwrap();
        save_activity(&event_store, second_date);
        let expected = suggestion(second_date, 5, "MBFS-1234");
        let (day_start, day_end) = day_bounds(second_date);
        suggestion_store
            .replace_between(
                day_start,
                day_end,
                &SuggestionSet::new(vec![expected.clone()], vec![]),
            )
            .unwrap();
        let mut app = app_with_stores(first_date, event_store, suggestion_store, vec![], vec![]);

        app.handle_action(Action::NextDay).unwrap();

        assert_eq!(app.calendar.date, second_date);
        assert_eq!(
            app.calendar.five_minute_intervals[0].suggestion,
            Some(expected)
        );
    }

    #[test]
    fn calendar_refresh_builds_intervals_when_no_suggestions_are_stored() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
        let event_store = EventStore::build(":memory:").unwrap();
        let suggestion_store = SuggestionStore::build(":memory:").unwrap();
        save_activity(&event_store, date);
        let mut app = app_with_stores(date, event_store, suggestion_store, vec![], vec![]);

        app.refresh_calendar().unwrap();

        assert_eq!(app.calendar.five_minute_intervals.len(), 1);
        assert!(app.calendar.five_minute_intervals[0].suggestion.is_none());
        assert_eq!(app.calendar.fifteen_minute_intervals.len(), 1);
        assert!(
            app.calendar.fifteen_minute_intervals[0]
                .suggestion
                .is_none()
        );
    }

    #[test]
    fn today_action_loads_the_current_date() {
        let before = Local::now().date_naive();
        let mut app = empty_app(before.pred_opt().unwrap());

        app.handle_action(Action::Today).unwrap();

        let after = Local::now().date_naive();
        assert!(app.selected_date == before || app.selected_date == after);
        assert_eq!(app.calendar.date, app.selected_date);
    }

    #[test]
    fn quit_action_marks_the_app_for_exit() {
        let mut app = empty_app(NaiveDate::from_ymd_opt(2026, 8, 11).unwrap());

        app.handle_action(Action::Quit).unwrap();

        assert!(app.should_quit);
    }

    fn empty_app(date: NaiveDate) -> App {
        app(date, vec![], vec![])
    }

    fn app(
        date: NaiveDate,
        five_minute_intervals: Vec<CalendarInterval>,
        fifteen_minute_intervals: Vec<CalendarInterval>,
    ) -> App {
        app_with_stores(
            date,
            EventStore::build(":memory:").unwrap(),
            SuggestionStore::build(":memory:").unwrap(),
            five_minute_intervals,
            fifteen_minute_intervals,
        )
    }

    fn app_with_stores(
        date: NaiveDate,
        store: EventStore,
        suggestion_store: SuggestionStore,
        five_minute_intervals: Vec<CalendarInterval>,
        fifteen_minute_intervals: Vec<CalendarInterval>,
    ) -> App {
        App {
            event_store: store,
            suggestion_store,
            suggestion_agent: SuggestionAgent::new(),
            suggestion_job: SuggestionJob::Idle,
            selected_date: date,
            calendar: Calendar {
                date,
                blocks: vec![],
                five_minute_intervals,
                fifteen_minute_intervals,
            },
            calendar_view: CalendarView::FiveMinuteIntervals,
            focus: Focus::Intervals,
            interval_list_state: ListState::default(),
            interval_context_offset: 0,
            interval_pane_area: Rect::default(),
            details_pane_area: Rect::default(),
            refresh_at: Instant::now(),
            should_quit: false,
        }
    }

    fn save_activity(store: &EventStore, date: NaiveDate) {
        for second in [0, 5] {
            store
                .save(&ActivityEvent::new_foreground_window_event(
                    local_time(date, 10, 0, second),
                    1,
                    "Context A".to_owned(),
                    "application-a.exe".to_owned(),
                    None,
                ))
                .unwrap();
        }
    }

    fn suggestion(date: NaiveDate, duration_minutes: u64, jira_issue_key: &str) -> Suggestion {
        Suggestion::new(
            local_time(date, 10, 0, 0),
            local_time(date, 10, u32::try_from(duration_minutes).unwrap(), 0),
            local_time(date, 12, 0, 0),
            Some(jira_issue_key.to_owned()),
        )
    }

    fn day_bounds(date: NaiveDate) -> (SystemTime, SystemTime) {
        (
            local_time(date, 0, 0, 0),
            local_time(date.succ_opt().unwrap(), 0, 0, 0),
        )
    }

    fn local_time(date: NaiveDate, hour: u32, minute: u32, second: u32) -> SystemTime {
        Local
            .from_local_datetime(&date.and_hms_opt(hour, minute, second).unwrap())
            .single()
            .unwrap()
            .into()
    }

    fn interval(contexts: Vec<CalendarIntervalContext>) -> CalendarInterval {
        CalendarInterval::new(
            UNIX_EPOCH,
            UNIX_EPOCH + StdDuration::from_secs(300),
            contexts,
        )
    }

    fn context(
        duration_seconds: u64,
        executable: &str,
        description: &str,
    ) -> CalendarIntervalContext {
        CalendarIntervalContext::new(
            StdDuration::from_secs(duration_seconds),
            executable.to_owned(),
            description.to_owned(),
        )
    }

    fn key_action(code: KeyCode) -> Option<Action> {
        action_for_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn mouse_action(kind: MouseEventKind, column: u16, row: u16) -> Option<Action> {
        action_for_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }))
    }

    fn render_app(app: &mut App) {
        let area = Rect::new(0, 0, 80, 24);
        let mut buffer = Buffer::empty(area);
        app.render(area, &mut buffer);
    }

    fn rendered_line(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    fn rendered_text(buffer: &Buffer) -> String {
        (0..buffer.area.height)
            .map(|y| rendered_line(buffer, y))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
