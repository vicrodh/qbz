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
mod album_map;
mod artist;
mod artwork;
mod auth;
mod commands;
mod custom_artwork;
mod dates;
mod discover_browse;
mod discovery_dismiss;
mod fav_cache;
mod favorites;
mod favorites_prefs;
mod foryou;
mod genre_filter;
mod home;
mod label;
mod location_view;
mod mix;
mod musician;
mod nav;
mod play_history;
mod strip_html;
mod playback;
mod queue;
mod drag;
mod folders;
mod library_db;
mod local_library;
mod offline;
mod offline_cache;
mod offline_manager;
mod playlist;
mod playlist_manager;
mod playlist_picker;
mod recently;
mod search;
mod settings;
mod share;
mod sidebar;
mod toast;
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
    // Bind the local library DB to this user (folders / playlist
    // settings live in the per-user library.db).
    library_db::set_user(session.user_id);

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

    // Load the sidebar playlists list.
    load_sidebar_playlists(runtime.clone(), weak.clone(), &tokio::runtime::Handle::current());

    // Warm the shared favorite-track cache so track rows can show the
    // correct heart state from their first paint (album / artist / search
    // / playlist / mix / favorites all read it). Background-fetched once.
    {
        let runtime = runtime.clone();
        tokio::spawn(async move {
            match runtime.core().favorite_track_ids().await {
                Ok(ids) => fav_cache::set_all(ids),
                Err(e) => log::warn!("[qbz-slint] favorite cache load failed: {e}"),
            }
        });
    }

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

    // Load the genre-filter parents + persisted selection, then seed
    // the popup state. Done before the discover load so the first
    // fetch honors a remembered genre selection.
    genre_filter::load_parents(&runtime).await;
    let _ = weak.upgrade_in_event_loop(|w| {
        genre_filter::apply_state(&w);
    });

    reload_home(&runtime, &weak, &image_cache, "home".to_string()).await;

    // Seed the favorites tab counts so the badges are ready before the
    // user opens each tab (they otherwise only count on first visit).
    let counts = favorites::load_counts(&runtime).await;
    let _ = weak.upgrade_in_event_loop(move |w| {
        favorites::apply_counts(&w, counts);
    });
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
            // Artwork for the active tab's section set (Section-targeted,
            // so it lands in HomeState.sections once select_tab swaps it
            // in below). Built before `data` is moved. For You renders
            // from its own view, so it has no discover-index section
            // set here.
            let empty: Vec<home::SectionData> = Vec::new();
            let active_set = match active_tab.as_str() {
                "editorPicks" => &data.editor_sections,
                "forYou" => &empty,
                _ => &data.sections,
            };
            let mut jobs = home::section_artwork_jobs(active_set);
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
            jobs.extend(data.recent_albums.iter().enumerate().filter_map(|(idx, card)| {
                (!card.artwork_url.is_empty()).then(|| artwork::ArtworkJob {
                    target: artwork::ArtworkTarget::RecentAlbum { idx },
                    url: card.artwork_url.clone(),
                })
            }));

            let weak_for_artwork = weak.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                home::apply_home(&w, data);
                // apply_home shows the home set; swap to the requested
                // tab (no-op when it is "home").
                home::select_tab(&w, &active_tab);
                w.global::<HomeState>().set_loading(false);
            });
            artwork::spawn_loads(jobs, weak_for_artwork, image_cache.clone());
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
    let runtime = runtime.clone();
    let weak = weak.clone();
    let image_cache = image_cache.clone();
    handle.spawn(async move {
        let data = foryou::load_for_you(&runtime).await;
        let jobs = foryou::artwork_jobs(&data);
        let _ = weak.upgrade_in_event_loop(move |w| {
            foryou::apply_for_you(&w, &data);
        });
        artwork::spawn_loads(jobs, weak, image_cache);
    });
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

