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

/// Render the right-side queue panel.
///
/// Layout (inside the panel border):
///   row 0       — header tabs ("Queue" active, Lyrics/Cava placeholder)
///   row 1       — separator line
///   rows 2..N-1 — scrollable content: now playing + up next list
///   row N       — footer: shuffle / repeat status
pub fn render_queue_panel(frame: &mut Frame, area: Rect, state: &AppState) {
    // Outer block with a left border to visually separate from main content
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(TEXT_DIM))
        .style(Style::default().bg(BG_SECONDARY));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 4 {
        // Too small to render anything meaningful
        return;
    }

    // Vertical split: header(1) + separator(1) + content(fill) + footer(1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let header_area = chunks[0];
    let separator_area = chunks[1];
    let content_area = chunks[2];
    let footer_area = chunks[3];

    render_header(frame, header_area);
    render_separator(frame, separator_area);
    render_content(frame, content_area, state);
    render_footer(frame, footer_area, state);
}

/// Render the tab bar: "Queue" active, "Lyrics" and "Cava" dimmed.
fn render_header(frame: &mut Frame, area: Rect) {
    let tabs = Line::from(vec![
        Span::styled(
            "Queue",
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        Span::styled("  Lyrics  Cava", Style::default().fg(TEXT_DIM)),
    ]);
    frame.render_widget(Paragraph::new(tabs), area);
}

/// Render a thin separator below the header.
fn render_separator(frame: &mut Frame, area: Rect) {
    let width = area.width as usize;
    let line = "\u{2500}".repeat(width);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().fg(TEXT_DIM)),
        area,
    );
}

/// Render the now-playing section and the "Up Next" track list.
fn render_content(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    // ── Now Playing ────────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        "Now Playing",
        Style::default()
            .fg(TEXT_MUTED)
            .add_modifier(Modifier::BOLD),
    )));

    match (
        state.current_track_title.as_deref(),
        state.current_track_artist.as_deref(),
    ) {
        (Some(title), Some(artist)) => {
            lines.push(Line::from(Span::styled(
                truncate(title, area.width as usize),
                Style::default()
                    .fg(TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                truncate(artist, area.width as usize),
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

    // ── Blank separator ────────────────────────────────────────────
    lines.push(Line::from(""));

    // ── Up Next ────────────────────────────────────────────────────
    // queue_tracks[0] is current; the rest are upcoming.
    let upcoming = if state.queue_tracks.len() > 1 {
        &state.queue_tracks[1..]
    } else {
        &[]
    };

    if upcoming.is_empty() {
        lines.push(Line::from(Span::styled(
            "Up Next",
            Style::default()
                .fg(TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "Queue is empty",
            Style::default().fg(TEXT_DIM),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Up Next",
            Style::default()
                .fg(TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        )));

        // Render as many tracks as fit in the area
        let max_tracks = (area.height as usize).saturating_sub(lines.len());
        for (idx, track) in upcoming.iter().enumerate().take(max_tracks / 2) {
            let index_str = format!("{:>2}. ", idx + 1);
            let avail = (area.width as usize).saturating_sub(index_str.len());

            // Title line
            lines.push(Line::from(vec![
                Span::styled(index_str.clone(), Style::default().fg(TEXT_DIM)),
                Span::styled(
                    truncate(&track.title, avail),
                    Style::default().fg(TEXT_PRIMARY),
                ),
            ]));
            // Artist line (indented to align with title)
            let indent = " ".repeat(index_str.len());
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

/// Render the footer showing shuffle and repeat status.
fn render_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    let shuffle_label = if state.queue_shuffle {
        Span::styled("Shuffle:ON", Style::default().fg(ACCENT))
    } else {
        Span::styled("Shuffle:off", Style::default().fg(TEXT_DIM))
    };

    let repeat_label = match state.queue_repeat {
        RepeatMode::Off => Span::styled(" Repeat:off", Style::default().fg(TEXT_DIM)),
        RepeatMode::All => Span::styled(" Repeat:ALL", Style::default().fg(ACCENT)),
        RepeatMode::One => Span::styled(" Repeat:ONE", Style::default().fg(ACCENT)),
    };

    let footer = Line::from(vec![shuffle_label, repeat_label]);
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(TEXT_MUTED)),
        area,
    );
}

/// Truncate a string to fit within `max_chars`, appending `…` if needed.
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
        result.push('\u{2026}'); // …
        result
    }
}
