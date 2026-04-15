mod api;
mod config;
mod daemon;
mod adapter;
mod login;
mod mpris;
mod qconnect;
mod resources;
mod wizard;
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
    /// Interactive setup wizard (audio, playback, integrations)
    Setup {
        /// Run only a specific section (audio, playback, cache, integrations, qconnect)
        #[arg(long)]
        section: Option<String>,
    },
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
            if let Err(e) = login::interactive_login().await {
                eprintln!("Login failed: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Status) => {
            let port = cfg.server.port;
            match reqwest::Client::new()
                .get(format!("http://127.0.0.1:{}/api/status", port))
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value = resp.json().await.unwrap_or_default();
                    println!("qbzd running on port {}", port);
                    println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                }
                Ok(resp) => {
                    println!("qbzd responded with HTTP {}", resp.status());
                }
                Err(_) => {
                    println!("qbzd not running (no response on port {})", port);
                }
            }
        }
        Some(Commands::Setup { section }) => {
            if let Err(e) = wizard::run(section.as_deref()) {
                eprintln!("Setup wizard error: {}", e);
                std::process::exit(1);
            }
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
