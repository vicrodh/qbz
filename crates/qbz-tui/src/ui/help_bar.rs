//! Bottom help bar — contextual keybinding hints.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{ActiveView, AppState, InputMode};
use crate::theme::{ACCENT, BG_SECONDARY, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY};

/// Render the 1-line bottom help bar with contextual keybinding hints.
pub fn render_help_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let hints = match state.input_mode {
        InputMode::TextInput => vec![
            ("Enter", "search"),
            ("Esc", "cancel"),
            ("Ctrl+U", "clear"),
        ],
        InputMode::Normal => {
            let mut base: Vec<(&str, &str)> = vec![
                ("Ctrl+Q", "quit"),
            ];

            match state.active_view {
                ActiveView::Discovery => {
                    base.extend([
                        ("Tab", "switch tab"),
                        ("j/k", "navigate"),
                        ("Enter", "open album"),
                        ("/", "search"),
                    ]);
                }
                ActiveView::Search => {
                    base.extend([
                        ("/", "type"),
                        ("j/k", "navigate"),
                        ("Enter", "play"),
                        ("a", "queue"),
                        ("g", "album"),
                    ]);
                }
                ActiveView::Favorites => {
                    base.extend([
                        ("Tab", "switch tab"),
                        ("j/k", "navigate"),
                        ("Enter", "play"),
                        ("a", "queue"),
                        ("g", "album"),
                    ]);
                }
                ActiveView::Album => {
                    base.extend([
                        ("j/k", "navigate"),
                        ("Enter", "play album"),
                        ("a", "queue"),
                        ("Bksp", "back"),
                    ]);
                }
                ActiveView::Playlists => {
                    if state.playlists.detail_playlist.is_some() {
                        base.extend([
                            ("j/k", "navigate"),
                            ("Enter", "play"),
                            ("a", "queue"),
                            ("Bksp", "back"),
                        ]);
                    } else {
                        base.extend([
                            ("j/k", "navigate"),
                            ("Enter", "open"),
                        ]);
                    }
                }
                ActiveView::Settings => {
                    base.extend([
                        ("j/k", "navigate"),
                        ("Enter", "toggle"),
                        ("+/-", "adjust"),
                        ("r", "reload"),
                    ]);
                }
                _ => {}
            }

            // Global playback controls
            base.extend([
                ("Space", "play/pause"),
                ("n/p", "next/prev"),
                ("q", "queue"),
            ]);

            base
        }
    };

    let mut spans: Vec<Span<'_>> = Vec::new();

    for (idx, (key, action)) in hints.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled("  ", Style::default().fg(TEXT_DIM)));
        }

        spans.push(Span::styled(
            *action,
            Style::default().fg(TEXT_MUTED),
        ));
        spans.push(Span::styled(
            format!(" <{}>", key),
            Style::default().fg(TEXT_PRIMARY),
        ));
    }

    // Status message on the right (if any)
    if let Some(ref msg) = state.status_message {
        let left_width: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let msg_width = msg.chars().count();
        let total_width = area.width as usize;
        let gap = total_width.saturating_sub(left_width + msg_width + 2);

        let msg_color = if state.is_buffering { ACCENT } else { TEXT_MUTED };

        if gap > 0 {
            spans.push(Span::raw(" ".repeat(gap)));
            spans.push(Span::styled(
                msg.clone(),
                Style::default().fg(msg_color),
            ));
        }
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line)
        .style(Style::default().bg(BG_SECONDARY));
    frame.render_widget(paragraph, area);
}
