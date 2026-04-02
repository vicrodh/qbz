//! Login modal popup — centered overlay triggered by `l` when not authenticated.
//! Follows Jellyfin-TUI popup pattern: Clear widget to blank background,
//! Block::bordered() with title, input fields for email and password.

use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::theme::{ACCENT, BG_SECONDARY, DANGER, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY};

/// Compute a centered popup area with fixed dimensions.
fn popup_area(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

/// Render the login modal popup as a centered overlay.
pub fn render_login_modal(frame: &mut Frame, state: &AppState) {
    let area = frame.area();

    // 50 chars wide, 10 lines tall, centered
    let popup = popup_area(area, 50, 10);

    // Clear the background area
    frame.render_widget(Clear, popup);

    // Outer block with border and title
    let border_color = ACCENT;

    let block = Block::bordered()
        .title(Line::from(vec![
            Span::styled(
                " Login to Qobuz ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title_bottom(Line::from(vec![Span::styled(
            " Tab: switch field | Enter: login | Esc: cancel ",
            Style::default().fg(TEXT_MUTED),
        )]))
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(BG_SECONDARY));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.height < 6 || inner.width < 20 {
        return;
    }

    // Split inner area: email_label(1) + email_input(1) + gap(1) +
    //                    password_label(1) + password_input(1) + gap(1) + status(1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // email label
            Constraint::Length(1), // email input
            Constraint::Length(1), // gap
            Constraint::Length(1), // password label
            Constraint::Length(1), // password input
            Constraint::Length(1), // gap
            Constraint::Min(1),   // status / error line
        ])
        .split(inner);

    let login = &state.login;
    let is_email_active = login.active_field == 0;
    let is_password_active = login.active_field == 1;

    // Email label
    let email_label_style = if is_email_active {
        Style::default().fg(ACCENT).bold()
    } else {
        Style::default().fg(TEXT_MUTED)
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(" Email", email_label_style))),
        chunks[0],
    );

    // Email input
    render_text_field(
        frame,
        chunks[1],
        &login.email,
        login.email_cursor,
        is_email_active,
        false,
    );

    // Password label
    let password_label_style = if is_password_active {
        Style::default().fg(ACCENT).bold()
    } else {
        Style::default().fg(TEXT_MUTED)
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(" Password", password_label_style))),
        chunks[3],
    );

    // Password input (masked)
    render_text_field(
        frame,
        chunks[4],
        &login.password,
        login.password_cursor,
        is_password_active,
        true,
    );

    // Status / error line
    if login.logging_in {
        let line = Line::from(Span::styled(
            " Logging in...",
            Style::default().fg(ACCENT),
        ));
        frame.render_widget(Paragraph::new(line), chunks[6]);
    } else if let Some(ref err) = login.error {
        let line = Line::from(Span::styled(
            format!(" {}", err),
            Style::default().fg(DANGER),
        ));
        frame.render_widget(Paragraph::new(line), chunks[6]);
    }
}

/// Render a single-line text field with cursor.
fn render_text_field(
    frame: &mut Frame,
    area: Rect,
    value: &str,
    cursor: usize,
    is_active: bool,
    masked: bool,
) {
    let display_value: String = if masked {
        "*".repeat(value.chars().count())
    } else {
        value.to_string()
    };

    let prefix = if is_active { " > " } else { "   " };
    let prefix_style = if is_active {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(TEXT_DIM)
    };

    if is_active {
        // Calculate the display cursor position (for masked text, cursor is char count)
        let display_cursor = if masked {
            // For masked text, each char maps to one '*'
            value[..cursor.min(value.len())]
                .chars()
                .count()
        } else {
            cursor.min(value.len())
        };

        let clamped = display_cursor.min(display_value.len());
        let before = &display_value[..clamped];

        if clamped < display_value.len() {
            let rest = &display_value[clamped..];
            let cursor_char = rest.chars().next().unwrap();
            let char_end = clamped + cursor_char.len_utf8();
            let after = &display_value[char_end..];

            let line = Line::from(vec![
                Span::styled(prefix, prefix_style),
                Span::styled(before.to_string(), Style::default().fg(TEXT_PRIMARY)),
                Span::styled(
                    cursor_char.to_string(),
                    Style::default().fg(TEXT_PRIMARY).bg(ACCENT),
                ),
                Span::styled(after.to_string(), Style::default().fg(TEXT_PRIMARY)),
            ]);
            frame.render_widget(Paragraph::new(line), area);
        } else {
            let line = Line::from(vec![
                Span::styled(prefix, prefix_style),
                Span::styled(before.to_string(), Style::default().fg(TEXT_PRIMARY)),
                Span::styled(" ", Style::default().bg(ACCENT)),
            ]);
            frame.render_widget(Paragraph::new(line), area);
        }
    } else if display_value.is_empty() {
        let placeholder = if masked { "" } else { "" };
        let line = Line::from(vec![
            Span::styled(prefix, prefix_style),
            Span::styled(placeholder, Style::default().fg(TEXT_DIM)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    } else {
        let line = Line::from(vec![
            Span::styled(prefix, prefix_style),
            Span::styled(display_value, Style::default().fg(TEXT_PRIMARY)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }
}
