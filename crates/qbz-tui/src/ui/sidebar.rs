//! Left panel — scrollable navigation list that stays visible as the left column.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{ActiveView, AppState};
use crate::theme::{ACCENT, BG_PRIMARY, BG_SELECTED, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY};

/// All navigation entries in display order.
pub const NAV_ITEMS: &[(ActiveView, &str)] = &[
    (ActiveView::Discovery, "Discovery"),
    (ActiveView::Favorites, "Favorites"),
    (ActiveView::Library, "Library"),
    (ActiveView::Playlists, "Playlists"),
    (ActiveView::Search, "Search"),
    (ActiveView::Settings, "Settings"),
];

/// Render the left navigation panel.
pub fn render_sidebar(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(TEXT_DIM))
        .style(Style::default().bg(BG_PRIMARY));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width < 4 {
        return;
    }

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Header
    lines.push(Line::from(Span::styled(
        " QBZ",
        Style::default()
            .fg(ACCENT)
            .add_modifier(Modifier::BOLD),
    )));

    // Separator
    let sep_width = (inner.width as usize).saturating_sub(1);
    let sep = "\u{2500}".repeat(sep_width);
    lines.push(Line::from(Span::styled(
        format!(" {}", sep),
        Style::default().fg(TEXT_DIM),
    )));

    // Navigation items
    for (view, label) in NAV_ITEMS {
        let is_active = *view == state.active_view;

        let style = if is_active {
            Style::default()
                .fg(ACCENT)
                .bg(BG_SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_PRIMARY)
        };

        let marker = if is_active { "\u{25b8}" } else { " " };
        lines.push(Line::from(Span::styled(
            format!(" {} {}", marker, label),
            style,
        )));
    }

    // Auth status at bottom (if space allows)
    let nav_lines = lines.len();
    let remaining = (inner.height as usize).saturating_sub(nav_lines + 1);
    if remaining > 0 {
        // Push empty lines to position auth info at bottom
        for _ in 0..remaining {
            lines.push(Line::from(""));
        }
    }

    // Auth indicator
    if let Some(ref email) = state.auth_email {
        // Truncate email to fit
        let max_w = inner.width as usize - 2;
        let display = if email.len() > max_w {
            format!(" {}...", &email[..max_w.saturating_sub(3)])
        } else {
            format!(" {}", email)
        };
        lines.push(Line::from(Span::styled(
            display,
            Style::default().fg(TEXT_MUTED),
        )));
    } else if !state.authenticated {
        lines.push(Line::from(Span::styled(
            " Not logged in",
            Style::default().fg(TEXT_DIM),
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}
