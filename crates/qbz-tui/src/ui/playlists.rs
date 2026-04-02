//! Playlists view — list of playlists with drill-down to track listing.
//!
//! Two modes:
//! - Playlist list: shows all user playlists with track count and duration
//! - Playlist detail: shows tracks in the selected playlist (Enter to open, Backspace to go back)

use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation};
use ratatui::Frame;

use crate::app::AppState;
use crate::theme::{ACCENT, BG_SELECTED, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

/// Render the playlists view inside `area`.
pub fn render_playlists(frame: &mut Frame, area: Rect, state: &mut AppState) {
    if state.playlists.detail_playlist.is_some() {
        render_playlist_detail(frame, area, state);
    } else {
        render_playlist_list(frame, area, state);
    }
}

/// Render the playlist list (overview).
fn render_playlist_list(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let playlists = &state.playlists;

    // Header
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(1),   // list
        ])
        .split(area);

    let header_spans = vec![
        Span::styled("Playlists", Style::default().fg(ACCENT).bold()),
        if playlists.loading {
            Span::styled("  Loading...", Style::default().fg(ACCENT))
        } else if !playlists.playlists.is_empty() {
            Span::styled(
                format!("  {} playlists", playlists.playlists.len()),
                Style::default().fg(TEXT_MUTED),
            )
        } else {
            Span::styled("", Style::default())
        },
    ];
    frame.render_widget(Paragraph::new(Line::from(header_spans)), chunks[0]);

    if playlists.playlists.is_empty() && !playlists.loading {
        let msg = if let Some(ref err) = playlists.error {
            format!("Error: {}", err)
        } else if playlists.loaded {
            "No playlists found".to_string()
        } else {
            String::new()
        };

        if !msg.is_empty() {
            let mid_y = chunks[1].y + chunks[1].height / 2;
            if mid_y < chunks[1].y + chunks[1].height {
                let row = Rect::new(chunks[1].x, mid_y, chunks[1].width, 1);
                let paragraph = Paragraph::new(msg)
                    .style(Style::default().fg(TEXT_DIM))
                    .alignment(ratatui::layout::Alignment::Center);
                frame.render_widget(paragraph, row);
            }
        }
        return;
    }

    let selected_index = playlists.selected_index;
    let total_width = chunks[1].width as usize;

    // Columns: # (4) | Name (flex) | Owner (20%) | Tracks (8) | Duration (8)
    let num_w: usize = 4;
    let tracks_w: usize = 8;
    let dur_w: usize = 8;
    let remaining = total_width.saturating_sub(num_w + tracks_w + dur_w + 2);
    let owner_w = remaining * 20 / 100;
    let name_w = remaining.saturating_sub(owner_w);

    let items: Vec<ListItem<'_>> = playlists
        .playlists
        .iter()
        .enumerate()
        .map(|(idx, playlist)| {
            let is_selected = idx == selected_index;
            let num = format!("{:>3} ", idx + 1);
            let name = truncate(&playlist.name, name_w);
            let owner = truncate(&playlist.owner.name, owner_w.saturating_sub(1));
            let track_count = format!("{:>3} trk", playlist.tracks_count);
            let dur = format_duration(playlist.duration);

            let style = if is_selected {
                Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let name_padded = format!("{:<width$}", name, width = name_w);
            let owner_padded = format!("{:<width$}", owner, width = owner_w);
            let tracks_padded = format!("{:>width$}", track_count, width = tracks_w);
            let dur_padded = format!("{:>width$}", dur, width = dur_w);

            let spans = vec![
                Span::styled(num, style.fg(TEXT_DIM)),
                Span::styled(name_padded, if is_selected { style.bold() } else { style }),
                Span::styled(owner_padded, style.fg(TEXT_MUTED)),
                Span::styled(tracks_padded, style.fg(TEXT_DIM)),
                Span::styled(dur_padded, style.fg(TEXT_MUTED)),
            ];

            ListItem::new(Line::from(spans))
        })
        .collect();

    let playlist_count = playlists.playlists.len();
    let list = List::new(items);
    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));
    frame.render_stateful_widget(list, chunks[1], &mut list_state);

    if playlist_count > 0 {
        state.playlists.scrollbar_state = state
            .playlists
            .scrollbar_state
            .content_length(playlist_count)
            .position(selected_index);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}"))
            .end_symbol(Some("\u{2193}"));

        frame.render_stateful_widget(
            scrollbar,
            chunks[1].inner(Margin { vertical: 0, horizontal: 1 }),
            &mut state.playlists.scrollbar_state,
        );
    }
}

