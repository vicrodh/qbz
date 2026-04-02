//! Top menu bar — ratatui Tabs widget with dot divider, status info on right.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Tabs};
use ratatui::Frame;

use crate::app::{ActiveView, AppState};
use crate::theme::{ACCENT, BG_SECONDARY, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY};

/// Ordered list of tab views matching the Tabs widget indices and number key accelerators (1-6).
const TAB_VIEWS: &[ActiveView] = &[
    ActiveView::Discovery,
    ActiveView::Favorites,
    ActiveView::Library,
    ActiveView::Playlists,
    ActiveView::Search,
    ActiveView::Settings,
];

/// Tab labels displayed in the Tabs widget (with number prefix accelerators).
const TAB_LABELS: &[&str] = &[
    "1:Discovery",
    "2:Favorites",
    "3:Library",
    "4:Playlists",
    "5:Search",
    "6:Settings",
];

/// Render the 1-line top menu bar using ratatui's Tabs widget.
pub fn render_menu_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    // Split horizontally: tabs (70%) | status info (30%)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(70),
            Constraint::Percentage(30),
        ])
        .split(area);

    // Determine which tab is selected
    let selected = TAB_VIEWS
        .iter()
        .position(|view| *view == state.active_view)
        .unwrap_or(0);

    // Build tab labels with accent-colored number prefixes
    let tab_lines: Vec<Line> = TAB_LABELS
        .iter()
        .enumerate()
        .map(|(idx, label)| {
            // Split "1:Discovery" into number prefix "1:" and name "Discovery"
            let (num_prefix, name) = label.split_once(':').unwrap_or(("", label));
            let is_selected = idx == selected;
            if is_selected {
                // Selected tab: both number and name in accent + bold
                Line::from(vec![
                    Span::styled(
                        format!("{}:", num_prefix),
                        Style::default()
                            .fg(ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        name.to_string(),
                        Style::default()
                            .fg(ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                ])
            } else {
                // Unselected tab: number in accent, name in muted
                Line::from(vec![
                    Span::styled(
                        format!("{}:", num_prefix),
                        Style::default().fg(ACCENT),
                    ),
                    Span::styled(
                        name.to_string(),
                        Style::default().fg(TEXT_MUTED),
                    ),
                ])
            }
        })
        .collect();

    // Render Tabs widget with dot divider (Jellyfin-TUI pattern)
    let tabs = Tabs::new(tab_lines)
        .style(Style::default().bg(BG_SECONDARY))
        .select(selected)
        .divider("\u{2022}") // bullet divider
        .padding(" ", " ");

    frame.render_widget(tabs, chunks[0]);

    // Right side: status info
    let right_spans = build_right_info(state);
    if !right_spans.is_empty() {
        let line = Line::from(right_spans);
        let paragraph = Paragraph::new(line)
            .style(Style::default().bg(BG_SECONDARY))
            .alignment(ratatui::layout::Alignment::Right);
        frame.render_widget(paragraph, chunks[1]);
    } else {
        // Fill with background color
        let paragraph = Paragraph::new("")
            .style(Style::default().bg(BG_SECONDARY));
        frame.render_widget(paragraph, chunks[1]);
    }
}

/// Build the right-side status info spans.
fn build_right_info(state: &AppState) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // Quality badge
    if let Some(ref quality) = state.current_track_quality {
        spans.push(Span::styled(
            quality.clone(),
            Style::default().fg(HIRES_BADGE),
        ));
    }

    // Volume percentage
    let vol_pct = (state.volume * 100.0).round() as u32;
    if !spans.is_empty() {
        spans.push(Span::styled(
            " \u{2022} ",
            Style::default().fg(TEXT_DIM),
        ));
    }
    spans.push(Span::styled(
        format!("{}%", vol_pct),
        Style::default().fg(TEXT_PRIMARY),
    ));

    // Auth indicator
    if !state.authenticated {
        spans.push(Span::styled(
            " \u{2022} ",
            Style::default().fg(TEXT_DIM),
        ));
        spans.push(Span::styled(
            "offline",
            Style::default().fg(TEXT_MUTED),
        ));
    }

    spans.push(Span::styled(" ", Style::default()));

    spans
}
