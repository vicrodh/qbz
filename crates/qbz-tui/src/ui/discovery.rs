//! Discovery view — tab bar (Home / Editor's Picks / For You), sectioned lists.

use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation};
use ratatui::Frame;

use crate::app::{AppState, DiscoveryTab};
use crate::theme::{ACCENT, BG_SELECTED, DANGER, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

/// Render the full discovery view inside `area`.
pub fn render_discovery(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar
            Constraint::Min(1),   // content
        ])
        .split(area);

    render_tab_bar(frame, chunks[0], state);

    match state.discovery.tab {
        DiscoveryTab::Home => render_home(frame, chunks[1], state),
        DiscoveryTab::EditorPicks => render_album_list(frame, chunks[1], state, AlbumSource::EditorPicks),
        DiscoveryTab::ForYou => render_for_you(frame, chunks[1], state),
    }
}

/// Tab bar with loading/error status.
fn render_tab_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    let disc = &state.discovery;
    let mut spans: Vec<Span<'_>> = Vec::new();

    let tabs = [
        (DiscoveryTab::Home, "Home"),
        (DiscoveryTab::EditorPicks, "Editor's Picks"),
        (DiscoveryTab::ForYou, "For You"),
    ];

    for (tab, label) in &tabs {
        if *tab == disc.tab {
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

    if disc.loading {
        spans.push(Span::styled(
            "  Loading...",
            Style::default().fg(ACCENT),
        ));
    } else if let Some(ref err) = disc.error {
        spans.push(Span::styled(
            format!("  Error: {}", err),
            Style::default().fg(DANGER),
        ));
    }

    if !state.authenticated {
        spans.push(Span::styled(
            "  [not logged in]",
            Style::default().fg(DANGER),
        ));
    }

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

// ==================== Home Tab ====================

/// A section in the Home tab. Albums and playlists use the same flat index space.
struct HomeSection<'a> {
    name: &'a str,
    kind: HomeSectionKind<'a>,
}

enum HomeSectionKind<'a> {
    Albums(&'a Vec<qbz_models::DiscoverAlbum>),
    Playlists(&'a Vec<qbz_models::DiscoverPlaylist>),
}

impl<'a> HomeSectionKind<'a> {
    fn len(&self) -> usize {
        match self {
            HomeSectionKind::Albums(v) => v.len(),
            HomeSectionKind::Playlists(v) => v.len(),
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Home tab: flat list with section headers for all 7 discover sections.
fn render_home(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let disc = &state.discovery;

    let sections: Vec<HomeSection<'_>> = vec![
        HomeSection { name: "New Releases", kind: HomeSectionKind::Albums(&disc.new_releases) },
        HomeSection { name: "Essential Discography", kind: HomeSectionKind::Albums(&disc.essential_discography) },
        HomeSection { name: "Editor's Picks", kind: HomeSectionKind::Albums(&disc.editor_picks_discover) },
        HomeSection { name: "Press Awards", kind: HomeSectionKind::Albums(&disc.press_awards) },
        HomeSection { name: "Most Streamed", kind: HomeSectionKind::Albums(&disc.most_streamed) },
        HomeSection { name: "Qobuzissimes", kind: HomeSectionKind::Albums(&disc.qobuzissimes) },
        HomeSection { name: "Qobuz Playlists", kind: HomeSectionKind::Playlists(&disc.qobuz_playlists) },
    ];

    // Check if all sections are empty
    let total_items: usize = sections.iter().map(|s| s.kind.len()).sum();
    if total_items == 0 && !disc.loading {
        let msg = if !disc.loaded {
            ""
        } else if disc.error.is_some() {
            "Failed to load discovery"
        } else {
            "No items found"
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

    let selected_index = disc.selected_index;
    let total_width = area.width as usize;

    // Build flat list with section headers and items
    let mut items: Vec<ListItem<'_>> = Vec::new();
    let mut flat_index: usize = 0;

    // Column widths for album rows: # (4) | Title - Artist (flex) | Quality (8)
    let num_w: usize = 4;
    let quality_w: usize = 8;
    let album_content_w = total_width.saturating_sub(num_w + quality_w + 4);

    // Column widths for playlist rows: # (4) | Name (flex) | Tracks (10) | Owner (20)
    let playlist_tracks_w: usize = 10;
    let playlist_owner_w: usize = 20;
    let playlist_name_w = total_width.saturating_sub(num_w + playlist_tracks_w + playlist_owner_w + 4);

    for section in &sections {
        if section.kind.is_empty() {
            continue;
        }

        // Section header
        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                format!("  \u{25a0} {}", section.name),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ])));

        match &section.kind {
            HomeSectionKind::Albums(albums) => {
                for album in *albums {
                    let is_selected = flat_index == selected_index;

                    let num = format!("{:>3} ", flat_index + 1);
                    let artist_name = album
                        .artists
                        .first()
                        .map(|a| a.name.as_str())
                        .unwrap_or("Unknown");
                    let title_artist = format!("{} \u{2014} {}", album.title, artist_name);
                    let title_display = truncate(&title_artist, album_content_w);
                    let title_padded = format!("{:<width$}", title_display, width = album_content_w);

                    let is_hires = album
                        .audio_info
                        .as_ref()
                        .and_then(|info| info.maximum_bit_depth)
                        .map(|bd| bd > 16)
                        .unwrap_or(false);
                    let quality = if is_hires { "Hi-Res" } else { "CD" };
                    let quality_color = if is_hires { HIRES_BADGE } else { TEXT_DIM };

                    let style = if is_selected {
                        Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
                    } else {
                        Style::default().fg(TEXT_SECONDARY)
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
                        Span::styled(format!(" {:<7}", quality), style.fg(quality_color)),
                    ];

                    items.push(ListItem::new(Line::from(spans)));
                    flat_index += 1;
                }
            }
            HomeSectionKind::Playlists(playlists) => {
                for playlist in *playlists {
                    let is_selected = flat_index == selected_index;

                    let num = format!("{:>3} ", flat_index + 1);
                    let name_display = truncate(&playlist.name, playlist_name_w);
                    let name_padded = format!("{:<width$}", name_display, width = playlist_name_w);
                    let tracks_str = format!("{:>4} trk", playlist.tracks_count);
                    let tracks_padded = format!("{:>width$}", tracks_str, width = playlist_tracks_w);
                    let owner_display = truncate(&playlist.owner.name, playlist_owner_w.saturating_sub(1));
                    let owner_padded = format!("{:<width$}", owner_display, width = playlist_owner_w);

                    let style = if is_selected {
                        Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
                    } else {
                        Style::default().fg(TEXT_SECONDARY)
                    };

                    let spans = vec![
                        Span::styled(num, style.fg(TEXT_DIM)),
                        Span::styled(
                            name_padded,
                            if is_selected {
                                style.add_modifier(Modifier::BOLD)
                            } else {
                                style
                            },
                        ),
                        Span::styled(tracks_padded, style.fg(TEXT_DIM)),
                        Span::styled(format!(" {}", owner_padded), style.fg(TEXT_MUTED)),
                    ];

                    items.push(ListItem::new(Line::from(spans)));
                    flat_index += 1;
                }
            }
        }

        // Empty line between sections
        items.push(ListItem::new(Line::from("")));
    }

    let list = List::new(items);
    frame.render_widget(list, area);

    // Scrollbar
    let total_selectable = flat_index;
    if total_selectable > 0 {
        state.discovery.scrollbar_state = state
            .discovery
            .scrollbar_state
            .content_length(total_selectable)
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
            &mut state.discovery.scrollbar_state,
        );
    }
}

// ==================== Editor's Picks Tab ====================

/// Which album source to render.
enum AlbumSource {
    EditorPicks,
}

/// Render a simple album list (used by Editor's Picks tab).
fn render_album_list(frame: &mut Frame, area: Rect, state: &mut AppState, source: AlbumSource) {
    let disc = &state.discovery;

    let (albums, loaded, empty_msg): (&Vec<qbz_models::Album>, bool, &str) = match source {
        AlbumSource::EditorPicks => (
            &disc.editor_picks,
            disc.editor_picks_loaded,
            "No editor's picks found",
        ),
    };

    if albums.is_empty() && !disc.loading {
        let msg = if !loaded {
            ""
        } else if disc.error.is_some() {
            "Failed to load albums"
        } else {
            empty_msg
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

    let selected_index = disc.selected_index;
    let total_width = area.width as usize;

    // Columns: # (4) | Title (flex) | Artist (30%) | Tracks (8) | Quality (6)
    let num_w: usize = 4;
    let tracks_w: usize = 8;
    let quality_w: usize = 6;
    let remaining = total_width.saturating_sub(num_w + tracks_w + quality_w + 2);
    let artist_w = remaining * 30 / 100;
    let title_w = remaining.saturating_sub(artist_w);

    let items: Vec<ListItem<'_>> = albums
        .iter()
        .enumerate()
        .map(|(idx, album)| {
            let is_selected = idx == selected_index;
            let num = format!("{:>3} ", idx + 1);
            let title = truncate(&album.title, title_w);
            let artist = truncate(&album.artist.name, artist_w.saturating_sub(1));
            let track_count = album
                .tracks_count
                .map(|c| format!("{:>3} trk", c))
                .unwrap_or_default();

            let quality = if album.hires_streamable {
                "Hi-Res"
            } else {
                "CD"
            };

            let style = if is_selected {
                Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let title_padded = format!("{:<width$}", title, width = title_w);
            let artist_padded = format!("{:<width$}", artist, width = artist_w);
            let tracks_padded = format!("{:>width$}", track_count, width = tracks_w);

            let quality_color = if album.hires_streamable {
                HIRES_BADGE
            } else {
                TEXT_DIM
            };

            let spans = vec![
                Span::styled(num, style.fg(TEXT_DIM)),
                Span::styled(
                    title_padded,
                    if is_selected { style.bold() } else { style },
                ),
                Span::styled(artist_padded, style.fg(TEXT_MUTED)),
                Span::styled(tracks_padded, style.fg(TEXT_DIM)),
                Span::styled(format!(" {:<5}", quality), style.fg(quality_color)),
            ];

            ListItem::new(Line::from(spans))
        })
        .collect();

    let album_count = albums.len();
    let list = List::new(items);
    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));
    frame.render_stateful_widget(list, area, &mut list_state);

    if album_count > 0 {
        state.discovery.scrollbar_state = state
            .discovery
            .scrollbar_state
            .content_length(album_count)
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
            &mut state.discovery.scrollbar_state,
        );
    }
}

// ==================== For You Tab ====================

/// Render the For You tab with Favorite Albums and Top Artists sections.
fn render_for_you(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let disc = &state.discovery;

    let albums = &disc.for_you_albums;
    let artists = &disc.for_you_artists;
    let total_items = albums.len() + artists.len();

    if total_items == 0 && !disc.loading {
        let msg = if !disc.for_you_loaded {
            ""
        } else if disc.error.is_some() {
            "Failed to load favorites"
        } else {
            "No favorites yet"
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

    let selected_index = disc.selected_index;
    let total_width = area.width as usize;

    let mut items: Vec<ListItem<'_>> = Vec::new();
    let mut flat_index: usize = 0;

    // Columns for albums: # (4) | Title (flex) | Artist (30%) | Tracks (8) | Quality (6)
    let num_w: usize = 4;
    let tracks_w: usize = 8;
    let quality_w: usize = 6;
    let remaining = total_width.saturating_sub(num_w + tracks_w + quality_w + 2);
    let artist_w = remaining * 30 / 100;
    let title_w = remaining.saturating_sub(artist_w);

    // Favorite Albums section
    if !albums.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                "  \u{25a0} Favorite Albums",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ])));

        for album in albums {
            let is_selected = flat_index == selected_index;
            let num = format!("{:>3} ", flat_index + 1);
            let title = truncate(&album.title, title_w);
            let artist = truncate(&album.artist.name, artist_w.saturating_sub(1));
            let track_count = album
                .tracks_count
                .map(|c| format!("{:>3} trk", c))
                .unwrap_or_default();

            let quality = if album.hires_streamable {
                "Hi-Res"
            } else {
                "CD"
            };

            let style = if is_selected {
                Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let title_padded = format!("{:<width$}", title, width = title_w);
            let artist_padded = format!("{:<width$}", artist, width = artist_w);
            let tracks_padded = format!("{:>width$}", track_count, width = tracks_w);

            let quality_color = if album.hires_streamable {
                HIRES_BADGE
            } else {
                TEXT_DIM
            };

            let spans = vec![
                Span::styled(num, style.fg(TEXT_DIM)),
                Span::styled(
                    title_padded,
                    if is_selected { style.bold() } else { style },
                ),
                Span::styled(artist_padded, style.fg(TEXT_MUTED)),
                Span::styled(tracks_padded, style.fg(TEXT_DIM)),
                Span::styled(format!(" {:<5}", quality), style.fg(quality_color)),
            ];

            items.push(ListItem::new(Line::from(spans)));
            flat_index += 1;
        }

        items.push(ListItem::new(Line::from("")));
    }

    // Top Artists section
    if !artists.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                "  \u{25a0} Top Artists",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ])));

        for artist in artists {
            let is_selected = flat_index == selected_index;
            let num = format!("{:>3} ", flat_index + 1);
            let name_display = truncate(&artist.name, total_width.saturating_sub(num_w + 2));

            let style = if is_selected {
                Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let spans = vec![
                Span::styled(num, style.fg(TEXT_DIM)),
                Span::styled(
                    name_display,
                    if is_selected {
                        style.add_modifier(Modifier::BOLD)
                    } else {
                        style
                    },
                ),
            ];

            items.push(ListItem::new(Line::from(spans)));
            flat_index += 1;
        }
    }

    let list = List::new(items);
    frame.render_widget(list, area);

    // Scrollbar
    let total_selectable = flat_index;
    if total_selectable > 0 {
        state.discovery.scrollbar_state = state
            .discovery
            .scrollbar_state
            .content_length(total_selectable)
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
            &mut state.discovery.scrollbar_state,
        );
    }
}

// ==================== Helpers ====================

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
