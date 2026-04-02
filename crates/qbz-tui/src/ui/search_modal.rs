//! Search modal popup — centered overlay triggered by `/` from any view.
//! Follows Jellyfin-TUI popup pattern: Clear widget to blank background,
//! Block::bordered() with title, input at top, results below.

use ratatui::layout::{Constraint, Direction, Flex, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation,
};
use ratatui::Frame;

use crate::app::{AppState, InputMode, SearchTab};
use crate::theme::{
    ACCENT, BG_SECONDARY, BG_SELECTED, DANGER, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY,
    TEXT_SECONDARY,
};

/// Compute a centered popup area (Jellyfin-TUI popup_area pattern).
fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)])
        .flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)])
        .flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

/// Render the search modal popup as a centered overlay.
pub fn render_search_modal(frame: &mut Frame, state: &mut AppState) {
    let area = frame.area();

    // 70% width, 60% height (centered)
    let popup = popup_area(area, 70, 60);

    // Clear the background area
    frame.render_widget(Clear, popup);

    // Outer block with border and title
    let is_editing = state.input_mode == InputMode::TextInput;
    let border_color = if is_editing { ACCENT } else { TEXT_MUTED };

    let block = Block::bordered()
        .title(Line::from(vec![
            Span::styled(" Search ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        ]))
        .title_bottom(Line::from(vec![
            Span::styled(
                if is_editing {
                    " Enter: search | Tab: switch tab | Esc: close "
                } else {
                    " /: type | Tab: tab | j/k: nav | Enter: play | a: queue | Esc: close "
                },
                Style::default().fg(TEXT_MUTED),
            ),
        ]))
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(BG_SECONDARY));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.height < 4 || inner.width < 10 {
        return;
    }

    // Split inner area: input (1) + status (1) + gap (1) + results (rest)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // search input
            Constraint::Length(1), // status / tab bar
            Constraint::Length(1), // gap
            Constraint::Min(1),   // results list
        ])
        .split(inner);

    render_input(frame, chunks[0], state);
    render_status(frame, chunks[1], state);
    render_results(frame, chunks[3], state);
}

/// The search input line with cursor.
fn render_input(frame: &mut Frame, area: Rect, state: &AppState) {
    let is_editing = state.input_mode == InputMode::TextInput;
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
                Span::styled(" > ", Style::default().fg(ACCENT)),
                Span::styled(before, Style::default().fg(TEXT_PRIMARY)),
                Span::styled(
                    cursor_char.to_string(),
                    Style::default().fg(TEXT_PRIMARY).bg(ACCENT),
                ),
                Span::styled(after, Style::default().fg(TEXT_PRIMARY)),
            ]);
            frame.render_widget(Paragraph::new(line), area);
        } else {
            let line = Line::from(vec![
                Span::styled(" > ", Style::default().fg(ACCENT)),
                Span::styled(before, Style::default().fg(TEXT_PRIMARY)),
                Span::styled(" ", Style::default().bg(ACCENT)),
            ]);
            frame.render_widget(Paragraph::new(line), area);
        }
    } else if query.is_empty() {
        let line = Line::from(vec![
            Span::styled(" > ", Style::default().fg(TEXT_DIM)),
            Span::styled("Type to search...", Style::default().fg(TEXT_DIM)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    } else {
        let line = Line::from(vec![
            Span::styled(" > ", Style::default().fg(ACCENT)),
            Span::styled(query.as_str(), Style::default().fg(TEXT_PRIMARY)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }
}

/// Tab bar and result count.
fn render_status(frame: &mut Frame, area: Rect, state: &AppState) {
    let search = &state.search;
    let mut spans: Vec<Span<'_>> = Vec::new();

    spans.push(Span::styled(" ", Style::default()));

    // Tab indicators
    let tabs = [
        (SearchTab::Tracks, "Tracks"),
        (SearchTab::Albums, "Albums"),
        (SearchTab::Artists, "Artists"),
    ];

    for (tab, label) in &tabs {
        if *tab == search.tab {
            spans.push(Span::styled(
                format!("[{}]", label),
                Style::default().fg(ACCENT).bold(),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {} ", label),
                Style::default().fg(TEXT_DIM),
            ));
        }
        spans.push(Span::styled(" ", Style::default()));
    }

    // Loading / result count
    if search.loading {
        spans.push(Span::styled(
            " Searching...",
            Style::default().fg(ACCENT),
        ));
    } else if let Some(ref err) = search.error {
        spans.push(Span::styled(
            format!(" Error: {}", err),
            Style::default().fg(DANGER),
        ));
    } else if search.total_results > 0 {
        spans.push(Span::styled(
            format!(" {} results", search.total_results),
            Style::default().fg(TEXT_MUTED),
        ));
    }

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

/// The results list with scrollbar — dispatches based on active tab.
fn render_results(frame: &mut Frame, area: Rect, state: &mut AppState) {
    match state.search.tab {
        SearchTab::Tracks => render_track_results(frame, area, state),
        SearchTab::Albums => render_album_results(frame, area, state),
        SearchTab::Artists => render_artist_results(frame, area, state),
    }
}

/// Check if the active tab has results and render empty state if not.
fn is_empty_state(frame: &mut Frame, area: Rect, state: &AppState) -> bool {
    let search = &state.search;
    let has_items = match search.tab {
        SearchTab::Tracks => !search.tracks.is_empty(),
        SearchTab::Albums => !search.albums.is_empty(),
        SearchTab::Artists => !search.artists.is_empty(),
    };

    if has_items || search.loading {
        return false;
    }

    let msg = if search.query.is_empty() {
        "Press / to start typing"
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
    true
}

fn render_track_results(frame: &mut Frame, area: Rect, state: &mut AppState) {
    if is_empty_state(frame, area, &*state) { return; }

    let selected_index = state.search.selected_index;
    let item_count = state.search.tracks.len();

    // Collect data to avoid borrow conflict
    let rows: Vec<(String, String, String, String, bool)> = state.search.tracks
        .iter()
        .map(|track| {
            let artist = track.performer.as_ref().map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".into());
            (
                track.title.clone(),
                artist,
                format_duration(track.duration),
                if track.hires_streamable { "HR".into() } else { String::new() },
                track.hires_streamable,
            )
        })
        .collect();

    let items: Vec<ListItem<'_>> = rows.iter().enumerate().map(|(idx, (title, artist, dur, quality, is_hires))| {
        let is_selected = idx == selected_index;
        let num = format!(" {:>3}  ", idx + 1);
        let style = if is_selected { Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED) } else { Style::default().fg(TEXT_SECONDARY) };
        let mut spans = vec![
            Span::styled(num, style.fg(TEXT_DIM)),
            Span::styled(title.as_str(), if is_selected { style.bold() } else { style }),
            Span::styled(format!("  {}", artist), style.fg(TEXT_MUTED)),
            Span::styled(format!("  {}", dur), style.fg(TEXT_MUTED)),
        ];
        if !quality.is_empty() {
            spans.push(Span::styled(format!(" {}", quality), style.fg(if *is_hires { HIRES_BADGE } else { TEXT_DIM })));
        }
        ListItem::new(Line::from(spans))
    }).collect();

    render_list_with_scrollbar(frame, area, state, items, item_count, selected_index);
}

fn render_album_results(frame: &mut Frame, area: Rect, state: &mut AppState) {
    if is_empty_state(frame, area, &*state) { return; }

    let selected_index = state.search.selected_index;
    let item_count = state.search.albums.len();

    let rows: Vec<(String, String, String, bool)> = state.search.albums
        .iter()
        .map(|album| {
            let tracks_str = album.tracks_count.map(|c| format!("{}trk", c)).unwrap_or_default();
            (album.title.clone(), album.artist.name.clone(), tracks_str, album.hires_streamable)
        })
        .collect();

    let items: Vec<ListItem<'_>> = rows.iter().enumerate().map(|(idx, (title, artist, tracks_str, is_hires))| {
        let is_selected = idx == selected_index;
        let num = format!(" {:>3}  ", idx + 1);
        let style = if is_selected { Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED) } else { Style::default().fg(TEXT_SECONDARY) };
        let mut spans = vec![
            Span::styled(num, style.fg(TEXT_DIM)),
            Span::styled(title.as_str(), if is_selected { style.bold() } else { style }),
            Span::styled(format!("  {}", artist), style.fg(TEXT_MUTED)),
            Span::styled(format!("  {}", tracks_str), style.fg(TEXT_DIM)),
        ];
        if *is_hires {
            spans.push(Span::styled(" HR", style.fg(HIRES_BADGE)));
        }
        ListItem::new(Line::from(spans))
    }).collect();

    render_list_with_scrollbar(frame, area, state, items, item_count, selected_index);
}

fn render_artist_results(frame: &mut Frame, area: Rect, state: &mut AppState) {
    if is_empty_state(frame, area, &*state) { return; }

    let selected_index = state.search.selected_index;
    let item_count = state.search.artists.len();

    let rows: Vec<(String, String)> = state.search.artists
        .iter()
        .map(|artist| {
            let albums_str = artist.albums_count.map(|c| format!("{} albums", c)).unwrap_or_default();
            (artist.name.clone(), albums_str)
        })
        .collect();

    let items: Vec<ListItem<'_>> = rows.iter().enumerate().map(|(idx, (name, albums_str))| {
        let is_selected = idx == selected_index;
        let num = format!(" {:>3}  ", idx + 1);
        let style = if is_selected { Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED) } else { Style::default().fg(TEXT_SECONDARY) };
        let spans = vec![
            Span::styled(num, style.fg(TEXT_DIM)),
            Span::styled(name.as_str(), if is_selected { style.bold() } else { style }),
            Span::styled(format!("  {}", albums_str), style.fg(TEXT_MUTED)),
        ];
        ListItem::new(Line::from(spans))
    }).collect();

    render_list_with_scrollbar(frame, area, state, items, item_count, selected_index);
}

fn render_list_with_scrollbar(
    frame: &mut Frame,
    area: Rect,
    state: &mut AppState,
    items: Vec<ListItem<'_>>,
    item_count: usize,
    selected_index: usize,
) {
    let list = List::new(items).highlight_symbol(">> ");
    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));
    frame.render_stateful_widget(list, area, &mut list_state);

    if item_count > 0 {
        state.search.scrollbar_state = state
            .search
            .scrollbar_state
            .content_length(item_count)
            .position(selected_index);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}"))
            .end_symbol(Some("\u{2193}"));

        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin { vertical: 0, horizontal: 0 }),
            &mut state.search.scrollbar_state,
        );
    }
}

fn format_duration(seconds: u32) -> String {
    let m = seconds / 60;
    let s = seconds % 60;
    format!("{}:{:02}", m, s)
}
