use std::time::Duration;

use chrono::{DateTime, Local};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, HighlightSpacing, List, ListItem, ListState, Paragraph, StatefulWidget, Widget,
};

use crate::calendar::{CalendarInterval, CalendarIntervalContext};

const HIGHLIGHT_SYMBOL: &str = "› ";
const HIGHLIGHT_SYMBOL_WIDTH: usize = 2;
const INLINE_TIMELINE_MINIMUM_WIDTH: usize = 32;
const TIME_LABEL_WIDTH: usize = 11;
const APPLICATION_COLORS: [Color; 10] = [
    Color::Rgb(122, 162, 247),
    Color::Rgb(125, 207, 255),
    Color::Rgb(115, 218, 202),
    Color::Rgb(42, 195, 222),
    Color::Rgb(158, 206, 106),
    Color::Rgb(224, 175, 104),
    Color::Rgb(255, 158, 100),
    Color::Rgb(247, 118, 142),
    Color::Rgb(187, 154, 247),
    Color::Rgb(157, 124, 216),
];

pub(super) fn render(
    title: &str,
    intervals: &[CalendarInterval],
    state: &mut ListState,
    area: Rect,
    buffer: &mut Buffer,
) {
    if intervals.is_empty() {
        state.select(None);

        Paragraph::new(format!("No {} yet.", title.to_lowercase()))
            .alignment(Alignment::Center)
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(format!(" {title} ")),
            )
            .render(area, buffer);

        return;
    }

    let selected = state.selected().unwrap_or(0).min(intervals.len() - 1);
    state.select(Some(selected));

    let block = Block::new()
        .borders(Borders::ALL)
        .title(format!(" {title} "));
    let inner_area = block.inner(area);
    block.render(area, buffer);

    let item_width = usize::from(inner_area.width).saturating_sub(HIGHLIGHT_SYMBOL_WIDTH);
    let items = intervals
        .iter()
        .enumerate()
        .map(|(index, interval)| {
            interval_item(
                interval,
                index == selected,
                item_width,
                usize::from(inner_area.height),
            )
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .highlight_symbol(HIGHLIGHT_SYMBOL)
        .highlight_spacing(HighlightSpacing::Always)
        .scroll_padding(1);

    StatefulWidget::render(list, inner_area, buffer, state);
}

fn interval_item(
    interval: &CalendarInterval,
    selected: bool,
    width: usize,
    maximum_height: usize,
) -> ListItem<'static> {
    let mut lines = interval_summary(interval, selected, width);

    if selected {
        let available_detail_lines = maximum_height.saturating_sub(lines.len());
        lines.extend(interval_details(interval, available_detail_lines));
    }

    ListItem::new(Text::from(lines))
}

fn interval_summary(
    interval: &CalendarInterval,
    selected: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let start: DateTime<Local> = interval.start.into();
    let finish: DateTime<Local> = interval.finish.into();
    let time_label = format!("{}–{}", start.format("%H:%M"), finish.format("%H:%M"));
    let time_style = if selected {
        Style::new().add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };

    if width >= INLINE_TIMELINE_MINIMUM_WIDTH {
        let timeline_width = width.saturating_sub(TIME_LABEL_WIDTH + 4);
        let mut spans = vec![Span::styled(time_label, time_style), Span::raw("  ▕")];
        spans.extend(timeline_spans(interval, timeline_width));
        spans.push(Span::raw("▏"));

        vec![Line::from(spans)]
    } else {
        let timeline_width = width.saturating_sub(2);
        let mut timeline = vec![Span::raw("▕")];
        timeline.extend(timeline_spans(interval, timeline_width));
        timeline.push(Span::raw("▏"));

        vec![Line::styled(time_label, time_style), Line::from(timeline)]
    }
}

fn interval_details(interval: &CalendarInterval, available_lines: usize) -> Vec<Line<'static>> {
    if available_lines == 0 || interval.contexts.is_empty() {
        return vec![];
    }

    let visible_context_count = if interval.contexts.len() > available_lines {
        available_lines.saturating_sub(1)
    } else {
        interval.contexts.len()
    };
    let hidden_context_count = interval.contexts.len() - visible_context_count;

    let mut lines = interval
        .contexts
        .iter()
        .take(visible_context_count)
        .enumerate()
        .map(|(index, context)| {
            let is_last_line = hidden_context_count == 0 && index + 1 == visible_context_count;
            interval_detail(context, is_last_line)
        })
        .collect::<Vec<_>>();

    if hidden_context_count > 0 {
        lines.push(Line::from(vec![
            Span::styled("  └─ ", Style::new().add_modifier(Modifier::DIM)),
            Span::styled(
                format!("… {hidden_context_count} more"),
                Style::new().add_modifier(Modifier::DIM),
            ),
        ]));
    }

    lines
}

