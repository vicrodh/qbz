//! First-class LOCAL playlists — Slint glue (offline-mode port, D7/D8).
//!
//! Storage lives in the shared per-user `library.db`
//! (`qbz_library::local_playlists`, ids `local:<uuid>`). This module routes
//! everything id-prefixed `local:` away from the Qobuz-bound playlist paths:
//! the detail view renders from the local repo through the SAME
//! `PlaylistState` + `playlist.rs` row machinery (search / sort /
//! multi-select / artwork reuse), playback builds `QueueTrack`s from the
//! resolvable rows, and an offline-only playlist stamps the queue
//! (`QbzCore::set_queue_offline_only`) so the QConnect push site skips the
//! cloud (D8: NOTHING from an offline-only playlist ever reaches Qobuz).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_library::local_playlists as repo;
use qbz_models::QueueTrack;
use slint::{ComponentHandle, Model};

use crate::adapter::SlintAdapter;
use crate::artwork::{self, ArtworkJob, ArtworkTarget, ImageCache};
use crate::playback::{after_track_change, refresh_sidebar};
use crate::{AppWindow, ContentView, NavState, PlaylistState, TrackItem};

type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Type guard (D7): a playlist reference is EITHER a Qobuz `u64` id or a
/// `local:<uuid>` string — Qobuz-bound calls take `u64` only, so a Local ref
/// is unrepresentable there by construction.
#[derive(Debug, Clone)]
pub enum PlaylistRef {
    Qobuz(u64),
    Local(String),
}

impl PlaylistRef {
    pub fn parse(id: &str) -> Option<Self> {
        if repo::is_local_playlist_id(id) {
            Some(Self::Local(id.to_string()))
        } else {
            id.parse::<u64>().ok().map(Self::Qobuz)
        }
    }
}

/// True when `id` names a local playlist.
pub fn is_local_id(id: &str) -> bool {
    repo::is_local_playlist_id(id)
}

// ──────────────────────── blocking repo wrappers ────────────────────────
// All open the per-user library.db fresh on the calling (blocking) thread
// via `library_db::with_db` — never call on the UI/event-loop thread.

pub fn list_blocking() -> Vec<repo::LocalPlaylist> {
    crate::library_db::with_db(|db| Ok(db.with_connection(repo::list)))
        .and_then(|r| r.ok())
        .unwrap_or_default()
}

pub fn get_blocking(id: &str) -> Option<repo::LocalPlaylist> {
    crate::library_db::with_db(|db| Ok(db.with_connection(|conn| repo::get(conn, id))))
        .and_then(|r| r.ok())
        .flatten()
}

pub fn get_tracks_blocking(id: &str) -> Vec<repo::LocalPlaylistTrack> {
    crate::library_db::with_db(|db| Ok(db.with_connection(|conn| repo::get_tracks(conn, id))))
        .and_then(|r| r.ok())
        .unwrap_or_default()
}

pub fn create_blocking(name: &str, description: Option<&str>, offline_only: bool) -> Option<String> {
    crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| repo::create(conn, name, description, offline_only)))
    })
    .and_then(|r| r.ok())
}

pub fn update_blocking(id: &str, name: &str, description: Option<&str>, offline_only: bool) -> bool {
    crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| {
            repo::rename(conn, id, name)?;
            repo::set_description(conn, id, description)?;
            repo::set_offline_only(conn, id, offline_only)
        }))
    })
    .map(|r| r.is_ok())
    .unwrap_or(false)
}

pub fn delete_blocking(id: &str) -> bool {
    crate::library_db::with_db(|db| Ok(db.with_connection(|conn| repo::delete(conn, id))))
        .map(|r| r.is_ok())
        .unwrap_or(false)
}

/// Append Qobuz track ids. Returns inserted count.
pub fn add_qobuz_tracks_blocking(id: &str, track_ids: &[u64]) -> usize {
    let entries: Vec<repo::LocalPlaylistTrackInput> = track_ids
        .iter()
        .map(|&tid| repo::LocalPlaylistTrackInput::Qobuz(tid))
        .collect();
    crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| repo::add_tracks(conn, id, &entries)))
    })
    .and_then(|r| r.ok())
    .unwrap_or(0)
}

/// Append LocalLibrary rows (by `local_tracks` row id), source-aware:
/// offline copies (`qobuz_download`) become Qobuz refs (real catalog id),
/// Plex rows become Plex refs (rating key in `file_path`), everything else
/// a local file path. Returns inserted count.
pub fn add_local_rows_blocking(id: &str, row_ids: &[i64]) -> usize {
    let entries: Vec<repo::LocalPlaylistTrackInput> = crate::library_db::with_db(|db| {
        let mut out = Vec::new();
        for &rid in row_ids {
            let Some(track) = db.get_track(rid)? else {
                log::warn!("[qbz-slint] local playlist add: unknown local row {rid}");
                continue;
            };
            let input = match track.source.as_deref() {
                Some("qobuz_download") => match track.qobuz_track_id {
                    Some(qid) => repo::LocalPlaylistTrackInput::Qobuz(qid as u64),
                    None => repo::LocalPlaylistTrackInput::Local(track.file_path.clone()),
                },
                Some("plex") => repo::LocalPlaylistTrackInput::Plex(track.file_path.clone()),
                _ => repo::LocalPlaylistTrackInput::Local(track.file_path.clone()),
            };
            out.push(input);
        }
        Ok(out)
    })
    .unwrap_or_default();
    if entries.is_empty() {
        return 0;
    }
    crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| repo::add_tracks(conn, id, &entries)))
    })
    .and_then(|r| r.ok())
    .unwrap_or(0)
}

