//! Top-level layout — menu bar, 3-column content area, player bar, help bar.
//!
//! ```text
//! +----------------------------------------------------------+
//! | Library . Favorites . Playlists . Search      flac 100%  |  <- menu bar (1 line)
//! +----------------------------------------------------------+
//! | Nav List    | Main Content              | Lyrics          |
//! | (left)      | (center)                  | (right top)     |
//! |             |                           |                 |
//! |             |                           |-----------------|
//! |             |                           | Queue           |
//! |             |                           | (right bottom)  |
//! +----------------------------------------------------------+
//! | [cover]  Title -- Album > Artist                         |
//! | [art  ]  > 33% ━━━━━━━━━───────── 1:11 / 3:32           |
//! | [     ]  flac -- 44.1 kHz -- stereo                      |
//! +----------------------------------------------------------+
//! | Help <?> Quit <Ctrl+Q>                                   |  <- help bar (1 line)
//! +----------------------------------------------------------+
//! ```

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::app::{ActiveView, AppState};
use super::album_detail::render_album_detail;
use super::artist_detail::render_artist_detail;
use super::discovery::render_discovery;
use super::favorites::render_favorites;
use super::help_bar::render_help_bar;
use super::library::render_library;
use super::menu_bar::render_menu_bar;
use super::player_bar::render_player_bar;
use super::queue_panel::render_queue_panel;
use super::playlists::render_playlists;
use super::search::render_search;
use super::search_modal::render_search_modal;
use super::settings::render_settings;
use super::sidebar::render_sidebar;

/// Width of the right panel (queue/lyrics) in columns.
const RIGHT_PANEL_WIDTH: u16 = 30;

/// Width of the left sidebar in columns.
const SIDEBAR_WIDTH: u16 = 20;

/// Computed layout areas from the last render, used for mouse hit-testing.
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutAreas {
    pub sidebar: Rect,
    pub content: Rect,
    pub now_playing: Rect,
    /// The area where search results are rendered (within content).
    /// Only valid when the active view is Search.
    pub search_results: Rect,
    /// The queue panel area. Zero-sized when the panel is hidden.
    pub queue_panel: Rect,
}

/// Top-level render function.
///
/// Splits the terminal into:
/// - Menu bar (1 line top)
/// - Content area: sidebar (left) + main (center) + right panel (optional)
/// - Player bar (4 lines bottom)
/// - Help bar (1 line bottom)
///
/// Returns the computed [`LayoutAreas`] for mouse hit-testing.
pub fn render_layout(frame: &mut Frame, state: &mut AppState) -> LayoutAreas {
    let size = frame.area();

    // Vertical split: menu(1) + content(fill) + player(4) + help(1)
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // menu bar
            Constraint::Min(1),    // content area
            Constraint::Length(4), // player bar
            Constraint::Length(1), // help bar
        ])
        .split(size);

    let menu_area = vertical[0];
    let main_area = vertical[1];
    let player_area = vertical[2];
    let help_area = vertical[3];

    // Render chrome
    render_menu_bar(frame, menu_area, state);
    render_player_bar(frame, player_area, state);
    render_help_bar(frame, help_area, state);

    // Horizontal split: sidebar + content [+ right panel]
    let (sidebar_area, content_area, queue_panel_area) = if state.show_queue_panel {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(SIDEBAR_WIDTH),
                Constraint::Min(1),
                Constraint::Length(RIGHT_PANEL_WIDTH),
            ])
            .split(main_area);
        (horizontal[0], horizontal[1], horizontal[2])
    } else {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(SIDEBAR_WIDTH),
                Constraint::Min(1),
            ])
            .split(main_area);
        (horizontal[0], horizontal[1], Rect::default())
    };

    render_sidebar(frame, sidebar_area, state);

    // Compute search results area (only meaningful when in Search view).
    let search_results_area = if state.active_view == ActiveView::Search {
        let search_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(content_area);
        search_chunks[2]
    } else {
        Rect::default()
    };

    // Render view-specific content
    let active = state.active_view;
    match active {
        ActiveView::Discovery => render_discovery(frame, content_area, state),
        ActiveView::Search => render_search(frame, content_area, state),
        ActiveView::Favorites => render_favorites(frame, content_area, state),
        ActiveView::Library => render_library(frame, content_area, state),
        ActiveView::Album => render_album_detail(frame, content_area, state),
        ActiveView::Artist => render_artist_detail(frame, content_area, state),
        ActiveView::Settings => render_settings(frame, content_area, state),
        ActiveView::Playlists => render_playlists(frame, content_area, state),
    }

    // Render right panel if visible
    if state.show_queue_panel && queue_panel_area.width > 0 {
        render_queue_panel(frame, queue_panel_area, state);
    }

    // Render search modal overlay if active
    if state.show_search_modal {
        render_search_modal(frame, state);
    }

    LayoutAreas {
        sidebar: sidebar_area,
        content: content_area,
        now_playing: player_area,
        search_results: search_results_area,
        queue_panel: queue_panel_area,
    }
}
