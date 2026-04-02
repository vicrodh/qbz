//! Right panel — split into Lyrics (top) and Queue (bottom).

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use qbz_models::RepeatMode;

use crate::app::AppState;
use crate::theme::{
    ACCENT, BG_SECONDARY, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};

/// Render the right-side panel, split vertically into Lyrics (top) and Queue (bottom).
pub fn render_queue_panel(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(TEXT_DIM))
        .style(Style::default().bg(BG_SECONDARY));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 6 {
        // Too small — render queue only
        render_queue_section(frame, inner, state);
        return;
    }

    // Split: lyrics (40%) | separator (1) | queue (60%)
    let lyrics_height = (inner.height as f32 * 0.35).round() as u16;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(lyrics_height),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);

    render_lyrics_section(frame, chunks[0]);
    render_section_separator(frame, chunks[1]);
    render_queue_section(frame, chunks[2], state);
}

/// Lyrics placeholder (top portion of right panel).
fn render_lyrics_section(frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    // Header
    lines.push(Line::from(Span::styled(
        "Lyrics",
        Style::default()
            .fg(TEXT_MUTED)
            .add_modifier(Modifier::BOLD),
    )));

    // Placeholder
    if area.height > 2 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "No lyrics available",
            Style::default().fg(TEXT_DIM),
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// Thin separator between lyrics and queue.
fn render_section_separator(frame: &mut Frame, area: Rect) {
    let width = area.width as usize;
    let line = "\u{2500}".repeat(width);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().fg(TEXT_DIM)),
        area,
    );
}

/// Queue section (bottom portion of right panel).
fn render_queue_section(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.height == 0 {
        return;
    }

    // Vertical split: header(1) + content(fill) + footer(1)
    let has_footer = area.height >= 4;
    let chunks = if has_footer {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(area)
    };

    render_queue_header(frame, chunks[0]);
    render_queue_content(frame, chunks[1], state);

    if has_footer && chunks.len() > 2 {
        render_queue_footer(frame, chunks[2], state);
    }
}

/// Queue header.
fn render_queue_header(frame: &mut Frame, area: Rect) {
    let header = Line::from(Span::styled(
        "Queue",
        Style::default()
            .fg(TEXT_MUTED)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(Paragraph::new(header), area);
}

/// Queue content: now playing + up next tracks.
fn render_queue_content(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    // Now playing
    match (
        state.current_track_title.as_deref(),
        state.current_track_artist.as_deref(),
    ) {
        (Some(title), Some(artist)) => {
            lines.push(Line::from(vec![
                Span::styled(
                    "\u{25b6} ",
                    Style::default().fg(ACCENT),
                ),
                Span::styled(
                    truncate(title, area.width as usize - 2),
                    Style::default()
                        .fg(TEXT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                format!("  {}", truncate(artist, area.width as usize - 2)),
                Style::default().fg(TEXT_SECONDARY),
            )));
        }
        _ => {
            lines.push(Line::from(Span::styled(
                "Nothing playing",
                Style::default().fg(TEXT_DIM),
            )));
        }
    }

    lines.push(Line::from(""));

    // Up next
    let upcoming = if state.queue_tracks.len() > 1 {
        &state.queue_tracks[1..]
    } else {
        &[]
    };

    if upcoming.is_empty() {
        lines.push(Line::from(Span::styled(
            "Queue is empty",
            Style::default().fg(TEXT_DIM),
        )));
    } else {
        let max_tracks = (area.height as usize).saturating_sub(lines.len());
        for (idx, track) in upcoming.iter().enumerate().take(max_tracks / 2) {
            let num = format!("{:>2}  ", idx + 1);
            let avail = (area.width as usize).saturating_sub(num.len());

            // Title line
            lines.push(Line::from(vec![
                Span::styled(num.clone(), Style::default().fg(TEXT_DIM)),
                Span::styled(
                    truncate(&track.title, avail),
                    Style::default().fg(TEXT_PRIMARY),
                ),
            ]));
            // Artist line (indented)
            let indent = " ".repeat(num.len());
            lines.push(Line::from(vec![
                Span::raw(indent),
                Span::styled(
                    truncate(&track.artist, avail),
                    Style::default().fg(TEXT_SECONDARY),
                ),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// Queue footer: shuffle and repeat status.
fn render_queue_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    let shuffle_label = if state.queue_shuffle {
        Span::styled("S", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("S", Style::default().fg(TEXT_DIM))
    };

    let repeat_label = match state.queue_repeat {
        RepeatMode::Off => Span::styled(" R", Style::default().fg(TEXT_DIM)),
        RepeatMode::All => Span::styled(
            " R*",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        RepeatMode::One => Span::styled(
            " R1",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    };

    let footer = Line::from(vec![shuffle_label, repeat_label]);
    frame.render_widget(Paragraph::new(footer), area);
}

/// Truncate a string to fit within `max_chars`, appending an ellipsis if needed.
fn truncate(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else if max_chars <= 1 {
        chars[..max_chars].iter().collect()
    } else {
        let mut result: String = chars[..max_chars - 1].iter().collect();
        result.push('\u{2026}');
        result
    }
}
