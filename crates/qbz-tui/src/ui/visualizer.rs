//! Cava-style frequency bar visualizer for the right panel.
//!
//! Renders vertical bars using Unicode block characters (U+2581..U+2588)
//! with smooth decay animation. Uses the dynamic accent color from cover
//! art when available, falling back to the theme accent.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::theme::{ACCENT, BG_SECONDARY, TEXT_DIM, TEXT_MUTED};

/// Unicode block characters from 1/8 to full block (bottom-up fill).
const BLOCK_CHARS: [char; 8] = [
    '\u{2581}', // 1/8 ▁
    '\u{2582}', // 2/8 ▂
    '\u{2583}', // 3/8 ▃
    '\u{2584}', // 4/8 ▄
    '\u{2585}', // 5/8 ▅
    '\u{2586}', // 6/8 ▆
    '\u{2587}', // 7/8 ▇
    '\u{2588}', // 8/8 █
];

/// Render the visualizer panel in the given area.
pub fn render_visualizer(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(TEXT_DIM))
        .style(Style::default().bg(BG_SECONDARY));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 4 || inner.height < 3 {
        return;
    }

    // Vertical split: header(1) + visualizer(fill)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    render_header(frame, chunks[0]);
    render_bars(frame, chunks[1], state);
}

/// Render the "Visualizer" header.
fn render_header(frame: &mut Frame, area: Rect) {
    let header = Line::from(Span::styled(
        "Visualizer",
        Style::default()
            .fg(TEXT_MUTED)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(Paragraph::new(header), area);
}

/// Render the frequency bars bottom-to-top using block characters.
fn render_bars(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let accent = state.dynamic_accent.unwrap_or(ACCENT);
    let num_bars = state.visualizer_bars.len();
    let bar_height = area.height as usize;

    if num_bars == 0 {
        // No data — render empty state
        if area.height > 2 {
            let empty = Line::from(Span::styled(
                "No audio signal",
                Style::default().fg(TEXT_DIM),
            ));
            let y_offset = area.height / 2;
            let empty_area = Rect {
                x: area.x,
                y: area.y + y_offset,
                width: area.width,
                height: 1,
            };
            frame.render_widget(Paragraph::new(empty), empty_area);
        }
        return;
    }

    // Calculate how many columns each bar gets.
    // We want spacing between bars, so each bar uses 1 column with 1 space gap.
    // Available width for bars = area.width
    // Each bar+gap takes 2 columns, except the last bar takes 1.
    let available_width = area.width as usize;

    // Determine bar width and spacing to fill the area nicely
    let total_units = num_bars * 2 - 1; // bar(1) + gap(1) repeated, minus last gap
    let (bar_w, gap_w) = if total_units <= available_width {
        // All bars fit with 1-col width and 1-col gap
        (1usize, 1usize)
    } else if num_bars <= available_width {
        // All bars fit with no gap
        (1, 0)
    } else {
        // Not enough space — show as many bars as we can
        (1, 0)
    };

    let bars_to_show = if gap_w > 0 {
        ((available_width + gap_w) / (bar_w + gap_w)).min(num_bars)
    } else {
        available_width.min(num_bars)
    };

    // Center the bars horizontally
    let total_bar_width = if bars_to_show > 0 {
        bars_to_show * bar_w + (bars_to_show - 1) * gap_w
    } else {
        return;
    };
    let x_offset = (available_width.saturating_sub(total_bar_width)) / 2;

    // Build each row from top to bottom
    for row in 0..bar_height {
        let mut spans: Vec<Span> = Vec::new();
        // Leading padding
        if x_offset > 0 {
            spans.push(Span::raw(" ".repeat(x_offset)));
        }

        for bar_idx in 0..bars_to_show {
            let bar_value = state.visualizer_bars[bar_idx];
            // The height of this bar in sub-character units (8 per row)
            let total_sub_units = (bar_value * bar_height as f32 * 8.0) as usize;
            // Which row from the bottom are we at?
            let row_from_bottom = bar_height - 1 - row;
            // Sub-units that start at this row
            let sub_units_at_row = total_sub_units.saturating_sub(row_from_bottom * 8);

            let ch = if sub_units_at_row >= 8 {
                BLOCK_CHARS[7] // full block
            } else if sub_units_at_row > 0 {
                BLOCK_CHARS[sub_units_at_row - 1]
            } else {
                ' '
            };

            // Color gradient: brighter at the top of the bar
            let style = if sub_units_at_row > 0 {
                let brightness_factor =
                    (row_from_bottom as f32 / bar_height as f32 * 0.5 + 0.5).clamp(0.5, 1.0);
                let bar_color = brighten_color(accent, brightness_factor);
                Style::default().fg(bar_color)
            } else {
                Style::default().fg(BG_SECONDARY)
            };

            let bar_str: String = std::iter::repeat(ch).take(bar_w).collect();
            spans.push(Span::styled(bar_str, style));

            // Add gap between bars (except after last)
            if gap_w > 0 && bar_idx < bars_to_show - 1 {
                spans.push(Span::raw(" ".repeat(gap_w)));
            }
        }

        let line = Line::from(spans);
        let row_area = Rect {
            x: area.x,
            y: area.y + row as u16,
            width: area.width,
            height: 1,
        };
        frame.render_widget(Paragraph::new(line), row_area);
    }
}

/// Brighten or dim a color by a factor (0.0 = black, 1.0 = original, >1.0 = brighter).
fn brighten_color(color: ratatui::style::Color, factor: f32) -> ratatui::style::Color {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => {
            let r = ((r as f32 * factor).clamp(0.0, 255.0)) as u8;
            let g = ((g as f32 * factor).clamp(0.0, 255.0)) as u8;
            let b = ((b as f32 * factor).clamp(0.0, 255.0)) as u8;
            ratatui::style::Color::Rgb(r, g, b)
        }
        // For non-RGB colors, just return the original
        other => other,
    }
}
