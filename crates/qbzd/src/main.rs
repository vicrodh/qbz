use clap::{Parser, Subcommand};

mod api;
mod config;
mod lock;
mod paths;
mod state;

pub const API_VERSION: u32 = 1; // 02-cli-and-api.md §1.6

#[derive(Parser)]
#[command(name = "qbzd", version, arg_required_else_help = true,
          about = "QBZ headless Qobuz playback daemon")]
struct Cli {
    /// Target daemon (default 127.0.0.1:8182; env QBZD_HOST)
    #[arg(long, global = true)]
    host: Option<String>,
    /// API token (default: auto-read ~/.config/qbzd/api_token; env QBZD_TOKEN)
    #[arg(long, global = true)]
    token: Option<String>,
    #[arg(short, long, global = true)]
    quiet: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the daemon in the foreground (systemd ExecStart)
    Run,
    /// Log in to Qobuz (one-shot browser listener; --paste; --token)
    Login {
        #[arg(long)] callback_host: Option<String>,
        #[arg(long)] paste: bool,
        #[arg(long)] token: Option<String>,
    },
    Logout,
    /// Interactive configurator (six screens)
    Setup,
    /// Composite daemon diagnostic
    Status { #[arg(long)] json: bool },
    Ping   { #[arg(long)] json: bool },
    /// One-line now-playing
    Now    { #[arg(long)] json: bool },
    Play, Pause, Toggle, Stop, Next, Prev,
    /// Absolute secs, +N/-N, or mm:ss
    Seek   { position: String },
    /// Bare = read; 0-100, +N, -N
    Volume { value: Option<String>, #[arg(long)] json: bool },
    /// Bare = toggle
    Mute   { state: Option<String> },
    Queue    { #[command(subcommand)] cmd: QueueCmd },
    Settings { #[command(subcommand)] cmd: SettingsCmd },
    Qconnect { #[command(subcommand)] cmd: QconnectCmd },
    Config   { #[command(subcommand)] cmd: ConfigCmd },
    Version  { #[arg(long)] json: bool },
    /// Shell completions (hidden; packaged by T14)
    #[command(hide = true)]
    Completions { shell: clap_complete::Shell },
}

#[derive(Subcommand)]
enum QueueCmd {
    List  { #[arg(long)] json: bool },
    Add   { track_id: u64, #[arg(long)] next: bool },
    Remove{ index: usize },
    Clear { #[arg(long)] keep_current: bool },
}

#[derive(Subcommand)]
enum SettingsCmd {
    Export {
        file: Option<String>,
        #[arg(long, default_value = "daemon")] from: String, // daemon|desktop
        #[arg(long)] include_auth: bool,
    },
    Import {
        file: String,
        #[arg(long)] include_auth: bool,
        #[arg(long)] trust_dsd: bool,
        #[arg(long)] remap: Vec<String>,   // OLD=NEW, repeatable
        #[arg(long)] dry_run: bool,
    },
    Show { #[arg(long)] json: bool },
    Set  { key: String, value: String },
}

#[derive(Subcommand)]
enum QconnectCmd { Enable, Disable, Name { name: String } }

#[derive(Subcommand)]
enum ConfigCmd { Path, Show { #[arg(long)] json: bool }, RegenToken }

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::Version { json } => {
            if json {
                println!("{{\"version\":\"{}\",\"api_version\":{}}}",
                         env!("CARGO_PKG_VERSION"), API_VERSION);
            } else {
                println!("qbzd {} (api v{})", env!("CARGO_PKG_VERSION"), API_VERSION);
            }
            0
        }
        Cmd::Completions { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(shell, &mut Cli::command(), "qbzd",
                                    &mut std::io::stdout());
            0
        }
        _ => { eprintln!("not implemented yet"); 1 } // burned down task by task
    };
    std::process::exit(code);
}
