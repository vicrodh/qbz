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
mod artist;
mod artwork;
mod auth;
mod commands;
mod custom_artwork;
mod discovery_dismiss;
mod home;
mod label;
mod musician;
mod nav;
mod play_history;
mod strip_html;
mod playback;
mod queue;
mod recently;
mod search;
mod settings;
mod ui_prefs;

use std::sync::Arc;

use slint::Model;

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
    settings_ctx: Arc<settings::SettingsCtx>,
    session: auth::SessionInfo,
) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        let state = w.global::<SessionState>();
        state.set_user_name(session.display_name.into());
        state.set_subscription(session.subscription.into());
        w.global::<HomeState>().set_loading(true);
        w.set_screen(AppScreen::Shell);
    });

    // Start the playback poll loop — it runs for the app lifetime,
    // ticking position/progress onto NowPlayingState and auto-advancing
    // the queue on track end. Safe to start once per shell entry.
    playback::start_poll_loop(runtime.clone(), weak.clone(), tokio::runtime::Handle::current());

    // Load Audio + Playback settings into the Settings page in the
    // background — store reads and device enumeration are blocking.
    {
        let settings_ctx = settings_ctx.clone();
        let weak = weak.clone();
        tokio::spawn(async move {
            match tokio::task::spawn_blocking(move || settings::load_snapshot(&settings_ctx)).await
            {
                Ok(snap) => {
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        settings::apply_snapshot(&w, snap);
                    });
                }
                Err(e) => log::error!("[qbz-slint] settings load task failed: {e}"),
            }
        });
    }

    match home::load_home(&runtime).await {
        Ok(data) => {
            // Collect artwork jobs before the data is consumed by apply_home.
            let mut jobs: Vec<artwork::ArtworkJob> = data
                .sections
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
                                    target: artwork::ArtworkTarget::Section {
                                        section_idx,
                                        album_idx,
                                    },
                                    url: card.artwork_url.clone(),
                                })
                            }
                        })
                })
                .collect();
            jobs.extend(data.popular.iter().enumerate().filter_map(|(idx, slim)| {
                if slim.artwork_url.is_empty() {
                    None
                } else {
                    Some(artwork::ArtworkJob {
                        target: artwork::ArtworkTarget::Popular { idx },
                        url: slim.artwork_url.clone(),
                    })
                }
            }));
            jobs.extend(data.recent.iter().enumerate().filter_map(|(idx, slim)| {
                if slim.artwork_url.is_empty() {
                    None
                } else {
                    Some(artwork::ArtworkJob {
                        target: artwork::ArtworkTarget::Recent { idx },
                        url: slim.artwork_url.clone(),
                    })
                }
            }));
            jobs.extend(
                data.recent_albums
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, card)| {
                        if card.artwork_url.is_empty() {
                            None
                        } else {
                            Some(artwork::ArtworkJob {
                                target: artwork::ArtworkTarget::RecentAlbum { idx },
                                url: card.artwork_url.clone(),
                            })
                        }
                    }),
            );
            let weak_for_artwork = weak.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                home::apply_home(&w, data);
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

/// Push the navigation history flags onto `NavState`. UI thread only.
fn update_nav_flags(window: &AppWindow) {
    let state = window.global::<NavState>();
    state.set_can_back(nav::can_back());
    state.set_can_forward(nav::can_forward());
}

/// Load an album and show the album view, then fetch its artwork. Shared
/// by the `open-album` callback and by history back/forward.
fn navigate_album(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    album_id: String,
) {
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            album::reset_album(&w);
            w.global::<NavState>().set_view(ContentView::Album);
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
}

