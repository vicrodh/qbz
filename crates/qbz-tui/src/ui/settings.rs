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
    QConnect,
}

/// Build the list of setting items from the current audio settings.
pub fn build_settings_list(state: &AppState) -> Vec<SettingItem> {
    let settings = &state.settings.audio_settings;
    let mut items = Vec::new();

    // Backend detection for conditional settings (same constraints as desktop)
    use qbz_audio::{AudioBackendType, AlsaPlugin};
    let is_alsa = settings.backend_type == Some(AudioBackendType::Alsa);
    let is_pipewire = settings.backend_type == Some(AudioBackendType::PipeWire);
    let is_alsa_hw = is_alsa && settings.alsa_plugin == Some(AlsaPlugin::Hw);

    // Pretty backend name (matches desktop: PipeWire, ALSA Direct, PulseAudio, System Audio)
    let backend_pretty = match &settings.backend_type {
        Some(AudioBackendType::PipeWire) => "PipeWire".to_string(),
        Some(AudioBackendType::Alsa) => "ALSA Direct".to_string(),
        Some(AudioBackendType::Pulse) => "PulseAudio".to_string(),
        Some(AudioBackendType::SystemDefault) => "System Audio".to_string(),
        None => "Auto".to_string(),
    };

    // Pretty ALSA plugin name
    let alsa_plugin_pretty = match &settings.alsa_plugin {
        Some(AlsaPlugin::Hw) => "hw (Direct Hardware)".to_string(),
        Some(AlsaPlugin::PlugHw) => "plughw (Plugin)".to_string(),
        Some(AlsaPlugin::Pcm) => "default (PCM)".to_string(),
        None => "Default".to_string(),
    };

    // === Audio Configuration (matches desktop SettingsView order exactly) ===

    // 1. Streaming Quality
    items.push(SettingItem {
        label: "Streaming Quality".into(),
        value: state.settings.streaming_quality.clone(),
        kind: SettingKind::Cycle,
        section: SettingSection::Audio,
    });

    // 2. Limit Quality to Device
    items.push(SettingItem {
        label: "Limit Quality to Device".into(),
        value: if settings.limit_quality_to_device { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Audio,
    });

    // 3. Device Max Sample Rate (only shown when limit_quality_to_device is ON)
    if settings.limit_quality_to_device {
        items.push(SettingItem {
            label: "Device Max Sample Rate".into(),
            value: settings
                .device_max_sample_rate
                .map(|r| match r {
                    44100 => "44.1 kHz (CD)".into(),
                    48000 => "48 kHz (DVD)".into(),
                    96000 => "96 kHz (Hi-Res)".into(),
                    192000 => "192 kHz (Hi-Res+)".into(),
                    384000 => "384 kHz (DSD)".into(),
                    other => format!("{} Hz", other),
                })
                .unwrap_or_else(|| "No limit".into()),
            kind: SettingKind::Numeric,
            section: SettingSection::Audio,
        });
    }

    // 4. Audio Backend
    items.push(SettingItem {
        label: "Audio Backend".into(),
        value: backend_pretty,
        kind: SettingKind::Cycle,
        section: SettingSection::Audio,
    });

    // 5. Output Device
    items.push(SettingItem {
        label: "Output Device".into(),
        value: settings.output_device.clone().unwrap_or_else(|| "System Default".into()),
        kind: SettingKind::Cycle,
        section: SettingSection::Audio,
    });

    // 6. ALSA Plugin (only when backend = ALSA Direct)
    if is_alsa {
        items.push(SettingItem {
            label: "ALSA Plugin".into(),
            value: alsa_plugin_pretty,
            kind: SettingKind::Cycle,
            section: SettingSection::Audio,
        });
    }

    // 7. Hardware Volume (only when ALSA Direct + Hw plugin)
    if is_alsa_hw {
        items.push(SettingItem {
            label: "Hardware Volume".into(),
            value: if settings.alsa_hardware_volume { "ON" } else { "OFF" }.into(),
            kind: SettingKind::Toggle,
            section: SettingSection::Audio,
        });
    }

    // 8. Exclusive Mode (only available with ALSA Direct)
    items.push(SettingItem {
        label: "Exclusive Mode".into(),
        value: if !is_alsa {
            "N/A (ALSA only)".into()
        } else if settings.exclusive_mode {
            "ON".into()
        } else {
            "OFF".into()
        },
        kind: if is_alsa { SettingKind::Toggle } else { SettingKind::ReadOnly },
        section: SettingSection::Audio,
    });

    // 9. DAC Passthrough (only available with PipeWire)
    items.push(SettingItem {
        label: "DAC Passthrough".into(),
        value: if !is_pipewire {
            "N/A (PipeWire only)".into()
        } else if settings.dac_passthrough {
            "ON".into()
        } else {
            "OFF".into()
        },
        kind: if is_pipewire { SettingKind::Toggle } else { SettingKind::ReadOnly },
        section: SettingSection::Audio,
    });

    // 10. PW Force Bit-Perfect (only when PipeWire + DAC Passthrough ON)
    if is_pipewire && settings.dac_passthrough {
        items.push(SettingItem {
            label: "PW Force Bit-Perfect".into(),
            value: if settings.pw_force_bitperfect { "ON" } else { "OFF" }.into(),
            kind: SettingKind::Toggle,
            section: SettingSection::Audio,
        });
    }

    // 11. Volume (read-only display)
    items.push(SettingItem {
        label: "Volume".into(),
        value: format!("{}%", (state.volume * 100.0) as u32),
        kind: SettingKind::ReadOnly,
        section: SettingSection::Audio,
    });

    // === Playback Settings (matches desktop order) ===

    // Gapless (disabled when streaming_only is ON)
    items.push(SettingItem {
        label: "Gapless Playback".into(),
        value: if settings.streaming_only {
            "N/A (streaming only)".into()
        } else if settings.gapless_enabled {
            "ON".into()
        } else {
            "OFF".into()
        },
        kind: if settings.streaming_only { SettingKind::ReadOnly } else { SettingKind::Toggle },
        section: SettingSection::Playback,
    });

    // Stream Uncached (stream_first_track)
    items.push(SettingItem {
        label: "Stream Uncached".into(),
        value: if settings.stream_first_track { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Playback,
    });

    // Initial Buffer (only when stream_first_track is ON)
    if settings.stream_first_track {
        items.push(SettingItem {
            label: "Initial Buffer".into(),
            value: format!("{} seconds", settings.stream_buffer_seconds),
            kind: SettingKind::Numeric,
            section: SettingSection::Playback,
        });
    }

    // Streaming Only
    items.push(SettingItem {
        label: "Streaming Only".into(),
        value: if settings.streaming_only { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Playback,
    });

    // Volume Normalization
    items.push(SettingItem {
        label: "Volume Normalization".into(),
        value: if settings.normalization_enabled { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::Playback,
    });

    // Normalization Target (only when normalization is ON)
    if settings.normalization_enabled {
        items.push(SettingItem {
            label: "Normalization Target".into(),
            value: format!("{:.1} LUFS", settings.normalization_target_lufs),
            kind: SettingKind::Numeric,
            section: SettingSection::Playback,
        });
    }

    // === QConnect (Qobuz Connect) ===

    items.push(SettingItem {
        label: "QConnect".into(),
        value: if state.qconnect.enabled { "ON" } else { "OFF" }.into(),
        kind: SettingKind::Toggle,
        section: SettingSection::QConnect,
    });

    items.push(SettingItem {
        label: "Status".into(),
        value: state.qconnect.status.clone(),
        kind: SettingKind::ReadOnly,
        section: SettingSection::QConnect,
    });

    if let Some(ref err) = state.qconnect.last_error {
        items.push(SettingItem {
            label: "Last Error".into(),
            value: err.clone(),
            kind: SettingKind::ReadOnly,
            section: SettingSection::QConnect,
        });
    }

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
                SettingSection::QConnect => "QConnect (Qobuz Connect)",
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
