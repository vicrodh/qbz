use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

use crate::app::AppState;
use super::now_playing::render_now_playing;
use super::placeholder::render_placeholder;
use super::sidebar::render_sidebar;

/// Top-level render function.
///
/// Splits the terminal into:
/// - Vertical: `[main_area (Min(1)), now_playing (Length(3))]`
/// - Main area horizontal: `[sidebar (Length(sidebar_width)), content (Min(1))]`
pub fn render_layout(frame: &mut Frame, state: &AppState) {
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
    render_placeholder(frame, content_area, state.active_view.label());
    render_now_playing(frame, now_playing_area, state);
}
