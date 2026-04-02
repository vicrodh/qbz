//! Device picker modal — centered overlay for selecting an audio output device.
//!
//! Triggered from Settings when the user presses Enter on "Output Device".
//! Lists all devices for the current backend with name, description, and max sample rate.

use ratatui::layout::{Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Clear, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
};
use ratatui::Frame;

use crate::app::AppState;
use crate::theme::{ACCENT, BG_SECONDARY, BG_SELECTED, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

/// Compute a centered popup area using percentage of terminal size.
fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

/// Format a sample rate for display (e.g., 192000 -> "192kHz", 44100 -> "44.1kHz").
fn format_sample_rate(rate: u32) -> String {
    if rate % 1000 == 0 {
        format!("{}kHz", rate / 1000)
    } else {
        format!("{:.1}kHz", rate as f64 / 1000.0)
    }
}

/// Render the device picker modal as a centered overlay.
pub fn render_device_picker(frame: &mut Frame, state: &mut AppState) {
    let area = frame.area();

    // 60% width, 50% height, centered
    let popup = popup_area(area, 60, 50);

    // Clear the background area
    frame.render_widget(Clear, popup);

    // Outer block with border and title
    let block = Block::bordered()
        .title(Line::from(vec![
            Span::styled(
                " Select Output Device ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title_bottom(Line::from(vec![Span::styled(
            " Enter: select  Esc: cancel  j/k: navigate ",
            Style::default().fg(TEXT_MUTED),
        )]))
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG_SECONDARY));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.height < 3 || inner.width < 20 {
        return;
    }

    // Loading state
    if state.devices_loading {
        let msg = Paragraph::new("Scanning devices...")
            .style(Style::default().fg(ACCENT))
            .alignment(ratatui::layout::Alignment::Center);
        let mid_y = inner.y + inner.height / 2;
        if mid_y < inner.y + inner.height {
            frame.render_widget(msg, Rect::new(inner.x, mid_y, inner.width, 1));
        }
        return;
    }

    // Empty state
    if state.available_devices.is_empty() {
        let msg = Paragraph::new("No devices found")
            .style(Style::default().fg(TEXT_DIM))
            .alignment(ratatui::layout::Alignment::Center);
        let mid_y = inner.y + inner.height / 2;
        if mid_y < inner.y + inner.height {
            frame.render_widget(msg, Rect::new(inner.x, mid_y, inner.width, 1));
        }
        return;
    }

    let current_device_id = &state.settings.audio_settings.output_device;
    let selected_index = state.device_picker_index;
    let device_count = state.available_devices.len();

    let items: Vec<ListItem<'_>> = state
        .available_devices
        .iter()
        .enumerate()
        .map(|(idx, device)| {
            let is_highlighted = idx == selected_index;

            // Determine if this is the currently configured device
            let is_current = current_device_id
                .as_ref()
                .map(|cid| cid == &device.id)
                .unwrap_or(device.is_default);

            let marker = if is_current { "[*]" } else { "[ ]" };

            let style = if is_highlighted {
                Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let mut spans = vec![
                Span::styled(
                    format!("  {} ", marker),
                    if is_current {
                        style.fg(HIRES_BADGE)
                    } else {
                        style.fg(TEXT_DIM)
                    },
                ),
                Span::styled(
                    device.name.clone(),
                    if is_highlighted { style.bold() } else { style },
                ),
            ];

            // Add description if available
            if let Some(ref desc) = device.description {
                spans.push(Span::styled(
                    format!("  ({})", desc),
                    style.fg(TEXT_MUTED),
                ));
            }

            // Add max sample rate
            if let Some(rate) = device.max_sample_rate {
                let rate_str = format_sample_rate(rate);
                let rate_color = if rate >= 192000 {
                    HIRES_BADGE
                } else {
                    TEXT_MUTED
                };
                spans.push(Span::styled(
                    format!("  {}", rate_str),
                    style.fg(rate_color),
                ));
            }

            // Add bus type badge
            if let Some(ref bus) = device.device_bus {
                spans.push(Span::styled(
                    format!("  [{}]", bus),
                    style.fg(TEXT_DIM),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items);

    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));

    frame.render_stateful_widget(list, inner, &mut list_state);

    // Scrollbar (when there are more devices than fit)
    if device_count > inner.height as usize {
        let mut scrollbar_state = ratatui::widgets::ScrollbarState::default()
            .content_length(device_count)
            .position(selected_index);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}"))
            .end_symbol(Some("\u{2193}"));

        frame.render_stateful_widget(
            scrollbar,
            inner.inner(Margin {
                vertical: 0,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}
