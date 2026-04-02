//! Artist detail view — header with artist info, top tracks listing.
//!
//! Navigated to via Enter on an artist in search/favorites/library results.
//! Shows the artist's top tracks and supports playing individual tracks
//! or navigating to album detail.

use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
};
use ratatui::Frame;

use crate::app::AppState;
use crate::theme::{ACCENT, BG_SELECTED, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

/// Render the artist detail view inside `area`.
pub fn render_artist_detail(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let artist_state = &state.artist_detail;

    // Loading state
    if artist_state.loading {
        let msg = Paragraph::new("Loading artist...")
            .style(Style::default().fg(ACCENT))
            .alignment(ratatui::layout::Alignment::Center);
        let mid_y = area.y + area.height / 2;
        if mid_y < area.y + area.height {
            frame.render_widget(msg, Rect::new(area.x, mid_y, area.width, 1));
        }
        return;
    }

    // Error state
    if let Some(ref err) = artist_state.error {
        let msg = Paragraph::new(format!("Error: {}", err))
            .style(Style::default().fg(crate::theme::DANGER))
            .alignment(ratatui::layout::Alignment::Center);
        let mid_y = area.y + area.height / 2;
        if mid_y < area.y + area.height {
            frame.render_widget(msg, Rect::new(area.x, mid_y, area.width, 1));
        }
        return;
    }

    let artist = match &artist_state.artist {
        Some(a) => a,
        None => return,
    };

    // Split vertically: header (4 lines) + top tracks (rest)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // artist header
            Constraint::Min(1),   // top tracks listing
        ])
        .split(area);

    render_artist_header(frame, chunks[0], artist);
    render_top_tracks(frame, chunks[1], state);
}

/// Render the artist header: name, category, albums/tracks count.
fn render_artist_header(frame: &mut Frame, area: Rect, artist: &qbz_models::PageArtistResponse) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(TEXT_DIM));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    // Line 1: Artist name (bold, accent)
    let title = Line::from(Span::styled(
        &artist.name.display,
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ));

    // Line 2: Category
    let category_text = artist.artist_category.as_deref().unwrap_or("");
    let category = Line::from(Span::styled(
        category_text,
        Style::default().fg(TEXT_PRIMARY),
    ));

    // Line 3: Top tracks count + releases info
    let mut info_spans: Vec<Span<'_>> = Vec::new();

    if let Some(ref top_tracks) = artist.top_tracks {
        info_spans.push(Span::styled(
            format!("{} top tracks", top_tracks.len()),
            Style::default().fg(TEXT_MUTED),
        ));
    }

    if let Some(ref releases) = artist.releases {
        let total_albums: usize = releases.iter().map(|rg| rg.items.len()).sum();
        if !info_spans.is_empty() {
            info_spans.push(Span::styled(
                " \u{2022} ",
                Style::default().fg(TEXT_DIM),
            ));
        }
        info_spans.push(Span::styled(
            format!("{} releases", total_albums),
            Style::default().fg(TEXT_MUTED),
        ));
    }

    let info_line = Line::from(info_spans);

    let lines = vec![title, category, info_line];
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render the artist's top tracks listing with scrollbar.
fn render_top_tracks(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let artist = match &state.artist_detail.artist {
        Some(a) => a,
        None => return,
    };

    let top_tracks = match &artist.top_tracks {
        Some(tracks) => tracks,
        None => {
            let msg = Paragraph::new("No top tracks")
                .style(Style::default().fg(TEXT_DIM))
                .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(msg, area);
            return;
        }
    };

    if top_tracks.is_empty() {
        let msg = Paragraph::new("No top tracks")
            .style(Style::default().fg(TEXT_DIM))
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(msg, area);
        return;
    }

    // Section header
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // section label
            Constraint::Min(1),   // track list
        ])
        .split(area);

    let header = Paragraph::new(Line::from(Span::styled(
        " Top Tracks",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(header, chunks[0]);

    let selected_index = state.artist_detail.selected_index;
    let track_count = top_tracks.len();
    let total_width = chunks[1].width as usize;

    // Column widths: # (4) | Title (flex) | Album (30%) | Duration (8) | Quality (6)
    let num_w: usize = 4;
    let dur_w: usize = 8;
    let quality_w: usize = 6;
    let remaining = total_width.saturating_sub(num_w + dur_w + quality_w + 2);
    let album_w = remaining * 30 / 100;
    let title_w = remaining.saturating_sub(album_w);

    let items: Vec<ListItem<'_>> = top_tracks
        .iter()
        .enumerate()
        .map(|(idx, track)| {
            let is_selected = idx == selected_index;

            let num = format!("{:>3} ", idx + 1);
            let title = truncate(&track.title, title_w);

            let album_name = track
                .album
                .as_ref()
                .map(|a| truncate(&a.title, album_w.saturating_sub(2)))
                .unwrap_or_default();

            let dur = track.duration.map(format_duration).unwrap_or_default();

            let is_hires = track
                .audio_info
                .as_ref()
                .and_then(|ai| ai.maximum_bit_depth)
                .map(|bd| bd > 16)
                .unwrap_or(false);

            let quality = if is_hires { "Hi-Res" } else { "CD" };

            let style = if is_selected {
                Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let title_padded = format!("{:<width$}", title, width = title_w);
            let album_padded = if album_name.is_empty() {
                " ".repeat(album_w)
            } else {
                format!("{:<width$}", album_name, width = album_w)
            };
            let dur_padded = format!("{:>width$}", dur, width = dur_w);

            let quality_color = if is_hires { HIRES_BADGE } else { TEXT_DIM };

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
                Span::styled(album_padded, style.fg(TEXT_DIM)),
                Span::styled(dur_padded, style.fg(TEXT_MUTED)),
                Span::styled(format!(" {:<5}", quality), style.fg(quality_color)),
            ];

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items);

    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));

    frame.render_stateful_widget(list, chunks[1], &mut list_state);

    // Scrollbar
    if track_count > 0 {
        state.artist_detail.scrollbar_state = state
            .artist_detail
            .scrollbar_state
            .content_length(track_count)
            .position(selected_index);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}"))
            .end_symbol(Some("\u{2193}"));

        frame.render_stateful_widget(
            scrollbar,
            chunks[1].inner(Margin {
                vertical: 0,
                horizontal: 1,
            }),
            &mut state.artist_detail.scrollbar_state,
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
