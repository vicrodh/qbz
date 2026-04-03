//! Discovery view — 3 tabs: Home, Editor's Picks, For You.
//!
//! Each tab shows sectioned content with headers and scrollable lists.
//! - Home: Mix of editorial + personal (all discover sections)
//! - Editor's Picks: Editorial content from Qobuz discover API
//! - For You: Personalized content (favorites, listening history)

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
        DiscoveryTab::Home => render_home_tab(frame, chunks[1], state),
        DiscoveryTab::EditorPicks => render_editor_picks_tab(frame, chunks[1], state),
        DiscoveryTab::ForYou => render_for_you_tab(frame, chunks[1], state),
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
        spans.push(Span::styled("  Loading...", Style::default().fg(ACCENT)));
    } else if let Some(ref err) = disc.error {
        spans.push(Span::styled(format!("  Error: {}", err), Style::default().fg(DANGER)));
    }

    if !state.authenticated {
        spans.push(Span::styled("  [not logged in]", Style::default().fg(DANGER)));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ==================== Shared rendering helpers ====================

/// A section of items to render with a header.
struct Section<'a> {
    title: &'a str,
    items: Vec<SectionItem>,
    /// Whether items should be numbered (default: true).
    numbered: bool,
    /// Maximum items to display (0 = show all).
    max_display: usize,
}

/// A single item within a section.
struct SectionItem {
    title: String,
    subtitle: String,
    detail: String,
    is_hires: bool,
}

/// Build items from DiscoverAlbum list.
fn discover_albums_to_items(albums: &[qbz_models::DiscoverAlbum]) -> Vec<SectionItem> {
    albums.iter().map(|album| {
        let artist = album.artists.first().map(|a| a.name.as_str()).unwrap_or("Unknown");
        let is_hires = album.audio_info.as_ref()
            .and_then(|info| info.maximum_bit_depth)
            .map(|bd| bd > 16)
            .unwrap_or(false);
        SectionItem {
            title: album.title.clone(),
            subtitle: artist.to_string(),
            detail: if is_hires { "Hi-Res".into() } else { "CD".into() },
            is_hires,
        }
    }).collect()
}

/// Build items from Album list.
fn albums_to_items(albums: &[qbz_models::Album]) -> Vec<SectionItem> {
    albums.iter().map(|album| {
        let tracks_str = album.tracks_count.map(|c| format!("{} trk", c)).unwrap_or_default();
        SectionItem {
            title: album.title.clone(),
            subtitle: album.artist.name.clone(),
            detail: if album.hires_streamable {
                format!("Hi-Res  {}", tracks_str)
            } else {
                format!("CD  {}", tracks_str)
            },
            is_hires: album.hires_streamable,
        }
    }).collect()
}

/// Build items from DiscoverPlaylist list.
fn playlists_to_items(playlists: &[qbz_models::DiscoverPlaylist]) -> Vec<SectionItem> {
    playlists.iter().map(|playlist| {
        SectionItem {
            title: playlist.name.clone(),
            subtitle: playlist.owner.name.clone(),
            detail: format!("{} tracks", playlist.tracks_count),
            is_hires: false,
        }
    }).collect()
}

/// Build items from Track list (for Continue Listening).
fn tracks_to_items(tracks: &[qbz_models::Track]) -> Vec<SectionItem> {
    tracks.iter().map(|track| {
        let artist = track.performer.as_ref().map(|a| a.name.as_str()).unwrap_or("Unknown");
        let album_title = track.album.as_ref().map(|a| a.title.as_str()).unwrap_or("");
        let mins = track.duration / 60;
        let secs = track.duration % 60;
        let quality = if track.hires_streamable { "Hi-Res" } else { "CD" };
        SectionItem {
            title: format!("{} \u{2014} {}", track.title, artist),
            subtitle: album_title.to_string(),
            detail: format!("{}:{:02}  {}", mins, secs, quality),
            is_hires: track.hires_streamable,
        }
    }).collect()
}

/// Build items from Artist list.
fn artists_to_items(artists: &[qbz_models::Artist]) -> Vec<SectionItem> {
    artists.iter().map(|artist| {
        let albums_str = artist.albums_count.map(|c| format!("{} albums", c)).unwrap_or_default();
        SectionItem {
            title: artist.name.clone(),
            subtitle: String::new(),
            detail: albums_str,
            is_hires: false,
        }
    }).collect()
}