/// Render the playlist detail view (track listing).
fn render_playlist_detail(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let playlist = match &state.playlists.detail_playlist {
        Some(p) => p,
        None => return,
    };

    let tracks = match &playlist.tracks {
        Some(tc) => &tc.items,
        None => {
            let msg = Paragraph::new("No tracks in this playlist")
                .style(Style::default().fg(TEXT_DIM))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(msg, area);
            return;
        }
    };

    // Header (2 lines) + track list
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // playlist header
            Constraint::Min(1),   // track list
        ])
        .split(area);

    // Header: playlist name + track count
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(&playlist.name, Style::default().fg(ACCENT).bold()),
            Span::styled(
                format!("  {} tracks", playlist.tracks_count),
                Style::default().fg(TEXT_MUTED),
            ),
        ]),
        Line::from(Span::styled(
            "Backspace: back  Enter: play  a: add to queue",
            Style::default().fg(TEXT_DIM),
        )),
    ]);
    frame.render_widget(header, chunks[0]);

    // Track list
    let selected_index = state.playlists.detail_selected_index;
    let total_width = chunks[1].width as usize;

    let num_w: usize = 4;
    let dur_w: usize = 8;
    let quality_w: usize = 6;
    let remaining = total_width.saturating_sub(num_w + dur_w + quality_w + 2);
    let album_w = remaining * 30 / 100;
    let title_w = remaining.saturating_sub(album_w);

    let items: Vec<ListItem<'_>> = tracks
        .iter()
        .enumerate()
        .map(|(idx, track)| {
            let is_selected = idx == selected_index;
            let num = format!("{:>3} ", idx + 1);

            let title = truncate(&track.title, title_w.saturating_sub(1));
            let artist_name = track
                .performer
                .as_ref()
                .map(|a| a.name.as_str())
                .unwrap_or("Unknown");

            let album_name = track
                .album
                .as_ref()
                .map(|a| truncate(&a.title, album_w.saturating_sub(2)))
                .unwrap_or_default();

            let dur = format_duration(track.duration);
            let quality = if track.hires_streamable { "Hi-Res" } else { "CD" };

            let style = if is_selected {
                Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let title_artist = format!("{} \u{2014} {}", title, artist_name);
            let title_artist = truncate(&title_artist, title_w);
            let title_padded = format!("{:<width$}", title_artist, width = title_w);
            let album_padded = if album_name.is_empty() {
                " ".repeat(album_w)
            } else {
                format!("{:<width$}", album_name, width = album_w)
            };
            let dur_padded = format!("{:>width$}", dur, width = dur_w);

            let quality_color = if track.hires_streamable { HIRES_BADGE } else { TEXT_DIM };

            let spans = vec![
                Span::styled(num, style.fg(TEXT_DIM)),
                Span::styled(title_padded, if is_selected { style.add_modifier(Modifier::BOLD) } else { style }),
                Span::styled(album_padded, style.fg(TEXT_DIM)),
                Span::styled(dur_padded, style.fg(TEXT_MUTED)),
                Span::styled(format!(" {:<5}", quality), style.fg(quality_color)),
            ];

            ListItem::new(Line::from(spans))
        })
        .collect();

    let track_count = tracks.len();
    let list = List::new(items);
    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));
    frame.render_stateful_widget(list, chunks[1], &mut list_state);

    if track_count > 0 {
        state.playlists.detail_scrollbar_state = state
            .playlists
            .detail_scrollbar_state
            .content_length(track_count)
            .position(selected_index);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}"))
            .end_symbol(Some("\u{2193}"));

        frame.render_stateful_widget(
            scrollbar,
            chunks[1].inner(Margin { vertical: 0, horizontal: 1 }),
            &mut state.playlists.detail_scrollbar_state,
        );
    }
}

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