/// Copy `src` into the artwork cache and store it as this local playlist's
/// custom artwork (mirrors `playlist::set_custom_artwork` for Qobuz ones).
/// Returns the stored path. Blocking.
pub fn set_custom_artwork_blocking(id: &str, src: &str) -> Option<String> {
    let cache = crate::library_db::artwork_cache_dir()?;
    std::fs::create_dir_all(&cache).ok()?;
    let ext = std::path::Path::new(src)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let suffix = id.trim_start_matches(repo::LOCAL_PLAYLIST_PREFIX);
    let dest = cache.join(format!("local_playlist_{suffix}_{ts}.{ext}"));
    if let Err(e) = std::fs::copy(src, &dest) {
        log::error!("[qbz-slint] copy local playlist artwork failed: {e}");
        return None;
    }
    let dest_str = dest.to_string_lossy().to_string();
    match crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| repo::set_custom_artwork(conn, id, Some(&dest_str))))
    }) {
        Some(Ok(())) => Some(dest_str),
        Some(Err(e)) => {
            log::error!("[qbz-slint] store local playlist artwork failed: {e}");
            None
        }
        None => None,
    }
}

/// Clear this local playlist's custom artwork. Blocking.
pub fn clear_custom_artwork_blocking(id: &str) {
    crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| repo::set_custom_artwork(conn, id, None)))
    });
}

// ──────────────────────── detail view (load/apply) ────────────────────────

/// The open local playlist's playable queue snapshot, aligned with the row
/// `TrackItem.id`s (`QueueTrack.id.to_string()`), plus per-item repo
/// positions for removal. Mirrors `playlist.rs::CURRENT` for Qobuz lists.
static CURRENT_QUEUE: LazyLock<Mutex<Vec<QueueTrack>>> = LazyLock::new(|| Mutex::new(Vec::new()));
/// (playlist id, offline_only) of the open local playlist detail.
static CURRENT_META: LazyLock<Mutex<Option<(String, bool)>>> = LazyLock::new(|| Mutex::new(None));
/// Row TrackItem id -> repo `position` (for remove-selected).
static ROW_POSITIONS: LazyLock<Mutex<HashMap<String, i32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// One resolved, renderable row (Send — built on the worker).
pub enum RowItem {
    /// Full catalog track (online fetch).
    Qobuz(Box<qbz_models::Track>),
    /// Offline-cache metadata (D11: a Qobuz row renders offline ONLY when
    /// this metadata source exists; un-cached rows are filtered out).
    Cached {
        track_id: u64,
        title: String,
        artist: String,
        album: String,
        duration_secs: u64,
        bit_depth: Option<u32>,
        sample_rate: Option<f64>,
        /// On-disk cover thumb (B5: index `artwork_path` / CMAF `cover.jpg`),
        /// loaded through the local-file artwork path like Local rows.
        artwork_path: Option<String>,
    },
    /// Local file resolved from library.db by path.
    Local(Box<qbz_library::LocalTrack>),
    /// Plex ref — no detailed resolve in v1 (rendered basic, not playable).
    Plex { key: String },
}

pub struct LoadedRow {
    pub position: i32,
    pub item: RowItem,
}

pub struct LocalPlaylistData {
    pub id: String,
    pub name: String,
    pub description: String,
    pub offline_only: bool,
    pub custom_artwork_path: Option<String>,
    pub rows: Vec<LoadedRow>,
}