/// Render a list of sections with headers, navigable with j/k.
/// Returns the number of selectable items rendered.
fn render_sectioned_list(
    frame: &mut Frame,
    area: Rect,
    sections: &[Section<'_>],
    selected_index: usize,
    scrollbar_state: &mut ratatui::widgets::ScrollbarState,
) {
    let total_width = area.width as usize;
    let num_w: usize = 4;
    let detail_w: usize = 14;
    let remaining = total_width.saturating_sub(num_w + detail_w + 2);
    let subtitle_w = remaining * 30 / 100;
    let title_w = remaining.saturating_sub(subtitle_w);

    let mut list_items: Vec<ListItem<'_>> = Vec::new();
    let mut data_index: usize = 0;
    let mut visual_to_data: Vec<Option<usize>> = Vec::new();

    for (sec_idx, section) in sections.iter().enumerate() {
        if section.items.is_empty() {
            continue;
        }

        // Blank line between sections (except first)
        if sec_idx > 0 && !list_items.is_empty() {
            list_items.push(ListItem::new(Line::from("")));
            visual_to_data.push(None);
        }

        // Section header
        list_items.push(ListItem::new(Line::from(Span::styled(
            format!("  {} {}", "\u{25a0}", section.title),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))));
        visual_to_data.push(None);

        // Section items (capped by max_display, 0 = show all)
        let max_items = if section.max_display > 0 { section.max_display } else { section.items.len() };
        for item in section.items.iter().take(max_items) {
            let is_selected = data_index == selected_index;

            let num = if section.numbered {
                format!("{:>3} ", data_index + 1)
            } else {
                "    ".to_string()
            };
            let title_display = truncate(&item.title, title_w);
            let title_padded = format!("{:<width$}", title_display, width = title_w);

            let subtitle_display = if item.subtitle.is_empty() {
                " ".repeat(subtitle_w)
            } else {
                let sub = truncate(&item.subtitle, subtitle_w.saturating_sub(1));
                format!("{:<width$}", sub, width = subtitle_w)
            };

            let detail_padded = format!("{:>width$}", item.detail, width = detail_w);

            let style = if is_selected {
                Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let detail_color = if item.is_hires { HIRES_BADGE } else { TEXT_DIM };

            let spans = vec![
                Span::styled(num, style.fg(TEXT_DIM)),
                Span::styled(title_padded, if is_selected { style.bold() } else { style }),
                Span::styled(subtitle_display, style.fg(TEXT_MUTED)),
                Span::styled(detail_padded, style.fg(detail_color)),
            ];

            list_items.push(ListItem::new(Line::from(spans)));
            visual_to_data.push(Some(data_index));
            data_index += 1;
        }
    }

    if list_items.is_empty() {
        return;
    }

    let list = List::new(list_items);

    // Find the visual index for the selected data index
    let visual_selected = visual_to_data
        .iter()
        .position(|v| *v == Some(selected_index))
        .unwrap_or(0);

    let mut list_state = ListState::default();
    list_state.select(Some(visual_selected));

    frame.render_stateful_widget(list, area, &mut list_state);

    // Scrollbar
    let total_items = data_index;
    if total_items > 0 {
        *scrollbar_state = scrollbar_state
            .content_length(total_items)
            .position(selected_index);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}"))
            .end_symbol(Some("\u{2193}"));

        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin { vertical: 0, horizontal: 1 }),
            scrollbar_state,
        );
    }
}

/// Effective number of displayed items in a section.
fn effective_count(section: &Section<'_>) -> usize {
    if section.max_display > 0 {
        section.items.len().min(section.max_display)
    } else {
        section.items.len()
    }
}

/// Show centered empty/loading message.
fn render_empty_message(frame: &mut Frame, area: Rect, msg: &str) {
    if msg.is_empty() { return; }
    let mid_y = area.y + area.height / 2;
    if mid_y < area.y + area.height {
        let row = Rect::new(area.x, mid_y, area.width, 1);
        let paragraph = Paragraph::new(msg)
            .style(Style::default().fg(TEXT_DIM))
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(paragraph, row);
    }
}

// ==================== Home tab ====================

fn render_home_tab(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let disc = &state.discovery;

    if !disc.loaded && !disc.loading {
        render_empty_message(frame, area, "Press 1 to load Discovery");
        return;
    }

    let sections = vec![
        Section { title: "New Releases", items: discover_albums_to_items(&disc.new_releases), numbered: true, max_display: 8 },
        Section { title: "Popular Albums", items: discover_albums_to_items(&disc.most_streamed), numbered: true, max_display: 8 },
        Section { title: "Essential Discography", items: discover_albums_to_items(&disc.essential_discography), numbered: true, max_display: 8 },
        Section { title: "Qobuzissimes", items: discover_albums_to_items(&disc.qobuzissimes), numbered: true, max_display: 8 },
        Section { title: "Qobuz Playlists", items: playlists_to_items(&disc.qobuz_playlists), numbered: true, max_display: 8 },
    ];

    let total: usize = sections.iter().map(|s| effective_count(s)).sum();
    if total == 0 && !disc.loading {
        render_empty_message(frame, area, if disc.loaded { "No content available" } else { "" });
        return;
    }

    let selected = disc.selected_index;
    let mut scrollbar = disc.scrollbar_state;
    render_sectioned_list(frame, area, &sections, selected, &mut scrollbar);
    state.discovery.scrollbar_state = scrollbar;
}

