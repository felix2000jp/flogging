use std::time::Duration;

use chrono::{DateTime, Local};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, HighlightSpacing, List, ListItem, ListState, Paragraph, Row,
    StatefulWidget, Table, Widget,
};

use super::{Focus, theme};
use crate::calendar::CalendarInterval;

const HIGHLIGHT_SYMBOL: &str = "› ";
const HIGHLIGHT_SYMBOL_WIDTH: usize = 2;
const TIME_LABEL_WIDTH: usize = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PaneAreas {
    pub(super) intervals: Rect,
    pub(super) details: Rect,
}

pub(super) fn render(
    title: &str,
    intervals: &[CalendarInterval],
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

    render_intervals(title, intervals, state, focus, intervals_area, buffer);
    render_details(
        intervals,
        state.selected(),
        context_offset,
        focus,
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
    let block = pane_block(title, focus == Focus::Intervals);

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
        .map(|interval| ListItem::new(interval_line(interval, item_width)))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(block)
        .highlight_symbol(HIGHLIGHT_SYMBOL)
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(
            Style::new()
                .bg(theme::SELECTED_BACKGROUND)
                .fg(theme::PRIMARY_TEXT)
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

    if interval.contexts.is_empty() {
        Paragraph::new("No observed contexts in this interval.")
            .alignment(Alignment::Center)
            .style(Style::new().fg(theme::SECONDARY_TEXT))
            .block(block)
            .render(area, buffer);
        return;
    }

    let context_offset = context_offset.min(interval.contexts.len() - 1);
    let visible_context_count = usize::from(inner_area.height.saturating_sub(1));
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
    let header = Row::new(["Time", "Application", "Context"]).style(
        Style::new()
            .fg(theme::SECONDARY_TEXT)
            .add_modifier(Modifier::BOLD),
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(22),
            Constraint::Fill(1),
        ],
    )
    .header(header)
    .column_spacing(2)
    .block(block);

    Widget::render(table, area, buffer);
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

    use super::{color_for_executable, render, timeline_spans};
    use crate::calendar::{CalendarInterval, CalendarIntervalContext};
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
            "5-minute intervals",
            &intervals,
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
    fn detail_offset_controls_the_first_visible_context() {
        let intervals = vec![interval(vec![
            interval_context(100, "first.exe", "First"),
            interval_context(100, "second.exe", "Second"),
            interval_context(100, "third.exe", "Third"),
        ])];
        let area = Rect::new(0, 0, 80, 14);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        render(
            "5-minute intervals",
            &intervals,
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
    fn empty_intervals_render_both_panes_without_a_selection() {
        let area = Rect::new(0, 0, 60, 14);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(2));

        render(
            "5-minute intervals",
            &[],
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
