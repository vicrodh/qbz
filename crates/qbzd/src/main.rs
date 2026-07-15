use clap::{Parser, Subcommand};

mod adapter;
mod api;
mod cli;
mod config;
mod daemon;
mod lock;
mod login;
mod paths;
mod qconnect;
mod state;

pub const API_VERSION: u32 = 1; // 02-cli-and-api.md §1.6

#[derive(Parser)]
#[command(name = "qbzd", version, arg_required_else_help = true,
          about = "QBZ headless Qobuz playback daemon")]
struct Cli {
    /// Target daemon (default 127.0.0.1:8182; env QBZD_HOST)
    #[arg(long, global = true)]
    host: Option<String>,
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

// The tokenless default has no rotation verb (02 §3.1.2): `config` is just
// path|show. Rotating the opt-in [server] token = edit qbzd.toml + restart.
#[derive(Subcommand)]
enum ConfigCmd { Path, Show { #[arg(long)] json: bool } }

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
        Cmd::Run => {
            // Phase 1: resolve the config root and load qbzd.toml. The config's
            // `data_root` (a container override) can redirect the data/cache
            // roots, so resolve those in phase 2 once it is known.
            let bootstrap = paths::ProfileRoots::resolve(None, None);
            let cfg_path = bootstrap.config.join("qbzd.toml");
            let (cfg, warns) = match config::QbzdConfig::load(&cfg_path) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("error: {e}");
                    eprintln!("  → fix or remove the config:  {}", cfg_path.display());
                    std::process::exit(1);
                }
            };
            // Phase 2: honor an explicit `data_root` container override.
            let data_root = cfg.data_root.clone();
            let roots = paths::ProfileRoots::resolve(
                None,
                data_root.as_deref().map(std::path::Path::new),
            );
            match daemon::run(roots, cfg, warns).await {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        Cmd::Login {
            callback_host,
            paste,
            token,
        } => {
            let roots = login_roots();
            let result = if let Some(tok) = token {
                login::login_with_token_arg(&roots, &tok).await
            } else if paste {
                login::login_paste(&roots).await
            } else {
                login::login_browser(&roots, callback_host).await
            };
            match result {
                Ok(session) => {
                    println!("{}", cli::copy::login_success(&session));
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        Cmd::Logout => {
            let roots = login_roots();
            match login::logout(&roots) {
                Ok(daemon_nudged) => {
                    println!("{}", cli::copy::logout_success(daemon_nudged));
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        Cmd::Status { json } => {
            // The CLI reads only local qbzd.toml (for the opt-in token); the
            // config root is always at its XDG default.
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::status::status(cli.host, json, &roots).await
        }
        Cmd::Ping { json } => {
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::status::ping(cli.host, json, &roots).await
        }
        Cmd::Now { json } => {
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::transport::now(cli.host, json, &roots).await
        }
        Cmd::Play => {
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::transport::play(cli.host, &roots).await
        }
        Cmd::Pause => {
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::transport::pause(cli.host, &roots).await
        }
        Cmd::Toggle => {
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::transport::toggle(cli.host, &roots).await
        }
        Cmd::Stop => {
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::transport::stop(cli.host, &roots).await
        }
        Cmd::Next => {
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::transport::next(cli.host, &roots).await
        }
        Cmd::Prev => {
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::transport::prev(cli.host, &roots).await
        }
        Cmd::Seek { position } => {
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::transport::seek(cli.host, &roots, position).await
        }
        Cmd::Volume { value, json } => {
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::transport::volume(cli.host, &roots, value, json).await
        }
        Cmd::Mute { state } => {
            let roots = paths::ProfileRoots::resolve(None, None);
            cli::transport::mute(cli.host, &roots, state).await
        }
        Cmd::Queue { cmd } => {
            let roots = paths::ProfileRoots::resolve(None, None);
            match cmd {
                QueueCmd::List { json } => cli::queue::list(cli.host, json, &roots).await,
                QueueCmd::Add { track_id, next } => {
                    cli::queue::add(cli.host, &roots, track_id, next).await
                }
                QueueCmd::Remove { index } => cli::queue::remove(cli.host, &roots, index).await,
                QueueCmd::Clear { keep_current } => {
                    cli::queue::clear(cli.host, &roots, keep_current).await
                }
            }
        }
        Cmd::Settings { cmd } => {
            let roots = login_roots();
            match cmd {
                SettingsCmd::Show { json } => cli::settings::show(json, &roots),
                SettingsCmd::Set { key, value } => cli::settings::set(&roots, &key, &value),
                // Export/import land in T12 (04-settings-portability.md).
                SettingsCmd::Export { .. } | SettingsCmd::Import { .. } => {
                    eprintln!("not implemented yet — lands in T12 (04-settings-portability.md)");
                    1
                }
            }
        }
        Cmd::Qconnect { cmd } => {
            let roots = login_roots();
            match cmd {
                QconnectCmd::Enable => cli::settings::qconnect_enable(&roots),
                QconnectCmd::Disable => cli::settings::qconnect_disable(&roots),
                QconnectCmd::Name { name } => cli::settings::qconnect_name(&roots, &name),
            }
        }
        Cmd::Config { cmd } => {
            let roots = login_roots();
            match cmd {
                ConfigCmd::Path => cli::settings::config_path(&roots),
                ConfigCmd::Show { json } => cli::settings::config_show(json, &roots),
            }
        }
        _ => { eprintln!("not implemented yet"); 1 } // burned down task by task
    };
    std::process::exit(code);
}

/// Resolve the daemon profile roots for a local CLI auth operation. `login` and
/// `logout` write the credential file into the config root and nudge the LOCAL
/// daemon, so — like `run` — they honor a `qbzd.toml` `data_root` override while
/// keeping the config root at its XDG default.
fn login_roots() -> paths::ProfileRoots {
    let bootstrap = paths::ProfileRoots::resolve(None, None);
    let cfg_path = bootstrap.config.join("qbzd.toml");
    let data_root = config::QbzdConfig::load(&cfg_path)
        .ok()
        .and_then(|(c, _)| c.data_root);
    paths::ProfileRoots::resolve(None, data_root.as_deref().map(std::path::Path::new))
}