/// Load an artist page and show the artist view, then fetch the portrait.
/// Shared by the `open-artist` callback and by history back/forward.
fn navigate_artist(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    artist_id: String,
) {
    let artist_id_for_state = artist_id.clone();
    handle.spawn(async move {
        let id_for_apply = artist_id_for_state.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            artist::reset_artist(&w);
            artist::reset_network_sidebar(&w);
            w.global::<ArtistState>().set_id(id_for_apply.into());
            w.global::<NavState>().set_view(ContentView::Artist);
        });
        match artist::load_artist(&runtime, &artist_id).await {
            Ok(data) => {
                let artwork_url = data.artwork_url.clone();
                let jobs = artist::artwork_jobs(&data);
                let artist_name = data.name.clone();
                let similar_names_for_discovery: Vec<String> = data
                    .similar_artists
                    .iter()
                    .map(|s| s.name.clone())
                    .collect();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    artist::apply_artist(&w, data);
                    w.global::<ArtistState>().set_loading(false);
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());

                // Network sidebar — kick the MB enrichment off in
                // parallel with artwork. Origin shows a loading state
                // until the resolve + metadata calls return; on success
                // the resolved mbid is used to fetch relationships and
                // discovery candidates in sequence (the V2 cache, when
                // wired, will collapse repeat visits to a single shot).
                let runtime_mb = runtime.clone();
                let weak_mb = weak.clone();
                tokio::spawn(async move {
                    let _ = weak_mb.upgrade_in_event_loop(|w| {
                        let state = w.global::<NetworkSidebarState>();
                        state.set_origin_loading(true);
                        state.set_relationships_loading(true);
                        state.set_discovery_loading(true);
                    });
                    match artist::load_mb_metadata(&runtime_mb, &artist_name).await {
                        Ok(Some(meta)) => {
                            let mbid = meta.mbid.clone();
                            let _ = weak_mb.upgrade_in_event_loop(move |w| {
                                artist::apply_mb_metadata(&w, meta);
                            });
                            match artist::load_mb_relationships(&runtime_mb, &mbid).await {
                                Ok(data) => {
                                    let _ = weak_mb.upgrade_in_event_loop(move |w| {
                                        artist::apply_mb_relationships(&w, data);
                                    });
                                }
                                Err(e) => {
                                    log::warn!("[qbz-slint] MB relationships failed: {e}");
                                    let _ = weak_mb.upgrade_in_event_loop(|w| {
                                        w.global::<NetworkSidebarState>()
                                            .set_relationships_loading(false);
                                    });
                                }
                            }
                            match artist::load_mb_discovery(
                                &runtime_mb,
                                &mbid,
                                &artist_name,
                                similar_names_for_discovery,
                            )
                            .await
                            {
                                Ok(disc) => {
                                    let _ = weak_mb.upgrade_in_event_loop(move |w| {
                                        artist::apply_mb_discovery(&w, disc);
                                    });
                                }
                                Err(e) => {
                                    log::warn!("[qbz-slint] MB discovery failed: {e}");
                                    let _ = weak_mb.upgrade_in_event_loop(|w| {
                                        w.global::<NetworkSidebarState>()
                                            .set_discovery_loading(false);
                                    });
                                }
                            }
                        }
                        Ok(None) => {
                            let _ = weak_mb.upgrade_in_event_loop(|w| {
                                artist::apply_mb_unavailable(&w);
                            });
                        }
                        Err(e) => {
                            log::warn!("[qbz-slint] MB metadata load failed: {e}");
                            let _ = weak_mb.upgrade_in_event_loop(|w| {
                                artist::apply_mb_unavailable(&w);
                            });
                        }
                    }
                });

                if !artwork_url.is_empty() {
                    if let Some((pixels, width, height)) =
                        artwork::fetch_and_decode(&artwork_url, &image_cache, 440).await
                    {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            artist::apply_artwork(&w, &pixels, width, height);
                        });
                    }
                }
            }
            Err(e) => {
                log::error!("[qbz-slint] artist load failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<ArtistState>().set_loading(false);
                });
            }
        }
    });
}

thread_local! {
    /// Debounce timer for the header live search — restarted on every
    /// keystroke, fires the search 300 ms after typing stops.
    static SEARCH_DEBOUNCE: slint::Timer = slint::Timer::default();
}

/// Run a search and show the results view. Shared by the search-submit
/// callback, the live-search debounce, and history back/forward.
fn navigate_search(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    query: String,
) {
    // Capture a version so a slow, stale load cannot overwrite a newer
    // search's results (the user kept typing).
    let version = search::next_search_version();
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            search::reset_search(&w);
            w.global::<NavState>().set_view(ContentView::Search);
        });
        match search::load_search(&runtime, &query).await {
            Ok(data) => {
                let jobs = search::artwork_jobs(&data);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    if search::is_current_version(version) {
                        search::apply_search(&w, data);
                        w.global::<SearchState>().set_loading(false);
                    }
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache);
            }
            Err(e) => {
                log::error!("[qbz-slint] search load failed: {e}");
                let _ = weak.upgrade_in_event_loop(move |w| {
                    if search::is_current_version(version) {
                        w.global::<SearchState>().set_loading(false);
                    }
                });
            }
        }
    });
}

/// Apply a history entry — set the view and re-load entity pages.
fn apply_entry(
    entry: nav::NavEntry,
    runtime: &Arc<AppRuntime<SlintAdapter>>,
    weak: &slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: &artwork::ImageCache,
) {
    match entry {
        nav::NavEntry::Home => {
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<NavState>().set_view(ContentView::Home);
            });
        }
        nav::NavEntry::Settings => {
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<NavState>().set_view(ContentView::Settings);
            });
        }
        nav::NavEntry::Album(id) => {
            navigate_album(runtime.clone(), weak.clone(), handle, image_cache.clone(), id);
        }
        nav::NavEntry::Artist(id) => {
            navigate_artist(runtime.clone(), weak.clone(), handle, image_cache.clone(), id);
        }
        nav::NavEntry::Search(query) => {
            navigate_search(runtime.clone(), weak.clone(), handle, image_cache.clone(), query);
        }
        nav::NavEntry::Musician { name, role } => {
            navigate_musician(
                runtime.clone(),
                weak.clone(),
                handle,
                image_cache.clone(),
                name,
                role,
            );
        }
        nav::NavEntry::Label { id, name } => {
            navigate_label(runtime.clone(), weak.clone(), handle, image_cache.clone(), id, name);
        }
    }
}

