//! Interactive TUI setup wizard for qbzd.
//!
//! Configures audio, playback, cache, integrations without
//! needing the desktop app. Reads/writes config files and
//! databases directly (runs without the daemon).

mod audio;
mod cache;
mod integrations;
mod qconnect;

use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{prelude::*, widgets::*};
use std::io::stdout;

/// Available wizard sections
const SECTIONS: &[(&str, &str)] = &[
    ("audio", "Audio Backend & Device"),
    ("cache", "Cache & Resources"),
    ("integrations", "Integrations (ListenBrainz)"),
    ("qconnect", "QConnect"),
];

/// Run the full wizard or a specific section.
pub fn run(section: Option<&str>) -> Result<(), String> {
    if let Some(s) = section {
        return run_section(s);
    }

    // Full wizard: show menu, let user pick sections
    enable_raw_mode().map_err(|e| e.to_string())?;
    stdout().execute(EnterAlternateScreen).map_err(|e| e.to_string())?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

    let mut selected = 0usize;
    let result = loop {
        terminal
            .draw(|frame| draw_menu(frame, selected))
            .map_err(|e| e.to_string())?;

        if let Ok(Event::Key(key)) = event::read() {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                KeyCode::Up | KeyCode::Char('k') => {
                    if selected > 0 {
                        selected -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if selected < SECTIONS.len() - 1 {
                        selected += 1;
                    }
                }
                KeyCode::Enter => {
                    // Temporarily leave TUI to run section
                    disable_raw_mode().ok();
                    stdout().execute(LeaveAlternateScreen).ok();

                    let section_key = SECTIONS[selected].0;
                    if let Err(e) = run_section(section_key) {
                        eprintln!("Section '{}' error: {}", section_key, e);
                    }

                    // Return to TUI
                    enable_raw_mode().ok();
                    stdout().execute(EnterAlternateScreen).ok();
                    terminal = Terminal::new(CrosstermBackend::new(stdout()))
                        .map_err(|e| e.to_string())?;
                }
                _ => {}
            }
        }
    };

    disable_raw_mode().ok();
    stdout().execute(LeaveAlternateScreen).ok();
    result
}

fn draw_menu(frame: &mut Frame, selected: usize) {
    let area = frame.area();

    let items: Vec<ListItem> = SECTIONS
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            let style = if i == selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if i == selected { "> " } else { "  " };
            ListItem::new(format!("{}{}", prefix, label)).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" QBZ Daemon Setup ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .padding(Padding::new(2, 2, 1, 1)),
        )
        .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black));

    let help = Paragraph::new("↑↓ Navigate  Enter Select  q Quit")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);

    let layout = Layout::vertical([
        Constraint::Min(SECTIONS.len() as u16 + 4),
        Constraint::Length(1),
    ])
    .split(area);

    frame.render_widget(list, layout[0]);
    frame.render_widget(help, layout[1]);
}

fn run_section(section: &str) -> Result<(), String> {
    match section {
        "audio" => audio::run_audio_wizard(),
        "cache" => cache::run_cache_wizard(),
        "integrations" => integrations::run_integrations_wizard(),
        "qconnect" => qconnect::run_qconnect_wizard(),
        _ => Err(format!("Unknown section: {}", section)),
    }
}