fn interval_detail(context: &CalendarIntervalContext, is_last: bool) -> Line<'static> {
    let branch = if is_last { "  └─ " } else { "  ├─ " };

    Line::from(vec![
        Span::styled(branch, Style::new().add_modifier(Modifier::DIM)),
        Span::styled(
            format_duration(context.duration),
            Style::new()
                .fg(color_for_executable(&context.executable))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  {} · {}",
            context.executable, context.description
        )),
    ])
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
            let color = color_for_executable(&context.executable);
            spans.push(Span::styled(
                "━".repeat(segment_width),
                Style::new().fg(color),
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
    Span::styled("─".repeat(width), Style::new().add_modifier(Modifier::DIM))
}

fn color_for_executable(executable: &str) -> Color {
    let hash = executable
        .bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });

    let palette_index = (hash % APPLICATION_COLORS.len() as u64) as usize;
    APPLICATION_COLORS[palette_index]
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

    use super::{APPLICATION_COLORS, color_for_executable, render, timeline_spans};
    use crate::calendar::{CalendarInterval, CalendarIntervalContext};

    #[test]
    fn executable_color_is_stable_and_case_insensitive() {
        assert_eq!(
            color_for_executable("idea64.exe"),
            color_for_executable("IDEA64.EXE")
        );
    }

    #[test]
    fn executable_color_comes_from_the_application_palette() {
        assert!(APPLICATION_COLORS.contains(&color_for_executable("idea64.exe")));
    }

    #[test]
    fn application_palette_contains_ten_distinct_colors() {
        assert_eq!(APPLICATION_COLORS.len(), 10);

        for (index, color) in APPLICATION_COLORS.iter().enumerate() {
            assert!(!APPLICATION_COLORS[..index].contains(color));
        }
    }

    #[test]
    fn timeline_uses_the_complete_available_width() {
        let interval = interval(vec![interval_context(300, "idea64.exe")]);
        let line = Line::from(timeline_spans(&interval, 17));

        assert_eq!(line.width(), 17);
    }

    #[test]
    fn timeline_represents_proportional_contexts() {
        let interval = interval(vec![
            interval_context(180, "idea64.exe"),
            interval_context(120, "msedge.exe"),
        ]);
        let spans = timeline_spans(&interval, 10);

        assert_eq!(spans[0].content, "━━━━━━");
        assert_eq!(spans[1].content, "━━━━");
        assert_eq!(Line::from(spans).width(), 10);
    }

    #[test]
    fn timeline_renders_unobserved_time_as_a_gap() {
        let interval = interval(vec![interval_context(60, "idea64.exe")]);
        let spans = timeline_spans(&interval, 10);

        assert_eq!(spans[0].content, "━━");
        assert_eq!(spans[1].content, "────────");
        assert_eq!(Line::from(spans).width(), 10);
    }

    #[test]
    fn sub_cell_activity_does_not_make_the_timeline_too_wide() {
        let interval = interval(vec![interval_context(1, "idea64.exe")]);
        let line = Line::from(timeline_spans(&interval, 10));

        assert_eq!(line.width(), 10);
    }

    #[test]
    fn empty_interval_view_has_no_selection() {
        let area = Rect::new(0, 0, 40, 5);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(2));

        render("5-minute intervals", &[], &mut state, area, &mut buffer);

        assert_eq!(state.selected(), None);
        let line = rendered_line(&buffer, 1);
        assert!(line.contains("No 5-minute intervals yet."), "{line:?}");
    }

    #[test]
    fn narrow_layout_moves_the_timeline_below_the_time_label() {
        let intervals = vec![interval(vec![interval_context(300, "idea64.exe")])];
        let area = Rect::new(0, 0, 24, 6);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        render(
            "5-minute intervals",
            &intervals,
            &mut state,
            area,
            &mut buffer,
        );

        assert!(rendered_line(&buffer, 1).contains('–'));
        assert!(rendered_line(&buffer, 2).contains("▕━━━━━━━━━━━━━━━━━━▏"));
        let detail = rendered_line(&buffer, 3);
        assert!(detail.contains("05:00"), "{detail:?}");
    }

    #[test]
    fn selected_interval_summarizes_contexts_that_do_not_fit() {
        let intervals = vec![interval(vec![
            interval_context(100, "idea64.exe"),
            interval_context(100, "msedge.exe"),
            interval_context(100, "WindowsTerminal.exe"),
        ])];
        let area = Rect::new(0, 0, 60, 5);
        let mut buffer = Buffer::empty(area);
        let mut state = ListState::default().with_selected(Some(0));

        render(
            "5-minute intervals",
            &intervals,
            &mut state,
            area,
            &mut buffer,
        );

        assert!(rendered_line(&buffer, 2).contains("idea64.exe"));
        assert!(rendered_line(&buffer, 3).contains("… 2 more"));
    }

    fn interval(contexts: Vec<CalendarIntervalContext>) -> CalendarInterval {
        CalendarInterval::new(UNIX_EPOCH, UNIX_EPOCH + Duration::from_secs(300), contexts)
    }

    fn interval_context(duration_seconds: u64, executable: &str) -> CalendarIntervalContext {
        CalendarIntervalContext::new(
            Duration::from_secs(duration_seconds),
            executable.to_owned(),
            "Context".to_owned(),
        )
    }

    fn rendered_line(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }
}
