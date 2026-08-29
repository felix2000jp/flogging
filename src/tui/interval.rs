use std::time::Duration;

use chrono::{DateTime, Local};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, HighlightSpacing, List, ListItem, ListState, Padding, Paragraph, Row,
    StatefulWidget, Table, Widget,
};

use super::{Focus, theme};
use crate::calendar::CalendarInterval;

const HIGHLIGHT_SYMBOL: &str = "› ";
const HIGHLIGHT_SYMBOL_WIDTH: usize = 2;
const TIME_LABEL_WIDTH: usize = 11;
pub(super) const INTERVAL_ITEM_HEIGHT: usize = 2;
pub(super) const INTERVAL_TOP_PADDING: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PaneAreas {
    pub(super) intervals: Rect,
    pub(super) details: Rect,
}

pub(super) struct IntervalView<'a> {
    title: &'a str,
    intervals: &'a [CalendarInterval],
    suggestion_status: SuggestionStatus<'a>,
}

impl<'a> IntervalView<'a> {
    pub(super) fn new(
        title: &'a str,
        intervals: &'a [CalendarInterval],
        suggestion_status: SuggestionStatus<'a>,
    ) -> Self {
        Self {
            title,
            intervals,
            suggestion_status,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SuggestionStatus<'a> {
    Idle,
    Running,
    Failed(&'a str),
}

pub(super) fn render(
    view: IntervalView<'_>,
    state: &mut ListState,
    context_offset: usize,
    focus: Focus,
    area: Rect,
    buffer: &mut Buffer,
) -> PaneAreas {
    let [intervals_area, details_area] =
        Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)])
            .spacing(1)
            .areas(area);

    render_intervals(
        view.title,
        view.intervals,
        state,
        focus,
        intervals_area,
        buffer,
    );
    render_details(
        view.intervals,
        state.selected(),
        context_offset,
        focus,
        view.suggestion_status,
        details_area,
        buffer,
    );

    PaneAreas {
        intervals: intervals_area,
        details: details_area,
    }
}

fn render_intervals(
    title: &str,
    intervals: &[CalendarInterval],
    state: &mut ListState,
    focus: Focus,
    area: Rect,
    buffer: &mut Buffer,
) {
    let block = pane_block(title, focus == Focus::Intervals).padding(Padding::new(
        1,
        1,
        INTERVAL_TOP_PADDING,
        0,
    ));

    if intervals.is_empty() {
        state.select(None);
        *state.offset_mut() = 0;

        Paragraph::new(format!("No {} yet.", title.to_lowercase()))
            .alignment(Alignment::Center)
            .style(Style::new().fg(theme::SECONDARY_TEXT))
            .block(block)
            .render(area, buffer);
        return;
    }

    let selected = state.selected().unwrap_or(0).min(intervals.len() - 1);
    state.select(Some(selected));

    let item_width = usize::from(block.inner(area).width).saturating_sub(HIGHLIGHT_SYMBOL_WIDTH);
    let items = intervals
        .iter()
        .map(|interval| ListItem::new(vec![interval_line(interval, item_width), Line::default()]))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(block)
        .highlight_symbol(HIGHLIGHT_SYMBOL)
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(
            Style::new()
                .bg(theme::SELECTED_BACKGROUND)
                .add_modifier(Modifier::BOLD),
        )
        .scroll_padding(1);

    StatefulWidget::render(list, area, buffer, state);
}

