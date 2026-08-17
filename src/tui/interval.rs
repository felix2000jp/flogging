use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, HighlightSpacing, List, ListItem, ListState, Paragraph, StatefulWidget, Widget,
};

use crate::calendar::{CalendarInterval, CalendarIntervalBlock};

const HIGHLIGHT_SYMBOL: &str = "› ";
const HIGHLIGHT_SYMBOL_WIDTH: usize = 2;
const INLINE_TIMELINE_MINIMUM_WIDTH: usize = 32;
const TIME_LABEL_WIDTH: usize = 11;

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
    if available_lines == 0 || interval.blocks.is_empty() {
        return vec![];
    }

    let visible_block_count = if interval.blocks.len() > available_lines {
        available_lines.saturating_sub(1)
    } else {
        interval.blocks.len()
    };
    let hidden_block_count = interval.blocks.len() - visible_block_count;

    let mut lines = interval
        .blocks
        .iter()
        .take(visible_block_count)
        .enumerate()
        .map(|(index, block)| {
            let is_last_line = hidden_block_count == 0 && index + 1 == visible_block_count;
            interval_detail(block, is_last_line)
        })
        .collect::<Vec<_>>();

    if hidden_block_count > 0 {
        lines.push(Line::from(vec![
            Span::styled("  └─ ", Style::new().add_modifier(Modifier::DIM)),
            Span::styled(
                format!("… {hidden_block_count} more"),
                Style::new().add_modifier(Modifier::DIM),
            ),
        ]));
    }

    lines
}

fn interval_detail(block: &CalendarIntervalBlock, is_last: bool) -> Line<'static> {
    let branch = if is_last { "  └─ " } else { "  ├─ " };
    let duration = block
        .finish
        .duration_since(block.start)
        .expect("an interval block cannot finish before it starts");

    Line::from(vec![
        Span::styled(branch, Style::new().add_modifier(Modifier::DIM)),
        Span::styled(
            format_duration(duration),
            Style::new()
                .fg(color_for_executable(&block.executable))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("  {} · {}", block.executable, block.description)),
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
    let mut previous_block_finished_at = None;

    for block in &interval.blocks {
        let start_column = timeline_column(
            block.start,
            interval.start,
            interval_duration,
            total_nanoseconds,
            width,
        );
        let finish_column = timeline_column(
            block.finish,
            interval.start,
            interval_duration,
            total_nanoseconds,
            width,
        );

        if start_column > cursor {
            spans.push(unobserved_span(start_column - cursor));
            cursor = start_column;
        }

        let segment_start = start_column.max(cursor);
        if finish_column > segment_start {
            let segment_width = finish_column - segment_start;
            let follows_another_block = previous_block_finished_at == Some(block.start);
            let color = color_for_executable(&block.executable);

            if follows_another_block {
                spans.push(Span::styled("▌", Style::new().fg(color)));

                if segment_width > 1 {
                    spans.push(Span::styled(
                        "█".repeat(segment_width - 1),
                        Style::new().fg(color),
                    ));
                }
            } else {
                spans.push(Span::styled(
                    "█".repeat(segment_width),
                    Style::new().fg(color),
                ));
            }

            cursor = finish_column;
        }

        previous_block_finished_at = Some(block.finish);
    }

    if cursor < width {
        spans.push(unobserved_span(width - cursor));
    }

    spans
}

fn timeline_column(
    time: SystemTime,
    interval_start: SystemTime,
    interval_duration: Duration,
    total_nanoseconds: u128,
    width: usize,
) -> usize {
    let elapsed = time
        .duration_since(interval_start)
        .unwrap_or_default()
        .min(interval_duration);

    ((elapsed.as_nanos() * width as u128) / total_nanoseconds) as usize
}

fn unobserved_span(width: usize) -> Span<'static> {
    Span::styled("░".repeat(width), Style::new().add_modifier(Modifier::DIM))
}

