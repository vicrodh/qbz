use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{ActiveView, AppState};
use crate::theme::{ACCENT, BG_PRIMARY, BG_SELECTED, TEXT_DIM, TEXT_PRIMARY, TEXT_SECONDARY};

/// All navigation entries in display order.
const NAV_ITEMS: &[(ActiveView, &str, &str)] = &[
    (ActiveView::Home, "Home", "H"),
    (ActiveView::Favorites, "Favorites", "F"),
    (ActiveView::Library, "Library", "L"),
    (ActiveView::Playlists, "Playlists", "P"),
    (ActiveView::Search, "Search", "S"),
    (ActiveView::Settings, "Settings", "\u{2699}"), // gear icon
];

pub fn render_sidebar(frame: &mut Frame, area: Rect, state: &AppState) {
    if state.sidebar_expanded {
        render_expanded(frame, area, state);
    } else {
        render_collapsed(frame, area, state);
    }
}

/// Collapsed sidebar: single-character icons stacked vertically, bordered on the right.
fn render_collapsed(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(TEXT_DIM))
        .style(Style::default().bg(BG_PRIMARY));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    for (idx, (view, _label, icon)) in NAV_ITEMS.iter().enumerate() {
        if idx as u16 >= inner.height {
            break;
        }

        let row = Rect::new(inner.x, inner.y + idx as u16, inner.width, 1);
        let style = if *view == state.active_view {
            Style::default().fg(ACCENT).bg(BG_SELECTED)
        } else {
            Style::default().fg(TEXT_SECONDARY)
        };

        let line = Line::from(Span::styled(format!(" {icon}"), style));
        frame.render_widget(Paragraph::new(line), row);
    }
}

/// Expanded sidebar with header, separator, and full labels.
fn render_expanded(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(TEXT_DIM))
        .style(Style::default().bg(BG_PRIMARY));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Header
    lines.push(Line::from(Span::styled(
        " QBZ",
        Style::default().fg(ACCENT).bold(),
    )));

    // Separator
    let sep = "\u{2500}".repeat((inner.width as usize).saturating_sub(1));
    lines.push(Line::from(Span::styled(
        format!(" {sep}"),
        Style::default().fg(TEXT_DIM),
    )));

    // Nav items
    for (view, label, _icon) in NAV_ITEMS {
        let is_active = *view == state.active_view;
        let marker = if is_active { "\u{25b6}" } else { " " };
        let style = if is_active {
            Style::default().fg(ACCENT).bg(BG_SELECTED)
        } else {
            Style::default().fg(TEXT_PRIMARY)
        };
        lines.push(Line::from(Span::styled(
            format!(" {marker} {label}"),
            style,
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}
