//! Top menu bar — navigation tabs with accelerator underlines, codec info on right.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{ActiveView, AppState};
use crate::theme::{ACCENT, BG_SECONDARY, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY};

/// Menu items: (view, label, accelerator_key position).
/// The accelerator position is the index of the underlined character in the label.
const MENU_ITEMS: &[(ActiveView, &str, usize)] = &[
    (ActiveView::Library, "Library", 0),     // L
    (ActiveView::Favorites, "Favorites", 0), // F
    (ActiveView::Playlists, "Playlists", 0), // P
    (ActiveView::Search, "Search", 0),       // S
    (ActiveView::Settings, "Settings", 2),   // t
];

/// Render the 1-line top menu bar.
pub fn render_menu_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let mut spans: Vec<Span<'_>> = Vec::new();

    // Left side: navigation tabs with dot separators
    for (idx, (view, label, accel_pos)) in MENU_ITEMS.iter().enumerate() {
        let is_active = *view == state.active_view;

        // Dot separator (skip before first item)
        if idx > 0 {
            spans.push(Span::styled(
                " \u{00b7} ",
                Style::default().fg(TEXT_DIM),
            ));
        }

        // Render the label with the accelerator character underlined
        let chars: Vec<char> = label.chars().collect();
        for (ci, ch) in chars.iter().enumerate() {
            let mut style = if is_active {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(TEXT_MUTED)
            };

            if ci == *accel_pos {
                style = style.add_modifier(Modifier::UNDERLINED);
            }

            if is_active {
                style = style.add_modifier(Modifier::BOLD);
            }

            spans.push(Span::styled(ch.to_string(), style));
        }
    }

    // Right side: codec/quality info
    let right_info = build_right_info(state);
    if !right_info.is_empty() {
        // Calculate how much space we've used on the left
        let left_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let right_width: usize = right_info.iter().map(|s| s.content.chars().count()).sum();
        let total_width = area.width as usize;

        // Fill the gap with spaces
        let gap = total_width.saturating_sub(left_width + right_width + 1);
        if gap > 0 {
            spans.push(Span::raw(" ".repeat(gap)));
        }
        spans.extend(right_info);
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line)
        .style(Style::default().bg(BG_SECONDARY));
    frame.render_widget(paragraph, area);
}

/// Build the right-side codec/quality info spans.
fn build_right_info(state: &AppState) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    if let Some(ref quality) = state.current_track_quality {
        spans.push(Span::styled(
            quality.clone(),
            Style::default().fg(HIRES_BADGE),
        ));
    }

    // Volume percentage
    let vol_pct = (state.volume * 100.0).round() as u32;
    if !spans.is_empty() {
        spans.push(Span::styled(" ", Style::default().fg(TEXT_DIM)));
    }
    spans.push(Span::styled(
        format!("{}%", vol_pct),
        Style::default().fg(TEXT_PRIMARY),
    ));

    spans
}