/// Open a LabelReleasesView for `label_id`. Fetches the label header
/// + first album page, then the header image.
fn navigate_label(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    label_id: u64,
    name: String,
) {
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            label::reset_label(&w);
            w.global::<NavState>().set_view(ContentView::Label);
        });
        match label::load_label(&runtime, label_id, &name).await {
            Ok(data) => {
                let jobs = label::artwork_jobs(&data);
                let image_url = data.image_url.clone();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    label::apply_label(&w, data);
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
                if !image_url.is_empty() {
                    if let Some((pixels, width, height)) =
                        artwork::fetch_and_decode(&image_url, &image_cache, 240).await
                    {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            label::apply_image(&w, &pixels, width, height);
                        });
                    }
                }
            }
            Err(e) => {
                log::error!("[qbz-slint] label load failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<LabelState>().set_loading(false);
                });
            }
        }
    });
}

/// Open a MusicianPageView for `name + role`. Routes to the artist
/// page instead when the resolved musician has a Confirmed Qobuz
/// match (Tauri's `confidence === 'confirmed'` shortcut). Fetches
/// the first page of appearances inline; subsequent pages come
/// through the MusicianActions::load-more handler.
fn navigate_musician(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    name: String,
    role: String,
) {
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            musician::reset_musician(&w);
            w.global::<NavState>().set_view(ContentView::Musician);
        });
        match musician::load_musician(&runtime, &name, &role).await {
            Ok(data) => {
                let jobs = musician::artwork_jobs(&data);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    musician::apply_musician(&w, data);
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
            }
            Err(e) => {
                log::error!("[qbz-slint] musician load failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<MusicianState>().set_loading(false);
                });
            }
        }
    });
}

