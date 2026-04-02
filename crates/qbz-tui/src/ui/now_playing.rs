use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, LineGauge, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::theme::{ACCENT, BG_SECONDARY, SUCCESS, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY};

/// Render the now-playing bar (3 lines tall).
pub fn render_now_playing(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(TEXT_DIM))
        .style(Style::default().bg(BG_SECONDARY));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if !state.is_playing && state.current_track_title.is_none() {
        render_idle(frame, inner);
    } else {
        render_active(frame, inner, state);
    }
}

/// Nothing is playing.
fn render_idle(frame: &mut Frame, area: Rect) {
    let line = Line::from(Span::styled(
        " \u{25a0} Not playing",
        Style::default().fg(TEXT_MUTED),
    ));
    // Center vertically: skip first row so text sits in the middle of 2 rows
    if area.height >= 1 {
        let row = Rect::new(area.x, area.y, area.width, 1);
        frame.render_widget(Paragraph::new(line), row);
    }
}

/// Track info + progress.
fn render_active(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.height < 2 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    // Row 1: progress gauge
    let ratio = if state.duration_secs > 0 {
        (state.position_secs as f64 / state.duration_secs as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let gauge = LineGauge::default()
        .filled_style(Style::default().fg(ACCENT))
        .unfilled_style(Style::default().fg(TEXT_DIM))
        .ratio(ratio);

    frame.render_widget(gauge, rows[0]);

    // Row 2: play/pause icon + track info + quality + time + volume
    let icon = if state.is_playing { "\u{25b6}" } else { "\u{23f8}" };
    let title = state
        .current_track_title
        .as_deref()
        .unwrap_or("Unknown");
    let artist = state
        .current_track_artist
        .as_deref()
        .unwrap_or("Unknown Artist");

    let mut spans: Vec<Span<'_>> = vec![
        Span::styled(format!(" {icon} "), Style::default().fg(ACCENT)),
        Span::styled(title, Style::default().fg(TEXT_PRIMARY).bold()),
        Span::styled(
            format!(" \u{2014} {artist}"),
            Style::default().fg(TEXT_MUTED),
        ),
    ];

    // Quality badge
    if let Some(ref quality) = state.current_track_quality {
        spans.push(Span::styled(
            format!("  {quality}"),
            Style::default().fg(SUCCESS).bold(),
        ));
    }

    // Time
    let pos = format_time(state.position_secs);
    let dur = format_time(state.duration_secs);
    spans.push(Span::styled(
        format!("  {pos} / {dur}"),
        Style::default().fg(TEXT_MUTED),
    ));

    // Volume mini-bar
    let vol_filled = (state.volume * 10.0).round() as usize;
    let vol_empty = 10usize.saturating_sub(vol_filled);
    let vol_bar = format!(
        "  {}{}",
        "\u{2588}".repeat(vol_filled),
        "\u{2591}".repeat(vol_empty),
    );
    spans.push(Span::styled(vol_bar, Style::default().fg(ACCENT)));

    let info_line = Line::from(spans);
    frame.render_widget(Paragraph::new(info_line), rows[1]);
}

fn format_time(secs: u64) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{m}:{s:02}")
}