// ==================== Editor's Picks tab ====================

fn render_editor_picks_tab(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let disc = &state.discovery;

    if !disc.loaded && !disc.loading {
        render_empty_message(frame, area, "Loading editorial content...");
        return;
    }

    let sections = vec![
        Section { title: "New Releases", items: discover_albums_to_items(&disc.new_releases), numbered: true, max_display: 8 },
        Section { title: "Albums of the Week", items: discover_albums_to_items(&disc.editor_picks_discover), numbered: true, max_display: 8 },
        Section { title: "Qobuzissimes", items: discover_albums_to_items(&disc.qobuzissimes), numbered: true, max_display: 8 },
        Section { title: "Press Accolades", items: discover_albums_to_items(&disc.press_awards), numbered: true, max_display: 8 },
        Section { title: "Popular Albums", items: discover_albums_to_items(&disc.most_streamed), numbered: true, max_display: 8 },
        Section { title: "Essential Discography", items: discover_albums_to_items(&disc.essential_discography), numbered: true, max_display: 8 },
        Section { title: "Qobuz Playlists", items: playlists_to_items(&disc.qobuz_playlists), numbered: true, max_display: 8 },
    ];

    let total: usize = sections.iter().map(|s| effective_count(s)).sum();
    if total == 0 && !disc.loading {
        render_empty_message(frame, area, if disc.loaded { "No editorial content" } else { "" });
        return;
    }

    let selected = disc.selected_index;
    let mut scrollbar = disc.scrollbar_state;
    render_sectioned_list(frame, area, &sections, selected, &mut scrollbar);
    state.discovery.scrollbar_state = scrollbar;
}

// ==================== For You tab ====================

fn render_for_you_tab(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let disc = &state.discovery;

    if !disc.for_you_loaded && !disc.loading {
        render_empty_message(frame, area, "Loading personalized content...");
        return;
    }

    let mut sections: Vec<Section<'_>> = Vec::new();

    // Your Mixes (static items — always shown)
    sections.push(Section {
        title: "Your Mixes",
        numbered: false,
        max_display: 0,
        items: vec![
            SectionItem {
                title: "DailyQ".into(),
                subtitle: "Personalized daily mix".into(),
                detail: String::new(),
                is_hires: false,
            },
            SectionItem {
                title: "WeeklyQ".into(),
                subtitle: "Fresh weekly journey".into(),
                detail: String::new(),
                is_hires: false,
            },
            SectionItem {
                title: "FavQ".into(),
                subtitle: "From your personal favorites".into(),
                detail: String::new(),
                is_hires: false,
            },
            SectionItem {
                title: "TopQ".into(),
                subtitle: "From your most-played playlists".into(),
                detail: String::new(),
                is_hires: false,
            },
        ],
    });

    // Continue Listening (favorite tracks)
    if !disc.for_you_tracks.is_empty() {
        sections.push(Section {
            title: "Continue Listening",
            numbered: true,
            max_display: 10,
            items: tracks_to_items(&disc.for_you_tracks),
        });
    }

    // Recently Played (favorite albums as proxy)
    if !disc.for_you_albums.is_empty() {
        sections.push(Section {
            title: "Recently Played",
            numbered: true,
            max_display: 8,
            items: albums_to_items(&disc.for_you_albums),
        });
    }

    // Your Top Artists (favorite artists)
    if !disc.for_you_artists.is_empty() {
        sections.push(Section {
            title: "Your Top Artists",
            numbered: true,
            max_display: 8,
            items: artists_to_items(&disc.for_you_artists),
        });
    }

    let total: usize = sections.iter().map(|s| effective_count(s)).sum();
    if total == 0 {
        render_empty_message(frame, area, "No personalized content yet");
        return;
    }

    let selected = disc.selected_index;
    let mut scrollbar = disc.scrollbar_state;
    render_sectioned_list(frame, area, &sections, selected, &mut scrollbar);
    state.discovery.scrollbar_state = scrollbar;
}

// ==================== Utilities ====================

fn truncate(s: &str, max_chars: usize) -> String {
    if max_chars == 0 { return String::new(); }
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
