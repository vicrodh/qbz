//! Player bar — 3-line bottom bar with cover art placeholder, track info,
//! progress bar, and codec details.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::theme::{ACCENT, BG_SECONDARY, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

/// Width of the cover art placeholder area in columns.
const COVER_ART_WIDTH: u16 = 16;

/// Render the player bar (3 lines tall, below main content).
pub fn render_player_bar(frame: &mut Frame, area: Rect, state: &AppState) {
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

/// Nothing is playing — show idle state.
fn render_idle(frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let mid_y = area.y + area.height / 2;
    let row = Rect::new(area.x, mid_y, area.width, 1);
    let line = Line::from(Span::styled(
        "  Not playing",
        Style::default().fg(TEXT_DIM),
    ));
    frame.render_widget(Paragraph::new(line), row);
}

/// Active playback: cover art placeholder | track info + progress + codec.
fn render_active(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.height < 2 {
        return;
    }

    // Horizontal split: cover art area | track info area
    let has_cover_space = area.width > COVER_ART_WIDTH + 20;

    let (cover_area, info_area) = if has_cover_space {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(COVER_ART_WIDTH),
                Constraint::Min(1),
            ])
            .split(area);
        (Some(h_chunks[0]), h_chunks[1])
    } else {
        (None, area)
    };

    // Render cover art placeholder
    if let Some(cover) = cover_area {
        render_cover_placeholder(frame, cover, state);
    }

    // Vertical split for info area: row0 (track info) | row1 (progress) | row2 (codec)
    let num_rows = info_area.height.min(3);
    let constraints: Vec<Constraint> = (0..num_rows).map(|_| Constraint::Length(1)).collect();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(info_area);

    // Row 0: Track title — Album (Year) > Artist
    if !rows.is_empty() {
        render_track_info(frame, rows[0], state);
    }

    // Row 1: Progress bar with percentage + elapsed/total
    if rows.len() > 1 {
        render_progress(frame, rows[1], state);
    }

    // Row 2: Codec info
    if rows.len() > 2 {
        render_codec_info(frame, rows[2], state);
    }
}

/// Render a simple cover art placeholder box with album initials.
fn render_cover_placeholder(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.height == 0 || area.width < 4 {
        return;
    }

    // Draw a bordered box for the cover art area
    let border = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(TEXT_DIM));
    let inner = border.inner(area);
    frame.render_widget(border, area);

    // Show album initials centered in the box
    let initials = state
        .current_track_title
        .as_deref()
        .unwrap_or("?")
        .chars()
        .take(2)
        .collect::<String>()
        .to_uppercase();

    let mid_y = inner.y + inner.height / 2;
    if mid_y < inner.y + inner.height {
        let row = Rect::new(inner.x, mid_y, inner.width, 1);
        let line = Line::from(Span::styled(
            format!("  {}", initials),
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
        frame.render_widget(Paragraph::new(line), row);
    }
}

/// Row 0: play/pause icon + Title -- Album > Artist
fn render_track_info(frame: &mut Frame, area: Rect, state: &AppState) {
    let icon = if state.is_playing { "\u{25b6}" } else { "\u{2016}" };

    let title = state
        .current_track_title
        .as_deref()
        .unwrap_or("Unknown");
    let artist = state
        .current_track_artist
        .as_deref()
        .unwrap_or("Unknown Artist");

    let spans = vec![
        Span::styled(
            format!("  {} ", icon),
            Style::default().fg(ACCENT),
        ),
        Span::styled(
            title,
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" \u{2014} {}", artist),
            Style::default().fg(TEXT_SECONDARY),
        ),
    ];

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

/// Row 1: progress bar with percentage + elapsed/total
fn render_progress(frame: &mut Frame, area: Rect, state: &AppState) {
    let ratio = if state.duration_secs > 0 {
        (state.position_secs as f64 / state.duration_secs as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let pct = (ratio * 100.0).round() as u32;

    let pos_str = format_time(state.position_secs);
    let dur_str = format_time(state.duration_secs);

    // Build the progress bar manually for more control
    let prefix = format!("  {} {}% ", if state.is_playing { "\u{25b6}" } else { "\u{2016}" }, pct);
    let suffix = format!(" {} / {} ", pos_str, dur_str);

    let prefix_width = prefix.chars().count();
    let suffix_width = suffix.chars().count();
    let bar_width = (area.width as usize).saturating_sub(prefix_width + suffix_width);

    let filled = ((bar_width as f64) * ratio).round() as usize;
    let empty = bar_width.saturating_sub(filled);

    let bar_filled = "\u{2501}".repeat(filled); // ━
    let bar_empty = "\u{2500}".repeat(empty);   // ─

    let spans = vec![
        Span::styled(prefix, Style::default().fg(TEXT_MUTED)),
        Span::styled(bar_filled, Style::default().fg(ACCENT)),
        Span::styled(bar_empty, Style::default().fg(TEXT_DIM)),
        Span::styled(suffix, Style::default().fg(TEXT_MUTED)),
    ];

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

/// Row 2: codec info — format, sample rate, channels, bitrate
fn render_codec_info(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut parts: Vec<Span<'_>> = Vec::new();
    parts.push(Span::styled("  ", Style::default()));

    if let Some(ref quality) = state.current_track_quality {
        parts.push(Span::styled(
            quality.clone(),
            Style::default().fg(HIRES_BADGE),
        ));
    } else {
        parts.push(Span::styled(
            "--",
            Style::default().fg(TEXT_DIM),
        ));
    }

    let line = Line::from(parts);
    frame.render_widget(Paragraph::new(line), area);
}

fn format_time(secs: u64) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{}:{:02}", m, s)
}
