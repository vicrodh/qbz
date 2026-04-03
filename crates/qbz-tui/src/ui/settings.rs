//! Settings view — read and edit audio and playback settings.
//!
//! Two-section layout:
//! - Audio Configuration: output device, backend, exclusive mode, DAC passthrough, etc.
//! - Playback Settings: streaming, caching, normalization, gapless.
//!
//! Navigate with j/k, toggle booleans with Enter/Space, edit numeric values with +/-.

use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
};
use ratatui::Frame;

use crate::app::AppState;
use crate::theme::{ACCENT, BG_SELECTED, HIRES_BADGE, TEXT_DIM, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

/// A single setting item for display.
#[derive(Debug, Clone)]
pub struct SettingItem {
    pub label: String,
    pub value: String,
    pub kind: SettingKind,
    pub section: SettingSection,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingKind {
    /// Boolean toggle (Enter/Space to toggle)
    Toggle,
    /// Numeric value (+/- to adjust)
    Numeric,
    /// Cycle through options (Enter to advance)
    Cycle,
    /// Read-only display value
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingSection {
    Audio,
    Playback,
}

/// Build the list of setting items from the current audio settings.
pub fn build_settings_list(state: &AppState) -> Vec<SettingItem> {
    let settings = &state.settings.audio_settings;
    let mut items = Vec::new();

    // === Audio Configuration ===
    items.push(SettingItem {
        label: "Output Device".into(),
        value: settings.output_device.clone().unwrap_or_else(|| "System Default".into()),
        kind: SettingKind::Cycle,
        section: SettingSection::Audio,
    });

    items.push(SettingItem {
        label: "Backend".into(),
        value: match &settings.backend_type {
            Some(b) => format!("{:?}", b),
            None => "Auto".into(),
        },
        kind: SettingKind::Cycle,
        section: SettingSection::Audio,
    });

    items.push(SettingItem {
        label: "Exclusive Mode".into(),
        value: if settings.exclusive_mode { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Audio,
    });

    items.push(SettingItem {
        label: "DAC Passthrough".into(),
        value: if settings.dac_passthrough { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Audio,
    });

    items.push(SettingItem {
        label: "PipeWire Force Bit-Perfect".into(),
        value: if settings.pw_force_bitperfect { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Audio,
    });

    items.push(SettingItem {
        label: "ALSA Plugin".into(),
        value: match &settings.alsa_plugin {
            Some(p) => format!("{:?}", p),
            None => "Default".into(),
        },
        kind: SettingKind::Cycle,
        section: SettingSection::Audio,
    });

    items.push(SettingItem {
        label: "ALSA Hardware Volume".into(),
        value: if settings.alsa_hardware_volume { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Audio,
    });

    items.push(SettingItem {
        label: "Preferred Sample Rate".into(),
        value: settings.preferred_sample_rate
            .map(|r| format!("{} Hz", r))
            .unwrap_or_else(|| "Auto".into()),
        kind: SettingKind::Numeric,
        section: SettingSection::Audio,
    });

    items.push(SettingItem {
        label: "Limit Quality to Device".into(),
        value: if settings.limit_quality_to_device { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Audio,
    });

    items.push(SettingItem {
        label: "Device Max Sample Rate".into(),
        value: settings
            .device_max_sample_rate
            .map(|r| format!("{} Hz", r))
            .unwrap_or_else(|| "Auto".into()),
        kind: SettingKind::ReadOnly,
        section: SettingSection::Audio,
    });

    items.push(SettingItem {
        label: "Volume".into(),
        value: format!("{}%", (state.volume * 100.0) as u32),
        kind: SettingKind::ReadOnly,
        section: SettingSection::Audio,
    });

    // === Playback Settings ===
    items.push(SettingItem {
        label: "Streaming Only".into(),
        value: if settings.streaming_only { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Playback,
    });

    items.push(SettingItem {
        label: "Stream First Track".into(),
        value: if settings.stream_first_track { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Playback,
    });

    items.push(SettingItem {
        label: "Stream Buffer".into(),
        value: format!("{} seconds", settings.stream_buffer_seconds),
        kind: SettingKind::Numeric,
        section: SettingSection::Playback,
    });

    items.push(SettingItem {
        label: "Gapless Playback".into(),
        value: if settings.gapless_enabled { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Playback,
    });

    items.push(SettingItem {
        label: "Volume Normalization".into(),
        value: if settings.normalization_enabled { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Playback,
    });

    items.push(SettingItem {
        label: "Normalization Target".into(),
        value: format!("{:.1} LUFS", settings.normalization_target_lufs),
        kind: SettingKind::Numeric,
        section: SettingSection::Playback,
    });

    items
}

/// Render the settings view inside `area`.
pub fn render_settings(frame: &mut Frame, area: Rect, state: &mut AppState) {
    if !state.settings.loaded {
        let msg = Paragraph::new("Loading settings...")
            .style(Style::default().fg(ACCENT))
            .alignment(ratatui::layout::Alignment::Center);
        let mid_y = area.y + area.height / 2;
        if mid_y < area.y + area.height {
            frame.render_widget(msg, Rect::new(area.x, mid_y, area.width, 1));
        }
        return;
    }

    // Title + help hint
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(1),   // settings list
        ])
        .split(area);

    // Header
    let header = Paragraph::new(Line::from(vec![
        Span::styled("Settings", Style::default().fg(ACCENT).bold()),
        Span::styled("  ", Style::default()),
        Span::styled(
            "Enter/Space: toggle  +/-: adjust  r: reload",
            Style::default().fg(TEXT_DIM),
        ),
    ]));
    frame.render_widget(header, chunks[0]);

    // Build items list
    let settings_items = build_settings_list(state);
    let selected_index = state.settings.selected_index;
    let total_items = settings_items.len();

    let mut list_items: Vec<ListItem<'_>> = Vec::new();
    let mut current_section: Option<SettingSection> = None;
    let mut visual_to_data: Vec<Option<usize>> = Vec::new(); // maps visual row to data index

    for (data_idx, item) in settings_items.iter().enumerate() {
        // Section header
        if current_section != Some(item.section) {
            current_section = Some(item.section);
            let section_name = match item.section {
                SettingSection::Audio => "Audio Configuration",
                SettingSection::Playback => "Playback Settings",
            };

            // Add blank line between sections (except the first)
            if data_idx > 0 {
                list_items.push(ListItem::new(Line::from("")));
                visual_to_data.push(None);
            }

            list_items.push(ListItem::new(Line::from(Span::styled(
                format!("  {}", section_name),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ))));
            visual_to_data.push(None);
        }

        let is_selected = data_idx == selected_index;

        let style = if is_selected {
            Style::default().fg(TEXT_PRIMARY).bg(BG_SELECTED)
        } else {
            Style::default().fg(TEXT_SECONDARY)
        };

        // Build the row: label (left) ... value (right)
        let label_width = 30;
        let label = format!("  {:<width$}", item.label, width = label_width);

        let value_style = match item.kind {
            SettingKind::Toggle => {
                if item.value == "ON" {
                    style.fg(HIRES_BADGE)
                } else {
                    style.fg(TEXT_DIM)
                }
            }
            SettingKind::Numeric => style.fg(ACCENT),
            SettingKind::Cycle => style.fg(ACCENT),
            SettingKind::ReadOnly => style.fg(TEXT_MUTED),
        };

        let kind_hint = match item.kind {
            SettingKind::Toggle => " [toggle]",
            SettingKind::Numeric => " [+/-]",
            SettingKind::Cycle => " [cycle]",
            SettingKind::ReadOnly => "",
        };

        let spans = vec![
            Span::styled(label, if is_selected { style.bold() } else { style }),
            Span::styled(&item.value, value_style),
            Span::styled(kind_hint, style.fg(TEXT_DIM)),
        ];

        list_items.push(ListItem::new(Line::from(spans)));
        visual_to_data.push(Some(data_idx));
    }

    let list = List::new(list_items);

    // Find the visual index for the selected data index
    let visual_selected = visual_to_data
        .iter()
        .position(|v| *v == Some(selected_index))
        .unwrap_or(0);

    let mut list_state = ListState::default();
    list_state.select(Some(visual_selected));

    frame.render_stateful_widget(list, chunks[1], &mut list_state);

    // Scrollbar
    if total_items > 0 {
        state.settings.scrollbar_state = state
            .settings
            .scrollbar_state
            .content_length(total_items)
            .position(selected_index);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{2191}"))
            .end_symbol(Some("\u{2193}"));

        frame.render_stateful_widget(
            scrollbar,
            chunks[1].inner(Margin {
                vertical: 0,
                horizontal: 1,
            }),
            &mut state.settings.scrollbar_state,
        );
    }
}