fn render_details(
    intervals: &[CalendarInterval],
    selected: Option<usize>,
    context_offset: usize,
    focus: Focus,
    suggestion_status: SuggestionStatus<'_>,
    area: Rect,
    buffer: &mut Buffer,
) {
    let Some(interval) = selected.and_then(|selected| intervals.get(selected)) else {
        Paragraph::new("Select an interval to see its details.")
            .alignment(Alignment::Center)
            .style(Style::new().fg(theme::SECONDARY_TEXT))
            .block(pane_block("Details", focus == Focus::Details))
            .render(area, buffer);
        return;
    };

    let start: DateTime<Local> = interval.start.into();
    let finish: DateTime<Local> = interval.finish.into();
    let title = format!(
        "Details · {}–{}",
        start.format("%H:%M"),
        finish.format("%H:%M")
    );
    let mut block = pane_block(&title, focus == Focus::Details);
    let inner_area = block.inner(area);
    let suggestion_height = u16::from(inner_area.height > 1);
    let [_suggestion_top_padding, suggestion_area, table_area] = Layout::vertical([
        Constraint::Length(suggestion_height),
        Constraint::Length(suggestion_height),
        Constraint::Min(0),
    ])
    .areas(inner_area);

    if interval.contexts.is_empty() {
        Widget::render(block, area, buffer);
        render_suggestion(interval, suggestion_status, suggestion_area, buffer);
        Paragraph::new("No observed contexts in this interval.")
            .alignment(Alignment::Center)
            .style(Style::new().fg(theme::SECONDARY_TEXT))
            .render(table_area, buffer);
        return;
    }

    let context_offset = context_offset.min(interval.contexts.len() - 1);
    let header_spacing = u16::from(table_area.height >= 5);
    let header_height = 1 + header_spacing * 2;
    let visible_context_count = usize::from(table_area.height.saturating_sub(header_height));
    let visible_context_finish =
        (context_offset + visible_context_count).min(interval.contexts.len());
    block = block.title(
        Line::from(format!(
            " {}–{}/{} ",
            context_offset + 1,
            visible_context_finish,
            interval.contexts.len()
        ))
        .alignment(Alignment::Right)
        .style(Style::new().fg(theme::DIM_TEXT)),
    );
    Widget::render(block, area, buffer);
    render_suggestion(interval, suggestion_status, suggestion_area, buffer);

    let rows = interval
        .contexts
        .iter()
        .skip(context_offset)
        .map(|context| {
            Row::new([
                Cell::from(Span::styled(
                    format_duration(context.duration),
                    Style::new()
                        .fg(color_for_executable(&context.executable))
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(context.executable.as_str()),
                Cell::from(context.description.as_str()),
            ])
            .style(Style::new().fg(theme::PRIMARY_TEXT))
        });
    let header = Row::new(["Time", "Application", "Context"])
        .top_margin(header_spacing)
        .bottom_margin(header_spacing)
        .style(Style::new().fg(theme::FOCUS).add_modifier(Modifier::BOLD));
    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(22),
            Constraint::Fill(1),
        ],
    )
    .header(header)
    .column_spacing(2);

    Widget::render(table, table_area, buffer);
}

fn render_suggestion(
    interval: &CalendarInterval,
    suggestion_status: SuggestionStatus<'_>,
    area: Rect,
    buffer: &mut Buffer,
) {
    if area.is_empty() {
        return;
    }

    let mut spans = vec![Span::styled(
        "Task suggestion  ",
        Style::new().fg(theme::FOCUS).add_modifier(Modifier::BOLD),
    )];

    match suggestion_status {
        SuggestionStatus::Running => {
            spans.push(Span::styled(
                "Calculating suggestions…",
                Style::new().fg(theme::INFO),
            ));
        }
        SuggestionStatus::Failed(message) => {
            spans.push(Span::styled("Error", Style::new().fg(theme::ERROR)));
            spans.push(Span::styled(
                format!(" · {message} · Press S to retry"),
                Style::new().fg(theme::DIM_TEXT),
            ));
        }
        SuggestionStatus::Idle => match &interval.suggestion {
            Some(suggestion) => {
                let generated_at: DateTime<Local> = suggestion.generated_at.into();
                if let Some(jira_issue_key) = &suggestion.jira_issue_key {
                    spans.push(Span::styled(
                        jira_issue_key.clone(),
                        Style::new().fg(theme::SUCCESS).add_modifier(Modifier::BOLD),
                    ));
                } else {
                    spans.push(Span::styled(
                        "No matching Jira task",
                        Style::new().fg(theme::WARNING),
                    ));
                }
                spans.push(Span::styled(
                    format!(" · generated {}", generated_at.format("%H:%M")),
                    Style::new().fg(theme::DIM_TEXT),
                ));
            }
            None => {
                spans.push(Span::styled(
                    "Not generated · Press S",
                    Style::new().fg(theme::DIM_TEXT),
                ));
            }
        },
    }

    Paragraph::new(Line::from(spans)).render(area, buffer);
}

