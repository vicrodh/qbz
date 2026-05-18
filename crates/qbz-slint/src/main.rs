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
//! system-browser OAuth, saved-session restore, and a real Discover /
//! Home view fed by the Qobuz discover index with cached artwork.

slint::include_modules!();

mod adapter;
mod album;
mod artwork;
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

/// Reveal the shell and load the Discover / Home view with real data,
/// then kick off cached artwork downloads.
async fn enter_shell(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    image_cache: artwork::ImageCache,
    user_name: String,
) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        w.global::<SessionState>().set_user_name(user_name.into());
        w.global::<HomeState>().set_loading(true);
        w.set_screen(AppScreen::Shell);
    });

    match home::load_home(&runtime).await {
        Ok(sections) => {
            // Collect artwork jobs before the data is consumed by
            // apply_sections.
            let jobs: Vec<artwork::ArtworkJob> = sections
                .iter()
                .enumerate()
                .flat_map(|(section_idx, section)| {
                    section
                        .albums
                        .iter()
                        .enumerate()
                        .filter_map(move |(album_idx, card)| {
                            if card.artwork_url.is_empty() {
                                None
                            } else {
                                Some(artwork::ArtworkJob {
                                    section_idx,
                                    album_idx,
                                    url: card.artwork_url.clone(),
                                })
                            }
                        })
                })
                .collect();
            let weak_for_artwork = weak.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                home::apply_sections(&w, sections);
                w.global::<HomeState>().set_loading(false);
            });
            artwork::spawn_loads(jobs, weak_for_artwork, image_cache);
        }
        Err(e) => {
            log::error!("[qbz-slint] discover load failed: {e}");
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<HomeState>().set_loading(false);
            });
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let tokio_rt = tokio::runtime::Runtime::new()?;
    let _enter = tokio_rt.enter();

    let window = AppWindow::new()?;
    let app_runtime = Arc::new(AppRuntime::new(SlintAdapter::new(window.as_weak())));

    // Shared QBZ image cache for album artwork; trim it on startup.
    let image_cache = artwork::open_cache();
    artwork::spawn_evict(image_cache.clone());

    // Startup: initialize the core, then try to restore a saved session.
    // A valid saved token jumps straight to the shell; otherwise the
    // login screen stays.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        tokio_rt.spawn(async move {
            if let Err(e) = runtime.init().await {
                log::error!("[qbz-slint] core init failed: {e}");
            }
            match auth::restore_saved_session(&runtime).await {
                Ok(Some((user_id, user_name))) => {
                    log::info!("[qbz-slint] session restored for user {user_id}");
                    enter_shell(runtime, weak, image_cache, user_name).await;
                }
                Ok(None) => {
                    log::info!("[qbz-slint] no saved session — showing login");
                    let _ = weak.upgrade_in_event_loop(|w| w.set_screen(AppScreen::Login));
                }
                Err(e) => {
                    log::error!("[qbz-slint] session restore failed: {e}");
                    let _ = weak.upgrade_in_event_loop(|w| w.set_screen(AppScreen::Login));
                }
            }
        });
    }

    // Sign in via the system browser → real OAuth → shell.
    // "Sign in via Browser" and "Use your system browser instead" are the
    // same flow in the MVP (the in-app webview path is intentionally absent).
    let on_browser_login = {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        move || {
            let runtime = runtime.clone();
            let weak = weak.clone();
            let image_cache = image_cache.clone();
            handle.spawn(async move {
                match auth::login_via_system_browser(&runtime).await {
                    Ok((user_id, user_name)) => {
                        log::info!("[qbz-slint] authenticated as user {user_id}");
                        enter_shell(runtime, weak, image_cache, user_name).await;
                    }
                    Err(e) => log::error!("[qbz-slint] sign-in failed: {e}"),
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

    // Open an album: load it, show the album view, then fetch its artwork.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.on_open_album(move |album_id| {
            let runtime = runtime.clone();
            let weak = weak.clone();
            let image_cache = image_cache.clone();
            let album_id = album_id.to_string();
            handle.spawn(async move {
                let _ = weak.upgrade_in_event_loop(|w| {
                    album::reset_album(&w);
                    w.global::<NavState>().set_showing_album(true);
                });
                match album::load_album(&runtime, &album_id).await {
                    Ok(data) => {
                        let artwork_url = data.artwork_url.clone();
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            album::apply_album(&w, data);
                            w.global::<AlbumState>().set_loading(false);
                        });
                        if !artwork_url.is_empty() {
                            if let Some((pixels, width, height)) =
                                artwork::fetch_and_decode(&artwork_url, &image_cache, 448).await
                            {
                                let _ = weak.upgrade_in_event_loop(move |w| {
                                    album::apply_artwork(&w, &pixels, width, height);
                                });
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("[qbz-slint] album load failed: {e}");
                        let _ = weak.upgrade_in_event_loop(|w| {
                            w.global::<AlbumState>().set_loading(false);
                        });
                    }
                }
            });
        });
    }

    // Log out: clear the session and return to the login screen.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.on_logout(move || {
            let runtime = runtime.clone();
            let weak = weak.clone();
            handle.spawn(async move {
                if let Err(e) = auth::logout(&runtime).await {
                    log::error!("[qbz-slint] logout failed: {e}");
                }
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<NavState>().set_showing_album(false);
                    w.global::<SessionState>().set_user_name("".into());
                    w.set_screen(AppScreen::Login);
                });
            });
        });
    }

    window.on_close_app(|| {
        log::info!("[qbz-slint] closing");
        let _ = slint::quit_event_loop();
    });

    window.on_open_tos(|| {
        dispatch(AppCommand::OpenTermsOfService);
        if let Err(e) = open::that(QOBUZ_TOS_URL) {
            log::error!("[qbz-slint] failed to open Terms of Service: {e}");
        }
    });

    log::info!("[qbz-slint] window ready");
    window.run()?;
    Ok(())
}