fn color_for_executable(executable: &str) -> Color {
    let hash = executable
        .bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });

    let hue = (hash % 360) as f64;
    let saturation = 0.65 + ((hash >> 16) % 21) as f64 / 100.0;
    let value = 0.75 + ((hash >> 24) % 16) as f64 / 100.0;
    let chroma = value * saturation;
    let hue_sector = hue / 60.0;
    let secondary = chroma * (1.0 - (hue_sector.rem_euclid(2.0) - 1.0).abs());
    let (red, green, blue) = match hue_sector as u8 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let minimum = value - chroma;

    Color::Rgb(
        ((red + minimum) * 255.0).round() as u8,
        ((green + minimum) * 255.0).round() as u8,
        ((blue + minimum) * 255.0).round() as u8,
    )
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
    use ratatui::style::Color;
    use ratatui::text::Line;
    use ratatui::widgets::ListState;

    use super::{color_for_executable, render, timeline_spans};
    use crate::calendar::{CalendarInterval, CalendarIntervalBlock};

    #[test]
    fn executable_color_is_stable_and_case_insensitive() {
        assert_eq!(
            color_for_executable("idea64.exe"),
            color_for_executable("IDEA64.EXE")
        );
    }

    #[test]
    fn executable_color_uses_constrained_rgb_values() {
        let Color::Rgb(red, green, blue) = color_for_executable("idea64.exe") else {
            panic!("executable colors must use RGB")
        };

        assert!(red.max(green).max(blue) >= 191);
        assert!(red.min(green).min(blue) <= 80);
    }

    #[test]
    fn timeline_uses_the_complete_available_width() {
        let interval = interval(vec![interval_block(0, 300, "idea64.exe")]);
        let line = Line::from(timeline_spans(&interval, 17));

        assert_eq!(line.width(), 17);
    }

    #[test]
    fn timeline_represents_proportional_blocks() {
        let interval = interval(vec![
            interval_block(0, 180, "idea64.exe"),
            interval_block(180, 300, "msedge.exe"),
        ]);
        let spans = timeline_spans(&interval, 10);

        assert_eq!(spans[0].content, "██████");
        assert_eq!(spans[1].content, "▌");
        assert_eq!(spans[2].content, "███");
        assert_eq!(Line::from(spans).width(), 10);
    }

    #[test]
    fn timeline_renders_unobserved_time_as_a_gap() {
        let interval = interval(vec![interval_block(60, 120, "idea64.exe")]);
        let spans = timeline_spans(&interval, 10);

        assert_eq!(spans[0].content, "░░");
        assert_eq!(spans[1].content, "██");
        assert_eq!(spans[2].content, "░░░░░░");
        assert_eq!(Line::from(spans).width(), 10);
    }

    #[test]
    fn sub_cell_activity_does_not_make_the_timeline_too_wide() {
        let interval = interval(vec![interval_block(60, 61, "idea64.exe")]);
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
        let intervals = vec![interval(vec![interval_block(0, 300, "idea64.exe")])];
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
        assert!(rendered_line(&buffer, 2).contains("▕██████████████████▏"));
        let detail = rendered_line(&buffer, 3);
        assert!(detail.contains("05:00"), "{detail:?}");
    }

    #[test]
    fn selected_interval_summarizes_details_that_do_not_fit() {
        let intervals = vec![interval(vec![
            interval_block(0, 100, "idea64.exe"),
            interval_block(100, 200, "msedge.exe"),
            interval_block(200, 300, "WindowsTerminal.exe"),
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

    fn interval(blocks: Vec<CalendarIntervalBlock>) -> CalendarInterval {
        CalendarInterval::new(UNIX_EPOCH, UNIX_EPOCH + Duration::from_secs(300), blocks)
    }

    fn interval_block(
        start_seconds: u64,
        finish_seconds: u64,
        executable: &str,
    ) -> CalendarIntervalBlock {
        CalendarIntervalBlock::new(
            UNIX_EPOCH + Duration::from_secs(start_seconds),
            UNIX_EPOCH + Duration::from_secs(finish_seconds),
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