fn pane_block(title: &str, focused: bool) -> Block<'static> {
    let border_color = if focused {
        theme::FOCUS
    } else {
        theme::INACTIVE_BORDER
    };
    let title = if focused {
        format!(" {title} · focused ")
    } else {
        format!(" {title} ")
    };

    Block::new()
        .borders(Borders::ALL)
        .padding(Padding::horizontal(1))
        .border_style(Style::new().fg(border_color))
        .style(
            Style::new()
                .bg(theme::PANEL_BACKGROUND)
                .fg(theme::PRIMARY_TEXT),
        )
        .title(Span::styled(title, Style::new().fg(border_color)))
}

fn interval_line(interval: &CalendarInterval, width: usize) -> Line<'static> {
    let start: DateTime<Local> = interval.start.into();
    let finish: DateTime<Local> = interval.finish.into();
    let time_label = format!("{}–{}", start.format("%H:%M"), finish.format("%H:%M"));
    let timeline_width = width.saturating_sub(TIME_LABEL_WIDTH + 4);
    let mut spans = vec![
        Span::styled(time_label, Style::new().fg(theme::SECONDARY_TEXT)),
        Span::styled("  ▕", Style::new().fg(theme::DIM_TEXT)),
    ];
    spans.extend(timeline_spans(interval, timeline_width));
    spans.push(Span::styled("▏", Style::new().fg(theme::DIM_TEXT)));

    Line::from(spans)
}

fn timeline_spans(interval: &CalendarInterval, width: usize) -> Vec<Span<'static>> {
    if width == 0 {
        return vec![];
    }

    let interval_duration = interval
        .finish
        .duration_since(interval.start)
        .expect("a calendar interval cannot finish before it starts");
    let total_nanoseconds = interval_duration.as_nanos();

    if total_nanoseconds == 0 {
        return vec![unobserved_span(width)];
    }

    let mut spans = Vec::new();
    let mut cursor = 0;
    let mut observed_nanoseconds = 0;

    for context in &interval.contexts {
        observed_nanoseconds =
            (observed_nanoseconds + context.duration.as_nanos()).min(total_nanoseconds);
        let finish_column = ((observed_nanoseconds * width as u128) / total_nanoseconds) as usize;
        let segment_width = finish_column.saturating_sub(cursor);

        if segment_width > 0 {
            spans.push(Span::styled(
                "━".repeat(segment_width),
                Style::new().fg(color_for_executable(&context.executable)),
            ));
            cursor = finish_column;
        }
    }

    if cursor < width {
        spans.push(unobserved_span(width - cursor));
    }

    spans
}

fn unobserved_span(width: usize) -> Span<'static> {
    Span::styled("─".repeat(width), Style::new().fg(theme::DIM_TEXT))
}

