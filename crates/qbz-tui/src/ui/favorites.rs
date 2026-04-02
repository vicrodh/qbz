//! Favorites view — tab bar, track list, and playback trigger.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, FavoritesTab};
use crate::theme::{ACCENT, BG_SELECTED, DANGER, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

/// Render the full favorites view inside `area`.
pub fn render_favorites(frame: &mut Frame, area: Rect, state: &AppState) {
    // Split vertically: tab bar (1) + results (rest)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar + status
            Constraint::Min(1),   // track list
        ])
        .split(area);

    render_tab_bar(frame, chunks[0], state);
    render_tracks(frame, chunks[1], state);
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
    } else if !favs.tracks.is_empty() {
        spans.push(Span::styled(
            format!("  {} tracks", favs.tracks.len()),
            Style::default().fg(TEXT_MUTED),
        ));
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

/// The favorites track list.
fn render_tracks(frame: &mut Frame, area: Rect, state: &AppState) {
    let favs = &state.favorites;

    if favs.tracks.is_empty() && !favs.loading {
        // Empty state
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

    // Build list items
    let items: Vec<ListItem<'_>> = favs
        .tracks
        .iter()
        .enumerate()
        .map(|(idx, track)| {
            let is_selected = idx == favs.selected_index;

            // Track number / index
            let num = format!("{:>3}. ", idx + 1);

            // Title
            let title = &track.title;

            // Artist
            let artist_name = track
                .performer
                .as_ref()
                .map(|a| a.name.as_str())
                .unwrap_or("Unknown Artist");

            // Album
            let album_name = track
                .album
                .as_ref()
                .map(|a| a.title.as_str())
                .unwrap_or("");

            // Duration
            let dur = format_duration(track.duration);

            // Quality badge
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

            let mut spans = vec![
                Span::styled(num, style.fg(TEXT_DIM)),
                Span::styled(title.to_string(), if is_selected {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    style
                }),
            ];

            // Artist (dimmer)
            if !artist_name.is_empty() {
                spans.push(Span::styled(
                    format!("  {}", artist_name),
                    style.fg(TEXT_MUTED),
                ));
            }

            // Album (even dimmer)
            if !album_name.is_empty() {
                spans.push(Span::styled(
                    format!("  [{}]", album_name),
                    style.fg(TEXT_DIM),
                ));
            }

            // Duration
            spans.push(Span::styled(
                format!("  {}", dur),
                style.fg(TEXT_MUTED),
            ));

            // Quality badge
            let quality_color = if track.hires_streamable {
                HIRES_BADGE
            } else {
                TEXT_DIM
            };
            spans.push(Span::styled(
                format!("  {}", quality),
                style.fg(quality_color),
            ));

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items);

    // Use ListState to enable scroll tracking
    let mut list_state = ListState::default();
    list_state.select(Some(favs.selected_index));

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn format_duration(seconds: u32) -> String {
    let m = seconds / 60;
    let s = seconds % 60;
    format!("{}:{:02}", m, s)
}
