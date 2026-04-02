//! Album detail view — header with album info, track listing with disc grouping.
//!
//! Navigated to via 'g' from search/favorites results. Shows the full album
//! track listing and supports playing the whole album or individual tracks.

use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
};
use ratatui::Frame;

use crate::app::AppState;
use crate::theme::{ACCENT, BG_SELECTED, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

/// Render the album detail view inside `area`.
pub fn render_album_detail(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let album_state = &state.album;

    // Loading state
    if album_state.loading {
        let msg = Paragraph::new("Loading album...")
            .style(Style::default().fg(ACCENT))
            .alignment(ratatui::layout::Alignment::Center);
        let mid_y = area.y + area.height / 2;
        if mid_y < area.y + area.height {
            frame.render_widget(msg, Rect::new(area.x, mid_y, area.width, 1));
        }
        return;
    }

    // Error state
    if let Some(ref err) = album_state.error {
        let msg = Paragraph::new(format!("Error: {}", err))
            .style(Style::default().fg(crate::theme::DANGER))
            .alignment(ratatui::layout::Alignment::Center);
        let mid_y = area.y + area.height / 2;
        if mid_y < area.y + area.height {
            frame.render_widget(msg, Rect::new(area.x, mid_y, area.width, 1));
        }
        return;
    }

    let album = match &album_state.album {
        Some(a) => a,
        None => return,
    };

    // Split vertically: header (5 lines) + track list (rest)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // album header
            Constraint::Min(1),   // track listing
        ])
        .split(area);

    render_album_header(frame, chunks[0], album);
    render_album_tracks(frame, chunks[1], state);
}

/// Render the album header: title, artist, year, quality badge, track count.
fn render_album_header(frame: &mut Frame, area: Rect, album: &qbz_models::Album) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(TEXT_DIM));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    // Line 1: Album title (bold, accent)
    let title = Line::from(Span::styled(
        &album.title,
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ));

    // Line 2: Artist name
    let artist = Line::from(Span::styled(
        &album.artist.name,
        Style::default().fg(TEXT_PRIMARY),
    ));

    // Line 3: Year | Label | Genre
    let mut meta_parts: Vec<String> = Vec::new();
    if let Some(ref date) = album.release_date_original {
        // Extract year from YYYY-MM-DD
        let year = date.split('-').next().unwrap_or(date);
        meta_parts.push(year.to_string());
    }
    if let Some(ref label) = album.label {
        meta_parts.push(label.name.clone());
    }
    if let Some(ref genre) = album.genre {
        meta_parts.push(genre.name.clone());
    }
    let meta_line = Line::from(Span::styled(
        meta_parts.join(" \u{2022} "),
        Style::default().fg(TEXT_MUTED),
    ));

    // Line 4: Quality badge + track count + total duration
    let mut info_spans: Vec<Span<'_>> = Vec::new();

    if album.hires_streamable {
        info_spans.push(Span::styled("Hi-Res ", Style::default().fg(HIRES_BADGE).bold()));
        if let (Some(bd), Some(sr)) = (album.maximum_bit_depth, album.maximum_sampling_rate) {
            info_spans.push(Span::styled(
                format!("{}-bit/{:.1}kHz", bd, sr),
                Style::default().fg(HIRES_BADGE),
            ));
        }
    } else {
        info_spans.push(Span::styled("CD Quality", Style::default().fg(TEXT_DIM)));
    }

    if let Some(count) = album.tracks_count {
        info_spans.push(Span::styled(
            format!(" \u{2022} {} tracks", count),
            Style::default().fg(TEXT_MUTED),
        ));
    }

    if let Some(dur) = album.duration {
        let mins = dur / 60;
        let secs = dur % 60;
        info_spans.push(Span::styled(
            format!(" \u{2022} {}:{:02}", mins, secs),
            Style::default().fg(TEXT_MUTED),
        ));
    }

    let info_line = Line::from(info_spans);

    let lines = vec![title, artist, meta_line, info_line];
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render the album track listing with disc grouping and scrollbar.
fn render_album_tracks(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let tracks = &state.album.tracks;
    if tracks.is_empty() {
        let msg = Paragraph::new("No tracks")
            .style(Style::default().fg(TEXT_DIM))
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(msg, area);
        return;
    }

    let selected_index = state.album.selected_index;
    let track_count = tracks.len();
    let total_width = area.width as usize;

    // Column widths: # (4) | Title (flex) | Duration (8) | Quality (6)
    let num_w: usize = 4;
    let dur_w: usize = 8;
    let quality_w: usize = 6;
    let title_w = total_width.saturating_sub(num_w + dur_w + quality_w + 2);

    // Detect multi-disc albums
    let max_disc = tracks
        .iter()
        .filter_map(|tr| tr.media_number)
        .max()
        .unwrap_or(1);
    let is_multi_disc = max_disc > 1;

    let mut items: Vec<ListItem<'_>> = Vec::new();
    let mut current_disc: u32 = 0;

    for (idx, track) in tracks.iter().enumerate() {
        let disc = track.media_number.unwrap_or(1);

        // Insert disc separator for multi-disc albums
        if is_multi_disc && disc != current_disc {
            current_disc = disc;
            let disc_label = format!("  \u{25CF} Disc {}", disc);
            items.push(ListItem::new(Line::from(Span::styled(
                disc_label,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ))));
        }

        let is_selected = idx == selected_index;

        let num = format!("{:>3} ", track.track_number);
        let title = truncate(&track.title, title_w);
        let dur = format_duration(track.duration);

        let quality = if track.hires_streamable {
            "Hi-Res"
        } else if track.hires {
            "CD+"
        } else {
            "CD"
        };

        let style = if is_selected {
            Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
        } else {
            Style::default().fg(TEXT_SECONDARY)
        };

        let title_padded = format!("{:<width$}", title, width = title_w);
        let dur_padded = format!("{:>width$}", dur, width = dur_w);

        let quality_color = if track.hires_streamable {
            HIRES_BADGE
        } else {
            TEXT_DIM
        };

        let spans = vec![
            Span::styled(num, style.fg(TEXT_DIM)),
            Span::styled(
                title_padded,
                if is_selected {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    style
                },
            ),
            Span::styled(dur_padded, style.fg(TEXT_MUTED)),
            Span::styled(format!(" {:<5}", quality), style.fg(quality_color)),
        ];

        items.push(ListItem::new(Line::from(spans)));
    }

    let list = List::new(items);

    let mut list_state = ListState::default();
    // Offset the selected index by disc separators above it
    if is_multi_disc {
        let mut visual_idx = selected_index;
        let mut seen_disc: u32 = 0;
        for (idx, track) in tracks.iter().enumerate() {
            let disc = track.media_number.unwrap_or(1);
            if disc != seen_disc {
                seen_disc = disc;
                visual_idx += 1; // account for disc separator line
            }
            if idx == selected_index {
                break;
            }
        }
        list_state.select(Some(visual_idx));
    } else {
        list_state.select(Some(selected_index));
    }

    frame.render_stateful_widget(list, area, &mut list_state);

    // Scrollbar
    if track_count > 0 {
        state.album.scrollbar_state = state
            .album
            .scrollbar_state
            .content_length(track_count)
            .position(selected_index);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}"))
            .end_symbol(Some("\u{2193}"));

        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin {
                vertical: 0,
                horizontal: 1,
            }),
            &mut state.album.scrollbar_state,
        );
    }
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

fn format_duration(seconds: u32) -> String {
    let m = seconds / 60;
    let s = seconds % 60;
    format!("{}:{:02}", m, s)
}