fn color_for_executable(executable: &str) -> ratatui::style::Color {
    let mut hash = executable
        .bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });

    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;

    let palette_index = (hash % theme::APPLICATION_COLORS.len() as u64) as usize;

    theme::APPLICATION_COLORS[palette_index]
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::text::Line;
    use ratatui::widgets::ListState;

    use super::{IntervalView, SuggestionStatus, color_for_executable, render, timeline_spans};
    use crate::calendar::{CalendarInterval, CalendarIntervalContext};
    use crate::suggestions::Suggestion;
    use crate::tui::{Focus, theme};

    #[test]
    fn executable_color_is_stable_and_case_insensitive() {
        assert_eq!(
            color_for_executable("idea64.exe"),
            color_for_executable("IDEA64.EXE")
        );
    }

    #[test]
    fn application_palette_contains_sixteen_distinct_colors() {
        assert_eq!(theme::APPLICATION_COLORS.len(), 16);

        for (index, color) in theme::APPLICATION_COLORS.iter().enumerate() {
            assert!(!theme::APPLICATION_COLORS[..index].contains(color));
        }
    }

    #[test]
    fn common_applications_are_distributed_across_the_palette() {
        let executables = [
            "WindowsTerminal.exe",
            "ms-teams.exe",
            "Bruno.exe",
            "Notepad.exe",
            "msedge.exe",
            "idea64.exe",
            "Code.exe",
        ];
        let colors = executables.map(color_for_executable);

        for (index, color) in colors.iter().enumerate() {
            assert!(!colors[..index].contains(color));
        }
    }

    #[test]
    fn timeline_represents_contexts_and_unobserved_time_proportionally() {
        let interval = interval(vec![
            interval_context(180, "idea64.exe", "Context A"),
            interval_context(60, "msedge.exe", "Context B"),
        ]);
        let spans = timeline_spans(&interval, 10);

        assert_eq!(spans[0].content, "━━━━━━");
        assert_eq!(spans[1].content, "━━");
        assert_eq!(spans[2].content, "──");
        assert_eq!(Line::from(spans).width(), 10);
    }

    #[test]
    fn renders_interval_list_and_selected_interval_details_in_separate_panes() {
        let intervals = vec![interval(vec![
            interval_context(180, "idea64.exe", "MBFSNL-11923"),
            interval_context(120, "msedge.exe", "Pull request"),
        ])];
        let area = Rect::new(0, 0, 80, 20);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        let panes = render(
            IntervalView::new("5-minute intervals", &intervals, SuggestionStatus::Idle),
            &mut state,
            0,
            Focus::Intervals,
            area,
            &mut buffer,
        );
        let rendered = rendered_text(&buffer);

        assert!(rendered.contains("5-minute intervals · focused"));
        assert!(rendered.contains("Details ·"));
        assert!(rendered.contains("idea64.exe"));
        assert!(rendered.contains("MBFSNL-11923"));
        assert!(panes.intervals.bottom() < panes.details.top());
        assert_eq!(
            buffer[(panes.intervals.x, panes.intervals.y)].fg,
            theme::FOCUS
        );
        assert_eq!(
            buffer[(panes.details.x, panes.details.y)].fg,
            theme::INACTIVE_BORDER
        );
    }

    #[test]
    fn selected_interval_retains_its_application_colors() {
        let executable = "idea64.exe";
        let intervals = vec![interval(vec![interval_context(
            300,
            executable,
            "MBFSNL-11923",
        )])];
        let area = Rect::new(0, 0, 80, 12);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        let panes = render(
            IntervalView::new("5-minute intervals", &intervals, SuggestionStatus::Idle),
            &mut state,
            0,
            Focus::Intervals,
            area,
            &mut buffer,
        );
        let timeline_cell = (panes.intervals.x..panes.intervals.right())
            .map(|x| &buffer[(x, panes.intervals.y + 2)])
            .find(|cell| cell.symbol() == "━")
            .expect("the selected interval should contain a timeline");

        assert_eq!(timeline_cell.fg, color_for_executable(executable));
        assert_eq!(timeline_cell.bg, theme::SELECTED_BACKGROUND);
    }

    #[test]
    fn interval_rows_are_separated_by_an_empty_line() {
        let intervals = vec![
            interval(vec![interval_context(300, "first.exe", "First")]),
            interval(vec![interval_context(300, "second.exe", "Second")]),
        ];
        let area = Rect::new(0, 0, 80, 20);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        let panes = render(
            IntervalView::new("5-minute intervals", &intervals, SuggestionStatus::Idle),
            &mut state,
            0,
            Focus::Intervals,
            area,
            &mut buffer,
        );

        assert!(!rendered_line(&buffer, panes.intervals.y + 1).contains('–'));
        assert!(rendered_line(&buffer, panes.intervals.y + 2).contains('–'));
        assert!(!rendered_line(&buffer, panes.intervals.y + 3).contains('–'));
        assert!(rendered_line(&buffer, panes.intervals.y + 4).contains('–'));
    }

    #[test]
    fn detail_offset_controls_the_first_visible_context() {
        let intervals = vec![interval(vec![
            interval_context(100, "first.exe", "First"),
            interval_context(100, "second.exe", "Second"),
            interval_context(100, "third.exe", "Third"),
        ])];
        let area = Rect::new(0, 0, 80, 24);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        render(
            IntervalView::new("5-minute intervals", &intervals, SuggestionStatus::Idle),
            &mut state,
            1,
            Focus::Details,
            area,
            &mut buffer,
        );
        let rendered = rendered_text(&buffer);

        assert!(!rendered.contains("first.exe"));
        assert!(rendered.contains("second.exe"));
        assert!(rendered.contains("third.exe"));
        assert!(rendered.contains("Details ·"));
    }

    #[test]
    fn details_header_is_accented_and_separated_from_contexts() {
        let intervals = vec![interval(vec![interval_context(
            300,
            "idea64.exe",
            "MBFSNL-11923",
        )])];
        let area = Rect::new(0, 0, 80, 30);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        let panes = render(
            IntervalView::new("5-minute intervals", &intervals, SuggestionStatus::Idle),
            &mut state,
            0,
            Focus::Details,
            area,
            &mut buffer,
        );
        let detail_lines = (panes.details.y + 1..panes.details.bottom() - 1)
            .map(|y| rendered_line(&buffer, y))
            .collect::<Vec<_>>();
        let header_index = detail_lines
            .iter()
            .position(|line| line.contains("Time"))
            .expect("the details table should render its header");
        let top_spacing = &detail_lines[header_index - 1];
        let header = &detail_lines[header_index];
        let bottom_spacing = &detail_lines[header_index + 1];
        let first_context = &detail_lines[header_index + 2];
        let header_cell = (panes.details.x..panes.details.right())
            .map(|x| {
                &buffer[(
                    x,
                    panes.details.y + 1 + u16::try_from(header_index).unwrap(),
                )]
            })
            .find(|cell| cell.symbol() == "T")
            .expect("the details table should render its header");

        assert!(!top_spacing.contains("Time"));
        assert!(header.contains("Time"));
        assert!(!bottom_spacing.contains("idea64.exe"));
        assert!(first_context.contains("idea64.exe"));
        assert_eq!(header_cell.fg, theme::FOCUS);
    }

    #[test]
    fn details_show_the_single_suggestion_surface() {
        let intervals = vec![interval(vec![interval_context(
            300,
            "idea64.exe",
            "MBFSNL-11923",
        )])];
        let area = Rect::new(0, 0, 100, 20);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        render(
            IntervalView::new("5-minute intervals", &intervals, SuggestionStatus::Idle),
            &mut state,
            0,
            Focus::Details,
            area,
            &mut buffer,
        );
        let rendered = rendered_text(&buffer);

        assert!(rendered.contains("Task suggestion"));
        assert!(rendered.contains("Not generated · Press S"));
    }

    #[test]
    fn suggestion_is_spaced_from_the_title_and_close_to_the_table() {
        let intervals = vec![interval(vec![interval_context(
            300,
            "idea64.exe",
            "MBFSNL-11923",
        )])];
        let area = Rect::new(0, 0, 100, 20);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        let panes = render(
            IntervalView::new("5-minute intervals", &intervals, SuggestionStatus::Idle),
            &mut state,
            0,
            Focus::Details,
            area,
            &mut buffer,
        );
        let detail_lines = (panes.details.y + 1..panes.details.bottom() - 1)
            .map(|y| rendered_line(&buffer, y))
            .collect::<Vec<_>>();
        let suggestion_index = detail_lines
            .iter()
            .position(|line| line.contains("Task suggestion"))
            .expect("the details pane should render the suggestion");
        let header_index = detail_lines
            .iter()
            .position(|line| line.contains("Time"))
            .expect("the details pane should render the table header");

        assert!(
            detail_lines[suggestion_index - 1]
                .chars()
                .all(|character| character == ' ' || character == '│')
        );
        assert_eq!(header_index, suggestion_index + 1);
    }

    #[test]
    fn details_show_when_suggestions_are_being_calculated() {
        let intervals = vec![interval(vec![interval_context(
            300,
            "idea64.exe",
            "MBFSNL-11923",
        )])];
        let area = Rect::new(0, 0, 100, 20);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        render(
            IntervalView::new("5-minute intervals", &intervals, SuggestionStatus::Running),
            &mut state,
            0,
            Focus::Details,
            area,
            &mut buffer,
        );

        assert!(rendered_text(&buffer).contains("Calculating suggestions…"));
    }

    #[test]
    fn details_show_a_generated_jira_suggestion() {
        let mut selected_interval = interval(vec![interval_context(
            300,
            "idea64.exe",
            "Implement feature",
        )]);
        selected_interval.suggestion = Some(Suggestion::new(
            selected_interval.start,
            selected_interval.finish,
            UNIX_EPOCH,
            Some("JIRA-42".to_owned()),
        ));
        let intervals = vec![selected_interval];
        let area = Rect::new(0, 0, 100, 20);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        render(
            IntervalView::new("5-minute intervals", &intervals, SuggestionStatus::Idle),
            &mut state,
            0,
            Focus::Details,
            area,
            &mut buffer,
        );
        let rendered = rendered_text(&buffer);

        assert!(rendered.contains("Task suggestion"));
        assert!(rendered.contains("JIRA-42"));
        assert!(rendered.contains("generated"));
    }

    #[test]
    fn details_show_agent_errors_in_the_suggestion_surface() {
        let intervals = vec![interval(vec![interval_context(
            300,
            "idea64.exe",
            "MBFSNL-11923",
        )])];
        let area = Rect::new(0, 0, 100, 20);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        render(
            IntervalView::new(
                "5-minute intervals",
                &intervals,
                SuggestionStatus::Failed("could not reach Ollama"),
            ),
            &mut state,
            0,
            Focus::Details,
            area,
            &mut buffer,
        );
        let rendered = rendered_text(&buffer);

        assert!(rendered.contains("Error · could not reach Ollama"));
        assert!(rendered.contains("Press S to retry"));
    }

    #[test]
    fn empty_intervals_render_both_panes_without_a_selection() {
        let area = Rect::new(0, 0, 60, 14);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(2));

        render(
            IntervalView::new("5-minute intervals", &[], SuggestionStatus::Idle),
            &mut state,
            0,
            Focus::Intervals,
            area,
            &mut buffer,
        );
        let rendered = rendered_text(&buffer);

        assert_eq!(state.selected(), None);
        assert!(rendered.contains("No 5-minute intervals yet."));
        assert!(rendered.contains("Select an interval to see its details."));
    }

    fn interval(contexts: Vec<CalendarIntervalContext>) -> CalendarInterval {
        CalendarInterval::new(UNIX_EPOCH, UNIX_EPOCH + Duration::from_secs(300), contexts)
    }

    fn interval_context(
        duration_seconds: u64,
        executable: &str,
        description: &str,
    ) -> CalendarIntervalContext {
        CalendarIntervalContext::new(
            Duration::from_secs(duration_seconds),
            executable.to_owned(),
            description.to_owned(),
        )
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
