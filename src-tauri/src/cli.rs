use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "qbz", about = "QBZ — Hi-Fi music player for Qobuz")]
pub struct Cli {
    /// Run in TUI mode (terminal interface)
    #[arg(long)]
    pub tui: bool,

    /// Run in headless mode (daemon, no UI)
    #[arg(long)]
    pub headless: bool,

    /// Start web server for remote control (auto-enabled in headless mode)
    #[arg(long)]
    pub web: bool,

    /// Disable terminal image rendering (sixel/kitty)
    #[arg(long)]
    pub no_images: bool,

    /// Export settings to a JSON file and exit
    #[arg(long, value_name = "FILE")]
    pub export_settings: Option<String>,

    /// Import settings from a JSON file and exit
    #[arg(long, value_name = "FILE")]
    pub import_settings: Option<String>,

    /// Auto-detect GPU and apply optimal graphics settings, then exit
    #[arg(long)]
    pub autoconfig_graphics: bool,

    /// Reset all graphics settings to safe defaults, then exit
    #[arg(long)]
    pub reset_graphics: bool,

    /// Reset only the force_dmabuf developer setting, then exit
    #[arg(long)]
    pub reset_dmabuf: bool,
}

#[derive(Debug, PartialEq)]
pub enum RunMode {
    Desktop,
    Tui,
    Headless,
}

impl Cli {
    pub fn run_mode(&self) -> RunMode {
        if self.headless {
            RunMode::Headless
        } else if self.tui {
            RunMode::Tui
        } else {
            RunMode::Desktop
        }
    }
}