/// Open a LOCAL album's detail in the shared AlbumPageView: load its tracks
/// (metadata-grouped), populate AlbumState with `is-local` set, then resolve
/// the folder/embedded cover from disk. `group_key` is the album's metadata
/// group key.
fn navigate_local_album(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    group_key: String,
) {
    let _ = runtime;
    handle.spawn(async move {
        let _ = weak.upgrade_in_event_loop(|w| {
            album::reset_album(&w);
            w.global::<NavState>().set_view(ContentView::Album);
        });
        let gk = group_key.clone();
        let tracks = tokio::task::spawn_blocking(move || {
            let mut t = local_library::fetch_album_tracks_blocking(&gk);
            playback::fill_missing_covers(&mut t);
            t
        })
        .await
        .unwrap_or_default();
        let cover = tracks
            .iter()
            .find_map(|t| t.artwork_path.clone())
            .unwrap_or_default();
        let gk2 = group_key.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            local_library::apply_local_album(&w, &gk2, tracks);
        });
        if !cover.is_empty() {
            let art = qbz_models::ArtworkRef::LocalFile(cover);
            if let Some((px, ww, hh)) =
                artwork::fetch_and_decode_ref(&art, &image_cache, 448).await
            {
                let _ = weak.upgrade_in_event_loop(move |w| {
                    album::apply_artwork(&w, &px, ww, hh);
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

/// Open a Local Library browse tab (Albums / Artists / Folders / Tracks).
///
/// Sets the active tab + switches the view, then lazily loads the tab's data
/// on first visit. Albums is the first slice (chunked grid); the other tabs
/// render their shell + a placeholder until their slices land.
fn navigate_local_library(
    _runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
    tab: local_library::LibTab,
) {
    let tab_id = tab.tab_id().to_string();
    let _ = weak.upgrade_in_event_loop(move |w| {
        w.global::<LocalLibraryState>().set_active_tab(tab_id.into());
        w.global::<NavState>().set_view(ContentView::LocalLibrary);
    });
    // Lazy per-tab load on first visit.
    match tab {
        local_library::LibTab::Albums => {
            local_library::ensure_albums_loaded(weak, handle.clone(), image_cache);
        }
        local_library::LibTab::Folders => {
            local_library::ensure_folders_loaded(weak, handle.clone(), image_cache);
        }
        local_library::LibTab::Tracks => {
            local_library::ensure_tracks_loaded(weak, handle.clone());
        }
        local_library::LibTab::Artists => {
            local_library::ensure_artists_loaded(weak, handle.clone());
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
    let Ok(id) = playlist_id.parse::<u64>() else {
        log::warn!("[qbz-slint] navigate_playlist: bad id {playlist_id}");
        return;
    };
    handle.spawn(async move {
        let active = playlist_id.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            playlist::reset(&w);
            sidebar::set_active(&w, &active);
            w.global::<NavState>().set_view(ContentView::Playlist);
        });
        if let Some(data) = playlist::load(&runtime, id).await {
            let jobs = playlist::artwork_jobs(&data);
            let pid = data.id.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                playlist::apply(&w, data);
                let owned = sidebar::contains(&w, &pid);
                w.global::<PlaylistState>().set_is_owner(owned);
            });
            artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
        }
    });
}

/// Resolve the track ids for a drag started on `track_id`. If the
/// current view has a multi-selection that includes the dragged row
/// (and is >1), the whole selection is dragged; otherwise just the
/// row. Mirrors Tauri's group-drag rule.
fn gather_drag_ids(w: &AppWindow, track_id: &str) -> Vec<u64> {
    use slint::Model;
    let view = w.global::<NavState>().get_view();
    let model = match view {
        ContentView::Playlist => Some(w.global::<PlaylistState>().get_tracks()),
        ContentView::Artist => Some(w.global::<ArtistState>().get_top_tracks()),
        _ => None,
    };
    if let Some(model) = model {
        let selected: Vec<u64> = (0..model.row_count())
            .filter_map(|i| model.row_data(i))
            .filter(|t| t.selected)
            .filter_map(|t| t.id.parse::<u64>().ok())
            .collect();
        if selected.len() > 1 && selected.iter().any(|&id| id.to_string() == track_id) {
            return selected;
        }
    }
    track_id.parse::<u64>().map(|id| vec![id]).unwrap_or_default()
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
        let jobs = sidebar::artwork_jobs(window);
        if !jobs.is_empty() {
            artwork::spawn_loads(jobs, window.as_weak(), cache);
        }
    }
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
        // Add-to-mixtape — the mixtape system is not yet ported to Slint;
        // log and no-op (matches the deferred mixtape surface).
        window
            .global::<PlaylistManagerActions>()
            .on_add_to_mixtape(move |id| {
                log::info!("[qbz-slint] add-to-mixtape not yet implemented (playlist {id})");
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
                                crate::toast::success_weak(&weak, "Added to favorites");
                            }
                            Err(e) => {
                                log::error!("[qbz-slint] favorite album failed: {e}");
                                crate::toast::error_weak(&weak, "Couldn't add to favorites");
                            }
                        }
                    });
                }
                ("album", "cache") => offline_cache::cache_album(
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
                ("track", "favorite") => {
                    // Toggle (not just add): read the cached state, flip it
                    // optimistically across every visible track model + the
                    // shared cache, then add/remove on the network.
                    let was_fav = fav_cache::is_favorite(&id);
                    let make_fav = !was_fav;
                    if let Ok(track_id) = id.parse::<u64>() {
                        fav_cache::set(track_id, make_fav);
                    }
                    if let Some(w) = weak.upgrade() {
                        set_row_favorite(&w, &id, make_fav);
                    }
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let track_id = id.clone();
                    handle.spawn(async move {
                        let res = if make_fav {
                            runtime.core().add_favorite("track", &track_id).await
                        } else {
                            runtime.core().remove_favorite("track", &track_id).await
                        };
                        if let Err(e) = res {
                            log::error!("[qbz-slint] toggle track favorite failed: {e}");
                            // Roll the optimistic change back on failure.
                            if let Ok(tid) = track_id.parse::<u64>() {
                                fav_cache::set(tid, was_fav);
                            }
                            let _ = weak.upgrade_in_event_loop(move |w| {
                                set_row_favorite(&w, &track_id, was_fav);
                            });
                        }
                    });
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
                ("track", "create-radio") => playback::play_track_radio(
                    runtime.clone(),
                    weak.clone(),
                    handle.clone(),
                    id.clone(),
                ),
                ("track", "add-to-playlist") => {
                    // Open the global picker for this track + load the
                    // user's playlists.
                    let Some(w) = weak.upgrade() else {
                        return;
                    };
                    playlist_picker::open(&w, &id);
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    handle.spawn(async move {
                        let playlists = playlist_picker::load(&runtime).await;
                        let _ = weak.upgrade_in_event_loop(move |w| {
                            playlist_picker::apply(&w, playlists);
                        });
                    });
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
                ("track", "toggle-select") => {
                    // Flip `selected` on the matching row, in whichever
                    // multi-select surface is showing: the playlist detail,
                    // the artist Popular Tracks, or the label Popular Tracks.
                    if let Some(w) = weak.upgrade() {
                        let model = match w.global::<NavState>().get_view() {
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
                ("mix-track", track_id) => {
                    let idx = mix::index_of(track_id);
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    handle.clone().spawn(async move {
                        let tracks = mix::current_tracks();
                        playback::play_tracks(runtime, weak, handle, tracks, idx);
                    });
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
                ("playlist", "play-all") => {
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    handle.clone().spawn(async move {
                        let tracks = playlist::current_tracks();
                        playback::play_tracks(runtime, weak, handle, tracks, 0);
                    });
                }
                ("playlist", "shuffle") => {
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    handle.clone().spawn(async move {
                        let tracks = playlist::shuffled_tracks();
                        playback::play_tracks(runtime, weak, handle, tracks, 0);
                    });
                }
                ("playlist", "favorite") => {
                    // Favorite/unfavorite the open playlist.
                    if let Some(w) = weak.upgrade() {
                        let pid = w.global::<PlaylistState>().get_id().to_string();
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
                ("playlist", "remove-selected") => {
                    if let Some(w) = weak.upgrade() {
                        let pid = w.global::<PlaylistState>().get_id().to_string();
                        let ids = playlist::selected_ids(&w);
                        if let (Ok(pid), false) = (pid.parse::<u64>(), ids.is_empty()) {
                            let runtime = runtime.clone();
                            let weak = weak.clone();
                            let handle = handle.clone();
                            let image_cache = image_cache.clone();
                            handle.clone().spawn(async move {
                                match runtime
                                    .core()
                                    .remove_tracks_from_playlist(pid, &ids)
                                    .await
                                {
                                    Ok(()) => {
                                        // Reload the playlist + leave edit mode.
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
                                    }
                                    Err(e) => log::error!(
                                        "[qbz-slint] remove tracks from playlist failed: {e}"
                                    ),
                                }
                            });
                        }
                    }
                }
                ("playlist", "set-artwork") => {
                    // Pick an image, copy it into the artwork cache, store
                    // it as the playlist's custom cover, then reload.
                    if let Some(w) = weak.upgrade() {
                        let pid = w.global::<PlaylistState>().get_id().to_string();
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
                        let es = w.global::<EditPlaylistState>();
                        es.set_id(pid);
                        es.set_name(name);
                        es.set_description(desc);
                        es.set_open(true);
                    }
                }
                ("track", "move-up") | ("track", "move-down") => {
                    // Custom-order reorder (playlist view). Optimistic UI
                    // move, then persist the full order off-thread.
                    if let Some(w) = weak.upgrade() {
                        let up = action == "move-up";
                        let orders = playlist::move_track(&w, id.as_str(), up);
                        let pid = w.global::<PlaylistState>().get_id().to_string();
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
                ("playlist-track", track_id) => {
                    let idx = playlist::index_of(track_id);
                    let runtime = runtime.clone();
                    let weak = weak.clone();
                    let handle = handle.clone();
                    handle.clone().spawn(async move {
                        let tracks = playlist::current_tracks();
                        playback::play_tracks(runtime, weak, handle, tracks, idx);
                    });
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
                // Management/maintenance/danger actions live under
                // Settings > Local Library. TODO(locallibrary): pre-open the
                // Local Library settings sub-page via target-tab routing once
                // that page exists.
                nav::record(nav::NavEntry::Settings);
                if let Some(w) = weak.upgrade() {
                    w.global::<NavState>().set_view(ContentView::Settings);
                    update_nav_flags(&w);
                }
            });
    }

    // Local Library — Albums tab controls (search / sort re-query page 1;
    // load-more pages on scroll; retry) + the shared AlbumCollectionView's
    // open / per-card actions (album-detail + playback land with later slices).
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<LocalLibraryActions>()
            .on_albums_search(move |_query| {
                // The query is two-way bound to albums-search; reload page 1.
                local_library::reload_albums(weak.clone(), handle.clone(), image_cache.clone());
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<LocalLibraryActions>()
            .on_albums_set_sort(move |sort| {
                if let Some(w) = weak.upgrade() {
                    w.global::<LocalLibraryState>().set_albums_sort(sort);
                }
                local_library::reload_albums(weak.clone(), handle.clone(), image_cache.clone());
            });
    }
    {
        let weak = window.as_weak();
        let handle = tokio_rt.handle().clone();
        let image_cache = image_cache.clone();
        window
            .global::<LocalLibraryActions>()
            .on_albums_load_more(move || {
                local_library::load_more_albums(weak.clone(), handle.clone(), image_cache.clone());
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
        window
            .global::<LocalLibraryActions>()
            .on_open_artist(move |id| {
                // TODO(locallibrary): open the local Artists tab / artist.
                log::debug!("[qbz-slint] local artist open (Artists slice pending): {id}");
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
                if action.as_str() == "play" {
                    if let Ok(row_id) = id.parse::<i64>() {
                        // Queue the current (searched) list so playback
                        // continues down it from the clicked row.
                        let query = weak
                            .upgrade()
                            .map(|w| {
                                w.global::<LocalLibraryState>()
                                    .get_tracks_search()
                                    .to_string()
                            })
                            .unwrap_or_default();
                        playback::play_local_tracks_from(
                            runtime.clone(),
                            weak.clone(),
                            handle.clone(),
                            query,
                            row_id,
                        );
                    }
                } else {
                    // play-next / queue land with a later slice.
                    log::debug!("[qbz-slint] local track action (queue slice pending): {id} {action}");
                }
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
        window
            .global::<PlaylistPickerActions>()
            .on_pick(move |playlist_id| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let picker = w.global::<PlaylistPickerState>();
                // Bulk add (favorites) carries track-ids; single add carries
                // track-id.
                let ids_model = picker.get_track_ids();
                let mut ids: Vec<u64> = (0..ids_model.row_count())
                    .filter_map(|i| ids_model.row_data(i))
                    .filter_map(|s| s.parse::<u64>().ok())
                    .collect();
                if ids.is_empty() {
                    if let Ok(tid) = picker.get_track_id().parse::<u64>() {
                        ids.push(tid);
                    }
                }
                picker.set_open(false);
                let (Ok(pid), false) = (playlist_id.parse::<u64>(), ids.is_empty()) else {
                    return;
                };
                let runtime = runtime.clone();
                handle.spawn(async move {
                    if let Err(e) = runtime.core().add_tracks_to_playlist(pid, &ids).await {
                        log::error!("[qbz-slint] add to playlist failed: {e}");
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
                    w.global::<PlaylistPickerState>().set_open(false);
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
                let ids = gather_drag_ids(&w, track_id.as_str());
                drag::set_dragged(ids.clone());
                let ds = w.global::<DragState>();
                ds.set_count(ids.len() as i32);
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
        window.global::<DragActions>().on_end(move || {
            let Some(w) = weak.upgrade() else { return };
            let ds = w.global::<DragState>();
            let pid = ds.get_over_playlist_id().to_string();
            ds.set_active(false);
            ds.set_over_playlist_id("".into());
            let ids = drag::dragged();
            drag::clear();
            if let (Ok(pid), false) = (pid.parse::<u64>(), ids.is_empty()) {
                let runtime = runtime.clone();
                handle.spawn(async move {
                    match runtime.core().add_tracks_to_playlist(pid, &ids).await {
                        Ok(()) => log::info!(
                            "[qbz-slint] dropped {} track(s) onto playlist {pid}",
                            ids.len()
                        ),
                        Err(e) => log::error!("[qbz-slint] drop add to playlist failed: {e}"),
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
                        let ids = playlist::current_track_ids();
                        let weak = weak.clone();
                        handle.spawn(async move {
                            let orders = tokio::task::spawn_blocking(move || {
                                playlist::load_or_init_custom(pid, ids)
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
                    // Build the folder dropdown from the sidebar's folder
                    // list: index 0 = "No folder" (id ""), then each folder.
                    let folders = w.global::<SidebarState>().get_folders();
                    let mut opts: Vec<slint::SharedString> = vec!["No folder".into()];
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
                let Ok(pid) = playlist_id.parse::<u64>() else { return; };
                let fid = folder_id.to_string();
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
        // Edit playlist (sidebar context menu) — open the edit-playlist
        // modal, prefilled from the cached name + description.
        let weak = window.as_weak();
        window
            .global::<SidebarActions>()
            .on_edit_playlist(move |id| {
                let Some(w) = weak.upgrade() else { return };
                let (name, description) = id
                    .parse::<u64>()
                    .ok()
                    .and_then(sidebar::playlist_name_desc)
                    .unwrap_or_else(|| (id.to_string(), String::new()));
                let es = w.global::<EditPlaylistState>();
                es.set_id(id);
                es.set_name(name.into());
                es.set_description(description.into());
                es.set_busy(false);
                es.set_open(true);
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
                            playlist_picker::open_multi(&w, &ids);
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