fn mmss(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn total_duration_label(rows: &[LoadedRow]) -> String {
    let secs: u64 = rows
        .iter()
        .map(|r| match &r.item {
            RowItem::Qobuz(t) => t.duration as u64,
            RowItem::Cached { duration_secs, .. } => *duration_secs,
            RowItem::Local(t) => t.duration_secs,
            RowItem::Plex { .. } => 0,
        })
        .sum();
    let mins = secs / 60;
    if mins >= 60 {
        format!("{} h {} min", mins / 60, mins % 60)
    } else {
        format!("{} min", mins)
    }
}

/// Load + resolve a local playlist off the UI thread. Qobuz rows resolve
/// via `get_tracks_batch` when online, via the offline-cache index when
/// offline (or when the batch fails); local rows via library.db by path;
/// Plex rows stay basic. Unresolvable rows are filtered out (D11) and
/// counted in `hidden_unavailable`.
pub async fn load(runtime: &Runtime, playlist_id: &str) -> Option<LocalPlaylistData> {
    let id = playlist_id.to_string();
    let (header, tracks) = tokio::task::spawn_blocking({
        let id = id.clone();
        move || (get_blocking(&id), get_tracks_blocking(&id))
    })
    .await
    .ok()?;
    let header = header?;

    let offline = crate::offline_mode::engine().is_offline();

    // Qobuz rows: one batch fetch when online; cached-metadata fallback for
    // everything the batch did not return (and for the whole set offline).
    let qobuz_ids: Vec<u64> = tracks.iter().filter_map(|t| t.qobuz_track_id).collect();
    let mut fetched: HashMap<u64, qbz_models::Track> = HashMap::new();
    if !offline && !qobuz_ids.is_empty() {
        match runtime.core().get_tracks_batch(&qobuz_ids).await {
            Ok(list) => {
                for t in list {
                    fetched.insert(t.id, t);
                }
            }
            Err(e) => {
                log::warn!("[qbz-slint] local playlist {id}: qobuz batch failed: {e}");
            }
        }
    }
    let missing: Vec<u64> = qobuz_ids
        .iter()
        .copied()
        .filter(|tid| !fetched.contains_key(tid))
        .collect();
    let mut cached: HashMap<u64, RowItem> = HashMap::new();
    if !missing.is_empty() {
        if let Some(off) = crate::offline::get().await {
            let cache_path = off.get_cache_path();
            let guard = off.db.lock().await;
            if let Some(db) = guard.as_ref() {
                for tid in &missing {
                    if let Ok(Some(info)) = db.get_track(*tid) {
                        if matches!(info.status, qbz_offline_cache::OfflineCacheStatus::Ready) {
                            let artwork_path = info.resolve_cover_path(&cache_path);
                            cached.insert(
                                *tid,
                                RowItem::Cached {
                                    track_id: info.track_id,
                                    title: info.title,
                                    artist: info.artist,
                                    album: info.album.unwrap_or_default(),
                                    duration_secs: info.duration_secs,
                                    bit_depth: info.bit_depth,
                                    sample_rate: info.sample_rate,
                                    artwork_path,
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    // Local rows: resolve library rows by file path (blocking).
    let local_paths: Vec<String> = tracks.iter().filter_map(|t| t.local_path.clone()).collect();
    let locals: HashMap<String, qbz_library::LocalTrack> = if local_paths.is_empty() {
        HashMap::new()
    } else {
        tokio::task::spawn_blocking(move || {
            crate::library_db::with_db(|db| {
                let mut out = HashMap::new();
                for path in &local_paths {
                    if let Some(track) = db.get_track_by_path(path)? {
                        out.insert(path.clone(), track);
                    }
                }
                Ok(out)
            })
            .unwrap_or_default()
        })
        .await
        .unwrap_or_default()
    };

    let mut rows: Vec<LoadedRow> = Vec::new();
    let mut hidden = 0usize;
    for t in tracks {
        let item = match t.source {
            repo::LocalPlaylistTrackSource::Qobuz => {
                let Some(tid) = t.qobuz_track_id else {
                    hidden += 1;
                    continue;
                };
                if let Some(track) = fetched.remove(&tid) {
                    RowItem::Qobuz(Box::new(track))
                } else if let Some(item) = cached.remove(&tid) {
                    item
                } else {
                    // D11: no metadata source for this Qobuz row right now.
                    hidden += 1;
                    continue;
                }
            }
            repo::LocalPlaylistTrackSource::Local => {
                let Some(track) = t.local_path.as_ref().and_then(|p| locals.get(p)) else {
                    hidden += 1;
                    continue;
                };
                RowItem::Local(Box::new(track.clone()))
            }
            repo::LocalPlaylistTrackSource::Plex => RowItem::Plex {
                key: t.plex_key.clone().unwrap_or_default(),
            },
        };
        rows.push(LoadedRow {
            position: t.position,
            item,
        });
    }
    if hidden > 0 {
        log::info!("[qbz-slint] local playlist {id}: {hidden} row(s) unavailable, hidden (D11)");
    }

    Some(LocalPlaylistData {
        id: header.id,
        name: header.name,
        description: header.description.unwrap_or_default(),
        offline_only: header.offline_only,
        custom_artwork_path: header.custom_artwork_path.filter(|p| !p.is_empty()),
        rows,
    })
}

/// Build the queue track for a resolved row, if it is playable.
fn row_queue_track(item: &RowItem) -> Option<QueueTrack> {
    match item {
        RowItem::Qobuz(track) => {
            let (album_id, album_title, album_artwork) = track
                .album
                .as_ref()
                .map(|a| {
                    (
                        a.id.clone(),
                        a.title.clone(),
                        a.image.best().cloned().unwrap_or_default(),
                    )
                })
                .unwrap_or_default();
            let album_artist = track
                .performer
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_default();
            Some(crate::playback::make_queue_track(
                track,
                &album_id,
                &album_title,
                &album_artist,
                &album_artwork,
            ))
        }
        RowItem::Cached {
            track_id,
            title,
            artist,
            album,
            duration_secs,
            bit_depth,
            sample_rate,
            ..
        } => Some(QueueTrack {
            id: *track_id,
            title: title.clone(),
            version: None,
            artist: artist.clone(),
            album: album.clone(),
            duration_secs: *duration_secs,
            artwork_url: None,
            hires: bit_depth.map(|d| d >= 24).unwrap_or(false),
            bit_depth: *bit_depth,
            sample_rate: *sample_rate,
            is_local: false,
            album_id: None,
            artist_id: None,
            streamable: true,
            // Plain "qobuz": the play tier-walk serves the offline-cache hit.
            source: Some("qobuz".to_string()),
            parental_warning: false,
            source_item_id_hint: None,
        }),
        RowItem::Local(track) => Some(crate::playback::local_queue_track(track)),
        RowItem::Plex { .. } => None,
    }
}

/// Build the display row for a resolved item. `queue` (when playable)
/// dictates the row id so visible-order playback maps 1:1.
fn row_item(item: &RowItem, queue: Option<&QueueTrack>) -> TrackItem {
    match item {
        RowItem::Qobuz(track) => crate::playlist::to_item(track),
        RowItem::Cached {
            track_id,
            title,
            artist,
            album,
            duration_secs,
            bit_depth,
            sample_rate,
            artwork_path,
        } => TrackItem {
            id: track_id.to_string().into(),
            number: "".into(),
            title: title.clone().into(),
            artist: artist.clone().into(),
            album: album.clone().into(),
            duration: mmss(*duration_secs).into(),
            quality_tier: match bit_depth {
                Some(d) if *d >= 24 => "hires",
                Some(_) => "cd",
                None => "",
            }
            .into(),
            quality_detail: crate::quality::detail(*bit_depth, *sample_rate).into(),
            explicit: false,
            selected: false,
            artwork_url: artwork_path.clone().unwrap_or_default().into(),
            artwork: slint::Image::default(),
            is_favorite: crate::fav_cache::is_favorite(&track_id.to_string()),
            artist_id: "".into(),
            album_id: "".into(),
            removing: false,
            cache_status: 3,
            cache_progress: 0.0,
            source: "qobuz".into(),
            unlocking: false,
        },
        RowItem::Local(track) => {
            let (tier, quality_detail, _) = crate::quality::badge(
                &track.format.to_string(),
                track.bit_depth,
                Some(track.sample_rate),
            );
            TrackItem {
                // The queue id (library row id; the Qobuz id for offline
                // copies) so visible-order playback resolves this row.
                id: queue
                    .map(|q| q.id.to_string())
                    .unwrap_or_else(|| track.id.to_string())
                    .into(),
                number: "".into(),
                title: track.title.clone().into(),
                artist: track.artist.clone().into(),
                album: track.album.clone().into(),
                duration: mmss(track.duration_secs).into(),
                quality_tier: tier.into(),
                quality_detail: quality_detail.into(),
                explicit: false,
                selected: false,
                artwork_url: track.artwork_path.clone().unwrap_or_default().into(),
                artwork: slint::Image::default(),
                is_favorite: false,
                artist_id: "".into(),
                album_id: "".into(),
                removing: false,
                cache_status: 0,
                cache_progress: 0.0,
                source: match track.source.as_deref() {
                    Some("qobuz_download") => "qobuz",
                    Some("plex") => "plex",
                    _ => "local",
                }
                .into(),
                unlocking: false,
            }
        }
        RowItem::Plex { key } => TrackItem {
            id: format!("plex:{key}").into(),
            number: "".into(),
            title: "Plex track".into(),
            artist: "Plex".into(),
            album: "".into(),
            duration: "".into(),
            quality_tier: "".into(),
            quality_detail: "".into(),
            explicit: false,
            selected: false,
            artwork_url: "".into(),
            artwork: slint::Image::default(),
            is_favorite: false,
            artist_id: "".into(),
            album_id: "".into(),
            removing: false,
            cache_status: 0,
            cache_progress: 0.0,
            source: "plex".into(),
            unlocking: false,
        },
    }
}

/// Build the queue snapshot + display rows + id->position map for a resolved
/// row list. Shared by the LOCAL detail [`apply`] and the offline MIXED
/// detail ([`apply_qobuz_offline`]).
fn build_row_models(
    rows: &[LoadedRow],
) -> (Vec<QueueTrack>, Vec<TrackItem>, HashMap<String, i32>) {
    let mut queue: Vec<QueueTrack> = Vec::new();
    let mut items: Vec<TrackItem> = Vec::with_capacity(rows.len());
    let mut positions: HashMap<String, i32> = HashMap::new();
    for row in rows {
        let qt = row_queue_track(&row.item);
        let item = row_item(&row.item, qt.as_ref());
        positions.insert(item.id.to_string(), row.position);
        if let Some(qt) = qt {
            queue.push(qt);
        }
        items.push(item);
    }
    (queue, items, positions)
}

/// Apply loaded data into `PlaylistState` (header + rows through the shared
/// `playlist.rs` row machinery) and snapshot the playable queue. UI thread.
pub fn apply(window: &AppWindow, data: LocalPlaylistData) {
    let (queue, items, positions) = build_row_models(&data.rows);

    if let Ok(mut cur) = CURRENT_QUEUE.lock() {
        *cur = queue;
    }
    if let Ok(mut meta) = CURRENT_META.lock() {
        *meta = Some((data.id.clone(), data.offline_only));
    }
    if let Ok(mut pos) = ROW_POSITIONS.lock() {
        *pos = positions;
    }

    let duration = total_duration_label(&data.rows);
    let state = window.global::<PlaylistState>();
    state.set_id(data.id.into());
    state.set_name(data.name.into());
    state.set_owner(if data.offline_only {
        "Offline-only playlist"
    } else {
        "Local playlist"
    }
    .into());
    let description = crate::strip_html::strip_html(&data.description);
    state.set_description(description.clone().into());
    state.set_description_short(description.into());
    state.set_is_local(true);
    state.set_offline_only(data.offline_only);
    state.set_is_owner(true);
    // Custom artwork (local file) or the row-collage fallback.
    let custom = data
        .custom_artwork_path
        .as_ref()
        .filter(|p| std::path::Path::new(p).exists())
        .and_then(|p| slint::Image::load_from_path(std::path::Path::new(p)).ok());
    if let Some(img) = custom {
        state.set_cover(img);
        state.set_cover_url(data.custom_artwork_path.clone().unwrap_or_default().into());
        state.set_has_custom(true);
    } else {
        state.set_cover_url("".into());
        state.set_has_custom(false);
    }
    state.set_total_duration(duration.into());
    crate::playlist::apply_local_items(window, items);
}

/// Row artwork jobs — Qobuz rows have http URLs, local rows file paths.
/// Returns (http jobs, local-file jobs) targeting `PlaylistTrack{index}`
/// (the same target the Qobuz detail uses; indexes are FULL_ITEMS order).
pub fn artwork_jobs(rows: &[LoadedRow]) -> (Vec<ArtworkJob>, Vec<ArtworkJob>) {
    let mut http = Vec::new();
    let mut local = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        match &row.item {
            RowItem::Qobuz(track) => {
                if let Some(url) = track.album.as_ref().and_then(|a| a.image.smallest().cloned()) {
                    http.push(ArtworkJob {
                        url,
                        target: ArtworkTarget::PlaylistTrack { index },
                    });
                }
            }
            RowItem::Local(track) => {
                if let Some(path) = track.artwork_path.clone().filter(|p| !p.is_empty()) {
                    local.push(ArtworkJob {
                        url: path,
                        target: ArtworkTarget::PlaylistTrack { index },
                    });
                }
            }
            // Offline-resolved Qobuz rows: the cached cover.jpg loads through
            // the same local-file path as Local rows (B5).
            RowItem::Cached { artwork_path, .. } => {
                if let Some(path) = artwork_path.clone().filter(|p| !p.is_empty()) {
                    local.push(ArtworkJob {
                        url: path,
                        target: ArtworkTarget::PlaylistTrack { index },
                    });
                }
            }
            _ => {}
        }
    }
    (http, local)
}

/// Open a local playlist detail (the `local:` branch of
/// `navigate_playlist`). Loads + resolves off-thread, then renders through
/// the shared playlist view.
pub fn navigate(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: ImageCache,
    playlist_id: String,
) {
    handle.spawn(async move {
        let active = playlist_id.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            crate::playlist::reset(&w);
            let state = w.global::<PlaylistState>();
            state.set_is_local(true);
            state.set_offline_only(false);
            crate::sidebar::set_active(&w, &active);
            w.global::<NavState>().set_view(ContentView::Playlist);
        });
        let Some(data) = load(&runtime, &playlist_id).await else {
            log::warn!("[qbz-slint] local playlist {playlist_id} not found");
            let _ = weak.upgrade_in_event_loop(|w| {
                w.global::<PlaylistState>().set_loading(false);
            });
            return;
        };
        let (http_jobs, local_jobs) = artwork_jobs(&data.rows);
        let _ = weak.upgrade_in_event_loop(move |w| {
            apply(&w, data);
        });
        if !http_jobs.is_empty() {
            artwork::spawn_loads(http_jobs, weak.clone(), image_cache.clone());
        }
        if !local_jobs.is_empty() {
            artwork::spawn_local_loads(local_jobs, weak.clone(), image_cache.clone());
        }
    });
}

// ──────────────── Mixed Qobuz playlist — offline detail (D11.a) ────────────────

/// Open the OFFLINE rendering of a MIXED (Qobuz-id) playlist: its local
/// sidecar rows (`playlist_local_tracks`) plus — under INDUCED offline only —
/// its Plex sidecar rows (`playlist_plex_tracks`; availability rule). The
/// Qobuz membership lives in the API and is not enumerable offline, so the
/// detail structurally shows ZERO Qobuz rows (spec D11 honest limit). The
/// name/description come from the sidebar's last-loaded cache (real names
/// after a mid-session flip; the synthesized fallback on a cold offline
/// start — the name is not stored locally anywhere).
pub fn navigate_qobuz_offline(
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: ImageCache,
    playlist_id: u64,
) {
    handle.spawn(async move {
        {
            let weak = weak.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                crate::playlist::reset(&w);
                // Set BEFORE the view switch so the AppShell mounts the
                // detail (not the OfflinePlaceholder) with no flash.
                w.global::<PlaylistState>().set_offline_subset(true);
                crate::sidebar::set_active(&w, &playlist_id.to_string());
                w.global::<NavState>().set_view(ContentView::Playlist);
            });
        }

        let plex_allowed = crate::offline_mode::engine().status().mode
            == qbz_app::offline_mode::OfflineMode::InducedOffline;
        let (rows, custom_artwork_path) = tokio::task::spawn_blocking(move || {
            crate::library_db::with_db(|db| {
                let mut rows: Vec<LoadedRow> = db
                    .get_playlist_local_tracks_with_position(playlist_id)?
                    .into_iter()
                    .map(|r| LoadedRow {
                        position: r.playlist_position,
                        item: RowItem::Local(Box::new(r.track)),
                    })
                    .collect();
                if plex_allowed {
                    rows.extend(
                        db.get_playlist_plex_tracks_with_position(playlist_id)?
                            .into_iter()
                            .map(|(key, position)| LoadedRow {
                                position,
                                item: RowItem::Plex { key },
                            }),
                    );
                    rows.sort_by_key(|r| r.position);
                }
                let custom = db
                    .get_playlist_settings(playlist_id)?
                    .and_then(|s| s.custom_artwork_path)
                    .filter(|p| !p.is_empty());
                Ok((rows, custom))
            })
            .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

        let (name, description) = crate::sidebar::playlist_name_desc(playlist_id)
            .unwrap_or_else(|| ("Playlist".to_string(), String::new()));
        let (http_jobs, local_jobs) = artwork_jobs(&rows);
        let _ = weak.upgrade_in_event_loop(move |w| {
            apply_qobuz_offline(&w, playlist_id, name, description, custom_artwork_path, rows);
        });
        // Sidecar rows carry file paths (local) — the http set stays empty,
        // kept for symmetry with the local detail's dispatch.
        if !http_jobs.is_empty() {
            artwork::spawn_loads(http_jobs, weak.clone(), image_cache.clone());
        }
        if !local_jobs.is_empty() {
            artwork::spawn_local_loads(local_jobs, weak.clone(), image_cache.clone());
        }
    });
}

/// Apply the offline sidecar rows of a mixed Qobuz playlist into
/// `PlaylistState`. Read-only header (`is_owner` false — Qobuz edits can't
/// run offline); playback flows through this module's queue snapshot, the
/// same machinery the LOCAL detail uses. UI thread.
fn apply_qobuz_offline(
    window: &AppWindow,
    playlist_id: u64,
    name: String,
    description: String,
    custom_artwork_path: Option<String>,
    rows: Vec<LoadedRow>,
) {
    let (queue, items, positions) = build_row_models(&rows);
    if let Ok(mut cur) = CURRENT_QUEUE.lock() {
        *cur = queue;
    }
    // NOT offline-only (D8 stamp stays off): this is a real Qobuz playlist
    // rendered partially.
    if let Ok(mut meta) = CURRENT_META.lock() {
        *meta = Some((playlist_id.to_string(), false));
    }
    if let Ok(mut pos) = ROW_POSITIONS.lock() {
        *pos = positions;
    }

    let duration = total_duration_label(&rows);
    let state = window.global::<PlaylistState>();
    state.set_id(playlist_id.to_string().into());
    state.set_name(name.into());
    state.set_owner("Local tracks only — offline".into());
    let description = crate::strip_html::strip_html(&description);
    state.set_description(description.clone().into());
    state.set_description_short(description.into());
    state.set_is_local(false);
    state.set_offline_only(false);
    state.set_offline_subset(true);
    // Read-only offline: Qobuz-side edits (rename / remove tracks / custom
    // order writes) can't reach the API, so the owner affordances hide.
    state.set_is_owner(false);
    let custom = custom_artwork_path
        .as_ref()
        .filter(|p| std::path::Path::new(p).exists())
        .and_then(|p| slint::Image::load_from_path(std::path::Path::new(p)).ok());
    if let Some(img) = custom {
        state.set_cover(img);
        state.set_cover_url(custom_artwork_path.unwrap_or_default().into());
        state.set_has_custom(true);
    } else {
        state.set_cover_url("".into());
        state.set_has_custom(false);
    }
    state.set_total_duration(duration.into());
    crate::playlist::apply_local_items(window, items);
}

// ──────────────────────── playback ────────────────────────

/// Replace the queue with `tracks`, stamp the offline-only flag (D8 guard:
/// the QConnect push site reads it and skips the cloud), start at `start`.
async fn play_stamped(runtime: &Runtime, weak: &slint::Weak<AppWindow>, tracks: Vec<QueueTrack>, start: usize) {
    if tracks.is_empty() {
        crate::toast::error_weak(weak, "Nothing playable in this playlist right now");
        return;
    }
    let offline_only = CURRENT_META
        .lock()
        .ok()
        .and_then(|m| m.as_ref().map(|(_, o)| *o))
        .unwrap_or(false);
    let start = start.min(tracks.len() - 1);
    let first_id = tracks[start].id;
    runtime.core().set_queue(tracks, Some(start)).await;
    // AFTER set_queue (which clears the stamp on every replacement).
    runtime.core().set_queue_offline_only(offline_only);
    after_track_change(runtime, weak, first_id).await;
    refresh_sidebar(true);
}

/// Order the queue snapshot by the VISIBLE row order (sort/search applied),
/// mirroring `playback`'s visible-order rule for the Qobuz detail.
fn visible_ordered_queue(window: &AppWindow) -> Vec<QueueTrack> {
    let snapshot = CURRENT_QUEUE.lock().map(|q| q.clone()).unwrap_or_default();
    let by_id: HashMap<String, &QueueTrack> =
        snapshot.iter().map(|q| (q.id.to_string(), q)).collect();
    let model = window.global::<PlaylistState>().get_tracks();
    let mut out: Vec<QueueTrack> = Vec::new();
    for i in 0..model.row_count() {
        if let Some(it) = model.row_data(i) {
            if let Some(q) = by_id.get(it.id.as_str()) {
                out.push((*q).clone());
            }
        }
    }
    if out.is_empty() {
        snapshot
    } else {
        out
    }
}

/// Hero Play (visible order) / Shuffle for the open local playlist.
pub fn play_all(
    window: &AppWindow,
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    shuffle: bool,
) {
    let mut tracks = visible_ordered_queue(window);
    if shuffle {
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
    }
    let ctx_id = window.global::<PlaylistState>().get_id().to_string();
    crate::playback::set_now_playing_context(&weak, "playlist", &ctx_id);
    handle.spawn(async move {
        play_stamped(&runtime, &weak, tracks, 0).await;
    });
}

/// Per-row "play from here" — queue the visible order starting at the
/// clicked row (the local branch of `play_track_in_context`'s Playlist arm).
/// Returns false when the clicked row is not in the playable snapshot.
pub fn play_from_visible(
    window: &AppWindow,
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    clicked_id: &str,
) -> bool {
    let tracks = visible_ordered_queue(window);
    let Some(idx) = tracks.iter().position(|q| q.id.to_string() == clicked_id) else {
        return false;
    };
    let ctx_id = window.global::<PlaylistState>().get_id().to_string();
    crate::playback::set_now_playing_context(&weak, "playlist", &ctx_id);
    handle.spawn(async move {
        play_stamped(&runtime, &weak, tracks, idx).await;
    });
    true
}

/// Queue / play-next a local playlist by id (sidebar + now-playing context
/// actions). When the playlist is OFFLINE-ONLY the queue gets stamped even
/// on append (D8 strict reading: not even its numeric track ids may reach
/// the QConnect cloud push) — the stamp clears on the next replacement.
pub fn enqueue_by_id(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    playlist_id: String,
    play_next: bool,
) {
    handle.spawn(async move {
        let Some(data) = load(&runtime, &playlist_id).await else {
            crate::toast::error_weak(&weak, "Couldn't load this playlist");
            return;
        };
        let tracks: Vec<QueueTrack> = data
            .rows
            .iter()
            .filter_map(|r| row_queue_track(&r.item))
            .collect();
        if tracks.is_empty() {
            crate::toast::error_weak(&weak, "Nothing playable in this playlist right now");
            return;
        }
        if play_next {
            for track in tracks.into_iter().rev() {
                runtime.core().add_track_next(track).await;
            }
        } else {
            runtime.core().add_tracks(tracks).await;
        }
        if data.offline_only {
            runtime.core().set_queue_offline_only(true);
        }
        refresh_sidebar(false);
        crate::toast::success_weak(&weak, if play_next { "Playing next" } else { "Added to queue" });
    });
}

// ──────────────────────── removal (multi-select) ────────────────────────

/// Remove the selected rows from the open local playlist (repo positions,
/// highest first so each removal's compaction never shifts a pending one),
/// then reload the detail. UI thread entry; DB work off-thread.
pub fn remove_selected(
    window: &AppWindow,
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
) {
    let playlist_id = window.global::<PlaylistState>().get_id().to_string();
    if !is_local_id(&playlist_id) {
        return;
    }
    let model = window.global::<PlaylistState>().get_tracks();
    let selected: Vec<String> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|t| t.selected)
        .map(|t| t.id.to_string())
        .collect();
    if selected.is_empty() {
        return;
    }
    let mut positions: Vec<i32> = {
        let map = ROW_POSITIONS.lock().map(|m| m.clone()).unwrap_or_default();
        selected.iter().filter_map(|id| map.get(id).copied()).collect()
    };
    positions.sort_unstable_by(|a, b| b.cmp(a));
    if positions.is_empty() {
        return;
    }
    crate::playlist::set_multi_select(window, false);
    let handle2 = handle.clone();
    handle.spawn(async move {
        let pid = playlist_id.clone();
        tokio::task::spawn_blocking(move || {
            crate::library_db::with_db(|db| {
                Ok(db.with_connection(|conn| {
                    for pos in positions {
                        if let Err(e) = repo::remove_track(conn, &pid, pos) {
                            log::error!("[qbz-slint] local playlist remove pos {pos}: {e}");
                        }
                    }
                }))
            })
        })
        .await
        .ok();
        navigate(runtime, weak, &handle2, image_cache, playlist_id);
    });
}

// ──────────────────────── Upload to Qobuz (D8) ────────────────────────

/// Convert a non-offline-only local playlist into a real Qobuz playlist:
/// create it, add the Qobuz-source rows, attach local rows via the existing
/// mixed-playlist sidecar (`playlist_local_tracks`) and Plex rows via the
/// plex sidecar (`playlist_plex_tracks`, the same table Tauri's
/// `v2_playlist_add_plex_track` writes), then delete the local entity. On
/// any attach failure the local entity is KEPT so the user can retry.
/// Never reached for offline-only playlists (the UI hides the action and
/// this guards again).
pub fn upload_to_qobuz(
    runtime: Runtime,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    playlist_id: String,
) {
    handle.clone().spawn(async move {
        let id = playlist_id.clone();
        let (header, rows) = match tokio::task::spawn_blocking({
            let id = id.clone();
            move || (get_blocking(&id), get_tracks_blocking(&id))
        })
        .await
        {
            Ok(pair) => pair,
            Err(_) => return,
        };
        let Some(header) = header else {
            crate::toast::error_weak(&weak, "Couldn't load this playlist");
            return;
        };
        if header.offline_only {
            log::warn!("[qbz-slint] upload_to_qobuz refused: {id} is offline-only (D8)");
            return;
        }
        if crate::offline_mode::engine().is_offline() {
            crate::toast::error_weak(&weak, "You're offline — try again when connected");
            return;
        }

        let desc = header.description.as_deref().filter(|d| !d.trim().is_empty());
        let created = match runtime.core().create_playlist(&header.name, desc, false).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("[qbz-slint] upload to Qobuz: create failed: {e}");
                crate::toast::error_weak(&weak, "Couldn't create the Qobuz playlist");
                return;
            }
        };
        let new_id = created.id;

        // Qobuz rows -> real membership.
        let qobuz_ids: Vec<u64> = rows.iter().filter_map(|r| r.qobuz_track_id).collect();
        if !qobuz_ids.is_empty() {
            if let Err(e) = runtime.core().add_tracks_to_playlist(new_id, &qobuz_ids).await {
                // Leave BOTH entities in place — the user can retry; deleting
                // the local copy after a partial upload would lose data.
                log::error!("[qbz-slint] upload to Qobuz: add tracks failed: {e}");
                crate::toast::error_weak(&weak, "Upload incomplete — local playlist kept");
                return;
            }
        }

        // Local rows -> the existing mixed-playlist sidecar, positioned
        // after the Qobuz block (Tauri's append convention). Plex rows ->
        // the plex sidecar, after the local block, relative order preserved
        // (B1). The local entity is deleted ONLY when the sidecar attach
        // succeeds — on a DB failure it stays so the user can retry.
        let local_paths: Vec<String> = rows.iter().filter_map(|r| r.local_path.clone()).collect();
        let plex_keys: Vec<String> = rows.iter().filter_map(|r| r.plex_key.clone()).collect();
        let qobuz_count = qobuz_ids.len();
        let id_for_delete = id.clone();
        let attached = tokio::task::spawn_blocking(move || {
            let ok = crate::library_db::with_db(|db| {
                for (i, path) in local_paths.iter().enumerate() {
                    match db.get_track_by_path(path)? {
                        Some(track) => {
                            db.add_local_track_to_playlist(
                                new_id,
                                track.id,
                                (qobuz_count + i) as i32,
                            )?;
                        }
                        None => {
                            log::warn!(
                                "[qbz-slint] upload to Qobuz: local row missing from library: {path}"
                            );
                        }
                    }
                }
                for (i, key) in plex_keys.iter().enumerate() {
                    db.add_plex_track_to_playlist(
                        new_id,
                        key,
                        (qobuz_count + local_paths.len() + i) as i32,
                    )?;
                }
                Ok(())
            })
            .is_some();
            if ok {
                delete_blocking(&id_for_delete);
            }
            ok
        })
        .await
        .unwrap_or(false);
        if !attached {
            // The Qobuz playlist exists with its Qobuz tracks, but the
            // local/Plex sidecar rows didn't attach — keep the local entity.
            log::error!("[qbz-slint] upload to Qobuz: sidecar attach failed — local playlist kept");
            crate::toast::error_weak(&weak, "Upload incomplete — local playlist kept");
            let weak2 = weak.clone();
            let r2 = runtime.clone();
            let h2 = handle.clone();
            let _ = weak.upgrade_in_event_loop(move |_w| {
                crate::load_sidebar_playlists(r2, weak2, &h2);
            });
            return;
        }

        crate::toast::success_weak(&weak, "Playlist uploaded to Qobuz");
        let weak2 = weak.clone();
        let r2 = runtime.clone();
        let h2 = handle.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            crate::load_sidebar_playlists(r2.clone(), weak2.clone(), &h2);
            crate::nav::record(crate::nav::NavEntry::Playlist(new_id.to_string()));
            crate::navigate_playlist(r2, weak2.clone(), &h2, image_cache, new_id.to_string());
            crate::update_nav_flags(&w);
        });
    });
}
