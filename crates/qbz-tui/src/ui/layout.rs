use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::app::{ActiveView, AppState};
use super::now_playing::render_now_playing;
use super::placeholder::render_placeholder;
use super::search::render_search;
use super::sidebar::render_sidebar;

/// Computed layout areas from the last render, used for mouse hit-testing.
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutAreas {
    pub sidebar: Rect,
    pub content: Rect,
    pub now_playing: Rect,
    /// The area where search results are rendered (within content).
    /// Only valid when the active view is Search.
    pub search_results: Rect,
}

/// Top-level render function.
///
/// Splits the terminal into:
/// - Vertical: `[main_area (Min(1)), now_playing (Length(3))]`
/// - Main area horizontal: `[sidebar (Length(sidebar_width)), content (Min(1))]`
///
/// Returns the computed [`LayoutAreas`] for mouse hit-testing.
pub fn render_layout(frame: &mut Frame, state: &AppState) -> LayoutAreas {
    let size = frame.area();

    // Vertical split: main content + now-playing bar at bottom
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(size);

    let main_area = vertical[0];
    let now_playing_area = vertical[1];

    // Horizontal split: sidebar + content
    let sidebar_width = if state.sidebar_expanded { 22 } else { 4 };
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(sidebar_width),
            Constraint::Min(1),
        ])
        .split(main_area);

    let sidebar_area = horizontal[0];
    let content_area = horizontal[1];

    render_sidebar(frame, sidebar_area, state);

    // Compute search results area (only meaningful when in Search view).
    // The search view splits content_area into: input(3) + status(1) + results(rest).
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
    match state.active_view {
        ActiveView::Search => render_search(frame, content_area, state),
        _ => render_placeholder(frame, content_area, state.active_view.label()),
    }

    render_now_playing(frame, now_playing_area, state);

    LayoutAreas {
        sidebar: sidebar_area,
        content: content_area,
        now_playing: now_playing_area,
        search_results: search_results_area,
    }
}
