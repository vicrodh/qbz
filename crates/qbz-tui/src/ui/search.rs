//! Search view — text input, results list with scrollbar, and playback trigger.
//! Also used as the non-modal search view when navigating to the Search tab.

use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
};
use ratatui::Frame;

use crate::app::{AppState, InputMode, SearchTab};
use crate::theme::{ACCENT, BG_SELECTED, DANGER, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

/// Render the full search view inside `area`.
pub fn render_search(frame: &mut Frame, area: Rect, state: &mut AppState) {
    // Split vertically: search bar (3) + status line (1) + results (rest)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // search input
            Constraint::Length(1), // status / tab bar
            Constraint::Min(1),   // results list
        ])
        .split(area);

    render_search_input(frame, chunks[0], state);
    render_status_bar(frame, chunks[1], state);

    match state.search.tab {
        SearchTab::Tracks => render_results(frame, chunks[2], state),
        SearchTab::Albums => render_album_results(frame, chunks[2], state),
        SearchTab::Artists => render_artist_results(frame, chunks[2], state),
    }
}

/// The search input box with cursor.
fn render_search_input(frame: &mut Frame, area: Rect, state: &AppState) {
    let is_editing = state.input_mode == InputMode::TextInput;

    let border_color = if is_editing { ACCENT } else { TEXT_DIM };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(if is_editing {
            " Search (Enter to search, Esc to cancel) "
        } else {
            " Search (/ to type) "
        });

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build the query text with a visible cursor
    let query = &state.search.query;
    let cursor = state.search.cursor;

    if is_editing {
        let clamped = cursor.min(query.len());
        let before = &query[..clamped];

        if clamped < query.len() {
            let rest = &query[clamped..];
            let cursor_char = rest.chars().next().unwrap();
            let char_end = clamped + cursor_char.len_utf8();
            let after = &query[char_end..];

            let line = Line::from(vec![
                Span::styled(before, Style::default().fg(TEXT_PRIMARY)),
                Span::styled(
                    cursor_char.to_string(),
                    Style::default().fg(TEXT_PRIMARY).bg(ACCENT),
                ),
                Span::styled(after, Style::default().fg(TEXT_PRIMARY)),
            ]);
            frame.render_widget(Paragraph::new(line), inner);
        } else {
            let line = Line::from(vec![
                Span::styled(before, Style::default().fg(TEXT_PRIMARY)),
                Span::styled(" ", Style::default().bg(ACCENT)),
            ]);
            frame.render_widget(Paragraph::new(line), inner);
        }
    } else if query.is_empty() {
        let placeholder = Paragraph::new("Type a search query...")
            .style(Style::default().fg(TEXT_DIM));
        frame.render_widget(placeholder, inner);
    } else {
        let line = Paragraph::new(query.as_str())
            .style(Style::default().fg(TEXT_PRIMARY));
        frame.render_widget(line, inner);
    }
}

