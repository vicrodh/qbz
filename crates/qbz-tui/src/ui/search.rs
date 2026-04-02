//! Search view — text input, results list, and playback trigger.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, InputMode, SearchTab};
use crate::theme::{ACCENT, BG_SELECTED, DANGER, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

/// Render the full search view inside `area`.
pub fn render_search(frame: &mut Frame, area: Rect, state: &AppState) {
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
    render_results(frame, chunks[2], state);
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
            " Search (i or / to type) "
        });

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build the query text with a visible cursor
    let query = &state.search.query;
    let cursor = state.search.cursor;

    if is_editing {
        // Cursor position is a byte offset maintained by the input handler.
        let clamped = cursor.min(query.len());
        let before = &query[..clamped];

        if clamped < query.len() {
            // There is a character under the cursor — highlight it.
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
            // Cursor is at the end — show a trailing block.
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

    // Tab indicators
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
    } else if !search.tracks.is_empty() {
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

/// The results list (tracks only for v1).
fn render_results(frame: &mut Frame, area: Rect, state: &AppState) {
    let search = &state.search;

    if search.tracks.is_empty() && !search.loading {
        // Empty state
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

    // Build list items
    let items: Vec<ListItem<'_>> = search
        .tracks
        .iter()
        .enumerate()
        .map(|(idx, track)| {
            let is_selected = idx == search.selected_index;

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

            // Album (even dimmer, only if space allows)
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
    list_state.select(Some(search.selected_index));

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn format_duration(seconds: u32) -> String {
    let m = seconds / 60;
    let s = seconds % 60;
    format!("{}:{:02}", m, s)
}
