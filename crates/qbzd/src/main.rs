mod config;
mod daemon;
mod adapter;
mod resources;
mod session;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "qbzd", about = "QBZ headless music daemon")]
#[command(version)]
struct Cli {
    /// HTTP port
    #[arg(short, long, default_value_t = 8182)]
    port: u16,

    /// Bind address
    #[arg(short, long, default_value = "0.0.0.0")]
    bind: String,

    /// Data directory
    #[arg(short, long)]
    data_dir: Option<String>,

    /// Config file path
    #[arg(short, long)]
    config: Option<String>,

    /// Auth token (auto-generated if not provided)
    #[arg(long)]
    token: Option<String>,

    /// Disable MPRIS/D-Bus integration
    #[arg(long)]
    no_mpris: bool,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Print a new token and exit
    #[arg(long)]
    generate_token: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive Qobuz login (OAuth via system browser)
    Login,
    /// Show daemon status
    Status,
    /// Show or regenerate API token
    Token,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Init logging
    env_logger::Builder::new()
        .filter_level(cli.log_level.parse().unwrap_or(log::LevelFilter::Info))
        .format_timestamp_millis()
        .init();

    log::info!("qbzd starting...");

    // Load config (TOML file -> env vars -> CLI overrides)
    let mut cfg = config::DaemonConfig::load(cli.config.as_deref());

    // CLI overrides
    cfg.server.port = cli.port;
    cfg.server.bind = cli.bind.clone();
    if let Some(ref dir) = cli.data_dir {
        cfg.data.dir = dir.clone();
    }
    if let Some(ref token) = cli.token {
        cfg.server.token = token.clone();
    }
    cfg.mpris.enabled = !cli.no_mpris;

    // Generate token and exit
    if cli.generate_token {
        let token = config::generate_token();
        println!("{}", token);
        return;
    }

    // Auto-detect resources
    resources::auto_detect_cache_config(&mut cfg);

    // Handle subcommands
    match cli.command {
        Some(Commands::Login) => {
            log::info!("Starting interactive login...");
            // TODO: Phase 1 — OAuth via system browser
            eprintln!("Login not yet implemented. Use qbzd.toml to configure credentials.");
        }
        Some(Commands::Status) => {
            println!("qbzd status: not running (status check requires daemon to be active)");
        }
        Some(Commands::Token) => {
            if cfg.server.token == "auto" {
                let token = config::generate_token();
                println!("Generated token: {}", token);
            } else {
                println!("Current token: {}", cfg.server.token);
            }
        }
        None => {
            // Main daemon mode
            if let Err(e) = daemon::run(cfg).await {
                log::error!("Daemon exited with error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