/// Tab bar and result count.
fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    let search = &state.search;

    let mut spans: Vec<Span<'_>> = Vec::new();

    let tabs = [
        (SearchTab::Tracks, "Tracks"),
        (SearchTab::Albums, "Albums"),
        (SearchTab::Artists, "Artists"),
    ];

    for (tab, label) in &tabs {
        if *tab == search.tab {
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

    // Loading / result count
    if search.loading {
        spans.push(Span::styled(
            "  Searching...",
            Style::default().fg(ACCENT),
        ));
    } else if let Some(ref err) = search.error {
        spans.push(Span::styled(
            format!("  Error: {}", err),
            Style::default().fg(DANGER),
        ));
    } else if search.total_results > 0 {
        spans.push(Span::styled(
            format!("  {} results", search.total_results),
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

/// The results list with Jellyfin-TUI-style track formatting and scrollbar.
///
/// Column layout: # (4) | Title (flex) | Album (30%) | Duration (8)
fn render_results(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let search = &state.search;

    if search.tracks.is_empty() && !search.loading {
        let msg = if search.query.is_empty() {
            "Enter a search query to find tracks"
        } else if search.error.is_some() {
            "Search failed"
        } else if search.total_results == 0 && !search.query.is_empty() {
            "No results found"
        } else {
            ""
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

    let selected_index = search.selected_index;
    let track_count = search.tracks.len();
    let total_width = area.width as usize;

    // Column widths following Jellyfin-TUI pattern
    let num_w: usize = 4;      // "#"
    let dur_w: usize = 8;      // "  M:SS"
    let quality_w: usize = 6;  // " Hi-Res" or " CD"
    let remaining = total_width.saturating_sub(num_w + dur_w + quality_w + 2);
    let album_w = remaining * 30 / 100;
    let title_w = remaining.saturating_sub(album_w);

    let items: Vec<ListItem<'_>> = search
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

            // Build formatted row: # | Title - Artist | Album | Duration | Quality
            let title_artist = if !artist_name.is_empty() {
                format!("{} \u{2014} {}", title, artist_name)
            } else {
                title
            };
            let title_artist = truncate(&title_artist, title_w);

            // Pad title to fill column
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
        state.search.scrollbar_state = state
            .search
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
            &mut state.search.scrollbar_state,
        );
    }
}

/// Render album search results.
fn render_album_results(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let search = &state.search;

    if search.albums.is_empty() && !search.loading {
        let msg = if search.query.is_empty() {
            "Enter a search query to find albums"
        } else if search.error.is_some() {
            "Search failed"
        } else if search.total_results == 0 && !search.query.is_empty() {
            "No albums found"
        } else {
            ""
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

    let selected_index = search.selected_index;
    let album_count = search.albums.len();
    let total_width = area.width as usize;

    let num_w: usize = 4;
    let tracks_w: usize = 8;
    let quality_w: usize = 6;
    let remaining = total_width.saturating_sub(num_w + tracks_w + quality_w + 2);
    let artist_w = remaining * 30 / 100;
    let title_w = remaining.saturating_sub(artist_w);

    let items: Vec<ListItem<'_>> = search
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
                Span::styled(title_padded, if is_selected { style.add_modifier(Modifier::BOLD) } else { style }),
                Span::styled(artist_padded, style.fg(TEXT_MUTED)),
                Span::styled(tracks_padded, style.fg(TEXT_DIM)),
                Span::styled(format!(" {:<5}", quality), style.fg(quality_color)),
            ];

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items);
    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));
    frame.render_stateful_widget(list, area, &mut list_state);

    if album_count > 0 {
        state.search.scrollbar_state = state
            .search
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
            &mut state.search.scrollbar_state,
        );
    }
}

/// Render artist search results.
fn render_artist_results(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let search = &state.search;

    if search.artists.is_empty() && !search.loading {
        let msg = if search.query.is_empty() {
            "Enter a search query to find artists"
        } else if search.error.is_some() {
            "Search failed"
        } else if search.total_results == 0 && !search.query.is_empty() {
            "No artists found"
        } else {
            ""
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

    let selected_index = search.selected_index;
    let artist_count = search.artists.len();
    let total_width = area.width as usize;

    let num_w: usize = 4;
    let albums_w: usize = 12;
    let name_w = total_width.saturating_sub(num_w + albums_w + 2);

    let items: Vec<ListItem<'_>> = search
        .artists
        .iter()
        .enumerate()
        .map(|(idx, artist)| {
            let is_selected = idx == selected_index;
            let num = format!("{:>3} ", idx + 1);
            let name = truncate(&artist.name, name_w);
            let album_count_str = artist.albums_count
                .map(|c| format!("{} albums", c))
                .unwrap_or_default();

            let style = if is_selected {
                Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let name_padded = format!("{:<width$}", name, width = name_w);
            let albums_padded = format!("{:>width$}", album_count_str, width = albums_w);

            let spans = vec![
                Span::styled(num, style.fg(TEXT_DIM)),
                Span::styled(name_padded, if is_selected { style.add_modifier(Modifier::BOLD) } else { style }),
                Span::styled(albums_padded, style.fg(TEXT_MUTED)),
            ];

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items);
    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));
    frame.render_stateful_widget(list, area, &mut list_state);

    if artist_count > 0 {
        state.search.scrollbar_state = state
            .search
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
            &mut state.search.scrollbar_state,
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
