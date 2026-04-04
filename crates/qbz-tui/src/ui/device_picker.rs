//! Device picker modal — categorized device list matching desktop app.
//!
//! Groups devices by type: DEFAULTS, USB AUDIO (BIT-PERFECT CAPABLE),
//! OTHER HARDWARE, following the same logic as DeviceDropdown.svelte.

use ratatui::layout::{Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Clear, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
};
use ratatui::Frame;

use crate::app::AppState;
use crate::theme::{ACCENT, BG_SECONDARY, BG_SELECTED, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

/// Get a pretty display name for a PipeWire device (strips alsa_output. prefix).
fn pretty_device_name(device: &qbz_audio::AudioDevice) -> String {
    // Prefer description if available (PipeWire provides friendly names)
    if let Some(ref desc) = device.description {
        if !desc.is_empty() {
            return desc.clone();
        }
    }

    let name = &device.name;

    // Strip common PipeWire prefixes for cleaner display
    if let Some(stripped) = name.strip_prefix("alsa_output.") {
        // "usb-Cambridge_Audio_Cambridge_Audio_USB_Audio_2.0_0000-00.analog-stereo"
        // → "Cambridge Audio USB Audio 2.0 Analog Stereo"
        let cleaned = stripped
            .replace('_', " ")
            .replace('-', " ")
            .replace(".analog stereo", " Analog Stereo")
            .replace(".iec958 stereo", " Digital Stereo");
        // Capitalize first letters of significant words
        return cleaned;
    }

    name.clone()
}

/// Render the device picker modal as a categorized, grouped list.
pub fn render_device_picker(frame: &mut Frame, state: &mut AppState) {
    let area = frame.area();
    let popup = popup_area(area, 65, 55);

    frame.render_widget(Clear, popup);

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

    // Group devices by category (same logic as desktop DeviceDropdown.svelte)
    let current_device_id = &state.settings.audio_settings.output_device;
    let selected_index = state.device_picker_index;

    let mut defaults: Vec<usize> = Vec::new();
    let mut usb_audio: Vec<usize> = Vec::new();
    let mut other_hw: Vec<usize> = Vec::new();

    for (idx, device) in state.available_devices.iter().enumerate() {
        if device.is_default {
            defaults.push(idx);
        } else if device.device_bus.as_deref() == Some("usb") && device.is_hardware {
            usb_audio.push(idx);
        } else if device.is_hardware {
            other_hw.push(idx);
        } else {
            other_hw.push(idx); // Virtual sinks go to Other
        }
    }

    // Build the visual list with group headers
    let mut list_items: Vec<ListItem<'_>> = Vec::new();
    let mut visual_to_device: Vec<Option<usize>> = Vec::new();

    let groups: Vec<(&str, &[usize])> = vec![
        ("DEFAULTS", &defaults),
        ("USB AUDIO (BIT-PERFECT CAPABLE)", &usb_audio),
        ("OTHER HARDWARE", &other_hw),
    ];

    for (group_label, device_indices) in &groups {
        if device_indices.is_empty() {
            continue;
        }

        // Group header
        if !list_items.is_empty() {
            list_items.push(ListItem::new(Line::from("")));
            visual_to_device.push(None);
        }
        list_items.push(ListItem::new(Line::from(Span::styled(
            format!("  {}", group_label),
            Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
        ))));
        visual_to_device.push(None);

        // Devices in this group
        for &dev_idx in *device_indices {
            let device = &state.available_devices[dev_idx];
            let is_highlighted = dev_idx == selected_index;

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

            let display_name = pretty_device_name(device);

            let mut spans = vec![
                Span::styled(
                    format!("  {} ", marker),
                    if is_current { style.fg(HIRES_BADGE) } else { style.fg(TEXT_DIM) },
                ),
                Span::styled(
                    display_name,
                    if is_highlighted { style.bold() } else { style },
                ),
            ];

            // BP badge for USB audio devices
            if device.device_bus.as_deref() == Some("usb") && device.is_hardware {
                spans.push(Span::styled(
                    "  BP",
                    style.fg(HIRES_BADGE).add_modifier(Modifier::BOLD),
                ));
            }

            list_items.push(ListItem::new(Line::from(spans)));
            visual_to_device.push(Some(dev_idx));
        }
    }

    let list = List::new(list_items);

    // Find visual index for selected device
    let visual_selected = visual_to_device
        .iter()
        .position(|v| *v == Some(selected_index))
        .unwrap_or(0);

    let mut list_state = ListState::default();
    list_state.select(Some(visual_selected));

    frame.render_stateful_widget(list, inner, &mut list_state);

    // Scrollbar
    let device_count = state.available_devices.len();
    if device_count > 0 {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}"))
            .end_symbol(Some("\u{2193}"));

        let mut scrollbar_state = ratatui::widgets::ScrollbarState::default()
            .content_length(device_count)
            .position(selected_index);

        frame.render_stateful_widget(
            scrollbar,
            inner.inner(Margin { vertical: 0, horizontal: 1 }),
            &mut scrollbar_state,
        );
    }
}
