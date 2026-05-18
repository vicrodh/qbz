//! QBZ Slint MVP binary.
//!
//! A native Slint front end for QBZ built on the framework-agnostic
//! `qbz-app` / `qbz-core` stack — no Tauri, no WebView. See the MVP ADR
//! (`qbz-nix-docs/qbz-adr/qbz_slint_functional_poc_adr.md`).
//!
//! Lives only on the private `slint-mvp` branch (ADR-007). The Slint UI
//! tree is compiled from `ui/app.slint` by `build.rs`; `include_modules!`
//! pulls in the generated Rust bindings.
//!
//! Status: foundation tokens, login screen, app shell, functional
//! system-browser OAuth, and a real Discover / Home view fed by the
//! Qobuz discover index.

slint::include_modules!();

mod adapter;
mod auth;
mod commands;
mod home;

use std::sync::Arc;

use adapter::SlintAdapter;
use commands::AppCommand;
use qbz_app::shell::AppRuntime;

/// Login Terms-of-Service link target.
const QOBUZ_TOS_URL: &str = "https://www.qobuz.com/us-en/legal/terms";

fn dispatch(command: AppCommand) {
    log::info!("[qbz-slint] AppCommand::{} dispatched", command.id());
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let tokio_rt = tokio::runtime::Runtime::new()?;

    let window = AppWindow::new()?;
    let app_runtime = Arc::new(AppRuntime::new(SlintAdapter::new(window.as_weak())));

    // Extract Qobuz bundle tokens in the background so OAuth is ready
    // by the time the user signs in.
    {
        let runtime = app_runtime.clone();
        tokio_rt.spawn(async move {
            if let Err(e) = runtime.init().await {
                log::error!("[qbz-slint] core init failed: {e}");
            }
        });
    }

    // Sign in via the system browser → real OAuth → Discover/Home.
    // "Sign in via Browser" and "Use your system browser instead" are the
    // same flow in the MVP (the in-app webview path is intentionally absent).
    let on_browser_login = {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        move || {
            let runtime = runtime.clone();
            let weak = weak.clone();
            handle.spawn(async move {
                let outcome = match auth::login_via_system_browser(&runtime).await {
                    Ok(outcome) => outcome,
                    Err(e) => {
                        log::error!("[qbz-slint] sign-in failed: {e}");
                        return;
                    }
                };
                log::info!("[qbz-slint] authenticated as user {}", outcome.user_id);

                let greeting = home::greeting(&outcome.display_name);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let state = w.global::<HomeState>();
                    state.set_greeting(greeting.into());
                    state.set_loading(true);
                    w.set_screen(AppScreen::Shell);
                });

                match home::load_home(&runtime).await {
                    Ok(sections) => {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            home::apply_sections(&w, sections);
                            w.global::<HomeState>().set_loading(false);
                        });
                    }
                    Err(e) => {
                        log::error!("[qbz-slint] discover load failed: {e}");
                        let _ = weak.upgrade_in_event_loop(|w| {
                            w.global::<HomeState>().set_loading(false);
                        });
                    }
                }
            });
        }
    };

    {
        let login = on_browser_login.clone();
        window.on_sign_in_via_browser(move || {
            dispatch(AppCommand::SignInViaBrowser);
            login();
        });
    }
    {
        let login = on_browser_login.clone();
        window.on_use_system_browser(move || {
            dispatch(AppCommand::UseSystemBrowser);
            login();
        });
    }

    // Offline: activate an offline-only session, then show the shell.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.on_start_offline(move || {
            dispatch(AppCommand::StartOffline);
            let runtime = runtime.clone();
            let weak = weak.clone();
            handle.spawn(async move {
                match runtime.activate_offline().await {
                    Ok(()) => {
                        let _ = weak.upgrade_in_event_loop(|w| w.set_screen(AppScreen::Shell));
                    }
                    Err(e) => log::error!("[qbz-slint] offline start failed: {e}"),
                }
            });
        });
    }

    window.on_open_tos(|| {
        dispatch(AppCommand::OpenTermsOfService);
        if let Err(e) = open::that(QOBUZ_TOS_URL) {
            log::error!("[qbz-slint] failed to open Terms of Service: {e}");
        }
    });

    log::info!("[qbz-slint] window ready — login screen");
    window.run()?;
    Ok(())
}