/// Resolve the desktop environment's UI font family. Reads the KDE
/// Plasma general font from `kdeglobals` (`[General] font=`), whose
/// value is a Qt font string `Family,pointSize,...` — only the family
/// is taken. Returns `None` off KDE or when the key is absent, so the
/// caller can fall back to the Slint default.
fn system_font_family() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let path = std::path::Path::new(&home).join(".config/kdeglobals");
    let content = std::fs::read_to_string(path).ok()?;
    let mut in_general = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_general = line == "[General]";
            continue;
        }
        if in_general {
            if let Some(rest) = line.strip_prefix("font=") {
                let family = rest.split(',').next().unwrap_or("").trim();
                if !family.is_empty() {
                    return Some(family.to_string());
                }
            }
        }
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let tokio_rt = tokio::runtime::Runtime::new()?;
    let _enter = tokio_rt.enter();

    let window = AppWindow::new()?;
    // FONT TEST (slint-mvp): render with bundled Inter 18pt. Inter is a
    // clean, screen-tuned UI face; combined with the femtovg #5177/#11335
    // text fixes this is the candidate for the final look. Flip
    // `FONT_TEST_INTER` to false to fall back to the KDE system font.
    const FONT_TEST_INTER: bool = true;
    if FONT_TEST_INTER {
        log::info!("[qbz-slint] font test: using bundled Inter 18pt");
        window.set_system_font("Inter 18pt".into());
    } else if let Some(font) = system_font_family() {
        log::info!("[qbz-slint] using system font: {font}");
        window.set_system_font(font.into());
    }
    let app_runtime = Arc::new(AppRuntime::new(SlintAdapter::new(window.as_weak())));

    // MusicBrainz cache — opens a SQLite store at
    // <data-dir>/qbz/cache/musicbrainz_cache.db so artist metadata
    // and relationships persist across sessions (matches Tauri's
    // MusicBrainzCache init path). Failure to open just degrades to
    // direct network calls — the methods skip the cache when none
    // is set.
    if let Some(data_dir) = dirs::data_dir() {
        let cache_dir = data_dir.join("qbz").join("cache");
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            log::warn!("[qbz-slint] MB cache dir create failed: {e}");
        } else {
            let db_path = cache_dir.join("musicbrainz_cache.db");
            match qbz_integrations::musicbrainz::cache::MusicBrainzCache::new(&db_path) {
                Ok(cache) => {
                    app_runtime.core().set_musicbrainz_cache(cache);
                    log::info!("[qbz-slint] MB cache opened at {db_path:?}");
                }
                Err(e) => log::warn!("[qbz-slint] MB cache open failed: {e}"),
            }
        }
    }

    // Shared QBZ image cache for album artwork; trim it on startup.
    let image_cache = artwork::open_cache();
    artwork::spawn_evict(image_cache.clone());
    // Publish it so the playback controller can resolve now-playing /
    // queue cover art without the cache being threaded through.
    artwork::set_shared_cache(image_cache.clone());

    // Audio + Playback settings stores, opened once for the app lifetime.
    let settings_ctx = settings::SettingsCtx::open();

    // Startup: initialize the core, then try to restore a saved session.
    // A valid saved token jumps straight to the shell; otherwise the
    // login screen stays.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        let settings_ctx = settings_ctx.clone();
        tokio_rt.spawn(async move {
            if let Err(e) = runtime.init().await {
                log::error!("[qbz-slint] core init failed: {e}");
            }
            match auth::restore_saved_session(&runtime).await {
                Ok(Some(session)) => {
                    log::info!(
                        "[qbz-slint] session restored for user {}",
                        session.user_id
                    );
                    enter_shell(runtime, weak, image_cache, settings_ctx, session).await;
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
        let settings_ctx = settings_ctx.clone();
        move || {
            let runtime = runtime.clone();
            let weak = weak.clone();
            let image_cache = image_cache.clone();
            let settings_ctx = settings_ctx.clone();
            handle.spawn(async move {
                match auth::login_via_system_browser(&runtime).await {
                    Ok(session) => {
                        log::info!(
                            "[qbz-slint] authenticated as user {}",
                            session.user_id
                        );
                        enter_shell(runtime, weak, image_cache, settings_ctx, session).await;
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

    // Open an album: record history, then load and show it.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.on_open_album(move |album_id| {
            let album_id = album_id.to_string();
            nav::record(nav::NavEntry::Album(album_id.clone()));
            navigate_album(
                runtime.clone(),
                weak.clone(),
                &handle,
                image_cache.clone(),
                album_id,
            );
            if let Some(w) = weak.upgrade() {
                update_nav_flags(&w);
            }
        });
    }

    // Open an artist: record history, then load and show the page.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.on_open_artist(move |artist_id| {
            let artist_id = artist_id.to_string();
            nav::record(nav::NavEntry::Artist(artist_id.clone()));
            navigate_artist(
                runtime.clone(),
                weak.clone(),
                &handle,
                image_cache.clone(),
                artist_id,
            );
            if let Some(w) = weak.upgrade() {
                update_nav_flags(&w);
            }
        });
    }

    // Submit search (Enter): record history and show the results page.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<SearchActions>().on_submit(move |query| {
            let q = query.trim().to_string();
            if q.len() < 2 {
                return;
            }
            SEARCH_DEBOUNCE.with(|t| t.stop());
            nav::push_or_replace_search(q.clone());
            navigate_search(runtime.clone(), weak.clone(), &handle, image_cache.clone(), q);
            if let Some(w) = weak.upgrade() {
                update_nav_flags(&w);
            }
        });
    }

    // Live search: debounce 300 ms, minimum 2 characters. Does not record
    // history (per-keystroke entries would pollute the back stack).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<SearchActions>().on_live(move |query| {
            let q = query.trim().to_string();
            if q.len() < 2 {
                SEARCH_DEBOUNCE.with(|t| t.stop());
                return;
            }
            let runtime = runtime.clone();
            let weak = weak.clone();
            let handle = handle.clone();
            let image_cache = image_cache.clone();
            SEARCH_DEBOUNCE.with(|t| {
                t.start(
                    slint::TimerMode::SingleShot,
                    std::time::Duration::from_millis(300),
                    move || {
                        // Record (or replace) the Search history entry so
                        // back/forward returns to this search instead of
                        // skipping past it.
                        nav::push_or_replace_search(q.clone());
                        navigate_search(
                            runtime.clone(),
                            weak.clone(),
                            &handle,
                            image_cache.clone(),
                            q.clone(),
                        );
                        if let Some(w) = weak.upgrade() {
                            update_nav_flags(&w);
                        }
                    },
                );
            });
        });
    }

    // Switch search results tab. search_all already loaded every
    // category, so this only changes which one the view renders.
    {
        let weak = window.as_weak();
        window.global::<SearchActions>().on_tab_changed(move |tab| {
            if let Some(w) = weak.upgrade() {
                w.global::<SearchState>().set_tab(tab);
            }
        });
    }

    // Load more results for the active per-type tab. The offset is the
    // count already loaded into that category's list.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<SearchActions>().on_load_more(move |tab| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(category) = search::category_for_tab(tab) else {
                return;
            };
            let st = w.global::<SearchState>();
            let query = st.get_query().to_string();
            let filter = search::search_type_for_filter(st.get_filter_index());
            let offset = match category {
                search::SearchCategory::Albums => st.get_albums().row_count(),
                search::SearchCategory::Tracks => st.get_tracks().row_count(),
                search::SearchCategory::Artists => st.get_artists().row_count(),
                search::SearchCategory::Playlists => st.get_playlists().row_count(),
            } as u32;
            let runtime = runtime.clone();
            let weak = weak.clone();
            let image_cache = image_cache.clone();
            handle.spawn(async move {
                match search::load_more(&runtime, &query, category, filter, offset).await {
                    Ok(more) => {
                        let jobs = search::artwork_jobs_for_more(&more, offset as usize);
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            search::append_results(&w, more);
                        });
                        artwork::spawn_loads(jobs, weak.clone(), image_cache);
                    }
                    Err(e) => log::error!("[qbz-slint] search load-more failed: {e}"),
                }
            });
        });
    }

    // Change the searchType filter: re-query the three filterable
    // categories (albums / tracks / artists) and replace their lists, so
    // the filter takes effect on every tab including All.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<SearchActions>().on_filter_changed(move |index| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let st = w.global::<SearchState>();
            st.set_filter_index(index);
            let query = st.get_query().to_string();
            if query.trim().is_empty() {
                return;
            }
            let search_type = search::search_type_for_filter(index);
            let runtime = runtime.clone();
            let weak = weak.clone();
            let image_cache = image_cache.clone();
            handle.spawn(async move {
                for category in [
                    search::SearchCategory::Albums,
                    search::SearchCategory::Tracks,
                    search::SearchCategory::Artists,
                ] {
                    match search::load_more(&runtime, &query, category, search_type.clone(), 0)
                        .await
                    {
                        Ok(more) => {
                            let jobs = search::artwork_jobs_for_more(&more, 0);
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                search::replace_category(&w, more);
                            });
                            artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
                        }
                        Err(e) => log::error!("[qbz-slint] search filter failed: {e}"),
                    }
                }
            });
        });
    }

    // History navigation — back / forward / settings, all recorded by the
    // nav module so the [<] [>] pair and the mouse buttons stay in sync.
    {
        let weak = window.as_weak();
        window.global::<NavState>().on_request_settings(move || {
            nav::record(nav::NavEntry::Settings);
            if let Some(w) = weak.upgrade() {
                w.global::<NavState>().set_view(ContentView::Settings);
                update_nav_flags(&w);
            }
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<NavState>().on_request_back(move || {
            if let Some(entry) = nav::go_back() {
                apply_entry(entry, &runtime, &weak, &handle, &image_cache);
            }
            if let Some(w) = weak.upgrade() {
                update_nav_flags(&w);
            }
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<NavState>().on_request_forward(move || {
            if let Some(entry) = nav::go_forward() {
                apply_entry(entry, &runtime, &weak, &handle, &image_cache);
            }
            if let Some(w) = weak.upgrade() {
                update_nav_flags(&w);
            }
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
                    w.global::<NavState>().set_view(ContentView::Home);
                    w.global::<SessionState>().set_user_name("".into());
                    w.set_screen(AppScreen::Login);
                });
            });
        });
    }

    // Settings — a toggle changed: persist it and apply audio ones to the
    // live player.
    {
        let runtime = app_runtime.clone();
        let settings_ctx = settings_ctx.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.on_settings_bool(move |key, value| {
            let runtime = runtime.clone();
            let settings_ctx = settings_ctx.clone();
            let weak = weak.clone();
            let key = key.to_string();
            handle.spawn(async move {
                settings::handle_bool(settings_ctx, runtime, weak, key, value).await;
            });
        });
    }

    // Settings — a dropdown changed: persist it, apply audio ones, and
    // re-enumerate devices on a backend switch.
    {
        let runtime = app_runtime.clone();
        let settings_ctx = settings_ctx.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.on_settings_select(move |key, index| {
            let runtime = runtime.clone();
            let settings_ctx = settings_ctx.clone();
            let weak = weak.clone();
            let key = key.to_string();
            let index = index.max(0) as usize;
            handle.spawn(async move {
                settings::handle_select(settings_ctx, runtime, weak, key, index).await;
            });
        });
    }

    // Settings — a slider changed (Initial Buffer Size): persist it and
    // reload the player settings.
    {
        let runtime = app_runtime.clone();
        let settings_ctx = settings_ctx.clone();
        let handle = tokio_rt.handle().clone();
        window.on_settings_slider(move |key, value| {
            let runtime = runtime.clone();
            let settings_ctx = settings_ctx.clone();
            let key = key.to_string();
            handle.spawn(async move {
                settings::handle_slider(&settings_ctx, &runtime, &key, value);
            });
        });
    }

    // Settings — Reset: restore Audio + Playback defaults, rebuild the
    // snapshot, and re-apply the audio settings to the player.
    {
        let runtime = app_runtime.clone();
        let settings_ctx = settings_ctx.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.on_settings_reset(move || {
            let runtime = runtime.clone();
            let settings_ctx = settings_ctx.clone();
            let weak = weak.clone();
            handle.spawn(async move {
                settings::handle_reset(settings_ctx, runtime, weak).await;
            });
        });
    }

    // Context-menu / overlay media actions — route play / queue actions
    // into the playback controller; favorite / download stay logged.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.on_media_action(move |kind, id, action| {
            let kind = kind.to_string();
            let id = id.to_string();
            let action = action.to_string();
            log::info!("[qbz-slint] media-action: kind={kind} id={id} action={action}");
            match (kind.as_str(), action.as_str()) {
                ("album", "play") => playback::play_album(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id,
                    0,
                ),
                ("track", "play") => {
                    if let Ok(track_id) = id.parse::<u64>() {
                        playback::play_track_now(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            track_id,
                        );
                    }
                }
                ("album", "queue") => playback::enqueue_album(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id,
                ),
                ("track", "queue") => {
                    if let Ok(track_id) = id.parse::<u64>() {
                        playback::enqueue_track(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            track_id,
                        );
                    }
                }
                ("album", "play-next") => playback::enqueue_album_next(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id,
                ),
                ("track", "play-next") => {
                    if let Ok(track_id) = id.parse::<u64>() {
                        playback::play_track_next(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            track_id,
                        );
                    }
                }
                ("artist", "play-top") => playback::play_artist_top_tracks(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id.clone(),
                ),
                ("artist", "follow") => {
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let artist_id = id.clone();
                    handle.spawn(async move {
                        match runtime.core().add_favorite("artist", &artist_id).await {
                            Ok(()) => {
                                let _ = weak.upgrade_in_event_loop(move |w| {
                                    search::mark_artist_followed(&w, &artist_id, true);
                                });
                            }
                            Err(e) => {
                                log::error!("[qbz-slint] follow artist failed: {e}");
                            }
                        }
                    });
                }
                ("track", "toggle-select") => {
                    // Multi-select foundation — flips `selected` on the
                    // matching row in ArtistState.top-tracks. Other
                    // track-row contexts (album, search) will be wired as
                    // selection lands there.
                    if let Some(w) = weak.upgrade() {
                        let model = w.global::<ArtistState>().get_top_tracks();
                        if let Some(vm) = model
                            .as_any()
                            .downcast_ref::<slint::VecModel<AlbumTrackItem>>()
                        {
                            for i in 0..vm.row_count() {
                                if let Some(mut item) = vm.row_data(i) {
                                    if item.id == id.as_str() {
                                        item.selected = !item.selected;
                                        vm.set_row_data(i, item);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        });
    }

    // Transport — wired through the NowPlayingState global callbacks.
    {
        let runtime = app_runtime.clone();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_toggle_play(move || {
                playback::toggle_play_pause(runtime.clone(), handle.clone());
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<NowPlayingState>().on_next(move || {
            playback::next(runtime.clone(), weak.clone(), handle.clone());
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<NowPlayingState>().on_previous(move || {
            playback::previous(runtime.clone(), weak.clone(), handle.clone());
        });
    }
    {
        let runtime = app_runtime.clone();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_seek(move |fraction| {
                playback::seek(runtime.clone(), handle.clone(), fraction);
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_set_volume(move |fraction| {
                playback::set_volume(runtime.clone(), weak.clone(), handle.clone(), fraction);
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_toggle_mute(move || {
                playback::toggle_mute(runtime.clone(), weak.clone(), handle.clone());
            });
    }

    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_toggle_shuffle(move || {
                playback::toggle_shuffle(runtime.clone(), weak.clone(), handle.clone());
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_cycle_repeat(move || {
                playback::cycle_repeat(runtime.clone(), weak.clone(), handle.clone());
            });
    }

    // Queue sidebar — build the controller and wire every callback.
    {
        let controller = queue::QueueController::new(
            app_runtime.clone(),
            window.as_weak(),
            tokio_rt.handle().clone(),
            settings_ctx.playback_prefs(),
        );
        // Publish it so the playback paths refresh the sidebar after every
        // queue mutation (play / skip / auto-advance / enqueue).
        playback::set_queue_controller(controller.clone());

        let qs = window.global::<QueueState>();
        {
            let c = controller.clone();
            qs.on_play_upcoming(move |index| c.play_upcoming(index.max(0) as usize));
        }
        {
            let c = controller.clone();
            qs.on_play_history(move |index| c.play_history(index.max(0) as usize));
        }
        {
            let c = controller.clone();
            qs.on_remove_upcoming(move |index| c.remove_upcoming(index.max(0) as usize));
        }
        {
            let c = controller.clone();
            qs.on_clear_queue(move || c.clear());
        }
        {
            let c = controller.clone();
            qs.on_toggle_now_playing_favorite(move || c.toggle_favorite());
        }
        {
            let c = controller.clone();
            qs.on_save_as_playlist(move || c.save_as_playlist());
        }
        {
            let c = controller.clone();
            qs.on_toggle_infinite_play(move || c.toggle_infinite_play());
        }
        {
            let c = controller.clone();
            let weak = window.as_weak();
            qs.on_search_changed(move || {
                let query = weak
                    .upgrade()
                    .map(|w| w.global::<QueueState>().get_search_query().to_string())
                    .unwrap_or_default();
                c.search_changed(query);
            });
        }
        {
            let c = controller.clone();
            qs.on_prev_page(move || c.prev_page());
        }
        {
            let c = controller.clone();
            qs.on_next_page(move || c.next_page());
        }
        {
            let c = controller.clone();
            qs.on_set_tab(move |tab| c.set_tab(tab));
        }
        {
            let c = controller.clone();
            // On open, also re-pull favorites so the heart is accurate.
            qs.on_panel_opened(move || c.refresh_with_favorites());
        }
    }

    // Album track search — client-side filter, no backend round-trip.
    {
        let weak = window.as_weak();
        window
            .global::<AlbumActions>()
            .on_search(move |query| {
                if let Some(w) = weak.upgrade() {
                    album::filter_tracks(&w, query.as_str());
                }
            });
    }

    // Artist in-page search — client-side filter over Popular Tracks
    // and every release-section album.
    {
        let weak = window.as_weak();
        window
            .global::<ArtistActions>()
            .on_search(move |query| {
                if let Some(w) = weak.upgrade() {
                    artist::filter_artist(&w, query.as_str());
                }
            });
    }

    // Artist network sidebar — no persistence. Default open, user can
    // close per-session, and reset_network_sidebar reopens it on every
    // artist navigation. The toggle callback stays a no-op on the
    // Rust side — Slint already flips NetworkSidebarState.open
    // directly in the click handler.
    window
        .global::<NetworkSidebarActions>()
        .on_toggle(|| {});

    // Network sidebar — typed click callbacks. Each delivers the
    // minimum payload the future target views (ArtistsByLocation,
    // LabelReleases, MusicianPage) will need. Logged-only until those
    // views land in Slint.
    window
        .global::<NetworkSidebarActions>()
        .on_location_clicked(|mbid| {
            log::info!("[qbz-slint] network sidebar: location clicked for mbid={mbid}");
        });
    // Label click — open LabelReleasesView.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<NetworkSidebarActions>()
            .on_label_clicked(move |id, name| {
                let Ok(label_id) = id.parse::<u64>() else {
                    log::warn!("[qbz-slint] label clicked: invalid id {id}");
                    return;
                };
                let name = name.to_string();
                nav::record(nav::NavEntry::Label {
                    id: label_id,
                    name: name.clone(),
                });
                navigate_label(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    label_id,
                    name,
                );
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
            });
    }
    // artist-clicked actually navigates — the target view (artist page)
    // already exists in Slint, unlike LabelReleases / ArtistsByLocation /
    // MusicianPage. Same flow as the top-level on_open_artist handler.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<NetworkSidebarActions>()
            .on_artist_clicked(move |id| {
                let artist_id = id.to_string();
                nav::record(nav::NavEntry::Artist(artist_id.clone()));
                navigate_artist(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    artist_id,
                );
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
            });
    }
    // Musician click — resolve the (name, role) first; if Qobuz has
    // a confirmed exact match, jump straight to that artist's page.
    // Otherwise open MusicianPageView (Contextual / Weak / None).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<NetworkSidebarActions>()
            .on_musician_clicked(move |name, role| {
                let name = name.to_string();
                let role = role.to_string();
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                let image_cache = image_cache.clone();
                tokio::spawn(async move {
                    let resolved =
                        runtime.core().musicbrainz_resolve_musician(&name, &role).await;
                    match resolved {
                        Ok(r) if matches!(
                            r.confidence,
                            qbz_integrations::musicbrainz::MusicianConfidence::Confirmed
                        ) =>
                        {
                            if let Some(id) = r.qobuz_artist_id {
                                let artist_id = id.to_string();
                                let weak2 = weak.clone();
                                let _ = weak.clone().upgrade_in_event_loop(move |_| {
                                    nav::record(nav::NavEntry::Artist(artist_id.clone()));
                                });
                                navigate_artist(
                                    runtime,
                                    weak2,
                                    &handle,
                                    image_cache,
                                    id.to_string(),
                                );
                                return;
                            }
                            log::warn!(
                                "[qbz-slint] musician confirmed but no qobuz id"
                            );
                        }
                        Ok(_) => {
                            // Fall through to MusicianPageView for
                            // Contextual / Weak / None.
                        }
                        Err(e) => {
                            log::warn!("[qbz-slint] musician resolve failed: {e}");
                        }
                    }
                    nav::record(nav::NavEntry::Musician {
                        name: name.clone(),
                        role: role.clone(),
                    });
                    navigate_musician(runtime, weak, &handle, image_cache, name, role);
                });
            });
    }
    // discovery-dismissed — persist the rejection under the current
    // tag, then remove the row from the visible list.
    {
        let weak = window.as_weak();
        window
            .global::<NetworkSidebarActions>()
            .on_discovery_dismissed(move |mbid, name| {
                if let Some(w) = weak.upgrade() {
                    let tag = w
                        .global::<NetworkSidebarState>()
                        .get_discovery_tag()
                        .to_string()
                        .to_lowercase();
                    if !tag.is_empty() {
                        let normalized =
                            qbz_core::normalize_artist_name(name.as_str());
                        discovery_dismiss::dismiss(&tag, &normalized);
                    }
                    artist::remove_discovery_artist(&w, mbid.as_str());
                }
            });
    }

    // Musician appearances pagination — Load more in
    // MusicianPageView appends the next 20 albums onto the existing
    // grid.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<MusicianActions>()
            .on_load_more(move || {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let state = w.global::<MusicianState>();
                let name = state.get_name().to_string();
                let role = state.get_role().to_string();
                let offset = state.get_appearances().row_count() as u32;
                if name.is_empty() {
                    return;
                }
                state.set_load_more_loading(true);
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                let image_cache = image_cache.clone();
                handle.clone().spawn(async move {
                    match musician::load_more_appearances(&runtime, &name, &role, offset).await {
                        Ok((data, total)) => {
                            let jobs: Vec<artwork::ArtworkJob> = data
                                .iter()
                                .enumerate()
                                .filter(|(_, a)| !a.artwork_url.is_empty())
                                .map(|(i, a)| artwork::ArtworkJob {
                                    url: a.artwork_url.clone(),
                                    target: artwork::ArtworkTarget::MusicianAppearance {
                                        index: offset as usize + i,
                                    },
                                })
                                .collect();
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                musician::append_appearances(&w, data, total);
                            });
                            artwork::spawn_loads(jobs, weak, image_cache);
                        }
                        Err(e) => {
                            log::error!("[qbz-slint] musician load-more failed: {e}");
                            let _ = weak.upgrade_in_event_loop(|w| {
                                w.global::<MusicianState>().set_load_more_loading(false);
                            });
                        }
                    }
                });
            });
    }

    // Label album pagination — Load more in LabelReleasesView
    // appends the next page onto the grid.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<LabelActions>()
            .on_load_more(move || {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let state = w.global::<LabelState>();
                let Ok(label_id) = state.get_id().to_string().parse::<u64>() else {
                    return;
                };
                let offset = state.get_albums().row_count() as u32;
                state.set_load_more_loading(true);
                let runtime = runtime.clone();
                let weak = weak.clone();
                let image_cache = image_cache.clone();
                handle.spawn(async move {
                    match label::load_more_albums(&runtime, label_id, offset).await {
                        Ok((data, total)) => {
                            let jobs: Vec<artwork::ArtworkJob> = data
                                .iter()
                                .enumerate()
                                .filter(|(_, a)| !a.artwork_url.is_empty())
                                .map(|(i, a)| artwork::ArtworkJob {
                                    url: a.artwork_url.clone(),
                                    target: artwork::ArtworkTarget::LabelAlbum {
                                        index: offset as usize + i,
                                    },
                                })
                                .collect();
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                label::append_albums(&w, data, total);
                            });
                            artwork::spawn_loads(jobs, weak, image_cache);
                        }
                        Err(e) => {
                            log::error!("[qbz-slint] label load-more failed: {e}");
                            let _ = weak.upgrade_in_event_loop(|w| {
                                w.global::<LabelState>().set_load_more_loading(false);
                            });
                        }
                    }
                });
            });
    }

    // Artwork right-click menu wiring — Open in browser / Save as /
    // Add custom / Remove custom. Mirrors the v2_library_* + native
    // dialog flow Tauri uses on artist portraits + album covers.
    window
        .global::<ArtworkActions>()
        .on_open_in_browser(|url| {
            if url.is_empty() {
                return;
            }
            if let Err(e) = open::that(url.as_str()) {
                log::error!("[qbz-slint] artwork open-in-browser failed: {e}");
            }
        });
    {
        let handle = tokio_rt.handle().clone();
        window
            .global::<ArtworkActions>()
            .on_save_as(move |url, default_name| {
                if url.is_empty() {
                    return;
                }
                let url = url.to_string();
                let default = default_name.to_string();
                handle.spawn(async move {
                    let Some(dest) = rfd::AsyncFileDialog::new()
                        .set_file_name(&default)
                        .add_filter("Images", &["jpg", "jpeg", "png"])
                        .save_file()
                        .await
                    else {
                        return;
                    };
                    let bytes = match reqwest::get(&url).await {
                        Ok(resp) => match resp.bytes().await {
                            Ok(b) => b,
                            Err(e) => {
                                log::error!(
                                    "[qbz-slint] artwork save-as fetch body: {e}"
                                );
                                return;
                            }
                        },
                        Err(e) => {
                            log::error!("[qbz-slint] artwork save-as request: {e}");
                            return;
                        }
                    };
                    if let Err(e) = tokio::fs::write(dest.path(), &bytes).await {
                        log::error!("[qbz-slint] artwork save-as write: {e}");
                    }
                });
            });
    }
    {
        let handle = tokio_rt.handle().clone();
        let weak = window.as_weak();
        window
            .global::<ArtworkActions>()
            .on_add_custom(move |kind, key| {
                let kind = kind.to_string();
                let key = key.to_string();
                let weak = weak.clone();
                handle.spawn(async move {
                    let Some(file) = rfd::AsyncFileDialog::new()
                        .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                        .pick_file()
                        .await
                    else {
                        return;
                    };
                    let path = file.path().to_string_lossy().into_owned();
                    match kind.as_str() {
                        "artist" => {
                            custom_artwork::set_artist_image(&key, &path);
                            let _ = weak.upgrade_in_event_loop(|w| {
                                w.global::<ArtistState>().set_has_custom_image(true);
                            });
                        }
                        "album" => {
                            custom_artwork::set_album_cover(&key, &path);
                            let _ = weak.upgrade_in_event_loop(|w| {
                                w.global::<AlbumState>().set_has_custom_cover(true);
                            });
                        }
                        _ => log::warn!(
                            "[qbz-slint] artwork add-custom: unknown kind {kind}"
                        ),
                    }
                });
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<ArtworkActions>()
            .on_remove_custom(move |kind, key| {
                match kind.as_str() {
                    "artist" => {
                        custom_artwork::remove_artist_image(key.as_str());
                        if let Some(w) = weak.upgrade() {
                            w.global::<ArtistState>().set_has_custom_image(false);
                        }
                    }
                    "album" => {
                        custom_artwork::remove_album_cover(key.as_str());
                        if let Some(w) = weak.upgrade() {
                            w.global::<AlbumState>().set_has_custom_cover(false);
                        }
                    }
                    _ => log::warn!(
                        "[qbz-slint] artwork remove-custom: unknown kind {kind}"
                    ),
                }
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
