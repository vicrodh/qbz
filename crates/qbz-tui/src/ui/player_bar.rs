//! Player bar — 3-line bottom bar with cover art, track info,
//! progress bar (LineGauge), and codec details.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, LineGauge, Paragraph};
use ratatui::Frame;
use ratatui_image::StatefulImage;

use crate::app::AppState;
use crate::theme::{ACCENT, BG_SECONDARY, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_SECONDARY};

/// Width of the cover art area in columns (approx 2:1 aspect ratio for terminal chars).
const COVER_ART_WIDTH: u16 = 10;

/// Render the player bar (3 lines tall, below main content).
///
/// Takes `&mut AppState` because rendering the cover art with `StatefulImage`
/// requires a mutable reference to the stateful protocol.
pub fn render_player_bar(frame: &mut Frame, area: Rect, state: &mut AppState) {
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

/// Nothing is playing — show idle state with stopped icon.
fn render_idle(frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let mid_y = area.y + area.height / 2;
    let row = Rect::new(area.x, mid_y, area.width, 1);
    let line = Line::from(vec![
        Span::styled("  \u{25a0} ", Style::default().fg(TEXT_DIM)),
        Span::styled("Not playing", Style::default().fg(TEXT_DIM)),
    ]);
    frame.render_widget(Paragraph::new(line), row);
}

/// Active playback: cover art | track info + progress + codec.
fn render_active(frame: &mut Frame, area: Rect, state: &mut AppState) {
    if area.height < 2 {
        return;
    }

    // Determine if we should show cover art:
    // - Need enough horizontal space
    // - Images not disabled
    // - Either have actual art or show placeholder
    let has_cover_space = area.width > COVER_ART_WIDTH + 20;
    let show_cover = has_cover_space && !state.no_images;

    let (cover_area, info_area) = if show_cover {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(COVER_ART_WIDTH),
                Constraint::Min(1),
            ])
            .split(area);
        (Some(h_chunks[0]), h_chunks[1])
    } else if has_cover_space {
        // no_images mode: still show placeholder
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

    // Render cover art or placeholder
    if let Some(cover) = cover_area {
        if !state.no_images {
            if let Some(ref mut protocol) = state.cover_art {
                render_cover_image(frame, cover, protocol);
            } else {
                render_cover_placeholder(frame, cover, state);
            }
        } else {
            render_cover_placeholder(frame, cover, state);
        }
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

    // Row 1: Progress bar using LineGauge
    if rows.len() > 1 {
        render_progress(frame, rows[1], state);
    }

    // Row 2: Codec info
    if rows.len() > 2 {
        render_codec_info(frame, rows[2], state);
    }
}

/// Render actual cover art image using ratatui-image StatefulImage.
fn render_cover_image(
    frame: &mut Frame,
    area: Rect,
    protocol: &mut ratatui_image::protocol::StatefulProtocol,
) {
    if area.height == 0 || area.width < 2 {
        return;
    }

    let image_widget = StatefulImage::default();
    frame.render_stateful_widget(image_widget, area, protocol);
}

/// Render a simple cover art placeholder box with album initials.
fn render_cover_placeholder(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.height == 0 || area.width < 4 {
        return;
    }

    // Draw a bordered box for the cover art area
    let border = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(TEXT_DIM));
    let inner = border.inner(area);
    frame.render_widget(border, area);

    // Show album initials centered in the box
    let album_title = state
        .current_track_title
        .as_deref()
        .unwrap_or("?");
    // Take first letter of each word (up to 2)
    let initials: String = album_title
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();

    let mid_y = inner.y + inner.height / 2;
    if mid_y < inner.y + inner.height && inner.width > 0 {
        let row = Rect::new(inner.x, mid_y, inner.width, 1);
        // Center the initials
        let pad = (inner.width as usize).saturating_sub(initials.len()) / 2;
        let line = Line::from(Span::styled(
            format!("{}{}", " ".repeat(pad), initials),
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
        frame.render_widget(Paragraph::new(line), row);
    }
}

/// Row 0: play state icon + Title — Artist
fn render_track_info(frame: &mut Frame, area: Rect, state: &AppState) {
    let accent = state.dynamic_accent.unwrap_or(ACCENT);

    // Play state icons: ⟳ (buffering), ► (playing), ⏸︎ (paused), ■ (stopped)
    let icon = if state.is_buffering {
        "\u{27f3}" // ⟳
    } else if state.is_playing {
        "\u{25b6}" // ►
    } else if state.current_track_title.is_some() {
        "\u{23f8}\u{fe0e}" // ⏸︎
    } else {
        "\u{25a0}" // ■
    };

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
            Style::default().fg(accent),
        ),
        Span::styled(
            title,
            Style::default()
                .fg(accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " \u{2014} ",
            Style::default().fg(TEXT_DIM),
        ),
        Span::styled(
            artist,
            Style::default().fg(TEXT_SECONDARY),
        ),
    ];

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

/// Row 1: LineGauge progress bar with play state icon, percentage, and duration.
/// When buffering, shows the buffering status text instead of a progress gauge.
fn render_progress(frame: &mut Frame, area: Rect, state: &AppState) {
    let accent = state.dynamic_accent.unwrap_or(ACCENT);

    // When buffering, replace the progress bar with the buffering status text
    if state.is_buffering {
        let status_text = state
            .buffering_status
            .as_deref()
            .unwrap_or("Buffering...");
        let line = Line::from(vec![
            Span::styled(
                format!("  \u{27f3}  {}", status_text),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let ratio = if state.duration_secs > 0 {
        (state.position_secs as f64 / state.duration_secs as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let pct = (ratio * 100.0).round() as u32;

    let pos_str = format_time(state.position_secs);
    let dur_str = format_time(state.duration_secs);

    // Play state icon for the gauge label (matches Jellyfin-TUI pattern)
    let icon = if state.is_playing {
        "\u{25b6}" // ►
    } else if state.current_track_title.is_some() {
        "\u{23f8}\u{fe0e}" // ⏸︎
    } else {
        "\u{25a0}" // ■
    };

    // Duration text rendered to the right of the gauge
    let duration_text = format!(" {} / {} ", pos_str, dur_str);
    let duration_width = duration_text.len() as u16;

    // Split the area: gauge (fill) | duration (fixed)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(duration_width),
        ])
        .split(area);

    // LineGauge with bold filled style (Jellyfin-TUI pattern)
    let gauge = LineGauge::default()
        .block(Block::default())
        .filled_style(
            Style::default()
                .fg(accent)
                .add_modifier(Modifier::BOLD),
        )
        .unfilled_style(
            Style::default()
                .fg(TEXT_DIM)
                .add_modifier(Modifier::BOLD),
        )
        .ratio(ratio)
        .label(Line::from(format!(
            "  {}   {:.0}%",
            icon, pct,
        )));

    frame.render_widget(gauge, chunks[0]);

    // Duration text
    let duration_line = Line::from(Span::styled(
        duration_text,
        Style::default().fg(TEXT_MUTED),
    ));
    frame.render_widget(Paragraph::new(duration_line), chunks[1]);
}

/// Row 2: codec info — format — sample rate — channels — bit depth
fn render_codec_info(frame: &mut Frame, area: Rect, state: &AppState) {
    if let Some(ref quality) = state.current_track_quality {
        // Parse and format codec info in Jellyfin-TUI style: "flac — 96.0 kHz — stereo — 24-bit"
        let sep = Span::styled(
            " \u{2014} ",
            Style::default().fg(TEXT_DIM),
        );

        let mut spans: Vec<Span<'_>> = Vec::new();
        spans.push(Span::styled("  ", Style::default()));

        // Try to decompose the quality string into components
        // Quality strings come in formats like "Hi-Res", "24-bit / 96.0kHz", "16bit/44.1kHz"
        let parts: Vec<&str> = quality.split(&['/', '-'][..]).collect();
        if parts.len() >= 2 {
            // We have structured quality info
            let formatted = format_codec_line(quality);
            spans.push(Span::styled(
                formatted,
                Style::default().fg(HIRES_BADGE),
            ));
        } else {
            spans.push(Span::styled(
                quality.clone(),
                Style::default().fg(HIRES_BADGE),
            ));
        }

        let _ = sep; // separator used in future structured codec info

        let line = Line::from(spans);
        frame.render_widget(Paragraph::new(line), area);
    } else {
        let line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("--", Style::default().fg(TEXT_DIM)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }
}

/// Format a quality string into the Jellyfin-TUI codec line style.
/// Input examples: "24-bit / 96.0kHz", "Hi-Res"
/// Output: "flac \u{2014} 96.0 kHz \u{2014} stereo \u{2014} 24-bit"
fn format_codec_line(quality: &str) -> String {
    // For now, just pass through the quality string with nicer formatting.
    // The full structured codec info requires sample_rate, channels, bit_depth
    // fields in AppState which we don't have yet (quality is a pre-formatted string).
    quality.to_string()
}

fn format_time(secs: u64) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{}:{:02}", m, s)
}
