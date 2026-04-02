//! Favorites view — tab bar, track list with Jellyfin-TUI formatting, scrollbar.

use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation};
use ratatui::Frame;

use crate::app::{AppState, FavoritesTab};
use crate::theme::{ACCENT, BG_SELECTED, DANGER, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

/// Render the full favorites view inside `area`.
pub fn render_favorites(frame: &mut Frame, area: Rect, state: &mut AppState) {
    // Split vertically: tab bar (1) + results (rest)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar + status
            Constraint::Min(1),   // content list
        ])
        .split(area);

    render_tab_bar(frame, chunks[0], state);

    match state.favorites.tab {
        FavoritesTab::Tracks => render_tracks(frame, chunks[1], state),
        FavoritesTab::Albums => render_albums(frame, chunks[1], state),
        FavoritesTab::Artists => render_artists(frame, chunks[1], state),
        FavoritesTab::Playlists => {
            let msg = Paragraph::new("Use Playlists view (key 4) for playlists")
                .style(Style::default().fg(TEXT_DIM))
                .alignment(ratatui::layout::Alignment::Center);
            let mid_y = chunks[1].y + chunks[1].height / 2;
            if mid_y < chunks[1].y + chunks[1].height {
                frame.render_widget(msg, Rect::new(chunks[1].x, mid_y, chunks[1].width, 1));
            }
        }
    }
}

/// Tab bar and track count.
fn render_tab_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    let favs = &state.favorites;

    let mut spans: Vec<Span<'_>> = Vec::new();

    let tabs = [
        (FavoritesTab::Tracks, "Tracks"),
        (FavoritesTab::Albums, "Albums"),
        (FavoritesTab::Artists, "Artists"),
        (FavoritesTab::Playlists, "Playlists"),
    ];

    for (tab, label) in &tabs {
        if *tab == favs.tab {
            spans.push(Span::styled(
                format!(" [{}] ", label),
                Style::default().fg(ACCENT).bold(),
            ));
        } else {
            spans.push(Span::styled(
                format!("  {}  ", label),
                Style::default().fg(TEXT_DIM),
            ));
        }
    }

    // Loading / error / count
    if favs.loading {
        spans.push(Span::styled(
            "  Loading...",
            Style::default().fg(ACCENT),
        ));
    } else if let Some(ref err) = favs.error {
        spans.push(Span::styled(
            format!("  Error: {}", err),
            Style::default().fg(DANGER),
        ));
    } else {
        let count_label = match favs.tab {
            FavoritesTab::Tracks if !favs.tracks.is_empty() => {
                Some(format!("  {} tracks", favs.tracks.len()))
            }
            FavoritesTab::Albums if !favs.albums.is_empty() => {
                Some(format!("  {} albums", favs.albums.len()))
            }
            FavoritesTab::Artists if !favs.artists.is_empty() => {
                Some(format!("  {} artists", favs.artists.len()))
            }
            _ => None,
        };
        if let Some(label) = count_label {
            spans.push(Span::styled(label, Style::default().fg(TEXT_MUTED)));
        }
    }

    // Auth status
    if !state.authenticated {
        spans.push(Span::styled(
            "  [not logged in]",
            Style::default().fg(DANGER),
        ));
    }

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

/// The favorites track list with Jellyfin-TUI-style formatting and scrollbar.
///
/// Column layout: ♥ (2) | # (4) | Title - Artist (flex) | Album (30%) | Duration (8) | Quality (6)
fn render_tracks(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let favs = &state.favorites;

    if favs.tracks.is_empty() && !favs.loading {
        let msg = if !favs.loaded {
            ""
        } else if favs.error.is_some() {
            "Failed to load favorites"
        } else {
            "No favorite tracks yet"
        };

        if !msg.is_empty() {
            let mid_y = area.y + area.height / 2;
            if mid_y < area.y + area.height {
                let row = Rect::new(area.x, mid_y, area.width, 1);
                let paragraph = Paragraph::new(msg)
                    .style(Style::default().fg(TEXT_DIM))
                    .alignment(ratatui::layout::Alignment::Center);
                frame.render_widget(paragraph, row);
            }
        }
        return;
    }

    let selected_index = favs.selected_index;
    let track_count = favs.tracks.len();
    let total_width = area.width as usize;

    // Column widths following Jellyfin-TUI pattern
    let fav_w: usize = 2;      // "♥ "
    let num_w: usize = 4;      // "#"
    let dur_w: usize = 8;      // "  M:SS"
    let quality_w: usize = 6;  // " Hi-Res" or " CD"
    let remaining = total_width.saturating_sub(fav_w + num_w + dur_w + quality_w + 2);
    let album_w = remaining * 30 / 100;
    let title_w = remaining.saturating_sub(album_w);

    let items: Vec<ListItem<'_>> = favs
        .tracks
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
                .unwrap_or("Unknown Artist");

            let album_name = track
                .album
                .as_ref()
                .map(|a| truncate(&a.title, album_w.saturating_sub(2)))
                .unwrap_or_default();

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

            // Build formatted row: ♥ | # | Title - Artist | Album | Duration | Quality
            let title_artist = if !artist_name.is_empty() {
                format!("{} \u{2014} {}", title, artist_name)
            } else {
                title
            };
            let title_artist = truncate(&title_artist, title_w);
            let title_padded = format!("{:<width$}", title_artist, width = title_w);

            let album_padded = if album_name.is_empty() {
                " ".repeat(album_w)
            } else {
                format!("{:<width$}", album_name, width = album_w)
            };

            // Right-align duration
            let dur_padded = format!("{:>width$}", dur, width = dur_w);

            let quality_color = if track.hires_streamable {
                HIRES_BADGE
            } else {
                TEXT_DIM
            };

            let spans = vec![
                Span::styled("\u{2665} ", style.fg(ACCENT)), // ♥
                Span::styled(num, style.fg(TEXT_DIM)),
                Span::styled(title_padded, if is_selected {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    style
                }),
                Span::styled(album_padded, style.fg(TEXT_DIM)),
                Span::styled(dur_padded, style.fg(TEXT_MUTED)),
                Span::styled(
                    format!(" {:<5}", quality),
                    style.fg(quality_color),
                ),
            ];

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items);

    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));

    frame.render_stateful_widget(list, area, &mut list_state);

    // Scrollbar
    if track_count > 0 {
        state.favorites.scrollbar_state = state
            .favorites
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
            &mut state.favorites.scrollbar_state,
        );
    }
}

