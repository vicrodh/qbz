//! QBZ Slint MVP binary.
//!
//! A native Slint front end for QBZ built on the framework-agnostic
//! `qbz-app` / `qbz-core` stack — no Tauri, no WebView. See the MVP ADR
//! (`qbz-nix-docs/qbz-adr/qbz_slint_functional_poc_adr.md`).
//!
//! Lives only on the private `slint-mvp` branch (ADR-007). The Slint UI
//! tree is compiled in the `qbz-ui` library crate (its `build.rs` runs
//! `slint-build` on `ui/app.slint`); this binary re-exports all generated
//! types at the crate root so existing `crate::AppWindow` / `crate::*State`
//! references resolve unchanged.
//!
//! Status: foundation tokens, login screen, app shell, functional
//! system-browser OAuth, saved-session restore, and a real Discover /
//! Home view fed by the Qobuz discover index with cached artwork.

pub use qbz_ui::*;

mod adapter;
mod album;
mod album_map;
mod artist;
mod artist_blacklist;
mod artist_prefs;
mod artist_releases;
mod artwork;
mod auth;
mod award;
mod blacklist_manager;
mod booklet;
mod commands;
mod custom_artwork;
pub use qbz_text_utils::{dates, strip_html};
mod discover_browse;
mod discord_rpc;
mod discover_prefs;
mod discovery_dismiss;
mod fav_cache;
mod favorites;
mod favorites_prefs;
mod foryou;
mod genre_filter;
use qbz_dac_wizard as dac_wizard;
mod home;
mod immersive;
mod info_modals;
mod keybindings;
mod label;
mod link_resolver;
mod miniplayer;
mod location_view;
mod mix;
mod musician;
mod myqbz;
mod myqbz_add;
mod myqbz_builder;
mod myqbz_cover;
mod myqbz_detail;
mod myqbz_edit;
mod myqbz_mix;
mod myqbz_play;
mod myqbz_prefs;
mod myqbz_view_prefs;
mod nav;
mod play_history;
mod playback;
mod qconnect_engine;
mod cast_service;
mod qconnect_event_sink;
mod qconnect_service;
mod qconnect_transport;
mod queue;
mod remote_stream;
mod drag;
mod ephemeral;
mod folders;
mod library_db;
mod local_library;
mod local_playlist;
mod local_library_settings;
mod lyrics;
mod lyrics_measure;
mod lyrics_prefs;
mod lyrics_sync;
mod media_controls;
mod locallibrary_prefs;
mod tag_editor;
mod offline;
mod offline_cache;
mod offline_favorites;
mod visualizer;
mod offline_manager;
mod offline_mode;
mod playlist;
mod playlist_import;
mod playlist_manager;
mod playlist_snapshot;
mod purchases;
mod plex_auth;
mod plex_settings;
mod playlist_picker;
mod quality;
mod reco;
mod recently;
mod scrobble;
mod scrobbler_settings;
mod search;
mod session_persist;
mod search_service;
// WGPU UNDERLAY SPIKE: GPU fragment-shader background for ImmersiveView.
mod shader_underlay;
mod settings;
mod share;
mod sidebar;
mod suggestions;
mod theme;
pub use qbz_slint_common::toast;
mod tray;
mod tray_settings;
mod ui_prefs;

use std::sync::Arc;

use i_slint_backend_winit::{
    winit::event::{ElementState, MouseButton, WindowEvent},
    winit::keyboard::{Key, NamedKey},
    EventResult, WinitWindowAccessor,
};
use slint::Model;

use adapter::SlintAdapter;
use commands::AppCommand;
use qbz_app::shell::AppRuntime;

/// Login Terms-of-Service link target.
const QOBUZ_TOS_URL: &str = "https://www.qobuz.com/us-en/legal/terms";

fn dispatch(command: AppCommand) {
    log::info!("[qbz-slint] AppCommand::{} dispatched", command.id());
}

/// Per-user shell wiring shared by the online and offline session entries.
/// None of it requires a Qobuz session: local library DB binding (+ mixtape
/// migrations), per-user pref stores, system tray and media controls.
/// Returns the tray settings snapshot for the UI seeding.
fn init_shell_for_user(
    runtime: &Arc<AppRuntime<SlintAdapter>>,
    weak: &slint::Weak<AppWindow>,
    user_id: u64,
) -> tray_settings::TraySettings {
    // Bind the local library DB to this user (folders / playlist
    // settings live in the per-user library.db).
    library_db::set_user(user_id);

    // Run the Mixtapes & Collections schema migrations against the same
    // per-user library.db (the mixtape tables live in that file). Mirrors
    // the Tauri build's session_lifecycle.rs `run_mixtape_migrations`.
    // Best-effort: log on error, never block shell entry.
    library_db::with_db(|db| {
        Ok(db.with_connection(|conn| {
            if let Err(e) = qbz_mixtape::schema::run_mixtape_migrations(conn) {
                log::error!("[qbz-slint] mixtape migrations failed: {e}");
            }
        }))
    });

    // Bind tray settings to this user (per-user tray_settings.db, shared with
    // the Tauri build) and snapshot them to seed the settings UI.
    tray_settings::init_for_user(user_id);
    let tray = tray_settings::get();

    // Bind Plex connection settings to this user (per-user plex_settings.db,
    // Slint-only). Seeded into PlexSettingsState lazily on panel open.
    plex_settings::init_for_user(user_id);

    // Bind scrobbler (Last.fm + ListenBrainz) settings to this user (per-user
    // scrobbler_settings.db), then start the scrobble runtime: tokio handle
    // for the source-agnostic now-playing/scrobble fire, LB credential seed
    // from the shared cache, and the offline-queue flush watcher (drains the
    // shared scrobble_queue + listen_queue on every offline -> online edge).
    scrobbler_settings::init_for_user(user_id);
    scrobble::start(tokio::runtime::Handle::current());

    // Discord Rich Presence: apply the persisted opt-in AFTER the session is
    // active (PR #477 — never at early boot). Runs for both the online and
    // offline entry paths since both call init_shell_for_user. No-op + no IPC
    // when the user hasn't opted in.
    discord_rpc::init(runtime, &tokio::runtime::Handle::current());

    // Restore the persisted player volume so audio starts at the saved level
    // (the poll loop then mirrors it onto NowPlayingState for the slider).
    playback::set_volume(
        runtime.clone(),
        weak.clone(),
        tokio::runtime::Handle::current(),
        crate::ui_prefs::load().volume,
    );

    // Bind "My QBZ" nav branding (custom label + icon) to this user
    // (per-user myqbz_branding.json). Seeded into MyQbzBrandingState by the
    // caller so the sidebar row + Settings row reflect the persisted values.
    myqbz_prefs::init_for_user(user_id);

    // Bind per-collection DETAIL view-prefs (toolbar viewMode/sort/filter) to
    // this user (per-user collection_view_prefs.json). Restored on collection
    // open, cleared on delete (spec 12 §18).
    myqbz_view_prefs::init_for_user(user_id);

    // Bind the lyrics display prefs (auto-follow / font / size / dimming /
    // active color / uppercase — per-user lyrics_prefs.json) and seed them
    // into LyricsState so the sidebar + controls flyout reflect the
    // persisted values from the first open (defaults = Tauri's).
    lyrics_prefs::init_for_user(user_id);
    {
        let prefs = lyrics_prefs::load();
        let _ = weak.upgrade_in_event_loop(move |w| {
            lyrics_prefs::apply_to_ui(&w, &prefs);
        });
    }

    // Create the system tray from this user's persisted settings (gated by
    // enable_tray). Reflects the chosen icon variant. On Linux the ksni
    // service runs on its own thread; macOS/Windows are no-ops until the
    // tray-icon slice lands.
    tray::init(
        runtime.clone(),
        weak.clone(),
        tokio::runtime::Handle::current(),
        tray.tray_icon_theme.clone(),
        tray.enable_tray,
    );

    // System media controls — MPRIS on Linux (publishes DesktopEntry so GNOME
    // shows the app icon), SMTC/MediaRemote on macOS/Windows. Independent of
    // the tray; pushes metadata/state from the playback paths.
    media_controls::init(
        runtime.clone(),
        weak.clone(),
        tokio::runtime::Handle::current(),
    );

    tray
}

/// Background-load the Audio + Playback settings into the Settings page —
/// store reads and device enumeration are blocking and fully local. Shared
/// by the online and offline session entries.
fn spawn_settings_snapshot_load(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    settings_ctx: Arc<settings::SettingsCtx>,
) {
    tokio::spawn(async move {
        let ctx_for_load = settings_ctx.clone();
        match tokio::task::spawn_blocking(move || settings::load_snapshot(&ctx_for_load)).await {
            Ok(snap) => {
                let _ = weak.upgrade_in_event_loop(move |w| {
                    settings::apply_snapshot(&w, snap);
                });
            }
            Err(e) => log::error!("[qbz-slint] settings load task failed: {e}"),
        }
        // Bit-perfect (ALSA + hw) forces local volume to 100% at startup so
        // the bar reflects unity gain before Settings is ever opened. No-op
        // otherwise (and while controlling a peer). Mirrors Tauri.
        settings::apply_startup_bitperfect_volume(&settings_ctx, &runtime, &weak).await;
    });
}

/// Seed the tray settings UI from the persisted per-user store.
fn seed_tray_appearance(w: &AppWindow, tray: &tray_settings::TraySettings) {
    let appearance = w.global::<AppearanceState>();
    appearance.set_tray_enable(tray.enable_tray);
    appearance.set_tray_minimize_to_tray(tray.minimize_to_tray);
    appearance.set_tray_close_to_tray(tray.close_to_tray);
    appearance.set_tray_mac_hide_dock(tray.mac_hide_dock);
    appearance.set_tray_icon_theme_index(tray_settings::icon_theme_index(&tray.tray_icon_theme));
}

/// Refresh the blacklist count + enabled flag on `BlacklistState` (T10).
/// The Settings content-filtering row binds to these, and Settings is reached
/// independently of the Manager (which seeds them on its own load), so we
/// re-read the wrapper whenever the Settings view is shown. Fail-open: with no
/// session the wrapper returns count 0 / enabled true.
fn seed_blacklist_status(w: &AppWindow) {
    let st = w.global::<BlacklistState>();
    st.set_count(crate::artist_blacklist::count() as i32);
    st.set_enabled(crate::artist_blacklist::is_enabled());
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
    let tray = init_shell_for_user(&runtime, &weak, session.user_id);

    let _ = weak.upgrade_in_event_loop(move |w| {
        let state = w.global::<SessionState>();
        state.set_user_name(session.display_name.into());
        state.set_subscription(session.subscription.into());
        // A successful login means a previous session now exists; clear any
        // stale boot restore error from the login screen.
        let offline_state = w.global::<OfflineState>();
        offline_state.set_has_previous_session(true);
        offline_state.set_login_error("".into());
        seed_tray_appearance(&w, &tray);
        // Seed the My QBZ branding (label + icon) from the per-user store so
        // the sidebar row + Settings row paint the custom values immediately.
        myqbz_prefs::seed(&w);
        // Seed the Discover configurator descriptor lists so the prefs-driven
        // render loop has order/visibility data before the first apply_home.
        discover_prefs::seed(&w);
        w.global::<HomeState>().set_loading(true);
        w.set_screen(AppScreen::Shell);
    });

    // Start the playback poll loop — it runs for the app lifetime,
    // ticking position/progress onto NowPlayingState and auto-advancing
    // the queue on track end. Safe to start once per shell entry.
    playback::start_poll_loop(runtime.clone(), weak.clone(), tokio::runtime::Handle::current());
    // Store the miniplayer context (idempotent). The mini window itself is
    // created lazily on first enter.
    miniplayer::init(runtime.clone(), weak.clone(), tokio::runtime::Handle::current());
    // Bind the exit context so the window close handlers can flush a final
    // session snapshot before the loop quits (idempotent).
    session_persist::bind_exit_ctx(runtime.clone(), tokio::runtime::Handle::current());

    // Load the sidebar playlists list.
    load_sidebar_playlists(runtime.clone(), weak.clone(), &tokio::runtime::Handle::current());

    // Warm the shared favorite-track cache so track rows can show the
    // correct heart state from their first paint (album / artist / search
    // / playlist / mix / favorites / queue all read it). The disk seed
    // already ran at session activation (fav_cache::init_for_user); this
    // refreshes from the network and writes the fresh set back — skipped
    // while offline, where the disk seed is the truth.
    {
        let runtime = runtime.clone();
        tokio::spawn(async move {
            if crate::offline_mode::engine().is_offline() {
                return;
            }
            match runtime.core().favorite_track_ids().await {
                Ok(ids) => {
                    // set_all mirrors to disk (blocking rusqlite) — keep it
                    // off the async worker.
                    let _ = tokio::task::spawn_blocking(move || fav_cache::set_all(ids)).await;
                }
                Err(e) => log::warn!("[qbz-slint] favorite cache load failed: {e}"),
            }
        });
    }

    // Same for favorite ALBUMS — seeds fav_cache so the album header heart is
    // correct from first open without visiting the Favorites view.
    {
        let runtime = runtime.clone();
        tokio::spawn(async move {
            if crate::offline_mode::engine().is_offline() {
                return;
            }
            let ids = favorites::favorite_album_ids(&runtime).await;
            let _ = tokio::task::spawn_blocking(move || fav_cache::set_all_albums(ids)).await;
        });
    }

    // Same for followed AWARDS — seeds fav_cache so the AwardView follow heart
    // is correct from first open (Tauri's loadAwardFavorites, plural read).
    {
        let runtime = runtime.clone();
        tokio::spawn(async move {
            if crate::offline_mode::engine().is_offline() {
                return;
            }
            let ids = award::favorite_award_ids(&runtime).await;
            let _ = tokio::task::spawn_blocking(move || fav_cache::set_all_awards(ids)).await;
        });
    }

    // Load Audio + Playback settings into the Settings page in the
    // background — store reads and device enumeration are blocking.
    spawn_settings_snapshot_load(runtime.clone(), weak.clone(), settings_ctx.clone());

    // Load the genre-filter parents + persisted selection, then seed
    // the popup state. Done before the discover load so the first
    // fetch honors a remembered genre selection.
    genre_filter::load_parents(&runtime).await;
    let _ = weak.upgrade_in_event_loop(|w| {
        genre_filter::apply_state(&w);
    });

    reload_home(&runtime, &weak, &image_cache, "home".to_string()).await;

    // Session persistence: restore the last queue + current track PAUSED (gated
    // on `persist_session`). set_queue_with_order emits QueueUpdated so the queue
    // sidebar repaints itself; the now-playing bar reads current_track, so we
    // refresh its metadata explicitly. No audio is loaded — playback stays
    // stopped until the user hits play (Phase B then seeks to the saved
    // position when `resume_playback_position` is on).
    if session_persist::restore(&runtime).await {
        playback::refresh_now_playing_meta(&runtime, &weak).await;
        // Repaint the queue sidebar/list — set_queue_with_order emits
        // QueueUpdated, but the queue UI repaints from explicit refreshes.
        playback::refresh_sidebar(true);
        // Seed the seek bar + timers to the resume position so they show it
        // immediately (refresh_now_playing_meta above reset them to 0; the poll
        // loop only catches up once playback starts). Peeks — the actual resume
        // still fires on first play.
        //
        // KNOWN ISSUE / NEEDS WORK: this seed does NOT visibly stick — at rest
        // the bar + timer still read 0:00 and only jump to the resume position
        // once the user presses play (the audio resume itself works correctly).
        // Something repaints NowPlayingState position/progress back to 0 after
        // this runs (a later refresh_now_playing_meta closure, the poll loop's
        // idle tick reporting position 0 while no audio is loaded, or the bar
        // binding not reflecting a paused non-loaded position). Left as-is on
        // purpose — revisit the pre-play seek-bar seed for paused restore.
        let resume_pos = session_persist::pending_resume_position();
        if resume_pos > 0 {
            if let Some(track) = runtime.core().current_track().await {
                let dur = track.duration_secs;
                let _ = weak.upgrade_in_event_loop(move |w| {
                    playback::seed_seek_display(&w, resume_pos, dur);
                });
            }
        }
    }

    // Startup page = "where you left off": restore the last SAFE top-level view
    // (online only — the offline entry keeps its D12 LocalLibrary root). Home
    // was loaded just above; if a different view is remembered, re-root the nav
    // history there (on the UI thread, like the offline path) and apply_entry it
    // — which loads the view's data, NOT a blank set_view (the Tauri precedent).
    {
        let prefs = crate::ui_prefs::load();
        if prefs.startup_page == "remember" {
            let entry = match prefs.last_view.as_str() {
                "favorites" => Some(nav::NavEntry::Favorites { tab: "tracks".to_string() }),
                "local-library" => Some(nav::NavEntry::LocalLibrary {
                    tab: local_library::LibTab::Albums.tab_id().to_string(),
                }),
                "mixtapes" => Some(nav::NavEntry::Mixtapes),
                "collections" => Some(nav::NavEntry::Collections),
                // Purchases restores at startup (id-free top-level surface).
                // `purchase-album` is never persisted (id-gated, not in
                // safe_view_key), so it never reaches here.
                "purchases" => Some(nav::NavEntry::Purchases),
                // "home" / unknown → keep the Home already loaded above.
                _ => None,
            };
            if let Some(entry) = entry {
                let root_entry = entry.clone();
                let _ = weak.upgrade_in_event_loop(move |_w| {
                    nav::reset_root(root_entry);
                });
                apply_entry(
                    entry,
                    &runtime,
                    &weak,
                    &tokio::runtime::Handle::current(),
                    &image_cache,
                );
            }
        }
    }

    // Seed the favorites tab counts so the badges are ready before the
    // user opens each tab (they otherwise only count on first visit).
    let counts = favorites::load_counts(&runtime).await;
    let _ = weak.upgrade_in_event_loop(move |w| {
        favorites::apply_counts(&w, counts);
    });
}

/// Offline session entry — "Start offline" on the login screen (spec §4.1).
///
/// Mirrors the subset of `enter_shell` + Tauri's `activate_offline_session`
/// that works without a Qobuz session: session scaffolding, local library,
/// offline cache, per-user pref stores, tray/media controls, the playback
/// poll loop and the settings snapshot. Everything Qobuz-bound is skipped
/// (sidebar playlists, favorites warm, genre filter, home/discover load) —
/// the engine's gate refuses those calls anyway (D3).
async fn enter_shell_offline(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    image_cache: artwork::ImageCache,
    settings_ctx: Arc<settings::SettingsCtx>,
) -> Result<(), String> {
    // Never open the empty user-0 profile: offline mode needs a previous
    // session's data (Tauri falls back to user 0; the port refuses — the
    // login UI hides the link in that case, this is the backstop).
    let Some(user_id) = qbz_app::user_data::UserDataPaths::load_last_user_id() else {
        return Err("no previous session — offline mode requires a prior login".to_string());
    };

    // Session scaffolding at the last user (session store, runtime state).
    runtime.activate_offline().await?;

    // Offline cache (shared index.db + library.db) + the in-memory cached-ids
    // set the track rows read. Must precede offline_mode::init_for_user so
    // the subscription purge consumer can reach the cache.
    crate::offline::activate(user_id).await;
    crate::offline_cache::load_cached_ids().await;

    // Local library, per-user pref stores, tray, media controls.
    let tray = init_shell_for_user(&runtime, &weak, user_id);

    // Offline-MODE engine: bind the per-user stores and flag the
    // unauthenticated offline session (D1 — session-scoped, never persisted;
    // a later successful login clears it). The favorites-cache bind seeds
    // hearts from disk — the start-offline gap Tauri never closed.
    if let Some(dir) = crate::offline_mode::user_data_dir(user_id) {
        crate::offline_mode::init_for_user(&dir);
        crate::fav_cache::init_for_user(&dir);
        // Recommendation event store (shared events.db with Tauri).
        crate::reco::init_for_user(&dir);
        // Playlist Suggested Songs: open the per-user artist-vector store.
        if let Ok(store) = qbz_reco::ArtistVectorStore::open_at(&dir) {
            runtime.core().set_artist_vectors(store).await;
        }
        crate::discover_prefs::init_for_user(&dir);
        // D-FIX-a: bind the blacklist offline too — Tauri never initialized it
        // in offline mode, so blacklisted artists leaked into offline surfaces.
        crate::artist_blacklist::init_for_user(&dir);
        // Intelligent Search (cache + ranking), seeded from the persisted pref.
        // Cached results stay searchable offline; live revalidation no-ops.
        crate::search_service::init(&dir, crate::ui_prefs::load().intelligent_search);
        // Session persistence (queue + playback): open the per-user session.db
        // and seed the persist/resume gates from the playback prefs.
        crate::session_persist::init_for_user(&dir);
    }
    // Lyrics cache (per-user, shared file with Tauri) — offline sessions
    // serve cached lyrics (deviation D3, cache-first offline contract).
    crate::lyrics::init_for_user(runtime.core().client(), user_id);
    crate::offline_mode::engine().set_offline_session(true);

    {
        let weak = weak.clone();
        let _ = weak.clone().upgrade_in_event_loop(move |w| {
            seed_tray_appearance(&w, &tray);
            myqbz_prefs::seed(&w);
            // Seed the Discover configurator descriptor lists (works offline —
            // the prefs store is per-user and bound at session activation).
            discover_prefs::seed(&w);
            // No HomeState loading spinner: the discover load is skipped offline
            // (the gating slice adds the placeholder views).
            w.set_screen(AppScreen::Shell);
            // D12: an offline session lands on LocalLibrary (Home is a blocked
            // placeholder offline). Root the nav history at it so back/forward
            // never lead to a phantom blocked Home.
            nav::reset_root(nav::NavEntry::LocalLibrary {
                tab: local_library::LibTab::Albums.tab_id().to_string(),
            });
            update_nav_flags(&w);
        });
    }
    navigate_local_library(
        runtime.clone(),
        weak.clone(),
        &tokio::runtime::Handle::current(),
        image_cache,
        local_library::LibTab::Albums,
    );

    // Sidebar playlists: offline the load lists the LOCAL playlists plus the
    // MIXED Qobuz playlists with local sidecar content (D11.b) — the Qobuz
    // fetch itself fast-fails at the gate.
    load_sidebar_playlists(runtime.clone(), weak.clone(), &tokio::runtime::Handle::current());

    // Playback poll loop — local/cached playback and queue advance work
    // offline. Same lifetime semantics as the online entry.
    playback::start_poll_loop(runtime.clone(), weak.clone(), tokio::runtime::Handle::current());
    // Store the miniplayer context (idempotent). The mini window itself is
    // created lazily on first enter.
    miniplayer::init(runtime.clone(), weak.clone(), tokio::runtime::Handle::current());
    // Bind the exit context so the window close handlers can flush a final
    // session snapshot before the loop quits (idempotent).
    session_persist::bind_exit_ctx(runtime.clone(), tokio::runtime::Handle::current());

    // Load Audio + Playback settings into the Settings page in the
    // background — fully local, same path as the online entry.
    spawn_settings_snapshot_load(runtime.clone(), weak.clone(), settings_ctx.clone());

    log::info!("[qbz-slint] offline session entered for user {user_id}");
    Ok(())
}

/// The shared genre-filter selection expanded to descendant ids, as the
/// `Option<Vec<u64>>` the discover endpoints take (None = no filter).
/// Shared by the home re-fetch and the DiscoverBrowse "View all" page.
fn current_genre_filter() -> Option<Vec<u64>> {
    let ids = genre_filter::filter_ids("discover");
    (!ids.is_empty()).then_some(ids)
}

/// Fetch the discover index (honoring the shared genre selection),
/// apply all three tab section sets, show the requested tab, and fan
/// out artwork. Shared by the initial shell load and genre-filter /
/// tab re-fetches.
async fn reload_home(
    runtime: &Arc<AppRuntime<SlintAdapter>>,
    weak: &slint::Weak<AppWindow>,
    image_cache: &artwork::ImageCache,
    active_tab: String,
) {
    // Expand the selection to descendants so a parent selection
    // covers its child genres (the child-genre filtering recovery).
    let genre_ids = genre_filter::filter_ids("discover");
    let genre_ids = (!genre_ids.is_empty()).then_some(genre_ids);

    match home::load_home(runtime, genre_ids).await {
        Ok(data) => {
            // Album-carousel covers are now fired by select_tab below: the
            // prefs-driven render loop draws Home/Editor album sections from the
            // DiscoverState descriptor lists, so their artwork is descriptor-
            // targeted (DiscoverSectionAlbum) and returned by select_tab once the
            // lists are built. Here we only prebuild the artwork for the models
            // that still bind HomeState fields (slim grids, recent albums,
            // playlists), which select_tab does not rebuild.
            let mut jobs: Vec<artwork::ArtworkJob> = Vec::new();
            // Home-only slim grids (their models are populated regardless
            // of the visible tab; harmless to prefetch).
            jobs.extend(data.popular.iter().enumerate().filter_map(|(idx, slim)| {
                (!slim.artwork_url.is_empty()).then(|| artwork::ArtworkJob {
                    target: artwork::ArtworkTarget::Popular { idx },
                    url: slim.artwork_url.clone(),
                })
            }));
            jobs.extend(data.recent.iter().enumerate().filter_map(|(idx, slim)| {
                (!slim.artwork_url.is_empty()).then(|| artwork::ArtworkJob {
                    target: artwork::ArtworkTarget::Recent { idx },
                    url: slim.artwork_url.clone(),
                })
            }));
            // Recently-played album covers: Qobuz covers use the plain loader;
            // Plex/local covers need the source-aware funnel (PlexThumb
            // tokenization / local file read), else they never resolve.
            let mut plex_album_jobs: Vec<artwork::ArtworkJob> = Vec::new();
            for (idx, card) in data.recent_albums.iter().enumerate() {
                if card.artwork_url.is_empty() {
                    continue;
                }
                let job = artwork::ArtworkJob {
                    target: artwork::ArtworkTarget::RecentAlbum { idx },
                    url: card.artwork_url.clone(),
                };
                if card.source == "plex" || card.source == "local" {
                    plex_album_jobs.push(job);
                } else {
                    jobs.push(job);
                }
            }

            // Qobuz Playlists row covers for the active tab (single-cover,
            // Qobuz CDN URLs → the plain loader, never the local/Plex funnel).
            let empty_playlists: Vec<home::PlaylistCardData> = Vec::new();
            let active_playlists = match active_tab.as_str() {
                "editorPicks" => &data.editor_playlists,
                "forYou" => &empty_playlists,
                _ => &data.playlists,
            };
            jobs.extend(home::playlist_artwork_jobs(active_playlists));

            let weak_for_artwork = weak.clone();
            let weak_for_local = weak.clone();
            let image_cache_local = image_cache.clone();
            let image_cache_sections = image_cache.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                home::apply_home(&w, data);
                // apply_home caches the section sets + pushes the descriptor
                // lists; select_tab renders the requested tab from them and
                // returns the descriptor-targeted album-section artwork jobs
                // (DiscoverSectionAlbum) — spawn them here, on the UI thread.
                let section_jobs = home::select_tab(&w, &active_tab);
                artwork::spawn_loads(section_jobs, w.as_weak(), image_cache_sections.clone());
                w.global::<HomeState>().set_loading(false);
            });
            artwork::spawn_loads(jobs, weak_for_artwork, image_cache.clone());
            if !plex_album_jobs.is_empty() {
                let plex = crate::plex_settings::get();
                artwork::spawn_local_or_plex_loads(
                    plex_album_jobs,
                    plex.base_url,
                    plex.token,
                    weak_for_local,
                    image_cache_local,
                );
            }
        }
        Err(e) => {
            log::error!("[qbz-slint] discover load failed: {e}");
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<HomeState>().set_loading(false);
            });
        }
    }
}

/// Read the current DiscoverBrowse "View all" target when that page is
/// the active view, so a genre-filter change can re-navigate it instead
/// of the Discover home index (the selection is shared across surfaces).
/// Returns None when any other view is showing. UI thread only.
fn current_browse_target(window: &AppWindow) -> Option<(String, String)> {
    if window.global::<NavState>().get_view() != ContentView::DiscoverBrowse {
        return None;
    }
    let state = window.global::<DiscoverBrowseState>();
    let endpoint = state.get_endpoint().to_string();
    if endpoint.is_empty() {
        return None;
    }
    Some((endpoint, state.get_title().to_string()))
}

/// Push the navigation history flags onto `NavState`. UI thread only.
fn update_nav_flags(window: &AppWindow) {
    let state = window.global::<NavState>();
    state.set_can_back(nav::can_back());
    state.set_can_forward(nav::can_forward());
}

/// Navigate to the entity a resolved "Open Qobuz Link" points at. Albums /
/// artists / playlists open their detail view directly; a track is fetched to
/// resolve its album, then that album opens (mirrors the Tauri behavior).
fn apply_resolved_link(
    link: qbz_music_link::ResolvedLink,
    runtime: &Arc<AppRuntime<SlintAdapter>>,
    weak: &slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: &artwork::ImageCache,
) {
    use qbz_music_link::ResolvedLink;
    match link {
        ResolvedLink::OpenAlbum(id) => {
            navigate_album(runtime.clone(), weak.clone(), handle, image_cache.clone(), id);
        }
        ResolvedLink::OpenArtist(id) => {
            navigate_artist(
                runtime.clone(),
                weak.clone(),
                handle,
                image_cache.clone(),
                id.to_string(),
            );
        }
        ResolvedLink::OpenPlaylist(id) => {
            navigate_playlist(
                runtime.clone(),
                weak.clone(),
                handle,
                image_cache.clone(),
                id.to_string(),
            );
        }
        ResolvedLink::OpenTrack(id) => {
            let runtime = runtime.clone();
            let weak = weak.clone();
            let handle = handle.clone();
            let image_cache = image_cache.clone();
            handle.clone().spawn(async move {
                match runtime.core().get_track(id).await {
                    Ok(track) => {
                        if let Some(album_id) = track.album.as_ref().map(|a| a.id.clone()) {
                            navigate_album(
                                runtime.clone(),
                                weak.clone(),
                                &handle,
                                image_cache.clone(),
                                album_id,
                            );
                        }
                    }
                    Err(e) => {
                        log::error!("[qbz-slint] open-link: get_track failed: {e}");
                    }
                }
            });
        }
    }
}

/// Capture browser mouse buttons before Slint routes them to the topmost
/// TouchArea, and mirror the cursor position into `ShellState` for passive
/// hover chrome that must not install a TouchArea over interactive content.
/// Otherwise cards/sidebar rows can swallow Back/Forward while empty chrome
/// still works, and hover-only scrollbars cannot reliably see card hover.
fn install_browser_mouse_nav(window: &AppWindow) {
    let weak = window.as_weak();
    window.window().on_winit_window_event(move |slint_window, event| {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let Some(window) = weak.upgrade() else {
                    return EventResult::Propagate;
                };
                let position = position.to_logical::<f64>(slint_window.scale_factor() as f64);
                let state = window.global::<ShellState>();
                state.set_pointer_x(position.x as f32);
                state.set_pointer_y(position.y as f32);
                state.set_pointer_in_window(true);
                return EventResult::Propagate;
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(window) = weak.upgrade() {
                    let state = window.global::<ShellState>();
                    state.set_pointer_in_window(false);
                    state.set_pointer_x(-1.0);
                    state.set_pointer_y(-1.0);
                }
                return EventResult::Propagate;
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                let Some(window) = weak.upgrade() else {
                    return EventResult::Propagate;
                };

                match button {
                    MouseButton::Back => {
                        window.global::<NavState>().invoke_request_back();
                        EventResult::PreventDefault
                    }
                    MouseButton::Forward => {
                        window.global::<NavState>().invoke_request_forward();
                        EventResult::PreventDefault
                    }
                    _ => EventResult::Propagate,
                }
            }
            // Steal Up/Down ONLY while a search dropdown is open, BEFORE the
            // search input's cursor can eat the first press. This is what makes
            // the very first ArrowDown move the selection from the input INTO the
            // dropdown (the input is a single-line TextInput that otherwise
            // consumes the first arrow as a cursor move). Enter/Escape stay with
            // the FocusScope. Two surfaces share this hook: the IMMERSIVE dropdown
            // takes priority when immersive is open + its search is open (it sits
            // on top of everything), otherwise the MAIN header cortinilla.
            // Track modifier state for the keybindings dispatcher (winit
            // delivers modifiers separately from key presses).
            WindowEvent::ModifiersChanged(modifiers) => {
                let m = modifiers.state();
                crate::keybindings::set_mods(
                    m.control_key() || m.super_key(),
                    m.alt_key(),
                    m.shift_key(),
                );
                EventResult::Propagate
            }
            WindowEvent::KeyboardInput { event: key_event, .. }
                if key_event.state == ElementState::Pressed =>
            {
                let Some(window) = weak.upgrade() else {
                    return EventResult::Propagate;
                };

                // (A) Customize-shortcuts editor capture: while recording, the
                // next combo is captured as a binding instead of dispatched.
                let recording = window.global::<KeybindingsState>().get_recording_id();
                if !recording.is_empty() {
                    return crate::keybindings::handle_capture(
                        &window,
                        recording.as_str(),
                        &key_event.logical_key,
                    );
                }

                // (B) Steal Up/Down ONLY while a search dropdown is open, BEFORE
                // the search input's cursor can eat the first press (lets the
                // very first ArrowDown move selection from the input INTO the
                // dropdown). Immersive takes priority when its search is open.
                let immersive_search_open = window.global::<ImmersiveState>().get_open()
                    && window.global::<ImmersiveState>().get_search_open();
                let main_cortinilla_open =
                    window.global::<SearchState>().get_cortinilla_open();
                if immersive_search_open || main_cortinilla_open {
                    let move_selection = |delta: i32| {
                        if immersive_search_open {
                            window
                                .global::<ImmersiveSearchActions>()
                                .invoke_move_selection(delta);
                        } else {
                            window
                                .global::<SearchActions>()
                                .invoke_cortinilla_move_selection(delta);
                        }
                    };
                    return match &key_event.logical_key {
                        Key::Named(NamedKey::ArrowDown) => {
                            move_selection(1);
                            EventResult::PreventDefault
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            move_selection(-1);
                            EventResult::PreventDefault
                        }
                        _ => EventResult::Propagate,
                    };
                }

                // (C) Global hotkeys — never while typing in a text field
                // (mirrors the Tauri `isInputTarget` guard).
                if window.global::<UiFocusState>().get_text_input_focused() {
                    return EventResult::Propagate;
                }
                crate::keybindings::dispatch(&window, &key_event.logical_key)
            }
            // Persist main-window geometry so the next launch restores it
            // (mirrors miniplayer.rs). The startup restore clamps to the
            // monitor; here we just record what the WM settled on. A change
            // guard avoids redundant writes on the many no-op events the WM
            // emits. The app minimum is 940x600 (app.slint) — ignore smaller
            // frames (minimize reports 0x0, mid-transition frames undershoot).
            WindowEvent::Resized(size) => {
                let scale = slint_window.scale_factor().max(0.01) as f64;
                let lw = (size.width as f64 / scale) as f32;
                let lh = (size.height as f64 / scale) as f32;
                if lw >= 940.0 && lh >= 600.0 {
                    let mut prefs = crate::ui_prefs::load();
                    if (prefs.window_width - lw).abs() > 0.5
                        || (prefs.window_height - lh).abs() > 0.5
                    {
                        prefs.window_width = lw;
                        prefs.window_height = lh;
                        crate::ui_prefs::save(&prefs);
                    }
                }
                EventResult::Propagate
            }
            WindowEvent::Moved(pos) => {
                let mut prefs = crate::ui_prefs::load();
                if prefs.window_x != pos.x || prefs.window_y != pos.y {
                    prefs.window_x = pos.x;
                    prefs.window_y = pos.y;
                    crate::ui_prefs::save(&prefs);
                }
                EventResult::Propagate
            }
            _ => EventResult::Propagate,
        }
    });
}

/// Wire the immersive header's native window-control cluster
/// (WindowControlActions) to the MAIN AppWindow's winit handle. The AppWindow
/// uses native OS decorations, so the only seam to move/min/max/fullscreen it
/// is the winit backend — reached via `WinitWindowAccessor::with_winit_window`,
/// exactly as miniplayer.rs does for the mini window. `with_winit_window`
/// returns `Option<T>` (None when not on the winit backend / window gone), so
/// every handler degrades gracefully. Read-then-set state (maximize/fullscreen)
/// returns the new flag OUT of the closure, then pushes it onto the global
/// AFTER the borrow ends.
fn wire_window_controls(window: &AppWindow) {
    let actions = window.global::<WindowControlActions>();

    // Window state (minimize / maximize / fullscreen) is driven through SLINT's
    // own Window API, NOT a direct winit call. The winit adapter reconciles the
    // window from Slint's own properties on every realization
    // (winitwindowadapter.rs ~1328 toggles winit to match `properties.is_fullscreen()`),
    // so a direct `winit.set_fullscreen(..)` gets UNDONE on the next frame — the
    // same "use Slint's native mechanism, don't fight winit" lesson as the
    // miniplayer's decorations fix. `slint::Window::{set_minimized,set_maximized,
    // set_fullscreen}` (api.rs:576/586/596) write the property the adapter reads.

    // Minimize.
    {
        let weak = window.as_weak();
        actions.on_minimize(move || {
            if let Some(w) = weak.upgrade() {
                w.window().set_minimized(true);
            }
        });
    }

    // Maximize / Restore toggle.
    {
        let weak = window.as_weak();
        actions.on_toggle_maximize(move || {
            if let Some(w) = weak.upgrade() {
                let m = !w.window().is_maximized();
                w.window().set_maximized(m);
                w.global::<WindowControlActions>().set_is_maximized(m);
            }
        });
    }

    // Fullscreen toggle (true fullscreen hides the native titlebar — the
    // genuinely useful immersive control). MUST go through slint::Window so the
    // realization reconciliation keeps it instead of reverting it.
    {
        let weak = window.as_weak();
        actions.on_toggle_fullscreen(move || {
            if let Some(w) = weak.upgrade() {
                let fs = !w.window().is_fullscreen();
                w.window().set_fullscreen(fs);
                w.global::<WindowControlActions>().set_is_fullscreen(fs);
            }
        });
    }

    // Close app — reuse the AppWindow's existing close-app choreography
    // (close-to-tray vs quit lives in `window.on_close_app`, main.rs ~13558;
    // miniplayer.rs calls the same `invoke_close_app`).
    {
        let weak = window.as_weak();
        actions.on_close_app(move || {
            if let Some(w) = weak.upgrade() {
                w.invoke_close_app();
            }
        });
    }

    // Drag-move — start a window-move drag (same idiom as miniplayer start_drag).
    {
        let weak = window.as_weak();
        actions.on_drag_move(move || {
            if let Some(w) = weak.upgrade() {
                w.window().with_winit_window(|win| {
                    let _ = win.drag_window();
                });
            }
        });
    }
}

/// Whether the CURRENT content view is one the AppShell swaps for the
/// OfflinePlaceholder while offline. KEEP IN SYNC with `qobuz-view-blocked`
/// in `AppShell.slint`. The playlist view blocks only when it is neither a
/// LOCAL playlist nor the offline sidecar rendering of a mixed one (D11.a).
/// UI thread only (reads the globals).
fn is_offline_blocked_view(window: &AppWindow) -> bool {
    match window.global::<NavState>().get_view() {
        ContentView::Home
        | ContentView::DiscoverBrowse
        | ContentView::Search
        | ContentView::Favorites
        | ContentView::Album
        | ContentView::Artist
        | ContentView::Musician
        | ContentView::Label
        | ContentView::LabelReleases
        | ContentView::Location
        | ContentView::Mix => true,
        ContentView::Playlist => {
            let ps = window.global::<PlaylistState>();
            !ps.get_is_local() && !ps.get_offline_subset()
        }
        _ => false,
    }
}

/// Map a top-level view to its persisted key for "Startup page = where you left
/// off" — ONLY safe, id-free views (the main nav destinations). Detail views
/// (album/artist/playlist/…) need a context id that may be stale on restart, so
/// they return None and the last safe view is kept instead.
fn safe_view_key(view: ContentView) -> Option<&'static str> {
    match view {
        // Home is also the Discover landing (its tabs render in the Home view),
        // so this covers both. Detail views (album/artist/playlist/…) + the
        // endpoint-scoped DiscoverBrowse "View all" pages are not restorable.
        ContentView::Home => Some("home"),
        ContentView::Favorites => Some("favorites"),
        ContentView::LocalLibrary => Some("local-library"),
        ContentView::Mixtapes => Some("mixtapes"),
        ContentView::Collections => Some("collections"),
        // Purchases is session-restore-safe (id-free top-level surface — §8.6).
        // The `purchase-album` detail view carries a context id that may be
        // stale on restart, so it is NOT restorable (returns None below),
        // matching how Album/Artist detail views are handled here.
        ContentView::Purchases => Some("purchases"),
        _ => None,
    }
}

/// Flip the `is-favorite` flag on every visible row matching `track_id`,
/// across all track-list surfaces (album, artist Popular, search,
/// playlist, mix, favorites). Used for the optimistic favorite toggle so
/// the heart updates the instant the user clicks, regardless of which
/// view they are on.
fn set_row_favorite(window: &AppWindow, track_id: &str, favorite: bool) {
    let flip = |model: &slint::ModelRc<TrackItem>| {
        if let Some(vm) = model.as_any().downcast_ref::<slint::VecModel<TrackItem>>() {
            for i in 0..vm.row_count() {
                if let Some(mut item) = vm.row_data(i) {
                    if item.id == track_id {
                        if item.is_favorite != favorite {
                            item.is_favorite = favorite;
                            vm.set_row_data(i, item);
                        }
                    }
                }
            }
        }
    };
    flip(&window.global::<AlbumState>().get_tracks());
    flip(&window.global::<ArtistState>().get_top_tracks());
    flip(&window.global::<SearchState>().get_tracks());
    flip(&window.global::<PlaylistState>().get_tracks());
    flip(&window.global::<MixState>().get_tracks());
    flip(&window.global::<FavoritesState>().get_tracks());

    // Search's most-popular track hero is a standalone TrackItem.
    let search = window.global::<SearchState>();
    let mut hero = search.get_most_popular_track();
    if hero.id == track_id && hero.is_favorite != favorite {
        hero.is_favorite = favorite;
        search.set_most_popular_track(hero);
    }
}

/// Feed Capa B (intelligent-search ranking) from a RESULTS-PAGE click, but only
/// when the results page is the active view. `on_open_album` / `on_open_artist`
/// / `on_media_action` are global handlers shared by every view (album detail,
/// home, favorites, …); without this gate a play/open from any of those would
/// be misattributed to the current search query. The `SearchState.query`
/// (results-page query, distinct from the live `cortinilla-query`) is the key.
///
/// No-op when the active view is not Search, when the module is disabled (the
/// `record` accessor itself no-ops then too), or when the query is empty. LOCAL
/// entities are NOT routed here — local rows never reach the Qobuz results page
/// (D1/D2) and use a different id space (D4).
fn record_search_interaction(
    window: &AppWindow,
    kind: &str,
    id: &str,
    action: crate::search_service::InteractionAction,
) {
    if window.global::<NavState>().get_view() != ContentView::Search {
        return;
    }
    if !crate::search_service::is_enabled() {
        return;
    }
    let query = window.global::<SearchState>().get_query().to_string();
    if query.trim().is_empty() {
        return;
    }
    crate::search_service::record(&query, kind, id, action);
}

/// Toggle a track favorite by its REAL Qobuz id: offline guard (read-only
/// hearts, spec 4.3), optimistic flip across the visible rows + the shared
/// fav cache, then the network add/remove with rollback on failure. Shared
/// by the Qobuz-surface `("track","favorite")` media-action arm and the
/// library-surface favorite entry (qobuz_download rows resolve their
/// `qobuz_track_id` first — never the local row id, which is Tauri's latent
/// "Add to Library" bug; LocalLibrary track-menu spec §3.2). UI-thread only
/// (upgrades `weak` directly).
fn toggle_track_favorite(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    id: String,
) {
    if offline_mode::engine().is_offline() {
        if let Some(w) = weak.upgrade() {
            toast::info(&w, "Not available offline");
        }
        return;
    }
    // Toggle (not just add): read the cached state, flip it optimistically
    // across every visible track model + the shared cache, then add/remove
    // on the network.
    let was_fav = fav_cache::is_favorite(&id);
    let make_fav = !was_fav;
    if let Ok(track_id) = id.parse::<u64>() {
        fav_cache::set(track_id, make_fav);
    }
    if let Some(w) = weak.upgrade() {
        set_row_favorite(&w, &id, make_fav);
    }
    handle.spawn(async move {
        let res = if make_fav {
            runtime.core().add_favorite("track", &id).await
        } else {
            runtime.core().remove_favorite("track", &id).await
        };
        // reco: log a favorite ADD on success (skip removes/failures) for taste
        // scoring; blocking SQLite off the async path.
        if make_fav && res.is_ok() {
            if let Ok(tid) = id.parse::<u64>() {
                tokio::task::spawn_blocking(move || {
                    crate::reco::log_favorite_track(tid, None, None)
                });
            }
        }
        if let Err(e) = res {
            log::error!("[qbz-slint] toggle track favorite failed: {e}");
            // Roll the optimistic change back on failure.
            if let Ok(tid) = id.parse::<u64>() {
                fav_cache::set(tid, was_fav);
            }
            let _ = weak.upgrade_in_event_loop(move |w| {
                set_row_favorite(&w, &id, was_fav);
            });
        }
    });
}

/// Look up the display name of an "Add to Mixtape/Collection" picker row by id
/// (for the post-add toast). Returns "" if not found.
fn myqbz_add_row_name(window: &AppWindow, collection_id: &str) -> String {
    use slint::Model;
    let model = window.global::<MyQbzAddState>().get_rows();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .find(|r| r.id == collection_id)
        .map(|r| r.name.to_string())
        .unwrap_or_default()
}

/// Open the global "Add to Mixtape/Collection" picker for `items` (mirrors
/// Tauri's `openAddToMixtape`). Hops onto the event loop to show the modal,
/// then loads the picker rows (kind-restricted + recency-sorted +
/// `item_exists`-resolved) on a blocking worker. Empty `items` is a no-op
/// (the controller guards too). Callable from any thread.
fn open_add_to_mixtape(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    items: Vec<myqbz_add::AddItem>,
) {
    if items.is_empty() {
        return;
    }
    let restrict = items.iter().any(|it| it.item_type != "album");
    let items_for_open = items.clone();
    let _ = weak.upgrade_in_event_loop(move |w| {
        myqbz_add::open(&w, items_for_open);
    });
    handle.spawn(async move {
        let rows =
            tokio::task::spawn_blocking(move || myqbz_add::load_rows(restrict, &items))
                .await
                .unwrap_or_default();
        let _ = weak.upgrade_in_event_loop(move |w| {
            myqbz_add::apply_rows(&w, rows);
        });
    });
}

/// Update the offline cache-status (+ progress) of every visible row matching
/// `track_id`. Mirrors `set_row_favorite`. status: 0 none / 1 queued / 2
/// downloading / 3 ready / 4 failed; `progress` is 0.0..1.0.
fn set_row_cache_status(window: &AppWindow, track_id: &str, status: i32, progress: f32) {
    let apply = |model: &slint::ModelRc<TrackItem>| {
        if let Some(vm) = model.as_any().downcast_ref::<slint::VecModel<TrackItem>>() {
            for i in 0..vm.row_count() {
                if let Some(mut item) = vm.row_data(i) {
                    if item.id == track_id
                        && (item.cache_status != status || item.cache_progress != progress)
                    {
                        item.cache_status = status;
                        item.cache_progress = progress;
                        vm.set_row_data(i, item);
                    }
                }
            }
        }
    };
    apply(&window.global::<AlbumState>().get_tracks());
    apply(&window.global::<ArtistState>().get_top_tracks());
    apply(&window.global::<SearchState>().get_tracks());
    apply(&window.global::<PlaylistState>().get_tracks());
    apply(&window.global::<MixState>().get_tracks());
    apply(&window.global::<FavoritesState>().get_tracks());

    let search = window.global::<SearchState>();
    let mut hero = search.get_most_popular_track();
    if hero.id == track_id {
        hero.cache_status = status;
        hero.cache_progress = progress;
        search.set_most_popular_track(hero);
    }

    // Keep the album header's "fully cached" gate live as the album's own
    // rows flip to ready (drives Make-available-offline -> Refresh in the
    // ⋯ menu). Only the open album view consults it.
    {
        let album = window.global::<AlbumState>();
        let tracks = album.get_tracks();
        let n = tracks.row_count();
        let fully = n > 0
            && (0..n).all(|i| tracks.row_data(i).is_some_and(|t| t.cache_status == 3));
        if album.get_album_fully_cached() != fully {
            album.set_album_fully_cached(fully);
        }
    }
}

/// Toggle the unlocking (padlock) flag of every visible row matching
/// `track_id`. Drives the offline-decrypt animation on the row.
fn set_row_unlocking(window: &AppWindow, track_id: &str, unlocking: bool) {
    let apply = |model: &slint::ModelRc<TrackItem>| {
        if let Some(vm) = model.as_any().downcast_ref::<slint::VecModel<TrackItem>>() {
            for i in 0..vm.row_count() {
                if let Some(mut item) = vm.row_data(i) {
                    if item.id == track_id && item.unlocking != unlocking {
                        item.unlocking = unlocking;
                        vm.set_row_data(i, item);
                    }
                }
            }
        }
    };
    apply(&window.global::<AlbumState>().get_tracks());
    apply(&window.global::<ArtistState>().get_top_tracks());
    apply(&window.global::<SearchState>().get_tracks());
    apply(&window.global::<PlaylistState>().get_tracks());
    apply(&window.global::<MixState>().get_tracks());
    apply(&window.global::<FavoritesState>().get_tracks());
}

/// Lazy-load the Discover > For You sections the first time the tab is
/// opened. No-op once loaded (the data persists for the session).
fn ensure_for_you_loaded(
    runtime: &Arc<AppRuntime<SlintAdapter>>,
    weak: &slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: &artwork::ImageCache,
) {
    let Some(w) = weak.upgrade() else {
        return;
    };
    if w.global::<ForYouState>().get_loaded() {
        return;
    }
    foryou::reset_loading(&w);
    foryou::spawn_for_you(runtime.clone(), weak.clone(), handle, image_cache.clone());
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
                // Carousel inputs, captured before `data` is moved into apply.
                let carousel_artist_id = data.artist_id.clone();
                let carousel_artist_name = data.artist.clone();
                // A user-set custom cover (keyed by album id) wins and is the
                // only image source for albums with no Qobuz cover. Same bug
                // class as the artist portrait fix.
                let custom_cover_path = crate::custom_artwork::album_cover(&album_id);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    album::apply_album(&w, data);
                    w.global::<AlbumState>().set_loading(false);
                });

                // Polish carousels — "From the same artist" + "Listening
                // suggestions". Qobuz-only (this is the Qobuz album path; local
                // albums load through navigate_local_album), best-effort: each
                // failure hides its own carousel. Loaded after the album so the
                // tracklist paints first.
                {
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let image_cache = image_cache.clone();
                    let album_id = album_id.clone();
                    tokio::spawn(async move {
                        let more = album::load_more_from_artist(
                            &runtime,
                            &carousel_artist_id,
                            &carousel_artist_name,
                            &album_id,
                        )
                        .await;
                        let image_cache_more = image_cache.clone();
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            let jobs = album::apply_more_from_artist(&w, more);
                            artwork::spawn_loads(jobs, w.as_weak(), image_cache_more);
                        });

                        let suggestions = album::load_suggestions(&runtime, &album_id).await;
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            let jobs = album::apply_suggestions(&w, suggestions);
                            artwork::spawn_loads(jobs, w.as_weak(), image_cache);
                        });
                    });
                }
                if let Some(path) = custom_cover_path {
                    if let Some((pixels, width, height)) = artwork::fetch_and_decode_ref(
                        &qbz_models::ArtworkRef::LocalFile(path),
                        &image_cache,
                        448,
                    )
                    .await
                    {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            album::apply_artwork(&w, &pixels, width, height);
                        });
                    }
                } else if !artwork_url.is_empty() {
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

/// Open a LOCAL album's detail in the shared AlbumPageView: load its tracks
/// (metadata-grouped), populate AlbumState with `is-local` set, then resolve
/// the folder/embedded cover from disk. `group_key` is the album's metadata
/// group key.
fn navigate_local_album(
    _runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    group_key: String,
) {
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<NavState>().set_view(ContentView::LocalAlbum);
        update_nav_flags(&w);
    });
    // The dedicated local album view owns the load (versions + cover).
    local_library::open_local_album(weak, handle.clone(), image_cache, group_key);
}

/// True when an "album id" is actually a Local-Library / Plex metadata group
/// key rather than a numeric Qobuz album id. Qobuz album ids are numeric
/// strings; local group keys are `album|artist`, a folder path, the
/// `__unknown_album__` sentinel, or a `plex:` cache key (see
/// qbz_library::album_grouping + local_queue_track / map_plex_cached_to_local_track).
/// Lets the shared `open-album` callback route Plex/local items (now-playing
/// bar, Home "Recently played", etc.) to the LocalAlbum view instead of the
/// empty Qobuz album view.
fn is_local_album_key(id: &str) -> bool {
    id.starts_with("plex:") || id.contains('|') || id.contains('/') || id == "__unknown_album__"
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
            // Reflect whether THIS artist is blacklisted so the overflow
            // menu shows Show/Blacklist correctly and the hidden banner
            // appears. is_blacklisted auto-gates on the enabled flag (reads
            // false when the feature is disabled, which is acceptable here).
            let is_bl = crate::artist_blacklist::is_blacklisted_id_str(&id_for_apply);
            let st = w.global::<ArtistState>();
            st.set_id(id_for_apply.into());
            st.set_is_blacklisted(is_bl);
            w.global::<NavState>().set_view(ContentView::Artist);
        });
        match artist::load_artist(&runtime, &artist_id).await {
            Ok(data) => {
                let artwork_url = data.artwork_url.clone();
                let jobs = artist::artwork_jobs(&data);
                let artist_name = data.name.clone();
                // Resolve a user-set custom portrait (keyed by artist name)
                // up front — it persists across navigation and is the ONLY
                // image source for artists with no Qobuz portrait (Vicky).
                let custom_image_path = crate::custom_artwork::artist_image(&artist_name);
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

                // Magazine / Stories — fetch the editorial teasers in
                // parallel; the sidebar section stays hidden if there are none.
                {
                    let runtime_story = runtime.clone();
                    let weak_story = weak.clone();
                    let artist_id_story = artist_id.clone();
                    let image_cache_story = image_cache.clone();
                    tokio::spawn(async move {
                        let stories = artist::load_stories(&runtime_story, &artist_id_story).await;
                        let _ = weak_story.upgrade_in_event_loop(move |w| {
                            let jobs = artist::apply_stories(&w, stories);
                            artwork::spawn_loads(jobs, w.as_weak(), image_cache_story);
                        });
                    });
                }

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

                if let Some(path) = custom_image_path {
                    // User-set custom portrait wins (and is the only source
                    // for artists with no Qobuz image).
                    if let Some((pixels, width, height)) = artwork::fetch_and_decode_ref(
                        &qbz_models::ArtworkRef::LocalFile(path),
                        &image_cache,
                        440,
                    )
                    .await
                    {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            artist::apply_artwork(&w, &pixels, width, height);
                        });
                    }
                } else if !artwork_url.is_empty() {
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

/// Open the Discography Builder for `artist_id` (spec 13). Fetches the artist's
/// releases from Qobuz (sets name + avatar), then local + Plex by that name
/// (sequential — parallelizing drops local matches against an empty name),
/// dedupes into groups, installs the default selection, and decodes the avatar.
/// Plex gets a single 2-second cold-start retry when enabled and the first
/// fetch returns nothing.
fn navigate_discography_builder(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    artist_id: String,
) {
    let id_for_reset = artist_id.clone();
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(move |w| {
            myqbz_builder::reset(&w, &id_for_reset);
            w.global::<NavState>().set_view(ContentView::DiscographyBuilder);
        });

        // 1. Qobuz first — sets artist name + avatar URL (side effect).
        match myqbz_builder::fetch_qobuz(&runtime, &artist_id).await {
            Ok((qobuz, artist_name, avatar_url)) => {
                // 2. Local + Plex by the resolved name (sequential, mandatory).
                let name_for_local = artist_name.clone();
                let mut local = tokio::task::spawn_blocking(move || {
                    myqbz_builder::fetch_local_and_plex(&name_for_local)
                })
                .await
                .unwrap_or_default();

                // 2b. Plex cold-start retry: if Plex is enabled and we got
                //     nothing from the Plex source, wait 2s and refetch once.
                let plex_enabled = crate::plex_settings::get().enabled;
                let got_plex = local.iter().any(|c| c.source == "plex");
                if plex_enabled && !got_plex {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    let name_retry = artist_name.clone();
                    let retried = tokio::task::spawn_blocking(move || {
                        myqbz_builder::fetch_local_and_plex(&name_retry)
                    })
                    .await
                    .unwrap_or_default();
                    if retried.iter().any(|c| c.source == "plex") {
                        local = retried;
                    }
                }

                // 3. Merge + group (Qobuz first so it wins primary ties).
                let mut all = qobuz;
                all.extend(local);
                let groups = myqbz_builder::build_groups(all);

                let avatar_for_fetch = avatar_url.clone();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    myqbz_builder::install(&w, artist_name, avatar_url, groups);
                });

                // 4. Decode the avatar (72px circle).
                if !avatar_for_fetch.is_empty() {
                    if let Some((pixels, width, height)) =
                        artwork::fetch_and_decode(&avatar_for_fetch, &image_cache, 144).await
                    {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            myqbz_builder::apply_avatar(&w, &pixels, width, height);
                        });
                    }
                }
            }
            Err(e) => {
                log::error!("[qbz-slint] discography builder load failed: {e}");
                let _ = weak.upgrade_in_event_loop(move |w| {
                    myqbz_builder::fail(&w, e);
                });
            }
        }
    });
}

thread_local! {
    /// Debounce timer for the header live search — restarted on every
    /// keystroke, fires the search 300 ms after typing stops.
    static SEARCH_DEBOUNCE: slint::Timer = slint::Timer::default();

    /// Debounce timer for the cortinilla (live dropdown) network load —
    /// restarted on every keystroke so the skeleton shows while typing and a
    /// single clean result paint lands ~220 ms after typing stops (no cached
    /// instant-paint, so results never "jump" from a cached to a fresh state).
    static CORTINILLA_DEBOUNCE: slint::Timer = slint::Timer::default();

    /// Snapshot of the cortinilla payload currently shown, so a
    /// `cortinilla-row-clicked(flat_index)` can resolve the flat index back to
    /// the concrete row (kind/id/source) and dispatch. UI thread only; set
    /// whenever `apply_cortinilla` writes a new payload.
    static LAST_CORTINILLA: std::cell::RefCell<Option<search::CortinillaData>> =
        const { std::cell::RefCell::new(None) };

    /// Snapshot of the raw `LocalTrack` rows that backed the cortinilla's "On
    /// this device" section currently shown. The click router resolves a local
    /// row (`source == "local"`) to its concrete `LocalTrack` here (the row's
    /// `id` is the library row id) so it can play through
    /// `playback::play_local_tracks`. Updated in lockstep with `LAST_CORTINILLA`
    /// whenever a cortinilla payload is applied. UI thread only.
    static LAST_CORTINILLA_LOCAL: std::cell::RefCell<Vec<qbz_library::LocalTrack>> =
        const { std::cell::RefCell::new(Vec::new()) };

    /// Debounce timer for the IMMERSIVE search dropdown network load — SEPARATE
    /// from `CORTINILLA_DEBOUNCE` so a keystroke in one surface never cancels
    /// the other's pending load. Same 220 ms single-shot skeleton-then-paint as
    /// the main cortinilla.
    static IMMERSIVE_SEARCH_DEBOUNCE: slint::Timer = slint::Timer::default();

    /// Snapshot of the immersive-search payload currently shown, so an
    /// immersive `row-clicked(flat_index)` / `move-selection(delta)` can resolve
    /// the flat index back to the concrete row and dispatch to playback. The
    /// immersive dropdown has no local section, so (unlike the main cortinilla)
    /// no parallel `LocalTrack` snapshot is needed. UI thread only; set whenever
    /// `apply_immersive_search` writes a new payload.
    static LAST_IMMERSIVE_SEARCH: std::cell::RefCell<Option<search::CortinillaData>> =
        const { std::cell::RefCell::new(None) };

    /// Stash for the "Duplicate tracks" confirm sub-modal. Slint can't hold a
    /// `Vec<u64>` ergonomically, so when a Qobuz→Qobuz add finds duplicates we
    /// park the full context here and the DuplicateConfirmActions handlers read
    /// it back. Cleared on add-all / add-new-only / cancel. The tuple is
    /// `(playlist_id, all_track_ids, duplicate_track_ids, playlist_name)`.
    static DUP_CONFIRM_STASH: std::cell::RefCell<
        Option<(u64, Vec<u64>, std::collections::HashSet<u64>, String)>
    > = const { std::cell::RefCell::new(None) };
}

/// Look up a playlist's display name from the picker state model by id
/// (the picker only carries names UI-side in `PlaylistPickItem`). Used for
/// the "Added N tracks to <name>" success toast. Falls back to an empty
/// string when the id is not found.
fn picker_playlist_name(w: &AppWindow, id: &str) -> String {
    use slint::Model;
    let model = w.global::<PlaylistPickerState>().get_playlists();
    for i in 0..model.row_count() {
        if let Some(item) = model.row_data(i) {
            if item.id == id {
                return item.name.to_string();
            }
        }
    }
    String::new()
}

/// Success toast for a playlist add ("Added N tracks to <playlist>"). Hops
/// onto the event loop, so it is safe to call from a worker task. An empty
/// `name` degrades to "Added N tracks". The count is the number actually
/// written.
fn toast_added_tracks(weak: &slint::Weak<AppWindow>, count: usize, name: String) {
    if count == 0 {
        return;
    }
    let msg = if name.is_empty() {
        format!("Added {count} tracks")
    } else {
        format!("Added {count} tracks to {name}")
    };
    crate::toast::success_weak(weak, msg);
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
/// Stable scroll-restore id for an entry's primary list container, matching
/// the `restore-scope` strings the Slint scroll containers compare against.
/// Returns `""` for views without a wired scroll memory (no container will
/// match, so nothing restores). Tab/sub-page views carry the tab in the id so
/// each tab keeps its own position. Keep in sync with the `.slint` views.
fn scope_for(entry: &nav::NavEntry) -> String {
    match entry {
        // HomeView is one persistent Flickable shared by the Discover tabs;
        // a single scope is enough (each tab entry stores its own scroll).
        nav::NavEntry::Home | nav::NavEntry::Discover { .. } => "home".into(),
        nav::NavEntry::Favorites { tab } => format!("fav:{tab}"),
        nav::NavEntry::LocalLibrary { tab } => format!("ll:{tab}"),
        nav::NavEntry::DiscoverBrowse { .. } => "discover-browse".into(),
        nav::NavEntry::Mix { .. } => "mix".into(),
        nav::NavEntry::Playlist(_) => "playlist".into(),
        nav::NavEntry::PlaylistManager => "playlist-manager".into(),
        nav::NavEntry::OfflineManager => "offline-manager".into(),
        nav::NavEntry::BlacklistManager => "blacklist-manager".into(),
        nav::NavEntry::Mixtapes => "mixtapes".into(),
        nav::NavEntry::Collections => "collections".into(),
        nav::NavEntry::MixtapeDetail(_) => "mixtape-detail".into(),
        nav::NavEntry::DiscographyBuilder(_) => "discography-builder".into(),
        nav::NavEntry::Album(_) => "album".into(),
        nav::NavEntry::LocalAlbum(_) => "local-album".into(),
        nav::NavEntry::Artist(_) => "artist".into(),
        nav::NavEntry::Settings => "settings".into(),
        nav::NavEntry::Search(_) => "search".into(),
        nav::NavEntry::Musician { .. } => "musician".into(),
        nav::NavEntry::Label { .. } => "label".into(),
        nav::NavEntry::LabelReleases { .. } => "label-releases".into(),
        nav::NavEntry::Award { .. } => "award".into(),
        nav::NavEntry::AwardAlbums { .. } => "award-albums".into(),
        nav::NavEntry::Purchases => "purchases".into(),
        nav::NavEntry::PurchaseDetail(_) => "purchase-album".into(),
        nav::NavEntry::ArtistReleases { .. } => "artist-releases".into(),
        nav::NavEntry::Location { .. } => "location".into(),
    }
}

/// Arm `NavState` so the destination scroll container restores its saved
/// position once it mounts. Must run before `apply_entry` switches the view.
fn arm_scroll_restore(weak: &slint::Weak<AppWindow>, entry: &nav::NavEntry, scroll: f32) {
    if let Some(w) = weak.upgrade() {
        let ns = w.global::<NavState>();
        ns.set_restore_scope(scope_for(entry).into());
        ns.set_scroll_restore(scroll);
    }
}

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
        nav::NavEntry::Discover { tab } => {
            let for_you = tab == "forYou";
            {
                let weak = weak.clone();
                let image_cache = image_cache.clone();
                let _ = weak.clone().upgrade_in_event_loop(move |w| {
                    w.global::<NavState>().set_view(ContentView::Home);
                    let jobs = home::select_tab(&w, &tab);
                    artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
                });
            }
            if for_you {
                ensure_for_you_loaded(runtime, weak, handle, image_cache);
            }
        }
        nav::NavEntry::Favorites { tab } => {
            if let Some(fav_tab) = favorites::FavTab::from_tab_id(&tab) {
                navigate_favorites(
                    runtime.clone(),
                    weak.clone(),
                    handle,
                    image_cache.clone(),
                    fav_tab,
                    &tab,
                );
            }
        }
        nav::NavEntry::LocalLibrary { tab } => {
            if let Some(lib_tab) = local_library::LibTab::from_tab_id(&tab) {
                navigate_local_library(
                    runtime.clone(),
                    weak.clone(),
                    handle,
                    image_cache.clone(),
                    lib_tab,
                );
            }
        }
        nav::NavEntry::Settings => {
            let _ = weak.upgrade_in_event_loop(|w| {
                seed_blacklist_status(&w);
                w.global::<NavState>().set_view(ContentView::Settings);
            });
        }
        nav::NavEntry::Album(id) => {
            navigate_album(runtime.clone(), weak.clone(), handle, image_cache.clone(), id);
        }
        nav::NavEntry::LocalAlbum(gk) => {
            navigate_local_album(runtime.clone(), weak.clone(), handle, image_cache.clone(), gk);
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
        nav::NavEntry::LabelReleases { id, name } => {
            navigate_label_releases(
                runtime.clone(),
                weak.clone(),
                handle,
                image_cache.clone(),
                id,
                name,
            );
        }
        nav::NavEntry::Award { id, name } => {
            navigate_award(runtime.clone(), weak.clone(), handle, image_cache.clone(), id, name);
        }
        nav::NavEntry::AwardAlbums { id, name } => {
            navigate_award_albums(
                runtime.clone(),
                weak.clone(),
                handle,
                image_cache.clone(),
                id,
                name,
            );
        }
        nav::NavEntry::Purchases => {
            navigate_purchases(runtime.clone(), weak.clone(), handle, image_cache.clone());
        }
        nav::NavEntry::PurchaseDetail(id) => {
            // Seed the bound id + switch the view; the detail view's
            // `changed album-id` handler fires the fresh fetch (§A.3 reactive
            // reload), so back/forward between two purchase albums never shows
            // stale data. The conditional-mount guard requires the non-empty id.
            navigate_purchase_detail(weak.clone(), &id);
        }
        nav::NavEntry::ArtistReleases {
            id,
            name,
            release_type,
        } => {
            navigate_artist_releases(
                runtime.clone(),
                weak.clone(),
                handle,
                image_cache.clone(),
                id,
                name,
                release_type,
            );
        }
        nav::NavEntry::DiscoverBrowse { endpoint, title } => {
            discover_browse::navigate(
                runtime.clone(),
                weak.clone(),
                handle,
                image_cache.clone(),
                endpoint,
                title,
                current_genre_filter(),
            );
        }
        nav::NavEntry::Mix { kind } => {
            navigate_mix(runtime.clone(), weak.clone(), handle, image_cache.clone(), kind);
        }
        nav::NavEntry::Playlist(id) => {
            navigate_playlist(runtime.clone(), weak.clone(), handle, image_cache.clone(), id);
        }
        nav::NavEntry::PlaylistManager => {
            playlist_manager::navigate(
                runtime.clone(),
                weak.clone(),
                handle,
                image_cache.clone(),
            );
        }
        nav::NavEntry::OfflineManager => {
            let w2 = weak.clone();
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<NavState>().set_view(ContentView::OfflineManager);
            });
            offline_manager::load(w2, handle.clone());
        }
        nav::NavEntry::BlacklistManager => {
            let w2 = weak.clone();
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<NavState>().set_view(ContentView::BlacklistManager);
            });
            blacklist_manager::load(w2);
        }
        nav::NavEntry::Mixtapes => {
            myqbz::navigate(
                weak.clone(),
                handle.clone(),
                image_cache.clone(),
                qbz_models::mixtape::CollectionKind::Mixtape,
            );
        }
        nav::NavEntry::Collections => {
            myqbz::navigate(
                weak.clone(),
                handle.clone(),
                image_cache.clone(),
                qbz_models::mixtape::CollectionKind::Collection,
            );
        }
        nav::NavEntry::MixtapeDetail(id) => {
            myqbz_detail::navigate(
                runtime.clone(),
                weak.clone(),
                handle.clone(),
                image_cache.clone(),
                id,
            );
        }
        nav::NavEntry::DiscographyBuilder(artist_id) => {
            navigate_discography_builder(
                runtime.clone(),
                weak.clone(),
                handle,
                image_cache.clone(),
                artist_id,
            );
        }
        nav::NavEntry::Location {
            mbid,
            area_id,
            area_name,
            country,
            genres,
            tags,
        } => {
            let params = artist::LocationParams {
                mbid,
                area_id,
                area_name,
                country,
                genres,
                tags,
            };
            navigate_location(runtime.clone(), weak.clone(), handle, image_cache.clone(), params);
        }
    }
}

/// Open an ArtistsByLocationView for the given scene params. Runs the
/// discovery on a worker, applies the validated artist grid, then
/// fans out artwork jobs for the candidates' Qobuz thumbnails.
fn navigate_location(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    params: artist::LocationParams,
) {
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            location_view::reset_scene(&w);
            w.global::<NavState>().set_view(ContentView::Location);
        });
        match location_view::load_scene(&runtime, &params, 0).await {
            Ok(data) => {
                let jobs = location_view::artwork_jobs(&data);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    location_view::apply_scene(&w, data);
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
            }
            Err(e) => {
                log::error!("[qbz-slint] scene discovery failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<LocationViewState>().set_loading(false);
                });
            }
        }
    });
}

/// Open the LabelView landing — the rich label page (header + popular
/// tracks + releases/critics/playlists/artists/more-labels carousels).
/// Reached by clicking a label anywhere.
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
            label::reset_label_page(&w);
            w.global::<NavState>().set_view(ContentView::Label);
        });
        match label::load_label_page(&runtime, label_id, &name).await {
            Ok(payload) => {
                let jobs = label::page_artwork_jobs(&payload);
                let image_url = payload.image_url.clone();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    label::apply_label_page(&w, payload);
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
                log::error!("[qbz-slint] label page load failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    let s = w.global::<LabelState>();
                    s.set_loading(false);
                    s.set_page_loaded(true);
                });
            }
        }
    });
}

/// Open the full "See all releases" sub-view for `label_id`. Fetches the
/// label header + first album page, then the header image.
fn navigate_label_releases(
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
            w.global::<NavState>().set_view(ContentView::LabelReleases);
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
                log::error!("[qbz-slint] label releases load failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<LabelState>().set_loading(false);
                });
            }
        }
    });
}

/// Open the dedicated discography page for one release bucket of an artist.
/// Reuses `artist::load_release_page` (get_releases_grid) for the first page.
fn navigate_artist_releases(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    artist_id: String,
    name: String,
    release_type: String,
) {
    handle.spawn(async move {
        let title = artist::release_type_title(&release_type);
        let aid = artist_id.clone();
        let nm = name.clone();
        let rt = release_type.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            artist_releases::reset(&w, &aid, &nm, &rt, &title);
            w.global::<NavState>().set_view(ContentView::ArtistReleases);
        });
        match artist::load_release_page(&runtime, &artist_id, &release_type, 0).await {
            Ok((cards, has_more)) => {
                let image_cache = image_cache.clone();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let jobs = artist_releases::apply_page(&w, cards, has_more, true);
                    artwork::spawn_loads(jobs, w.as_weak(), image_cache);
                });
            }
            Err(e) => {
                log::error!("[qbz-slint] artist releases load failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    let st = w.global::<ArtistReleasesState>();
                    st.set_loading(false);
                    st.set_load_error(true);
                });
            }
        }
    });
}

/// Open the AwardView landing for `award_id`: hero + first /award/getAlbums
/// preview page + other-awards rail, then the header image.
fn navigate_award(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    award_id: String,
    name: String,
) {
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            award::reset_award_page(&w);
            w.global::<NavState>().set_view(ContentView::Award);
        });
        let payload = award::load_award_page(&runtime, &award_id, &name).await;
        let jobs = award::page_artwork_jobs(&payload);
        let image_url = payload.image_url.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            award::apply_award_page(&w, payload);
        });
        artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
        if !image_url.is_empty() {
            if let Some((pixels, width, height)) =
                artwork::fetch_and_decode(&image_url, &image_cache, 240).await
            {
                let _ = weak.upgrade_in_event_loop(move |w| {
                    award::apply_image(&w, &pixels, width, height);
                });
            }
        }
    });
}

/// Load the immersive Suggestions split panel for the current track. Reads the
/// artist-id + track-id + title off NowPlayingState, resets the panel, and
/// spawns the live artist load (mirror of navigate_award). An empty artist-id
/// or track-id resets to the no-track empty state. Refreshed on track change
/// while the panel is open via the mount's `changed live-track-id` -> the
/// SuggestionsActions.load callback (which calls this).
fn navigate_suggestions(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    artist_id: String,
    track_id: String,
    track_name: String,
) {
    // No track / no artist -> reset to the empty state and stop. apply with an
    // empty payload clears cards/tracks and leaves artist-id "" (the empty
    // branch in the panel).
    let (Ok(aid), Ok(tid)) = (artist_id.parse::<u64>(), track_id.parse::<u64>()) else {
        let _ = weak.upgrade_in_event_loop(|w| {
            suggestions::apply_suggestions(&w, suggestions::empty_payload());
        });
        return;
    };
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            suggestions::reset_suggestions(&w);
        });
        let payload = suggestions::load_suggestions(&runtime, aid, tid, track_name).await;
        let jobs = suggestions::suggestions_artwork_jobs(&payload);
        let _ = weak.upgrade_in_event_loop(move |w| {
            suggestions::apply_suggestions(&w, payload);
        });
        artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
    });
}

/// Open the full AwardAlbumsView listing for `award_id` (paginated grid +
/// client-side search). No hero — the kicker uses the passed name (1:1 Tauri).
fn navigate_award_albums(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    award_id: String,
    name: String,
) {
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            award::reset_award_albums(&w);
            w.global::<NavState>().set_view(ContentView::AwardAlbums);
        });
        match award::load_award_albums(&runtime, &award_id, &name).await {
            Ok(payload) => {
                let jobs = award::albums_artwork_jobs(&payload.albums, 0);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    award::apply_award_albums(&w, payload);
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
            }
            Err(e) => {
                log::error!("[qbz-slint] award albums load failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    let s = w.global::<AwardState>();
                    s.set_loading(false);
                    s.set_load_error(true);
                });
            }
        }
    });
}

/// Open the My-Purchases surface and lazy-load the active tab. Mirrors
/// `navigate_favorites`: the view mounts immediately (spinner), then the active
/// tab's data + the metadata (dlIds + per-type totals) load and project onto
/// `PurchasesState`. The toolbar/filter state survives navigation in the
/// controller cache, so re-entering shows the same view without a refetch.
fn navigate_purchases(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
) {
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<NavState>().set_view(ContentView::Purchases);
    });
    // Lazy-load the active tab (skip if already cached — the switch-back quirk).
    let active = purchases_active_tab(&weak);
    load_purchases_tab(runtime, weak, handle.clone(), image_cache, active, false, false);
}

/// Read the active purchases tab off `PurchasesState` (default Albums).
fn purchases_active_tab(weak: &slint::Weak<AppWindow>) -> purchases::PurchaseTab {
    weak.upgrade()
        .map(|w| {
            if w.global::<PurchasesState>().get_active_tab() == "tracks" {
                purchases::PurchaseTab::Tracks
            } else {
                purchases::PurchaseTab::Albums
            }
        })
        .unwrap_or(purchases::PurchaseTab::Albums)
}

/// Map a `load_purchases_by_tab` error to its display string: the i18n key
/// `purchases.loadFailed` resolves to its (English) text — the list view
/// i18n-maps `purchases.`-prefixed errors — while a raw error passes through.
fn map_purchases_error_display(err: &str) -> String {
    if err == "purchases.loadFailed" {
        qbz_i18n::t("Couldn't load purchases. Check your connection.")
    } else {
        err.to_string()
    }
}

/// Load (or refresh) one purchases tab: set loading, fetch metadata + the tab
/// list, then apply or surface the error. `force` skips the cache guard;
/// `search_overwrote` marks BOTH tabs loaded (the search path). On success
/// spawns artwork for the tab's covers.
fn load_purchases_tab(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    tab: purchases::PurchaseTab,
    force: bool,
    search_overwrote: bool,
) {
    // Cache guard (Svelte `loadPurchasesByTab` early-return): skip the refetch
    // when the tab is already loaded and we're not forcing.
    if !force && purchases::tab_cached(tab) {
        let _ = weak.upgrade_in_event_loop(move |w| {
            // Still ensure the rendered models reflect the current toolbar state.
            purchases::set_loading_done(&w);
            purchases::derive_purchases(&w);
        });
        return;
    }
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            purchases::set_loading(&w);
        });
        let metadata = purchases::load_purchases_metadata(&runtime).await;
        match purchases::load_purchases_by_tab(&runtime, tab, &metadata).await {
            Ok(payload) => {
                let _ = weak.upgrade_in_event_loop(move |w| {
                    purchases::apply_purchases_tab(&w, tab, payload, &metadata, search_overwrote);
                });
                let jobs = purchases::artwork_jobs_for_tab(tab);
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
            }
            Err(e) => {
                let display = map_purchases_error_display(&e);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    purchases::set_load_error(&w, &display);
                });
            }
        }
    });
}

/// Execute a tracks-tab per-track download for a chosen format (§2.1.13
/// `executeTrackDownload`): folder-pick (cancel = abort), derive the qualityDir
/// from the format label, then fire the single-track download. The live row
/// status refreshes via the controller's `start_track_download` refresh nudge.
fn execute_track_download(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    track_id: u64,
    fmt: qbz_models::PurchaseFormatOption,
) {
    let Some(track) = purchases::find_track(track_id) else {
        return;
    };
    let album_id = match track.album.as_ref() {
        Some(a) if !a.id.is_empty() => a.id.clone(),
        _ => return,
    };
    let handle2 = handle.clone();
    handle.spawn(async move {
        // Folder picker (cancel → abort, Svelte `if (!dest) return`).
        let Some(dest) = purchases::pick_download_folder().await else {
            return;
        };
        let quality_dir = purchases::quality_dir(&fmt.label);
        purchases::start_track_download(
            runtime,
            weak,
            handle2,
            album_id,
            track_id,
            fmt.id,
            dest,
            quality_dir,
        );
    });
}

/// Open the PurchaseDetailView for `album_id`. Seeds the bound id on
/// `PurchaseDetailState`, resets the previous album's content (so an
/// album→album jump never flashes stale data), and switches the view. The
/// actual fetch is fired by the view's `changed album-id => load(id)` handler
/// (§A.3 reactive reload) — `load_purchase_detail` runs that load. The
/// conditional-mount guard in AppShell requires the non-empty id we set here.
fn navigate_purchase_detail(weak: slint::Weak<AppWindow>, album_id: &str) {
    let album_id = album_id.to_string();
    let _ = weak.upgrade_in_event_loop(move |w| {
        purchases::reset_detail(&w);
        w.global::<PurchaseDetailState>().set_album_id(album_id.into());
        w.global::<NavState>().set_view(ContentView::PurchaseAlbum);
    });
}

/// Fetch + apply the detail album (Svelte `loadAlbum`, §2.2.2). Sets the
/// loading shell, loads the `PurchaseAlbum` + formats via the controller, then
/// applies + spawns the header-cover artwork. On error surfaces the RAW string
/// (NOT i18n-mapped, unlike the list view — §2.2.4). Re-entrant safe: a newer
/// `album-id` change supersedes an in-flight load by reseeding the cache.
fn load_purchase_detail(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    album_id: String,
) {
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            purchases::set_detail_loading(&w);
        });
        match purchases::load_purchase_album(&runtime, &album_id).await {
            Ok(payload) => {
                let _ = weak.upgrade_in_event_loop(move |w| {
                    purchases::apply_detail(&w, payload);
                });
                let jobs = purchases::detail_artwork_jobs();
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
            }
            Err(e) => {
                let _ = weak.upgrade_in_event_loop(move |w| {
                    // RAW error (NOT i18n-mapped, §2.2.4).
                    purchases::set_detail_error(&w, &e);
                });
            }
        }
    });
}

/// Open Library > Favorites on `tab` and lazy-load that tab's data.
/// Switching the active tab also routes here so each tab fetches on
/// first view (Tauri's loadTabIfNeeded).
fn navigate_favorites(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    tab: favorites::FavTab,
    tab_id: &str,
) {
    let tab_id = tab_id.to_string();
    handle.spawn(async move {
        let tab_id_for_ui = tab_id.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            let state = w.global::<FavoritesState>();
            state.set_active_tab(tab_id_for_ui.into());
            favorites::reset_loading(&w);
            w.global::<NavState>().set_view(ContentView::Favorites);
            // The genre popup edits the favorites context here, and the
            // toolbar genre button shows the favorites selection count.
            genre_filter::set_context("favorites");
            genre_filter::apply_state(&w);
            // Restore persisted toolbar choices before the data applies +
            // derives, so the loaded view honors them.
            favorites_prefs::load(&w);
        });
        match favorites::load_favorites(&runtime, tab).await {
            Ok(data) => {
                let jobs = favorites::artwork_jobs(&data);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    favorites::apply_favorites(&w, data);
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
            }
            Err(e) => {
                log::error!("[qbz-slint] favorites load failed: {e}");
                let msg = e.to_string();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let st = w.global::<FavoritesState>();
                    st.set_loading(false);
                    st.set_load_error(msg.into());
                });
            }
        }
    });
}

/// Navigate to the LocalLibrary Artists tab and auto-select `name`. Local/Plex
/// artists have no id — they're keyed by NAME. The selection is latched and
/// consumed by `ensure_artists_loaded` once the tab's data is ready (handles
/// both the already-loaded and still-loading cases). Used by the LocalAlbum
/// header artist link, the now-playing "Go to artist", and local track menus.
fn open_local_artist(
    runtime: &Arc<AppRuntime<SlintAdapter>>,
    weak: &slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: &artwork::ImageCache,
    name: String,
) {
    if name.trim().is_empty() {
        return;
    }
    local_library::set_pending_artist(name);
    nav::record(nav::NavEntry::LocalLibrary {
        tab: "artists".to_string(),
    });
    navigate_local_library(
        runtime.clone(),
        weak.clone(),
        handle,
        image_cache.clone(),
        local_library::LibTab::Artists,
    );
    if let Some(w) = weak.upgrade() {
        update_nav_flags(&w);
    }
}

/// "Go to album" / "Go to artist" for a LOCAL-surface track row
/// (LocalLibrary Tracks tab / folder detail / local album detail) — an
/// owner improvement over Tauri, which omits both entries on local rows.
/// Source-routed (same split as the MyQBZ artist links and the real-id
/// favorite entry):
///   - local rows -> the LOCAL album view by the row's `album_group_key`
///     (the same navigation key the now-playing bar's "Go to album" uses)
///     / the LocalLibrary Artists tab by NAME (local artists have no id).
///   - plex rows  -> the LOCAL album view via the content-hash
///     `plex_album_key(artist, album)` — the row's `album_group_key` is
///     the per-edition split key the Plex album cache is NOT keyed by
///     (`local_queue_track` parity) — / LocalLibrary artist by name.
///   - qobuz_download rows -> the REAL Qobuz pages. The library index
///     carries ONLY `qobuz_track_id` (no Qobuz album/artist id columns),
///     so the target ids are recovered with the same `get_track` resolve
///     the Qobuz surfaces' go-to arms use; when the resolve can't deliver
///     (offline / API error / missing id) the row falls back to the LOCAL
///     destinations above, so the click always lands.
/// The window's open-album / open-artist callbacks do the final routing
/// (and the history recording).
fn local_row_goto(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    row: qbz_library::LocalTrack,
    to_artist: bool,
) {
    let album_key = if row.source.as_deref() == Some("plex") {
        qbz_plex::plex_album_key(&row.artist, &row.album)
    } else {
        row.album_group_key.clone()
    };
    let artist_name = row.artist.clone();
    // Local destination (the primary route for local/plex rows, the
    // fallback for qobuz_download ones). FnOnce — each path calls it at
    // most once, on the UI thread.
    let open_local = move |w: &AppWindow| {
        if to_artist {
            if artist_name.trim().is_empty() {
                log::debug!("[qbz-slint] go-to-artist: local row has no artist name");
                return;
            }
            w.invoke_open_artist(artist_name.into());
        } else {
            if album_key.is_empty() {
                log::debug!("[qbz-slint] go-to-album: local row has no album group key");
                return;
            }
            w.invoke_open_album(album_key.into());
        }
    };
    let qobuz_id = (row.source.as_deref() == Some("qobuz_download"))
        .then_some(row.qobuz_track_id)
        .flatten();
    match qobuz_id {
        Some(qid) if qid > 0 => {
            handle.spawn(async move {
                let resolved: Option<String> = match runtime.core().get_track(qid as u64).await {
                    Ok(track) => {
                        if to_artist {
                            track.performer.as_ref().map(|p| p.id.to_string())
                        } else {
                            track.album.as_ref().map(|a| a.id.clone())
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "[qbz-slint] go-to: get_track {qid} failed ({e}) — using the local destination"
                        );
                        None
                    }
                };
                let _ = weak.upgrade_in_event_loop(move |w| match resolved {
                    Some(qobuz_ref) if to_artist => w.invoke_open_artist(qobuz_ref.into()),
                    Some(qobuz_ref) => w.invoke_open_album(qobuz_ref.into()),
                    None => open_local(&w),
                });
            });
        }
        _ => {
            let _ = weak.upgrade_in_event_loop(move |w| open_local(&w));
        }
    }
}

/// Open a Local Library browse tab (Albums / Artists / Folders / Tracks).
///
/// Sets the active tab + switches the view, then lazily loads the tab's data
/// on first visit. Albums is the first slice (chunked grid); the other tabs
/// render their shell + a placeholder until their slices land.
fn navigate_local_library(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    tab: local_library::LibTab,
) {
    let tab_id = tab.tab_id().to_string();
    let _ = weak.upgrade_in_event_loop(move |w| {
        // Restore the persisted Tracks group-by before the tab derives.
        locallibrary_prefs::load(&w);
        w.global::<LocalLibraryState>().set_active_tab(tab_id.into());
        w.global::<NavState>().set_view(ContentView::LocalLibrary);
    });
    // Seed all four tab-badge counts up front (like Favorites) so the nav
    // badges show without visiting each tab.
    local_library::seed_counts(weak.clone(), handle.clone());
    // Lazy per-tab load on first visit.
    match tab {
        local_library::LibTab::Albums => {
            local_library::ensure_albums_loaded(weak, handle.clone(), image_cache);
        }
        local_library::LibTab::Folders => {
            // Tree is the default mode → load the tree roots too (the flat set
            // stays loaded so toggling to flat is instant).
            local_library::ensure_folders_loaded(weak.clone(), handle.clone(), image_cache);
            local_library::ensure_folder_tree_loaded(weak, handle.clone());
        }
        local_library::LibTab::Tracks => {
            local_library::ensure_tracks_loaded(weak, handle.clone());
        }
        local_library::LibTab::Artists => {
            local_library::ensure_artists_loaded(runtime, weak, handle.clone(), image_cache);
        }
    }
}

/// Open a Qobuz mix detail view (daily / weekly / fav / top) and load
/// its tracks.
fn navigate_mix(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    kind: String,
) {
    handle.spawn(async move {
        let kind_for_reset = kind.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            mix::reset_mix(&w, &kind_for_reset);
            w.global::<NavState>().set_view(ContentView::Mix);
        });
        let tracks = mix::load_mix(&runtime, &kind).await;
        let jobs = mix::artwork_jobs(&tracks);
        let _ = weak.upgrade_in_event_loop(move |w| {
            mix::apply_mix(&w, &kind, tracks);
        });
        artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
    });
}

fn navigate_playlist(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    playlist_id: String,
) {
    // Route by id namespace (D7 type guard): `local:<uuid>` ids open the
    // LOCAL detail path and can never reach the Qobuz fetch below.
    let id = match local_playlist::PlaylistRef::parse(&playlist_id) {
        Some(local_playlist::PlaylistRef::Local(id)) => {
            local_playlist::navigate(runtime, weak, handle, image_cache, id);
            return;
        }
        Some(local_playlist::PlaylistRef::Qobuz(id)) => {
            // D11.a: offline, a mixed playlist's detail renders ONLY its
            // local sidecar rows — the Qobuz membership is not enumerable
            // offline, so the API fetch below never runs.
            if offline_mode::engine().is_offline() {
                local_playlist::navigate_qobuz_offline(weak, handle, image_cache, id);
                return;
            }
            id
        }
        None => {
            log::warn!("[qbz-slint] navigate_playlist: bad id {playlist_id}");
            return;
        }
    };
    handle.spawn(async move {
        let active = playlist_id.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            playlist::reset(&w);
            sidebar::set_active(&w, &active);
            w.global::<NavState>().set_view(ContentView::Playlist);
        });
        if let Some(data) = playlist::load(&runtime, id).await {
            // Mixed rows split across loaders like the LOCAL detail:
            // Qobuz rows = http covers, local sidecar rows = file paths,
            // plex rows = tokenized Plex thumbs.
            let (http_jobs, local_jobs, plex_jobs) = playlist::artwork_jobs(&data);
            let pid = data.id.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                playlist::apply(&w, data);
                let owned = sidebar::contains(&w, &pid);
                w.global::<PlaylistState>().set_is_owner(owned);
            });
            if !http_jobs.is_empty() {
                artwork::spawn_loads(http_jobs, weak.clone(), image_cache.clone());
            }
            if !local_jobs.is_empty() {
                artwork::spawn_local_loads(local_jobs, weak.clone(), image_cache.clone());
            }
            if !plex_jobs.is_empty() {
                let plex = plex_settings::get();
                artwork::spawn_local_or_plex_loads(
                    plex_jobs,
                    plex.base_url,
                    plex.token,
                    weak.clone(),
                    image_cache.clone(),
                );
            }
        }
    });
}

/// Namespace-split removal from the ONLINE Qobuz playlist detail (Seam D):
/// Qobuz rows go to the Qobuz API as `playlist_track_id`s (resolved through
/// the loaded detail — fixing the old bulk path that shipped TRACK ids),
/// local rows to `remove_local_track_from_playlist`, plex rows to
/// `remove_plex_track_from_playlist`; then the detail reloads (re-merge).
/// The bulk bar calls this with the selection; the per-row "Remove from
/// playlist" menu entry (follow-up) calls it with a single row.
fn playlist_remove_rows(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    pid: u64,
    rows: Vec<playlist::SelectedRow>,
) {
    // Resolve on the UI thread: ptids from the loaded Track cache, plex
    // keys from the open queue snapshot.
    let split = playlist::split_for_removal(&rows);
    if split.playlist_track_ids.is_empty()
        && split.local_track_ids.is_empty()
        && split.plex_keys.is_empty()
    {
        log::warn!("[qbz-slint] playlist {pid}: nothing resolvable in the removal selection");
        return;
    }
    handle.clone().spawn(async move {
        let local_ids = split.local_track_ids;
        let plex_keys = split.plex_keys;
        if !local_ids.is_empty() || !plex_keys.is_empty() {
            let _ = tokio::task::spawn_blocking(move || {
                crate::library_db::with_db(|db| {
                    for rid in &local_ids {
                        db.remove_local_track_from_playlist(pid, *rid)?;
                    }
                    for key in &plex_keys {
                        db.remove_plex_track_from_playlist(pid, key)?;
                    }
                    Ok(())
                })
            })
            .await;
        }
        if !split.playlist_track_ids.is_empty() {
            if let Err(e) = runtime
                .core()
                .remove_tracks_from_playlist(pid, &split.playlist_track_ids)
                .await
            {
                log::error!("[qbz-slint] remove tracks from playlist failed: {e}");
            }
        }
        // Reload + leave edit mode (the reload re-merges the sidecar).
        let _ = weak.upgrade_in_event_loop(|w| {
            playlist::set_multi_select(&w, false);
        });
        navigate_playlist(
            runtime.clone(),
            weak.clone(),
            &handle,
            image_cache.clone(),
            pid.to_string(),
        );
    });
}

/// True while the OPEN view is a playlist detail whose rows ride the merged
/// queue snapshot (LOCAL detail / offline subset / ONLINE mixed detail) —
/// the guard for consulting snapshot row ids from the universal track arms.
/// Only then may a row id be a library row id / synthetic Plex id; a stale
/// snapshot id could otherwise collide with a genuine Qobuz catalog id from
/// another surface (both are small integers).
fn snapshot_detail_open(w: &AppWindow) -> bool {
    w.global::<NavState>().get_view() == ContentView::Playlist
        && (w.global::<PlaylistState>().get_is_local()
            || w.global::<PlaylistState>().get_offline_subset()
            || playlist::is_mixed())
}

/// Type a LocalLibrary row for the drag payload: Plex rows carry their
/// rating key (their row id is synthetic — never resolvable in
/// `local_tracks`), everything else its real library row id.
fn local_drag_track(track: &qbz_library::LocalTrack) -> drag::DragTrack {
    if track.source.as_deref() == Some("plex") {
        drag::DragTrack::Plex(track.file_path.clone())
    } else {
        drag::DragTrack::LocalRow(track.id)
    }
}

/// Build a playlist-picker local-mode ref for a LocalLibrary row: Plex rows
/// carry their rating key ("plex:<key>" — their synthetic row id never
/// resolves through `get_track`), everything else its library row id.
fn local_picker_ref(track: &qbz_library::LocalTrack) -> String {
    if track.source.as_deref() == Some("plex") {
        format!("plex:{}", track.file_path)
    } else {
        track.id.to_string()
    }
}

/// Type a model row (Playlist / Artist surfaces) for the drag payload.
/// The LOCAL playlist detail mixes namespaces: "plex:<key>" unresolved
/// Plex rows, NUMERIC synthetic ids on RESOLVED Plex rows (`source ==
/// "plex"` — the rating key is recovered from the open detail's queue
/// snapshot, NEVER typed as a Qobuz id), library row ids on `source ==
/// "local"` rows, Qobuz catalog ids on everything else (incl.
/// offline-cached rows). Render-only rows ("file:"/"broken:" fallbacks)
/// type to None and drop out of the drag.
fn row_drag_track(row: &TrackItem) -> Option<drag::DragTrack> {
    let id = row.id.to_string();
    if let Some(key) = id.strip_prefix("plex:") {
        return Some(drag::DragTrack::Plex(key.to_string()));
    }
    if row.source.as_str() == "plex" {
        // Resolved Plex row: numeric display id; the rating key lives in
        // the queue snapshot. No key recoverable -> drop from the drag;
        // falling through to the Qobuz parse would store the synthetic id
        // as a catalog id (the exact garbage class found in the field).
        return local_playlist::plex_key_for_row(&id).map(drag::DragTrack::Plex);
    }
    if row.source.as_str() == "local" {
        return id.parse::<i64>().ok().map(drag::DragTrack::LocalRow);
    }
    id.parse::<u64>().ok().map(drag::DragTrack::Qobuz)
}

/// Resolve the SOURCE-TYPED track refs for a drag started on `track_id`
/// — the id namespace depends on the view the drag started in (Qobuz
/// surfaces carry catalog ids; LocalLibrary surfaces carry library row
/// ids, Plex rows rating keys). If the current view has a multi-selection
/// that includes the dragged row (and is >1), the whole selection is
/// dragged; otherwise just the row. Mirrors Tauri's group-drag rule.
fn gather_drag_tracks(w: &AppWindow, track_id: &str) -> Vec<drag::DragTrack> {
    use slint::Model;
    let view = w.global::<NavState>().get_view();
    match view {
        ContentView::LocalAlbum => {
            // Single-row surface; resolve through the open album's version
            // cache (the only place a Plex row's rating key lives).
            local_library::current_album_version_tracks(w)
                .iter()
                .find(|t| t.id.to_string() == track_id)
                .map(|t| vec![local_drag_track(t)])
                .unwrap_or_default()
        }
        ContentView::LocalLibrary => {
            // Tracks tab (group-drag over the multi-selection first).
            let selected = local_library::selected_local_tracks(w);
            if selected.len() > 1 && selected.iter().any(|t| t.id.to_string() == track_id) {
                return selected.iter().map(local_drag_track).collect();
            }
            if let Some(track) = local_library::local_track_by_id(track_id) {
                return vec![local_drag_track(&track)];
            }
            // Folder-detail rows aren't in the Tracks cache but are real
            // library rows — type by row id (resolved at insert).
            track_id
                .parse::<i64>()
                .map(|id| vec![drag::DragTrack::LocalRow(id)])
                .unwrap_or_default()
        }
        ContentView::Playlist | ContentView::Artist => {
            let model = match view {
                ContentView::Playlist => w.global::<PlaylistState>().get_tracks(),
                _ => w.global::<ArtistState>().get_top_tracks(),
            };
            let rows: Vec<TrackItem> = (0..model.row_count())
                .filter_map(|i| model.row_data(i))
                .collect();
            let selected: Vec<drag::DragTrack> = rows
                .iter()
                .filter(|t| t.selected)
                .filter_map(row_drag_track)
                .collect();
            if selected.len() > 1 && rows.iter().any(|t| t.selected && t.id == track_id) {
                return selected;
            }
            if let Some(row) = rows.iter().find(|t| t.id == track_id) {
                return row_drag_track(row).map(|d| vec![d]).unwrap_or_default();
            }
            track_id
                .parse::<u64>()
                .map(|id| vec![drag::DragTrack::Qobuz(id)])
                .unwrap_or_default()
        }
        // Every other surface (album / search / favorites / mix / …) is
        // Qobuz-backed: rows carry catalog ids.
        _ => track_id
            .parse::<u64>()
            .map(|id| vec![drag::DragTrack::Qobuz(id)])
            .unwrap_or_default(),
    }
}

/// Load (or reload) the sidebar playlists list off-thread.
fn load_sidebar_playlists(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
) {
    let _ = weak.upgrade_in_event_loop(|w| sidebar::set_loading(&w, true));
    handle.spawn(async move {
        let data = sidebar::load(&runtime).await;
        let _ = weak.upgrade_in_event_loop(move |w| {
            sidebar::apply(&w, data);
            refresh_sidebar_covers(&w);
        });
    });
}

/// (Re)spawn the per-playlist micro-collage cover downloads for the
/// current `SidebarState.entries`. Called after any rebuild that replaces
/// the rows (load / toggle / move / sort / search), since `set_row_data`
/// resets the decoded cover images. Each completion updates only its own
/// row (see artwork.rs), and the shared image cache means already-fetched
/// covers resolve from disk without a re-download.
fn refresh_sidebar_covers(window: &AppWindow) {
    if let Some(cache) = artwork::shared_cache() {
        let (qobuz_jobs, local_jobs) = sidebar::artwork_jobs(window);
        if !qobuz_jobs.is_empty() {
            artwork::spawn_loads(qobuz_jobs, window.as_weak(), cache.clone());
        }
        // LOCAL playlist collage covers are file paths / Plex thumb paths — route
        // them through the local-or-Plex loader (http loader would miss them).
        if !local_jobs.is_empty() {
            let plex = plex_settings::get();
            artwork::spawn_local_or_plex_loads(
                local_jobs,
                plex.base_url,
                plex.token,
                window.as_weak(),
                cache,
            );
        }
    }
}

/// Reseed the AppearanceState dropdown OPTION arrays from the Rust-side i18n
/// catalog (`qbz_i18n::t`). These arrays are declared in `ui/state.slint` as
/// `@tr(...)` PROPERTY DEFAULTS, which are evaluated once and do NOT react to a
/// runtime `select_bundled_translation()` switch — so without this reseed the
/// option contents stay in the language that was active at first paint.
/// `QbzSelect` binds `options` live, so writing the arrays here updates the
/// rendered dropdowns immediately.
///
/// Call this (a) at startup right after `select_bundled_translation` (post
/// `AppWindow::new()`) and (b) in the "language" appearance-select arm after the
/// translation switch. Only the string arrays are reseeded; the `*_index`
/// selection properties are intentionally left untouched. Brand names and
/// language endonyms (macOS, Adwaita, English, Español, …) stay as literals.
fn reseed_i18n_labels(window: &AppWindow) {
    use slint::{ModelRc, SharedString, VecModel};
    let t = |s: &str| SharedString::from(qbz_i18n::t(s));
    let state = window.global::<AppearanceState>();

    state.set_auto_theme_sources(ModelRc::new(VecModel::from(vec![
        t("System Colors"),
        t("Wallpaper Sync"),
        t("Custom Image"),
    ])));
    state.set_languages(ModelRc::new(VecModel::from(vec![
        t("Auto"),
        "English".into(),
        "Español".into(),
        "Français".into(),
        "Deutsch".into(),
        "Português".into(),
    ])));
    state.set_immersive_search_actions(ModelRc::new(VecModel::from(vec![
        t("Disabled"),
        t("Replace current queue"),
        t("Play next"),
        t("Add to queue"),
    ])));
    state.set_immersive_default_views(ModelRc::new(VecModel::from(vec![
        t("Remember last"),
        t("Album Reactive"),
        t("Static"),
        t("Coverflow"),
        t("Spectrum"),
        t("Lyrics"),
        t("Queue"),
    ])));
    state.set_nav_tb_positions(ModelRc::new(VecModel::from(vec![
        t("Auto (opposite to window controls)"),
        t("Left"),
        t("Right"),
    ])));
    state.set_wc_positions(ModelRc::new(VecModel::from(vec![t("Left"), t("Right")])));
    state.set_wc_styles(ModelRc::new(VecModel::from(vec![
        t("Rectangular"),
        t("Full-height rounded"),
        t("Circular"),
        t("Square"),
    ])));
    state.set_wc_sizes(ModelRc::new(VecModel::from(vec![
        t("Small"),
        t("Normal"),
        t("Large"),
    ])));
    state.set_wc_color_presets(ModelRc::new(VecModel::from(vec![
        t("Default"),
        "macOS".into(),
        "Adwaita".into(),
        t("Monochrome"),
        t("Custom"),
    ])));
    state.set_miniplayer_views(ModelRc::new(VecModel::from(vec![
        t("Remember last used"),
        t("Micro"),
        t("Compact"),
        t("Artwork"),
        t("Queue"),
        t("Lyrics"),
    ])));
    state.set_startup_pages(ModelRc::new(VecModel::from(vec![
        t("Home"),
        t("Where you left off"),
    ])));
    state.set_tray_icon_themes(ModelRc::new(VecModel::from(vec![
        t("Auto"),
        t("Mono light"),
        t("Mono dark"),
        t("Color"),
    ])));
    state.set_immersive_views(ModelRc::new(VecModel::from(vec![
        t("Remember last used"),
        t("Coverflow"),
        t("Static"),
        t("Vinyl"),
        t("Visualizer"),
        t("Neon Flow"),
        t("Tunnel"),
        t("Comet"),
        t("Lyrics"),
        t("Queue"),
        t("Split: Lyrics"),
        t("Split: Track Info"),
        t("Split: Suggestions"),
        t("Split: Queue"),
    ])));
    state.set_immersive_background_modes(ModelRc::new(VecModel::from(vec![
        t("Full"),
        t("Lite"),
        t("Off"),
    ])));
}

/// Open the folder editor modal for an existing folder, populating
/// `FolderEditState` from the stored record. Shared by the Playlist
/// Manager edit-folder action and the sidebar context menu so both open
/// the same editor. The icon-preset/color grids are populated once at
/// startup, so the editor works from anywhere.
fn open_folder_editor(window: &AppWindow, id: slint::SharedString) {
    let fid = id.to_string();
    if let Some(f) = playlist_manager::folder_for_edit(&fid) {
        let fes = window.global::<FolderEditState>();
        fes.set_id(id);
        fes.set_name(f.name.into());
        fes.set_icon_preset(f.icon_preset.into());
        fes.set_icon_color(f.icon_color.into());
        fes.set_is_hidden(f.is_hidden);
        fes.set_custom_image_path(f.custom_image_path.clone().unwrap_or_default().into());
        fes.set_open(true);
        // Decode the existing custom image, if any.
        if let Some(path) = f.custom_image_path {
            playlist_manager::load_editor_custom_image(window.as_weak(), path);
        }
    }
}

/// Re-fire the artwork pipeline for the Playlist Manager's currently
/// rendered cards (after a rebuild swaps the models).
fn refresh_pm_covers(window: &AppWindow) {
    if let Some(cache) = artwork::shared_cache() {
        let jobs = playlist_manager::artwork_jobs(window);
        if !jobs.is_empty() {
            artwork::spawn_loads(jobs, window.as_weak(), cache);
        }
        let handle = tokio::runtime::Handle::current();
        playlist_manager::load_folder_custom_images(window.as_weak(), &handle);
    }
}

/// Build the folder-editor icon-preset + solid-color models (matches
/// Tauri's FolderEditModal presets). Run once when wiring the editor.
fn folder_editor_presets() -> (Vec<PmIconPreset>, Vec<PmColorSwatch>) {
    // The icon glyphs are resolved in the .slint by id (a `@image-url`
    // chain keyed on `preset.id`), so the model only carries the id; the
    // image field stays default.
    let presets: Vec<PmIconPreset> =
        ["heart", "star", "music", "folder", "disc", "library", "headphones"]
            .iter()
            .map(|id| PmIconPreset {
                id: (*id).into(),
                icon: slint::Image::default(),
            })
            .collect();

    let parse = |hex: &str| -> slint::Color {
        let h = hex.trim_start_matches('#');
        let v = u32::from_str_radix(h, 16).unwrap_or(0);
        slint::Color::from_rgb_u8(
            ((v >> 16) & 0xff) as u8,
            ((v >> 8) & 0xff) as u8,
            (v & 0xff) as u8,
        )
    };
    let mut swatches = vec![PmColorSwatch {
        value: "".into(),
        color: slint::Color::default(),
        is_accent: true,
    }];
    for hex in [
        "#ef4444", "#f97316", "#f59e0b", "#10b981", "#06b6d4", "#3b82f6", "#a855f7", "#ec4899",
        "#f43f5e", "#64748b",
    ] {
        swatches.push(PmColorSwatch {
            value: hex.into(),
            color: parse(hex),
            is_accent: false,
        });
    }
    (presets, swatches)
}

/// Wire all Playlist Manager + folder-editor callbacks. Mirrors the
/// favorites + sidebar wiring: optimistic local mutations (rebuild from
/// cache) plus a backend write on a blocking thread.
fn wire_playlist_manager(
    window: &AppWindow,
    app_runtime: &Arc<AppRuntime<SlintAdapter>>,
    tokio_rt: &tokio::runtime::Runtime,
    image_cache: &artwork::ImageCache,
) {
    // The folder-editor preset + color grids (built once, never change).
    {
        let (presets, swatches) = folder_editor_presets();
        let fes = window.global::<FolderEditState>();
        fes.set_icon_presets(slint::ModelRc::new(slint::VecModel::from(presets)));
        fes.set_color_swatches(slint::ModelRc::new(slint::VecModel::from(swatches)));
    }

    // --- Open playlist ---------------------------------------------------
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<PlaylistManagerActions>()
            .on_open_playlist(move |id| {
                nav::record(nav::NavEntry::Playlist(id.to_string()));
                navigate_playlist(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    id.to_string(),
                );
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
            });
    }

    // --- Toolbar ---------------------------------------------------------
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistManagerActions>()
            .on_search_changed(move |query| {
                if let Some(w) = weak.upgrade() {
                    w.global::<PlaylistManagerState>().set_search_query(query);
                    playlist_manager::rebuild(&w);
                    refresh_pm_covers(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistManagerActions>()
            .on_search_folders(move |query| {
                if let Some(w) = weak.upgrade() {
                    playlist_manager::search_menu_folders(&w, &query);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistManagerActions>()
            .on_set_filter(move |value| {
                if let Some(w) = weak.upgrade() {
                    w.global::<PlaylistManagerState>().set_filter(value);
                    playlist_manager::rebuild(&w);
                    refresh_pm_covers(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistManagerActions>()
            .on_set_sort(move |value| {
                if let Some(w) = weak.upgrade() {
                    w.global::<PlaylistManagerState>().set_sort(value);
                    playlist_manager::rebuild(&w);
                    refresh_pm_covers(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistManagerActions>()
            .on_set_view_mode(move |value| {
                if let Some(w) = weak.upgrade() {
                    w.global::<PlaylistManagerState>().set_view_mode(value);
                    playlist_manager::rebuild(&w);
                    refresh_pm_covers(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistManagerActions>()
            .on_toggle_folder_mode(move || {
                if let Some(w) = weak.upgrade() {
                    let st = w.global::<PlaylistManagerState>();
                    let next = !st.get_folder_mode();
                    st.set_folder_mode(next);
                    // Leaving folder mode while in tree falls back to grid.
                    if !next && st.get_view_mode() == "tree" {
                        st.set_view_mode("grid".into());
                    }
                    playlist_manager::rebuild(&w);
                    refresh_pm_covers(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistManagerActions>()
            .on_toggle_folders_collapsed(move || {
                if let Some(w) = weak.upgrade() {
                    let st = w.global::<PlaylistManagerState>();
                    st.set_folders_collapsed(!st.get_folders_collapsed());
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistManagerActions>()
            .on_toggle_tree_folder(move |id| {
                if let Some(w) = weak.upgrade() {
                    playlist_manager::toggle_tree_folder(&w, id.as_str());
                    refresh_pm_covers(&w);
                }
            });
    }

    // --- Per-card playlist actions --------------------------------------
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlaylistManagerActions>()
            .on_toggle_favorite(move |id| {
                let Some(w) = weak.upgrade() else { return };
                // LOCAL playlist (B3): the flag lives on its own
                // local_playlists row — the u64 settings table can't hold it.
                if local_playlist::is_local_id(id.as_str()) {
                    let value = playlist_manager::toggle_local_favorite(&w, id.as_str());
                    refresh_pm_covers(&w);
                    let lid = id.to_string();
                    handle.spawn(async move {
                        tokio::task::spawn_blocking(move || {
                            local_playlist::set_favorite_blocking(&lid, value)
                        })
                        .await
                        .ok();
                    });
                    return;
                }
                let Ok(pid) = id.parse::<u64>() else { return };
                let value = playlist_manager::toggle_favorite_local(&w, pid);
                refresh_pm_covers(&w);
                handle.spawn(async move {
                    tokio::task::spawn_blocking(move || folders::set_favorite(pid, value))
                        .await
                        .ok();
                });
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlaylistManagerActions>()
            .on_toggle_hidden(move |id| {
                let Some(w) = weak.upgrade() else { return };
                // LOCAL playlist (B3): the flag lives on its own
                // local_playlists row; hidden locals drop from the sidebar.
                if local_playlist::is_local_id(id.as_str()) {
                    let value = playlist_manager::toggle_local_hidden(&w, id.as_str());
                    refresh_pm_covers(&w);
                    let lid = id.to_string();
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    handle.clone().spawn(async move {
                        tokio::task::spawn_blocking(move || {
                            local_playlist::set_hidden_blocking(&lid, value)
                        })
                        .await
                        .ok();
                        // The sidebar reflects hidden playlists, so refresh it.
                        load_sidebar_playlists(runtime, weak, &handle);
                    });
                    return;
                }
                let Ok(pid) = id.parse::<u64>() else { return };
                let value = playlist_manager::toggle_hidden_local(&w, pid);
                refresh_pm_covers(&w);
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    tokio::task::spawn_blocking(move || folders::set_hidden(pid, value))
                        .await
                        .ok();
                    // The sidebar reflects hidden playlists, so refresh it.
                    load_sidebar_playlists(runtime, weak, &handle);
                });
            });
    }
    {
        // Open the shared edit-playlist modal, prefilled from the card.
        let weak = window.as_weak();
        window
            .global::<PlaylistManagerActions>()
            .on_edit_playlist(move |id| {
                use slint::Model;
                let Some(w) = weak.upgrade() else { return };
                let model = w.global::<PlaylistManagerState>().get_playlists();
                let name = (0..model.row_count())
                    .filter_map(|i| model.row_data(i))
                    .find(|it| it.id == id)
                    .map(|it| it.name)
                    .unwrap_or_default();
                let es = w.global::<EditPlaylistState>();
                es.set_id(id);
                es.set_name(name);
                es.set_description("".into());
                es.set_open(true);
            });
    }
    {
        // Add a whole playlist to a Mixtape/Collection (callsite O). Builds the
        // `playlist` payload from the PM grid row (id / name / track count /
        // first cover); the owner subtitle isn't carried in the PM model, so it
        // is omitted (optional in the contract).
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlaylistManagerActions>()
            .on_add_to_mixtape(move |id| {
                use slint::Model;
                let Some(w) = weak.upgrade() else { return };
                let model = w.global::<PlaylistManagerState>().get_playlists();
                let Some(row) = (0..model.row_count())
                    .filter_map(|i| model.row_data(i))
                    .find(|it| it.id == id)
                else {
                    return;
                };
                let artwork = row.url1.to_string();
                let item = myqbz_add::AddItem {
                    item_type: "playlist".into(),
                    source: "qobuz".into(),
                    source_item_id: id.to_string(),
                    title: row.name.to_string(),
                    subtitle: None,
                    artwork_url: (!artwork.is_empty()).then_some(artwork),
                    year: None,
                    track_count: (row.total_count > 0).then_some(row.total_count),
                };
                open_add_to_mixtape(weak.clone(), handle.clone(), vec![item]);
            });
    }

    // --- Arrow reorder (custom sort) ------------------------------------
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlaylistManagerActions>()
            .on_move_up(move |id| {
                let Some(w) = weak.upgrade() else { return };
                let Ok(pid) = id.parse::<u64>() else { return };
                let order = playlist_manager::move_up(&w, pid);
                refresh_pm_covers(&w);
                if !order.is_empty() {
                    handle.spawn(async move {
                        tokio::task::spawn_blocking(move || folders::reorder_playlists(&order))
                            .await
                            .ok();
                    });
                }
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlaylistManagerActions>()
            .on_move_down(move |id| {
                let Some(w) = weak.upgrade() else { return };
                let Ok(pid) = id.parse::<u64>() else { return };
                let order = playlist_manager::move_down(&w, pid);
                refresh_pm_covers(&w);
                if !order.is_empty() {
                    handle.spawn(async move {
                        tokio::task::spawn_blocking(move || folders::reorder_playlists(&order))
                            .await
                            .ok();
                    });
                }
            });
    }
    {
        // Move a playlist into a folder ("" = root).
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlaylistManagerActions>()
            .on_move_to_folder(move |playlist_id, folder_id| {
                let Some(w) = weak.upgrade() else { return };
                let Ok(pid) = playlist_id.parse::<u64>() else { return };
                let fid = folder_id.to_string();
                playlist_manager::move_to_folder_local(&w, pid, &fid);
                refresh_pm_covers(&w);
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    let opt = fid.clone();
                    tokio::task::spawn_blocking(move || {
                        let o = if opt.is_empty() { None } else { Some(opt.as_str()) };
                        folders::move_playlist(pid, o);
                    })
                    .await
                    .ok();
                    load_sidebar_playlists(runtime, weak, &handle);
                });
            });
    }

    // --- Folder editor: open (new + edit) -------------------------------
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistManagerActions>()
            .on_new_folder(move || {
                if let Some(w) = weak.upgrade() {
                    let fes = w.global::<FolderEditState>();
                    fes.set_id("".into());
                    fes.set_name("".into());
                    fes.set_icon_preset("folder".into());
                    fes.set_icon_color("".into());
                    fes.set_is_hidden(false);
                    fes.set_custom_image_path("".into());
                    fes.set_open(true);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistManagerActions>()
            .on_edit_folder(move |id| {
                let Some(w) = weak.upgrade() else { return };
                open_folder_editor(&w, id);
            });
    }

    // --- Folder editor: field changes -----------------------------------
    {
        let weak = window.as_weak();
        window
            .global::<FolderEditActions>()
            .on_select_preset(move |id| {
                if let Some(w) = weak.upgrade() {
                    let fes = w.global::<FolderEditState>();
                    fes.set_icon_preset(id);
                    // Choosing a preset clears the custom image.
                    fes.set_custom_image_path("".into());
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<FolderEditActions>()
            .on_select_color(move |hex| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FolderEditState>().set_icon_color(hex);
                }
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<FolderEditActions>()
            .on_pick_image(move || {
                let weak = weak.clone();
                handle.spawn(async move {
                    let Some(file) = rfd::AsyncFileDialog::new()
                        .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
                        .pick_file()
                        .await
                    else {
                        return;
                    };
                    let path = file.path().to_string_lossy().to_string();
                    let path2 = path.clone();
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        w.global::<FolderEditState>().set_custom_image_path(path2.into());
                        playlist_manager::load_editor_custom_image(w.as_weak(), path);
                    });
                });
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<FolderEditActions>()
            .on_clear_image(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<FolderEditState>().set_custom_image_path("".into());
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<FolderEditActions>()
            .on_toggle_hidden(move || {
                if let Some(w) = weak.upgrade() {
                    let fes = w.global::<FolderEditState>();
                    fes.set_is_hidden(!fes.get_is_hidden());
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<FolderEditActions>()
            .on_close(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<FolderEditState>().set_open(false);
                }
            });
    }
    {
        // Save (create or update) the folder, then reload PM + sidebar.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<FolderEditActions>()
            .on_save(move || {
                let Some(w) = weak.upgrade() else { return };
                let fes = w.global::<FolderEditState>();
                let id = fes.get_id().to_string();
                let name = fes.get_name().to_string();
                if name.trim().is_empty() {
                    return;
                }
                let preset = fes.get_icon_preset().to_string();
                let color = fes.get_icon_color().to_string();
                let hidden = fes.get_is_hidden();
                let image_path = fes.get_custom_image_path().to_string();
                fes.set_open(false);
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                let image_cache = image_cache.clone();
                handle.clone().spawn(async move {
                    let nm = name.trim().to_string();
                    tokio::task::spawn_blocking(move || {
                        if id.is_empty() {
                            folders::create_folder_full(&nm, &preset, &color);
                            // A custom image on a brand-new folder: set it
                            // in a follow-up update once we have the id.
                            // (Rare path; the create flow defaults to a
                            // preset icon — image edits use the edit path.)
                        } else {
                            let icon_type = if image_path.is_empty() { "preset" } else { "custom" };
                            let img = if image_path.is_empty() {
                                Some(None)
                            } else {
                                Some(Some(image_path.as_str()))
                            };
                            folders::update_folder_full(
                                &id, &nm, icon_type, &preset, &color, img, hidden,
                            );
                        }
                    })
                    .await
                    .ok();
                    // Reload the manager data + sidebar.
                    let data = playlist_manager::load(&runtime).await;
                    let weak2 = weak.clone();
                    let r2 = runtime.clone();
                    let h2 = handle.clone();
                    let ic = image_cache.clone();
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        playlist_manager::apply(&w, data);
                        refresh_pm_covers(&w);
                        load_sidebar_playlists(r2, weak2, &h2);
                        let _ = ic;
                    });
                });
            });
    }
    {
        // Delete the folder (Tauri ask() confirm), then reload.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<FolderEditActions>()
            .on_delete(move || {
                let Some(w) = weak.upgrade() else { return };
                let id = w.global::<FolderEditState>().get_id().to_string();
                let name = w.global::<FolderEditState>().get_name().to_string();
                if id.is_empty() {
                    return;
                }
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    let confirmed = rfd::AsyncMessageDialog::new()
                        .set_title("Delete folder")
                        .set_description(format!(
                            "Delete the folder \u{201c}{name}\u{201d}? Its playlists move back to the root."
                        ))
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show()
                        .await;
                    if confirmed != rfd::MessageDialogResult::Yes {
                        return;
                    }
                    let fid = id.clone();
                    tokio::task::spawn_blocking(move || folders::delete_folder(&fid))
                        .await
                        .ok();
                    let _ = weak.upgrade_in_event_loop(|w| {
                        w.global::<FolderEditState>().set_open(false);
                    });
                    let data = playlist_manager::load(&runtime).await;
                    let weak2 = weak.clone();
                    let r2 = runtime.clone();
                    let h2 = handle.clone();
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        playlist_manager::apply(&w, data);
                        refresh_pm_covers(&w);
                        load_sidebar_playlists(r2, weak2, &h2);
                    });
                });
            });
    }
}

/// Wire the My QBZ (Mixtapes & Collections) index grids. READ-ONLY slice:
/// `open-card` / `create-*` are logging STUBS; the toolbar callbacks
/// (search / sort / view / kind-filter / reset) drive `crate::myqbz` rebuilds
/// + re-issue mosaic artwork jobs. Mirrors `wire_playlist_manager`.
fn wire_myqbz(
    window: &AppWindow,
    app_runtime: &Arc<AppRuntime<SlintAdapter>>,
    tokio_rt: &tokio::runtime::Runtime,
    image_cache: &artwork::ImageCache,
) {
    use myqbz::Grid;

    // Re-issue mosaic artwork jobs for a grid after a toolbar rebuild (the
    // row set / order changed, so visible cards need their covers reloaded).
    fn refresh_covers(window: &AppWindow, grid: Grid, image_cache: &artwork::ImageCache) {
        let jobs = myqbz::artwork_jobs(window, grid);
        artwork::spawn_loads(jobs, window.as_weak(), image_cache.clone());
    }

    // --- Open a card -> the collection-detail view (Phase-2 Slice 3) -----
    // NAV-IN: record history + navigate (loads via myqbz_detail::navigate),
    // mirroring the grid's own nav and the album/playlist detail openers.
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        let runtime = app_runtime.clone();
        window.global::<MyQbzActions>().on_open_card(move |id| {
            nav::record(nav::NavEntry::MixtapeDetail(id.to_string()));
            myqbz_detail::navigate(
                runtime.clone(),
                weak.clone(),
                handle.clone(),
                image_cache.clone(),
                id.to_string(),
            );
        });
    }

    // --- Create CTAs: open the create modal pre-set to the right kind ---
    // The kind is fixed by which grid opened it (Mixtapes -> mixtape;
    // Collections -> collection); the modal radio can flip it. Mirrors
    // Tauri's `openCreateModal(kind)`.
    fn open_create_modal(window: &AppWindow, kind: &str) {
        let st = window.global::<MyQbzCreateState>();
        st.set_kind(kind.into());
        st.set_name("".into());
        st.set_creating(false);
        st.set_open(true);
    }
    {
        let weak = window.as_weak();
        window.global::<MyQbzActions>().on_create_mixtape(move || {
            if let Some(w) = weak.upgrade() {
                open_create_modal(&w, "mixtape");
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<MyQbzActions>().on_create_collection(move || {
            if let Some(w) = weak.upgrade() {
                open_create_modal(&w, "collection");
            }
        });
    }

    // --- Create modal: cancel / submit ----------------------------------
    {
        let weak = window.as_weak();
        window.global::<MyQbzCreateActions>().on_close(move || {
            if let Some(w) = weak.upgrade() {
                w.global::<MyQbzCreateState>().set_open(false);
            }
        });
    }
    {
        // Submit: create the collection on a blocking worker, then close the
        // modal + drop the user straight into the new collection's detail
        // view (mirrors Tauri's `submitCreateModal` → `openMixtapeDetail`).
        // The grid is reloaded from the DB on back-nav, so the prepended row
        // shows up there.
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        let runtime = app_runtime.clone();
        window.global::<MyQbzCreateActions>().on_submit(move || {
            let Some(w) = weak.upgrade() else { return; };
            let st = w.global::<MyQbzCreateState>();
            let name = st.get_name().to_string();
            if name.trim().is_empty() || st.get_creating() {
                return;
            }
            let kind = myqbz::kind_from_str(st.get_kind().as_str());
            st.set_creating(true);

            let weak = weak.clone();
            let handle = handle.clone();
            let image_cache = image_cache.clone();
            let runtime = runtime.clone();
            handle.clone().spawn(async move {
                let nm = name.trim().to_string();
                let created =
                    tokio::task::spawn_blocking(move || myqbz::create_collection(kind, &nm))
                        .await
                        .ok()
                        .flatten();

                let weak2 = weak.clone();
                let handle2 = handle.clone();
                let image_cache2 = image_cache.clone();
                let runtime2 = runtime.clone();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let st = w.global::<MyQbzCreateState>();
                    st.set_creating(false);
                    match created {
                        Some(c) => {
                            st.set_open(false);
                            st.set_name("".into());
                            // Drop into the new collection's detail view.
                            nav::record(nav::NavEntry::MixtapeDetail(c.id.clone()));
                            myqbz_detail::navigate(
                                runtime2.clone(),
                                weak2.clone(),
                                handle2.clone(),
                                image_cache2.clone(),
                                c.id.clone(),
                            );
                        }
                        None => {
                            crate::toast::error(&w, "Failed to create collection");
                        }
                    }
                });
            });
        });
    }

    // --- Add to Mixtape/Collection picker (global singleton) ------------
    {
        // close — clear the pending payload + hide.
        let weak = window.as_weak();
        window.global::<MyQbzAddActions>().on_close(move || {
            if let Some(w) = weak.upgrade() {
                myqbz_add::close(&w);
            }
        });
    }
    {
        // search — re-filter the loaded rows client-side.
        let weak = window.as_weak();
        window
            .global::<MyQbzAddActions>()
            .on_search_changed(move |_query| {
                if let Some(w) = weak.upgrade() {
                    myqbz_add::rebuild(&w);
                }
            });
    }
    {
        // show-create — open the create sub-panel preset to a kind.
        let weak = window.as_weak();
        window
            .global::<MyQbzAddActions>()
            .on_show_create(move |kind| {
                if let Some(w) = weak.upgrade() {
                    let st = w.global::<MyQbzAddState>();
                    st.set_create_kind(kind);
                    st.set_create_name("".into());
                    st.set_creating(true);
                }
            });
    }
    {
        // create-back — return to the picker list.
        let weak = window.as_weak();
        window.global::<MyQbzAddActions>().on_create_back(move || {
            if let Some(w) = weak.upgrade() {
                w.global::<MyQbzAddState>().set_creating(false);
            }
        });
    }
    {
        // pick — add the pending items to the chosen collection.
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<MyQbzAddActions>()
            .on_pick(move |collection_id| {
                let Some(w) = weak.upgrade() else { return };
                let st = w.global::<MyQbzAddState>();
                if st.get_busy_id() != "" {
                    return;
                }
                st.set_busy_id(collection_id.clone());
                // The chosen collection's display name (for the toast).
                let name = myqbz_add_row_name(&w, collection_id.as_str());
                let items = myqbz_add::take_pending();
                let cid = collection_id.to_string();

                let weak = weak.clone();
                handle.spawn(async move {
                    let outcome = tokio::task::spawn_blocking(move || {
                        myqbz_add::add_items(&cid, &items)
                    })
                    .await
                    .unwrap_or(myqbz_add::AddOutcome { added: 0, skipped: 0 });
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        myqbz_add::toast_outcome(&w, &name, &outcome);
                        myqbz_add::close(&w);
                    });
                });
            });
    }
    {
        // create-and-add — create a new collection then add the items.
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<MyQbzAddActions>().on_create_and_add(move || {
            let Some(w) = weak.upgrade() else { return };
            let st = w.global::<MyQbzAddState>();
            let name = st.get_create_name().trim().to_string();
            if name.is_empty() || st.get_create_busy() {
                return;
            }
            let kind = st.get_create_kind().to_string();
            st.set_create_busy(true);
            let items = myqbz_add::take_pending();

            let weak = weak.clone();
            handle.spawn(async move {
                let created = {
                    let kind = kind.clone();
                    let name = name.clone();
                    tokio::task::spawn_blocking(move || {
                        myqbz_add::create_collection(&kind, &name)
                    })
                    .await
                    .ok()
                    .flatten()
                };
                match created {
                    Some((cid, cname)) => {
                        let outcome = tokio::task::spawn_blocking(move || {
                            myqbz_add::add_items(&cid, &items)
                        })
                        .await
                        .unwrap_or(myqbz_add::AddOutcome { added: 0, skipped: 0 });
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            myqbz_add::toast_outcome(&w, &cname, &outcome);
                            myqbz_add::close(&w);
                        });
                    }
                    None => {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            w.global::<MyQbzAddState>().set_create_busy(false);
                            crate::toast::error(&w, "Failed to create");
                        });
                    }
                }
            });
        });
    }

    // --- Mixtapes toolbar -----------------------------------------------
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window
            .global::<MyQbzActions>()
            .on_mix_search_changed(move |query| {
                if let Some(w) = weak.upgrade() {
                    w.global::<MyQbzState>().set_mix_search(query);
                    myqbz::rebuild(&w, Grid::Mixtapes);
                    refresh_covers(&w, Grid::Mixtapes, &image_cache);
                }
            });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window.global::<MyQbzActions>().on_mix_set_sort(move |field| {
            if let Some(w) = weak.upgrade() {
                myqbz::set_sort(&w, Grid::Mixtapes, field.as_str());
                refresh_covers(&w, Grid::Mixtapes, &image_cache);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<MyQbzActions>().on_mix_set_view(move |view| {
            if let Some(w) = weak.upgrade() {
                w.global::<MyQbzState>().set_mix_view(view);
            }
        });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window.global::<MyQbzActions>().on_mix_reset(move || {
            if let Some(w) = weak.upgrade() {
                myqbz::reset(&w, Grid::Mixtapes);
                refresh_covers(&w, Grid::Mixtapes, &image_cache);
            }
        });
    }

    // --- Collections toolbar --------------------------------------------
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window
            .global::<MyQbzActions>()
            .on_col_search_changed(move |query| {
                if let Some(w) = weak.upgrade() {
                    w.global::<MyQbzState>().set_col_search(query);
                    myqbz::rebuild(&w, Grid::Collections);
                    refresh_covers(&w, Grid::Collections, &image_cache);
                }
            });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window.global::<MyQbzActions>().on_col_set_sort(move |field| {
            if let Some(w) = weak.upgrade() {
                myqbz::set_sort(&w, Grid::Collections, field.as_str());
                refresh_covers(&w, Grid::Collections, &image_cache);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<MyQbzActions>().on_col_set_view(move |view| {
            if let Some(w) = weak.upgrade() {
                w.global::<MyQbzState>().set_col_view(view);
            }
        });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window
            .global::<MyQbzActions>()
            .on_col_set_kind_filter(move |kind| {
                if let Some(w) = weak.upgrade() {
                    w.global::<MyQbzState>().set_col_kind_filter(kind);
                    myqbz::rebuild(&w, Grid::Collections);
                    refresh_covers(&w, Grid::Collections, &image_cache);
                }
            });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window.global::<MyQbzActions>().on_col_reset(move || {
            if let Some(w) = weak.upgrade() {
                myqbz::reset(&w, Grid::Collections);
                refresh_covers(&w, Grid::Collections, &image_cache);
            }
        });
    }
}

/// Wire the My QBZ collection-DETAIL view (Phase-2 Slice 3, read-only). The
/// toolbar callbacks (search / sort / type-filter / source-filter / view-mode /
/// select / reset) drive `crate::myqbz_detail` re-derives + re-issue row
/// artwork; `open-item` / `open-artist` route to the existing
/// album/playlist/artist navigators (reusing the top-level open-album /
/// open-artist callbacks so local-vs-qobuz routing + history stay in one
/// place). Every hero CTA + per-row context action is a logging STUB — the
/// read-only boundary for this slice.
fn wire_myqbz_detail(
    window: &AppWindow,
    app_runtime: &Arc<AppRuntime<SlintAdapter>>,
    tokio_rt: &tokio::runtime::Runtime,
    image_cache: &artwork::ImageCache,
) {
    use MyQbzDetailActions as Act;

    // Stash the runtime for the mutation-reload paths (cover/edit) that re-run
    // `myqbz_detail::navigate` (whose resolveItems pass needs it) without
    // threading it through every entry point.
    myqbz_detail::set_runtime(app_runtime.clone());

    // After a toolbar re-derive the rendered model changed, so the visible
    // rows need their thumbnails reloaded — through the SOURCE-SPLIT dispatch
    // (Qobuz CDN urls via HTTP; local/Plex paths via the source-aware decoder).
    fn refresh_row_covers(window: &AppWindow, image_cache: &artwork::ImageCache) {
        let split = myqbz_detail::artwork_jobs(window);
        myqbz_detail::dispatch_artwork(split, window.as_weak(), image_cache.clone());
    }

    // A toolbar re-derive rebuilds the rendered model with fresh rows
    // (tracks_loaded reset to false). While in expanded view-mode the new
    // visible rows must (re-)fetch their inline tracks (spec §8 auto-fetch).
    fn ensure_expanded_if_active(
        window: &AppWindow,
        runtime: &Arc<AppRuntime<SlintAdapter>>,
        handle: &tokio::runtime::Handle,
    ) {
        if window.global::<MyQbzDetailState>().get_view_mode() == "expanded" {
            myqbz_detail::ensure_expanded(runtime.clone(), window.as_weak(), handle.clone());
        }
    }

    // --- Toolbar (client-side re-derive) --------------------------------
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        let runtime = app_runtime.clone();
        let handle = tokio_rt.handle().clone();
        window.global::<Act>().on_search_changed(move |q| {
            if let Some(w) = weak.upgrade() {
                myqbz_detail::search(&w, q.as_str());
                refresh_row_covers(&w, &image_cache);
                ensure_expanded_if_active(&w, &runtime, &handle);
            }
        });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        let runtime = app_runtime.clone();
        let handle = tokio_rt.handle().clone();
        window.global::<Act>().on_set_sort(move |field| {
            if let Some(w) = weak.upgrade() {
                myqbz_detail::set_sort(&w, field.as_str());
                refresh_row_covers(&w, &image_cache);
                ensure_expanded_if_active(&w, &runtime, &handle);
            }
        });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        let runtime = app_runtime.clone();
        let handle = tokio_rt.handle().clone();
        window.global::<Act>().on_set_type_filter(move |value| {
            if let Some(w) = weak.upgrade() {
                myqbz_detail::set_type_filter(&w, value.as_str());
                refresh_row_covers(&w, &image_cache);
                ensure_expanded_if_active(&w, &runtime, &handle);
            }
        });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        let runtime = app_runtime.clone();
        let handle = tokio_rt.handle().clone();
        window.global::<Act>().on_toggle_source_filter(move |kind| {
            if let Some(w) = weak.upgrade() {
                myqbz_detail::toggle_source_filter(&w, kind.as_str());
                refresh_row_covers(&w, &image_cache);
                ensure_expanded_if_active(&w, &runtime, &handle);
            }
        });
    }
    {
        let weak = window.as_weak();
        let runtime = app_runtime.clone();
        let handle = tokio_rt.handle().clone();
        window.global::<Act>().on_set_view_mode(move |mode| {
            if let Some(w) = weak.upgrade() {
                // Sets view-mode + persists the per-collection prefs (spec §18).
                myqbz_detail::set_view_mode(&w, mode.as_str());
                // Entering expanded mode: fetch every expandable item's tracks
                // (spec §8 — tracks render directly under each row).
                if mode == "expanded" {
                    myqbz_detail::ensure_expanded(runtime.clone(), weak.clone(), handle.clone());
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<Act>().on_toggle_select_mode(move || {
            if let Some(w) = weak.upgrade() {
                myqbz_detail::toggle_select_mode(&w);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<Act>().on_toggle_item_select(move |position| {
            if let Some(w) = weak.upgrade() {
                myqbz_detail::toggle_item_select(&w, position);
            }
        });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        let runtime = app_runtime.clone();
        let handle = tokio_rt.handle().clone();
        window.global::<Act>().on_reset_filters(move || {
            if let Some(w) = weak.upgrade() {
                myqbz_detail::reset_filters(&w);
                refresh_row_covers(&w, &image_cache);
                ensure_expanded_if_active(&w, &runtime, &handle);
            }
        });
    }

    // --- Open an item -> album / local-album / playlist -----------------
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<Act>()
            .on_open_item(move |_source, item_type, source_item_id| {
                let Some(w) = weak.upgrade() else { return };
                let id = source_item_id.to_string();
                match item_type.as_str() {
                    // Album / track items both open an album view; the top-level
                    // open-album callback handles Qobuz-vs-local routing + history.
                    "album" | "track" => {
                        w.invoke_open_album(id.into());
                    }
                    "playlist" => {
                        nav::record(nav::NavEntry::Playlist(id.clone()));
                        navigate_playlist(
                            runtime.clone(),
                            weak.clone(),
                            &handle,
                            image_cache.clone(),
                            id,
                        );
                        update_nav_flags(&w);
                    }
                    other => {
                        log::warn!("[qbz-slint] myqbz_detail open-item: unknown type {other}");
                    }
                }
            });
    }

    // --- Open an item's artist (route by SOURCE) -------------------------
    {
        let weak = window.as_weak();
        window
            .global::<Act>()
            .on_open_artist(move |source, artist_name, artist_id| {
                let Some(w) = weak.upgrade() else { return };
                // The top-level open-artist callback routes a numeric id to
                // the Qobuz artist page (with nav history — the same path
                // AlbumView's artist button uses) and a name to the
                // LocalLibrary Artists tab. Stored items only carry the
                // artist NAME, so Qobuz rows route by the numeric artist id
                // the resolveItems pass derived from their first track.
                if source == "qobuz" {
                    if !artist_id.trim().is_empty() {
                        w.invoke_open_artist(artist_id);
                    } else {
                        // Resolve still pending (or failed) — do NOT fall
                        // back to the name: that opens the WRONG page (the
                        // LocalLibrary artist) for a Qobuz item.
                        log::warn!(
                            "[qbz-slint] myqbz_detail open-artist: qobuz item '{artist_name}' \
                             has no resolved artist id yet — ignoring click"
                        );
                    }
                } else if !artist_name.trim().is_empty() {
                    // local / plex -> the LocalLibrary Artists tab by NAME.
                    w.invoke_open_artist(artist_name);
                }
            });
    }

    // --- Hero PLAY / SHUFFLE (Slice 5: detail playback) -----------------
    // Resolve the collection's items through the qbz-mixtape ENQUEUE resolver
    // and drive the queue (replace + auto-play). DJ-mix / edit / delete / sync
    // stay logging stubs (later slices).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<Act>().on_play_all(move || {
            let Some(w) = weak.upgrade() else { return };
            let id = w.global::<MyQbzDetailState>().get_id().to_string();
            if id.is_empty() {
                return;
            }
            myqbz_play::play_all(runtime.clone(), weak.clone(), handle.clone(), id);
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<Act>().on_shuffle(move || {
            let Some(w) = weak.upgrade() else { return };
            let id = w.global::<MyQbzDetailState>().get_id().to_string();
            if id.is_empty() {
                return;
            }
            myqbz_play::shuffle(
                runtime.clone(),
                weak.clone(),
                handle.clone(),
                image_cache.clone(),
                id,
            );
        });
    }

    // --- Hero DJ-mix CTA — open the "Random queue" sampler modal --------
    // Resolves the collection in-order + counts unique tracks (the slider max),
    // then the modal samples + replace-plays on confirm (myqbz_mix).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<Act>().on_dj_mix(move || {
            let Some(w) = weak.upgrade() else { return };
            let id = w.global::<MyQbzDetailState>().get_id().to_string();
            if id.is_empty() {
                return;
            }
            myqbz_mix::open(runtime.clone(), weak.clone(), handle.clone(), id);
        });
    }

    // --- STILL-STUBBED hero CTA: discography sync -----------------------
    // Sync: artist_discography has NO sync impl (spec §8) — no-op stub (the
    // hero button is shown only for artist_collection for Tauri parity).
    {
        let weak = window.as_weak();
        window.global::<Act>().on_sync(move || {
            let id = weak
                .upgrade()
                .map(|w| w.global::<MyQbzDetailState>().get_id().to_string())
                .unwrap_or_default();
            log::info!("[qbz-slint] myqbz_detail sync({id}) — no discography sync impl (spec §8)");
        });
    }

    // --- DJ-mix modal actions (slider / cancel / confirm) ---------------
    {
        let weak = window.as_weak();
        window.global::<MyQbzMixActions>().on_close(move || {
            if let Some(w) = weak.upgrade() {
                myqbz_mix::close(&w);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<MyQbzMixActions>().on_set_index(move |index| {
            if let Some(w) = weak.upgrade() {
                myqbz_mix::apply_index(&w, index);
            }
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<MyQbzMixActions>().on_shuffle(move || {
            let Some(w) = weak.upgrade() else { return };
            let ms = w.global::<MyQbzMixState>();
            let id = w.global::<MyQbzDetailState>().get_id().to_string();
            let size = ms.get_selected_size();
            if id.is_empty() || size <= 0 {
                return;
            }
            myqbz_mix::shuffle(runtime.clone(), weak.clone(), handle.clone(), id, size);
        });
    }

    // --- Bulk action bar (select-mode, spec 12 §13.1) ------------------
    // The full §13.1 group set:
    //  - "add-to-queue" / "play-next": resolve the selected items via the shared
    //    enqueue resolver + append / insert-next (no replace, no queue-source
    //    stamp — mirrors the per-row contract).
    //  - "add-to-playlist": resolve the selected items to their Qobuz track ids
    //    and open the existing playlist picker (Qobuz mode) with them.
    //  - "add-to-mixtape": open the global AddToMixtapeModal with the payloads.
    //  - "remove-selected": remove each selected position (highest-first) then
    //    reload the detail + clear selection.
    //  - "clear": clear the selection (exit-select / uncheck all).
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        let runtime = app_runtime.clone();
        window.global::<Act>().on_bulk_action(move |id| {
            let Some(w) = weak.upgrade() else { return };
            match id.as_str() {
                "add-to-queue" | "play-next" => {
                    let selected = myqbz_detail::selected_full_items(&w);
                    if selected.is_empty() {
                        return;
                    }
                    myqbz_play::bulk_enqueue(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        selected,
                        id.as_str() == "play-next",
                    );
                }
                "add-to-playlist" => {
                    let selected = myqbz_detail::selected_full_items(&w);
                    if selected.is_empty() {
                        return;
                    }
                    // Resolve to Qobuz track ids on a worker, then open the
                    // global picker (Qobuz mode) + load the user's playlists.
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    handle.spawn(async move {
                        let ids =
                            myqbz_play::resolve_bulk_qobuz_track_ids(&runtime, &selected).await;
                        if ids.is_empty() {
                            crate::toast::error_weak(
                                &weak,
                                "No Qobuz tracks in the selection to add to a playlist",
                            );
                            return;
                        }
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            playlist_picker::open_multi(&w, &ids, false);
                        });
                        let playlists = playlist_picker::load(&runtime).await;
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            playlist_picker::apply(&w, playlists);
                        });
                    });
                }
                "add-to-mixtape" => {
                    let selected = myqbz_detail::selected_full_items(&w);
                    let items: Vec<myqbz_add::AddItem> = selected
                        .iter()
                        .map(|it| myqbz_add::AddItem {
                            item_type: myqbz_detail::item_type_str(it.item_type).to_string(),
                            source: myqbz_detail::source_str(it.source).to_string(),
                            source_item_id: it.source_item_id.clone(),
                            title: it.title.clone(),
                            subtitle: it.subtitle.clone(),
                            artwork_url: it.artwork_url.clone(),
                            year: it.year,
                            track_count: it.track_count,
                        })
                        .collect();
                    open_add_to_mixtape(weak.clone(), handle.clone(), items);
                }
                "remove-selected" => {
                    let cid = w.global::<MyQbzDetailState>().get_id().to_string();
                    let positions = myqbz_detail::selected_positions(&w);
                    myqbz_edit::remove_selected(
                        weak.clone(),
                        handle.clone(),
                        image_cache.clone(),
                        cid,
                        positions,
                    );
                }
                "clear" => {
                    // Clear-X: uncheck every row + zero the count, staying in
                    // select-mode (spec §13.1 clear control).
                    myqbz_detail::clear_selection(&w);
                }
                other => {
                    log::warn!("[qbz-slint] myqbz_detail bulk-action: unknown id {other}");
                }
            }
        });
    }

    // --- Hero overflow (⋯) menu — open the edit modals (spec 12 §10/§11) ---
    // Rename / Edit description / Delete-confirm open the shared MyQbzEditModal
    // with the right mode + prefill; the mutations + reload run on submit.
    {
        let weak = window.as_weak();
        window.global::<Act>().on_open_rename(move || {
            let Some(w) = weak.upgrade() else { return };
            let ds = w.global::<MyQbzDetailState>();
            let es = w.global::<MyQbzEditState>();
            es.set_mode("rename".into());
            es.set_name(ds.get_name());
            es.set_draft_name(ds.get_name());
            es.set_busy(false);
            es.set_open(true);
        });
    }
    {
        let weak = window.as_weak();
        window.global::<Act>().on_open_description(move || {
            let Some(w) = weak.upgrade() else { return };
            let ds = w.global::<MyQbzDetailState>();
            let es = w.global::<MyQbzEditState>();
            es.set_mode("description".into());
            es.set_name(ds.get_name());
            es.set_draft_description(ds.get_description());
            es.set_busy(false);
            es.set_open(true);
        });
    }
    {
        let weak = window.as_weak();
        window.global::<Act>().on_open_delete(move || {
            let Some(w) = weak.upgrade() else { return };
            let ds = w.global::<MyQbzDetailState>();
            let es = w.global::<MyQbzEditState>();
            es.set_mode("delete".into());
            es.set_name(ds.get_name());
            es.set_busy(false);
            es.set_open(true);
        });
    }

    // --- Hero overflow — custom cover (set / remove) --------------------
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<Act>().on_upload_cover(move || {
            let Some(w) = weak.upgrade() else { return };
            let id = w.global::<MyQbzDetailState>().get_id().to_string();
            if id.is_empty() {
                return;
            }
            myqbz_cover::upload(weak.clone(), handle.clone(), image_cache.clone(), id);
        });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<Act>().on_remove_cover(move || {
            let Some(w) = weak.upgrade() else { return };
            let id = w.global::<MyQbzDetailState>().get_id().to_string();
            if id.is_empty() {
                return;
            }
            myqbz_cover::remove(weak.clone(), handle.clone(), image_cache.clone(), id);
        });
    }

    // --- Hero overflow — play-mode toggle / convert kind ---------------
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<Act>().on_toggle_play_mode(move || {
            let Some(w) = weak.upgrade() else { return };
            let ds = w.global::<MyQbzDetailState>();
            let id = ds.get_id().to_string();
            let mode = ds.get_play_mode().to_string();
            if id.is_empty() {
                return;
            }
            myqbz_edit::toggle_play_mode(weak.clone(), handle.clone(), image_cache.clone(), id, mode);
        });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<Act>().on_convert_kind(move || {
            let Some(w) = weak.upgrade() else { return };
            let ds = w.global::<MyQbzDetailState>();
            let id = ds.get_id().to_string();
            let kind = ds.get_kind().to_string();
            if id.is_empty() {
                return;
            }
            myqbz_edit::convert_kind(weak.clone(), handle.clone(), image_cache.clone(), id, kind);
        });
    }

    // --- Edit modals — submit (rename / description / delete) ----------
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<MyQbzEditActions>().on_submit_rename(move || {
            let Some(w) = weak.upgrade() else { return };
            let id = w.global::<MyQbzDetailState>().get_id().to_string();
            let name = w.global::<MyQbzEditState>().get_draft_name().to_string();
            myqbz_edit::rename(weak.clone(), handle.clone(), image_cache.clone(), id, name);
        });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<MyQbzEditActions>()
            .on_submit_description(move || {
                let Some(w) = weak.upgrade() else { return };
                let id = w.global::<MyQbzDetailState>().get_id().to_string();
                let desc = w.global::<MyQbzEditState>().get_draft_description().to_string();
                myqbz_edit::set_description(
                    weak.clone(),
                    handle.clone(),
                    image_cache.clone(),
                    id,
                    desc,
                );
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<MyQbzEditActions>().on_confirm_delete(move || {
            let Some(w) = weak.upgrade() else { return };
            let id = w.global::<MyQbzDetailState>().get_id().to_string();
            myqbz_edit::delete(weak.clone(), handle.clone(), id);
        });
    }
    {
        let weak = window.as_weak();
        window.global::<MyQbzEditActions>().on_close(move || {
            if let Some(w) = weak.upgrade() {
                let es = w.global::<MyQbzEditState>();
                es.set_open(false);
                es.set_mode("".into());
                es.set_busy(false);
            }
        });
    }

    // --- Per-row PLAY (default) -----------------------------------------
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<Act>().on_play_item(move |source_item_id| {
            let Some(w) = weak.upgrade() else { return };
            let id = w.global::<MyQbzDetailState>().get_id().to_string();
            if id.is_empty() {
                return;
            }
            myqbz_play::play_item(
                runtime.clone(),
                weak.clone(),
                handle.clone(),
                id,
                source_item_id.to_string(),
            );
        });
    }

    // --- Per-row context menu (play / play-next / add-to-queue) ---------
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<Act>()
            .on_item_action(move |source_item_id, action| {
                let Some(w) = weak.upgrade() else { return };
                let id = w.global::<MyQbzDetailState>().get_id().to_string();
                if id.is_empty() {
                    return;
                }
                myqbz_play::item_action(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id,
                    source_item_id.to_string(),
                    action.to_string(),
                );
            });
    }

    // --- Per-row REMOVE (single item) -----------------------------------
    // Routes ONE position through the audited bulk remover (remove-highest-
    // first compaction + clear-selection + toast + reload) with a 1-element
    // vec, so single-row remove reuses the exact same code path as the bulk
    // "remove-selected" action — no duplicated removal logic.
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<Act>().on_remove_item(move |position| {
            let Some(w) = weak.upgrade() else { return };
            let id = w.global::<MyQbzDetailState>().get_id().to_string();
            if id.is_empty() {
                return;
            }
            myqbz_edit::remove_selected(
                weak.clone(),
                handle.clone(),
                image_cache.clone(),
                id,
                vec![position],
            );
        });
    }

    // --- Expanded view-mode: inline tracks under every album/playlist (§8) -
    // Fired when the expanded view-mode becomes active; fetches each
    // expandable item's tracks (skipping already-cached rows).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<Act>().on_ensure_expanded(move || {
            myqbz_detail::ensure_expanded(runtime.clone(), weak.clone(), handle.clone());
        });
    }
    // Inline-track row actions (play / play-next / play-later / go-to-album).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<Act>()
            .on_inline_track_action(move |item_source_item_id, track_id, action| {
                let Some(w) = weak.upgrade() else { return };
                // go-to-album routes through the existing open-item path (Qobuz
                // album view vs local-album by id), keeping nav in one place.
                // It must open the PARENT item (spec 12 §8) — so route with the
                // parent's REAL item_type (album/playlist), not a hardcoded
                // "album": a playlist parent must reach the playlist view, not
                // be mis-routed to the album view. The parent's type is read off
                // the rendered row carrying this source-item-id.
                if action == "go-to-album" {
                    let parent_type = {
                        let model = w.global::<MyQbzDetailState>().get_items();
                        (0..model.row_count())
                            .filter_map(|i| model.row_data(i))
                            .find(|it| it.source_item_id == item_source_item_id)
                            .map(|it| it.item_type.to_string())
                            .unwrap_or_else(|| "album".to_string())
                    };
                    w.global::<Act>().invoke_open_item(
                        "".into(),
                        parent_type.into(),
                        item_source_item_id,
                    );
                    return;
                }
                let id = w.global::<MyQbzDetailState>().get_id().to_string();
                if id.is_empty() {
                    return;
                }
                myqbz_play::play_inline_track(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id,
                    item_source_item_id.to_string(),
                    track_id.to_string(),
                    action.to_string(),
                );
            });
    }
}

/// Wire the Discography Builder (spec 13). Back -> browser-back (returns to the
/// artist page). Name / order / checkbox / select-all / type-override drive the
/// `crate::myqbz_builder` session re-renders. Open-album routes through the
/// top-level `open-album` (Qobuz album view vs local-album by id). Create runs
/// the save flow on a blocking worker then navigates to the new collection.
fn wire_disco_builder(
    window: &AppWindow,
    tokio_rt: &tokio::runtime::Runtime,
    image_cache: &artwork::ImageCache,
) {
    use DiscoBuilderActions as Act;

    // Back / Cancel -> browser-back to the artist page.
    {
        let weak = window.as_weak();
        window.global::<Act>().on_back(move || {
            if let Some(w) = weak.upgrade() {
                w.global::<NavState>().invoke_request_back();
            }
        });
    }
    // Collection-name input.
    {
        let weak = window.as_weak();
        window.global::<Act>().on_name_changed(move |name| {
            if let Some(w) = weak.upgrade() {
                myqbz_builder::name_changed(&w, name.as_str());
            }
        });
    }
    // Order segmented control.
    {
        let weak = window.as_weak();
        window.global::<Act>().on_set_order(move |order| {
            if let Some(w) = weak.upgrade() {
                myqbz_builder::set_order(&w, order.as_str());
            }
        });
    }
    // Per-row checkbox.
    {
        let weak = window.as_weak();
        window
            .global::<Act>()
            .on_toggle_checked(move |group_key, cand_key| {
                if let Some(w) = weak.upgrade() {
                    myqbz_builder::toggle_checked(&w, group_key.as_str(), cand_key.as_str());
                }
            });
    }
    // Header select-all.
    {
        let weak = window.as_weak();
        window.global::<Act>().on_toggle_all(move || {
            if let Some(w) = weak.upgrade() {
                myqbz_builder::toggle_all(&w);
            }
        });
    }
    // Open an album (Qobuz album view, or local-album by group key).
    {
        let weak = window.as_weak();
        window
            .global::<Act>()
            .on_open_album(move |_source, source_item_id| {
                if let Some(w) = weak.upgrade() {
                    if !source_item_id.trim().is_empty() {
                        w.invoke_open_album(source_item_id);
                    }
                }
            });
    }
    // Release-type override set / reset.
    {
        let weak = window.as_weak();
        window
            .global::<Act>()
            .on_set_type_override(move |source, id, choice| {
                if let Some(w) = weak.upgrade() {
                    myqbz_builder::set_type_override(
                        &w,
                        source.as_str(),
                        id.as_str(),
                        choice.as_str(),
                    );
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<Act>()
            .on_reset_type_override(move |source, id| {
                if let Some(w) = weak.upgrade() {
                    myqbz_builder::reset_type_override(&w, source.as_str(), id.as_str());
                }
            });
    }
    // Create — save the artist_collection + bulk-add, then navigate to detail.
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<Act>().on_create(move || {
            let Some(w) = weak.upgrade() else { return };
            // Snapshot the selection in current sort order (UI thread).
            let Some(payload) = myqbz_builder::save_payload(&w) else {
                return;
            };
            if w.global::<DiscoBuilderState>().get_creating() {
                return;
            }
            myqbz_builder::set_creating(&w, true);

            let weak = weak.clone();
            let handle = handle.clone();
            let image_cache = image_cache.clone();
            handle.clone().spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    myqbz_builder::create_collection(&payload)
                })
                .await
                .ok()
                .flatten();

                let _ = weak.upgrade_in_event_loop(move |w| {
                    myqbz_builder::set_creating(&w, false);
                    match result {
                        Some(collection_id) => {
                            myqbz_builder::toast_created(&w);
                            // Navigate to the new collection's detail.
                            nav::record(nav::NavEntry::MixtapeDetail(collection_id.clone()));
                            if let Some(runtime) = myqbz_detail::global_runtime() {
                                myqbz_detail::navigate(
                                    runtime,
                                    w.as_weak(),
                                    handle.clone(),
                                    image_cache.clone(),
                                    collection_id,
                                );
                            }
                            update_nav_flags(&w);
                        }
                        None => {
                            myqbz_builder::toast_failed(&w);
                        }
                    }
                });
            });
        });
    }
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

    // Install the rustls process-level CryptoProvider ONCE, before ANY TLS use.
    // The full binary compiles BOTH rustls providers (aws-lc-rs via qbz-cast's
    // rust_cast, ring via reqwest), so rustls cannot auto-select one and panics
    // ("Could not automatically determine the process-level CryptoProvider") the
    // first time a TLS connection opens. Previously this was only installed lazily
    // on the cast paths, so QConnect's tokio-tungstenite WebSocket (it opens a
    // rustls TLS channel on its own worker thread) panicked when the user joined a
    // session WITHOUT ever casting. Installing here at startup covers every TLS
    // consumer — QConnect, cast, reqwest — regardless of which connects first.
    // Idempotent (the cast paths still call it; the second call is a no-op).
    qbz_cast::ensure_crypto_provider();

    let tokio_rt = tokio::runtime::Runtime::new()?;
    let _enter = tokio_rt.enter();

    // WGPU UNDERLAY SPIKE: select a wgpu-capable backend BEFORE the first window
    // is created. require_wgpu_28 forces the winit backend (the only one that
    // honours a graphics-API request) — which the app already uses, so the tray's
    // WinitWindowAccessor stays valid. The renderer is femtovg-wgpu (Cargo.toml),
    // so the femtovg text pipeline is preserved. Automatic config lets Slint init
    // wgpu (downlevel-webgl2 limits — fine for fragment-only shaders). If this
    // ever fails on the owner's GPU/driver, that failure IS the spike result.
    slint::BackendSelector::new()
        .require_wgpu_28(slint::wgpu_28::WGPUConfiguration::default())
        // Make ONLY the miniplayer window borderless at CREATION (the flag is
        // true solely while MiniPlayerWindow::new() runs). Decorations cannot be
        // reliably removed post-creation on Wayland/KDE (server-side decorations
        // are negotiated when the surface is created), so the AppWindow keeps its
        // system titlebar while the mini never has one. Transparency is already
        // default-on in this backend, so the mini's rounded card shows through.
        .with_winit_window_attributes_hook(|attributes| {
            let creating_mini = crate::miniplayer::is_creating_mini();
            log::info!("[mini] window-attributes hook: creating_mini={creating_mini}");
            if creating_mini {
                attributes.with_decorations(false)
            } else {
                attributes
            }
        })
        .select()?;

    // UI language: resolve the persisted language BEFORE the first window is
    // created, and set the Rust-side language now so `t()`/date helpers are
    // correct from the first call. The persisted key may be "auto" (follow the
    // OS locale) — resolve that to a concrete language. set_language() drives
    // our Rust-side `t`/dates helpers.
    //
    // NOTE: select_bundled_translation() operates on the component's GLOBAL
    // translation context, which only exists AFTER AppWindow::new() — calling
    // it before that is a no-op (returns NoTranslationsBundled) and leaves the
    // first paint in English. So we compute `lang` here, set the Rust language,
    // and defer the Slint translation switch + label reseed to just after
    // AppWindow::new() below.
    let lang = {
        let persisted = crate::ui_prefs::load().language;
        let lang = if persisted == "auto" {
            qbz_i18n::resolve_auto()
        } else {
            qbz_i18n::set_language(&persisted);
            qbz_i18n::current_language()
        };
        qbz_i18n::set_language(lang);
        lang
    };

    let window = AppWindow::new()?;
    // Now that the AppWindow (and its translation global context) exists, switch
    // the Slint bundled translations to `lang` and reseed the non-reactive
    // option arrays so the first paint is fully in the persisted language.
    if let Err(e) = slint::select_bundled_translation(lang) {
        log::warn!("[qbz-slint] select_bundled_translation('{lang}') failed: {e:?}");
    }
    reseed_i18n_labels(&window);
    install_browser_mouse_nav(&window);
    wire_window_controls(&window);
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

    // Now-playing bar layout (New = 0 / Classic = 1 / Small = 2 / Large = 3) —
    // restore the persisted choice before the shell renders so the bar opens in
    // the right mode. The sidebar defaults open (state 0), so a persisted "large"
    // restores a valid Large; if some future startup leaves the sidebar closed,
    // `large-active` stays false and the dock simply doesn't mount.
    let restored_prefs = crate::ui_prefs::load();
    let restored_npb = crate::ui_prefs::npb_mode_index(&restored_prefs.npb_mode);
    {
        let shell = window.global::<ShellState>();
        shell.set_npb_mode(restored_npb);
        // Restore the persisted sidebar state (0 open / 1 mini / 2 closed) +
        // section-nav placement before the shell renders.
        shell.set_sidebar_state(restored_prefs.sidebar_state);
        shell.set_nav_in_sidebar(restored_prefs.nav_in_sidebar);
        // Large dock toggles — restore the persisted visualizer state + spectrum
        // choice (default ON / Bars) before the shell renders.
        shell.set_large_visualizer_on(restored_prefs.large_visualizer);
        shell.set_large_spectrum_mode(crate::ui_prefs::large_spectrum_mode_index(
            &restored_prefs.large_spectrum_mode,
        ));
    }
    // Main window geometry: restore the persisted LOGICAL size + (best-effort)
    // position, each clamped to the current monitor so a smaller / disconnected
    // display never opens an oversized or stranded window. 0 size = never saved
    // → keep the `.slint` preferred size. The monitor query is best-effort
    // (`with_winit_window` returns None before the surface exists → the WM's own
    // clamping is the fallback, and the Resized handler re-saves the result).
    if restored_prefs.window_width >= 940.0 && restored_prefs.window_height >= 600.0 {
        let mut w = restored_prefs.window_width;
        let mut h = restored_prefs.window_height;
        window.window().with_winit_window(|win| {
            if let Some(mon) = win.current_monitor() {
                let scale = win.scale_factor().max(0.01);
                let avail_w = (mon.size().width as f64 / scale) as f32;
                let avail_h = (mon.size().height as f64 / scale) as f32;
                if avail_w >= 940.0 {
                    w = w.min(avail_w);
                }
                if avail_h >= 600.0 {
                    h = h.min(avail_h);
                }
            }
        });
        window.window().set_size(slint::LogicalSize::new(w, h));
    }
    if restored_prefs.window_x != i32::MIN && restored_prefs.window_y != i32::MIN {
        let mut px = restored_prefs.window_x;
        let mut py = restored_prefs.window_y;
        window.window().with_winit_window(|win| {
            if let Some(mon) = win.current_monitor() {
                let m = mon.size();
                let mp = mon.position();
                // Keep the top-left inside the monitor rect, leaving ~100px so a
                // sliver stays grabbable even if the saved spot is near an edge.
                let max_x = (mp.x + m.width as i32 - 100).max(mp.x);
                let max_y = (mp.y + m.height as i32 - 100).max(mp.y);
                px = px.clamp(mp.x, max_x);
                py = py.clamp(mp.y, max_y);
            }
        });
        window
            .window()
            .set_position(slint::PhysicalPosition::new(px, py));
    }
    // NOTE: the FFT tap is primed AFTER visualizer::install() (further below) — not
    // here — because install() registers the `on_set_enabled` handler this call
    // depends on.
    window
        .global::<AppearanceState>()
        .set_album_header_gradient(crate::ui_prefs::load().album_header_gradient);
    window
        .global::<AppearanceState>()
        .set_intelligent_search(crate::ui_prefs::load().intelligent_search);
    // Purchases opt-in nav visibility + title-bar placement (both default OFF).
    // Seeded here from the persisted prefs so the sidebar / header entries
    // reflect the user's choice on startup (the Slint equivalent of Tauri's
    // rehydrate-after-login: ui_prefs is read once the session/window is up).
    {
        let prefs = crate::ui_prefs::load();
        let appearance = window.global::<AppearanceState>();
        appearance.set_show_purchases(prefs.show_purchases);
        appearance.set_nav_tb_purchases(prefs.nav_tb_purchases);
    }
    // Purchases per-machine filter prefs + region-notice state. Seeded here so
    // filter selections survive restart and the region notice does not reappear
    // every launch (mirrors the show-purchases seed above; the Slint equivalent
    // of Tauri's per-user persisted purchase filters). The region notice is
    // shown UNTIL dismissed, so its visibility is the inverse of the "seen" flag.
    {
        let prefs = crate::ui_prefs::load();
        let purchases = window.global::<PurchasesState>();
        purchases.set_filter_hide_unavailable(prefs.purchases_hide_unavailable);
        purchases.set_filter_hide_downloaded(prefs.purchases_hide_downloaded);
        purchases.set_filter_quality(prefs.purchases_quality_filter.clone().into());
        purchases.set_show_region_notice(!prefs.purchases_region_notice_seen);
    }
    window.global::<AppearanceState>().set_immersive_search_action_index(
        crate::ui_prefs::immersive_search_action_index(
            &crate::ui_prefs::load().immersive_search_action,
        ),
    );
    window.global::<AppearanceState>().set_immersive_default_view_index(
        crate::ui_prefs::immersive_default_view_index(
            &crate::ui_prefs::load().immersive_default_view,
        ),
    );
    // System Notifications toggle: seed the UI + the poll-thread atomic gate.
    {
        let sys_notif = crate::ui_prefs::load().system_notifications;
        window
            .global::<AppearanceState>()
            .set_system_notifications(sys_notif);
        playback::NOTIFICATIONS_ENABLED
            .store(sys_notif, std::sync::atomic::Ordering::Relaxed);
    }
    // Miniplayer default-view select index from the persisted key.
    window.global::<AppearanceState>().set_miniplayer_view_index(
        crate::ui_prefs::mini_default_view_index(&crate::ui_prefs::load().mini_default_view),
    );
    window.global::<AppearanceState>().set_startup_page_index(
        crate::ui_prefs::startup_page_index(&crate::ui_prefs::load().startup_page),
    );
    // Language selector: seed the dropdown index from the persisted key (the raw
    // user choice, "auto" -> index 0). The live translation was already applied
    // before the window was created.
    window.global::<AppearanceState>().set_language_index(
        crate::ui_prefs::language_index(&crate::ui_prefs::load().language),
    );

    // Theme: seed the dropdown list from the Rust registry, then restore the
    // persisted theme (slug is the source of truth; the dropdown index is
    // derived). Fresh profiles default to OLED Dark (owner decision). This must
    // run before the shell renders so the first paint is the right palette.
    {
        let appearance = window.global::<AppearanceState>();
        let labels: Vec<slint::SharedString> = crate::theme::dropdown_labels()
            .into_iter()
            .map(slint::SharedString::from)
            .collect();
        appearance.set_themes(slint::ModelRc::new(slint::VecModel::from(labels)));

        let id = crate::theme::id_for_slug(&crate::ui_prefs::load().theme);
        appearance.set_theme_index(crate::theme::index_for_id(id));
        appearance.set_theme_is_system(id == qbz_theme::ThemeId::System);
        // Keep the legacy ThemeState.mode in sync with the dropdown index for
        // any residual reads; the palette itself is driven by apply_theme.
        window
            .global::<ThemeState>()
            .set_mode(crate::theme::index_for_id(id));
        crate::theme::apply_theme(&window, id);
    }

    // Tell the tray settings UI which platform it's on so it can show the
    // macOS-only controls ("Menu Bar" header, hide-Dock toggle) and hide the
    // Linux/Windows-only minimize-to-tray row.
    window
        .global::<AppearanceState>()
        .set_is_macos(cfg!(target_os = "macos"));

    let app_runtime = Arc::new(AppRuntime::with_visualizer(SlintAdapter::new(window.as_weak())));

    // ImmersiveView audio visualizers: spawn the frontend-agnostic FFT producer
    // against the runtime's tap and start the 30fps drain into VisualizerState.
    // Inert (tap disabled, no capture / no FFT cost) until the immersive view
    // opens. Must run on the UI thread before window.run().
    visualizer::install(&window, &app_runtime);

    // Prime the FFT tap if we restored straight into Large with the visualizer ON.
    // This MUST run AFTER visualizer::install() — install() registers the
    // `on_set_enabled` handler, so an earlier invoke would no-op (the cause of the
    // dock spectrum sitting idle on the very first playback until the eye toggle
    // was pressed). The AppShell `changed viz-should-run` handler covers every
    // later transition; this only seeds the initial value.
    {
        let shell = window.global::<ShellState>();
        if shell.get_large_active() && shell.get_large_visualizer_on() {
            window.global::<VisualizerState>().invoke_set_enabled(true);
        }
    }

    // WGPU UNDERLAY SPIKE: capture Slint's own wgpu Device/Queue at RenderingSetup
    // so shader_underlay allocates its texture + submits on the SAME device Slint
    // renders with (mandatory for Image::try_from). The render itself happens in
    // the 30fps drain (visualizer.rs). Only one rendering notifier is allowed per
    // window; the shader underlay owns it. Errors here are non-fatal — the shader
    // just stays dark and the rest of the UI is unaffected.
    if let Err(e) = window
        .window()
        .set_rendering_notifier(move |state, graphics_api| {
            match state {
                slint::RenderingState::RenderingSetup => {
                    if let slint::GraphicsAPI::WGPU28 { device, queue, .. } = graphics_api {
                        crate::shader_underlay::setup(device.clone(), queue.clone());
                    }
                }
                slint::RenderingState::RenderingTeardown => {
                    crate::shader_underlay::teardown();
                }
                _ => {}
            }
        })
    {
        log::warn!("[shader] set_rendering_notifier failed: {e:?} — underlay disabled");
    }

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

    // Offline-MODE engine: connectivity monitoring runs for the whole app
    // lifetime (login screen included — the restore flow and the D2 recovery
    // banner both depend on it). Per-user state binds later on activation.
    offline_mode::start();
    // Mirror engine status into the OfflineState Slint global (login
    // affordances + the D2 recovery banner) and seed has-previous-session.
    offline_mode::start_ui_forwarder(window.as_weak());

    // Offline EDGE reactions (D11/D12b). On online→offline: a user standing
    // on a placeholder-blocked Qobuz view auto-navigates to LocalLibrary (the
    // offline default view), the sidebar re-renders from cache (the offline
    // filter keeps locals + mixed-with-local-content, real names intact), and
    // an open My QBZ grid/detail reloads so unavailable items drop (D11.c).
    // On offline→online: NO navigation (blocked views unblock naturally);
    // the sidebar reloads the full Qobuz set.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        tokio_rt.spawn(async move {
            let mut rx = offline_mode::engine().subscribe();
            let initial = *rx.borrow_and_update();
            let mut was_offline = initial.is_offline();
            let mut was_conn_down =
                initial.connectivity == qbz_app::offline_mode::Connectivity::Down;
            while rx.changed().await.is_ok() {
                let status = *rx.borrow_and_update();
                let now_offline = status.is_offline();
                let now_conn_down =
                    status.connectivity == qbz_app::offline_mode::Connectivity::Down;
                let conn_changed = now_conn_down != was_conn_down;
                was_conn_down = now_conn_down;
                if now_offline == was_offline {
                    // Connectivity flipped WITHOUT a mode change (e.g. the
                    // link dying or returning during a logged-out session):
                    // the connectivity-keyed network-folder gate changes the
                    // browse SET, so refresh LocalLibrary in place.
                    if conn_changed {
                        let runtime2 = runtime.clone();
                        let nav_weak = weak.clone();
                        let handle2 = handle.clone();
                        let image_cache2 = image_cache.clone();
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            local_library::reset_browse_models(&w);
                            if w.global::<NavState>().get_view() == ContentView::LocalLibrary {
                                let tab = local_library::LibTab::from_tab_id(
                                    &w.global::<LocalLibraryState>().get_active_tab(),
                                )
                                .unwrap_or(local_library::LibTab::Albums);
                                navigate_local_library(
                                    runtime2, nav_weak, &handle2, image_cache2, tab,
                                );
                            }
                        });
                    }
                    continue;
                }
                was_offline = now_offline;
                if !now_offline {
                    // Back online: refresh the sidebar with the real Qobuz set
                    // (the offline cache may hold synthesized names).
                    load_sidebar_playlists(runtime.clone(), weak.clone(), &handle);
                    // Drop the LocalLibrary browse sets so the next visit
                    // re-fetches under the new state (the connectivity-keyed
                    // network-folder gate may change the SET), and reload in
                    // place when the user is standing there.
                    let runtime2 = runtime.clone();
                    let nav_weak = weak.clone();
                    let handle2 = handle.clone();
                    let image_cache2 = image_cache.clone();
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        local_library::reset_browse_models(&w);
                        if w.global::<NavState>().get_view() == ContentView::LocalLibrary {
                            let tab = local_library::LibTab::from_tab_id(
                                &w.global::<LocalLibraryState>().get_active_tab(),
                            )
                            .unwrap_or(local_library::LibTab::Albums);
                            navigate_local_library(
                                runtime2,
                                nav_weak,
                                &handle2,
                                image_cache2,
                                tab,
                            );
                        }
                    });
                    continue;
                }
                let runtime = runtime.clone();
                let nav_weak = weak.clone();
                let handle2 = handle.clone();
                let image_cache = image_cache.clone();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    // Sidebar: re-render from cache under the new offline state
                    // (the D11.b filter lives in sidebar::rebuild).
                    sidebar::rebuild(&w);
                    refresh_sidebar_covers(&w);
                    // Drop the browse sets so the next fetch (incl. the D12b
                    // navigation below) re-derives under offline. The SET is
                    // identical (network content is never hidden); the reset
                    // only refreshes per-row availability chrome.
                    local_library::reset_browse_models(&w);
                    match w.global::<NavState>().get_view() {
                        // D11.c: refresh the open grid/detail so unavailable
                        // items (and all-unavailable collections) drop.
                        ContentView::Mixtapes => {
                            myqbz::navigate(
                                nav_weak.clone(),
                                handle2.clone(),
                                image_cache.clone(),
                                qbz_models::mixtape::CollectionKind::Mixtape,
                            );
                        }
                        ContentView::Collections => {
                            myqbz::navigate(
                                nav_weak.clone(),
                                handle2.clone(),
                                image_cache.clone(),
                                qbz_models::mixtape::CollectionKind::Collection,
                            );
                        }
                        ContentView::MixtapeDetail => {
                            let id = w.global::<MyQbzDetailState>().get_id().to_string();
                            if !id.is_empty() {
                                myqbz_detail::navigate(
                                    runtime.clone(),
                                    nav_weak.clone(),
                                    handle2.clone(),
                                    image_cache.clone(),
                                    id,
                                );
                            }
                        }
                        ContentView::LocalLibrary => {
                            // Standing on a browse tab: the models were just
                            // reset — reload the active tab in place so the
                            // grid re-fetches under the offline gate instead
                            // of sitting empty until re-entry.
                            let tab = local_library::LibTab::from_tab_id(
                                &w.global::<LocalLibraryState>().get_active_tab(),
                            )
                            .unwrap_or(local_library::LibTab::Albums);
                            navigate_local_library(
                                runtime.clone(),
                                nav_weak.clone(),
                                &handle2,
                                image_cache.clone(),
                                tab,
                            );
                        }
                        _ => {
                            // D12b: blocked Qobuz view → LocalLibrary.
                            if is_offline_blocked_view(&w) {
                                nav::record(nav::NavEntry::LocalLibrary {
                                    tab: local_library::LibTab::Albums.tab_id().to_string(),
                                });
                                update_nav_flags(&w);
                                navigate_local_library(
                                    runtime.clone(),
                                    nav_weak.clone(),
                                    &handle2,
                                    image_cache.clone(),
                                    local_library::LibTab::Albums,
                                );
                            }
                        }
                    }
                });
            }
        });
    }

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
                    // Surface the failure on the login screen (init-error box,
                    // spec §4.1); cleared again on any successful shell entry.
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        w.global::<OfflineState>().set_login_error(e.into());
                        w.set_screen(AppScreen::Login);
                    });
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

    // Offline: enter a full offline session at the last user (local library,
    // offline cache, settings — no Qobuz auth), then show the shell.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        let settings_ctx = settings_ctx.clone();
        window.on_start_offline(move || {
            dispatch(AppCommand::StartOffline);
            let runtime = runtime.clone();
            let weak = weak.clone();
            let image_cache = image_cache.clone();
            let settings_ctx = settings_ctx.clone();
            handle.spawn(async move {
                if let Err(e) = enter_shell_offline(runtime, weak, image_cache, settings_ctx).await
                {
                    log::error!("[qbz-slint] offline start failed: {e}");
                }
            });
        });
    }

    // D2 recovery: one click on the shell banner re-logs-in with the saved
    // token and runs the full online entry over the live offline session.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        let settings_ctx = settings_ctx.clone();
        window.on_recovery_login(move || {
            // Logged BEFORE the spawn: records the click arriving from the
            // UI chain even if the async attempt below stalls or fails.
            log::info!("[qbz-slint] recovery sign-in requested");
            let runtime = runtime.clone();
            let weak = weak.clone();
            let image_cache = image_cache.clone();
            let settings_ctx = settings_ctx.clone();
            handle.spawn(async move {
                // No pre-lift anywhere: the auth endpoints are EXEMPT from
                // the offline gate (qbz-qobuz client), so the token login and
                // the OAuth exchange pass the closed gate — and
                // login_via_system_browser no longer clears offline_session
                // up front either. The flag ends up false only on SUCCESS
                // paths (restore_saved_session / login_via_system_browser
                // clear it after the login completes), so the shell never
                // sits unlocked-and-empty while an attempt is pending, and a
                // failed attempt leaves the live offline session intact.
                match auth::restore_saved_session(&runtime).await {
                    Ok(Some(session)) => {
                        log::info!(
                            "[qbz-slint] recovery login succeeded for user {}",
                            session.user_id
                        );
                        enter_shell(runtime, weak, image_cache, settings_ctx, session).await;
                    }
                    Ok(None) => {
                        // No saved token, or the token was explicitly
                        // rejected (and cleared). The user asked to sign in —
                        // fall back to the full system-browser OAuth. Show
                        // the LOGIN screen FIRST: its UX narrates the
                        // browser flow (the user shouldn't have to notice
                        // the opened browser on their own), and it replaces
                        // the offline shell instead of leaving it on screen
                        // while the attempt runs.
                        log::warn!(
                            "[qbz-slint] recovery login: saved session unusable — falling back to browser OAuth"
                        );
                        let _ = weak.upgrade_in_event_loop(|w| {
                            w.set_screen(AppScreen::Login);
                        });
                        match auth::login_via_system_browser(&runtime).await {
                            Ok(session) => {
                                log::info!(
                                    "[qbz-slint] recovery browser sign-in succeeded for user {}",
                                    session.user_id
                                );
                                enter_shell(runtime, weak, image_cache, settings_ctx, session)
                                    .await;
                            }
                            Err(e) => {
                                log::error!("[qbz-slint] recovery browser sign-in failed: {e}");
                                // The offline session was never lifted, so
                                // there is nothing to restore. Stay on the
                                // Login screen: the error box explains the
                                // failure, and the "Start offline" link
                                // (has-previous-session) leads back into
                                // the offline shell.
                                let _ = weak.upgrade_in_event_loop(move |w| {
                                    toast::error(&w, format!("Sign-in failed: {e}"));
                                    w.global::<OfflineState>().set_login_error(e.into());
                                });
                            }
                        }
                    }
                    Err(e) => {
                        // Init-class failure (gated/unreachable cold bundle
                        // fetch): any transient flag lift was already undone
                        // inside auth, so the offline shell state is intact —
                        // just surface the error.
                        log::error!("[qbz-slint] recovery login failed: {e}");
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            toast::error(&w, format!("Sign-in failed: {e}"));
                            w.global::<OfflineState>().set_login_error(e.into());
                        });
                    }
                }
            });
        });
    }

    // Open a purchase album DETAIL view (from a Purchases grid/list card).
    // Records the back/forward history entry, seeds the bound id + switches the
    // view; the view's `changed album-id` handler fires the fetch (§A.3
    // reactive reload). Mirrors `on_open_album`'s record + navigate +
    // update_nav_flags shape (§5.2 `handlePurchaseAlbumClick`).
    {
        let weak = window.as_weak();
        window.on_open_purchase_album(move |album_id| {
            let id = album_id.to_string();
            nav::record(nav::NavEntry::PurchaseDetail(id.clone()));
            navigate_purchase_detail(weak.clone(), &id);
            if let Some(w) = weak.upgrade() {
                update_nav_flags(&w);
            }
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
            // A Plex/local item carries a metadata group key, not a Qobuz id —
            // route it to the LocalAlbum view (Home "Recently played", the
            // now-playing bar's "Go to album", etc.) instead of the empty
            // Qobuz album view.
            if is_local_album_key(&album_id) {
                nav::record(nav::NavEntry::LocalAlbum(album_id.clone()));
                navigate_local_album(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    album_id,
                );
            } else {
                // Feed Capa B if this Qobuz album was opened from the search
                // results page (gated inside the helper). Local-album keys take
                // the branch above and never reach here.
                if let Some(w) = weak.upgrade() {
                    record_search_interaction(
                        &w,
                        "album",
                        &album_id,
                        crate::search_service::InteractionAction::Open,
                    );
                }
                nav::record(nav::NavEntry::Album(album_id.clone()));
                navigate_album(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    album_id,
                );
            }
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
        window.on_open_artist(move |artist_ref| {
            let artist_ref = artist_ref.to_string();
            // Qobuz artists are numeric ids → the Qobuz artist page. Local/Plex
            // artists have no id, so their surfaces (LocalAlbum link, now-playing
            // "Go to artist") pass the NAME instead → the LocalLibrary Artists
            // tab, focused on that artist.
            if artist_ref.parse::<u64>().is_ok() {
                // Feed Capa B if this Qobuz artist was opened from the search
                // results page (gated inside the helper). Local/Plex artists
                // pass a NAME (non-numeric) and take the branch below — never
                // recorded.
                if let Some(w) = weak.upgrade() {
                    record_search_interaction(
                        &w,
                        "artist",
                        &artist_ref,
                        crate::search_service::InteractionAction::Open,
                    );
                }
                nav::record(nav::NavEntry::Artist(artist_ref.clone()));
                navigate_artist(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    artist_ref,
                );
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
            } else if !artist_ref.trim().is_empty() {
                open_local_artist(&runtime, &weak, &handle, &image_cache, artist_ref);
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
                // Enter -> results page: dismiss the live dropdown and always
                // land on Search > All (never a lingering per-type tab).
                let st = w.global::<SearchState>();
                st.set_cortinilla_open(false);
                st.set_tab(0);
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
            // chars().count(): the >= 2 gate is on grapheme-ish length, not
            // bytes, so a 2-char multibyte query (e.g. CJK) is not rejected.
            if q.chars().count() < 2 {
                SEARCH_DEBOUNCE.with(|t| t.stop());
                // Below the threshold — close the cortinilla so a backspaced
                // query does not leave a stale dropdown open.
                if let Some(w) = weak.upgrade() {
                    w.global::<SearchState>().set_cortinilla_open(false);
                }
                return;
            }

            // --- Cortinilla (live dropdown), only when the module is ON (D5) ---
            // The results-page debounce below is untouched; the cortinilla is a
            // separate, additive surface gated on the kill switch.
            if crate::search_service::is_enabled() {
                let expand_local = if let Some(w) = weak.upgrade() {
                    let st = w.global::<SearchState>();
                    // Always reset the keyboard selection + scroll on (re)open or
                    // refine — never leave a stale "active row" from a prior
                    // search. Arrow nav fires no keystroke, so it is unaffected.
                    st.set_selected_index(-1);
                    st.set_cortinilla_scroll_y(0.0);
                    st.set_cortinilla_open(true);
                    st.set_cortinilla_query(q.clone().into());
                    st.set_cortinilla_loading(true);
                    // Offline OR an unauthenticated (offline) session → the Qobuz
                    // half is empty, so the dropdown is local-only; widen the
                    // on-device section caps.
                    let off = w.global::<OfflineState>();
                    off.get_offline() || off.get_offline_session()
                } else {
                    false
                };
                let cort_version = search::next_cortinilla_version();

                // No cached instant-paint. The cached -> fresh swap (plus the
                // local-fold mid-apply) made the results visibly "jump". Instead
                // the placeholder skeleton (cortinilla-loading) shows while typing
                // and a SINGLE apply paints the real results ~220 ms after the
                // last keystroke — debounced so rapid typing fires one load, not
                // one per keystroke. The version guard drops any stale in-flight
                // load; `load_cortinilla` already folds the on-device section in,
                // so this is one combined paint with no intermediate states.
                {
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    let image_cache = image_cache.clone();
                    let q = q.clone();
                    CORTINILLA_DEBOUNCE.with(|t| {
                        t.start(
                            slint::TimerMode::SingleShot,
                            std::time::Duration::from_millis(220),
                            move || {
                                let runtime = runtime.clone();
                                let weak = weak.clone();
                                let image_cache = image_cache.clone();
                                let q = q.clone();
                                handle.spawn(async move {
                                    match search::load_cortinilla(&runtime, &q, expand_local).await {
                                        Ok((data, local_rows)) => {
                                            let jobs = search::cortinilla_artwork_jobs(&data);
                                            let _ = weak.clone().upgrade_in_event_loop(move |w| {
                                                if search::is_current_cortinilla_version(cort_version) {
                                                    LAST_CORTINILLA.with(|c| {
                                                        *c.borrow_mut() = Some(data.clone())
                                                    });
                                                    LAST_CORTINILLA_LOCAL
                                                        .with(|c| *c.borrow_mut() = local_rows);
                                                    search::apply_cortinilla(&w, data);
                                                }
                                            });
                                            // Mixed payload (Qobuz http / local fs / Plex
                                            // /library/) — route each cover by scheme.
                                            let plex = crate::plex_settings::get();
                                            artwork::spawn_search_loads(
                                                jobs,
                                                plex.base_url,
                                                plex.token,
                                                weak.clone(),
                                                image_cache,
                                            );
                                        }
                                        Err(e) => {
                                            log::error!("[qbz-slint] cortinilla load failed: {e}");
                                            let _ = weak.upgrade_in_event_loop(move |w| {
                                                if search::is_current_cortinilla_version(cort_version) {
                                                    w.global::<SearchState>()
                                                        .set_cortinilla_loading(false);
                                                }
                                            });
                                        }
                                    }
                                });
                            },
                        );
                    });
                }
            }

            // --- Results page LIVE search — ONLY when the module is OFF --------
            // When Intelligent Search is ON, the cortinilla above is the live
            // preview; typing must NOT auto-navigate to the results page. The
            // 300 ms debounce-navigate would otherwise hijack navigation — a
            // pending fire lands on the results page ~300 ms after the last
            // keystroke and overrides wherever the user just went (e.g. a
            // cortinilla row-click), so "I can't navigate anywhere, it takes me
            // to the result". Enter (on_submit) still navigates there. When the
            // module is OFF, keep the Phase-1 live-results behavior unchanged.
            if crate::search_service::is_enabled() {
                SEARCH_DEBOUNCE.with(|t| t.stop());
                return;
            }
            // --- Results page (module OFF): debounce 300 ms, then full search ---
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

    // Cortinilla: dismiss (click-outside / Escape).
    {
        let weak = window.as_weak();
        window.global::<SearchActions>().on_cortinilla_dismiss(move || {
            if let Some(w) = weak.upgrade() {
                let st = w.global::<SearchState>();
                st.set_cortinilla_open(false);
                // Clear the keyboard/hover highlight too — a dismissed dropdown
                // has no meaningful selection, and the `changed view` close-hook
                // (AppShell) relies on this to reset the highlight on navigation.
                st.set_selected_index(-1);
            }
        });
    }

    // Cortinilla: arrow-key move the keyboard highlight (delta -1 up / +1 down).
    // The valid navigable flat indices are NOT guaranteed to be a contiguous
    // 0..=max range (when there is no top result, index 0 is skipped and the
    // section rows start at 1), so the order is rebuilt from the live snapshot:
    // the top-result's flat index first (when present), then every section row's
    // flat index in declaration order. `selected-index == -1` means "nothing
    // highlighted" (Enter falls through to search-all); Down from -1 lands on the
    // first row, Up from the first row returns to -1. Both ends clamp (no wrap).
    {
        let weak = window.as_weak();
        window
            .global::<SearchActions>()
            .on_cortinilla_move_selection(move |delta| {
                let Some(w) = weak.upgrade() else { return };
                // Build the ordered list of navigable flat indices.
                let order: Vec<i32> = LAST_CORTINILLA.with(|c| {
                    let snap = c.borrow();
                    let Some(data) = snap.as_ref() else {
                        return Vec::new();
                    };
                    let mut v: Vec<i32> = Vec::new();
                    if let Some(top) = &data.top {
                        v.push(top.flat_index as i32);
                    }
                    for section in &data.sections {
                        for row in &section.rows {
                            v.push(row.flat_index as i32);
                        }
                    }
                    v
                });
                if order.is_empty() {
                    return;
                }
                let st = w.global::<SearchState>();
                let current = st.get_selected_index();
                // Current position within the navigable order (-1 if nothing /
                // stale value not present anymore).
                let pos = order.iter().position(|&fi| fi == current);
                let new_index: i32 = if delta > 0 {
                    // Down: from "nothing" -> first; otherwise advance, clamping
                    // at the last row.
                    match pos {
                        None => order[0],
                        Some(p) if p + 1 < order.len() => order[p + 1],
                        Some(_) => order[order.len() - 1],
                    }
                } else {
                    // Up: from "nothing" stay nothing; from the first row -> -1;
                    // otherwise step back.
                    match pos {
                        None => -1,
                        Some(0) => -1,
                        Some(p) => order[p - 1],
                    }
                };
                st.set_selected_index(new_index);
                // Content-top y of the selected row so the overlay can scroll it
                // into view. Mirrors Cortinilla.slint's layout EXACTLY: top-result
                // block = padTop(4) + label(22) + row(56); each section block =
                // padTop(4) + header(24) + rows × 56. 0 when nothing is selected.
                let scroll_y: f32 = if new_index < 0 {
                    0.0
                } else {
                    LAST_CORTINILLA.with(|c| {
                        let snap = c.borrow();
                        let Some(data) = snap.as_ref() else {
                            return 0.0;
                        };
                        let mut y: f32 = 0.0;
                        if let Some(top) = &data.top {
                            if top.flat_index as i32 == new_index {
                                return y + 26.0; // padTop 4 + label 22
                            }
                            y += 82.0; // 4 + 22 + 56
                        }
                        for section in &data.sections {
                            y += 28.0; // padTop 4 + header 24
                            for row in &section.rows {
                                if row.flat_index as i32 == new_index {
                                    return y;
                                }
                                y += 56.0;
                            }
                        }
                        0.0
                    })
                };
                st.set_cortinilla_scroll_y(scroll_y);
            });
    }

    // Cortinilla: Enter with nothing highlighted — run a full search-all on the
    // current live query (same path as submit) and dismiss the dropdown.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<SearchActions>()
            .on_cortinilla_search_all(move || {
                let Some(w) = weak.upgrade() else { return };
                let q = w
                    .global::<SearchState>()
                    .get_cortinilla_query()
                    .trim()
                    .to_string();
                if q.chars().count() < 2 {
                    return;
                }
                let st = w.global::<SearchState>();
                st.set_cortinilla_open(false);
                // Activating the cortinilla's Enter affordance clears the input
                // too (consistent with row-click / View-more), so it can't
                // re-invoke the dropdown over the results page.
                st.set_header_search_text("".into());
                // Enter always lands on Search > All, never a per-type tab.
                st.set_tab(0);
                SEARCH_DEBOUNCE.with(|t| t.stop());
                nav::push_or_replace_search(q.clone());
                navigate_search(runtime.clone(), weak.clone(), &handle, image_cache.clone(), q);
                update_nav_flags(&w);
            });
    }

    // Cortinilla: "View more" on a section. Qobuz categories open the full
    // results page on the matching tab (albums=1, tracks=2, artists=3,
    // playlists=4); the "local" section opens the LocalLibrary Tracks tab
    // pre-filtered to the live query (local results never enter the Qobuz
    // results page — D1/D2).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<SearchActions>()
            .on_cortinilla_view_more(move |kind| {
                let Some(w) = weak.upgrade() else { return };
                let kind = kind.to_string();
                let q = w
                    .global::<SearchState>()
                    .get_cortinilla_query()
                    .trim()
                    .to_string();
                if q.chars().count() < 2 {
                    return;
                }
                {
                    let st = w.global::<SearchState>();
                    st.set_cortinilla_open(false);
                    // Clear the input so it can't re-invoke the dropdown later.
                    st.set_header_search_text("".into());
                }
                SEARCH_DEBOUNCE.with(|t| t.stop());

                // On-device "View more": leave the Qobuz results page entirely
                // and open the matching LocalLibrary tab pre-filtered to the live
                // query (D1/D2: local results never live in the Qobuz results
                // page). Albums / Artists / Tracks each route to their own tab,
                // setting that tab's search filter then forcing a re-derive so the
                // filtered set renders on both first-visit and re-entry.
                if kind == "local-album" {
                    w.global::<LocalLibraryState>().set_albums_search(q.clone().into());
                    nav::record(nav::NavEntry::LocalLibrary {
                        tab: local_library::LibTab::Albums.tab_id().to_string(),
                    });
                    navigate_local_library(
                        runtime.clone(),
                        weak.clone(),
                        &handle,
                        image_cache.clone(),
                        local_library::LibTab::Albums,
                    );
                    // Force a reload so the freshly-set search filter applies even
                    // when the Albums tab was already loaded (re-entry).
                    local_library::reload_albums(weak.clone(), handle.clone(), image_cache.clone());
                    update_nav_flags(&w);
                    return;
                }
                if kind == "local-artist" {
                    w.global::<LocalLibraryState>().set_artists_search(q.clone().into());
                    nav::record(nav::NavEntry::LocalLibrary {
                        tab: local_library::LibTab::Artists.tab_id().to_string(),
                    });
                    navigate_local_library(
                        runtime.clone(),
                        weak.clone(),
                        &handle,
                        image_cache.clone(),
                        local_library::LibTab::Artists,
                    );
                    // Re-derive in place so the filter applies on re-entry (the
                    // async first-load re-derives with the same filter on its own).
                    local_library::derive_artists(&w);
                    update_nav_flags(&w);
                    return;
                }
                if kind == "local" {
                    w.global::<LocalLibraryState>().set_tracks_search(q.clone().into());
                    nav::record(nav::NavEntry::LocalLibrary {
                        tab: local_library::LibTab::Tracks.tab_id().to_string(),
                    });
                    navigate_local_library(
                        runtime.clone(),
                        weak.clone(),
                        &handle,
                        image_cache.clone(),
                        local_library::LibTab::Tracks,
                    );
                    // `navigate_local_library` only lazy-loads on an EMPTY tracks
                    // model (re-entry keeps the prior set), so force a reload to
                    // apply the freshly-set search filter regardless.
                    local_library::reload_tracks(weak.clone(), handle.clone());
                    update_nav_flags(&w);
                    return;
                }

                // Qobuz category → open the full results page on the matching tab.
                let tab = match kind.as_str() {
                    "album" => 1,
                    "track" => 2,
                    "artist" => 3,
                    "playlist" => 4,
                    _ => 0,
                };
                nav::push_or_replace_search(q.clone());
                navigate_search(runtime.clone(), weak.clone(), &handle, image_cache.clone(), q);
                // search_all loads every category; the tab switch only changes
                // which list renders. Apply it after navigate so it sticks.
                w.global::<SearchState>().set_tab(tab);
                update_nav_flags(&w);
            });
    }

    // Cortinilla: a row was activated (click or Enter on a highlight). Resolve
    // the flat index against the controller snapshot, then dispatch to the SAME
    // nav/play seams the results page uses.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<SearchActions>()
            .on_cortinilla_row_clicked(move |flat_index| {
                let Some(w) = weak.upgrade() else { return };
                // Resolve flat_index -> the concrete row from the snapshot.
                let row = LAST_CORTINILLA.with(|c| {
                    let snap = c.borrow();
                    let data = snap.as_ref()?;
                    if let Some(top) = &data.top {
                        if top.flat_index as i32 == flat_index {
                            return Some(top.clone());
                        }
                    }
                    data.sections
                        .iter()
                        .flat_map(|s| s.rows.iter())
                        .find(|r| r.flat_index as i32 == flat_index)
                        .cloned()
                });
                let Some(row) = row else { return };

                // Capture the live cortinilla query BEFORE dismissing so the
                // ranking feedback (Capa B) is keyed off the query that produced
                // this row.
                let cort_query = w
                    .global::<SearchState>()
                    .get_cortinilla_query()
                    .to_string();

                // Dismiss the dropdown before acting AND clear the header input —
                // once the user activates a row, leftover text would otherwise
                // re-invoke the cortinilla when focus bounces back to the field.
                {
                    let st = w.global::<SearchState>();
                    st.set_cortinilla_open(false);
                    st.set_header_search_text("".into());
                }

                // Feed Capa B: a clicked QOBUZ row is an interaction with the
                // search-surfaced entity. action = Play for tracks (they play on
                // click), Open for album/artist/playlist (they navigate). LOCAL
                // rows are intentionally NOT recorded — local entities use a
                // different id space (D4) and are skipped in v1. record() no-ops
                // when the module is disabled, so the unconditional call is safe.
                if row.source != "local" {
                    let action = if row.kind == "track" {
                        crate::search_service::InteractionAction::Play
                    } else {
                        crate::search_service::InteractionAction::Open
                    };
                    crate::search_service::record(&cort_query, &row.kind, &row.id, action);
                }

                if row.source == "local" {
                    // On-device rows route by kind (the "links go to LocalLibrary"
                    // requirement): a local ALBUM opens the LocalAlbum view by its
                    // group key; a local ARTIST opens the LocalLibrary Artists tab
                    // by NAME (local artists have no id); a local TRACK plays
                    // through the LOCAL seam.
                    match row.kind.as_str() {
                        "album" => {
                            // `row.id` is the album_group_key (a local album key).
                            let key = row.id.clone();
                            nav::record(nav::NavEntry::LocalAlbum(key.clone()));
                            navigate_local_album(
                                runtime.clone(),
                                weak.clone(),
                                &handle,
                                image_cache.clone(),
                                key,
                            );
                            update_nav_flags(&w);
                        }
                        "artist" => {
                            // Local artists are keyed by NAME (`row.title`).
                            open_local_artist(
                                &runtime,
                                &weak,
                                &handle,
                                &image_cache,
                                row.title.clone(),
                            );
                        }
                        _ => {
                            // Track: play this on-device row + its siblings (so the
                            // queue continues down the list), starting at the
                            // clicked one. `row.id` is the library row id.
                            let tracks = LAST_CORTINILLA_LOCAL.with(|c| c.borrow().clone());
                            let start = tracks
                                .iter()
                                .position(|t| t.id.to_string() == row.id)
                                .unwrap_or(0);
                            if !tracks.is_empty() {
                                playback::play_local_tracks(
                                    runtime.clone(),
                                    weak.clone(),
                                    handle.clone(),
                                    tracks,
                                    start,
                                    false,
                                );
                            }
                        }
                    }
                    return;
                }

                match row.kind.as_str() {
                    "album" => {
                        let id = row.id.clone();
                        nav::record(nav::NavEntry::Album(id.clone()));
                        navigate_album(
                            runtime.clone(),
                            weak.clone(),
                            &handle,
                            image_cache.clone(),
                            id,
                        );
                        update_nav_flags(&w);
                    }
                    "artist" => {
                        let id = row.id.clone();
                        nav::record(nav::NavEntry::Artist(id.clone()));
                        navigate_artist(
                            runtime.clone(),
                            weak.clone(),
                            &handle,
                            image_cache.clone(),
                            id,
                        );
                        update_nav_flags(&w);
                    }
                    "playlist" => {
                        let id = row.id.clone();
                        nav::record(nav::NavEntry::Playlist(id.clone()));
                        navigate_playlist(
                            runtime.clone(),
                            weak.clone(),
                            &handle,
                            image_cache.clone(),
                            id,
                        );
                        update_nav_flags(&w);
                    }
                    "track" => {
                        // A clicked Qobuz track plays immediately (single-track
                        // queue), matching the results-row "play".
                        if let Ok(track_id) = row.id.parse::<u64>() {
                            playback::play_track_now(
                                runtime.clone(),
                                weak.clone(),
                                handle.clone(),
                                track_id,
                            );
                        }
                    }
                    _ => {}
                }
            });
    }

    // ===================== Immersive default view =======================
    // The ImmersiveView is conditionally mounted, so its `init` fires `opened()`
    // exactly when the overlay opens. We apply the configured default view here:
    //   - "remember": restore the persisted last view (view-mode/mode/split-panel).
    //   - pinned view: force FOCUS (view-mode 0) + the matching `mode`.
    // While the overlay is open, `view-changed()` persists the current view so a
    // "remember" default restores it next time (a pinned default never persists —
    // the pin always wins on the next open).
    {
        let weak = window.as_weak();
        window
            .global::<ImmersiveActions>()
            .on_opened(move || {
                let Some(w) = weak.upgrade() else { return };
                let prefs = crate::ui_prefs::load();
                let im = w.global::<ImmersiveState>();
                // A pinned/restored view must not be hidden behind a shader scene.
                im.set_shader_mode(0);
                match prefs.immersive_default_view.as_str() {
                    "remember" => {
                        im.set_view_mode(prefs.immersive_last_view_mode);
                        im.set_mode(prefs.immersive_last_mode);
                        im.set_split_panel(prefs.immersive_last_split_panel);
                    }
                    // Pinned FOCUS views: view-mode 0 + the matching `mode`.
                    // reactive=0, static=1, coverflow=2, spectrum=3, lyrics=4,
                    // queue=5 (FOCUS-mode panel enum in ImmersiveState).
                    "reactive" => {
                        im.set_view_mode(0);
                        im.set_mode(0);
                    }
                    "static" => {
                        im.set_view_mode(0);
                        im.set_mode(1);
                    }
                    "coverflow" => {
                        im.set_view_mode(0);
                        im.set_mode(2);
                    }
                    "spectrum" => {
                        im.set_view_mode(0);
                        im.set_mode(3);
                    }
                    "lyrics" => {
                        im.set_view_mode(0);
                        im.set_mode(4);
                    }
                    "queue" => {
                        im.set_view_mode(0);
                        im.set_mode(5);
                    }
                    _ => {}
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<ImmersiveActions>()
            .on_view_changed(move || {
                let Some(w) = weak.upgrade() else { return };
                // Only persist for the "remember last" default; a pinned default
                // always wins on the next open, so do not overwrite it.
                let mut prefs = crate::ui_prefs::load();
                if prefs.immersive_default_view != "remember" {
                    return;
                }
                let im = w.global::<ImmersiveState>();
                prefs.immersive_last_view_mode = im.get_view_mode();
                prefs.immersive_last_mode = im.get_mode();
                prefs.immersive_last_split_panel = im.get_split_panel();
                crate::ui_prefs::save(&prefs);
            });
    }

    // ===================== Immersive in-view search =====================
    // The in-immersive search dropdown mirrors the main header cortinilla but
    // (1) targets ImmersiveState/ImmersiveSearchActions, (2) loads ONLY
    // Albums/Artists/Playlists (no local, no top result), and (3) on select
    // ACTS ON PLAYBACK (per the Settings "search action") instead of navigating
    // — immersive has no navigation. Gated on ImmersiveState.open AND the
    // configured action != "disabled".

    // Immersive search: live load on type (>= 2 chars; 220 ms debounce; version
    // guard; skeleton-only, no cached instant-paint — same as the main
    // cortinilla now).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<ImmersiveSearchActions>()
            .on_live(move |query| {
                let Some(w) = weak.upgrade() else { return };
                // Gate: only while the immersive overlay is open AND the action
                // is not "disabled" (the action doubles as the enable switch).
                if !w.global::<ImmersiveState>().get_open() {
                    return;
                }
                if crate::ui_prefs::load().immersive_search_action == "disabled" {
                    return;
                }
                let q = query.trim().to_string();
                // chars().count(): grapheme-ish length so a 2-char multibyte
                // query (CJK) is not rejected.
                if q.chars().count() < 2 {
                    IMMERSIVE_SEARCH_DEBOUNCE.with(|t| t.stop());
                    // Below the threshold — close the dropdown so a backspaced
                    // query does not leave a stale one open.
                    w.global::<ImmersiveState>().set_search_open(false);
                    return;
                }

                {
                    let im = w.global::<ImmersiveState>();
                    im.set_search_open(true);
                    im.set_search_query(q.clone().into());
                    im.set_search_loading(true);
                    // ALWAYS reset selection + scroll on every open/refine — never
                    // leave a stale "active row" from a prior query. Arrow nav
                    // fires no keystroke through here, so it is unaffected.
                    im.set_search_selected_index(-1);
                    im.set_search_scroll_y(0.0);
                }
                // Offline OR an unauthenticated session → widen the on-device
                // album cap (immersive shows local albums only).
                let expand_local = {
                    let off = w.global::<OfflineState>();
                    off.get_offline() || off.get_offline_session()
                };
                let version = search::next_immersive_search_version();

                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                let image_cache = image_cache.clone();
                let q = q.clone();
                IMMERSIVE_SEARCH_DEBOUNCE.with(|t| {
                    t.start(
                        slint::TimerMode::SingleShot,
                        std::time::Duration::from_millis(220),
                        move || {
                            let runtime = runtime.clone();
                            let weak = weak.clone();
                            let image_cache = image_cache.clone();
                            let q = q.clone();
                            handle.spawn(async move {
                                match search::load_immersive_search(&runtime, &q, expand_local).await {
                                    Ok(data) => {
                                        let jobs =
                                            search::immersive_cortinilla_artwork_jobs(&data);
                                        let _ = weak.clone().upgrade_in_event_loop(move |w| {
                                            if search::is_current_immersive_search_version(version) {
                                                LAST_IMMERSIVE_SEARCH.with(|c| {
                                                    *c.borrow_mut() = Some(data.clone())
                                                });
                                                search::apply_immersive_search(&w, &data);
                                            }
                                        });
                                        // Mixed payload (Qobuz http / Plex /library/) —
                                        // route each cover by scheme.
                                        let plex = crate::plex_settings::get();
                                        artwork::spawn_search_loads(
                                            jobs,
                                            plex.base_url,
                                            plex.token,
                                            weak.clone(),
                                            image_cache,
                                        );
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "[qbz-slint] immersive search load failed: {e}"
                                        );
                                        let _ = weak.upgrade_in_event_loop(move |w| {
                                            if search::is_current_immersive_search_version(version)
                                            {
                                                w.global::<ImmersiveState>()
                                                    .set_search_loading(false);
                                            }
                                        });
                                    }
                                }
                            });
                        },
                    );
                });
            });
    }

    // Immersive search: arrow-key move the keyboard highlight (delta -1 up / +1
    // down). The immersive payload has NO top result, so the navigable order is
    // built from section rows only (flat indices start at 1). `-1` means
    // "nothing highlighted"; Down from -1 lands on the first row, Up from the
    // first row returns to -1. Both ends clamp (no wrap).
    {
        let weak = window.as_weak();
        window
            .global::<ImmersiveSearchActions>()
            .on_move_selection(move |delta| {
                let Some(w) = weak.upgrade() else { return };
                let order: Vec<i32> = LAST_IMMERSIVE_SEARCH.with(|c| {
                    let snap = c.borrow();
                    let Some(data) = snap.as_ref() else {
                        return Vec::new();
                    };
                    // No top result in the immersive payload — section rows only.
                    let mut v: Vec<i32> = Vec::new();
                    for section in &data.sections {
                        for row in &section.rows {
                            v.push(row.flat_index as i32);
                        }
                    }
                    v
                });
                if order.is_empty() {
                    return;
                }
                let im = w.global::<ImmersiveState>();
                let current = im.get_search_selected_index();
                let pos = order.iter().position(|&fi| fi == current);
                let new_index: i32 = if delta > 0 {
                    match pos {
                        None => order[0],
                        Some(p) if p + 1 < order.len() => order[p + 1],
                        Some(_) => order[order.len() - 1],
                    }
                } else {
                    match pos {
                        None => -1,
                        Some(0) => -1,
                        Some(p) => order[p - 1],
                    }
                };
                im.set_search_selected_index(new_index);
                // Content-top y of the selected row so the overlay scrolls it
                // into view. The immersive cortinilla has NO top-result block, so
                // each section block = padTop(4) + header(24) + rows × 56. These
                // constants MUST match the immersive cortinilla component's
                // layout (UI agent owns ImmersiveSearchCortinilla.slint — if its
                // row height / section header / pad differ from 56 / 24 / 4, this
                // arithmetic must be updated to match).
                let scroll_y: f32 = if new_index < 0 {
                    0.0
                } else {
                    LAST_IMMERSIVE_SEARCH.with(|c| {
                        let snap = c.borrow();
                        let Some(data) = snap.as_ref() else {
                            return 0.0;
                        };
                        let mut y: f32 = 0.0;
                        for section in &data.sections {
                            y += 28.0; // padTop 4 + header 24
                            for row in &section.rows {
                                if row.flat_index as i32 == new_index {
                                    return y;
                                }
                                y += 56.0; // row height
                            }
                        }
                        0.0
                    })
                };
                im.set_search_scroll_y(scroll_y);
            });
    }

    // Immersive search: dismiss (click-outside / Escape).
    {
        let weak = window.as_weak();
        window
            .global::<ImmersiveSearchActions>()
            .on_dismiss(move || {
                if let Some(w) = weak.upgrade() {
                    let im = w.global::<ImmersiveState>();
                    im.set_search_open(false);
                    im.set_search_selected_index(-1);
                }
            });
    }

    // Immersive search: a row was activated (click or Enter on a highlight).
    // Resolve the flat index against the controller snapshot, then DISPATCH TO
    // PLAYBACK per the configured "search action" (immersive has no navigation,
    // so a selection acts on the queue and STAYS in immersive).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<ImmersiveSearchActions>()
            .on_row_clicked(move |flat_index| {
                let Some(w) = weak.upgrade() else { return };
                // Resolve flat_index -> the concrete row from the snapshot (no
                // top result in the immersive payload — section rows only).
                let row = LAST_IMMERSIVE_SEARCH.with(|c| {
                    let snap = c.borrow();
                    let data = snap.as_ref()?;
                    data.sections
                        .iter()
                        .flat_map(|s| s.rows.iter())
                        .find(|r| r.flat_index as i32 == flat_index)
                        .cloned()
                });
                let Some(row) = row else { return };

                // Close the dropdown (STAY in immersive — no navigation) AND clear
                // the field, mirroring the main cortinilla: once a result is
                // activated, a lingering query would re-invoke the dropdown when
                // focus returns to the field.
                {
                    let im = w.global::<ImmersiveState>();
                    im.set_search_open(false);
                    im.set_search_input_text("".into());
                }

                // Read the configured action fresh (it can change in Settings
                // while immersive is open). "disabled" is also gated upstream in
                // on_live, but guard here too in case the dropdown was already
                // open when it flipped.
                let action = crate::ui_prefs::load().immersive_search_action;
                if action == "disabled" {
                    return;
                }

                // Local album rows queue the ON-DEVICE album per the action
                // (immersive shows local albums only). Branch BEFORE the Qobuz
                // (kind, action) match — `play_album`/`enqueue_album*` expect a
                // numeric Qobuz id, but a local album's id is a group key.
                if row.source == "local" {
                    let group_key = row.id.clone();
                    match action.as_str() {
                        "replace" => playback::play_local_album(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            group_key,
                            None,
                        ),
                        next_or_queue @ ("next" | "queue") => {
                            // Fetch the album's local tracks off-thread (covers
                            // filled like play_local_album), then enqueue: "next"
                            // inserts after the current track, "queue" appends.
                            let next = next_or_queue == "next";
                            let runtime = runtime.clone();
                            let handle = handle.clone();
                            handle.clone().spawn(async move {
                                let tracks = tokio::task::spawn_blocking(move || {
                                    let mut t = crate::local_library::fetch_album_tracks_blocking(
                                        &group_key,
                                    );
                                    playback::fill_missing_covers(&mut t);
                                    t
                                })
                                .await
                                .unwrap_or_default();
                                if !tracks.is_empty() {
                                    playback::enqueue_local_tracks(runtime, handle, tracks, next);
                                }
                            });
                        }
                        _ => {}
                    }
                    return;
                }

                let id = row.id.clone();
                match (row.kind.as_str(), action.as_str()) {
                    // ---- Replace the queue and play (the play_* seams) -------
                    ("album", "replace") => playback::play_album(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id,
                        0,
                    ),
                    ("playlist", "replace") => playback::play_playlist(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id,
                    ),
                    ("artist", "replace") => playback::play_artist_top_tracks(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id,
                    ),
                    // ---- Play next (insert after current, no replace) --------
                    ("album", "next") => playback::enqueue_album_next(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id,
                    ),
                    ("playlist", "next") => playback::enqueue_playlist(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id,
                        true,
                    ),
                    // ---- Add to queue (append, no replace) -------------------
                    ("album", "queue") => playback::enqueue_album(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id,
                    ),
                    ("playlist", "queue") => playback::enqueue_playlist(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id,
                        false,
                    ),
                    // ---- Artist next/queue: no id-less artist enqueue seam
                    // exists, so fetch the artist's top-track ids and route
                    // through the proven `enqueue_artist_top_selected` (it
                    // re-fetches + filters blacklist + is QConnect-aware). The
                    // page is cached, so the extra id-fetch is cheap.
                    ("artist", "next") | ("artist", "queue") => {
                        let next = action == "next";
                        let runtime = runtime.clone();
                        let weak = weak.clone();
                        let handle = handle.clone();
                        let artist_id = id.clone();
                        handle.clone().spawn(async move {
                            let pid: u64 = match artist_id.parse() {
                                Ok(v) => v,
                                Err(_) => return,
                            };
                            let ids: Vec<String> = match runtime.core().get_artist_page(pid, None).await
                            {
                                Ok(page) => page
                                    .top_tracks
                                    .unwrap_or_default()
                                    .into_iter()
                                    .map(|t| t.id.to_string())
                                    .collect(),
                                Err(e) => {
                                    log::error!(
                                        "[qbz-slint] immersive search: artist top fetch failed: {e}"
                                    );
                                    return;
                                }
                            };
                            if ids.is_empty() {
                                return;
                            }
                            // `enqueue_artist_top_selected` is a plain fn that
                            // spawns its own task (it does not touch Slint state
                            // synchronously), so it is safe to call straight from
                            // this worker task — no event-loop hop needed.
                            playback::enqueue_artist_top_selected(
                                runtime.clone(),
                                weak.clone(),
                                handle.clone(),
                                artist_id.clone(),
                                ids,
                                next,
                            );
                        });
                    }
                    _ => {}
                }
            });
    }

    // History navigation — back / forward / settings, all recorded by the
    // nav module so the [<] [>] pair and the mouse buttons stay in sync.
    {
        let weak = window.as_weak();
        window.global::<NavState>().on_request_settings(move || {
            nav::record(nav::NavEntry::Settings);
            if let Some(w) = weak.upgrade() {
                seed_blacklist_status(&w);
                w.global::<NavState>().set_view(ContentView::Settings);
                update_nav_flags(&w);
            }
        });
    }

    // Keyboard shortcuts (hotkeys): seed the cheatsheet/editor model + wire the
    // customize-editor capture callbacks. The global key dispatch itself lives
    // in `install_browser_mouse_nav`'s winit handler.
    keybindings::wire(&window);

    // "Open Qobuz Link" (Ctrl+L) — the cross-platform link resolver modal.
    {
        let weak = window.as_weak();
        window
            .global::<LinkResolverActions>()
            .on_url_changed(move |url| {
                if let Some(w) = weak.upgrade() {
                    w.global::<LinkResolverState>()
                        .set_platform(link_resolver::detect_platform(&url).into());
                }
            });
    }
    {
        let weak = window.as_weak();
        window.global::<LinkResolverActions>().on_close(move || {
            if let Some(w) = weak.upgrade() {
                w.global::<LinkResolverState>().set_open(false);
            }
        });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LinkResolverActions>()
            .on_open_importer(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<LinkResolverState>().set_open(false);
                    w.global::<PlaylistImportState>().set_open(true);
                }
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<LinkResolverActions>().on_submit(move |url| {
            let url = url.trim().to_string();
            if url.is_empty() {
                return;
            }
            if let Some(w) = weak.upgrade() {
                let s = w.global::<LinkResolverState>();
                s.set_resolving(true);
                s.set_error("".into());
                s.set_playlist_detected(false);
            }
            let runtime = runtime.clone();
            let weak = weak.clone();
            let handle = handle.clone();
            let image_cache = image_cache.clone();
            handle.clone().spawn(async move {
                let result = link_resolver::resolve(runtime.clone(), url).await;
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let s = w.global::<LinkResolverState>();
                    s.set_resolving(false);
                    match result {
                        Ok(qbz_music_link::MusicLinkResult::Resolved { link, .. }) => {
                            s.set_open(false);
                            apply_resolved_link(
                                link,
                                &runtime,
                                &w.as_weak(),
                                &handle,
                                &image_cache,
                            );
                        }
                        Ok(qbz_music_link::MusicLinkResult::PlaylistDetected { provider }) => {
                            s.set_playlist_detected(true);
                            s.set_playlist_provider(provider.into());
                        }
                        Ok(qbz_music_link::MusicLinkResult::NotOnQobuz { .. }) => {
                            s.set_error(
                                qbz_i18n::t("This content is not available on Qobuz").into(),
                            );
                        }
                        Err(e) => {
                            log::warn!("[qbz-slint] open-link resolve failed: {e}");
                            s.set_error(qbz_i18n::t("Could not resolve that link").into());
                        }
                    }
                });
            });
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<NavState>().on_request_back(move || {
            if let Some((entry, scroll)) = nav::go_back() {
                arm_scroll_restore(&weak, &entry, scroll);
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
            if let Some((entry, scroll)) = nav::go_forward() {
                arm_scroll_restore(&weak, &entry, scroll);
                apply_entry(entry, &runtime, &weak, &handle, &image_cache);
            }
            if let Some(w) = weak.upgrade() {
                update_nav_flags(&w);
            }
        });
    }
    {
        // The mounted scroll container reports its live viewport-y here so the
        // nav module can stamp the outgoing entry on the next navigation.
        window
            .global::<NavState>()
            .on_report_scroll(|y| nav::set_live_scroll(y));
    }

    // Log out: clear the session and return to the login screen.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.on_logout(move || {
            // Drop any Discord "now listening" activity on logout.
            discord_rpc::clear(&handle);
            // Clear the per-user purchases cache + download-status store so the
            // next account never sees the previous user's purchased items or
            // in-flight download statuses (cross-account data leak).
            purchases::reset_ui_cache();
            let runtime = runtime.clone();
            let weak = weak.clone();
            handle.spawn(async move {
                // Tear down any live cast session + poll (fixes the Tauri
                // logout leak where the connection + position interval lived on).
                if let Some(cast) = cast_service::service() {
                    cast.shutdown().await;
                }
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

    // Discord Rich Presence (Settings > Integrations): seed the toggle from the
    // persisted opt-in and wire the change to the controller (persist + apply).
    {
        let de = window.global::<DiscordState>();
        de.set_enabled(crate::ui_prefs::load().discord_rpc_enabled);
        let runtime = app_runtime.clone();
        let handle = tokio_rt.handle().clone();
        de.on_set_enabled(move |enabled| {
            discord_rpc::set_enabled(enabled, &runtime, &handle);
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

    // Settings — the output-device refresh/release button: free a device QBZ
    // holds exclusively (ALSA Direct) and re-enumerate, so a freed or
    // hot-plugged DAC reappears without an app restart.
    {
        let runtime = app_runtime.clone();
        let settings_ctx = settings_ctx.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.on_settings_release_device(move || {
            let runtime = runtime.clone();
            let settings_ctx = settings_ctx.clone();
            let weak = weak.clone();
            handle.spawn(async move {
                settings::handle_release_device(settings_ctx, runtime, weak).await;
            });
        });
    }

    // Settings > Offline MODE — re-seed the toggle states on panel mount
    // (the panel's init fires load), and the status row's "Check now"
    // connectivity re-probe. The toggles themselves persist through the
    // generic settings-bool path above.
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<OfflineModeActions>().on_load(move || {
            offline_mode::seed_settings(weak.clone(), handle.clone());
        });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<OfflineModeActions>().on_check_now(move || {
            offline_mode::check_now(weak.clone(), handle.clone());
        });
    }
    // The header badge flyout's quick offline toggle — same persistence +
    // #279 snapshot path as the Settings "Enable Offline Mode" toggle.
    {
        let runtime = app_runtime.clone();
        let settings_ctx = settings_ctx.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<OfflineModeActions>()
            .on_set_offline(move |value| {
                let runtime = runtime.clone();
                let settings_ctx = settings_ctx.clone();
                let weak = weak.clone();
                handle.spawn(async move {
                    settings::handle_bool(
                        settings_ctx,
                        runtime,
                        weak,
                        "offline-mode-enabled".to_string(),
                        value,
                    )
                    .await;
                });
            });
    }

    // B9 — offline Favorites "playable favorites" rail: rebuild on every
    // mount of the Favorites offline placeholder (the rail's init fires
    // load), play the rail from the clicked row.
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<OfflineFavoritesActions>().on_load(move || {
            offline_favorites::load(weak.clone(), handle.clone());
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<OfflineFavoritesActions>().on_play(move |id| {
            offline_favorites::play(
                runtime.clone(),
                weak.clone(),
                handle.clone(),
                id.to_string(),
            );
        });
    }

    // Appearance settings persistence. The toggles/selects set their
    // AppearanceState property locally, then fire these generic callbacks so
    // the choice survives restart. Tray keys persist to the shared per-user
    // tray_settings store; unknown keys are logged (other appearance settings
    // are wired as they land).
    {
        let appearance = window.global::<AppearanceState>();
        appearance.on_appearance_bool(|key, value| match key.as_str() {
            "album-header-gradient" => {
                let mut prefs = crate::ui_prefs::load();
                prefs.album_header_gradient = value;
                crate::ui_prefs::save(&prefs);
            }
            "intelligent-search" => {
                let mut prefs = crate::ui_prefs::load();
                prefs.intelligent_search = value;
                crate::ui_prefs::save(&prefs);
                // Propagate the toggle to the bound SearchService kill switch
                // (no-op if no session is bound; the next session init re-seeds
                // the flag from the persisted pref above).
                crate::search_service::set_enabled(value);
            }
            "system-notifications" => {
                let mut prefs = crate::ui_prefs::load();
                prefs.system_notifications = value;
                crate::ui_prefs::save(&prefs);
                // Live gate for the poll-thread notify path (no restart).
                playback::NOTIFICATIONS_ENABLED
                    .store(value, std::sync::atomic::Ordering::Relaxed);
            }
            "tray-enable" => tray_settings::set_enable_tray(value),
            "tray-minimize-to-tray" => tray_settings::set_minimize_to_tray(value),
            "tray-close-to-tray" => tray_settings::set_close_to_tray(value),
            "tray-mac-hide-dock" => tray_settings::set_mac_hide_dock(value),
            "show-purchases" => {
                // Opt-in Purchases nav visibility (default OFF). The bound
                // `AppearanceState.show-purchases` property was already flipped
                // by the toggle, so the sidebar / header entries update live;
                // here we only persist the choice so it survives restart.
                let mut prefs = crate::ui_prefs::load();
                prefs.show_purchases = value;
                crate::ui_prefs::save(&prefs);
            }
            "nav-tb-purchases" => {
                // Purchases title-bar placement (default OFF). Persist only —
                // the live flag is already on `AppearanceState.nav-tb-purchases`.
                let mut prefs = crate::ui_prefs::load();
                prefs.nav_tb_purchases = value;
                crate::ui_prefs::save(&prefs);
            }
            other => log::debug!("[qbz-slint] unhandled appearance-bool '{other}'"),
        });
        let theme_weak = window.as_weak();
        appearance.on_appearance_select(move |key, index| match key.as_str() {
            "tray-icon-theme" => {
                tray_settings::set_icon_theme_index(index);
                // Re-theme the running tray icon live (no restart).
                if let Some(t) = tray::handle() {
                    t.set_icon_theme(tray_settings::theme_for_index(index));
                }
            }
            "immersive-search-action" => {
                // 0 = Disabled, 1 = Replace, 2 = Play next, 3 = Add to queue.
                let mut prefs = crate::ui_prefs::load();
                prefs.immersive_search_action =
                    crate::ui_prefs::immersive_search_action_for_index(index).to_string();
                crate::ui_prefs::save(&prefs);
            }
            "immersive-default-view" => {
                // 0 = Remember last, 1-6 = pinned FOCUS views (reactive / static /
                // coverflow / spectrum / lyrics / queue).
                let mut prefs = crate::ui_prefs::load();
                prefs.immersive_default_view =
                    crate::ui_prefs::immersive_default_view_for_index(index).to_string();
                crate::ui_prefs::save(&prefs);
            }
            "miniplayer-view" => {
                // 0 = Remember last, 1-5 = micro / compact / artwork / queue /
                // lyrics. The miniplayer reads this key on open (miniplayer.rs).
                let mut prefs = crate::ui_prefs::load();
                prefs.mini_default_view =
                    crate::ui_prefs::mini_default_view_for_index(index).to_string();
                crate::ui_prefs::save(&prefs);
            }
            "startup-page" => {
                // 0 = Home, 1 = Where you left off (restore last_view).
                let mut prefs = crate::ui_prefs::load();
                prefs.startup_page = crate::ui_prefs::startup_page_for_index(index).to_string();
                crate::ui_prefs::save(&prefs);
            }
            "language" => {
                // 0 = Auto, 1 = English, 2 = Español, 3 = Français, 4 = Deutsch,
                // 5 = Português. Persist the RAW user choice ("auto" stays
                // "auto"), but resolve "auto" to a concrete language before
                // switching the live translations.
                let chosen = crate::ui_prefs::language_for_index(index);
                let mut prefs = crate::ui_prefs::load();
                prefs.language = chosen.to_string();
                crate::ui_prefs::save(&prefs);
                let lang = if chosen == "auto" {
                    qbz_i18n::resolve_auto()
                } else {
                    chosen
                };
                qbz_i18n::set_language(lang);
                if let Err(e) = slint::select_bundled_translation(lang) {
                    log::warn!(
                        "[qbz-slint] select_bundled_translation('{lang}') failed: {e:?}"
                    );
                }
                // Reseed the non-reactive option arrays (they live as @tr
                // property DEFAULTS, which do NOT react to a translation
                // switch) so the dropdown contents update live.
                if let Some(w) = theme_weak.upgrade() {
                    reseed_i18n_labels(&w);
                }
            }
            "theme" => {
                // Slug is the source of truth: map the dropdown index -> stable
                // id, persist the slug, then hot-switch the live palette.
                let id = crate::theme::id_for_index(index);
                let mut prefs = crate::ui_prefs::load();
                prefs.theme = id.slug().to_string();
                crate::ui_prefs::save(&prefs);
                if let Some(w) = theme_weak.upgrade() {
                    crate::theme::apply_theme(&w, id);
                }
            }
            other => log::debug!("[qbz-slint] unhandled appearance-select '{other}'"),
        });
    }

    // "My QBZ" nav branding (Settings > Appearance) — persist the label /
    // custom icon per-user and re-seed MyQbzBrandingState so the sidebar row
    // updates live. Re-homed from the Tauri sidebar context-menu modal (DQ3).
    {
        let branding = window.global::<MyQbzBrandingState>();
        // Label: persist (blank coerces to "My QBZ" in the store) and push the
        // coerced value onto the shared `label` property so the sidebar row
        // updates live. We set only `label` (not a full re-seed) so the bound
        // LineEdit isn't disturbed mid-edit beyond the documented blank->default
        // coercion. The icon state is left untouched here.
        let weak = window.as_weak();
        branding.on_set_label(move |label| {
            let coerced = myqbz_prefs::set_label(label.as_str());
            if let Some(w) = weak.upgrade() {
                w.global::<MyQbzBrandingState>().set_label(coerced.into());
            }
        });
        // Change icon: async native picker; persists + re-seeds on pick.
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        branding.on_pick_icon(move || {
            myqbz_prefs::pick_icon(weak.clone(), handle.clone());
        });
        // Reset icon: clear the custom path, re-seed to the default glyph.
        let weak = window.as_weak();
        branding.on_reset_icon(move || {
            myqbz_prefs::reset_icon();
            if let Some(w) = weak.upgrade() {
                myqbz_prefs::seed(&w);
            }
        });
    }

    // Context-menu / overlay media actions — route play / queue actions
    // into the playback controller; favorite / download stay logged.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.on_media_action(move |kind, id, action| {
            let kind = kind.to_string();
            let id = id.to_string();
            let action = action.to_string();
            log::info!("[qbz-slint] media-action: kind={kind} id={id} action={action}");
            // Local Library album detail reuses AlbumPageView. Route its play
            // actions to local playback — guarded to the album view + is-local
            // so Qobuz album/track play is untouched.
            if action == "play" && (kind == "album" || kind == "track") {
                if let Some(w) = weak.upgrade() {
                    let album_state = w.global::<AlbumState>();
                    if matches!(w.global::<NavState>().get_view(), ContentView::Album)
                        && album_state.get_is_local()
                    {
                        let album_id = album_state.get_id().to_string();
                        let start = if kind == "track" {
                            id.parse::<i64>().ok()
                        } else {
                            None
                        };
                        playback::play_local_album(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            album_id,
                            start,
                        );
                        return;
                    }
                }
            }

            // === Capa B feedback (intelligent search) ====================
            // Feed the ranking model from RESULTS-PAGE clicks, gated to the
            // Search view inside `record_search_interaction` so the same global
            // media-action handler fired from other views never mis-attributes.
            // Only QOBUZ result clicks are recorded; the search results page
            // never carries local rows (D1/D2), so no source check is needed.
            //   - track play              -> Play
            //   - album play              -> Play (an album-card play is still a
            //                                play interaction with the entity)
            //   - album favorite (add)    -> Favorite (the grid/menu heart arm is
            //                                add-only)
            //   - artist follow (add)     -> Favorite (search artist cards show
            //                                "Follow" only when NOT following, so
            //                                this action is always an add)
            //   - track favorite (toggle) -> Favorite ONLY when transitioning to
            //                                favorited (Favorite weight must only
            //                                ADD — never record on un-favorite)
            if let Some(w) = weak.upgrade() {
                use crate::search_service::InteractionAction;
                match (kind.as_str(), action.as_str()) {
                    ("track", "play") | ("album", "play") => {
                        record_search_interaction(&w, &kind, &id, InteractionAction::Play);
                    }
                    ("album", "favorite") | ("artist", "follow") => {
                        // Both are add-only paths on a search card.
                        record_search_interaction(&w, &kind, &id, InteractionAction::Favorite);
                    }
                    ("track", "favorite") => {
                        // Toggle: record ONLY when this click ADDS the favorite
                        // (the current cached state is "not favorite"). Reading
                        // the pre-toggle state here matches `toggle_track_favorite`,
                        // which flips off the same `fav_cache::is_favorite`.
                        if !crate::fav_cache::is_favorite(&id) {
                            record_search_interaction(&w, &kind, &id, InteractionAction::Favorite);
                        }
                    }
                    _ => {}
                }
            }

            match (kind.as_str(), action.as_str()) {
                // Now-playing bar layout switch (New / Classic / Small).
                // Persisted to ui_prefs so the choice survives restarts.
                // Large/window modes are disabled in the menu until those
                // layouts land.
                ("npb-view", "immersive") => {
                    if let Some(w) = weak.upgrade() {
                        let im = w.global::<ImmersiveState>();
                        // Open deterministically into Album Reactive (mode 0):
                        // the only real foreground this session. Property default
                        // is already 0; set explicitly so a prior session's mode
                        // (once persistence lands) never reopens onto an empty
                        // atmosphere-only placeholder.
                        im.set_mode(0);
                        im.set_open(true);
                        w.global::<VisualizerState>().invoke_set_enabled(true);
                    }
                }
                ("npb-view", "miniplayer") => {
                    crate::miniplayer::enter();
                }
                // Large (mode 3) — docks the cover + spectrum at the bottom of
                // the OPEN sidebar, so force the sidebar to its open state before
                // switching. The FFT tap is driven by AppShell's combined
                // `viz-should-run` changed handler (Large active OR immersive
                // open); forcing the state below makes `large-active` true, which
                // turns the tap on.
                ("npb-view", "large") => {
                    if let Some(w) = weak.upgrade() {
                        let shell = w.global::<ShellState>();
                        shell.set_sidebar_state(0);
                        shell.set_npb_mode(crate::ui_prefs::npb_mode_index("large"));
                        let mut prefs = crate::ui_prefs::load();
                        prefs.npb_mode = "large".to_string();
                        crate::ui_prefs::save(&prefs);
                    }
                }
                ("npb-view", mode @ ("new" | "classic" | "small")) => {
                    if let Some(w) = weak.upgrade() {
                        w.global::<ShellState>()
                            .set_npb_mode(crate::ui_prefs::npb_mode_index(mode));
                        let mut prefs = crate::ui_prefs::load();
                        prefs.npb_mode = mode.to_string();
                        crate::ui_prefs::save(&prefs);
                    }
                }
                // Large dock: visualizer on/off toggle (the cover's eye button).
                // Routed through Rust so the choice persists in ui_prefs; the
                // AppShell viz-should-run handler idles the FFT tap when off.
                ("npb-large", "viz-toggle") => {
                    if let Some(w) = weak.upgrade() {
                        let shell = w.global::<ShellState>();
                        let on = !shell.get_large_visualizer_on();
                        shell.set_large_visualizer_on(on);
                        let mut prefs = crate::ui_prefs::load();
                        prefs.large_visualizer = on;
                        crate::ui_prefs::save(&prefs);
                    }
                }
                // Large dock: cycle the spectrum visualization (Bars -> Waveform
                // -> Energy), persisted in ui_prefs.
                ("npb-large", "spectrum-cycle") => {
                    if let Some(w) = weak.upgrade() {
                        let shell = w.global::<ShellState>();
                        let next = (shell.get_large_spectrum_mode() + 1).rem_euclid(3);
                        shell.set_large_spectrum_mode(next);
                        let mut prefs = crate::ui_prefs::load();
                        prefs.large_spectrum_mode =
                            crate::ui_prefs::large_spectrum_mode_key(next).to_string();
                        crate::ui_prefs::save(&prefs);
                    }
                }
                // Track Info modal — opened from the NPB (i) button, the
                // song-card title, or a TrackRow context menu. Qobuz tracks
                // only (the id must be a real catalog u64).
                ("track", "track-info") => {
                    if let Ok(track_id) = id.parse::<u64>() {
                        info_modals::open_track_info(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            track_id,
                        );
                    }
                }
                // Album Info (Credits/Review) modal — opened from the album
                // header (i) button. Qobuz albums only (skip local/Plex keys).
                ("album", "info") => {
                    if !is_local_album_key(&id) {
                        info_modals::open_album_credits(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            id,
                        );
                    }
                }
                // Album booklet (digital liner-notes PDF) — the album-header
                // booklet button DOWNLOADS the goody PDF (stashed by
                // album::apply_album) to a user-chosen location. No-op when the
                // album bundles no booklet (empty stashed URL).
                ("album", "booklet") => {
                    crate::booklet::download_booklet(weak.clone(), handle.clone());
                }
                // "From the same artist" carousel "View all" — open the artist's
                // full Albums discography page. `id` is the artist id; reuse the
                // dedicated releases page (release_type "album").
                ("artist", "releases") => {
                    if !id.is_empty() {
                        let name = weak
                            .upgrade()
                            .map(|w| w.global::<AlbumState>().get_artist().to_string())
                            .unwrap_or_default();
                        nav::record(nav::NavEntry::ArtistReleases {
                            id: id.clone(),
                            name: name.clone(),
                            release_type: "album".to_string(),
                        });
                        navigate_artist_releases(
                            runtime.clone(),
                            weak.clone(),
                            &handle,
                            image_cache.clone(),
                            id.clone(),
                            name,
                            "album".to_string(),
                        );
                        if let Some(w) = weak.upgrade() {
                            update_nav_flags(&w);
                        }
                    }
                }
                ("album", "play") => {
                    // A Plex/local id is a metadata group key, not a Qobuz id —
                    // play it from the local/Plex cache (Home "Recently played",
                    // etc.) instead of trying to fetch a Qobuz album.
                    if is_local_album_key(&id) {
                        playback::play_local_album(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            id,
                            None,
                        );
                    } else {
                        playback::play_album(runtime.clone(), weak.clone(), handle.clone(), id, 0);
                    }
                }
                ("track", "play") => {
                    // Universal per-row play: queue the current view's VISIBLE
                    // tracklist starting at the clicked track (see
                    // playback::play_track_in_context). Every tracklist surface
                    // routes here — album, playlist, favorites, label, mix,
                    // artist, search.
                    if let Some(w) = weak.upgrade() {
                        playback::play_track_in_context(
                            &w,
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            &id,
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
                    // SOURCE-TYPED routing first (spec §3.2, mirrors the
                    // add-to-playlist arm): on a snapshot-backed playlist
                    // detail a local row's id is a library row id and a plex
                    // row's a synthetic 2^40 id — the catalog path below
                    // would mis-resolve them (wrong-track hazard / silent
                    // failure). The merged snapshot carries the ready,
                    // source-aware QueueTrack; enqueue it directly.
                    // DELIBERATE Tauri deviation for plex rows: Tauri renders
                    // Play Next / Add to Queue as silent no-ops there (spec
                    // §1.6.2) — Slint's queue carries plex rows fine, so we
                    // wire them instead of porting the dead entries.
                    if let Some(w) = weak.upgrade() {
                        if snapshot_detail_open(&w) {
                            if let Some(qt) = local_playlist::queue_track_for_row(&id) {
                                if matches!(qt.source.as_deref(), Some("local") | Some("plex")) {
                                    playback::enqueue_queue_tracks(
                                        runtime.clone(),
                                        weak.clone(),
                                        handle.clone(),
                                        vec![qt],
                                        false,
                                    );
                                    return;
                                }
                            }
                        }
                    }
                    // Qobuz rows (incl. offline copies with real catalog
                    // ids): the existing path — QConnect single-track
                    // admission + fresh fetch.
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
                ("album", "shuffle") => playback::play_album_shuffled(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id,
                ),
                ("album", "edit") => {
                    // Open the local-album tag editor (group_key == directory_path
                    // for folder-grouped local albums).
                    tag_editor::open_tag_editor(weak.clone(), handle.clone(), id.clone(), id);
                }
                ("album", "add-to-mixtape") => {
                    // The cassette button on the album header. Local albums
                    // (incl. Plex, stored as source "local") build the payload
                    // from AlbumState + the loaded tracks; Qobuz albums resolve
                    // via get_album (the proven fail-safe resolver).
                    let Some(w) = weak.upgrade() else { return };
                    let st = w.global::<AlbumState>();
                    if st.get_is_local() {
                        let item = myqbz_add::AddItem {
                            item_type: "album".into(),
                            source: "local".into(),
                            source_item_id: st.get_id().to_string(),
                            title: st.get_title().to_string(),
                            subtitle: {
                                let a = st.get_artist().to_string();
                                (!a.is_empty()).then_some(a)
                            },
                            artwork_url: None, // local albums omit artwork_url (1:1 PSD)
                            year: None,
                            track_count: {
                                use slint::Model;
                                let n = st.get_tracks().row_count();
                                (n > 0).then_some(n as i32)
                            },
                        };
                        open_add_to_mixtape(weak.clone(), handle.clone(), vec![item]);
                    } else {
                        let runtime = runtime.clone();
                        let weak = weak.clone();
                        let handle2 = handle.clone();
                        let album_id = id.clone();
                        handle.spawn(async move {
                            let item = match runtime.core().get_album(&album_id).await {
                                Ok(album) => {
                                    let artwork_url = album
                                        .image
                                        .thumbnail
                                        .clone()
                                        .or_else(|| album.image.small.clone());
                                    let year = album
                                        .release_date_original
                                        .as_deref()
                                        .and_then(|d| d.get(0..4))
                                        .and_then(|y| y.parse::<i32>().ok());
                                    let track_count = album
                                        .tracks_count
                                        .or(album.track_count)
                                        .map(|n| n as i32);
                                    myqbz_add::AddItem {
                                        item_type: "album".into(),
                                        source: "qobuz".into(),
                                        source_item_id: album.id.clone(),
                                        title: album.title.clone(),
                                        subtitle: {
                                            let a = album.artist.name.clone();
                                            (!a.is_empty()).then_some(a)
                                        },
                                        artwork_url,
                                        year,
                                        track_count,
                                    }
                                }
                                Err(e) => {
                                    log::warn!(
                                        "[qbz-slint] add-to-mixtape: get_album {album_id} failed: {e}"
                                    );
                                    return;
                                }
                            };
                            open_add_to_mixtape(weak, handle2, vec![item]);
                        });
                    }
                }
                ("album", "radio") => playback::play_album_radio(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id.clone(),
                ),
                ("album", "favorite") => {
                    // Add the album to the user's favorites. The album cards
                    // (grid hover heart + the "…" menu) all bubble this; the
                    // shared component means one handler covers the app.
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let album_id = id.clone();
                    handle.spawn(async move {
                        match runtime.core().add_favorite("album", &album_id).await {
                            Ok(()) => {
                                // Keep the favorite-album cache in sync so the
                                // album-header heart reflects a grid favorite.
                                crate::fav_cache::set_album(&album_id, true);
                                crate::toast::success_weak(&weak, "Added to favorites");
                                // reco: log the album favorite (add-only arm).
                                let aid = album_id.clone();
                                tokio::task::spawn_blocking(move || {
                                    crate::reco::log_favorite_album(aid, None)
                                });
                            }
                            Err(e) => {
                                log::error!("[qbz-slint] favorite album failed: {e}");
                                crate::toast::error_weak(&weak, "Couldn't add to favorites");
                            }
                        }
                    });
                }
                ("album", "favorite-toggle") => {
                    // The album-header heart: a TRUE toggle that reflects the
                    // favorite-album cache (the grid "favorite" arm above stays
                    // add-only). Optimistic on the open header, reconciled on
                    // the server result.
                    let Some(w) = weak.upgrade() else {
                        return;
                    };
                    let was_fav = crate::fav_cache::is_album_favorite(&id);
                    let new_state = !was_fav;
                    let st = w.global::<AlbumState>();
                    let is_open = st.get_id() == id.as_str();
                    if is_open {
                        st.set_is_favorite(new_state);
                        st.set_favorite_loading(true);
                    }
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let album_id = id.clone();
                    handle.spawn(async move {
                        let res = if new_state {
                            runtime.core().add_favorite("album", &album_id).await
                        } else {
                            runtime.core().remove_favorite("album", &album_id).await
                        };
                        let ok = res.is_ok();
                        if let Err(e) = &res {
                            log::error!(
                                "[qbz-slint] toggle favorite album {album_id} failed: {e}"
                            );
                        }
                        // reco: log the album favorite ADD on success (skip the
                        // un-favorite). Blocking SQLite off the async path.
                        if ok && new_state {
                            let aid = album_id.clone();
                            tokio::task::spawn_blocking(move || {
                                crate::reco::log_favorite_album(aid, None)
                            });
                        }
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            let st = w.global::<AlbumState>();
                            let open_now = st.get_id() == album_id.as_str();
                            if ok {
                                crate::fav_cache::set_album(&album_id, new_state);
                                if open_now {
                                    st.set_favorite_loading(false);
                                    st.set_is_favorite(new_state);
                                }
                                crate::toast::success(
                                    &w,
                                    if new_state {
                                        "Added to favorites"
                                    } else {
                                        "Removed from favorites"
                                    },
                                );
                            } else {
                                if open_now {
                                    st.set_favorite_loading(false);
                                    st.set_is_favorite(was_fav);
                                }
                                crate::toast::error(&w, "Couldn't update favorites");
                            }
                        });
                    });
                }
                ("album", "cache") => offline_cache::cache_album(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id,
                ),
                ("album", "recache") => offline_cache::redownload_album(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id,
                    // Refresh the WHOLE album (Tauri's "Refresh offline copy"
                    // re-downloads every track, not only the failed ones).
                    false,
                ),
                ("album", "add-to-playlist") => {
                    // Resolve the album's loaded tracks to their Qobuz catalog
                    // ids and open the playlist picker for the whole set
                    // (mirrors Tauri's album → Add to playlist). Local/Plex
                    // albums carry no catalog ids, so the entry no-ops there
                    // (the header menu is a Qobuz surface).
                    let Some(w) = weak.upgrade() else {
                        return;
                    };
                    let ids: Vec<String> = {
                        use slint::Model;
                        w.global::<AlbumState>()
                            .get_tracks()
                            .iter()
                            .map(|t| t.id.to_string())
                            .filter(|s| s.parse::<u64>().is_ok())
                            .collect()
                    };
                    if ids.is_empty() {
                        toast::error(&w, "No tracks to add");
                        return;
                    }
                    playlist_picker::open_multi(&w, &ids, false);
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    handle.spawn(async move {
                        let playlists = playlist_picker::load(&runtime).await;
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            playlist_picker::apply(&w, playlists);
                        });
                    });
                }
                ("album", "share-qobuz") => {
                    share::copy_to_clipboard(share::qobuz_album_url(&id));
                    log::info!("[qbz-slint] copied Qobuz link for album {id}");
                }
                ("album", "share-songlink") => {
                    let source = share::qobuz_album_url(&id);
                    let album = id.clone();
                    handle.spawn(async move {
                        match share::songlink_url(&source).await {
                            Some(url) => {
                                share::copy_to_clipboard(url);
                                log::info!("[qbz-slint] copied Album.link for album {album}");
                            }
                            None => {
                                log::warn!("[qbz-slint] Album.link resolution failed for {album}")
                            }
                        }
                    });
                }
                ("track", "play-next") => {
                    // Source-typed routing — see the ("track","queue") arm
                    // (same seam, insert-next instead of append).
                    if let Some(w) = weak.upgrade() {
                        if snapshot_detail_open(&w) {
                            if let Some(qt) = local_playlist::queue_track_for_row(&id) {
                                if matches!(qt.source.as_deref(), Some("local") | Some("plex")) {
                                    playback::enqueue_queue_tracks(
                                        runtime.clone(),
                                        weak.clone(),
                                        handle.clone(),
                                        vec![qt],
                                        true,
                                    );
                                    return;
                                }
                            }
                        }
                    }
                    if let Ok(track_id) = id.parse::<u64>() {
                        playback::play_track_next(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            track_id,
                        );
                    }
                }
                ("track", "favorite") => {
                    // Offline guard + optimistic toggle + network flip with
                    // rollback — shared with the library-surface favorite
                    // (see toggle_track_favorite).
                    toggle_track_favorite(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id.to_string(),
                    );
                }
                // Offline cache: "download"/"cache" make a track available
                // offline; "uncache" removes the local copy. The row affordance
                // and the context menu both bubble these.
                ("track", "cache") | ("track", "download") => {
                    if let Ok(track_id) = id.parse::<u64>() {
                        offline_cache::cache_track(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            track_id,
                        );
                    }
                }
                ("track", "uncache") => {
                    if let Ok(track_id) = id.parse::<u64>() {
                        offline_cache::remove_cached(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            track_id,
                        );
                    }
                }
                ("track", "recache") => {
                    // "Refresh offline copy" (cached-state menu entry, spec
                    // §3.5): remove + re-download, sequenced.
                    if let Ok(track_id) = id.parse::<u64>() {
                        offline_cache::refresh_cached(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            track_id,
                        );
                    }
                }
                ("track", "remove-from-playlist") => {
                    // Per-row removal on the playlist detail (spec §3.1).
                    // Ownership-gated in the UI (PlaylistState.is-owner —
                    // DELIBERATE: Tauri's available branch renders it
                    // un-gated on followed playlists where the owner-only
                    // API rejects, §1.6.1; we port the intent, not the
                    // hole). One-row ride on the same namespace-split seam
                    // as the bulk removal; the reload re-merges the sidecar.
                    let Some(w) = weak.upgrade() else { return };
                    if w.global::<NavState>().get_view() != ContentView::Playlist {
                        return;
                    }
                    if w.global::<PlaylistState>().get_is_local() {
                        local_playlist::remove_rows_by_ids(
                            &w,
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            image_cache.clone(),
                            vec![id.to_string()],
                        );
                        return;
                    }
                    let pid = w.global::<PlaylistState>().get_id().to_string();
                    let Some(row) = playlist::row_for_id(&id) else {
                        log::warn!("[qbz-slint] remove-from-playlist: row {id} not loaded");
                        return;
                    };
                    if let Ok(pid) = pid.parse::<u64>() {
                        playlist_remove_rows(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            image_cache.clone(),
                            pid,
                            vec![row],
                        );
                    }
                }
                ("track", "create-radio") => playback::play_track_radio(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id.clone(),
                ),
                ("track", "add-to-playlist") => {
                    // Open the global picker for this track + load the
                    // user's playlists. SOURCE-TYPED routing first: this
                    // shared arm also fires for local/Plex rows (local
                    // playlist detail, now-playing), whose ids are NOT
                    // Qobuz catalog ids — the untyped path stored a Plex
                    // row's synthetic 2^40 id as qobuz_track_id (the field
                    // garbage class). Type the ref, or refuse.
                    let Some(w) = weak.upgrade() else {
                        return;
                    };
                    // Only consult the local-playlist queue snapshot while
                    // its detail is the OPEN view — a stale snapshot row id
                    // could collide with a genuine catalog id from a Qobuz
                    // surface (both are small integers). The ONLINE mixed
                    // Qobuz detail shares the snapshot (E11), so its
                    // local/plex rows type their refs the same way.
                    let in_local_detail = snapshot_detail_open(&w);
                    let local_ref: Option<String> = if id.starts_with("plex:") {
                        // Unresolved Plex row in a playlist detail — the id
                        // already carries the rating key.
                        Some(id.to_string())
                    } else if in_local_detail {
                        // Open local-playlist detail row: the queue snapshot
                        // knows its source ("plex:<key>" / "<row id>"; None
                        // for Qobuz rows = catalog flow below).
                        local_playlist::local_picker_ref_for_row(id.as_str())
                    } else {
                        None
                    };
                    if let Some(track_ref) = local_ref {
                        playlist_picker::open_multi(&w, &[track_ref], true);
                    } else if id
                        .parse::<u64>()
                        .is_ok_and(|n| n >= local_library::PLEX_TRACK_ID_FLOOR)
                    {
                        // A synthetic (Plex/ephemeral) id with no resolvable
                        // ref — refuse rather than store a fake Qobuz id.
                        log::warn!(
                            "[qbz-slint] add-to-playlist: unresolvable non-catalog id {id} — refused"
                        );
                        toast::error(&w, "Couldn't resolve this track");
                        return;
                    } else {
                        playlist_picker::open(&w, &id);
                    }
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    handle.spawn(async move {
                        let playlists = playlist_picker::load(&runtime).await;
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            playlist_picker::apply(&w, playlists);
                        });
                    });
                }
                ("track", "add-to-mixtape") => {
                    // The menu only carries the track id; resolve the Qobuz
                    // track (this entry is gated to Qobuz/offline in the menu)
                    // to build the AddToMixtape payload, then open the picker.
                    if let Ok(track_id) = id.parse::<u64>() {
                        let runtime = runtime.clone();
                        let weak = weak.clone();
                        let handle2 = handle.clone();
                        handle.spawn(async move {
                            let item = match runtime.core().get_track(track_id).await {
                                Ok(track) => {
                                    let artist = track
                                        .performer
                                        .as_ref()
                                        .map(|p| p.name.clone())
                                        .unwrap_or_default();
                                    let album = track
                                        .album
                                        .as_ref()
                                        .map(|a| a.title.clone())
                                        .unwrap_or_default();
                                    let subtitle = [artist, album]
                                        .into_iter()
                                        .filter(|s| !s.is_empty())
                                        .collect::<Vec<_>>()
                                        .join(" · ");
                                    let artwork_url = track.album.as_ref().and_then(|a| {
                                        a.image
                                            .thumbnail
                                            .clone()
                                            .or_else(|| a.image.small.clone())
                                    });
                                    myqbz_add::AddItem {
                                        item_type: "track".into(),
                                        source: "qobuz".into(),
                                        source_item_id: track_id.to_string(),
                                        title: track.title.clone(),
                                        subtitle: (!subtitle.is_empty()).then_some(subtitle),
                                        artwork_url,
                                        year: None,
                                        track_count: None,
                                    }
                                }
                                Err(e) => {
                                    log::warn!(
                                        "[qbz-slint] add-to-mixtape: get_track {track_id} failed: {e}"
                                    );
                                    return;
                                }
                            };
                            open_add_to_mixtape(weak, handle2, vec![item]);
                        });
                    }
                }
                ("track", "share-qobuz") => {
                    share::copy_to_clipboard(share::qobuz_track_url(&id));
                    log::info!("[qbz-slint] copied Qobuz link for track {id}");
                }
                ("track", "share-songlink") => {
                    let source = share::qobuz_track_url(&id);
                    let track = id.clone();
                    handle.spawn(async move {
                        match share::songlink_url(&source).await {
                            Some(url) => {
                                share::copy_to_clipboard(url);
                                log::info!("[qbz-slint] copied Song.link for track {track}");
                            }
                            None => log::warn!("[qbz-slint] Song.link resolution failed for {track}"),
                        }
                    });
                }
                ("track", "go-to-album") => {
                    // Playlist-detail local/plex sidecar rows first (owner
                    // improvement — Tauri omits the entries there): their
                    // snapshot ids are library row ids / synthetic Plex ids,
                    // NOT catalog ids, and the snapshot QueueTrack's album_id
                    // already carries the LOCAL navigation key (the same one
                    // the now-playing bar navigates by — group key / Plex
                    // content-hash key). Qobuz + offline-copy rows fall
                    // through to the catalog resolve below (an offline copy's
                    // row id IS its Qobuz id).
                    if let Some(w) = weak.upgrade() {
                        if snapshot_detail_open(&w) {
                            if let Some(qt) = local_playlist::queue_track_for_row(&id) {
                                if matches!(qt.source.as_deref(), Some("local") | Some("plex")) {
                                    match qt.album_id.filter(|k| !k.is_empty()) {
                                        Some(key) => w.invoke_open_album(key.into()),
                                        None => log::debug!(
                                            "[qbz-slint] go-to-album: playlist row {id} has no album key"
                                        ),
                                    }
                                    return;
                                }
                            }
                        }
                    }
                    // The menu only carries the track id — resolve the
                    // track to find its album, then open it.
                    if let Ok(track_id) = id.parse::<u64>() {
                        let runtime = runtime.clone();
                        let weak = weak.clone();
                        handle.spawn(async move {
                            if let Ok(track) = runtime.core().get_track(track_id).await {
                                if let Some(album_id) =
                                    track.album.as_ref().map(|a| a.id.clone())
                                {
                                    let _ = weak.upgrade_in_event_loop(move |w| {
                                        w.invoke_open_album(album_id.into());
                                    });
                                }
                            }
                        });
                    }
                }
                ("track", "go-to-artist") => {
                    // Same local/plex diversion as go-to-album: local/plex
                    // artists have no id, so route by NAME to the LocalLibrary
                    // Artists tab (the open-artist callback's split).
                    if let Some(w) = weak.upgrade() {
                        if snapshot_detail_open(&w) {
                            if let Some(qt) = local_playlist::queue_track_for_row(&id) {
                                if matches!(qt.source.as_deref(), Some("local") | Some("plex")) {
                                    if qt.artist.trim().is_empty() {
                                        log::debug!(
                                            "[qbz-slint] go-to-artist: playlist row {id} has no artist name"
                                        );
                                    } else {
                                        w.invoke_open_artist(qt.artist.into());
                                    }
                                    return;
                                }
                            }
                        }
                    }
                    if let Ok(track_id) = id.parse::<u64>() {
                        let runtime = runtime.clone();
                        let weak = weak.clone();
                        handle.spawn(async move {
                            if let Ok(track) = runtime.core().get_track(track_id).await {
                                if let Some(artist_id) =
                                    track.performer.as_ref().map(|p| p.id)
                                {
                                    let _ = weak.upgrade_in_event_loop(move |w| {
                                        w.invoke_open_artist(artist_id.to_string().into());
                                    });
                                }
                            }
                        });
                    }
                }
                // Clickable artist name (album cards) -> artist page.
                ("artist", "open") => {
                    if let Some(w) = weak.upgrade() {
                        w.invoke_open_artist(id.clone().into());
                    }
                }
                // Clickable album name (track rows) -> album page.
                ("album", "open") => {
                    if let Some(w) = weak.upgrade() {
                        w.invoke_open_album(id.clone().into());
                    }
                }
                // Now-playing context (song-card layers button) -> playlist page.
                ("playlist", "open") => {
                    nav::record(nav::NavEntry::Playlist(id.clone()));
                    navigate_playlist(
                        runtime.clone(),
                        weak.clone(),
                        &handle,
                        image_cache.clone(),
                        id.clone(),
                    );
                }
                // Build Artist Collection — open the Discography Builder for the
                // current artist (the button passes an empty id, so resolve it
                // from ArtistState). Records a history entry then routes.
                ("artist", "build-collection") => {
                    if let Some(w) = weak.upgrade() {
                        let artist_id = if id.is_empty() {
                            w.global::<ArtistState>().get_id().to_string()
                        } else {
                            id.clone()
                        };
                        if !artist_id.is_empty() {
                            nav::record(nav::NavEntry::DiscographyBuilder(artist_id.clone()));
                            navigate_discography_builder(
                                runtime.clone(),
                                weak.clone(),
                                &handle,
                                image_cache.clone(),
                                artist_id,
                            );
                            update_nav_flags(&w);
                        }
                    }
                }
                // Blacklist / Show toggle from the ArtistView overflow
                // menu (and the hidden-artist banner). Resolves the id
                // from the passed value, falling back to ArtistState.id
                // (mirrors build-collection). Reads the name from
                // ArtistState for storage. Optimistic with rollback: flip
                // ArtistState.is-blacklisted immediately, perform the
                // mutation, revert + error-toast on failure. Synchronous
                // on the event-loop thread, so there is no re-entrancy
                // window (a second click can't interleave mid-toggle).
                ("artist", "share") => {
                    let artist_id = if id.is_empty() {
                        weak.upgrade()
                            .map(|w| w.global::<ArtistState>().get_id().to_string())
                            .unwrap_or_default()
                    } else {
                        id.clone()
                    };
                    if !artist_id.is_empty() {
                        share::copy_to_clipboard(share::qobuz_artist_url(&artist_id));
                        if let Some(w) = weak.upgrade() {
                            crate::toast::success(&w, "Link copied");
                        }
                    }
                }
                ("artist", "blacklist-toggle") => {
                    if let Some(w) = weak.upgrade() {
                        let st = w.global::<ArtistState>();
                        let artist_id = if id.is_empty() {
                            st.get_id().to_string()
                        } else {
                            id.clone()
                        };
                        let name = st.get_name().to_string();
                        if let Ok(id_num) = artist_id.parse::<u64>() {
                            let was_blacklisted =
                                crate::artist_blacklist::is_blacklisted(id_num);
                            // Optimistic flip.
                            st.set_is_blacklisted(!was_blacklisted);
                            let res = if was_blacklisted {
                                crate::artist_blacklist::remove(id_num)
                            } else {
                                crate::artist_blacklist::add(
                                    id_num,
                                    &name,
                                    None,
                                )
                            };
                            match res {
                                Ok(()) => {
                                    // Live refresh for the artist page is the
                                    // optimistic ArtistState.is-blacklisted
                                    // flip above (drives the banner + the
                                    // menu Show/Blacklist label). ArtistView
                                    // popular-tracks rows are deliberately
                                    // NOT per-row greyed (T6 scoping — the
                                    // banner is the artist-page surface);
                                    // other open views (search, album,
                                    // favorites) re-stamp on next navigation
                                    // (no global observer).
                                    let msg = if was_blacklisted {
                                        format!("{name} is now visible")
                                    } else {
                                        format!("{name} is now hidden")
                                    };
                                    crate::toast::success_weak(&weak, msg);
                                }
                                Err(e) => {
                                    log::error!(
                                        "[qbz-slint] blacklist toggle failed: {e}"
                                    );
                                    // Rollback the optimistic flip.
                                    st.set_is_blacklisted(was_blacklisted);
                                    crate::toast::error_weak(
                                        &weak,
                                        "Failed to update artist visibility",
                                    );
                                }
                            }
                        }
                    }
                }
                ("artist", "play-top") => playback::play_artist_top_tracks(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id.clone(),
                ),
                // Artist radio uses the smart qbz-radio pool builder
                // (the Qobuz /radio/artist endpoint remains available
                // via playback::play_artist_radio for an alternative).
                ("artist", "radio") => playback::play_smart_artist_radio(
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
                                // reco: log the artist favorite (add-only follow
                                // arm). Qobuz artist id -> u64.
                                if let Ok(aid) = artist_id.parse::<u64>() {
                                    tokio::task::spawn_blocking(move || {
                                        crate::reco::log_favorite_artist(aid)
                                    });
                                }
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
                // === Label landing actions ===============================
                ("label", "follow") => {
                    // Toggle the label favorite, optimistically flipping the
                    // header + any matching More-Labels card.
                    if let Some(w) = weak.upgrade() {
                        let make = !label::label_following_state(&w, &id);
                        label::mark_label_followed(&w, &id, make);
                        let runtime = runtime.clone();
                        let weak = weak.clone();
                        let label_id = id.clone();
                        handle.spawn(async move {
                            let res = if make {
                                runtime.core().add_favorite("label", &label_id).await
                            } else {
                                runtime.core().remove_favorite("label", &label_id).await
                            };
                            if let Err(e) = res {
                                log::error!("[qbz-slint] toggle label favorite failed: {e}");
                                let _ = weak.upgrade_in_event_loop(move |w| {
                                    label::mark_label_followed(&w, &label_id, !make);
                                });
                            }
                        });
                    }
                }
                ("label", "play-top") => {
                    // Popular tracks are cached on the UI thread by
                    // apply_label_page; read them here (UI thread) + queue.
                    let tracks = label::top_tracks_for_play();
                    if tracks.is_empty() {
                        crate::toast::error_weak(&weak, "No popular tracks for this label");
                    } else {
                        playback::play_tracks(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            tracks,
                            0,
                        );
                    }
                }
                // More-Labels card click -> open that label's landing.
                ("label", "open") => {
                    if let Ok(label_id) = id.parse::<u64>() {
                        let name = weak
                            .upgrade()
                            .map(|w| label::more_label_name(&w, &id))
                            .unwrap_or_default();
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
                    }
                }
                // "See all" -> the full releases sub-view for the open label.
                ("label", "see-all-releases") => {
                    if let (Some(w), Ok(label_id)) = (weak.upgrade(), id.parse::<u64>()) {
                        let name = w.global::<LabelState>().get_name().to_string();
                        nav::record(nav::NavEntry::LabelReleases {
                            id: label_id,
                            name: name.clone(),
                        });
                        navigate_label_releases(
                            runtime.clone(),
                            weak.clone(),
                            &handle,
                            image_cache.clone(),
                            label_id,
                            name,
                        );
                        update_nav_flags(&w);
                    }
                }
                // Album sidebar laurel (id present) -> open the award page.
                ("award", "open") => {
                    if !id.is_empty() {
                        let name = weak
                            .upgrade()
                            .map(|w| award::other_award_name(&w, &id))
                            .unwrap_or_default();
                        nav::record(nav::NavEntry::Award {
                            id: id.clone(),
                            name: name.clone(),
                        });
                        navigate_award(
                            runtime.clone(),
                            weak.clone(),
                            &handle,
                            image_cache.clone(),
                            id.clone(),
                            name,
                        );
                        if let Some(w) = weak.upgrade() {
                            update_nav_flags(&w);
                        }
                    }
                }
                // Album sidebar laurel WITHOUT an id -> `id` carries the award
                // NAME; resolve it to an id (cache / explore crawl) then open,
                // or toast awardUnavailable on a miss (1:1 Tauri handleAwardClick).
                ("award", "resolve-open") => {
                    let name = id.clone();
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let image_cache = image_cache.clone();
                    let handle_inner = handle.clone();
                    handle.spawn(async move {
                        match award::resolve_award_id_by_name(&runtime, &name).await {
                            Some(award_id) => {
                                let runtime2 = runtime.clone();
                                let ic = image_cache.clone();
                                let h = handle_inner.clone();
                                let name2 = name.clone();
                                let _ = weak.upgrade_in_event_loop(move |w| {
                                    nav::record(nav::NavEntry::Award {
                                        id: award_id.clone(),
                                        name: name2.clone(),
                                    });
                                    navigate_award(runtime2, w.as_weak(), &h, ic, award_id, name2);
                                    update_nav_flags(&w);
                                });
                            }
                            None => {
                                crate::toast::info_weak(
                                    &weak,
                                    "Award detail not available for this entry.",
                                );
                            }
                        }
                    });
                }
                ("track", "toggle-select") => {
                    // Flip `selected` on the matching row, in whichever
                    // multi-select surface is showing: the playlist detail,
                    // the artist Popular Tracks, or the label Popular Tracks.
                    if let Some(w) = weak.upgrade() {
                        let model = match w.global::<NavState>().get_view() {
                            ContentView::Album => w.global::<AlbumState>().get_tracks(),
                            ContentView::Playlist => w.global::<PlaylistState>().get_tracks(),
                            ContentView::Label => w.global::<LabelState>().get_top_tracks(),
                            ContentView::Favorites => {
                                w.global::<FavoritesState>().get_tracks_visible()
                            }
                            _ => w.global::<ArtistState>().get_top_tracks(),
                        };
                        if let Some(vm) = model
                            .as_any()
                            .downcast_ref::<slint::VecModel<TrackItem>>()
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
                        match w.global::<NavState>().get_view() {
                            ContentView::Album => album::recount_selected(&w),
                            ContentView::Artist => artist::recount_selected(&w),
                            ContentView::Playlist => playlist::recount_selected(&w),
                            ContentView::Favorites => favorites::recount_selected(&w),
                            _ => {}
                        }
                    }
                }
                // The mix tile sends id = mix kind, action = "open".
                ("mix", "open") => {
                    nav::record(nav::NavEntry::Mix { kind: id.clone() });
                    navigate_mix(
                        runtime.clone(),
                        weak.clone(),
                        &handle,
                        image_cache.clone(),
                        id.clone(),
                    );
                    if let Some(w) = weak.upgrade() {
                        update_nav_flags(&w);
                    }
                }
                ("mix", "play-all") => {
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    handle.clone().spawn(async move {
                        let tracks = mix::current_tracks();
                        playback::play_tracks(runtime, weak, handle, tracks, 0);
                    });
                }
                ("mix", "shuffle") => {
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    handle.clone().spawn(async move {
                        let tracks = mix::shuffled_tracks();
                        playback::play_tracks(runtime, weak, handle, tracks, 0);
                    });
                }
                ("mix", "refresh") => {
                    // Re-load the current mix (re-fetch its tracks).
                    if let Some(w) = weak.upgrade() {
                        let kind = w.global::<MixState>().get_kind().to_string();
                        if !kind.is_empty() {
                            navigate_mix(
                                runtime.clone(),
                                weak.clone(),
                                &handle,
                                image_cache.clone(),
                                kind,
                            );
                        }
                    }
                }
                ("playlist", "cache") => {
                    if let Ok(pid) = id.parse::<u64>() {
                        offline_cache::cache_playlist(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            pid,
                        );
                    }
                }
                ("playlist", "play") => {
                    // Play a playlist by id NOW (replace the queue), from any
                    // playlist CARD overlay / context menu (Discover qobuzPlaylists,
                    // Search, Label) where no PlaylistView is open. The `play-all`
                    // arm below reads the open detail's PlaylistState, so it cannot
                    // serve a cold card play — this fetches the playlist by id.
                    playback::play_playlist(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id.clone(),
                    );
                }
                ("playlist", "play-all") => {
                    // LOCAL playlist detail — its own queue snapshot +
                    // offline-only stamp (D8); the offline sidecar view of
                    // a MIXED playlist (D11.a) AND the ONLINE mixed detail
                    // (Seam B: source-aware merged queue) share that
                    // snapshot; the pure-Qobuz path is unchanged below.
                    if let Some(w) = weak.upgrade() {
                        let ps = w.global::<PlaylistState>();
                        if ps.get_is_local()
                            || ps.get_offline_subset()
                            || playlist::is_mixed()
                        {
                            local_playlist::play_all(
                                &w,
                                runtime.clone(),
                                weak.clone(),
                                handle.clone(),
                                false,
                            );
                            return;
                        }
                    }
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    handle.clone().spawn(async move {
                        let tracks = playlist::current_tracks();
                        playback::play_tracks(runtime, weak, handle, tracks, 0);
                    });
                }
                ("playlist", "shuffle") => {
                    // Mixed pool shuffles as ONE list, local/plex rows as
                    // equals (E9); the context stays the playlist id.
                    if let Some(w) = weak.upgrade() {
                        let ps = w.global::<PlaylistState>();
                        if ps.get_is_local()
                            || ps.get_offline_subset()
                            || playlist::is_mixed()
                        {
                            local_playlist::play_all(
                                &w,
                                runtime.clone(),
                                weak.clone(),
                                handle.clone(),
                                true,
                            );
                            return;
                        }
                    }
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    handle.clone().spawn(async move {
                        let tracks = playlist::shuffled_tracks();
                        playback::play_tracks(runtime, weak, handle, tracks, 0);
                    });
                }
                ("playlist", "queue") => {
                    if local_playlist::is_local_id(&id) {
                        local_playlist::enqueue_by_id(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            id,
                            false,
                        );
                        return;
                    }
                    playback::enqueue_playlist(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id,
                        false,
                    )
                }
                ("playlist", "play-next") => {
                    if local_playlist::is_local_id(&id) {
                        local_playlist::enqueue_by_id(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            id,
                            true,
                        );
                        return;
                    }
                    playback::enqueue_playlist(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id,
                        true,
                    )
                }
                ("playlist", "upload-to-qobuz") => {
                    // D8: convert a non-offline-only LOCAL playlist into a
                    // real Qobuz playlist (explicit user action, confirmed
                    // in the detail view — nothing ever auto-syncs).
                    if local_playlist::is_local_id(&id) {
                        local_playlist::upload_to_qobuz(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            image_cache.clone(),
                            id,
                        );
                    }
                }
                ("playlist", "favorite") => {
                    // Favorite/unfavorite the open playlist.
                    if let Some(w) = weak.upgrade() {
                        let pid = w.global::<PlaylistState>().get_id().to_string();
                        // LOCAL playlists have no Qobuz favorite — the UI
                        // hides the button; guard the call path too (the
                        // favorite endpoint takes the id as a string, so a
                        // "local:" id COULD otherwise leak through).
                        if local_playlist::is_local_id(&pid) {
                            return;
                        }
                        let was_fav = w.global::<PlaylistState>().get_is_favorite();
                        w.global::<PlaylistState>().set_is_favorite(!was_fav);
                        let runtime = runtime.clone();
                        handle.spawn(async move {
                            let res = if was_fav {
                                runtime.core().remove_favorite("playlist", &pid).await
                            } else {
                                runtime.core().add_favorite("playlist", &pid).await
                            };
                            if let Err(e) = res {
                                log::error!("[qbz-slint] toggle playlist favorite failed: {e}");
                            }
                        });
                    }
                }
                ("playlist", "select-toggle") => {
                    if let Some(w) = weak.upgrade() {
                        let on = w.global::<PlaylistState>().get_multi_select_mode();
                        playlist::set_multi_select(&w, !on);
                    }
                }
                ("playlist", "select-all") => {
                    if let Some(w) = weak.upgrade() {
                        playlist::select_all(&w);
                    }
                }
                ("playlist", "play-next-selected") | ("playlist", "queue-selected") => {
                    // Bulk Play next / Add to queue over the selection
                    // (Tauri's BulkActionBar split-button, spec §1.5) —
                    // source-aware: rows resolve through the merged queue
                    // snapshot (local/plex/cached keep their source — the
                    // T2 fix-forward) or the pure-Qobuz Track cache.
                    if let Some(w) = weak.upgrade() {
                        let next = action == "play-next-selected";
                        let tracks = playlist::selected_queue_tracks(&w);
                        if tracks.is_empty() {
                            toast::error(&w, "Nothing playable in the selection");
                            return;
                        }
                        // Selection clears, mode stays on (LL precedent).
                        playlist::clear_selection(&w);
                        playback::enqueue_queue_tracks(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            tracks,
                            next,
                        );
                    }
                }
                ("playlist", "add-selected-to-playlist") => {
                    // Bulk Add to playlist (spec §1.5). The picker is
                    // single-mode (catalog ids XOR local-mode refs), so:
                    // Qobuz rows ride the catalog flow; a selection with NO
                    // Qobuz rows rides the local-mode flow ("plex:<key>" /
                    // library row ids — per-row parity for sidecar rows); a
                    // MIXED selection follows Tauri (Qobuz rows only,
                    // sidecar rows skipped + logged).
                    let Some(w) = weak.upgrade() else { return };
                    let rows = playlist::selected_rows(&w);
                    if rows.is_empty() {
                        return;
                    }
                    let mut qobuz_ids: Vec<String> = Vec::new();
                    let mut local_refs: Vec<String> = Vec::new();
                    for row in &rows {
                        match row.source.as_str() {
                            "local" => local_refs.push(row.id.clone()),
                            "plex" => {
                                if row.id.starts_with("plex:") {
                                    local_refs.push(row.id.clone());
                                } else if let Some(key) =
                                    local_playlist::plex_key_for_row(&row.id)
                                {
                                    local_refs.push(format!("plex:{key}"));
                                } else {
                                    log::warn!(
                                        "[qbz-slint] bulk add-to-playlist: no rating key for plex row {} — skipped",
                                        row.id
                                    );
                                }
                            }
                            _ => {
                                if row.id.parse::<u64>().is_ok() {
                                    qobuz_ids.push(row.id.clone());
                                }
                            }
                        }
                    }
                    if !qobuz_ids.is_empty() {
                        if !local_refs.is_empty() {
                            log::info!(
                                "[qbz-slint] bulk add-to-playlist: mixed selection — {} sidecar row(s) skipped (single-mode picker; Tauri §1.5 behavior)",
                                local_refs.len()
                            );
                        }
                        playlist_picker::open_multi(&w, &qobuz_ids, false);
                    } else if !local_refs.is_empty() {
                        playlist_picker::open_multi(&w, &local_refs, true);
                    } else {
                        return;
                    }
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    handle.spawn(async move {
                        let playlists = playlist_picker::load(&runtime).await;
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            playlist_picker::apply(&w, playlists);
                        });
                    });
                }
                ("playlist", "remove-selected") => {
                    if let Some(w) = weak.upgrade() {
                        // LOCAL playlist — remove the selected rows from the
                        // library.db repo by stored position.
                        if w.global::<PlaylistState>().get_is_local() {
                            local_playlist::remove_selected(
                                &w,
                                runtime.clone(),
                                weak.clone(),
                                handle.clone(),
                                image_cache.clone(),
                            );
                            return;
                        }
                        // QOBUZ detail (pure or mixed): split by row
                        // namespace — qobuz rows resolve to ptids, local
                        // rows to the local sidecar delete, plex rows to
                        // the plex sidecar delete (Seam D).
                        let pid = w.global::<PlaylistState>().get_id().to_string();
                        let rows = playlist::selected_rows(&w);
                        if let (Ok(pid), false) = (pid.parse::<u64>(), rows.is_empty()) {
                            playlist_remove_rows(
                                runtime.clone(),
                                weak.clone(),
                                handle.clone(),
                                image_cache.clone(),
                                pid,
                                rows,
                            );
                        }
                    }
                }
                ("playlist", "set-artwork") => {
                    // Pick an image, copy it into the artwork cache, store
                    // it as the playlist's custom cover, then reload.
                    if let Some(w) = weak.upgrade() {
                        let pid = w.global::<PlaylistState>().get_id().to_string();
                        // LOCAL playlist — same flow, repo-backed.
                        if local_playlist::is_local_id(&pid) {
                            let runtime = runtime.clone();
                            let weak = weak.clone();
                            let handle = handle.clone();
                            let image_cache = image_cache.clone();
                            handle.clone().spawn(async move {
                                let Some(file) = rfd::AsyncFileDialog::new()
                                    .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                                    .pick_file()
                                    .await
                                else {
                                    return;
                                };
                                let src = file.path().to_string_lossy().into_owned();
                                let lid = pid.clone();
                                let ok = tokio::task::spawn_blocking(move || {
                                    local_playlist::set_custom_artwork_blocking(&lid, &src)
                                        .is_some()
                                })
                                .await
                                .unwrap_or(false);
                                if ok {
                                    local_playlist::navigate(
                                        runtime, weak, &handle, image_cache, pid,
                                    );
                                }
                            });
                            return;
                        }
                        if let Ok(pid) = pid.parse::<u64>() {
                            let runtime = runtime.clone();
                            let weak = weak.clone();
                            let handle = handle.clone();
                            let image_cache = image_cache.clone();
                            handle.clone().spawn(async move {
                                let Some(file) = rfd::AsyncFileDialog::new()
                                    .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                                    .pick_file()
                                    .await
                                else {
                                    return;
                                };
                                let src = file.path().to_string_lossy().into_owned();
                                let ok = tokio::task::spawn_blocking(move || {
                                    playlist::set_custom_artwork(pid, &src).is_some()
                                })
                                .await
                                .unwrap_or(false);
                                if ok {
                                    navigate_playlist(
                                        runtime, weak, &handle, image_cache, pid.to_string(),
                                    );
                                }
                            });
                        }
                    }
                }
                ("playlist", "clear-artwork") => {
                    if let Some(w) = weak.upgrade() {
                        let pid = w.global::<PlaylistState>().get_id().to_string();
                        // LOCAL playlist — clear the repo column + reload.
                        if local_playlist::is_local_id(&pid) {
                            let runtime = runtime.clone();
                            let weak = weak.clone();
                            let handle = handle.clone();
                            let image_cache = image_cache.clone();
                            handle.clone().spawn(async move {
                                let lid = pid.clone();
                                tokio::task::spawn_blocking(move || {
                                    local_playlist::clear_custom_artwork_blocking(&lid);
                                })
                                .await
                                .ok();
                                local_playlist::navigate(
                                    runtime, weak, &handle, image_cache, pid,
                                );
                            });
                            return;
                        }
                        if let Ok(pid) = pid.parse::<u64>() {
                            let runtime = runtime.clone();
                            let weak = weak.clone();
                            let handle = handle.clone();
                            let image_cache = image_cache.clone();
                            handle.clone().spawn(async move {
                                tokio::task::spawn_blocking(move || {
                                    playlist::clear_custom_artwork(pid);
                                })
                                .await
                                .ok();
                                navigate_playlist(
                                    runtime, weak, &handle, image_cache, pid.to_string(),
                                );
                            });
                        }
                    }
                }
                ("playlist", "edit") => {
                    // Open the edit modal, prefilled from the open playlist.
                    if let Some(w) = weak.upgrade() {
                        let ps = w.global::<PlaylistState>();
                        let pid = ps.get_id();
                        let name = ps.get_name();
                        let desc = ps.get_description();
                        let is_local = ps.get_is_local();
                        let offline_only = ps.get_offline_only();
                        let es = w.global::<EditPlaylistState>();
                        es.set_id(pid);
                        es.set_name(name);
                        es.set_description(desc);
                        es.set_is_local(is_local);
                        es.set_offline_only(offline_only);
                        es.set_open(true);
                    }
                }
                ("track", "move-up") | ("track", "move-down") => {
                    // Custom-order reorder (playlist view). Optimistic UI
                    // move, then persist the full order off-thread.
                    if let Some(w) = weak.upgrade() {
                        let up = action == "move-up";
                        let pid = w.global::<PlaylistState>().get_id().to_string();
                        // LOCAL playlist (B2): the move writes the repo's
                        // position order directly (no custom-order sidecar).
                        if local_playlist::is_local_id(&pid) {
                            local_playlist::move_row(&w, &handle, id.as_str(), up);
                        } else {
                            let orders = playlist::move_track(&w, id.as_str(), up);
                            if !orders.is_empty() {
                                if let Ok(pid) = pid.parse::<u64>() {
                                    handle.spawn(async move {
                                        tokio::task::spawn_blocking(move || {
                                            playlist::persist_custom(pid, orders);
                                        })
                                        .await
                                        .ok();
                                    });
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        });
    }

    // Qobuz Connect — initialize the service singleton and wire the bar's
    // connect/disconnect toggle. The session loop + renderer engine inherit the
    // shared qconnect-app hardening (watchdog/takeover/resync). Transport
    // `*_if_remote` routing + device picker land in the QConnect UI-polish step.
    {
        let svc = qconnect_service::init_service(app_runtime.clone(), window.as_weak());
        // D5 (offline-MODE): force-disconnect any live session on every
        // transition into offline (induced or real).
        svc.spawn_offline_force_disconnect(tokio_rt.handle());
        let handle = tokio_rt.handle().clone();
        let weak = window.as_weak();
        window
            .global::<NowPlayingState>()
            .on_qconnect_toggle(move || {
                let Some(svc) = qconnect_service::service() else {
                    return;
                };
                let weak = weak.clone();
                handle.spawn(async move {
                    let connected = if svc.is_running().await {
                        if let Err(err) = svc.disconnect().await {
                            log::warn!("[QConnect] disconnect failed: {err}");
                        }
                        false
                    } else {
                        match svc.connect().await {
                            Ok(()) => true,
                            Err(err) => {
                                log::warn!("[QConnect] connect failed: {err}");
                                false
                            }
                        }
                    };
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        w.global::<NowPlayingState>()
                            .set_qconnect_connected(connected);
                    });
                });
            });
    }
    // Cast (Chromecast / DLNA) — picker discovery + connect/disconnect. Distinct
    // from QConnect above; routes by source, never QConnect admission.
    {
        let _cast = cast_service::init_service(app_runtime.clone(), window.as_weak());
        let handle = tokio_rt.handle().clone();
        window.global::<CastActions>().on_open(move || {
            let Some(svc) = cast_service::service() else {
                return;
            };
            handle.spawn(async move {
                svc.start_discovery().await;
            });
        });
    }
    {
        let handle = tokio_rt.handle().clone();
        window.global::<CastActions>().on_close(move || {
            let Some(svc) = cast_service::service() else {
                return;
            };
            handle.spawn(async move {
                svc.stop_discovery().await;
            });
        });
    }
    {
        let handle = tokio_rt.handle().clone();
        window.global::<CastActions>().on_refresh(move || {
            let Some(svc) = cast_service::service() else {
                return;
            };
            // start_discovery re-arms the scan window + refresh loop.
            handle.spawn(async move {
                svc.start_discovery().await;
            });
        });
    }
    {
        let handle = tokio_rt.handle().clone();
        let weak = window.as_weak();
        window
            .global::<CastActions>()
            .on_connect(move |device_id, protocol| {
                let Some(svc) = cast_service::service() else {
                    return;
                };
                let weak = weak.clone();
                let id = device_id.to_string();
                let proto = protocol.to_string();
                handle.spawn(async move {
                    if let Err(e) = svc.connect(id, proto).await {
                        log::warn!("[Cast] connect failed: {e}");
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            use slint::ComponentHandle;
                            w.global::<CastState>().set_error(e.into());
                        });
                    }
                });
            });
    }
    {
        let handle = tokio_rt.handle().clone();
        window.global::<CastActions>().on_disconnect(move || {
            let Some(svc) = cast_service::service() else {
                return;
            };
            handle.spawn(async move {
                svc.disconnect().await;
            });
        });
    }
    {
        let weak = window.as_weak();
        window
            .global::<QconnectDevState>()
            .on_clear(move || {
                qconnect_service::dev_clear(&weak);
            });
    }
    // Device picker — switch the active renderer (or pull playback back to QBZ
    // via the local id = "Play here"). The topology refresh arrives on the next
    // session event and re-renders the picker + is-remote state.
    {
        let handle = tokio_rt.handle().clone();
        let weak = window.as_weak();
        window
            .global::<QconnectDevState>()
            .on_set_active(move |renderer_id| {
                let Some(svc) = qconnect_service::service() else {
                    return;
                };
                let weak = weak.clone();
                handle.spawn(async move {
                    if let Err(e) = svc.set_active_renderer(renderer_id).await {
                        log::warn!("[QConnect] set_active_renderer({renderer_id}): {e}");
                        crate::toast::error_weak(&weak, "Failed to switch renderer");
                    }
                });
            });
    }

    // Transport — wired through the NowPlayingState global callbacks.
    //
    // QConnect CONTROLLER gating: each callback first tries the remote handoff
    // (`*_if_remote`). When a PEER renderer is active the command is forwarded
    // to it and `Ok(true)` short-circuits. In EVERY non-controller situation
    // (disconnected, RENDERER mode where active==local, or no active renderer)
    // the remote method returns `Ok(false)` and the existing local `playback::*`
    // call runs unchanged — see qconnect_service.rs §safety. This cannot regress
    // renderer/local playback because the gate is `is_peer_renderer_active`.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_toggle_play(move || {
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    // Cast takes priority over QConnect over local.
                    if let Some(cast) = cast_service::service() {
                        match cast.toggle_play_if_cast().await {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(e) => {
                                log::warn!("[Cast] toggle_play: {e}");
                                return;
                            }
                        }
                    }
                    if let Some(svc) = qconnect_service::service() {
                        match svc.toggle_remote_renderer_playback_if_active().await {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(e) => {
                                log::warn!("[QConnect] toggle_play handoff: {e}");
                                return;
                            }
                        }
                    }
                    playback::toggle_play_pause(runtime, weak, handle);
                });
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<NowPlayingState>().on_next(move || {
            let runtime = runtime.clone();
            let weak = weak.clone();
            let handle = handle.clone();
            handle.clone().spawn(async move {
                // NOTE: no cast-specific branch here. While casting, the local
                // next() flow runs — it moves the core cursor, refreshes the
                // now-playing card + queue, and calls play_audible, which casts
                // the new current track (the play_audible cast gate). Routing
                // next() through a cast-only path would advance the renderer but
                // leave the UI cursor stale (and then queue-click resolves
                // against the wrong index).
                if let Some(svc) = qconnect_service::service() {
                    match svc.skip_next_if_remote().await {
                        Ok(true) => return,
                        Ok(false) => {}
                        Err(e) => {
                            log::warn!("[QConnect] next handoff: {e}");
                            return;
                        }
                    }
                }
                playback::next(runtime, weak, handle);
            });
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<NowPlayingState>().on_previous(move || {
            let runtime = runtime.clone();
            let weak = weak.clone();
            let handle = handle.clone();
            handle.clone().spawn(async move {
                // See on_next: no cast branch — the local previous() flow keeps
                // the cursor + UI in sync and play_audible casts the new track.
                if let Some(svc) = qconnect_service::service() {
                    match svc.skip_previous_if_remote().await {
                        Ok(true) => return,
                        Ok(false) => {}
                        Err(e) => {
                            log::warn!("[QConnect] previous handoff: {e}");
                            return;
                        }
                    }
                }
                playback::previous(runtime, weak, handle);
            });
        });
    }
    {
        let runtime = app_runtime.clone();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_seek(move |fraction| {
                let runtime = runtime.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    if let Some(cast) = cast_service::service() {
                        let frac = fraction.clamp(0.0, 1.0) as f64;
                        let dur = runtime.core().get_playback_state().duration as f64;
                        match cast.seek_if_cast(frac * dur).await {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(e) => {
                                log::warn!("[Cast] seek: {e}");
                                return;
                            }
                        }
                    }
                    if let Some(svc) = qconnect_service::service() {
                        // Remote API wants absolute position in ms; the bar gives
                        // a 0..1 fraction. Derive ms from the locally-known
                        // duration (seconds). Until Slice 4 reflects the peer's
                        // duration on the bar, this is the local track's duration
                        // (acceptable interim).
                        let fraction = fraction.clamp(0.0, 1.0);
                        let duration_secs = runtime.core().get_playback_state().duration;
                        let position_ms =
                            (fraction as f64 * duration_secs as f64 * 1000.0).round() as i64;
                        match svc.set_position_if_remote(position_ms).await {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(e) => {
                                log::warn!("[QConnect] seek handoff: {e}");
                                return;
                            }
                        }
                    }
                    playback::seek(runtime, handle, fraction);
                });
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_set_volume(move |fraction| {
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    if let Some(cast) = cast_service::service() {
                        match cast.set_volume_if_cast(fraction.clamp(0.0, 1.0)).await {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(e) => {
                                log::warn!("[Cast] set_volume: {e}");
                                return;
                            }
                        }
                    }
                    if let Some(svc) = qconnect_service::service() {
                        // Remote API wants 0..100; the bar gives a 0..1 fraction.
                        let volume = (fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
                        match svc.set_volume_if_remote(volume).await {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(e) => {
                                log::warn!("[QConnect] set_volume handoff: {e}");
                                return;
                            }
                        }
                    }
                    playback::set_volume(runtime, weak, handle, fraction);
                });
            });
    }
    // Persist sidebar state / section-nav placement / volume (drag-end only)
    // to ui_prefs. These callbacks just touch the prefs file — no runtime.
    {
        let shell = window.global::<ShellState>();
        shell.on_persist_sidebar_state(|state| {
            let mut prefs = crate::ui_prefs::load();
            prefs.sidebar_state = state;
            crate::ui_prefs::save(&prefs);
        });
        shell.on_persist_nav(|enabled| {
            let mut prefs = crate::ui_prefs::load();
            prefs.nav_in_sidebar = enabled;
            crate::ui_prefs::save(&prefs);
        });
        window.global::<NowPlayingState>().on_persist_volume(|fraction| {
            let mut prefs = crate::ui_prefs::load();
            prefs.volume = fraction.clamp(0.0, 1.0);
            crate::ui_prefs::save(&prefs);
        });
        // Remember the last SAFE top-level view for "where you left off".
        let weak = window.as_weak();
        shell.on_persist_view(move || {
            let Some(w) = weak.upgrade() else { return };
            if let Some(key) = safe_view_key(w.global::<NavState>().get_view()) {
                let mut prefs = crate::ui_prefs::load();
                if prefs.last_view != key {
                    prefs.last_view = key.to_string();
                    crate::ui_prefs::save(&prefs);
                }
            }
        });
    }

    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_toggle_mute(move || {
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    if let Some(svc) = qconnect_service::service() {
                        // Remote API wants the target value; send the negation of
                        // the authoritative local MUTED flag.
                        let target = !playback::is_muted();
                        match svc.mute_if_remote(target).await {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(e) => {
                                log::warn!("[QConnect] toggle_mute handoff: {e}");
                                return;
                            }
                        }
                    }
                    playback::toggle_mute(runtime, weak, handle);
                });
            });
    }

    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_toggle_shuffle(move || {
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    if let Some(svc) = qconnect_service::service() {
                        match svc.toggle_shuffle_if_remote().await {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(e) => {
                                log::warn!("[QConnect] toggle_shuffle handoff: {e}");
                                return;
                            }
                        }
                    }
                    playback::toggle_shuffle(runtime, weak, handle);
                });
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<NowPlayingState>()
            .on_cycle_repeat(move || {
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    if let Some(svc) = qconnect_service::service() {
                        match svc.cycle_repeat_if_remote().await {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(e) => {
                                log::warn!("[QConnect] cycle_repeat handoff: {e}");
                                return;
                            }
                        }
                    }
                    playback::cycle_repeat(runtime, weak, handle);
                });
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
            qs.on_play_coverflow_upcoming(move |index| {
                c.play_coverflow_upcoming(index.max(0) as usize)
            });
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
            qs.on_reorder(move |from, to| {
                c.reorder(from.max(0) as usize, to.max(0) as usize);
            });
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

    // Lyrics panel open (conditional mount, ADR-010): re-request lyrics for
    // the current track — a no-op while still loaded (duplicate-fetch guard),
    // a cache-served fetch otherwise (Tauri parity: load-while-open lands
    // immediately, lyricsStore.ts:386-389).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<LyricsState>().on_panel_opened(move || {
            // Immediate sync pass for an already-loaded doc, so opening
            // mid-song lands on the correct line instantly (even paused) —
            // the duplicate-fetch guard below skips the re-fetch then.
            lyrics_sync::kick();
            let runtime = runtime.clone();
            let weak = weak.clone();
            handle.spawn(async move {
                let state = runtime.core().get_queue_state().await;
                match state.current_track {
                    Some(track) => lyrics::on_track_changed(weak, &track),
                    None => lyrics::on_track_cleared(weak),
                }
            });
        });
    }

    // S5 lyrics controls flyout + prefs + settings cache row.
    {
        // Persist any flyout mutation (the flyout writes the in-out props
        // directly for live preview, then fires prefs-changed).
        let weak = window.as_weak();
        window.global::<LyricsState>().on_prefs_changed(move || {
            if let Some(w) = weak.upgrade() {
                lyrics_prefs::persist_from_ui(&w);
            }
        });
    }
    {
        // Reset to the Tauri defaults + persist (flyout footer).
        let weak = window.as_weak();
        window.global::<LyricsState>().on_reset_prefs(move || {
            if let Some(w) = weak.upgrade() {
                lyrics_prefs::reset(&w);
            }
        });
    }
    {
        // Copy the current lyrics to the clipboard (flyout footer).
        let weak = window.as_weak();
        window.global::<LyricsState>().on_copy_lyrics(move || {
            lyrics::copy_current_lyrics(&weak);
        });
    }
    {
        // Settings > Offline lyrics-cache row: stats refresh on section
        // mount + clear action (F1: stats from the real per-user DB).
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<LyricsState>().on_cache_refresh(move || {
            lyrics::refresh_cache_stats(&handle, weak.clone());
        });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<LyricsState>().on_cache_clear(move || {
            lyrics::clear_cache(&handle, weak.clone());
        });
    }

    // S4 lyrics sync engine — a UI-thread `slint::Timer` driving
    // `LyricsState.active-index` / `line-progress` at ~30Hz while the panel
    // is open + the doc is synced + playback is live (idle gate polling
    // otherwise). Position: local ms getter, or the published QConnect peer
    // anchor while controlling a remote renderer (Q7).
    lyrics_sync::start(app_runtime.clone(), window.as_weak());

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

    // Album multi-select: the toolbar toggle next to the search box.
    {
        let weak = window.as_weak();
        window
            .global::<AlbumActions>()
            .on_toggle_multi_select(move || {
                if let Some(w) = weak.upgrade() {
                    let on = w.global::<AlbumState>().get_multi_select();
                    album::set_multi_select(&w, !on);
                }
            });
    }

    // Album multi-select bulk bar — actions over the selected catalog rows.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<AlbumActions>()
            .on_bulk_action(move |action| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                match action.as_str() {
                    "select-all" => album::select_all(&w),
                    "clear" => album::clear_selection(&w),
                    "queue" => {
                        let tracks = album::selected_play_tracks(&w);
                        if !tracks.is_empty() {
                            playback::enqueue_tracks(
                                runtime.clone(),
                                handle.clone(),
                                tracks,
                                false,
                            );
                        }
                    }
                    "play-next" => {
                        let tracks = album::selected_play_tracks(&w);
                        if !tracks.is_empty() {
                            playback::enqueue_tracks(
                                runtime.clone(),
                                handle.clone(),
                                tracks,
                                true,
                            );
                        }
                    }
                    "make-offline" => {
                        let tracks = album::selected_play_tracks(&w);
                        if !tracks.is_empty() {
                            offline_cache::cache_tracks(
                                runtime.clone(),
                                weak.clone(),
                                handle.clone(),
                                tracks,
                            );
                            album::clear_selection(&w);
                        }
                    }
                    "add-to-playlist" => {
                        let ids = album::selected_ids(&w);
                        if !ids.is_empty() {
                            playlist_picker::open_multi(&w, &ids, false);
                            let runtime = runtime.clone();
                            let weak = weak.clone();
                            handle.spawn(async move {
                                let playlists = playlist_picker::load(&runtime).await;
                                let _ = weak.upgrade_in_event_loop(move |w| {
                                    playlist_picker::apply(&w, playlists);
                                });
                            });
                        }
                    }
                    "add-to-favorites" => {
                        let ids = album::selected_ids(&w);
                        if ids.is_empty() {
                            return;
                        }
                        let runtime = runtime.clone();
                        let weak = weak.clone();
                        handle.spawn(async move {
                            for id in &ids {
                                match runtime.core().add_favorite("track", id).await {
                                    Ok(()) => {
                                        if let Ok(tid) = id.parse::<u64>() {
                                            crate::fav_cache::set(tid, true);
                                        }
                                    }
                                    Err(e) => log::error!(
                                        "[qbz-slint] bulk favorite track {id} failed: {e}"
                                    ),
                                }
                            }
                            let _ = weak.upgrade_in_event_loop(|w| {
                                album::clear_selection(&w);
                                crate::toast::success(&w, "Added to favorites");
                            });
                        });
                    }
                    _ => {}
                }
            });
    }

    // Per-disc "Disc N" header ⋯ menu (Qobuz album) — each action is scoped to
    // that disc's tracks only, resolved from the album's stashed raw catalog
    // tracks. Reuses the SAME queue ops as the album-header buttons (play_tracks
    // / play_album_shuffled's xorshift / enqueue_tracks), just over the disc
    // subset rather than the whole album.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<AlbumActions>()
            .on_disc_action(move |disc, action| {
                let mut tracks = album::disc_play_tracks(disc);
                if tracks.is_empty() {
                    return;
                }
                match action.as_str() {
                    "play" => {
                        playback::play_tracks(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            tracks,
                            0,
                        );
                    }
                    "shuffle" => {
                        // Same SystemTime-seeded xorshift Fisher-Yates as the
                        // album-header Shuffle (playback::play_album_shuffled),
                        // applied to the disc subset before play_tracks.
                        let mut seed = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_nanos() as u64)
                            .unwrap_or(1)
                            | 1;
                        for i in (1..tracks.len()).rev() {
                            seed ^= seed << 13;
                            seed ^= seed >> 7;
                            seed ^= seed << 17;
                            let j = (seed % (i as u64 + 1)) as usize;
                            tracks.swap(i, j);
                        }
                        playback::play_tracks(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            tracks,
                            0,
                        );
                    }
                    "queue" => {
                        playback::enqueue_tracks(
                            runtime.clone(),
                            handle.clone(),
                            tracks,
                            false,
                        );
                    }
                    "play-next" => {
                        playback::enqueue_tracks(
                            runtime.clone(),
                            handle.clone(),
                            tracks,
                            true,
                        );
                    }
                    other => {
                        log::warn!("[qbz-slint] album disc-action: unknown action {other}");
                    }
                }
            });
    }

    // Album external-database links (Last.fm / Discogs / MusicBrainz) — open
    // the prebuilt url (AlbumState.*-url) in the system browser. Mirrors the
    // ArtworkActions open-in-browser handler.
    window
        .global::<AlbumActions>()
        .on_open_external_link(|url| {
            if url.is_empty() {
                return;
            }
            if let Err(e) = open::that(url.as_str()) {
                log::error!("[qbz-slint] album external link open failed: {e}");
            }
        });

    // Booklet reader removed — the album booklet button now downloads the PDF
    // (booklet::download_booklet via the ("album","booklet") media action). The
    // BookletActions/BookletState globals + AlbumBookletModal.slint are unused
    // now (left in place; remove in a UI cleanup pass that recompiles qbz-ui).

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

    // Artist per-section sort (persisted by release_type).
    {
        let weak = window.as_weak();
        window
            .global::<ArtistActions>()
            .on_set_section_sort(move |rt, sort| {
                if let Some(w) = weak.upgrade() {
                    artist::resort_section(&w, rt.as_str(), sort.as_str());
                }
            });
    }

    // Artist per-section load-more (capped to 4 pages; reuses get_releases_grid).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<ArtistActions>()
            .on_load_more_section(move |rt| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let release_type = rt.to_string();
                if !artist::section_can_load_more(&release_type) {
                    return;
                }
                let artist_id = w.global::<ArtistState>().get_id().to_string();
                if artist_id.is_empty() {
                    return;
                }
                let offset = artist::section_loaded_count(&w, &release_type) as u32;
                let runtime = runtime.clone();
                let weak2 = weak.clone();
                let image_cache = image_cache.clone();
                handle.spawn(async move {
                    match artist::load_release_page(&runtime, &artist_id, &release_type, offset)
                        .await
                    {
                        Ok((cards, has_more)) => {
                            let image_cache = image_cache.clone();
                            let rt2 = release_type.clone();
                            let _ = weak2.upgrade_in_event_loop(move |w| {
                                let jobs =
                                    artist::append_release_page(&w, &rt2, cards, has_more);
                                artwork::spawn_loads(jobs, w.as_weak(), image_cache);
                            });
                        }
                        Err(e) => {
                            log::warn!("[qbz-slint] load-more {release_type} failed: {e}")
                        }
                    }
                });
            });
    }

    // Artist "See discography" — open the dedicated releases page pre-filtered.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<ArtistActions>()
            .on_open_releases(move |rt| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let artist_id = w.global::<ArtistState>().get_id().to_string();
                let artist_name = w.global::<ArtistState>().get_name().to_string();
                if artist_id.is_empty() {
                    return;
                }
                let release_type = rt.to_string();
                nav::record(nav::NavEntry::ArtistReleases {
                    id: artist_id.clone(),
                    name: artist_name.clone(),
                    release_type: release_type.clone(),
                });
                navigate_artist_releases(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    artist_id,
                    artist_name,
                    release_type,
                );
                update_nav_flags(&w);
            });
    }

    // Dedicated discography page — infinite load-more.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<ArtistReleasesActions>()
            .on_load_more(move || {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let st = w.global::<ArtistReleasesState>();
                if st.get_load_more_loading() || !st.get_has_more() {
                    return;
                }
                let artist_id = st.get_id().to_string();
                let release_type = st.get_release_type().to_string();
                if artist_id.is_empty() {
                    return;
                }
                let offset = artist_releases::loaded_count(&w);
                st.set_load_more_loading(true);
                let runtime = runtime.clone();
                let weak2 = weak.clone();
                let image_cache = image_cache.clone();
                handle.spawn(async move {
                    match artist::load_release_page(&runtime, &artist_id, &release_type, offset)
                        .await
                    {
                        Ok((cards, has_more)) => {
                            let image_cache = image_cache.clone();
                            let _ = weak2.upgrade_in_event_loop(move |w| {
                                let jobs = artist_releases::apply_page(&w, cards, has_more, false);
                                artwork::spawn_loads(jobs, w.as_weak(), image_cache);
                            });
                        }
                        Err(e) => {
                            log::warn!("[qbz-slint] artist releases load-more failed: {e}");
                            let _ = weak2.upgrade_in_event_loop(|w| {
                                w.global::<ArtistReleasesState>().set_load_more_loading(false);
                            });
                        }
                    }
                });
            });
    }

    // Dedicated discography page — sort change (persisted, shared with index).
    {
        let weak = window.as_weak();
        window
            .global::<ArtistReleasesActions>()
            .on_set_sort(move |sort| {
                if let Some(w) = weak.upgrade() {
                    artist_releases::resort(&w, sort.as_str());
                }
            });
    }

    // Dedicated discography page — retry after a failed load.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<ArtistReleasesActions>()
            .on_retry(move || {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let st = w.global::<ArtistReleasesState>();
                let artist_id = st.get_id().to_string();
                let name = st.get_name().to_string();
                let release_type = st.get_release_type().to_string();
                if artist_id.is_empty() {
                    return;
                }
                navigate_artist_releases(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    artist_id,
                    name,
                    release_type,
                );
            });
    }

    // Artist Popular Tracks multi-select — the section toggle.
    {
        let weak = window.as_weak();
        window
            .global::<ArtistActions>()
            .on_toggle_top_tracks_select(move || {
                if let Some(w) = weak.upgrade() {
                    let on = w.global::<ArtistState>().get_top_tracks_multi_select();
                    artist::set_multi_select(&w, !on);
                }
            });
    }

    // Artist Popular Tracks bulk bar — actions over the selected rows.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<ArtistActions>()
            .on_top_tracks_bulk_action(move |action| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let artist_id = w.global::<ArtistState>().get_id().to_string();
                match action.as_str() {
                    "select-all" => artist::select_all(&w),
                    "clear" => artist::clear_selection(&w),
                    "play-next" => playback::enqueue_artist_top_selected(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        artist_id,
                        artist::selected_ids(&w),
                        true,
                    ),
                    "queue" => playback::enqueue_artist_top_selected(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        artist_id,
                        artist::selected_ids(&w),
                        false,
                    ),
                    "add-to-playlist" => {
                        let ids = artist::selected_ids(&w);
                        if !ids.is_empty() {
                            playlist_picker::open_multi(&w, &ids, false);
                            let runtime = runtime.clone();
                            let weak = weak.clone();
                            handle.spawn(async move {
                                let playlists = playlist_picker::load(&runtime).await;
                                let _ = weak.upgrade_in_event_loop(move |w| {
                                    playlist_picker::apply(&w, playlists);
                                });
                            });
                        }
                    }
                    "add-to-favorites" => {
                        let ids = artist::selected_ids(&w);
                        if ids.is_empty() {
                            return;
                        }
                        let runtime = runtime.clone();
                        let weak = weak.clone();
                        handle.spawn(async move {
                            for id in &ids {
                                match runtime.core().add_favorite("track", id).await {
                                    Ok(()) => {
                                        if let Ok(tid) = id.parse::<u64>() {
                                            crate::fav_cache::set(tid, true);
                                        }
                                    }
                                    Err(e) => log::error!(
                                        "[qbz-slint] bulk favorite track {id} failed: {e}"
                                    ),
                                }
                            }
                            let _ = weak.upgrade_in_event_loop(|w| {
                                artist::clear_selection(&w);
                                crate::toast::success(&w, "Added to favorites");
                            });
                        });
                    }
                    _ => {}
                }
            });
    }

    // Artist Popular Tracks section "more" menu — all-tracks actions.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<ArtistActions>()
            .on_top_tracks_menu_action(move |action| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let artist_id = w.global::<ArtistState>().get_id().to_string();
                if artist_id.is_empty() {
                    return;
                }
                match action.as_str() {
                    "next-all" => playback::enqueue_artist_top_selected(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        artist_id,
                        artist::all_top_track_ids(&w),
                        true,
                    ),
                    "queue-all" => playback::enqueue_artist_top_selected(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        artist_id,
                        artist::all_top_track_ids(&w),
                        false,
                    ),
                    "shuffle-all" => playback::play_artist_top_shuffled(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        artist_id,
                    ),
                    "playlist-all" => {
                        let ids = artist::all_top_track_ids(&w);
                        if !ids.is_empty() {
                            playlist_picker::open_multi(&w, &ids, false);
                            let runtime = runtime.clone();
                            let weak = weak.clone();
                            handle.spawn(async move {
                                let playlists = playlist_picker::load(&runtime).await;
                                let _ = weak.upgrade_in_event_loop(move |w| {
                                    playlist_picker::apply(&w, playlists);
                                });
                            });
                        }
                    }
                    _ => {}
                }
            });
    }

    // Artist network sidebar — no persistence. Default open, user can
    // close per-session, and reset_network_sidebar re-applies the open
    // state on every artist navigation (open unless the content area is
    // space-constrained — see reset_network_sidebar). The toggle
    // callback stays a no-op on the Rust side — Slint already flips
    // NetworkSidebarState.open directly in the click handler.
    window
        .global::<NetworkSidebarActions>()
        .on_toggle(|| {});

    // Network sidebar — typed click callbacks. Each delivers the
    // minimum payload the future target views (ArtistsByLocation,
    // LabelReleases, MusicianPage) will need. Logged-only until those
    // views land in Slint.
    // Location click — open ArtistsByLocationView using the cached
    // location params from the Origin metadata (area, genres, tags).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<NetworkSidebarActions>()
            .on_location_clicked(move |mbid| {
                let Some(params) = artist::location_params() else {
                    log::warn!(
                        "[qbz-slint] location clicked but no cached params (mbid={mbid})"
                    );
                    return;
                };
                nav::record(nav::NavEntry::Location {
                    mbid: params.mbid.clone(),
                    area_id: params.area_id.clone(),
                    area_name: params.area_name.clone(),
                    country: params.country.clone(),
                    genres: params.genres.clone(),
                    tags: params.tags.clone(),
                });
                navigate_location(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    params,
                );
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
            });
    }
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

    // Track Info + Album Info modal actions (close / tab / navigation / play).
    // Navigation reuses the same handlers the rest of the app uses (open-artist
    // callback, network-sidebar musician resolve, navigate_label).
    {
        let runtime = app_runtime.clone();
        // -- Track Info --
        let weak = window.as_weak();
        window
            .global::<TrackInfoActions>()
            .on_close(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<TrackInfoState>().set_open(false);
                }
            });
        let weak = window.as_weak();
        window
            .global::<TrackInfoActions>()
            .on_open_artist(move |artist_id| {
                if let Some(w) = weak.upgrade() {
                    w.global::<TrackInfoState>().set_open(false);
                    w.invoke_open_artist(artist_id);
                }
            });
        let weak = window.as_weak();
        let runtime_l = runtime.clone();
        let handle_l = tokio_rt.handle().clone();
        let image_cache_l = image_cache.clone();
        window
            .global::<TrackInfoActions>()
            .on_open_label(move |label_id| {
                if let Some(w) = weak.upgrade() {
                    let name = w.global::<TrackInfoState>().get_label().to_string();
                    w.global::<TrackInfoState>().set_open(false);
                    if let Ok(id) = label_id.parse::<u64>() {
                        navigate_label(
                            runtime_l.clone(),
                            w.as_weak(),
                            &handle_l,
                            image_cache_l.clone(),
                            id,
                            name,
                        );
                    }
                }
            });
        let weak = window.as_weak();
        window
            .global::<TrackInfoActions>()
            .on_open_musician(move |name, role| {
                if let Some(w) = weak.upgrade() {
                    w.global::<TrackInfoState>().set_open(false);
                    w.global::<NetworkSidebarActions>()
                        .invoke_musician_clicked(name, role);
                }
            });
        // Immersive split Track Info panel: populate TrackInfoState for the
        // given track WITHOUT opening the floating modal (open stays false).
        let weak = window.as_weak();
        let runtime_l = runtime.clone();
        let handle_l = tokio_rt.handle().clone();
        window
            .global::<TrackInfoActions>()
            .on_load_inline(move |track_id| {
                if let Ok(id) = track_id.parse::<u64>() {
                    info_modals::load_track_info_inline(
                        runtime_l.clone(),
                        weak.clone(),
                        handle_l.clone(),
                        id,
                    );
                }
            });

        // -- Album Info --
        let weak = window.as_weak();
        window
            .global::<AlbumInfoActions>()
            .on_close(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<AlbumInfoState>().set_open(false);
                }
            });
        let weak = window.as_weak();
        window
            .global::<AlbumInfoActions>()
            .on_set_tab(move |tab| {
                if let Some(w) = weak.upgrade() {
                    w.global::<AlbumInfoState>().set_active_tab(tab);
                }
            });
        let weak = window.as_weak();
        let runtime_p = runtime.clone();
        let handle_p = tokio_rt.handle().clone();
        window
            .global::<AlbumInfoActions>()
            .on_play_track(move |id| {
                if let Some(w) = weak.upgrade() {
                    // Album view is the modal's context, so this plays the
                    // album starting at the chosen track (Tauri keeps the
                    // modal open on play).
                    playback::play_track_in_context(
                        &w,
                        runtime_p.clone(),
                        w.as_weak(),
                        handle_p.clone(),
                        &id,
                    );
                }
            });
        let weak = window.as_weak();
        let runtime_a = runtime.clone();
        let handle_a = tokio_rt.handle().clone();
        let image_cache_a = image_cache.clone();
        window
            .global::<AlbumInfoActions>()
            .on_open_label(move |label_id| {
                if let Some(w) = weak.upgrade() {
                    let name = w.global::<AlbumInfoState>().get_label().to_string();
                    w.global::<AlbumInfoState>().set_open(false);
                    if let Ok(id) = label_id.parse::<u64>() {
                        navigate_label(
                            runtime_a.clone(),
                            w.as_weak(),
                            &handle_a,
                            image_cache_a.clone(),
                            id,
                            name,
                        );
                    }
                }
            });
        let weak = window.as_weak();
        window
            .global::<AlbumInfoActions>()
            .on_open_musician(move |name, role| {
                if let Some(w) = weak.upgrade() {
                    w.global::<AlbumInfoState>().set_open(false);
                    w.global::<NetworkSidebarActions>()
                        .invoke_musician_clicked(name, role);
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
                        Ok((data, total, has_more)) => {
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
                                label::append_albums(&w, data, total, has_more);
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

    // Label releases sub-view toolbar — sort / Hi-Res filter /
    // group-by-artist / search. The markup updates the bound LabelState
    // property first; each callback just re-derives the rendered list
    // (local filter over the loaded catalog).
    {
        let weak = window.as_weak();
        window.global::<LabelActions>().on_set_sort(move |_| {
            if let Some(w) = weak.upgrade() {
                label::derive_releases(&w);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<LabelActions>().on_set_hires(move |_| {
            if let Some(w) = weak.upgrade() {
                label::derive_releases(&w);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<LabelActions>().on_set_group(move |_| {
            if let Some(w) = weak.upgrade() {
                label::derive_releases(&w);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<LabelActions>().on_search(move |_| {
            if let Some(w) = weak.upgrade() {
                label::derive_releases(&w);
            }
        });
    }

    // Award pages actions (AwardView landing + AwardAlbumsView listing).
    {
        // Header follow heart — optimistic flip + three-state toggling, then
        // the API write (singular favType "award") + local cache mirror. On
        // error, revert. skip_if_remote: a remote Qobuz Connect session must
        // not fire favorite I/O while controlling another device.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<AwardActions>()
            .on_toggle_follow(move |award_id| {
                let award_id = award_id.to_string();
                if award_id.is_empty() {
                    return;
                }
                let runtime = runtime.clone();
                let weak = weak.clone();
                handle.spawn(async move {
                    // skip-if-remote: never fire favorite I/O while a remote
                    // QConnect renderer is being controlled (Tauri's skipIfRemote).
                    if let Some(svc) = crate::qconnect_service::service() {
                        if svc.is_peer_active().await {
                            return;
                        }
                    }
                    // Committed state is the local cache (mirrored on success).
                    // The heart is disabled while toggling, so no double-toggle.
                    let make = !crate::fav_cache::is_award_favorite(&award_id);
                    {
                        let award_id = award_id.clone();
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            award::mark_following(&w, &award_id, make, true);
                        });
                    }
                    // Write API first (singular "award" -> award_ids), then
                    // mirror the local cache, then clear the toggling flag.
                    let res = if make {
                        runtime.core().add_favorite("award", &award_id).await
                    } else {
                        runtime.core().remove_favorite("award", &award_id).await
                    };
                    match res {
                        Ok(_) => {
                            crate::fav_cache::set_award(&award_id, make);
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                award::mark_following(&w, &award_id, make, false);
                            });
                        }
                        Err(e) => {
                            log::error!("[qbz-slint] toggle award favorite failed: {e}");
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                award::mark_following(&w, &award_id, !make, false);
                            });
                        }
                    }
                });
            });
    }
    {
        // Landing "See all" -> the full albums listing for the open award.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<AwardActions>().on_open_albums(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let state = w.global::<AwardState>();
            let id = state.get_id().to_string();
            let name = state.get_name().to_string();
            if id.is_empty() {
                return;
            }
            nav::record(nav::NavEntry::AwardAlbums {
                id: id.clone(),
                name: name.clone(),
            });
            navigate_award_albums(
                runtime.clone(),
                weak.clone(),
                &handle,
                image_cache.clone(),
                id,
                name,
            );
            update_nav_flags(&w);
        });
    }
    {
        // "Other awards" card -> open that award's landing.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<AwardActions>()
            .on_open_award(move |award_id, name| {
                let award_id = award_id.to_string();
                let mut name = name.to_string();
                if award_id.is_empty() {
                    return;
                }
                // The other-awards carousel emits only the id; recover the
                // name from the card for a good history/back-forward label.
                if name.is_empty() {
                    if let Some(w) = weak.upgrade() {
                        name = award::other_award_name(&w, &award_id);
                    }
                }
                nav::record(nav::NavEntry::Award {
                    id: award_id.clone(),
                    name: name.clone(),
                });
                navigate_award(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    award_id,
                    name,
                );
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
            });
    }
    {
        // AwardAlbumsView load-more — append the next /award/getAlbums page.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<AwardActions>().on_load_more(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let state = w.global::<AwardState>();
            let award_id = state.get_id().to_string();
            if award_id.is_empty() {
                return;
            }
            let offset = state.get_albums().row_count() as u32;
            state.set_load_more_loading(true);
            let runtime = runtime.clone();
            let weak = weak.clone();
            let image_cache = image_cache.clone();
            handle.spawn(async move {
                match award::load_more_award_albums(&runtime, &award_id, offset).await {
                    Ok((data, total, has_more)) => {
                        let jobs = award::albums_artwork_jobs(&data, offset as usize);
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            award::append_award_albums(&w, data, total, has_more);
                        });
                        artwork::spawn_loads(jobs, weak, image_cache);
                    }
                    Err(e) => {
                        log::error!("[qbz-slint] award load-more failed: {e}");
                        let _ = weak.upgrade_in_event_loop(|w| {
                            w.global::<AwardState>().set_load_more_loading(false);
                        });
                    }
                }
            });
        });
    }
    {
        // AwardAlbumsView client-side search — re-derive `visible` (UI thread).
        let weak = window.as_weak();
        window.global::<AwardActions>().on_search(move |_| {
            if let Some(w) = weak.upgrade() {
                award::derive_award_albums(&w);
            }
        });
    }
    {
        // Error branch retry — reload whichever award surface is showing.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<AwardActions>().on_retry(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let state = w.global::<AwardState>();
            let id = state.get_id().to_string();
            let name = state.get_name().to_string();
            if id.is_empty() {
                return;
            }
            match w.global::<NavState>().get_view() {
                ContentView::AwardAlbums => navigate_award_albums(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    id,
                    name,
                ),
                _ => navigate_award(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    id,
                    name,
                ),
            }
        });
    }

    // PurchasesView actions (Slice 8 — list surface).
    {
        // select-tab — switch + lazy-load the new tab (cached after first load;
        // switching back does NOT refetch). Sets active-tab THEN loads.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<PurchasesActions>().on_select_tab(move |tab| {
            let Some(w) = weak.upgrade() else { return };
            w.global::<PurchasesState>().set_active_tab(tab.clone());
            // Close any open menu on a tab switch (Svelte closeFormatPicker too).
            w.global::<PurchasesState>().set_open_menu(slint::SharedString::new());
            let pt = if tab == "tracks" {
                purchases::PurchaseTab::Tracks
            } else {
                purchases::PurchaseTab::Albums
            };
            // Tab-switch effect: only loads if NOT search-scoped + not cached.
            if w.global::<PurchasesState>().get_search_query().trim().is_empty() {
                load_purchases_tab(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    image_cache.clone(),
                    pt,
                    false,
                    false,
                );
            } else {
                // Search active: just re-derive the new tab's view from the
                // already-loaded search arrays (no refetch).
                purchases::derive_purchases(&w);
            }
        });
    }
    {
        // search(query) — 300ms debounce in the controller. Empty query → force
        // reload the active tab; else fetch ALL purchases + filter, set both
        // arrays + both loaded.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<PurchasesActions>().on_search(move |query| {
            let query = query.to_string();
            let seq = purchases::next_search_seq();
            let runtime = runtime.clone();
            let weak = weak.clone();
            let handle2 = handle.clone();
            let image_cache = image_cache.clone();
            handle.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                // Debounce: drop a stale fire (a newer keystroke superseded it).
                if !purchases::search_seq_current(seq) {
                    return;
                }
                let trimmed = query.trim().to_string();
                if trimmed.is_empty() {
                    // Empty query → force reload the active tab (clearSearch-ish).
                    purchases::set_search_active(false);
                    let active = purchases_active_tab(&weak);
                    load_purchases_tab(
                        runtime, weak, handle2, image_cache, active, true, false,
                    );
                    return;
                }
                let _ = weak.upgrade_in_event_loop(|w| {
                    purchases::set_loading(&w);
                });
                let metadata = purchases::load_purchases_metadata(&runtime).await;
                match purchases::search_purchases(&runtime, &trimmed).await {
                    Ok((albums, tracks)) => {
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            purchases::apply_purchases_search(&w, albums, tracks, &metadata);
                        });
                        let jobs = purchases::artwork_jobs_for_both();
                        artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
                    }
                    Err(e) => {
                        let display = map_purchases_error_display(&e);
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            purchases::set_load_error(&w, &display);
                        });
                    }
                }
            });
        });
    }
    {
        // clear-search — reset query + force reload only the active tab (the
        // stale-other-tab quirk is preserved by NOT invalidating it).
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<PurchasesActions>().on_clear_search(move || {
            let Some(w) = weak.upgrade() else { return };
            w.global::<PurchasesState>().set_search_query(slint::SharedString::new());
            purchases::set_search_active(false);
            let active = purchases_active_tab(&weak);
            load_purchases_tab(
                runtime.clone(),
                weak.clone(),
                handle.clone(),
                image_cache.clone(),
                active,
                true,
                false,
            );
        });
    }
    {
        // Albums toolbar — group / view-mode / sort. Each writes state + derives.
        let weak = window.as_weak();
        window.global::<PurchasesActions>().on_set_album_group(move |mode| {
            let Some(w) = weak.upgrade() else { return };
            let s = w.global::<PurchasesState>();
            if mode == "off" {
                s.set_album_grouping_enabled(false);
            } else {
                s.set_album_group_mode(mode.clone());
                s.set_album_grouping_enabled(true);
            }
            s.set_open_menu(slint::SharedString::new());
            purchases::derive_purchases(&w);
        });
    }
    {
        let weak = window.as_weak();
        window.global::<PurchasesActions>().on_set_album_view_mode(move |mode| {
            let Some(w) = weak.upgrade() else { return };
            w.global::<PurchasesState>().set_album_view_mode(mode);
            purchases::derive_purchases(&w);
        });
    }
    {
        let weak = window.as_weak();
        window.global::<PurchasesActions>().on_select_album_sort(move |value| {
            let Some(w) = weak.upgrade() else { return };
            let s = w.global::<PurchasesState>();
            let (by, dir) = purchases::next_album_sort(
                &s.get_album_sort_by(),
                &s.get_album_sort_direction(),
                &value,
            );
            s.set_album_sort_by(by.into());
            s.set_album_sort_direction(dir.into());
            s.set_open_menu(slint::SharedString::new());
            purchases::derive_purchases(&w);
        });
    }
    {
        // Tracks toolbar — group dropdown.
        let weak = window.as_weak();
        window.global::<PurchasesActions>().on_set_track_group(move |mode| {
            let Some(w) = weak.upgrade() else { return };
            let s = w.global::<PurchasesState>();
            if mode == "off" {
                s.set_track_grouping_enabled(false);
            } else {
                s.set_track_group_mode(mode.clone());
                s.set_track_grouping_enabled(true);
            }
            s.set_open_menu(slint::SharedString::new());
            purchases::derive_purchases(&w);
        });
    }
    {
        // Filter panel setters. State + derive + persist the per-machine pref so
        // the selection survives restart (mirrors on_appearance_bool).
        let weak = window.as_weak();
        window
            .global::<PurchasesActions>()
            .on_set_filter_hide_unavailable(move |v| {
                let Some(w) = weak.upgrade() else { return };
                w.global::<PurchasesState>().set_filter_hide_unavailable(v);
                let mut prefs = crate::ui_prefs::load();
                prefs.purchases_hide_unavailable = v;
                crate::ui_prefs::save(&prefs);
                purchases::derive_purchases(&w);
            });
    }
    {
        let weak = window.as_weak();
        window.global::<PurchasesActions>().on_set_filter_quality(move |q| {
            let Some(w) = weak.upgrade() else { return };
            w.global::<PurchasesState>().set_filter_quality(q.clone());
            let mut prefs = crate::ui_prefs::load();
            prefs.purchases_quality_filter = q.to_string();
            crate::ui_prefs::save(&prefs);
            purchases::derive_purchases(&w);
        });
    }
    {
        let weak = window.as_weak();
        window
            .global::<PurchasesActions>()
            .on_set_filter_hide_downloaded(move |v| {
                let Some(w) = weak.upgrade() else { return };
                w.global::<PurchasesState>().set_filter_hide_downloaded(v);
                let mut prefs = crate::ui_prefs::load();
                prefs.purchases_hide_downloaded = v;
                crate::ui_prefs::save(&prefs);
                purchases::derive_purchases(&w);
            });
    }
    {
        let weak = window.as_weak();
        window.global::<PurchasesActions>().on_clear_all_filters(move || {
            let Some(w) = weak.upgrade() else { return };
            let s = w.global::<PurchasesState>();
            s.set_filter_hide_unavailable(false);
            s.set_filter_quality("all".into());
            s.set_filter_hide_downloaded(false);
            // Persist the reset so cleared filters survive restart.
            let mut prefs = crate::ui_prefs::load();
            prefs.purchases_hide_unavailable = false;
            prefs.purchases_hide_downloaded = false;
            prefs.purchases_quality_filter = "all".to_string();
            crate::ui_prefs::save(&prefs);
            purchases::derive_purchases(&w);
        });
    }
    {
        // Mutually-exclusive open menu ("" closes). No derive — UI-only.
        let weak = window.as_weak();
        window.global::<PurchasesActions>().on_set_open_menu(move |menu| {
            let Some(w) = weak.upgrade() else { return };
            w.global::<PurchasesState>().set_open_menu(menu);
            // Opening any toolbar dropdown closes a format picker (Svelte).
            w.global::<PurchasesState>().set_picker_track_id(slint::SharedString::new());
        });
    }
    {
        // Region notice dismiss — hide it AND persist the "seen" flag so it does
        // not reappear on the next launch (mirrors Tauri's
        // setUserItem('qbz-purchases-region-notice-seen', 'true')).
        let weak = window.as_weak();
        window.global::<PurchasesActions>().on_dismiss_region_notice(move || {
            let Some(w) = weak.upgrade() else { return };
            w.global::<PurchasesState>().set_show_region_notice(false);
            let mut prefs = crate::ui_prefs::load();
            prefs.purchases_region_notice_seen = true;
            crate::ui_prefs::save(&prefs);
        });
    }
    {
        // open-album / open-artist (navigation lands fully in Slice 11; here we
        // route artist clicks; album-detail clicks emit the view's open-album
        // callback wired in AppShell, so this is a no-op placeholder kept for
        // the Actions surface completeness).
        let weak = window.as_weak();
        window.global::<PurchasesActions>().on_open_artist(move |id| {
            let Some(w) = weak.upgrade() else { return };
            // Reuse the global open-artist path.
            w.invoke_open_artist(id);
        });
    }
    {
        // play-track — per-track row play (§A.2): plays the SINGLE clicked track
        // with NO surrounding album queue and NO playback-context seeding —
        // asymmetric with Play-album (§A.1, which DOES build a full queue). This
        // does NOT route through `media_action("track", …, "play")` /
        // `play_track_in_context` (which would build a queue from the visible
        // tracklist); it goes straight to the single-track primitive
        // `play_track_now`, which streams the catalog track (NOT the downloaded
        // file). Mirrors Svelte `handleDisplayTrackPlay` (+page.svelte:4016-4045).
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<PurchasesActions>().on_play_track(move |id| {
            let Ok(track_id) = id.parse::<u64>() else { return };
            playback::play_track_now(
                runtime.clone(),
                weak.clone(),
                handle.clone(),
                track_id,
            );
        });
    }
    {
        // download-track(track_id, ax, ay) — the tracks-tab per-track flow:
        // resolve formats; 0 → toast; 1 → download directly; >1 → open picker.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PurchasesActions>()
            .on_download_track(move |track_id_s, ax, ay| {
                let Some(w0) = weak.upgrade() else { return };
                let Ok(track_id) = track_id_s.parse::<u64>() else { return };
                let Some(track) = purchases::find_track(track_id) else { return };
                // No album id → noAlbum toast (Svelte §2.1.13).
                let album_id = track.album.as_ref().map(|a| a.id.clone()).unwrap_or_default();
                if album_id.is_empty() {
                    crate::toast::error(&w0, qbz_i18n::t("This track has no album to download"));
                    return;
                }
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle2 = handle.clone();
                handle.spawn(async move {
                    // 1:1 Svelte §2.1.13: a fetch FAILURE (Err) → `downloadFailed`;
                    // a successful-but-EMPTY format list (Ok(empty)) → `noFormats`.
                    let formats = match purchases::get_album_formats(&runtime, &album_id).await {
                        Ok(formats) => formats,
                        Err(_) => {
                            let _ = weak.upgrade_in_event_loop(|w| {
                                crate::toast::error(&w, qbz_i18n::t("Failed to start download. Please try again."));
                            });
                            return;
                        }
                    };
                    if formats.is_empty() {
                        let _ = weak.upgrade_in_event_loop(|w| {
                            crate::toast::error(&w, qbz_i18n::t("No downloadable formats available"));
                        });
                        return;
                    }
                    if formats.len() == 1 {
                        // 1 format → no picker; download directly.
                        let fmt = formats[0].clone();
                        execute_track_download(runtime, weak, handle2, track_id, fmt);
                        return;
                    }
                    // >1 → open the anchored picker. Cache the resolved formats
                    // keyed by track id so `pick-format` resolves the chosen
                    // option's label without a second `get_album` round-trip
                    // (the Svelte original keeps the formats in component state).
                    let items = purchases::format_picker_items(&formats);
                    purchases::cache_picker_formats(track_id, formats);
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        let s = w.global::<PurchasesState>();
                        s.set_picker_formats(slint::ModelRc::new(slint::VecModel::from(items)));
                        s.set_picker_track_id(track_id.to_string().into());
                        s.set_picker_anchor_x(ax);
                        s.set_picker_anchor_y(ay);
                        // Opening the picker closes any toolbar menu.
                        s.set_open_menu(slint::SharedString::new());
                    });
                });
            });
    }
    {
        // pick-format(track_id, format_id) — the user chose a format in the
        // picker: close it, then folder-pick + download.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PurchasesActions>()
            .on_pick_format(move |track_id_s, format_id| {
                let Some(w0) = weak.upgrade() else { return };
                w0.global::<PurchasesState>().set_picker_track_id(slint::SharedString::new());
                let Ok(track_id) = track_id_s.parse::<u64>() else { return };
                let fid = format_id as u32;
                // Resolve the chosen format option (for its label → qualityDir)
                // from the cache `download-track` seeded when it opened the
                // picker — no second `get_album` round-trip. The `_` arm is a
                // defensive fallback if the cache was never seeded.
                let cached = purchases::take_picker_formats(track_id)
                    .and_then(|formats| formats.into_iter().find(|f| f.id == fid));
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle2 = handle.clone();
                if let Some(fmt) = cached {
                    execute_track_download(runtime, weak, handle2, track_id, fmt);
                    return;
                }
                // Fallback: re-fetch (cache miss). Needs the album id.
                let Some(track) = purchases::find_track(track_id) else { return };
                let album_id = track.album.as_ref().map(|a| a.id.clone()).unwrap_or_default();
                if album_id.is_empty() {
                    return;
                }
                handle.spawn(async move {
                    // Cache-miss re-fetch: a fetch failure simply aborts the
                    // download here (the picker was already shown; there is no
                    // Svelte toast on this fallback path — `executeTrackDownload`
                    // just returns when the format can't be resolved).
                    let Ok(formats) = purchases::get_album_formats(&runtime, &album_id).await else {
                        return;
                    };
                    let Some(fmt) = formats.into_iter().find(|f| f.id == fid) else {
                        return;
                    };
                    execute_track_download(runtime, weak, handle2, track_id, fmt);
                });
            });
    }
    {
        // close-format-picker.
        let weak = window.as_weak();
        window.global::<PurchasesActions>().on_close_format_picker(move || {
            let Some(w) = weak.upgrade() else { return };
            let s = w.global::<PurchasesState>();
            if let Ok(track_id) = s.get_picker_track_id().parse::<u64>() {
                purchases::clear_picker_formats(track_id);
            }
            s.set_picker_track_id(slint::SharedString::new());
        });
    }
    {
        // retry — re-fetch the active tab (force). INTENTIONALLY UNWIRED in the
        // UI for 1:1 parity: the Svelte error state renders icon + text with no
        // retry button (PurchasesView.svelte:851-855), so the EmptyBlock has no
        // trigger that fires this. Kept as a seam for a future retry affordance.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<PurchasesActions>().on_retry(move || {
            let active = purchases_active_tab(&weak);
            load_purchases_tab(
                runtime.clone(),
                weak.clone(),
                handle.clone(),
                image_cache.clone(),
                active,
                true,
                false,
            );
        });
    }

    // ── PurchaseDetailView actions (Slice 9 — detail surface) ───────────────
    {
        // load(album-id) — the reactive-reload seam (§A.3). Fired by the view's
        // `changed album-id` handler + first mount. Fetches the detail album +
        // formats and applies them (RAW error on failure).
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<PurchaseDetailActions>()
            .on_load(move |album_id| {
                load_purchase_detail(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    image_cache.clone(),
                    album_id.to_string(),
                );
            });
    }
    {
        // open-artist(id) — reuse the global open-artist path.
        let weak = window.as_weak();
        window
            .global::<PurchaseDetailActions>()
            .on_open_artist(move |id| {
                let Some(w) = weak.upgrade() else { return };
                w.invoke_open_artist(id);
            });
    }
    {
        // play-album — Play-album routing (§A.1): plays via the REGULAR album
        // path, NOT a purchase command and NOT the downloaded local file. The
        // standard `media_action("album", id, "play")` arm routes a Qobuz album
        // id (a purchase album id is always numeric, never a local key) to
        // `playback::play_album`, which fetches the album through the regular
        // `get_album` core call, builds the FULL backend queue, replaces it at
        // index 0, and starts track 0 — the SAME path every other album surface
        // uses. Mirrors Svelte `playAlbumById` (+page.svelte:2404-2450, which
        // calls the 12th `v2_get_album` command + replacePlaybackQueue + playTrack).
        let weak = window.as_weak();
        window.global::<PurchaseDetailActions>().on_play_album(move || {
            let Some(w) = weak.upgrade() else { return };
            let album_id = purchases::detail_album_id();
            if album_id.is_empty() {
                return;
            }
            w.invoke_media_action("album".into(), album_id.into(), "play".into());
        });
    }
    {
        // play-track(id) — per-track single-track play (§A.2): SINGLE track, NO
        // album queue, NO playback-context seeding (asymmetric with Play-album,
        // §A.1). Routes straight to `play_track_now` (single-track primitive,
        // streams the catalog track — NOT the downloaded file), NOT through
        // `media_action("track", …, "play")` / `play_track_in_context` (which
        // would build a queue from the visible tracklist). Mirrors Svelte
        // `handleDisplayTrackPlay` (+page.svelte:4016-4045).
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PurchaseDetailActions>()
            .on_play_track(move |id| {
                let Ok(track_id) = id.parse::<u64>() else { return };
                playback::play_track_now(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    track_id,
                );
            });
    }
    {
        // select-format(index) — update the selected format id + re-derive (a
        // format change re-scopes per-track completion + the dropdown label).
        let weak = window.as_weak();
        window
            .global::<PurchaseDetailActions>()
            .on_select_format(move |index| {
                let Some(w) = weak.upgrade() else { return };
                purchases::select_detail_format(index.max(0) as usize);
                purchases::derive_detail(&w);
            });
    }
    {
        // download-all — folder pick → startAlbumDownload (§2.2.7 'all' branch).
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PurchaseDetailActions>()
            .on_download_all(move || {
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle2 = handle.clone();
                // Guard (Svelte §2.2.7): no album or no selected format → return.
                let album_id = purchases::detail_album_id();
                let Some(fmt_id) = purchases::detail_selected_format_id() else {
                    return;
                };
                let Some(fmt_label) = purchases::detail_selected_format_label() else {
                    return;
                };
                let track_ids = purchases::detail_track_ids();
                if album_id.is_empty() {
                    return;
                }
                handle.spawn(async move {
                    let Some(dest) = purchases::pick_download_folder().await else {
                        return;
                    };
                    let quality_dir = purchases::quality_dir(&fmt_label);
                    purchases::start_album_download(
                        runtime,
                        weak.clone(),
                        handle2,
                        album_id,
                        track_ids,
                        fmt_id,
                        dest,
                        quality_dir,
                    );
                    // Project the seeded progress immediately.
                    let _ = weak.upgrade_in_event_loop(|w| {
                        purchases::refresh_detail_download(&w);
                    });
                });
            });
    }
    {
        // download-track(id) — folder pick → startTrackDownload (§2.2.7 trackId
        // branch). Single-track MERGE into the album state (§A.7).
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PurchaseDetailActions>()
            .on_download_track(move |track_id_s| {
                let Ok(track_id) = track_id_s.parse::<u64>() else { return };
                let album_id = purchases::detail_album_id();
                let Some(fmt_id) = purchases::detail_selected_format_id() else {
                    return;
                };
                let Some(fmt_label) = purchases::detail_selected_format_label() else {
                    return;
                };
                if album_id.is_empty() {
                    return;
                }
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle2 = handle.clone();
                handle.spawn(async move {
                    let Some(dest) = purchases::pick_download_folder().await else {
                        return;
                    };
                    let quality_dir = purchases::quality_dir(&fmt_label);
                    purchases::start_track_download(
                        runtime,
                        weak.clone(),
                        handle2,
                        album_id,
                        track_id,
                        fmt_id,
                        dest,
                        quality_dir,
                    );
                    let _ = weak.upgrade_in_event_loop(|w| {
                        purchases::refresh_detail_download(&w);
                    });
                });
            });
    }
    {
        // cancel-download — set the abort flag (the running track finishes; the
        // rest become cancelled between tracks) + re-derive the progress.
        let weak = window.as_weak();
        window
            .global::<PurchaseDetailActions>()
            .on_cancel_download(move || {
                let Some(w) = weak.upgrade() else { return };
                let album_id = purchases::detail_album_id();
                if album_id.is_empty() {
                    return;
                }
                purchases::cancel_album_download(&album_id);
                purchases::refresh_detail_download(&w);
            });
    }
    {
        // add-to-library — the ONLY non-purchase backend write (§2.2.6). Reads
        // the rewritten album-folder destination from the store, fires the
        // add-folder + scan, clears the download state, then re-derives.
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PurchaseDetailActions>()
            .on_add_to_library(move || {
                let Some(w) = weak.upgrade() else { return };
                let album_id = purchases::detail_album_id();
                let Some(destination) = purchases::detail_destination() else {
                    return;
                };
                if album_id.is_empty() {
                    return;
                }
                // Show the spinner; `handle_add_to_library` holds it true across
                // the whole async add and clears it (plus re-derives the detail)
                // on EVERY exit path — so the spinner stays visible for the real
                // duration of the add (Svelte keeps `addingToLibrary` true across
                // the await), and on success the cleared download state hides the
                // progress + Add-to-Library blocks while on failure only the
                // spinner drops. No premature next-tick clear here.
                purchases::set_detail_adding_to_library(&w, true);
                purchases::handle_add_to_library(
                    weak.clone(),
                    handle.clone(),
                    album_id,
                    destination,
                );
            });
    }

    // Immersive Suggestions panel actions (Checkpoint D — split-panel == 2).
    {
        // load(track-id) — entry + now-playing-change refresh. Reads the
        // artist-id + title off NowPlayingState (the panel only has the track
        // id) and kicks the live artist load (mirror of navigate_award).
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<SuggestionsActions>()
            .on_load(move |track_id| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let np = w.global::<NowPlayingState>();
                let artist_id = np.get_artist_id().to_string();
                let track_id = track_id.to_string();
                let track_name = np.get_title().to_string();
                // Dedup: skip a reload when the panel already shows this artist
                // for this seed track (the changed-watcher can refire on
                // unrelated NowPlayingState churn).
                let ss = w.global::<SuggestionsState>();
                if ss.get_artist_id().as_str() == artist_id
                    && ss.get_seed_track_id().as_str() == track_id
                    && !track_id.is_empty()
                {
                    return;
                }
                navigate_suggestions(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    artist_id,
                    track_id,
                    track_name,
                );
            });
    }
    {
        // play / queue / play-next a curated artist playlist by id — reuse the
        // existing playback seams (same paths the playlist cards use).
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<SuggestionsActions>()
            .on_play_playlist(move |playlist_id| {
                let id = playlist_id.to_string();
                if id.is_empty() {
                    return;
                }
                playback::play_playlist(runtime.clone(), weak.clone(), handle.clone(), id);
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<SuggestionsActions>()
            .on_queue_playlist(move |playlist_id| {
                let id = playlist_id.to_string();
                if id.is_empty() {
                    return;
                }
                playback::enqueue_playlist(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id,
                    false,
                );
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<SuggestionsActions>()
            .on_play_next_playlist(move |playlist_id| {
                let id = playlist_id.to_string();
                if id.is_empty() {
                    return;
                }
                playback::enqueue_playlist(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id,
                    true,
                );
            });
    }
    {
        // start-radio — build the Song Radio off the seed track via core, then
        // start it (set_queue + play) through the shared play_tracks seam. The
        // radio card spinner is flipped on optimistically and cleared on apply.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<SuggestionsActions>()
            .on_start_radio(move |track_id, track_name, artist_id| {
                let (Ok(tid), Ok(aid)) =
                    (track_id.parse::<u64>(), artist_id.parse::<u64>())
                else {
                    return;
                };
                let track_name = track_name.to_string();
                suggestions::set_radio_loading(&weak, true);
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle2 = handle.clone();
                handle.spawn(async move {
                    let result = runtime
                        .core()
                        .create_smart_track_radio(tid, aid, track_name)
                        .await;
                    suggestions::set_radio_loading(&weak, false);
                    match result {
                        Ok(tracks) if !tracks.is_empty() => {
                            playback::play_tracks(runtime, weak, handle2, tracks, 0);
                        }
                        Ok(_) => {
                            log::warn!("[qbz-slint] song radio returned no tracks");
                        }
                        Err(e) => {
                            log::error!("[qbz-slint] song radio failed: {e}");
                        }
                    }
                });
            });
    }
    {
        // play-track — play a single recommended track by id NOW.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<SuggestionsActions>()
            .on_play_track(move |track_id| {
                let Ok(tid) = track_id.parse::<u64>() else {
                    return;
                };
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle2 = handle.clone();
                handle.spawn(async move {
                    match runtime.core().get_track(tid).await {
                        Ok(track) => {
                            playback::play_tracks(runtime, weak, handle2, vec![track], 0);
                        }
                        Err(e) => {
                            log::error!("[qbz-slint] suggestions play-track {tid} failed: {e}");
                        }
                    }
                });
            });
    }

    // Artist Blacklist Manager actions (Task 11). Mutations are synchronous
    // (in-memory set + single SQLite ops via the artist_blacklist wrapper), so
    // no tokio handle is needed; each callback runs on the event-loop thread.
    {
        // open() — the forward-open seam (T10's Settings content-filtering row
        // calls this). Records the nav entry, swaps the view, then loads the
        // blacklist. Mirrors OfflineManagerActions.on_open.
        let weak = window.as_weak();
        window.global::<BlacklistActions>().on_open(move || {
            nav::record(nav::NavEntry::BlacklistManager);
            if let Some(w) = weak.upgrade() {
                w.global::<NavState>()
                    .set_view(ContentView::BlacklistManager);
                update_nav_flags(&w);
            }
            blacklist_manager::load(weak.clone());
        });
    }
    {
        // back() — declared per the spec; the actual back chrome is the shared
        // header NavButtons (which drives nav::go_back). Wired here for any
        // future in-view trigger; routes through the same go-back path.
        let weak = window.as_weak();
        let app_runtime_bl = app_runtime.clone();
        let bl_handle = tokio_rt.handle().clone();
        let image_cache_bl = image_cache.clone();
        window.global::<BlacklistActions>().on_back(move || {
            if let Some((entry, scroll)) = nav::go_back() {
                let weak2 = weak.clone();
                arm_scroll_restore(&weak2, &entry, scroll);
                apply_entry(
                    entry,
                    &app_runtime_bl,
                    &weak2,
                    &bl_handle,
                    &image_cache_bl,
                );
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        let bl_runtime_a = app_runtime.clone();
        let bl_handle_a = tokio_rt.handle().clone();
        let bl_image_cache_a = image_cache.clone();
        window
            .global::<BlacklistActions>()
            .on_artist_select(move |id| {
                let artist_id = id.to_string();
                nav::record(nav::NavEntry::Artist(artist_id.clone()));
                navigate_artist(
                    bl_runtime_a.clone(),
                    weak.clone(),
                    &bl_handle_a,
                    bl_image_cache_a.clone(),
                    artist_id,
                );
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<BlacklistActions>()
            .on_toggle_enabled(move || {
                if let Some(w) = weak.upgrade() {
                    blacklist_manager::toggle_enabled(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window.global::<BlacklistActions>().on_remove(move |id| {
            if let Some(w) = weak.upgrade() {
                blacklist_manager::remove(&w, id);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<BlacklistActions>().on_clear_all(move || {
            if let Some(w) = weak.upgrade() {
                blacklist_manager::clear_all(&w);
            }
        });
    }
    {
        let weak = window.as_weak();
        window
            .global::<BlacklistActions>()
            .on_search_changed(move |q| {
                if let Some(w) = weak.upgrade() {
                    blacklist_manager::search_changed(&w, q.to_string());
                }
            });
    }

    // Offline Cache Manager actions.
    {
        let runtime = app_runtime.clone();
        let handle = tokio_rt.handle().clone();
    {
        let weak = window.as_weak();
        let handle = handle.clone();
        window.global::<OfflineManagerActions>().on_open(move || {
            nav::record(nav::NavEntry::OfflineManager);
            if let Some(w) = weak.upgrade() {
                w.global::<NavState>().set_view(ContentView::OfflineManager);
                update_nav_flags(&w);
            }
            offline_manager::load(weak.clone(), handle.clone());
        });
    }
    {
        let weak = window.as_weak();
        let handle = handle.clone();
        window.global::<OfflineManagerActions>().on_refresh(move || {
            offline_manager::load(weak.clone(), handle.clone());
        });
    }
    {
        let weak = window.as_weak();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_select_artist(move |name| {
                offline_manager::select_artist(weak.clone(), handle.clone(), name.to_string());
            });
    }
    {
        let weak = window.as_weak();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_set_sort(move |i| {
                offline_manager::set_sort(weak.clone(), handle.clone(), i);
            });
    }
    {
        let weak = window.as_weak();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_toggle_failed(move || {
                offline_manager::toggle_failed(weak.clone(), handle.clone());
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<OfflineManagerActions>()
            .on_toggle_select(move |id| {
                if let Some(w) = weak.upgrade() {
                    offline_manager::toggle_select(&w, &id);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<OfflineManagerActions>()
            .on_select_all(move || {
                if let Some(w) = weak.upgrade() {
                    offline_manager::set_all_selected(&w, true);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<OfflineManagerActions>()
            .on_clear_selection(move || {
                if let Some(w) = weak.upgrade() {
                    offline_manager::set_all_selected(&w, false);
                }
            });
    }
    {
        let weak = window.as_weak();
        let runtime = runtime.clone();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_bulk_redownload(move || {
                if let Some(w) = weak.upgrade() {
                    for id in offline_manager::selected_track_ids(&w) {
                        offline_cache::redownload_track(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            id,
                        );
                    }
                }
            });
    }
    {
        let weak = window.as_weak();
        let runtime = runtime.clone();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_bulk_remove(move || {
                if let Some(w) = weak.upgrade() {
                    for id in offline_manager::selected_track_ids(&w) {
                        offline_cache::remove_cached(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            id,
                        );
                    }
                }
            });
    }
    {
        let weak = window.as_weak();
        let runtime = runtime.clone();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_remove_track(move |id| {
                if let Ok(tid) = id.parse::<u64>() {
                    offline_cache::remove_cached(runtime.clone(), weak.clone(), handle.clone(), tid);
                }
            });
    }
    {
        let weak = window.as_weak();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_remove_album(move |aid| {
                offline_cache::remove_album(weak.clone(), handle.clone(), aid.to_string());
            });
    }
    {
        let weak = window.as_weak();
        let runtime = runtime.clone();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_redownload_track(move |id| {
                if let Ok(tid) = id.parse::<u64>() {
                    offline_cache::redownload_track(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        tid,
                    );
                }
            });
    }
    {
        let weak = window.as_weak();
        let runtime = runtime.clone();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_redownload_album(move |aid| {
                offline_cache::redownload_album(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    aid.to_string(),
                    false,
                );
            });
    }
    {
        let weak = window.as_weak();
        let runtime = runtime.clone();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_redownload_failed(move |aid| {
                offline_cache::redownload_album(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    aid.to_string(),
                    true,
                );
            });
    }
    {
        let weak = window.as_weak();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_set_limit(move |gb| {
                offline_manager::set_limit(weak.clone(), handle.clone(), gb);
            });
    }
    {
        let weak = window.as_weak();
        let handle = handle.clone();
        window.global::<OfflineManagerActions>().on_clear_all(move || {
            offline_cache::clear_all(weak.clone(), handle.clone());
        });
    }
    {
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_open_folder(move || {
                offline_cache::open_folder(handle.clone());
            });
    }
    {
        let weak = window.as_weak();
        let runtime = runtime.clone();
        let handle = handle.clone();
        window
            .global::<OfflineManagerActions>()
            .on_play_track(move |id| {
                if let Ok(tid) = id.parse::<u64>() {
                    playback::play_track_now(runtime.clone(), weak.clone(), handle.clone(), tid);
                }
            });
    }
    }

    // Scene (location) view actions — open-artist routes to the
    // artist page, load-more validates the next page of candidates.
    {
        let weak = window.as_weak();
        window
            .global::<LocationViewActions>()
            .on_open_artist(move |id| {
                if id.is_empty() {
                    return;
                }
                if let Some(w) = weak.upgrade() {
                    w.invoke_open_artist(id);
                }
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<LocationViewActions>()
            .on_load_more(move || {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let Some(params) = artist::location_params() else {
                    return;
                };
                let offset = w.global::<LocationViewState>().get_artists().row_count();
                w.global::<LocationViewState>().set_load_more_loading(true);
                let runtime = runtime.clone();
                let weak = weak.clone();
                let image_cache = image_cache.clone();
                handle.spawn(async move {
                    match location_view::load_scene(&runtime, &params, offset).await {
                        Ok(data) => {
                            let jobs: Vec<artwork::ArtworkJob> = data
                                .artists
                                .iter()
                                .enumerate()
                                .filter(|(_, a)| !a.image_url.is_empty())
                                .map(|(i, a)| artwork::ArtworkJob {
                                    url: a.image_url.clone(),
                                    target: artwork::ArtworkTarget::LocationArtist {
                                        index: offset + i,
                                    },
                                })
                                .collect();
                            let total = data.total;
                            let artists = data.artists.clone();
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                location_view::append_scene(&w, artists, total);
                            });
                            artwork::spawn_loads(jobs, weak, image_cache);
                        }
                        Err(e) => {
                            log::error!("[qbz-slint] scene load-more failed: {e}");
                            let _ = weak.upgrade_in_event_loop(|w| {
                                w.global::<LocationViewState>().set_load_more_loading(false);
                            });
                        }
                    }
                });
            });
    }

    // Discover tab switch (the in-view Home / Editor's Picks / For
    // You pill). Swaps the cached section set + re-fires artwork; For
    // You lazy-loads its dedicated sections on first open.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<HomeActions>()
            .on_select_tab(move |tab| {
                if let Some(w) = weak.upgrade() {
                    nav::record(nav::NavEntry::Discover {
                        tab: tab.to_string(),
                    });
                    let jobs = home::select_tab(&w, tab.as_str());
                    artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
                    update_nav_flags(&w);
                    if tab.as_str() == "forYou" {
                        ensure_for_you_loaded(&runtime, &weak, &handle, &image_cache);
                    }
                }
            });
    }

    // Qobuz Playlists category filter (multi-select, client-side). Toggling /
    // clearing a tag re-filters the cached playlists row and re-fires the
    // artwork for the new (filtered) positions — no re-fetch.
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window
            .global::<HomeActions>()
            .on_toggle_playlist_tag(move |slug| {
                if let Some(w) = weak.upgrade() {
                    let jobs = home::toggle_playlist_tag(&w, slug.as_str());
                    artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
                }
            });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window
            .global::<HomeActions>()
            .on_clear_playlist_tags(move || {
                if let Some(w) = weak.upgrade() {
                    let jobs = home::clear_playlist_tags(&w);
                    artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
                }
            });
    }

    // Discover section configurator (Slice 5) — gear opens the modal; toggle /
    // move / reset mutate the per-user prefs, persist, and re-render the active
    // tab from the cache (no refetch). The mutation handlers re-fire artwork for
    // newly-shown Home/Editor album sections, mirroring on_select_tab.
    {
        let weak = window.as_weak();
        window
            .global::<DiscoverActions>()
            .on_open_configurator(move || {
                if let Some(w) = weak.upgrade() {
                    discover_prefs::on_open_configurator(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<DiscoverActions>()
            .on_close_configurator(move || {
                if let Some(w) = weak.upgrade() {
                    discover_prefs::on_close_configurator(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window
            .global::<DiscoverActions>()
            .on_toggle_section(move |tab, id| {
                if let Some(w) = weak.upgrade() {
                    discover_prefs::on_toggle(&w, tab.as_str(), id.as_str(), &image_cache);
                }
            });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window
            .global::<DiscoverActions>()
            .on_move_section(move |tab, id, dir| {
                if let Some(w) = weak.upgrade() {
                    discover_prefs::on_move(&w, tab.as_str(), id.as_str(), dir, &image_cache);
                }
            });
    }
    {
        let weak = window.as_weak();
        let image_cache = image_cache.clone();
        window
            .global::<DiscoverActions>()
            .on_reset_tab(move |tab| {
                if let Some(w) = weak.upgrade() {
                    discover_prefs::on_reset(&w, tab.as_str(), &image_cache);
                }
            });
    }

    // Case-insensitive substring test backing the searchable QbzSelect
    // (Slint 1.16 has no `contains` builtin). Pure + stateless, so a single
    // registration at setup serves every searchable list.
    window
        .global::<TextUtil>()
        .on_contains_ci(|haystack: slint::SharedString, needle: slint::SharedString| {
            haystack
                .to_lowercase()
                .contains(needle.to_lowercase().as_str())
        });

    // Genre filter — selection is per context ("discover" / "favorites").
    // Toggling / clearing re-fetches the discover index (discover context)
    // or re-derives the favorites tab (favorites context).
    {
        let weak = window.as_weak();
        window
            .global::<GenreFilterActions>()
            .on_set_context(move |ctx| {
                genre_filter::set_context(ctx.as_str());
                if let Some(w) = weak.upgrade() {
                    genre_filter::apply_state(&w);
                }
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<GenreFilterActions>()
            .on_toggle(move |id| {
                let was_selected = genre_filter::selected_ids()
                    .iter()
                    .any(|x| x.to_string() == id.as_str());
                if !genre_filter::toggle(id.as_str()) {
                    return;
                }
                let Some(w) = weak.upgrade() else {
                    return;
                };
                genre_filter::apply_state(&w);
                // Favorites: client-side genre filter — re-derive the active
                // favorites tab instead of re-fetching the discover index.
                if genre_filter::current_context() == "favorites" {
                    let runtime_f = runtime.clone();
                    let weak_f = weak.clone();
                    let id_f = id.to_string();
                    handle.spawn(async move {
                        if !was_selected {
                            if let Ok(gid) = id_f.parse::<u64>() {
                                genre_filter::load_descendants(&runtime_f, gid).await;
                            }
                        }
                        let _ = weak_f.upgrade_in_event_loop(|w| {
                            genre_filter::apply_state(&w);
                            if w.global::<FavoritesState>().get_active_tab().as_str() == "albums" {
                                favorites::derive_albums(&w);
                            } else {
                                favorites::derive_tracks(&w);
                            }
                        });
                    });
                    return;
                }
                // When the DiscoverBrowse "View all" page is showing, the
                // genre change re-fetches THAT page; otherwise it reloads
                // the Discover home index.
                let browse_target = current_browse_target(&w);
                if browse_target.is_none() {
                    w.global::<HomeState>().set_loading(true);
                }
                let active = w.global::<HomeState>().get_active_tab().to_string();
                let id = id.to_string();
                let runtime = runtime.clone();
                let weak = weak.clone();
                let image_cache = image_cache.clone();
                let handle2 = handle.clone();
                handle.spawn(async move {
                    // On a newly-selected genre, eager-load its descendants
                    // so filter_ids covers the child genres.
                    if !was_selected {
                        if let Ok(gid) = id.parse::<u64>() {
                            genre_filter::load_descendants(&runtime, gid).await;
                            let _ = weak.upgrade_in_event_loop(|w| {
                                genre_filter::apply_state(&w);
                            });
                        }
                    }
                    if let Some((endpoint, title)) = browse_target {
                        discover_browse::navigate(
                            runtime.clone(),
                            weak.clone(),
                            &handle2,
                            image_cache.clone(),
                            endpoint,
                            title,
                            current_genre_filter(),
                        );
                    } else {
                        reload_home(&runtime, &weak, &image_cache, active).await;
                    }
                });
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<GenreFilterActions>()
            .on_toggle_expand(move |id| {
                let now_expanded = genre_filter::toggle_expand(id.as_str());
                let Some(w) = weak.upgrade() else {
                    return;
                };
                genre_filter::apply_state(&w);
                // Lazy-load the node's children the first time it expands.
                if now_expanded {
                    if let Ok(gid) = id.to_string().parse::<u64>() {
                        if !genre_filter::children_loaded(gid) {
                            let runtime = runtime.clone();
                            let weak = weak.clone();
                            handle.spawn(async move {
                                genre_filter::load_children(&runtime, gid).await;
                                let _ = weak.upgrade_in_event_loop(|w| {
                                    genre_filter::apply_state(&w);
                                });
                            });
                        }
                    }
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<GenreFilterActions>()
            .on_search(move |query| {
                genre_filter::set_search(query.as_str());
                if let Some(w) = weak.upgrade() {
                    genre_filter::apply_state(&w);
                }
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<GenreFilterActions>()
            .on_clear(move || {
                genre_filter::clear();
                let Some(w) = weak.upgrade() else {
                    return;
                };
                genre_filter::apply_state(&w);
                if genre_filter::current_context() == "favorites" {
                    if w.global::<FavoritesState>().get_active_tab().as_str() == "albums" {
                        favorites::derive_albums(&w);
                    } else {
                        favorites::derive_tracks(&w);
                    }
                    return;
                }
                let browse_target = current_browse_target(&w);
                if browse_target.is_none() {
                    w.global::<HomeState>().set_loading(true);
                }
                let active = w.global::<HomeState>().get_active_tab().to_string();
                let runtime = runtime.clone();
                let weak = weak.clone();
                let image_cache = image_cache.clone();
                let handle2 = handle.clone();
                handle.spawn(async move {
                    if let Some((endpoint, title)) = browse_target {
                        discover_browse::navigate(
                            runtime.clone(),
                            weak.clone(),
                            &handle2,
                            image_cache.clone(),
                            endpoint,
                            title,
                            current_genre_filter(),
                        );
                    } else {
                        reload_home(&runtime, &weak, &image_cache, active).await;
                    }
                });
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<GenreFilterActions>()
            .on_set_remember(move |v| {
                genre_filter::set_remember(v);
                if let Some(w) = weak.upgrade() {
                    genre_filter::apply_state(&w);
                }
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<GenreFilterActions>()
            .on_set_advanced(move |v| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                w.global::<GenreFilterState>().set_advanced(v);
                // First time advanced view opens, eager-load every
                // parent's children so the tree shows child counts.
                if v {
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    handle.spawn(async move {
                        genre_filter::load_all_parent_children(&runtime).await;
                        let _ = weak.upgrade_in_event_loop(|w| {
                            genre_filter::apply_state(&w);
                        });
                    });
                }
            });
    }

    // Header nav-menu navigation — currently routes the Library
    // dropdown rows into Library > Favorites tabs.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.on_header_menu_navigate(move |route| {
            if route == "home" {
                if let Some(w) = weak.upgrade() {
                    w.global::<NavState>().set_view(ContentView::Home);
                }
                return;
            }
            // Purchases — the opt-in My-Purchases surface. Direct nav item (no
            // tab dropdown): record history + navigate (loads the active tab) +
            // update_nav_flags, mirroring the My QBZ / Favorites per-route
            // pattern. Active highlight for both `purchases`/`purchase-album`
            // is computed view-side off NavState.view (§5.3).
            if route == "purchases" {
                nav::record(nav::NavEntry::Purchases);
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
                navigate_purchases(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                );
                return;
            }
            // My QBZ — Mixtapes / Collections index grids (read-only slice).
            // Record history + navigate (loads via myqbz::navigate), mirroring
            // the Favorites / Local Library per-route pattern.
            if route == "myqbz-mixtapes" {
                nav::record(nav::NavEntry::Mixtapes);
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
                myqbz::navigate(
                    weak.clone(),
                    handle.clone(),
                    image_cache.clone(),
                    qbz_models::mixtape::CollectionKind::Mixtape,
                );
                return;
            }
            if route == "myqbz-collections" {
                nav::record(nav::NavEntry::Collections);
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
                myqbz::navigate(
                    weak.clone(),
                    handle.clone(),
                    image_cache.clone(),
                    qbz_models::mixtape::CollectionKind::Collection,
                );
                return;
            }
            // Discover tabs — switch to Home and select the tab. The
            // section sets are already cached from the initial load,
            // so this just swaps the visible set + re-fires artwork.
            if let Some(tab) = route.strip_prefix("discover-") {
                let tab = tab.to_string();
                if let Some(w) = weak.upgrade() {
                    nav::record(nav::NavEntry::Discover { tab: tab.clone() });
                    w.global::<NavState>().set_view(ContentView::Home);
                    let jobs = home::select_tab(&w, &tab);
                    artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
                    update_nav_flags(&w);
                    if tab == "forYou" {
                        ensure_for_you_loaded(&runtime, &weak, &handle, &image_cache);
                    }
                }
                return;
            }
            if let Some(tab) = favorites::FavTab::from_route(route.as_str()) {
                let tab_id = route.strip_prefix("favorites-").unwrap_or("tracks");
                nav::record(nav::NavEntry::Favorites {
                    tab: tab_id.to_string(),
                });
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
                navigate_favorites(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    tab,
                    tab_id,
                );
                return;
            }
            // Local Library tabs — same per-tab history pattern as Favorites.
            if let Some(tab) = local_library::LibTab::from_route(route.as_str()) {
                nav::record(nav::NavEntry::LocalLibrary {
                    tab: tab.tab_id().to_string(),
                });
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
                navigate_local_library(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    tab,
                );
            }
        });
    }

    // Local Library — in-view tab bar (select-tab) + the gear button
    // (open-settings -> Settings > Local Library). Same per-tab history
    // pattern as Favorites.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<LocalLibraryActions>()
            .on_select_tab(move |tab_id| {
                if let Some(tab) = local_library::LibTab::from_tab_id(tab_id.as_str()) {
                    nav::record(nav::NavEntry::LocalLibrary {
                        tab: tab.tab_id().to_string(),
                    });
                    if let Some(w) = weak.upgrade() {
                        update_nav_flags(&w);
                    }
                    navigate_local_library(
                        runtime.clone(),
                        weak.clone(),
                        &handle,
                        image_cache.clone(),
                        tab,
                    );
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_open_settings(move || {
                // Management/maintenance/danger live under Settings > Local
                // Library — pre-select that sub-section (index 4). The panel's
                // `init` lazy-loads the folder list.
                nav::record(nav::NavEntry::Settings);
                if let Some(w) = weak.upgrade() {
                    w.global::<SettingsState>().set_section(4);
                    w.global::<NavState>().set_view(ContentView::Settings);
                    update_nav_flags(&w);
                }
            });
    }

    // Settings > Local Library — folder management + maintenance + danger.
    // (Scan callbacks scan-all/scan-folder/stop-scan are wired with Slice B.)
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LibraryManageActions>()
            .on_load(move || local_library_settings::load_folders(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LibraryManageActions>()
            .on_add_folder(move || local_library_settings::add_folder(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LibraryManageActions>()
            .on_remove_folders(move || {
                local_library_settings::remove_folders(weak.clone(), handle.clone())
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LibraryManageActions>()
            .on_remove_folder(move |id| {
                local_library_settings::remove_folder(weak.clone(), handle.clone(), id as i64)
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LibraryManageActions>()
            .on_toggle_folder_select(move |id| {
                local_library_settings::toggle_select(weak.clone(), id as i64)
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LibraryManageActions>()
            .on_edit_folder(move |id| {
                local_library_settings::edit_folder(weak.clone(), handle.clone(), id as i64)
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LibraryManageActions>()
            .on_save_folder_settings(move |id, alias, enabled, is_network, fs_type, user_override| {
                local_library_settings::save_folder_settings(
                    weak.clone(),
                    handle.clone(),
                    id as i64,
                    alias.to_string(),
                    enabled,
                    is_network,
                    fs_type.to_string(),
                    user_override,
                )
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LibraryManageActions>()
            .on_change_folder_path(move |id| {
                local_library_settings::change_folder_path(weak.clone(), handle.clone(), id as i64)
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LibraryManageActions>()
            .on_cleanup_missing(move || {
                local_library_settings::cleanup_missing(weak.clone(), handle.clone())
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LibraryManageActions>()
            .on_clear_library(move || {
                local_library_settings::clear_library(weak.clone(), handle.clone())
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LibraryManageActions>()
            .on_set_filter(move |_q| local_library_settings::set_filter(weak.clone()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LibraryManageActions>()
            .on_scan_all(move || local_library_settings::scan_all(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LibraryManageActions>()
            .on_scan_folder(move |id| {
                local_library_settings::scan_folder(weak.clone(), handle.clone(), id as i64)
            });
    }
    {
        window
            .global::<LibraryManageActions>()
            .on_stop_scan(move || local_library_settings::stop_scan());
    }

    // Settings > Plex — connection + library selection (LAN-only). The PIN
    // poll, ping, library sync, and disconnect/clear-cache all live in
    // `plex_auth`; the persisted store is the per-user `plex_settings.db`.
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlexAuthActions>()
            .on_load(move || plex_auth::load(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlexAuthActions>()
            .on_enable_toggle(move |b| plex_auth::enable_toggle(weak.clone(), handle.clone(), b));
    }
    {
        window
            .global::<PlexAuthActions>()
            .on_collapse_toggle(move |b| plex_auth::collapse_toggle(b));
    }
    {
        let weak = window.as_weak();
        window
            .global::<PlexAuthActions>()
            .on_set_server_url(move |url| plex_auth::set_server_url(weak.clone(), url.to_string()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlexAuthActions>()
            .on_generate_code(move || plex_auth::generate_code(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlexAuthActions>()
            .on_open_auth_url(move || plex_auth::open_auth_url(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        window
            .global::<PlexAuthActions>()
            .on_copy_code(move || plex_auth::copy_code(weak.clone()));
    }
    {
        window
            .global::<PlexAuthActions>()
            .on_manual_token_toggle(move |b| plex_auth::manual_token_toggle(b));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlexAuthActions>()
            .on_set_token(move |tok| plex_auth::set_token(weak.clone(), handle.clone(), tok.to_string()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlexAuthActions>()
            .on_ping(move || plex_auth::ping(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlexAuthActions>()
            .on_load_sections(move || plex_auth::load_sections(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlexAuthActions>()
            .on_toggle_section(move |key| {
                plex_auth::toggle_section(weak.clone(), handle.clone(), key.to_string())
            });
    }
    {
        window
            .global::<PlexAuthActions>()
            .on_metadata_write_toggle(move |b| plex_auth::metadata_write_toggle(b));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlexAuthActions>()
            .on_disconnect(move || plex_auth::disconnect(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlexAuthActions>()
            .on_clear_cache(move || plex_auth::clear_cache(weak.clone(), handle.clone()));
    }

    // Settings > Integrations — scrobblers (Last.fm + ListenBrainz). The auth
    // flows + the now-playing/scrobble fire live in `scrobble`; the persisted
    // store is the per-user `scrobbler_settings.db`.
    {
        let weak = window.as_weak();
        window
            .global::<ScrobbleActions>()
            .on_load(move || scrobble::load(weak.clone()));
    }
    {
        let weak = window.as_weak();
        window
            .global::<ScrobbleActions>()
            .on_enable_toggle(move |b| scrobble::enable_toggle(weak.clone(), b));
    }
    {
        window
            .global::<ScrobbleActions>()
            .on_collapse_toggle(move |b| scrobble::collapse_toggle(b));
    }
    {
        let weak = window.as_weak();
        window
            .global::<ScrobbleActions>()
            .on_lastfm_enable_toggle(move |b| scrobble::lastfm_enable_toggle(weak.clone(), b));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<ScrobbleActions>()
            .on_lastfm_connect(move || scrobble::lastfm_connect(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        window
            .global::<ScrobbleActions>()
            .on_lastfm_open_auth_url(move || scrobble::lastfm_open_auth_url(weak.clone()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<ScrobbleActions>()
            .on_lastfm_confirm(move || scrobble::lastfm_confirm(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        window
            .global::<ScrobbleActions>()
            .on_lastfm_disconnect(move || scrobble::lastfm_disconnect(weak.clone()));
    }
    {
        let weak = window.as_weak();
        window
            .global::<ScrobbleActions>()
            .on_listenbrainz_enable_toggle(move |b| {
                scrobble::listenbrainz_enable_toggle(weak.clone(), b)
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<ScrobbleActions>()
            .on_listenbrainz_set_token(move |tok| {
                scrobble::listenbrainz_set_token(weak.clone(), handle.clone(), tok.to_string())
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<ScrobbleActions>()
            .on_listenbrainz_disconnect(move || scrobble::listenbrainz_disconnect(weak.clone()));
    }

    // Tag editor (local album metadata) — open via on_media_action("album",
    // "edit"); these wire the modal's own actions.
    {
        let weak = window.as_weak();
        window
            .global::<TagEditorActions>()
            .on_close(move || tag_editor::close_tag_editor(weak.clone()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<TagEditorActions>()
            .on_save(move || tag_editor::save_tags(weak.clone(), handle.clone(), image_cache.clone()));
    }
    {
        let weak = window.as_weak();
        window
            .global::<TagEditorActions>()
            .on_set_persistence(move |i| {
                if let Some(w) = weak.upgrade() {
                    let s = w.global::<TagEditorState>();
                    // Ignore selecting Direct when unavailable (CUE album).
                    if i == 1 && !s.get_can_direct_write() {
                        s.set_persistence_index(0);
                    } else {
                        s.set_persistence_index(i);
                    }
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<TagEditorActions>()
            .on_set_provider(move |i| {
                if let Some(w) = weak.upgrade() {
                    w.global::<TagEditorState>().set_remote_provider_index(i);
                }
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<TagEditorActions>()
            .on_search_remote(move || tag_editor::search_remote(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        window
            .global::<TagEditorActions>()
            .on_select_result(move |id| tag_editor::select_result(weak.clone(), id.to_string()));
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<TagEditorActions>()
            .on_apply_remote(move || tag_editor::apply_remote(weak.clone(), handle.clone()));
    }
    {
        let weak = window.as_weak();
        window
            .global::<TagEditorActions>()
            .on_open_in_browser(move || tag_editor::open_in_browser(weak.clone()));
    }

    // Dedicated Local album view actions (play / shuffle / edit / add / version).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<LocalAlbumActions>().on_play_all(move || {
            if let Some(w) = weak.upgrade() {
                let tracks = local_library::current_album_version_tracks(&w);
                playback::play_local_tracks(runtime.clone(), weak.clone(), handle.clone(), tracks, 0, false);
            }
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<LocalAlbumActions>().on_shuffle(move || {
            if let Some(w) = weak.upgrade() {
                let tracks = local_library::current_album_version_tracks(&w);
                playback::play_local_tracks(runtime.clone(), weak.clone(), handle.clone(), tracks, 0, true);
            }
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<LocalAlbumActions>().on_play_track(move |id| {
            if let Some(w) = weak.upgrade() {
                let tracks = local_library::current_album_version_tracks(&w);
                let start = id
                    .parse::<i64>()
                    .ok()
                    .and_then(|tid| tracks.iter().position(|t| t.id == tid))
                    .unwrap_or(0);
                playback::play_local_tracks(runtime.clone(), weak.clone(), handle.clone(), tracks, start, false);
            }
        });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<LocalAlbumActions>().on_edit_tags(move || {
            if let Some(w) = weak.upgrade() {
                let idx = w.global::<LocalAlbumState>().get_version_index();
                if let Some(dir) = local_library::album_version_dir(idx) {
                    tag_editor::open_tag_editor(weak.clone(), handle.clone(), dir.clone(), dir);
                }
            }
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<LocalAlbumActions>().on_add_to_playlist(move || {
            if let Some(w) = weak.upgrade() {
                let tracks = local_library::current_album_version_tracks(&w);
                let refs: Vec<String> = tracks.iter().map(local_picker_ref).collect();
                if !refs.is_empty() {
                    playlist_picker::open_multi(&w, &refs, true);
                    let runtime = runtime.clone();
                    let weak2 = weak.clone();
                    handle.spawn(async move {
                        let pls = playlist_picker::load(&runtime).await;
                        let _ = weak2.upgrade_in_event_loop(move |w| {
                            playlist_picker::apply(&w, pls);
                        });
                    });
                }
            }
        });
    }
    {
        // Per-row context-menu actions on the local album detail (play-next /
        // queue / add-to-playlist / add-to-mixtape / favorite) — resolved
        // against the open version's track cache; "play" stays on
        // LocalAlbumActions.play-track.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalAlbumActions>()
            .on_track_menu_action(move |id, action| {
                let Some(w) = weak.upgrade() else { return };
                let tracks = local_library::current_album_version_tracks(&w);
                let Some(row) = tracks.iter().find(|t| t.id.to_string() == id.as_str())
                else {
                    return;
                };
                match action.as_str() {
                    "play-next" | "queue" => {
                        playback::enqueue_local_tracks(
                            runtime.clone(),
                            handle.clone(),
                            vec![row.clone()],
                            action.as_str() == "play-next",
                        );
                    }
                    "add-to-playlist" => {
                        playlist_picker::open_multi(&w, &[local_picker_ref(row)], true);
                        let runtime = runtime.clone();
                        let weak2 = weak.clone();
                        handle.spawn(async move {
                            let pls = playlist_picker::load(&runtime).await;
                            let _ = weak2.upgrade_in_event_loop(move |w| {
                                playlist_picker::apply(&w, pls);
                            });
                        });
                    }
                    "add-to-mixtape" => {
                        // Single-row Add to Mixtape/Collection on the local
                        // album detail (spec §3.1) — the row is already
                        // resolved from the open version's track cache.
                        let items =
                            myqbz_add::track_items_from_local(std::slice::from_ref(row));
                        open_add_to_mixtape(weak.clone(), handle.clone(), items);
                    }
                    "favorite" => {
                        // qobuz_download rows only (the menu gates the entry);
                        // toggle by the REAL Qobuz id, never the local row id
                        // (spec §3.2 — Tauri's latent bug, not ported).
                        match row.qobuz_track_id {
                            Some(qid) => toggle_track_favorite(
                                runtime.clone(),
                                weak.clone(),
                                handle.clone(),
                                qid.to_string(),
                            ),
                            None => log::debug!(
                                "[qbz-slint] favorite: album row {id} has no qobuz_track_id"
                            ),
                        }
                    }
                    "go-to-album" | "go-to-artist" => {
                        // Owner improvement over Tauri — source-routed in
                        // local_row_goto. On this surface "Go to album"
                        // reopens the open album for local rows (Qobuz
                        // album-view parity, where the entry also exists);
                        // qobuz_download rows reach their REAL Qobuz pages.
                        local_row_goto(
                            runtime.clone(),
                            weak.clone(),
                            &handle,
                            row.clone(),
                            action.as_str() == "go-to-artist",
                        );
                    }
                    _ => {
                        log::debug!(
                            "[qbz-slint] unhandled local album track action: {id} {action}"
                        );
                    }
                }
            });
    }
    {
        // Add the whole local/Plex album to a Mixtape/Collection. Builds the
        // `album` payload (source "local", no artwork_url — 1:1 PSD) from the
        // LocalAlbumState header + the current version's track count.
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<LocalAlbumActions>().on_add_to_mixtape(move || {
            if let Some(w) = weak.upgrade() {
                let st = w.global::<LocalAlbumState>();
                let id = st.get_id().to_string();
                if id.is_empty() {
                    return;
                }
                let tracks = local_library::current_album_version_tracks(&w);
                let item = myqbz_add::AddItem {
                    item_type: "album".into(),
                    source: "local".into(),
                    source_item_id: id,
                    title: st.get_title().to_string(),
                    subtitle: {
                        let a = st.get_artist().to_string();
                        (!a.is_empty()).then_some(a)
                    },
                    artwork_url: None,
                    year: None,
                    track_count: (!tracks.is_empty()).then_some(tracks.len() as i32),
                };
                open_add_to_mixtape(weak.clone(), handle.clone(), vec![item]);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<LocalAlbumActions>().on_select_version(move |i| {
            if let Some(w) = weak.upgrade() {
                local_library::apply_album_version(&w, i);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.global::<LocalAlbumActions>().on_search(move |q| {
            local_library::search_album(weak.clone(), q.to_string());
        });
    }
    {
        // Per-disc "Disc N" header ⋯ menu (local album) — scoped to that disc's
        // tracks only, resolved from the open version's track cache. Reuses the
        // SAME local queue ops as the header play-all / shuffle buttons
        // (play_local_tracks, shuffle flag) and the per-row menu's
        // enqueue_local_tracks, just over the disc subset.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalAlbumActions>()
            .on_disc_action(move |disc, action| {
                let Some(w) = weak.upgrade() else { return };
                let tracks = local_library::current_album_disc_tracks(&w, disc);
                if tracks.is_empty() {
                    return;
                }
                match action.as_str() {
                    "play" => playback::play_local_tracks(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        tracks,
                        0,
                        false,
                    ),
                    "shuffle" => playback::play_local_tracks(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        tracks,
                        0,
                        true,
                    ),
                    "queue" => playback::enqueue_local_tracks(
                        runtime.clone(),
                        handle.clone(),
                        tracks,
                        false,
                    ),
                    "play-next" => playback::enqueue_local_tracks(
                        runtime.clone(),
                        handle.clone(),
                        tracks,
                        true,
                    ),
                    other => {
                        log::warn!("[qbz-slint] local disc-action: unknown action {other}");
                    }
                }
            });
    }

    // Local Library — Albums tab controls (search / sort re-query page 1;
    // load-more pages on scroll; retry) + the shared AlbumCollectionView's
    // open / per-card actions (album-detail + playback land with later slices).
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_albums_search(move |_query| {
                // Two-way bound to albums-search; re-derive in memory (full-load).
                if let Some(w) = weak.upgrade() {
                    local_library::derive_albums(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_albums_set_sort(move |sort| {
                if let Some(w) = weak.upgrade() {
                    w.global::<LocalLibraryState>().set_albums_sort(sort);
                    local_library::derive_albums(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_albums_set_group(move |mode| {
                if let Some(w) = weak.upgrade() {
                    w.global::<LocalLibraryState>().set_albums_group(mode);
                    local_library::derive_albums(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_albums_set_view(move |mode| {
                if let Some(w) = weak.upgrade() {
                    w.global::<LocalLibraryState>().set_albums_view_mode(mode);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_albums_filter_changed(move || {
                if let Some(w) = weak.upgrade() {
                    local_library::derive_albums(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_albums_clear_filter(move || {
                if let Some(w) = weak.upgrade() {
                    local_library::clear_album_filter(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<LocalLibraryActions>()
            .on_albums_retry(move || {
                local_library::reload_albums(weak.clone(), handle.clone(), image_cache.clone());
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<LocalLibraryActions>()
            .on_open_album(move |id| {
                nav::record(nav::NavEntry::LocalAlbum(id.to_string()));
                navigate_local_album(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    id.to_string(),
                );
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<LocalLibraryActions>()
            .on_open_artist(move |name| {
                // `name` is the artist NAME (local/Plex artists have no id).
                open_local_artist(&runtime, &weak, &handle, &image_cache, name.to_string());
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_album_action(move |id, action| {
                if action.as_str() == "play" {
                    // The whole album becomes the queue and auto-advances.
                    playback::play_local_album(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        id.to_string(),
                        None,
                    );
                } else {
                    // play-next / queue land with a later slice.
                    log::debug!("[qbz-slint] local album action (queue slice pending): {id} {action}");
                }
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_tracks_search(move |_query| {
                // The query is two-way bound to tracks-search; reload page 1.
                local_library::reload_tracks(weak.clone(), handle.clone());
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_tracks_load_more(move || {
                local_library::load_more_tracks(weak.clone(), handle.clone());
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_tracks_retry(move || {
                local_library::reload_tracks(weak.clone(), handle.clone());
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_track_action(move |id, action| {
                match action.as_str() {
                    "play" => {
                        if let Ok(row_id) = id.parse::<i64>() {
                            // Queue the already-loaded rows (instant — no DB
                            // re-query / cover-fill that delayed the queue) so
                            // playback continues down the list from the click.
                            let tracks = local_library::tracks_current_snapshot();
                            if !tracks.is_empty() {
                                let start = tracks
                                    .iter()
                                    .position(|t| t.id == row_id)
                                    .unwrap_or(0);
                                playback::play_local_tracks(
                                    runtime.clone(),
                                    weak.clone(),
                                    handle.clone(),
                                    tracks,
                                    start,
                                    false,
                                );
                            }
                        }
                    }
                    "toggle-select" => {
                        if let Some(w) = weak.upgrade() {
                            local_library::toggle_track_select(&w, id.as_str());
                        }
                    }
                    "play-next" | "queue" => {
                        // Resolve the row from the loaded cache (no DB) and
                        // enqueue; folder-detail rows aren't in the Tracks
                        // cache, so fall back to a DB resolve off-thread.
                        let play_next = action.as_str() == "play-next";
                        if let Some(row) = local_library::local_track_by_id(id.as_str()) {
                            playback::enqueue_local_tracks(
                                runtime.clone(),
                                handle.clone(),
                                vec![row],
                                play_next,
                            );
                        } else if let Ok(rid) = id.parse::<i64>() {
                            let runtime = runtime.clone();
                            let handle2 = handle.clone();
                            handle.spawn(async move {
                                let row = tokio::task::spawn_blocking(move || {
                                    crate::library_db::with_db(|db| db.get_track(rid))
                                        .flatten()
                                })
                                .await
                                .ok()
                                .flatten();
                                if let Some(row) = row {
                                    playback::enqueue_local_tracks(
                                        runtime,
                                        handle2,
                                        vec![row],
                                        play_next,
                                    );
                                }
                            });
                        }
                    }
                    "add-to-playlist" => {
                        // Per-row picker (Tracks tab + folder-detail rows).
                        // Plex rows ride as "plex:<key>"; plain row ids are
                        // resolved source-aware at insert, so a folder row
                        // missing from the Tracks cache still works.
                        let Some(w) = weak.upgrade() else { return };
                        let track_ref = match local_library::local_track_by_id(id.as_str()) {
                            Some(row) => local_picker_ref(&row),
                            None => id.to_string(),
                        };
                        playlist_picker::open_multi(&w, &[track_ref], true);
                        let runtime = runtime.clone();
                        let weak2 = weak.clone();
                        handle.spawn(async move {
                            let playlists = playlist_picker::load(&runtime).await;
                            let _ = weak2.upgrade_in_event_loop(move |w| {
                                playlist_picker::apply(&w, playlists);
                            });
                        });
                    }
                    "add-to-mixtape" => {
                        // Single-row Add to Mixtape/Collection (Tracks tab +
                        // folder-detail rows; spec §3.1). Same resolution as
                        // play-next: loaded cache first (Plex rows included —
                        // stored as source "local" in the mixtape contract),
                        // DB fallback off-thread for folder rows.
                        if let Some(row) = local_library::local_track_by_id(id.as_str()) {
                            let items = myqbz_add::track_items_from_local(&[row]);
                            open_add_to_mixtape(weak.clone(), handle.clone(), items);
                        } else if let Ok(rid) = id.parse::<i64>() {
                            let weak2 = weak.clone();
                            let handle2 = handle.clone();
                            handle.spawn(async move {
                                let row = tokio::task::spawn_blocking(move || {
                                    crate::library_db::with_db(|db| db.get_track(rid))
                                        .flatten()
                                })
                                .await
                                .ok()
                                .flatten();
                                if let Some(row) = row {
                                    let items = myqbz_add::track_items_from_local(&[row]);
                                    open_add_to_mixtape(weak2, handle2, items);
                                }
                            });
                        }
                    }
                    "favorite" => {
                        // Library-surface favorite: the menu only shows the
                        // entry on qobuz_download rows (TrackRow gates on
                        // source == "qobuz"), and the toggle uses the row's
                        // REAL qobuz_track_id — never the local row id, which
                        // is what Tauri sends (spec §3.2 latent bug; we port
                        // the intent, not the bug).
                        if let Some(row) = local_library::local_track_by_id(id.as_str()) {
                            match row.qobuz_track_id {
                                Some(qid) => toggle_track_favorite(
                                    runtime.clone(),
                                    weak.clone(),
                                    handle.clone(),
                                    qid.to_string(),
                                ),
                                None => log::debug!(
                                    "[qbz-slint] favorite: local row {id} has no qobuz_track_id"
                                ),
                            }
                        } else if let Ok(rid) = id.parse::<i64>() {
                            // Folder rows aren't in the Tracks cache: resolve
                            // off-thread, then hop back to the UI thread (the
                            // toggle reads/writes UI models).
                            let runtime = runtime.clone();
                            let weak2 = weak.clone();
                            let handle2 = handle.clone();
                            handle.spawn(async move {
                                let row = tokio::task::spawn_blocking(move || {
                                    crate::library_db::with_db(|db| db.get_track(rid))
                                        .flatten()
                                })
                                .await
                                .ok()
                                .flatten();
                                let Some(qid) = row.and_then(|r| r.qobuz_track_id) else {
                                    log::debug!(
                                        "[qbz-slint] favorite: row {rid} has no qobuz_track_id"
                                    );
                                    return;
                                };
                                let weak3 = weak2.clone();
                                let _ = weak2.upgrade_in_event_loop(move |_w| {
                                    toggle_track_favorite(
                                        runtime,
                                        weak3,
                                        handle2,
                                        qid.to_string(),
                                    );
                                });
                            });
                        }
                    }
                    "go-to-album" | "go-to-artist" => {
                        // Owner improvement over Tauri (which omits both on
                        // local rows): resolve the row (Tracks cache first,
                        // DB fallback for folder-detail rows — same seam as
                        // favorite) and source-route in local_row_goto
                        // (local/plex -> local album view / LocalLibrary
                        // artist by name; qobuz_download -> the REAL Qobuz
                        // pages via its qobuz_track_id).
                        let to_artist = action.as_str() == "go-to-artist";
                        if let Some(row) = local_library::local_track_by_id(id.as_str()) {
                            local_row_goto(runtime.clone(), weak.clone(), &handle, row, to_artist);
                        } else if let Ok(rid) = id.parse::<i64>() {
                            let runtime = runtime.clone();
                            let weak2 = weak.clone();
                            let handle2 = handle.clone();
                            handle.spawn(async move {
                                let row = tokio::task::spawn_blocking(move || {
                                    crate::library_db::with_db(|db| db.get_track(rid))
                                        .flatten()
                                })
                                .await
                                .ok()
                                .flatten();
                                match row {
                                    Some(row) => local_row_goto(
                                        runtime, weak2, &handle2, row, to_artist,
                                    ),
                                    None => log::debug!(
                                        "[qbz-slint] go-to: local row {rid} not found"
                                    ),
                                }
                            });
                        }
                    }
                    _ => {
                        log::debug!("[qbz-slint] unhandled local track action: {id} {action}");
                    }
                }
            });
    }

    // ---- Tracks tab: group-by + multi-select + bulk ----
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_tracks_set_group(move |mode| {
                if let Some(w) = weak.upgrade() {
                    local_library::set_tracks_group(&w, mode.as_str());
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_tracks_toggle_multi_select(move || {
                if let Some(w) = weak.upgrade() {
                    let on = w.global::<LocalLibraryState>().get_tracks_multi_select();
                    local_library::set_tracks_multi_select(&w, !on);
                }
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_tracks_bulk_action(move |action| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                match action.as_str() {
                    "select-all" => local_library::select_all_tracks(&w),
                    "clear" => local_library::clear_tracks_selection(&w),
                    "queue" => {
                        let rows = local_library::selected_local_tracks(&w);
                        playback::enqueue_local_tracks(runtime.clone(), handle.clone(), rows, false);
                        local_library::clear_tracks_selection(&w);
                    }
                    "play-next" => {
                        let rows = local_library::selected_local_tracks(&w);
                        playback::enqueue_local_tracks(runtime.clone(), handle.clone(), rows, true);
                        local_library::clear_tracks_selection(&w);
                    }
                    "add-to-playlist" => {
                        // Source-aware refs: Plex rows ride as "plex:<key>",
                        // the rest as library row ids (resolved at insert).
                        let rows = local_library::selected_local_tracks(&w);
                        let ids: Vec<String> = rows.iter().map(local_picker_ref).collect();
                        if !ids.is_empty() {
                            playlist_picker::open_multi(&w, &ids, true);
                            let runtime = runtime.clone();
                            let weak2 = weak.clone();
                            handle.spawn(async move {
                                let playlists = playlist_picker::load(&runtime).await;
                                let _ = weak2.upgrade_in_event_loop(move |w| {
                                    playlist_picker::apply(&w, playlists);
                                });
                            });
                        }
                    }
                    "add-to-mixtape" => {
                        // All selected tracks (Plex INCLUDED — Plex rows are
                        // stored as source "local" in the mixtape contract).
                        let rows = local_library::selected_local_tracks(&w);
                        let items = myqbz_add::track_items_from_local(&rows);
                        if !items.is_empty() {
                            open_add_to_mixtape(weak.clone(), handle.clone(), items);
                            local_library::clear_tracks_selection(&w);
                        }
                    }
                    _ => {}
                }
            });
    }

    // ---- Folders tree rail: search / collapse / multi-select ----
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_folders_tree_search(move |query| {
                if let Some(w) = weak.upgrade() {
                    local_library::folders_tree_search(&w, query.as_str());
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_folders_collapse_all(move || {
                if let Some(w) = weak.upgrade() {
                    local_library::collapse_all_tree(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_folders_toggle_select_mode(move || {
                if let Some(w) = weak.upgrade() {
                    local_library::toggle_tree_select_mode(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_folders_toggle_folder_select(move |path| {
                local_library::toggle_tree_folder_select(weak.clone(), handle.clone(), path.to_string());
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_folders_toggle_track_select(move |path| {
                local_library::toggle_tree_track_select(weak.clone(), handle.clone(), path.to_string());
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_folders_bulk_action(move |action| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                match action.as_str() {
                    "select-all" => {
                        local_library::tree_select_all(weak.clone(), handle.clone());
                    }
                    "clear" => local_library::tree_clear_selection(&w),
                    "queue" => {
                        let rows = local_library::tree_selected_snapshot();
                        playback::enqueue_local_tracks(runtime.clone(), handle.clone(), rows, false);
                        local_library::tree_clear_selection(&w);
                    }
                    "play-next" => {
                        let rows = local_library::tree_selected_snapshot();
                        playback::enqueue_local_tracks(runtime.clone(), handle.clone(), rows, true);
                        local_library::tree_clear_selection(&w);
                    }
                    "add-to-playlist" => {
                        // Source-aware refs (Plex rows as "plex:<key>").
                        let rows = local_library::tree_selected_snapshot();
                        let ids: Vec<String> = rows.iter().map(local_picker_ref).collect();
                        if !ids.is_empty() {
                            playlist_picker::open_multi(&w, &ids, true);
                            let runtime = runtime.clone();
                            let weak2 = weak.clone();
                            handle.spawn(async move {
                                let playlists = playlist_picker::load(&runtime).await;
                                let _ = weak2.upgrade_in_event_loop(move |w| {
                                    playlist_picker::apply(&w, playlists);
                                });
                            });
                        }
                    }
                    "add-to-mixtape" => {
                        // All selected tracks (Plex included — stored as "local").
                        let rows = local_library::tree_selected_snapshot();
                        let items = myqbz_add::track_items_from_local(&rows);
                        if !items.is_empty() {
                            open_add_to_mixtape(weak.clone(), handle.clone(), items);
                            local_library::tree_clear_selection(&w);
                        }
                    }
                    _ => {}
                }
            });
    }

    // ---- Folders tab actions ----
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_folders_search(move |_query| {
                // Query is two-way bound to folders-search; re-derive in place.
                if let Some(w) = weak.upgrade() {
                    local_library::derive_folders(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_folders_set_sort(move |sort| {
                if let Some(w) = weak.upgrade() {
                    w.global::<LocalLibraryState>().set_folders_sort(sort);
                    local_library::derive_folders(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_folders_set_group(move |group| {
                if let Some(w) = weak.upgrade() {
                    w.global::<LocalLibraryState>().set_folders_group(group);
                    local_library::derive_folders(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_folders_set_mode(move |mode| {
                if let Some(w) = weak.upgrade() {
                    w.global::<LocalLibraryState>()
                        .set_folders_view_mode(mode.clone());
                }
                // Lazy-load the tree roots the first time tree mode is shown.
                if mode.as_str() == "tree" {
                    local_library::ensure_folder_tree_loaded(weak.clone(), handle.clone());
                }
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_folders_toggle_node(move |path, expand| {
                local_library::toggle_folder_node(
                    weak.clone(),
                    handle.clone(),
                    path.to_string(),
                    expand,
                );
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<LocalLibraryActions>()
            .on_folders_select(move |path, segment| {
                local_library::select_folder(
                    weak.clone(),
                    handle.clone(),
                    image_cache.clone(),
                    path.to_string(),
                    segment.to_string(),
                );
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_folder_detail_search(move |query| {
                if let Some(w) = weak.upgrade() {
                    local_library::folder_detail_search(&w, query.as_str());
                }
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_folders_play_node(move |path| {
                playback::play_local_folder_recursive(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    path.to_string(),
                );
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_folders_play_track(move |id| {
                if let Ok(row_id) = id.parse::<i64>() {
                    let path = weak
                        .upgrade()
                        .map(|w| {
                            w.global::<LocalLibraryState>()
                                .get_folders_selected_path()
                                .to_string()
                        })
                        .unwrap_or_default();
                    if !path.is_empty() {
                        playback::play_local_folder_tracks_from(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            path,
                            row_id,
                        );
                    }
                }
            });
    }

    // ---- Ephemeral folder actions ----
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_ephemeral_open(move || {
                local_library::open_ephemeral(runtime.clone(), weak.clone(), handle.clone());
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_ephemeral_play_all(move || {
                playback::ephemeral_play_or_prompt(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    "all".to_string(),
                    String::new(),
                );
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_ephemeral_play_track(move |id| {
                playback::ephemeral_play_or_prompt(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    "track".to_string(),
                    id.to_string(),
                );
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_ephemeral_play_album(move |key| {
                playback::ephemeral_play_or_prompt(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    "album".to_string(),
                    key.to_string(),
                );
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<LocalLibraryActions>()
            .on_ephemeral_clear(move || {
                let runtime = runtime.clone();
                let weak = weak.clone();
                handle.spawn(async move {
                    // Stop a playing ephemeral track before dropping the session
                    // so its (about-to-be-reused) id can't false-highlight rows.
                    playback::wipe_ephemeral_if_playing(&runtime, &weak).await;
                    let _ = weak.upgrade_in_event_loop(|w| {
                        local_library::clear_ephemeral(&w);
                    });
                });
            });
    }
    // Ephemeral "already playing" choice modal — clear-and-play vs add-to-queue.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<EphemeralPlayChoiceActions>()
            .on_replace(move || {
                if let Some(w) = weak.upgrade() {
                    let s = w.global::<EphemeralPlayChoiceState>();
                    let kind = s.get_intent_kind().to_string();
                    let arg = s.get_intent_arg().to_string();
                    s.set_open(false);
                    playback::ephemeral_play(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        kind,
                        arg,
                    );
                }
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<EphemeralPlayChoiceActions>()
            .on_enqueue(move || {
                if let Some(w) = weak.upgrade() {
                    let s = w.global::<EphemeralPlayChoiceState>();
                    let kind = s.get_intent_kind().to_string();
                    let arg = s.get_intent_arg().to_string();
                    s.set_open(false);
                    playback::ephemeral_enqueue(
                        runtime.clone(),
                        weak.clone(),
                        handle.clone(),
                        kind,
                        arg,
                    );
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<EphemeralPlayChoiceActions>()
            .on_close(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<EphemeralPlayChoiceState>().set_open(false);
                }
            });
    }

    // Restore a previously-open ephemeral folder (re-scans the path; does NOT
    // switch the landing view). Runs once at startup.
    local_library::rehydrate_ephemeral(window.as_weak(), tokio_rt.handle().clone());

    // ---- Artists tab actions ----
    {
        let weak = window.as_weak();
        window
            .global::<LocalLibraryActions>()
            .on_artists_search(move |_query| {
                // Query is two-way bound to artists-search; re-derive in place.
                if let Some(w) = weak.upgrade() {
                    local_library::derive_artists(&w);
                }
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<LocalLibraryActions>()
            .on_artists_select(move |name| {
                local_library::select_local_artist(
                    weak.clone(),
                    handle.clone(),
                    image_cache.clone(),
                    name.to_string(),
                );
            });
    }

    // Discover "View all" — open the full-list page for a section,
    // recording it as a history entry (mirrors the favorites branch
    // of on_header_menu_navigate).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.on_discover_view_all(move |endpoint, title| {
            nav::record(nav::NavEntry::DiscoverBrowse {
                endpoint: endpoint.to_string(),
                title: title.to_string(),
            });
            if let Some(w) = weak.upgrade() {
                update_nav_flags(&w);
            }
            discover_browse::navigate(
                runtime.clone(),
                weak.clone(),
                &handle,
                image_cache.clone(),
                endpoint.to_string(),
                title.to_string(),
                current_genre_filter(),
            );
        });
    }

    // Discover "View all" pagination — load the next album page when
    // the grid scrolls near the bottom.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<DiscoverBrowseActions>()
            .on_load_more(move || {
                discover_browse::load_more(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    current_genre_filter(),
                );
            });
    }

    // Discover "View all" search — re-filter the loaded albums
    // client-side after the search box changes (UI thread).
    {
        let weak = window.as_weak();
        window
            .global::<DiscoverBrowseActions>()
            .on_search_changed(move || {
                if let Some(w) = weak.upgrade() {
                    discover_browse::apply_filter(&w);
                }
            });
    }

    // Favorites view actions — tab switch (lazy-load), open album /
    // artist, and per-row track actions routed to the media-action
    // "Add to playlist" picker — pick adds the pending track to the
    // chosen playlist; close dismisses.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<PlaylistPickerActions>()
            .on_pick(move |playlist_id| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let picker = w.global::<PlaylistPickerState>();
                let is_local = picker.get_local_mode();
                // Bulk add carries track-ids; single add carries track-id.
                let ids_model = picker.get_track_ids();
                let track_id_single = picker.get_track_id().to_string();
                // Resolve the target name for the success toast BEFORE the
                // model is torn down by closing the picker.
                let target_name = picker_playlist_name(&w, playlist_id.as_str());
                picker.set_open(false);

                // LOCAL playlist target (id "local:<uuid>") — writes go to
                // the library.db repo (works offline; D7 routing).
                if local_playlist::is_local_id(playlist_id.as_str()) {
                    let target = playlist_id.to_string();
                    if is_local {
                        // Local-mode refs — LocalLibrary row ids ("<i64>",
                        // source-aware mapping: local path / offline-copy
                        // Qobuz id) or Plex rows ("plex:<rating key>").
                        let refs: Vec<String> = (0..ids_model.row_count())
                            .filter_map(|i| ids_model.row_data(i))
                            .map(|s| s.to_string())
                            .collect();
                        if refs.is_empty() {
                            return;
                        }
                        let weak = weak.clone();
                        let tname = target_name.clone();
                        handle.spawn(async move {
                            let added = tokio::task::spawn_blocking(move || {
                                local_playlist::add_local_refs_blocking(&target, &refs)
                            })
                            .await
                            .unwrap_or(0);
                            // reco: local refs are not Qobuz catalog ids — not
                            // logged (same source gate as local plays).
                            toast_added_tracks(&weak, added, tname);
                        });
                        return;
                    }
                    let mut ids: Vec<u64> = (0..ids_model.row_count())
                        .filter_map(|i| ids_model.row_data(i))
                        .filter_map(|s| s.parse::<u64>().ok())
                        .collect();
                    if ids.is_empty() {
                        if let Ok(tid) = track_id_single.parse::<u64>() {
                            ids.push(tid);
                        }
                    }
                    if ids.is_empty() {
                        return;
                    }
                    let weak = weak.clone();
                    let tname = target_name.clone();
                    handle.spawn(async move {
                        // reco: keep the full Qobuz ids before they move into
                        // the add closure (local-playlist target = no Qobuz pid).
                        let reco_ids = ids.clone();
                        let added = tokio::task::spawn_blocking(move || {
                            local_playlist::add_qobuz_tracks_blocking(&target, &ids)
                        })
                        .await
                        .unwrap_or(0);
                        tokio::task::spawn_blocking(move || {
                            crate::reco::log_playlist_add(None, reco_ids)
                        });
                        toast_added_tracks(&weak, added, tname);
                    });
                    return;
                }

                let Ok(pid) = playlist_id.parse::<u64>() else {
                    return;
                };

                if is_local {
                    // Local-mode refs onto a QOBUZ playlist: row ids attach
                    // via the local sidecar, "plex:<key>" refs via the Plex
                    // sidecar (same tables the offline detail renders).
                    let refs: Vec<String> = (0..ids_model.row_count())
                        .filter_map(|i| ids_model.row_data(i))
                        .map(|s| s.to_string())
                        .collect();
                    if refs.is_empty() {
                        return;
                    }
                    // Seam C: append after the whole merged list AND past
                    // any stored position (the old 0-based `enumerate`
                    // write collided slots -> silent row loss in the
                    // interleave). Base = the Qobuz block size from the
                    // sidebar's session cache; re-adding an existing ref
                    // MOVES it to the append slot (INSERT OR REPLACE, E4).
                    let qobuz_count = sidebar::playlist_track_count(pid).unwrap_or(0);
                    let refs_count = refs.len();
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle2 = handle.clone();
                    let image_cache = image_cache.clone();
                    let tname = target_name.clone();
                    handle.spawn(async move {
                        let _ = tokio::task::spawn_blocking(move || {
                            crate::library_db::with_db(|db| {
                                let mut next =
                                    db.next_playlist_sidecar_position(pid, qobuz_count)?;
                                for r in refs.iter() {
                                    if let Some(key) = r.strip_prefix("plex:") {
                                        db.add_plex_track_to_playlist(pid, key, next)?;
                                        next += 1;
                                    } else if let Ok(lid) = r.parse::<i64>() {
                                        db.add_local_track_to_playlist(pid, lid, next)?;
                                        next += 1;
                                    }
                                }
                                Ok(())
                            })
                        })
                        .await;
                        // reco: local refs are not Qobuz catalog ids — not
                        // logged (same source gate as local plays).
                        toast_added_tracks(&weak, refs_count, tname);
                        // E12: the open detail re-merges so the rows show
                        // up immediately.
                        let _ = weak.clone().upgrade_in_event_loop(move |w| {
                            if w.global::<NavState>().get_view() == ContentView::Playlist
                                && w.global::<PlaylistState>().get_id().to_string()
                                    == pid.to_string()
                            {
                                navigate_playlist(
                                    runtime,
                                    weak,
                                    &handle2,
                                    image_cache,
                                    pid.to_string(),
                                );
                            }
                        });
                    });
                    return;
                }

                // Qobuz tracks → Qobuz playlist. Run duplicate detection first
                // (Tauri parity: this is the ONLY branch that checks dupes).
                // If any of the ids are already in the target, park the context
                // in DUP_CONFIRM_STASH and open the confirm sub-modal; the user
                // then chooses add-all / add-new-only. With no dupes we add
                // directly and toast.
                let mut ids: Vec<u64> = (0..ids_model.row_count())
                    .filter_map(|i| ids_model.row_data(i))
                    .filter_map(|s| s.parse::<u64>().ok())
                    .collect();
                if ids.is_empty() {
                    if let Ok(tid) = track_id_single.parse::<u64>() {
                        ids.push(tid);
                    }
                }
                if ids.is_empty() {
                    return;
                }
                let runtime = runtime.clone();
                let weak = weak.clone();
                let tname = target_name.clone();
                handle.spawn(async move {
                    match runtime.core().check_playlist_duplicates(pid, &ids).await {
                        Ok(dup) if dup.duplicate_count > 0 => {
                            // Stash the full context; the confirm handlers read
                            // it back. Open the sub-modal with the counts.
                            let total = dup.total_tracks as i32;
                            let dupc = dup.duplicate_count as i32;
                            let dup_ids = dup.duplicate_track_ids.clone();
                            let stash = (pid, ids.clone(), dup_ids, tname.clone());
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                DUP_CONFIRM_STASH.with(|c| *c.borrow_mut() = Some(stash));
                                let st = w.global::<DuplicateConfirmState>();
                                st.set_duplicate_count(dupc);
                                st.set_total_count(total);
                                st.set_busy(false);
                                st.set_playlist_name(tname.into());
                                st.set_open(true);
                            });
                        }
                        Ok(_) => {
                            // No duplicates — add directly + toast.
                            let n = ids.len();
                            if let Err(e) =
                                runtime.core().add_tracks_to_playlist(pid, &ids).await
                            {
                                log::error!("[qbz-slint] add to playlist failed: {e}");
                            } else {
                                // reco: log the full requested Qobuz ids.
                                let reco_ids = ids.clone();
                                tokio::task::spawn_blocking(move || {
                                    crate::reco::log_playlist_add(Some(pid), reco_ids)
                                });
                                toast_added_tracks(&weak, n, tname);
                            }
                        }
                        Err(e) => {
                            // Dup check failed (e.g. transient API) — fall back
                            // to a direct add so the action still completes.
                            log::warn!(
                                "[qbz-slint] dup check failed, adding directly: {e}"
                            );
                            let n = ids.len();
                            if let Err(e) =
                                runtime.core().add_tracks_to_playlist(pid, &ids).await
                            {
                                log::error!("[qbz-slint] add to playlist failed: {e}");
                            } else {
                                // reco: log the full requested Qobuz ids.
                                let reco_ids = ids.clone();
                                tokio::task::spawn_blocking(move || {
                                    crate::reco::log_playlist_add(Some(pid), reco_ids)
                                });
                                toast_added_tracks(&weak, n, tname);
                            }
                        }
                    }
                });
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistPickerActions>()
            .on_close(move || {
                if let Some(w) = weak.upgrade() {
                    let st = w.global::<PlaylistPickerState>();
                    st.set_open(false);
                    // Reset the inline-create + filter affordances so the next
                    // open starts clean.
                    st.set_creating_open(false);
                    st.set_create_name("".into());
                    st.set_creating(false);
                    st.set_filter("".into());
                }
            });
    }

    // Inline "Create new playlist" → create-and-add. Creates a playlist
    // (Qobuz online / local offline per D8) and adds the carried tracks to
    // it, then closes the picker. Discriminates the carried ids exactly like
    // the pick handler (local-mode refs vs Qobuz u64 ids).
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlaylistPickerActions>()
            .on_create_and_add(move || {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                use slint::Model;
                let picker = w.global::<PlaylistPickerState>();
                let name = picker.get_create_name().to_string();
                if name.trim().is_empty() || picker.get_creating() {
                    return;
                }
                let is_local = picker.get_local_mode();
                let ids_model = picker.get_track_ids();
                let track_id_single = picker.get_track_id().to_string();
                // Local-mode refs (LocalLibrary row ids / "plex:<key>") for the
                // local-playlist add; Qobuz u64 ids for the online path.
                let refs: Vec<String> = (0..ids_model.row_count())
                    .filter_map(|i| ids_model.row_data(i))
                    .map(|s| s.to_string())
                    .collect();
                let mut qobuz_ids: Vec<u64> = (0..ids_model.row_count())
                    .filter_map(|i| ids_model.row_data(i))
                    .filter_map(|s| s.parse::<u64>().ok())
                    .collect();
                if qobuz_ids.is_empty() {
                    if let Ok(tid) = track_id_single.parse::<u64>() {
                        qobuz_ids.push(tid);
                    }
                }
                picker.set_creating(true);

                let offline_now = offline_mode::engine().is_offline();
                let nm = name.trim().to_string();
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle2 = handle.clone();

                if offline_now {
                    // D8: offline ⇒ LOCAL playlist (library.db), never the
                    // retired pending-playlist engine. Mirrors the create
                    // modal's offline branch.
                    let local_refs = refs.clone();
                    let local_qobuz = qobuz_ids.clone();
                    // reco: the full Qobuz ids (empty when adding local refs).
                    let reco_qobuz: Vec<u64> = if is_local { Vec::new() } else { qobuz_ids.clone() };
                    handle.spawn(async move {
                        let created = tokio::task::spawn_blocking({
                            let nm = nm.clone();
                            move || local_playlist::create_blocking(&nm, None, true)
                        })
                        .await
                        .ok()
                        .flatten();
                        let mut added = 0usize;
                        if let Some(ref new_id) = created {
                            let new_id = new_id.clone();
                            added = tokio::task::spawn_blocking(move || {
                                if is_local {
                                    local_playlist::add_local_refs_blocking(
                                        &new_id,
                                        &local_refs,
                                    )
                                } else {
                                    local_playlist::add_qobuz_tracks_blocking(
                                        &new_id,
                                        &local_qobuz,
                                    )
                                }
                            })
                            .await
                            .unwrap_or(0);
                        }
                        // reco: log the new playlist's Qobuz tracks (new local
                        // playlist = no Qobuz pid; empty when local refs).
                        if created.is_some() {
                            let reco_ids = reco_qobuz;
                            tokio::task::spawn_blocking(move || {
                                crate::reco::log_playlist_add(None, reco_ids)
                            });
                        }
                        let r2 = runtime.clone();
                        let h2 = handle2.clone();
                        let weak2 = weak.clone();
                        let nm2 = nm.clone();
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            let st = w.global::<PlaylistPickerState>();
                            st.set_creating(false);
                            st.set_creating_open(false);
                            st.set_create_name("".into());
                            st.set_open(false);
                            match created {
                                Some(_) => {
                                    toast_added_tracks(&weak2, added, nm2);
                                    load_sidebar_playlists(r2, weak2, &h2);
                                }
                                None => {
                                    log::error!(
                                        "[qbz-slint] create-and-add (local) failed"
                                    );
                                }
                            }
                        });
                    });
                    return;
                }

                // Online ⇒ Qobuz playlist, then add the carried tracks.
                handle.spawn(async move {
                    match runtime.core().create_playlist(&nm, None, false).await {
                        Ok(playlist) => {
                            let pid = playlist.id;
                            let n = qobuz_ids.len();
                            if !qobuz_ids.is_empty() {
                                if let Err(e) = runtime
                                    .core()
                                    .add_tracks_to_playlist(pid, &qobuz_ids)
                                    .await
                                {
                                    log::error!(
                                        "[qbz-slint] create-and-add: add failed: {e}"
                                    );
                                }
                                // reco: log the new playlist's Qobuz tracks.
                                let reco_ids = qobuz_ids.clone();
                                tokio::task::spawn_blocking(move || {
                                    crate::reco::log_playlist_add(Some(pid), reco_ids)
                                });
                            }
                            let r2 = runtime.clone();
                            let h2 = handle2.clone();
                            let weak2 = weak.clone();
                            let nm2 = nm.clone();
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                let st = w.global::<PlaylistPickerState>();
                                st.set_creating(false);
                                st.set_creating_open(false);
                                st.set_create_name("".into());
                                st.set_open(false);
                                toast_added_tracks(&weak2, n, nm2);
                                load_sidebar_playlists(r2, weak2, &h2);
                            });
                        }
                        Err(e) => {
                            log::error!("[qbz-slint] create-and-add: create failed: {e}");
                            let _ = weak.upgrade_in_event_loop(|w| {
                                w.global::<PlaylistPickerState>().set_creating(false);
                            });
                        }
                    }
                });
            });
    }

    // Picker client-side filter — recompute each row's `filter-rank`
    // (case-insensitive substring; Slint 1.16 has no string `.contains`, so
    // the match runs here). Rank = match ordinal among the filtered rows,
    // -1 = filtered out; the total lands in `filter-matches`. Pure frontend,
    // no backend call.
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistPickerActions>()
            .on_filter_changed(move |query| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                use slint::Model;
                let needle = query.to_lowercase();
                let model = w.global::<PlaylistPickerState>().get_playlists();
                let mut rank: i32 = 0;
                for i in 0..model.row_count() {
                    if let Some(mut item) = model.row_data(i) {
                        let matches = needle.is_empty()
                            || item.name.to_lowercase().contains(&needle);
                        let new_rank = if matches { rank } else { -1 };
                        if matches {
                            rank += 1;
                        }
                        if item.filter_rank != new_rank {
                            item.filter_rank = new_rank;
                            model.set_row_data(i, item);
                        }
                    }
                }
                w.global::<PlaylistPickerState>().set_filter_matches(rank);
            });
    }

    // Duplicate-confirm sub-modal handlers. The pending context lives in
    // DUP_CONFIRM_STASH (set by the picker's Qobuz→Qobuz branch). Each handler
    // reads it, performs the chosen add, toasts, then closes + clears.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<DuplicateConfirmActions>()
            .on_add_all(move || {
                let Some(stash) = DUP_CONFIRM_STASH.with(|c| c.borrow_mut().take()) else {
                    return;
                };
                let (pid, all_ids, _dup_ids, name) = stash;
                if let Some(w) = weak.upgrade() {
                    w.global::<DuplicateConfirmState>().set_busy(true);
                }
                let runtime = runtime.clone();
                let weak = weak.clone();
                handle.spawn(async move {
                    let n = all_ids.len();
                    if let Err(e) = runtime.core().add_tracks_to_playlist(pid, &all_ids).await
                    {
                        log::error!("[qbz-slint] dup add-all failed: {e}");
                    } else {
                        // reco: log the full requested Qobuz ids (add-all).
                        let reco_ids = all_ids.clone();
                        tokio::task::spawn_blocking(move || {
                            crate::reco::log_playlist_add(Some(pid), reco_ids)
                        });
                        toast_added_tracks(&weak, n, name);
                    }
                    let _ = weak.upgrade_in_event_loop(|w| {
                        let st = w.global::<DuplicateConfirmState>();
                        st.set_busy(false);
                        st.set_open(false);
                    });
                });
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<DuplicateConfirmActions>()
            .on_add_new_only(move || {
                let Some(stash) = DUP_CONFIRM_STASH.with(|c| c.borrow_mut().take()) else {
                    return;
                };
                let (pid, all_ids, dup_ids, name) = stash;
                // reco: keep the FULL requested ids before the dedup consumes
                // them (Tauri logs the full request, not the inserted subset).
                let reco_all = all_ids.clone();
                // Add only the non-duplicate ids (all \ duplicates). If nothing
                // is left, just close.
                let to_add: Vec<u64> =
                    all_ids.into_iter().filter(|id| !dup_ids.contains(id)).collect();
                if to_add.is_empty() {
                    if let Some(w) = weak.upgrade() {
                        w.global::<DuplicateConfirmState>().set_open(false);
                    }
                    return;
                }
                if let Some(w) = weak.upgrade() {
                    w.global::<DuplicateConfirmState>().set_busy(true);
                }
                let runtime = runtime.clone();
                let weak = weak.clone();
                handle.spawn(async move {
                    let n = to_add.len();
                    if let Err(e) = runtime.core().add_tracks_to_playlist(pid, &to_add).await
                    {
                        log::error!("[qbz-slint] dup add-new-only failed: {e}");
                    } else {
                        // reco: log the FULL requested ids (Tauri parity), not
                        // just the non-duplicate subset that was inserted.
                        let reco_ids = reco_all;
                        tokio::task::spawn_blocking(move || {
                            crate::reco::log_playlist_add(Some(pid), reco_ids)
                        });
                        toast_added_tracks(&weak, n, name);
                    }
                    let _ = weak.upgrade_in_event_loop(|w| {
                        let st = w.global::<DuplicateConfirmState>();
                        st.set_busy(false);
                        st.set_open(false);
                    });
                });
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<DuplicateConfirmActions>()
            .on_cancel(move || {
                DUP_CONFIRM_STASH.with(|c| *c.borrow_mut() = None);
                if let Some(w) = weak.upgrade() {
                    let st = w.global::<DuplicateConfirmState>();
                    st.set_busy(false);
                    st.set_open(false);
                }
            });
    }

    // Track drag onto sidebar playlists (a star QBZ feature).
    {
        let weak = window.as_weak();
        window.global::<DragActions>().on_start(
            move |track_id, title, subtitle, gx, gy| {
                let Some(w) = weak.upgrade() else { return };
                log::info!("[qbz-slint][drag] start gx={gx} gy={gy} (cursor should be here)");
                let tracks = gather_drag_tracks(&w, track_id.as_str());
                let count = tracks.len();
                drag::set_dragged(tracks);
                let ds = w.global::<DragState>();
                ds.set_count(count as i32);
                ds.set_ghost_title(title);
                ds.set_ghost_subtitle(subtitle);
                ds.set_pointer_x(gx);
                ds.set_pointer_y(gy);
                ds.set_over_playlist_id("".into());
                ds.set_active(true);
            },
        );
    }
    {
        let weak = window.as_weak();
        window.global::<DragActions>().on_move(move |gx, gy| {
            if let Some(w) = weak.upgrade() {
                let ds = w.global::<DragState>();
                ds.set_pointer_x(gx);
                ds.set_pointer_y(gy);
            }
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window.global::<DragActions>().on_end(move || {
            let Some(w) = weak.upgrade() else { return };
            let ds = w.global::<DragState>();
            let pid = ds.get_over_playlist_id().to_string();
            ds.set_active(false);
            ds.set_over_playlist_id("".into());
            let tracks = drag::dragged();
            drag::clear();
            if tracks.is_empty() {
                return;
            }
            // Drop onto a LOCAL playlist row — write the repo source-aware
            // (D7 routing): local file rows store local_path, Plex rows
            // plex_key, Qobuz/offline-cached rows qobuz_track_id.
            if local_playlist::is_local_id(&pid) {
                handle.spawn(async move {
                    let n = tokio::task::spawn_blocking(move || {
                        local_playlist::add_drag_tracks_blocking(&pid, &tracks)
                    })
                    .await
                    .unwrap_or(0);
                    log::info!("[qbz-slint] dropped {n} track(s) onto local playlist");
                });
                return;
            }
            if let Ok(pid) = pid.parse::<u64>() {
                // Qobuz playlist target: catalog ids become real membership;
                // local rows / Plex rows attach via the mixed-playlist
                // sidecars (the same tables the picker's local mode writes).
                let mut qobuz: Vec<u64> = Vec::new();
                let mut local_rows: Vec<i64> = Vec::new();
                let mut plex: Vec<String> = Vec::new();
                for item in tracks {
                    match item {
                        drag::DragTrack::Qobuz(id) => qobuz.push(id),
                        drag::DragTrack::LocalRow(id) => local_rows.push(id),
                        drag::DragTrack::Plex(key) => plex.push(key),
                    }
                }
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle2 = handle.clone();
                let image_cache = image_cache.clone();
                handle.spawn(async move {
                    let mut added = 0usize;
                    if !qobuz.is_empty() {
                        match runtime.core().add_tracks_to_playlist(pid, &qobuz).await {
                            Ok(()) => added += qobuz.len(),
                            Err(e) => {
                                log::error!("[qbz-slint] drop add to playlist failed: {e}")
                            }
                        }
                    }
                    let sidecar_added = !local_rows.is_empty() || !plex.is_empty();
                    if sidecar_added {
                        // Seam C: append after the merged list / past any
                        // stored position — never 0-based. The Qobuz block
                        // size includes the rows the SAME drop just added
                        // (the sidebar cache hasn't seen them yet).
                        let qobuz_count = sidebar::playlist_track_count(pid)
                            .unwrap_or(0)
                            + qobuz.len() as u32;
                        let n = tokio::task::spawn_blocking(move || {
                            crate::library_db::with_db(|db| {
                                let mut next =
                                    db.next_playlist_sidecar_position(pid, qobuz_count)?;
                                for rid in local_rows.iter() {
                                    db.add_local_track_to_playlist(pid, *rid, next)?;
                                    next += 1;
                                }
                                for key in plex.iter() {
                                    db.add_plex_track_to_playlist(pid, key, next)?;
                                    next += 1;
                                }
                                Ok(local_rows.len() + plex.len())
                            })
                            .unwrap_or(0)
                        })
                        .await
                        .unwrap_or(0);
                        added += n;
                    }
                    if added > 0 {
                        log::info!(
                            "[qbz-slint] dropped {added} track(s) onto playlist {pid}"
                        );
                    }
                    if sidecar_added {
                        // E12: re-merge the open detail after a sidecar
                        // write to the same playlist.
                        let _ = weak.clone().upgrade_in_event_loop(move |w| {
                            if w.global::<NavState>().get_view() == ContentView::Playlist
                                && w.global::<PlaylistState>().get_id().to_string()
                                    == pid.to_string()
                            {
                                navigate_playlist(
                                    runtime,
                                    weak,
                                    &handle2,
                                    image_cache,
                                    pid.to_string(),
                                );
                            }
                        });
                    }
                });
            }
        });
    }

    // Playlist in-page track search (client-side filter).
    {
        let weak = window.as_weak();
        window
            .global::<PlaylistActions>()
            .on_search(move |query| {
                if let Some(w) = weak.upgrade() {
                    playlist::filter_tracks(&w, query.as_str());
                }
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<PlaylistActions>()
            .on_set_sort(move |field| {
                let Some(w) = weak.upgrade() else { return; };
                playlist::set_sort(&w, field.as_str());
                // Entering custom: load (or seed) the local order, then
                // re-render. Off-thread (opens library.db).
                if field.as_str() == "custom" {
                    let pid = w.global::<PlaylistState>().get_id().to_string();
                    if let Ok(pid) = pid.parse::<u64>() {
                        // Seed keys carry (id, is_local) — Qobuz rows then
                        // local sidecar rows, plex excluded (Tauri parity).
                        let seed = playlist::custom_seed_keys();
                        let weak = weak.clone();
                        handle.spawn(async move {
                            let orders = tokio::task::spawn_blocking(move || {
                                playlist::load_or_init_custom(pid, seed)
                            })
                            .await
                            .unwrap_or_default();
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                playlist::apply_custom_order(&w, orders);
                            });
                        });
                    }
                }
            });
    }

    // Edit playlist (rename / delete).
    {
        let weak = window.as_weak();
        window
            .global::<EditPlaylistActions>()
            .on_close(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<EditPlaylistState>().set_open(false);
                }
            });
    }
    {
        // Rename the playlist, then refresh the detail header + sidebar.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<EditPlaylistActions>()
            .on_save(move || {
                let Some(w) = weak.upgrade() else { return; };
                let es = w.global::<EditPlaylistState>();
                let name = es.get_name().to_string();
                let description = es.get_description().to_string();
                let id = es.get_id().to_string();
                if name.trim().is_empty() || es.get_busy() {
                    return;
                }
                // LOCAL playlist (id "local:<uuid>") — rename/description/
                // offline-only write the library.db repo; no Qobuz call.
                if local_playlist::is_local_id(&id) {
                    let offline_only = es.get_offline_only();
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    handle.clone().spawn(async move {
                        let nm = name.trim().to_string();
                        let ds = description.trim().to_string();
                        let lid = id.clone();
                        let (nm2, ds2) = (nm.clone(), ds.clone());
                        let ok = tokio::task::spawn_blocking(move || {
                            let desc = if ds2.is_empty() { None } else { Some(ds2.as_str()) };
                            local_playlist::update_blocking(&lid, &nm2, desc, offline_only)
                        })
                        .await
                        .unwrap_or(false);
                        if !ok {
                            log::error!("[qbz-slint] update local playlist failed");
                            return;
                        }
                        let r2 = runtime.clone();
                        let w2 = weak.clone();
                        let h2 = handle.clone();
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            let ps = w.global::<PlaylistState>();
                            // Only refresh the open detail header if this IS
                            // the open playlist.
                            if ps.get_id().as_str() == id {
                                ps.set_name(nm.into());
                                ps.set_description(ds.into());
                                ps.set_offline_only(offline_only);
                            }
                            w.global::<EditPlaylistState>().set_open(false);
                            load_sidebar_playlists(r2, w2, &h2);
                        });
                    });
                    return;
                }
                let (Ok(pid),) = (id.parse::<u64>(),) else { return; };
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    let desc_opt = Some(description.trim());
                    match runtime
                        .core()
                        .update_playlist(pid, Some(name.trim()), desc_opt, None)
                        .await
                    {
                        Ok(_) => {
                            let nm = name.trim().to_string();
                            let ds = description.trim().to_string();
                            let r2 = runtime.clone();
                            let w2 = weak.clone();
                            let h2 = handle.clone();
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                w.global::<PlaylistState>().set_name(nm.into());
                                w.global::<PlaylistState>().set_description(ds.into());
                                w.global::<EditPlaylistState>().set_open(false);
                                load_sidebar_playlists(r2, w2, &h2);
                            });
                        }
                        Err(e) => log::error!("[qbz-slint] update playlist failed: {e}"),
                    }
                });
            });
    }
    {
        // Delete the playlist, then navigate back + refresh the sidebar.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<EditPlaylistActions>()
            .on_delete(move || {
                let Some(w) = weak.upgrade() else { return; };
                let id = w.global::<EditPlaylistState>().get_id().to_string();
                // LOCAL playlist — delete the library.db entity (cascades
                // its membership rows), then back + sidebar reload.
                if local_playlist::is_local_id(&id) {
                    w.global::<EditPlaylistState>().set_busy(true);
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    handle.clone().spawn(async move {
                        let lid = id.clone();
                        let ok = tokio::task::spawn_blocking(move || {
                            local_playlist::delete_blocking(&lid)
                        })
                        .await
                        .unwrap_or(false);
                        let r2 = runtime.clone();
                        let w2 = weak.clone();
                        let h2 = handle.clone();
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            w.global::<EditPlaylistState>().set_busy(false);
                            if ok {
                                w.global::<EditPlaylistState>().set_open(false);
                                load_sidebar_playlists(r2, w2, &h2);
                                w.global::<NavState>().invoke_request_back();
                            }
                        });
                    });
                    return;
                }
                let Ok(pid) = id.parse::<u64>() else { return; };
                w.global::<EditPlaylistState>().set_busy(true);
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    match runtime.core().delete_playlist(pid).await {
                        Ok(()) => {
                            let r2 = runtime.clone();
                            let w2 = weak.clone();
                            let h2 = handle.clone();
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                w.global::<EditPlaylistState>().set_busy(false);
                                w.global::<EditPlaylistState>().set_open(false);
                                load_sidebar_playlists(r2, w2, &h2);
                                w.global::<NavState>().invoke_request_back();
                            });
                        }
                        Err(e) => {
                            log::error!("[qbz-slint] delete playlist failed: {e}");
                            let _ = weak.upgrade_in_event_loop(|w| {
                                w.global::<EditPlaylistState>().set_busy(false);
                            });
                        }
                    }
                });
            });
    }

    // Sidebar playlists — open a row, or create a new playlist.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<SidebarActions>()
            .on_open_playlist(move |id| {
                nav::record(nav::NavEntry::Playlist(id.to_string()));
                navigate_playlist(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    id.to_string(),
                );
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
            });
    }
    {
        // Populate the collapsed-sidebar folder flyout's playlist list.
        let weak = window.as_weak();
        window
            .global::<SidebarActions>()
            .on_load_folder_popup(move |folder_id| {
                if let Some(w) = weak.upgrade() {
                    sidebar::load_folder_popup(&w, folder_id.as_str());
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<SidebarActions>()
            .on_create_playlist(move || {
                if let Some(w) = weak.upgrade() {
                    use slint::Model;
                    let cps = w.global::<CreatePlaylistState>();
                    cps.set_name("".into());
                    cps.set_description("".into());
                    cps.set_is_public(false);
                    cps.set_creating(false);
                    cps.set_folder_index(0);
                    // D8: while offline, creation always produces a LOCAL
                    // playlist — the toggle shows ON and locked with a hint.
                    let offline = offline_mode::engine().is_offline();
                    cps.set_offline_only(offline);
                    cps.set_offline_locked(offline);
                    // Build the folder dropdown from the sidebar's folder
                    // list: index 0 = "No folder" (id ""), then each folder.
                    let folders = w.global::<SidebarState>().get_folders();
                    let mut opts: Vec<slint::SharedString> = vec![qbz_i18n::t("No folder").into()];
                    let mut ids: Vec<slint::SharedString> = vec!["".into()];
                    for i in 0..folders.row_count() {
                        if let Some(f) = folders.row_data(i) {
                            opts.push(f.name);
                            ids.push(f.id);
                        }
                    }
                    cps.set_folder_options(slint::ModelRc::new(slint::VecModel::from(opts)));
                    cps.set_folder_ids(slint::ModelRc::new(slint::VecModel::from(ids)));
                    cps.set_open(true);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<CreateFolderActions>()
            .on_close(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<CreateFolderState>().set_open(false);
                }
            });
    }
    {
        // Create a folder, then refresh the sidebar.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<CreateFolderActions>()
            .on_submit(move || {
                let Some(w) = weak.upgrade() else { return; };
                let name = w.global::<CreateFolderState>().get_name().to_string();
                if name.trim().is_empty() || w.global::<CreateFolderState>().get_creating() {
                    return;
                }
                w.global::<CreateFolderState>().set_creating(true);
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    let nm = name.trim().to_string();
                    tokio::task::spawn_blocking(move || {
                        folders::create_folder(&nm);
                    })
                    .await
                    .ok();
                    let r2 = runtime.clone();
                    let w2 = weak.clone();
                    let h2 = handle.clone();
                    let _ = weak.upgrade_in_event_loop(move |w| {
                        w.global::<CreateFolderState>().set_creating(false);
                        w.global::<CreateFolderState>().set_open(false);
                        load_sidebar_playlists(r2, w2, &h2);
                    });
                });
            });
    }
    {
        // Toggle a folder's expanded state (cheap, rebuilds from cache).
        let weak = window.as_weak();
        window
            .global::<SidebarActions>()
            .on_toggle_folder(move |id| {
                if let Some(w) = weak.upgrade() {
                    sidebar::toggle_folder(&w, id.as_str());
                    refresh_sidebar_covers(&w);
                }
            });
    }
    {
        // Open the create-folder modal.
        let weak = window.as_weak();
        window
            .global::<SidebarActions>()
            .on_create_folder(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<CreateFolderState>().set_name("".into());
                    w.global::<CreateFolderState>().set_creating(false);
                    w.global::<CreateFolderState>().set_open(true);
                }
            });
    }
    {
        // Delete a folder (its playlists fall back to root via the
        // library DB's ON DELETE SET NULL), then reload the sidebar.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<SidebarActions>()
            .on_delete_folder(move |id| {
                let id = id.to_string();
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    let fid = id.clone();
                    tokio::task::spawn_blocking(move || folders::delete_folder(&fid))
                        .await
                        .ok();
                    load_sidebar_playlists(runtime, weak, &handle);
                });
            });
    }
    {
        // Move a playlist into a folder ("" = root). Optimistic local
        // rebuild + a DB write.
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<SidebarActions>()
            .on_move_playlist(move |playlist_id, folder_id| {
                let Some(w) = weak.upgrade() else { return; };
                let fid = folder_id.to_string();
                // LOCAL playlists (`local:<uuid>`) persist into the
                // local_playlists.folder_id column; Qobuz ones into
                // playlist_settings. Both join the SAME shared folders.
                if local_playlist::is_local_id(&playlist_id) {
                    let id = playlist_id.to_string();
                    sidebar::move_local_playlist_local(&w, &id, &fid);
                    refresh_sidebar_covers(&w);
                    handle.spawn(async move {
                        tokio::task::spawn_blocking(move || {
                            let opt = if fid.is_empty() { None } else { Some(fid.as_str()) };
                            folders::move_local_playlist(&id, opt);
                        })
                        .await
                        .ok();
                    });
                    return;
                }
                let Ok(pid) = playlist_id.parse::<u64>() else { return; };
                sidebar::move_playlist_local(&w, pid, &fid);
                refresh_sidebar_covers(&w);
                handle.spawn(async move {
                    tokio::task::spawn_blocking(move || {
                        let opt = if fid.is_empty() { None } else { Some(fid.as_str()) };
                        folders::move_playlist(pid, opt);
                    })
                    .await
                    .ok();
                });
            });
    }
    {
        // Pick a playlist sort option (name/recent/tracks/playcount/custom).
        let weak = window.as_weak();
        window
            .global::<SidebarActions>()
            .on_set_sort(move |option| {
                if let Some(w) = weak.upgrade() {
                    sidebar::set_sort(&w, option.as_str());
                    refresh_sidebar_covers(&w);
                }
            });
    }
    {
        // Re-run the playlist-name filter as the search input edits.
        let weak = window.as_weak();
        window
            .global::<SidebarActions>()
            .on_search_changed(move |query| {
                if let Some(w) = weak.upgrade() {
                    sidebar::set_search(&w, query.as_str());
                    refresh_sidebar_covers(&w);
                }
            });
    }
    {
        // Refresh — re-fetch the playlist list from the network.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<SidebarActions>()
            .on_refresh_playlists(move || {
                load_sidebar_playlists(runtime.clone(), weak.clone(), &handle);
            });
    }
    {
        // Manage playlists — open the full Playlist Manager view.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<SidebarActions>()
            .on_manage_playlists(move || {
                nav::record(nav::NavEntry::PlaylistManager);
                playlist_manager::navigate(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                );
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
            });
    }
    {
        // Import playlist — open the importer modal fully reset, with the
        // folder dropdown rebuilt from the current sidebar folder list.
        let weak = window.as_weak();
        window
            .global::<SidebarActions>()
            .on_import_playlist(move || {
                if let Some(w) = weak.upgrade() {
                    playlist_import::open(&w);
                }
            });
    }
    {
        // Edit playlist (sidebar context menu) — open the edit-playlist
        // modal, prefilled from the cached name + description.
        let weak = window.as_weak();
        window
            .global::<SidebarActions>()
            .on_edit_playlist(move |id| {
                let Some(w) = weak.upgrade() else { return };
                let es = w.global::<EditPlaylistState>();
                // LOCAL playlist row — prefill from the sidebar's local
                // cache (name/description/offline-only).
                if local_playlist::is_local_id(id.as_str()) {
                    let (name, description, offline_only) =
                        sidebar::local_playlist_meta(id.as_str())
                            .unwrap_or_else(|| (id.to_string(), String::new(), false));
                    es.set_id(id);
                    es.set_name(name.into());
                    es.set_description(description.into());
                    es.set_is_local(true);
                    es.set_offline_only(offline_only);
                    es.set_busy(false);
                    es.set_open(true);
                    return;
                }
                let (name, description) = id
                    .parse::<u64>()
                    .ok()
                    .and_then(sidebar::playlist_name_desc)
                    .unwrap_or_else(|| (id.to_string(), String::new()));
                es.set_id(id);
                es.set_name(name.into());
                es.set_description(description.into());
                es.set_is_local(false);
                es.set_offline_only(false);
                es.set_busy(false);
                es.set_open(true);
            });
    }
    {
        // Add to Mixtape/Collection (sidebar playlist context menu) — build a
        // 1-item playlist payload from the cached SidebarEntry row + the cached
        // track count, then open the global AddToMixtapeModal. Because the
        // item_type is "playlist", `open_add_to_mixtape` computes restrict=true
        // → the picker lists mixtapes only and hides the "+ Collections" chip (a
        // playlist can't live in a Collection). Mirrors the PlaylistManager path.
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<SidebarActions>()
            .on_add_to_mixtape(move |id| {
                use slint::Model;
                let Some(w) = weak.upgrade() else { return };
                let model = w.global::<SidebarState>().get_entries();
                let Some(row) = (0..model.row_count())
                    .filter_map(|i| model.row_data(i))
                    .find(|e| e.kind == "playlist" && e.id == id)
                else {
                    return;
                };
                let artwork = row.url1.to_string();
                let item = myqbz_add::AddItem {
                    item_type: "playlist".into(),
                    source: "qobuz".into(),
                    source_item_id: id.to_string(),
                    title: row.name.to_string(),
                    subtitle: None,
                    artwork_url: (!artwork.is_empty()).then_some(artwork),
                    year: None,
                    // SidebarEntry doesn't carry the count; pull it from the
                    // sidebar cache by id (None if unknown — it's optional).
                    track_count: id
                        .parse::<u64>()
                        .ok()
                        .and_then(sidebar::playlist_track_count)
                        .map(|n| n as i32),
                };
                open_add_to_mixtape(weak.clone(), handle.clone(), vec![item]);
            });
    }
    {
        // Edit folder (sidebar context menu) — open the folder editor.
        let weak = window.as_weak();
        window
            .global::<SidebarActions>()
            .on_edit_folder(move |id| {
                let Some(w) = weak.upgrade() else { return };
                open_folder_editor(&w, id);
            });
    }
    {
        // Filter the move-to-folder menu list by a substring query.
        let weak = window.as_weak();
        window
            .global::<SidebarActions>()
            .on_search_folders(move |query| {
                if let Some(w) = weak.upgrade() {
                    sidebar::search_menu_folders(&w, query.as_str());
                }
            });
    }
    {
        // Hide playlist from the sidebar (local setting), then reload.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<SidebarActions>()
            .on_hide_playlist(move |id| {
                let Ok(pid) = id.parse::<u64>() else { return };
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    tokio::task::spawn_blocking(move || folders::set_hidden(pid, true))
                        .await
                        .ok();
                    load_sidebar_playlists(runtime, weak, &handle);
                });
            });
    }
    {
        // Hide folder from the sidebar (local setting), then reload.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<SidebarActions>()
            .on_hide_folder(move |id| {
                let fid = id.to_string();
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                handle.clone().spawn(async move {
                    tokio::task::spawn_blocking(move || folders::set_folder_hidden(&fid, true))
                        .await
                        .ok();
                    load_sidebar_playlists(runtime, weak, &handle);
                });
            });
    }

    // === Playlist Manager actions ======================================
    wire_playlist_manager(&window, &app_runtime, &tokio_rt, &image_cache);
    wire_myqbz(&window, &app_runtime, &tokio_rt, &image_cache);
    wire_myqbz_detail(&window, &app_runtime, &tokio_rt, &image_cache);
    wire_disco_builder(&window, &tokio_rt, &image_cache);
    {
        let weak = window.as_weak();
        window
            .global::<CreatePlaylistActions>()
            .on_close(move || {
                if let Some(w) = weak.upgrade() {
                    w.global::<CreatePlaylistState>().set_open(false);
                }
            });
    }
    {
        // Create the playlist, then refresh the sidebar + open it.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<CreatePlaylistActions>()
            .on_submit(move || {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                use slint::Model;
                let state = w.global::<CreatePlaylistState>();
                let name = state.get_name().to_string();
                let description = state.get_description().to_string();
                let is_public = state.get_is_public();
                // Resolve the selected folder id ("" = No folder).
                let folder_idx = state.get_folder_index();
                let folder_id = state
                    .get_folder_ids()
                    .row_data(folder_idx.max(0) as usize)
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                if name.trim().is_empty() || state.get_creating() {
                    return;
                }
                // D8: offline-only toggle ON — or the app is offline (always
                // local then) — creates a LOCAL playlist in library.db. The
                // online + toggle OFF path below stays byte-unchanged.
                let offline_now = offline_mode::engine().is_offline();
                if state.get_offline_only() || offline_now {
                    // Offline-only when the user opted in; a playlist forced
                    // local by being offline keeps the flag too (it can be
                    // unmarked later in Edit to enable "Upload to Qobuz").
                    state.set_creating(true);
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    let image_cache = image_cache.clone();
                    handle.clone().spawn(async move {
                        let nm = name.trim().to_string();
                        let ds = description.trim().to_string();
                        let created = tokio::task::spawn_blocking(move || {
                            let desc = if ds.is_empty() { None } else { Some(ds.as_str()) };
                            local_playlist::create_blocking(&nm, desc, true)
                        })
                        .await
                        .ok()
                        .flatten();
                        let weak2 = weak.clone();
                        let r2 = runtime.clone();
                        let h2 = handle.clone();
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            w.global::<CreatePlaylistState>().set_creating(false);
                            match created {
                                Some(new_id) => {
                                    w.global::<CreatePlaylistState>().set_open(false);
                                    load_sidebar_playlists(r2.clone(), weak2.clone(), &h2);
                                    nav::record(nav::NavEntry::Playlist(new_id.clone()));
                                    navigate_playlist(r2, weak2.clone(), &h2, image_cache, new_id);
                                    update_nav_flags(&w);
                                }
                                None => {
                                    log::error!("[qbz-slint] create local playlist failed");
                                }
                            }
                        });
                    });
                    return;
                }
                state.set_creating(true);
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                let image_cache = image_cache.clone();
                handle.clone().spawn(async move {
                    let desc = description.trim();
                    let desc_opt = if desc.is_empty() { None } else { Some(desc) };
                    match runtime.core().create_playlist(name.trim(), desc_opt, is_public).await {
                        Ok(playlist) => {
                            let new_id = playlist.id.to_string();
                            // Assign to the chosen folder (local DB) before
                            // the sidebar reloads, mirroring Tauri.
                            if !folder_id.is_empty() {
                                let pid = playlist.id;
                                let fid = folder_id.clone();
                                tokio::task::spawn_blocking(move || {
                                    folders::move_playlist(pid, Some(fid.as_str()));
                                })
                                .await
                                .ok();
                            }
                            let weak2 = weak.clone();
                            let r2 = runtime.clone();
                            let h2 = handle.clone();
                            let ic2 = image_cache.clone();
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                w.global::<CreatePlaylistState>().set_creating(false);
                                w.global::<CreatePlaylistState>().set_open(false);
                                load_sidebar_playlists(r2.clone(), weak2.clone(), &h2);
                                nav::record(nav::NavEntry::Playlist(new_id.clone()));
                                navigate_playlist(r2, weak2.clone(), &h2, ic2, new_id);
                                update_nav_flags(&w);
                            });
                        }
                        Err(e) => {
                            log::error!("[qbz-slint] create playlist failed: {e}");
                            let _ = weak.upgrade_in_event_loop(|w| {
                                w.global::<CreatePlaylistState>().set_creating(false);
                            });
                        }
                    }
                });
            });
    }

    // ---- HiFi Wizard (DAC setup) — Slice 6 (check step) ----
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<DacWizardActions>().on_open(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            dac_wizard::open_immediate(&w);
            // Probe the audio stack off the UI thread; fill the check step when done.
            let weak2 = w.as_weak();
            handle.spawn_blocking(move || {
                let health = qbz_audio::audio_stack_health();
                let _ = weak2.upgrade_in_event_loop(move |w| {
                    dac_wizard::apply_health(&w, health);
                });
            });
        });
    }
    {
        let weak = window.as_weak();
        window
            .global::<DacWizardActions>()
            .on_set_distro(move |index| {
                if let Some(w) = weak.upgrade() {
                    dac_wizard::set_distro(&w, index);
                }
            });
    }
    {
        let weak = window.as_weak();
        window.global::<DacWizardActions>().on_set_init(move |index| {
            if let Some(w) = weak.upgrade() {
                dac_wizard::set_init(&w, index);
            }
        });
    }
    {
        // Enumerate DACs (Slice 7) off the UI thread when entering the step.
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<DacWizardActions>().on_run_detect(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            dac_wizard::begin_detect(&w);
            let weak2 = w.as_weak();
            handle.spawn_blocking(move || {
                let data = dac_wizard::detect_blocking();
                let _ = weak2.upgrade_in_event_loop(move |w| {
                    dac_wizard::apply_candidates(&w, data);
                });
            });
        });
    }
    {
        let weak = window.as_weak();
        window.global::<DacWizardActions>().on_toggle_dac(move |i| {
            if let Some(w) = weak.upgrade() {
                dac_wizard::toggle_dac(&w, i);
            }
        });
    }
    {
        let weak = window.as_weak();
        window
            .global::<DacWizardActions>()
            .on_validate_manual(move |t| {
                if let Some(w) = weak.upgrade() {
                    dac_wizard::validate_manual(&w, t.as_str());
                }
            });
    }
    {
        // Self-service playback test (Slice 9): resolve the 4 curated tracks
        // (id-hint then "artist title" search), route output to the DAC under
        // test, and play them. The N6 read-back is driven by on_poll_test.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<DacWizardActions>().on_start_test(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            dac_wizard::begin_test(&w);
            let runtime = runtime.clone();
            let weak2 = w.as_weak();
            let play_handle = handle.clone();
            handle.spawn(async move {
                let mut tracks: Vec<qbz_models::Track> = Vec::new();
                for seed in dac_wizard::TEST_SEEDS.iter() {
                    let mut chosen = match runtime.core().get_track(seed.id_hint).await {
                        Ok(t) if dac_wizard::track_matches_seed(&t, seed) => Some(t),
                        _ => None,
                    };
                    if chosen.is_none() {
                        let q = format!("{} {}", seed.artist, seed.title);
                        if let Ok(page) = runtime.core().search_tracks(&q, 10, 0, None).await {
                            chosen = page
                                .items
                                .into_iter()
                                .find(|t| dac_wizard::track_matches_seed(t, seed));
                        }
                    }
                    if let Some(t) = chosen {
                        tracks.push(t);
                    }
                }
                // Keep the resolved tracks so the user can jump between them.
                dac_wizard::stash_test_tracks(tracks.clone());
                let runtime2 = runtime.clone();
                let _ = weak2.upgrade_in_event_loop(move |w| {
                    if tracks.is_empty() {
                        w.global::<DacWizardState>()
                            .set_test_requested_label("Couldn't load the test tracks (offline?)".into());
                        return;
                    }
                    crate::playback::play_tracks(runtime2, w.as_weak(), play_handle, tracks, 0);
                });
            });
        });
    }
    {
        // Poll the requested vs negotiated rate while the test plays.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<DacWizardActions>().on_poll_test(move || {
            if weak.upgrade().is_none() {
                return;
            }
            let runtime = runtime.clone();
            let weak2 = weak.clone();
            handle.spawn_blocking(move || {
                let player = runtime.core().player();
                let req_rate = player.state.get_sample_rate();
                let req_bits = player.state.get_bit_depth();
                let negotiated = qbz_audio::negotiated_active_rate();
                let _ = weak2.upgrade_in_event_loop(move |w| {
                    dac_wizard::apply_poll(&w, req_rate, req_bits, negotiated);
                });
            });
        });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        window.global::<DacWizardActions>().on_stop_test(move || {
            let _ = runtime.core().pause();
            if let Some(w) = weak.upgrade() {
                dac_wizard::end_test(&w);
            }
        });
    }
    {
        // Jump straight to one of the test tracks (skip the long waits) by
        // re-setting the queue at that index via the working play path.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<DacWizardActions>()
            .on_test_play_index(move |i| {
                let tracks = dac_wizard::test_tracks();
                if tracks.is_empty() {
                    return;
                }
                let start = (i.max(0) as usize).min(tracks.len().saturating_sub(1));
                crate::playback::play_tracks(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    tracks,
                    start,
                );
            });
    }
    {
        // "Use my own music": start the read-back without queuing the test
        // tracks — the user plays whatever they want; the poll reads the rate.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<DacWizardActions>().on_verify_own(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let runtime = runtime.clone();
            let weak2 = w.as_weak();
            handle.spawn(async move {
                // Guardrail: don't start a read-back on an empty queue.
                let (tracks, _) = runtime.core().get_all_queue_tracks().await;
                let empty = tracks.is_empty();
                if !empty {
                    let _ = runtime.core().resume();
                }
                let _ = weak2.upgrade_in_event_loop(move |w| {
                    if empty {
                        dac_wizard::queue_empty_notice(&w);
                    } else {
                        dac_wizard::begin_test(&w);
                    }
                });
            });
        });
    }
    {
        // Generate the per-DAC copy-paste config (Slice 10): re-probe rates off
        // the UI thread, then fill the review step.
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<DacWizardActions>().on_gen_configs(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let dacs = dac_wizard::checked_dacs(&w);
            let weak2 = w.as_weak();
            handle.spawn_blocking(move || {
                let data = dac_wizard::gen_configs_blocking(dacs);
                let _ = weak2.upgrade_in_event_loop(move |w| {
                    dac_wizard::apply_configs(&w, data);
                });
            });
        });
    }
    {
        let weak = window.as_weak();
        window.global::<DacWizardActions>().on_toggle_config(move |i| {
            if let Some(w) = weak.upgrade() {
                dac_wizard::toggle_config(&w, i);
            }
        });
    }
    {
        window
            .global::<DacWizardActions>()
            .on_copy_command(move |cmd| {
                share::copy_to_clipboard(cmd.to_string());
            });
    }

    // ---- Playlist Importer (public playlists) — spec §3.3 ----
    {
        // No cancel exists: a running import task continues to completion
        // (§1.8); closing only hides the modal.
        let weak = window.as_weak();
        window.global::<PlaylistImportActions>().on_close(move || {
            if let Some(w) = weak.upgrade() {
                w.global::<PlaylistImportState>().set_open(false);
            }
        });
    }
    {
        // Provider detection per keystroke (Slint 1.16 has no `.contains`).
        let weak = window.as_weak();
        window
            .global::<PlaylistImportActions>()
            .on_url_edited(move |text| {
                if let Some(w) = weak.upgrade() {
                    playlist_import::on_url_edited(&w, text.as_str());
                }
            });
    }
    {
        window
            .global::<PlaylistImportActions>()
            .on_name_edited(move |text| {
                playlist_import::on_name_edited(text.as_str());
            });
    }
    {
        // Step A: fetch the preview (no session needed).
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window.global::<PlaylistImportActions>().on_fetch(move || {
            let Some(w) = weak.upgrade() else { return; };
            let Some(url) = playlist_import::begin_fetch(&w) else {
                return;
            };
            // A reopen mid-fetch bumps the generation; the stale preview
            // result must not land on the fresh modal (§1.8).
            let generation = playlist_import::current_generation();
            let weak = weak.clone();
            handle.spawn(async move {
                let res = qbz_playlist_import::preview_public_playlist(&url).await;
                let _ = weak.upgrade_in_event_loop(move |w| {
                    if generation != playlist_import::current_generation() {
                        return;
                    }
                    match res {
                        Ok(p) => playlist_import::apply_preview_ok(&w, &url, p),
                        Err(e) => playlist_import::apply_preview_err(&w, &e.to_string()),
                    }
                });
            });
        });
    }
    {
        // Step B: execute the import (re-fetches the URL internally —
        // Tauri behavior, kept) with live sink progress.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<PlaylistImportActions>()
            .on_execute(move || {
                let Some(w) = weak.upgrade() else { return; };
                let Some(args) = playlist_import::begin_execute(&w) else {
                    return;
                };
                let runtime = runtime.clone();
                let weak = weak.clone();
                let handle = handle.clone();
                let image_cache = image_cache.clone();
                handle.clone().spawn(async move {
                    // Tauri's RequiresUserSession gate: execute needs a
                    // logged-in client (the preview does not).
                    let client = runtime.core().client().read().await.clone();
                    let Some(client) = client else {
                        let g = args.generation;
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            if g == playlist_import::current_generation() {
                                playlist_import::apply_execute_err(
                                    &w,
                                    "Not logged in to Qobuz",
                                );
                            }
                            toast::show(&w, "Playlist import failed", ToastKind::Error);
                        });
                        return;
                    };
                    let sink: Arc<dyn qbz_playlist_import::ImportProgressSink> = Arc::new(
                        playlist_import::SlintSink::new(weak.clone(), args.generation),
                    );
                    let res = qbz_playlist_import::import_public_playlist(
                        &args.url,
                        &client,
                        args.name_override.as_deref(),
                        false, // is_public — Tauri hardcodes false, no toggle
                        sink,
                    )
                    .await;
                    match res {
                        Ok(summary) => {
                            // reco: NOT logged. The importer is a bulk external
                            // import (Spotify/Apple/...), not a per-track taste
                            // action, and Tauri never logged it — left unlogged
                            // for parity. (Re-evaluate if the owner wants it.)
                            // Assign every created part to the chosen folder
                            // (local DB) BEFORE the sidebar reload — mirrors
                            // the create-playlist precedent above.
                            if !args.folder_id.is_empty() {
                                for pid in &summary.qobuz_playlist_ids {
                                    let (pid, fid) = (*pid, args.folder_id.clone());
                                    tokio::task::spawn_blocking(move || {
                                        folders::move_playlist(pid, Some(fid.as_str()));
                                    })
                                    .await
                                    .ok();
                                }
                            }
                            let g = args.generation;
                            let weak2 = weak.clone();
                            let r2 = runtime.clone();
                            let h2 = handle.clone();
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                // Toast + sidebar refresh fire even after a
                                // close mid-import (§1.8); the generation
                                // guard keeps a stale run's writes off a
                                // reopened modal's fresh state.
                                if g == playlist_import::current_generation() {
                                    playlist_import::apply_execute_ok(&w, &summary);
                                }
                                if summary.matched_tracks > 0 {
                                    toast::show(&w, "Playlist imported", ToastKind::Success);
                                }
                                load_sidebar_playlists(r2.clone(), weak2.clone(), &h2);
                                if let Some(first) = summary.qobuz_playlist_ids.first() {
                                    // Navigate only while the modal is still
                                    // open AND this run is current (§1.8).
                                    if g == playlist_import::current_generation()
                                        && w.global::<PlaylistImportState>().get_open()
                                    {
                                        nav::record(nav::NavEntry::Playlist(first.to_string()));
                                        navigate_playlist(
                                            r2,
                                            weak2,
                                            &h2,
                                            image_cache,
                                            first.to_string(),
                                        );
                                        update_nav_flags(&w);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            let g = args.generation;
                            let msg = e.to_string();
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                if g == playlist_import::current_generation() {
                                    playlist_import::apply_execute_err(&w, &msg);
                                }
                                toast::show(&w, "Playlist import failed", ToastKind::Error);
                            });
                        }
                    }
                });
            });
    }

    // handler.
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<FavoritesActions>()
            .on_select_tab(move |id| {
                let Some(tab) = favorites::FavTab::from_tab_id(id.as_str()) else {
                    // Playlists / Labels: just switch the visible tab,
                    // their content is not implemented yet.
                    if let Some(w) = weak.upgrade() {
                        w.global::<FavoritesState>().set_active_tab(id);
                    }
                    return;
                };
                // Each favorites tab is its own history page (mirrors the
                // Discover tabs) so back/forward moves between them.
                nav::record(nav::NavEntry::Favorites { tab: id.to_string() });
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
                navigate_favorites(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    tab,
                    id.as_str(),
                );
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_open_album(move |id| {
                if let Some(w) = weak.upgrade() {
                    w.invoke_open_album(id);
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_open_artist(move |id| {
                if let Some(w) = weak.upgrade() {
                    w.invoke_open_artist(id);
                }
            });
    }
    {
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<FavoritesActions>()
            .on_open_label(move |id, name| {
                let Ok(label_id) = id.parse::<u64>() else {
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
            });
    }
    {
        // Favorite playlist click — open the playlist detail view.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<FavoritesActions>()
            .on_open_playlist(move |id| {
                nav::record(nav::NavEntry::Playlist(id.to_string()));
                navigate_playlist(
                    runtime.clone(),
                    weak.clone(),
                    &handle,
                    image_cache.clone(),
                    id.to_string(),
                );
                if let Some(w) = weak.upgrade() {
                    update_nav_flags(&w);
                }
            });
    }
    {
        // Switch the Playlists sub-tab (Library / Following) + re-derive.
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_playlists_set_sub_tab(move |sub| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>().set_playlists_sub_tab(sub);
                    favorites::derive_playlists(&w);
                }
            });
    }
    {
        // Local search over the loaded favorite playlists (name | owner).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_search_playlists(move |q| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>().set_playlists_search(q);
                    favorites::derive_playlists(&w);
                }
            });
    }
    {
        // Playlists grid/list view toggle (persisted).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_playlists_set_view(move |v| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>().set_playlists_view_mode(v);
                    favorites_prefs::save(&w);
                }
            });
    }
    {
        // Local search over the loaded favorite artists (name).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_search_artists(move |q| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>().set_artists_search(q);
                    favorites::derive_artists(&w);
                }
            });
    }
    {
        // Artists header Shuffle = open a random visible artist (random
        // ARTIST, not a random album — matches Tauri).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_artists_shuffle(move || {
                if let Some(w) = weak.upgrade() {
                    if let Some(id) = favorites::random_visible_artist(&w) {
                        w.invoke_open_artist(id.into());
                    }
                }
            });
    }
    {
        // Playlists "random" — play a random visible playlist (reuses the
        // playlist-action "play" path).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_playlists_random(move || {
                if let Some(w) = weak.upgrade() {
                    if let Some(id) = favorites::random_visible_playlist(&w) {
                        w.global::<FavoritesActions>()
                            .invoke_playlist_action(id.into(), "play".into());
                    }
                }
            });
    }
    {
        // Labels "random" — open a random visible label's landing.
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_labels_random(move || {
                if let Some(w) = weak.upgrade() {
                    if let Some((id, name)) = favorites::random_visible_label(&w) {
                        w.global::<FavoritesActions>()
                            .invoke_open_label(id.into(), name.into());
                    }
                }
            });
    }
    {
        // Group the favorite artists (off / A-Z) — persisted.
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_artists_set_group(move |g| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>()
                        .set_artists_group_enabled(g == "alpha");
                    favorites::derive_artists(&w);
                    favorites_prefs::save(&w);
                }
            });
    }
    {
        // Artists grid <-> sidepanel view toggle (persisted). Switching back to
        // grid clears the sidepanel selection (matches Tauri).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_artists_set_view(move |v| {
                if let Some(w) = weak.upgrade() {
                    let st = w.global::<FavoritesState>();
                    st.set_artists_view_mode(v.clone());
                    if v == "grid" {
                        st.set_selected_artist_id("".into());
                    }
                    // Rebuild grouped/alpha for the new mode (the sidepanel
                    // left list is always grouped).
                    favorites::derive_artists(&w);
                    favorites_prefs::save(&w);
                }
            });
    }
    {
        // Sidepanel: load + show the selected artist's albums, reusing the
        // standalone artist page's /artist/page release_type classifier.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<FavoritesActions>()
            .on_select_artist(move |id, name| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let st = w.global::<FavoritesState>();
                st.set_selected_artist_id(id.clone());
                st.set_selected_artist_name(name);
                st.set_selected_albums_loading(true);
                st.set_selected_albums_error("".into());
                let runtime = runtime.clone();
                let weak2 = weak.clone();
                let image_cache = image_cache.clone();
                let id_s = id.to_string();
                handle.spawn(async move {
                    match artist::load_artist(&runtime, &id_s).await {
                        Ok(data) => {
                            let sections = data.release_sections;
                            let jobs = favorites::selected_artist_artwork_jobs(&sections);
                            let _ = weak2.upgrade_in_event_loop(move |w| {
                                favorites::apply_selected_artist(&w, sections);
                            });
                            artwork::spawn_loads(jobs, weak2.clone(), image_cache.clone());
                        }
                        Err(e) => {
                            log::error!("[qbz-slint] sidepanel artist {id_s} load failed: {e}");
                            let _ = weak2.upgrade_in_event_loop(move |w| {
                                let st = w.global::<FavoritesState>();
                                st.set_selected_albums_loading(false);
                                st.set_selected_albums_error(e.into());
                            });
                        }
                    }
                });
            });
    }
    {
        // Playlist card actions: play / play-next / queue / share / favorite.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<FavoritesActions>()
            .on_playlist_action(move |id, action| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                match action.as_str() {
                    "share" => share::copy_to_clipboard(share::qobuz_playlist_url(&id)),
                    "favorite" => {
                        // Library sub-tab: un-favorite in place (remove the
                        // local favorite + drop the row). Following sub-tab:
                        // add to the local Library (per user decision).
                        let library = w
                            .global::<FavoritesState>()
                            .get_playlists_sub_tab()
                            .to_string()
                            != "following";
                        if let Ok(pid) = id.parse::<u64>() {
                            let fav = !library;
                            handle.spawn_blocking(move || {
                                crate::library_db::with_db(|db| db.set_playlist_favorite(pid, fav));
                            });
                        }
                        if library {
                            favorites::remove_playlist_row(&w, &id);
                        }
                    }
                    act => {
                        // play / play-next / queue: fetch the playlist's tracks,
                        // then play or enqueue.
                        let Ok(pid) = id.parse::<u64>() else {
                            return;
                        };
                        let runtime = runtime.clone();
                        let weak2 = weak.clone();
                        let handle2 = handle.clone();
                        let act = act.to_string();
                        handle.spawn(async move {
                            let tracks = match runtime.core().get_playlist(pid).await {
                                Ok(p) => p.tracks.map(|t| t.items).unwrap_or_default(),
                                Err(e) => {
                                    log::error!("[qbz-slint] playlist {pid} load failed: {e}");
                                    return;
                                }
                            };
                            if tracks.is_empty() {
                                return;
                            }
                            match act.as_str() {
                                "play-next" => {
                                    playback::enqueue_tracks(runtime, handle2, tracks, true)
                                }
                                "queue" => {
                                    playback::enqueue_tracks(runtime, handle2, tracks, false)
                                }
                                _ => {
                                    playback::play_tracks(runtime, weak2, handle2, tracks, 0);
                                }
                            }
                        });
                    }
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_play_track(move |id| {
                if let Some(w) = weak.upgrade() {
                    w.invoke_media_action("track".into(), id, "play".into());
                }
            });
    }
    {
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_track_action(move |id, action| {
                if let Some(w) = weak.upgrade() {
                    w.invoke_media_action("track".into(), id, action);
                }
            });
    }
    {
        // Favorite album card actions (play / queue / favorite / go-to)
        // route through the album media-action arms.
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_album_action(move |id, action| {
                if let Some(w) = weak.upgrade() {
                    w.invoke_media_action("album".into(), id, action);
                }
            });
    }
    {
        // Local search over the loaded favorite albums (title / artist).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_albums_search(move |q| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>().set_albums_search(q);
                    favorites::derive_albums(&w);
                }
            });
    }
    {
        // Sort the favorite albums (default / title / artist).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_albums_set_sort(move |s| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>().set_albums_sort_by(s);
                    favorites::derive_albums(&w);
                    favorites_prefs::save(&w);
                }
            });
    }
    {
        // Albums grid/list view toggle (persisted).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_albums_set_view(move |v| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>().set_albums_view_mode(v);
                    favorites_prefs::save(&w);
                }
            });
    }
    {
        // Group the favorite albums (off / alpha / artist).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_albums_set_group(move |g| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>().set_albums_group_mode(g);
                    favorites::derive_albums(&w);
                    favorites_prefs::save(&w);
                }
            });
    }
    {
        // Play a random album from the visible favorites set.
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_albums_shuffle(move || {
                if let Some(w) = weak.upgrade() {
                    if let Some(id) = favorites::random_visible_album(&w) {
                        w.invoke_media_action("album".into(), id.into(), "play".into());
                    }
                }
            });
    }
    {
        // Un-favorite a track from the favorites list: fade the row, remove
        // the favorite on the server, then drop the row after the fade.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<FavoritesActions>()
            .on_unfavorite_track(move |id| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                // Offline = read-only hearts (spec 4.3).
                if offline_mode::engine().is_offline() {
                    toast::info(&w, "Not available offline");
                    return;
                }
                favorites::mark_track_removing(&w, &id);
                if let Ok(tid) = id.parse::<u64>() {
                    crate::fav_cache::set(tid, false);
                }
                let id_srv = id.to_string();
                let runtime = runtime.clone();
                handle.spawn(async move {
                    if let Err(e) = runtime.core().remove_favorite("track", &id_srv).await {
                        log::error!("[qbz-slint] unfavorite track {id_srv} failed: {e}");
                    }
                });
                let weak2 = weak.clone();
                let id_rm = id.to_string();
                slint::Timer::single_shot(std::time::Duration::from_millis(280), move || {
                    if let Some(w) = weak2.upgrade() {
                        favorites::remove_track_row(&w, &id_rm);
                    }
                });
            });
    }
    {
        // Un-favorite an album from the favorites list (same fade + remove).
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<FavoritesActions>()
            .on_unfavorite_album(move |id| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                favorites::mark_album_removing(&w, &id);
                // Keep the favorite-album cache in sync so the album-header
                // heart reflects an unfavorite done from the Favorites view.
                crate::fav_cache::set_album(&id, false);
                let id_srv = id.to_string();
                let runtime = runtime.clone();
                handle.spawn(async move {
                    if let Err(e) = runtime.core().remove_favorite("album", &id_srv).await {
                        log::error!("[qbz-slint] unfavorite album {id_srv} failed: {e}");
                    }
                });
                let weak2 = weak.clone();
                let id_rm = id.to_string();
                slint::Timer::single_shot(std::time::Duration::from_millis(280), move || {
                    if let Some(w) = weak2.upgrade() {
                        favorites::remove_album_row(&w, &id_rm);
                    }
                });
            });
    }
    {
        // Retry loading the current favorites tab after a load error.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<FavoritesActions>()
            .on_retry_load(move || {
                if let Some(w) = weak.upgrade() {
                    let tab_id = w.global::<FavoritesState>().get_active_tab().to_string();
                    if let Some(tab) = favorites::FavTab::from_tab_id(&tab_id) {
                        navigate_favorites(
                            runtime.clone(),
                            weak.clone(),
                            &handle,
                            image_cache.clone(),
                            tab,
                            &tab_id,
                        );
                    }
                }
            });
    }
    {
        // Local search over the loaded favorite tracks (title / artist /
        // album), re-deriving the rendered list.
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_search_tracks(move |q| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>().set_tracks_search(q);
                    favorites::derive_tracks(&w);
                }
            });
    }
    {
        // Local search over the loaded favorite labels (name).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_search_labels(move |q| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>().set_labels_search(q);
                    favorites::derive_labels(&w);
                }
            });
    }
    {
        // Group the favorite tracks (off / album / artist / name).
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_tracks_set_group(move |g| {
                if let Some(w) = weak.upgrade() {
                    w.global::<FavoritesState>().set_tracks_group_mode(g);
                    favorites::derive_tracks(&w);
                    favorites_prefs::save(&w);
                }
            });
    }
    {
        // Play all favorite tracks as a fresh queue.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<FavoritesActions>()
            .on_play_all_tracks(move || {
                playback::play_tracks(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    favorites::play_tracks(),
                    0,
                );
            });
    }
    {
        // Shuffle-play the favorite tracks.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        window
            .global::<FavoritesActions>()
            .on_shuffle_tracks(move || {
                playback::play_tracks(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    favorites::shuffled_tracks(),
                    0,
                );
            });
    }
    {
        // Enter / leave the tracks multi-select edit mode.
        let weak = window.as_weak();
        window
            .global::<FavoritesActions>()
            .on_toggle_multi_select(move || {
                if let Some(w) = weak.upgrade() {
                    let on = w.global::<FavoritesState>().get_tracks_multi_select();
                    favorites::set_multi_select(&w, !on);
                }
            });
    }
    {
        // Bulk bar actions over the selected favorite tracks.
        let runtime = app_runtime.clone();
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<FavoritesActions>()
            .on_bulk_action(move |action| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                match action.as_str() {
                    "select-all" => favorites::select_all(&w),
                    "clear" => favorites::clear_selection(&w),
                    "queue" => {
                        let tracks = favorites::selected_tracks(&w);
                        playback::enqueue_tracks(runtime.clone(), handle.clone(), tracks, false);
                    }
                    "play-next" => {
                        let tracks = favorites::selected_tracks(&w);
                        playback::enqueue_tracks(runtime.clone(), handle.clone(), tracks, true);
                    }
                    "make-offline" => {
                        let tracks = favorites::selected_tracks(&w);
                        offline_cache::cache_tracks(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            tracks,
                        );
                        favorites::clear_selection(&w);
                    }
                    "add-to-playlist" => {
                        let ids = favorites::selected_ids(&w);
                        if !ids.is_empty() {
                            playlist_picker::open_multi(&w, &ids, false);
                            let runtime = runtime.clone();
                            let weak = weak.clone();
                            handle.spawn(async move {
                                let playlists = playlist_picker::load(&runtime).await;
                                let _ = weak.upgrade_in_event_loop(move |w| {
                                    playlist_picker::apply(&w, playlists);
                                });
                            });
                        }
                    }
                    "remove-selected" => {
                        // Offline = read-only hearts (spec 4.3).
                        if offline_mode::engine().is_offline() {
                            toast::info(&w, "Not available offline");
                            return;
                        }
                        let ids = favorites::selected_ids(&w);
                        if ids.is_empty() {
                            return;
                        }
                        let runtime = runtime.clone();
                        let weak = weak.clone();
                        let handle = handle.clone();
                        let image_cache = image_cache.clone();
                        handle.clone().spawn(async move {
                            for id in &ids {
                                if let Err(e) =
                                    runtime.core().remove_favorite("track", id).await
                                {
                                    log::error!(
                                        "[qbz-slint] bulk remove favorite {id} failed: {e}"
                                    );
                                }
                                if let Ok(tid) = id.parse::<u64>() {
                                    crate::fav_cache::set(tid, false);
                                }
                            }
                            let _ = weak.upgrade_in_event_loop(|w| {
                                favorites::set_multi_select(&w, false);
                            });
                            navigate_favorites(
                                runtime.clone(),
                                weak.clone(),
                                &handle,
                                image_cache.clone(),
                                favorites::FavTab::Tracks,
                                "tracks",
                            );
                        });
                    }
                    _ => {}
                }
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
                    // Offline: serve the shared disk-cache copy; never attempt
                    // the download.
                    if offline_mode::engine().is_offline() {
                        match artwork::cached_path_for(&url) {
                            Some(path) => {
                                if let Err(e) = tokio::fs::copy(&path, dest.path()).await {
                                    log::error!(
                                        "[qbz-slint] artwork save-as offline copy: {e}"
                                    );
                                }
                            }
                            None => log::warn!(
                                "[qbz-slint] artwork save-as skipped offline: not in disk cache"
                            ),
                        }
                        return;
                    }
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
        let image_cache = image_cache.clone();
        window
            .global::<ArtworkActions>()
            .on_add_custom(move |kind, key| {
                let kind = kind.to_string();
                let key = key.to_string();
                let weak = weak.clone();
                let image_cache = image_cache.clone();
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
                            // Decode + apply immediately so the new image shows
                            // without a reload — critical for artists with no
                            // Qobuz portrait (e.g. Vicky Psarakis), where there
                            // is no network artwork to fall back on.
                            let decoded = artwork::fetch_and_decode_ref(
                                &qbz_models::ArtworkRef::LocalFile(path.clone()),
                                &image_cache,
                                440,
                            )
                            .await;
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                w.global::<ArtistState>().set_has_custom_image(true);
                                if let Some((pixels, iw, ih)) = decoded {
                                    artist::apply_artwork(&w, &pixels, iw, ih);
                                }
                            });
                        }
                        "album" => {
                            custom_artwork::set_album_cover(&key, &path);
                            let decoded = artwork::fetch_and_decode_ref(
                                &qbz_models::ArtworkRef::LocalFile(path.clone()),
                                &image_cache,
                                448,
                            )
                            .await;
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                w.global::<AlbumState>().set_has_custom_cover(true);
                                if let Some((pixels, iw, ih)) = decoded {
                                    album::apply_artwork(&w, &pixels, iw, ih);
                                }
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

    window.on_close_app({
        let weak = window.as_weak();
        move || {
            // Custom titlebar close button. Hide to tray when close-to-tray is
            // enabled and the tray is live; otherwise quit.
            if tray_settings::get().close_to_tray && tray::handle().is_some() {
                log::info!("[qbz-slint] close-to-tray (titlebar): hiding to tray");
                // Flush the session even when only hiding — the process may be
                // killed from the tray / shell without a real quit afterwards.
                session_persist::save_on_exit();
                tray::hide_window(&weak);
            } else {
                log::info!("[qbz-slint] closing");
                // Flush the final session snapshot before quitting.
                session_persist::save_on_exit();
                let _ = slint::quit_event_loop();
            }
        }
    });

    // Intercept the window-manager close (native titlebar X / compositor
    // close). Mirrors the custom titlebar: hide to tray when close-to-tray is
    // on + the tray is live, otherwise quit. Required because the loop runs
    // with quit_on_last_window_closed = false (so a tray-hide keeps the app
    // alive) — without this, the native close would leave a headless process.
    let cast_exit_handle = tokio_rt.handle().clone();
    window.window().on_close_requested(move || {
        let settings = tray_settings::get();
        if settings.close_to_tray && tray::handle().is_some() {
            // Slint performs the hide (destroys the surface) for HideWindow;
            // we only sync the shown flag so the next tray toggle shows it.
            log::info!("[qbz-slint] close-to-tray (WM close): hiding to tray");
            // Flush the session even when only hiding — the process may be
            // killed from the tray / shell without a real quit afterwards.
            session_persist::save_on_exit();
            tray::set_window_shown(false);
            // macOS: drop the Dock icon if the user opted in (no-op elsewhere).
            if settings.mac_hide_dock {
                tray::set_mac_dock_hidden(true);
            }
            slint::CloseRequestResponse::HideWindow
        } else {
            log::info!("[qbz-slint] WM close requested: quitting");
            // Flush the final session snapshot before quitting.
            session_persist::save_on_exit();
            // Best-effort: stop the renderer + media server so a cast device
            // doesn't keep playing after the app quits (Tauri parity, #32).
            if let Some(cast) = cast_service::service() {
                cast_exit_handle.block_on(async move {
                    cast.shutdown().await;
                });
            }
            let _ = slint::quit_event_loop();
            slint::CloseRequestResponse::HideWindow
        }
    });

    window.on_open_tos(|| {
        dispatch(AppCommand::OpenTermsOfService);
        if let Err(e) = open::that(QOBUZ_TOS_URL) {
            log::error!("[qbz-slint] failed to open Terms of Service: {e}");
        }
    });

    log::info!("[qbz-slint] window ready");
    // NOT `window.run()`: that quits the event loop when the last window
    // closes, which would kill the app the moment the window hides to tray.
    // `run_event_loop_until_quit()` keeps the loop alive until an explicit
    // `quit_event_loop()` (custom titlebar / WM close when not close-to-tray /
    // tray Quit), so hide-to-tray works.
    window.show()?;
    slint::run_event_loop_until_quit()?;
    Ok(())
}