/// Render the favorite albums list.
fn render_albums(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let favs = &state.favorites;

    if favs.albums.is_empty() && !favs.loading {
        let msg = if !favs.albums_loaded {
            "Press Tab to load albums"
        } else {
            "No favorite albums yet"
        };
        let mid_y = area.y + area.height / 2;
        if mid_y < area.y + area.height {
            let row = Rect::new(area.x, mid_y, area.width, 1);
            let paragraph = Paragraph::new(msg)
                .style(Style::default().fg(TEXT_DIM))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(paragraph, row);
        }
        return;
    }

    let selected_index = favs.selected_index;
    let total_width = area.width as usize;

    // Columns: # (4) | Title (flex) | Artist (30%) | Tracks (8) | Quality (6)
    let num_w: usize = 4;
    let tracks_w: usize = 8;
    let quality_w: usize = 6;
    let remaining = total_width.saturating_sub(num_w + tracks_w + quality_w + 2);
    let artist_w = remaining * 30 / 100;
    let title_w = remaining.saturating_sub(artist_w);

    let items: Vec<ListItem<'_>> = favs
        .albums
        .iter()
        .enumerate()
        .map(|(idx, album)| {
            let is_selected = idx == selected_index;
            let num = format!("{:>3} ", idx + 1);
            let title = truncate(&album.title, title_w);
            let artist = truncate(&album.artist.name, artist_w.saturating_sub(1));
            let track_count = album.tracks_count.map(|c| format!("{:>3} trk", c)).unwrap_or_default();

            let quality = if album.hires_streamable { "Hi-Res" } else { "CD" };

            let style = if is_selected {
                Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let title_padded = format!("{:<width$}", title, width = title_w);
            let artist_padded = format!("{:<width$}", artist, width = artist_w);
            let tracks_padded = format!("{:>width$}", track_count, width = tracks_w);

            let quality_color = if album.hires_streamable { HIRES_BADGE } else { TEXT_DIM };

            let spans = vec![
                Span::styled(num, style.fg(TEXT_DIM)),
                Span::styled(title_padded, if is_selected { style.bold() } else { style }),
                Span::styled(artist_padded, style.fg(TEXT_MUTED)),
                Span::styled(tracks_padded, style.fg(TEXT_DIM)),
                Span::styled(format!(" {:<5}", quality), style.fg(quality_color)),
            ];

            ListItem::new(Line::from(spans))
        })
        .collect();

    let album_count = favs.albums.len();
    let list = List::new(items);
    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));
    frame.render_stateful_widget(list, area, &mut list_state);

    if album_count > 0 {
        state.favorites.scrollbar_state = state
            .favorites
            .scrollbar_state
            .content_length(album_count)
            .position(selected_index);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}"))
            .end_symbol(Some("\u{2193}"));

        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin { vertical: 0, horizontal: 1 }),
            &mut state.favorites.scrollbar_state,
        );
    }
}

/// Render the favorite artists list.
fn render_artists(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let favs = &state.favorites;

    if favs.artists.is_empty() && !favs.loading {
        let msg = if !favs.artists_loaded {
            "Press Tab to load artists"
        } else {
            "No favorite artists yet"
        };
        let mid_y = area.y + area.height / 2;
        if mid_y < area.y + area.height {
            let row = Rect::new(area.x, mid_y, area.width, 1);
            let paragraph = Paragraph::new(msg)
                .style(Style::default().fg(TEXT_DIM))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(paragraph, row);
        }
        return;
    }

    let selected_index = favs.selected_index;
    let total_width = area.width as usize;

    let num_w: usize = 4;
    let albums_w: usize = 12;
    let name_w = total_width.saturating_sub(num_w + albums_w + 2);

    let items: Vec<ListItem<'_>> = favs
        .artists
        .iter()
        .enumerate()
        .map(|(idx, artist)| {
            let is_selected = idx == selected_index;
            let num = format!("{:>3} ", idx + 1);
            let name = truncate(&artist.name, name_w);
            let album_count = artist.albums_count
                .map(|c| format!("{} albums", c))
                .unwrap_or_default();

            let style = if is_selected {
                Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let name_padded = format!("{:<width$}", name, width = name_w);
            let albums_padded = format!("{:>width$}", album_count, width = albums_w);

            let spans = vec![
                Span::styled(num, style.fg(TEXT_DIM)),
                Span::styled(name_padded, if is_selected { style.bold() } else { style }),
                Span::styled(albums_padded, style.fg(TEXT_MUTED)),
            ];

            ListItem::new(Line::from(spans))
        })
        .collect();

    let artist_count = favs.artists.len();
    let list = List::new(items);
    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));
    frame.render_stateful_widget(list, area, &mut list_state);

    if artist_count > 0 {
        state.favorites.scrollbar_state = state
            .favorites
            .scrollbar_state
            .content_length(artist_count)
            .position(selected_index);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}"))
            .end_symbol(Some("\u{2193}"));

        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin { vertical: 0, horizontal: 1 }),
            &mut state.favorites.scrollbar_state,
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
