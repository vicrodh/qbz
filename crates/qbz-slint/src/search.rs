//! Search results controller.
//!
//! Three stages, mirroring `album.rs`: `load_search` fetches a combined
//! search through `QbzCore` on a worker thread, `map_*` turns the domain
//! types into plain `Send` rows (the unit-tested layer), and
//! `apply_search` writes the `SearchState` global on the Slint event loop.

use std::collections::HashSet;
use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{Album, Artist, MostPopularItem, Playlist, SearchAllResults, Track};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{AlbumCardItem, AppWindow, SearchPlaylistItem, SearchState, TrackItem, SlimItem};

thread_local! {
    /// Monotonic search-attempt counter. Each `navigate_search` captures the
    /// current value; a stale async load whose version is no longer current
    /// must not overwrite a newer search's results. UI thread only.
    static SEARCH_VERSION: std::cell::Cell<u64> = std::cell::Cell::new(0);
}

/// Bump the search version and return the new value.
pub fn next_search_version() -> u64 {
    SEARCH_VERSION.with(|c| {
        let v = c.get() + 1;
        c.set(v);
        v
    })
}

/// Whether `version` is still the most recent search attempt.
pub fn is_current_version(version: u64) -> bool {
    SEARCH_VERSION.with(|c| c.get() == version)
}

// ==================== Plain (Send) row types ====================

/// An album result row, before it becomes a Slint `AlbumCardItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct AlbumRow {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    pub genre: String,
    pub year: String,
    pub quality_tier: String,
    pub quality_label: String,
    pub artwork_url: String,
}

/// A track result row, before it becomes a Slint `TrackItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRowData {
    pub id: String,
    pub title: String,
    pub artist: String,
    /// Performer id for the clickable artist link ("" = plain text).
    pub artist_id: String,
    /// Album id for the clickable album link ("" = plain text).
    pub album_id: String,
    pub duration: String,
    pub quality_tier: String,
    /// Detailed quality label, e.g. "Hi-Res 24-bit / 192 kHz". Used by the
    /// most-popular track hero (shown as text instead of an icon badge).
    pub quality_label: String,
    /// Exact bit-depth / sample-rate line, e.g. "24-bit / 192 kHz" — feeds the
    /// track-row quality badge (no tier prefix, unlike `quality_label`).
    pub quality_detail: String,
    pub explicit: bool,
    pub artwork_url: String,
}

/// An artist result row, before it becomes a Slint `SlimItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtistRow {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub artwork_url: String,
    /// Whether the user already follows (favorites) this artist.
    pub following: bool,
}

/// A playlist result row, before it becomes a Slint `SearchPlaylistItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistRow {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    /// Up to four distinct cover URLs for the collage.
    pub cover_urls: Vec<String>,
}

/// The most-popular hero entry.
#[derive(Debug, Clone, PartialEq)]
pub enum MostPopularRow {
    None,
    Album(AlbumRow),
    Artist(ArtistRow),
    Track(TrackRowData),
}

/// The full result of a combined search, as plain `Send` data.
pub struct SearchData {
    pub query: String,
    pub albums: Vec<AlbumRow>,
    pub tracks: Vec<TrackRowData>,
    pub artists: Vec<ArtistRow>,
    pub playlists: Vec<PlaylistRow>,
    pub albums_total: u32,
    pub tracks_total: u32,
    pub artists_total: u32,
    pub playlists_total: u32,
    pub most_popular: MostPopularRow,
}

// ==================== Pure helpers ====================

/// 24-bit and up is Hi-Res, anything else with depth info is CD-quality.
fn tier(bit_depth: Option<u32>) -> &'static str {
    match bit_depth {
        Some(depth) if depth >= 24 => "hires",
        Some(_) => "cd",
        None => "",
    }
}

/// Quality-badge tooltip, e.g. "Hi-Res 24-bit / 96 kHz". Empty when no
/// quality info is available.
fn quality_label(bit_depth: Option<u32>, sample_rate: Option<f64>) -> String {
    match bit_depth {
        None => String::new(),
        Some(depth) => {
            let prefix = if depth >= 24 { "Hi-Res" } else { "CD" };
            let rate = sample_rate.unwrap_or(if depth >= 24 { 96.0 } else { 44.1 });
            let rate = if rate.fract().abs() < f64::EPSILON {
                format!("{}", rate as i64)
            } else {
                format!("{rate}")
            };
            format!("{prefix} {depth}-bit / {rate} kHz")
        }
    }
}

/// `m:ss` track duration.
fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// First four characters of an ISO date, or empty.
fn year_of(date: Option<&str>) -> String {
    date.and_then(|d| d.get(0..4)).unwrap_or("").to_string()
}

/// Up to four distinct cover URLs for a playlist collage. Qobuz returns
/// pre-built cover lists in `images300` / `images150` / `images`; the
/// highest-resolution non-empty list wins.
fn playlist_cover_urls(playlist: &Playlist) -> Vec<String> {
    let source = [
        &playlist.images300,
        &playlist.images150,
        &playlist.images,
    ]
    .into_iter()
    .flatten()
    .find(|v| !v.is_empty());

    let mut out: Vec<String> = Vec::new();
    if let Some(list) = source {
        for url in list {
            if !url.is_empty() && !out.contains(url) {
                out.push(url.clone());
            }
            if out.len() == 4 {
                break;
            }
        }
    }
    out
}

// ==================== Mappers (unit-tested) ====================

pub fn map_album(album: Album) -> AlbumRow {
    AlbumRow {
        id: album.id,
        title: album.title,
        artist: album.artist.name,
        artist_id: album.artist.id.to_string(),
        genre: album
            .genre
            .map(|g| g.name)
            .filter(|n| !n.is_empty())
            .unwrap_or_default(),
        year: year_of(album.release_date_original.as_deref()),
        quality_tier: tier(album.maximum_bit_depth).to_string(),
        quality_label: quality_label(album.maximum_bit_depth, album.maximum_sampling_rate),
        artwork_url: album.image.best().cloned().unwrap_or_default(),
    }
}

pub fn map_track(track: Track) -> TrackRowData {
    let mut title = track.title;
    if let Some(version) = track.version.as_ref().filter(|v| !v.is_empty()) {
        title = format!("{title} ({version})");
    }
    let artwork_url = track
        .album
        .as_ref()
        .and_then(|a| a.image.best().cloned())
        .unwrap_or_default();
    let album_id = track.album.as_ref().map(|a| a.id.clone()).unwrap_or_default();
    let (artist, artist_id) = track
        .performer
        .map(|p| (p.name, p.id.to_string()))
        .unwrap_or_default();
    TrackRowData {
        id: track.id.to_string(),
        title,
        artist,
        artist_id,
        album_id,
        duration: mmss(track.duration),
        quality_tier: tier(track.maximum_bit_depth).to_string(),
        quality_label: quality_label(track.maximum_bit_depth, track.maximum_sampling_rate),
        quality_detail: crate::quality::detail(
            track.maximum_bit_depth,
            track.maximum_sampling_rate,
        ),
        explicit: track.parental_warning,
        artwork_url,
    }
}

pub fn map_artist(artist: &Artist, following: bool) -> ArtistRow {
    ArtistRow {
        id: artist.id.to_string(),
        name: artist.name.clone(),
        subtitle: match artist.albums_count {
            Some(n) if n > 0 => format!("{n} albums"),
            _ => String::new(),
        },
        artwork_url: artist
            .image
            .as_ref()
            .and_then(|i| i.best().cloned())
            .unwrap_or_default(),
        following,
    }
}

pub fn map_playlist(playlist: Playlist) -> PlaylistRow {
    let cover_urls = playlist_cover_urls(&playlist);
    let mut subtitle = playlist.owner.name.clone();
    if playlist.tracks_count > 0 {
        if subtitle.is_empty() {
            subtitle = format!("{} tracks", playlist.tracks_count);
        } else {
            subtitle = format!("{}   •   {} tracks", subtitle, playlist.tracks_count);
        }
    }
    PlaylistRow {
        id: playlist.id.to_string(),
        title: playlist.name,
        subtitle,
        cover_urls,
    }
}

fn map_most_popular(item: Option<MostPopularItem>, favorite_artists: &HashSet<u64>) -> MostPopularRow {
    match item {
        Some(MostPopularItem::Albums(a)) => MostPopularRow::Album(map_album(a)),
        Some(MostPopularItem::Artists(a)) => {
            let following = favorite_artists.contains(&a.id);
            MostPopularRow::Artist(map_artist(&a, following))
        }
        Some(MostPopularItem::Tracks(t)) => MostPopularRow::Track(map_track(t)),
        None => MostPopularRow::None,
    }
}

/// Map a combined-search result into plain `Send` data. `favorite_artists`
/// is the set of artist ids the user already follows.
pub fn map_search_all(
    query: &str,
    results: SearchAllResults,
    favorite_artists: &HashSet<u64>,
) -> SearchData {
    let artists: Vec<ArtistRow> = results
        .artists
        .items
        .iter()
        .map(|a| map_artist(a, favorite_artists.contains(&a.id)))
        .collect();
    let most_popular = map_most_popular(results.most_popular, favorite_artists);
    // Dedupe used to drop the top-result artist from the artists list
    // here, but the Artists tab does not show the Most-popular hero —
    // it should keep the artist. The dedupe now lives at `apply_search`
    // where the carousel-only `artists_carousel` is built.
    SearchData {
        query: query.to_string(),
        albums_total: results.albums.total,
        tracks_total: results.tracks.total,
        artists_total: results.artists.total,
        playlists_total: results.playlists.total,
        albums: results.albums.items.into_iter().map(map_album).collect(),
        tracks: results.tracks.items.into_iter().map(map_track).collect(),
        artists,
        playlists: results.playlists.items.into_iter().map(map_playlist).collect(),
        most_popular,
    }
}

// ==================== Load (async, worker thread) ====================

/// Run a combined search and map it to plain `Send` data. The search and
/// the user's followed-artist set are fetched concurrently.
pub async fn load_search<A>(
    runtime: &Arc<AppRuntime<A>>,
    query: &str,
) -> Result<SearchData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    // Blacklist filtering (featured-aware via qbz-core helpers); skipped when the feature is disabled.
    let blacklist = if crate::artist_blacklist::is_enabled() {
        crate::artist_blacklist::ids_snapshot()
    } else {
        std::collections::HashSet::new()
    };
    let core = runtime.core();
    let (results, favs) = tokio::join!(
        core.search_all(query, &blacklist),
        core.favorite_artist_ids(),
    );
    let results = results.map_err(|e| e.to_string())?;
    let favs = favs.unwrap_or_default();
    Ok(map_search_all(query, results, &favs))
}

// ==================== Apply (Slint event loop) ====================

fn album_item(row: AlbumRow) -> AlbumCardItem {
    AlbumCardItem {
        id: row.id.into(),
        title: row.title.into(),
        artist: row.artist.into(),
        artist_id: row.artist_id.into(),
        genre: row.genre.into(),
        year: row.year.into(),
        quality_tier: row.quality_tier.into(),
        quality_label: row.quality_label.into(),
        ribbon: Default::default(),
        ribbon_kind: Default::default(),
        artwork_url: row.artwork_url.into(),
        artwork: slint::Image::default(),
        ..Default::default()
    }
}

fn track_item(row: TrackRowData) -> TrackItem {
    let is_favorite = crate::fav_cache::is_favorite(&row.id);
    let is_cached = crate::offline_cache::is_cached(&row.id);
    TrackItem {
        // Combined search DROPS blacklisted rows at build time (T4 snapshot
        // filter), so a row reaching here is never blacklisted (no greyout).
        is_blacklisted: false,
        id: row.id.into(),
        number: "".into(),
        title: row.title.into(),
        artist: row.artist.into(),
        album: "".into(),
        duration: row.duration.into(),
        quality_tier: row.quality_tier.into(),
        quality_detail: row.quality_detail.into(),
        explicit: row.explicit,
        selected: false,
        artwork_url: row.artwork_url.into(),
        artwork: slint::Image::default(),
        is_favorite,
        artist_id: row.artist_id.into(),
        album_id: row.album_id.into(),
        removing: false,
        cache_status: if is_cached { 3 } else { 0 },
        cache_progress: 0.0,
        source: "qobuz".into(),
        unlocking: false,
        // Disc grouping is album-detail only; flat lists carry none.
        disc_header_number: 0,
    }
}

fn artist_item(row: ArtistRow) -> SlimItem {
    SlimItem {
        id: row.id.into(),
        title: row.name.into(),
        subtitle: row.subtitle.into(),
        rank: Default::default(),
        artwork_url: row.artwork_url.into(),
        artwork: slint::Image::default(),
        following: row.following,
    }
}

pub(crate) fn playlist_item(row: PlaylistRow) -> SearchPlaylistItem {
    let url = |i: usize| -> slint::SharedString {
        row.cover_urls.get(i).cloned().unwrap_or_default().into()
    };
    SearchPlaylistItem {
        id: row.id.into(),
        title: row.title.into(),
        subtitle: row.subtitle.into(),
        cover_count: row.cover_urls.len().min(4) as i32,
        url1: url(0),
        url2: url(1),
        url3: url(2),
        url4: url(3),
        cover1: slint::Image::default(),
        cover2: slint::Image::default(),
        cover3: slint::Image::default(),
        cover4: slint::Image::default(),
        // Search playlist results carry no category subtag, and a transparent
        // dominant-colour is the sentinel for "no letterbox" — the collage
        // keeps the legacy cover-fit (contain + dominant colour is Discover-
        // only).
        category: "".into(),
        dominant_color: slint::Color::from_argb_u8(0, 0, 0, 0),
    }
}

/// Apply search results to the `SearchState` global. Runs on the Slint
/// event loop.
pub fn apply_search(window: &AppWindow, data: SearchData) {
    let state = window.global::<SearchState>();
    state.set_query(data.query.into());

    let albums: Vec<AlbumCardItem> = data.albums.into_iter().map(album_item).collect();
    let tracks: Vec<TrackItem> = data.tracks.into_iter().map(track_item).collect();
    let artists: Vec<SlimItem> = data.artists.into_iter().map(artist_item).collect();
    let playlists: Vec<SearchPlaylistItem> =
        data.playlists.into_iter().map(playlist_item).collect();
    // Carousel variant of the artists list — drops the first entry when
    // it equals the most-popular hero, so the All tab does not duplicate
    // the Top result alongside the carousel.
    let mp_id = if let MostPopularRow::Artist(ref mp) = data.most_popular {
        Some(mp.id.clone())
    } else {
        None
    };
    let artists_carousel: Vec<SlimItem> = match (mp_id, artists.first()) {
        (Some(id), Some(first)) if first.id == id.as_str() => artists[1..].to_vec(),
        _ => artists.clone(),
    };

    state.set_albums(ModelRc::new(VecModel::from(albums)));
    state.set_tracks(ModelRc::new(VecModel::from(tracks)));
    state.set_artists(ModelRc::new(VecModel::from(artists)));
    state.set_artists_carousel(ModelRc::new(VecModel::from(artists_carousel)));
    state.set_playlists(ModelRc::new(VecModel::from(playlists)));

    state.set_albums_total(data.albums_total as i32);
    state.set_tracks_total(data.tracks_total as i32);
    state.set_artists_total(data.artists_total as i32);
    state.set_playlists_total(data.playlists_total as i32);

    // Default the hero quality label off; only the track branch sets it.
    state.set_most_popular_quality_label("".into());
    match data.most_popular {
        MostPopularRow::Album(row) => {
            state.set_most_popular_kind("album".into());
            state.set_most_popular_album(album_item(row));
        }
        MostPopularRow::Artist(row) => {
            state.set_most_popular_kind("artist".into());
            state.set_most_popular_artist(artist_item(row));
        }
        MostPopularRow::Track(row) => {
            state.set_most_popular_kind("track".into());
            state.set_most_popular_quality_label(row.quality_label.clone().into());
            state.set_most_popular_track(track_item(row));
        }
        MostPopularRow::None => {
            state.set_most_popular_kind("".into());
        }
    }
}

/// Clear search state and show the loading state (used when starting a new
/// search so the previous results do not flash).
pub fn reset_search(window: &AppWindow) {
    let state = window.global::<SearchState>();
    state.set_albums(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
    state.set_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_artists(ModelRc::new(VecModel::from(Vec::<SlimItem>::new())));
    state.set_playlists(ModelRc::new(VecModel::from(Vec::<SearchPlaylistItem>::new())));
    state.set_albums_total(0);
    state.set_tracks_total(0);
    state.set_artists_total(0);
    state.set_playlists_total(0);
    state.set_most_popular_kind("".into());
    state.set_most_popular_quality_label("".into());
    state.set_filter_index(0);
    state.set_loading(true);
}

/// Mark an artist as followed in every `SearchState` list it appears in
/// (results list + most-popular hero). Runs on the Slint event loop.
pub fn mark_artist_followed(window: &AppWindow, artist_id: &str, following: bool) {
    let state = window.global::<SearchState>();
    if let Some(vm) = state
        .get_artists()
        .as_any()
        .downcast_ref::<VecModel<SlimItem>>()
    {
        for i in 0..vm.row_count() {
            if let Some(mut item) = vm.row_data(i) {
                if item.id == artist_id {
                    item.following = following;
                    vm.set_row_data(i, item);
                }
            }
        }
    }
    if state.get_most_popular_kind() == "artist" {
        let mut mp = state.get_most_popular_artist();
        if mp.id == artist_id {
            mp.following = following;
            state.set_most_popular_artist(mp);
        }
    }
}

// ==================== Load-more (pagination) ====================

/// Which category a load-more request targets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SearchCategory {
    Albums,
    Tracks,
    Artists,
    Playlists,
}

/// Map a results-tab index to the category whose list it paginates.
/// Tab 0 (All) has no single category.
pub fn category_for_tab(tab: i32) -> Option<SearchCategory> {
    match tab {
        1 => Some(SearchCategory::Albums),
        2 => Some(SearchCategory::Tracks),
        3 => Some(SearchCategory::Artists),
        4 => Some(SearchCategory::Playlists),
        _ => None,
    }
}

/// Map a filter index to the Qobuz `search_type` value. Index 0 maps to
/// `None` (no filter).
pub fn search_type_for_filter(index: i32) -> Option<String> {
    match index {
        1 => Some("MainArtist".into()),
        2 => Some("Performer".into()),
        3 => Some("Composer".into()),
        4 => Some("Label".into()),
        5 => Some("ReleaseName".into()),
        _ => None,
    }
}

/// A page of additional rows fetched by load-more, ready to append.
pub enum MoreRows {
    Albums(Vec<AlbumRow>),
    Tracks(Vec<TrackRowData>),
    Artists(Vec<ArtistRow>),
    Playlists(Vec<PlaylistRow>),
}

/// Load-more page size (matches the Tauri search page size).
const PAGE_SIZE: u32 = 20;

/// Fetch the next page for one category, starting at `offset`.
pub async fn load_more<A>(
    runtime: &Arc<AppRuntime<A>>,
    query: &str,
    category: SearchCategory,
    search_type: Option<String>,
    offset: u32,
) -> Result<MoreRows, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let core = runtime.core();
    let search_type = search_type.as_deref();
    match category {
        SearchCategory::Albums => {
            let page = core
                .search_albums(query, PAGE_SIZE, offset, search_type)
                .await
                .map_err(|e| e.to_string())?;
            Ok(MoreRows::Albums(
                page.items.into_iter().map(map_album).collect(),
            ))
        }
        SearchCategory::Tracks => {
            let page = core
                .search_tracks(query, PAGE_SIZE, offset, search_type)
                .await
                .map_err(|e| e.to_string())?;
            Ok(MoreRows::Tracks(
                page.items.into_iter().map(map_track).collect(),
            ))
        }
        SearchCategory::Artists => {
            let (page, favs) = tokio::join!(
                core.search_artists(query, PAGE_SIZE, offset, search_type),
                core.favorite_artist_ids(),
            );
            let page = page.map_err(|e| e.to_string())?;
            let favs = favs.unwrap_or_default();
            Ok(MoreRows::Artists(
                page.items
                    .iter()
                    .map(|a| map_artist(a, favs.contains(&a.id)))
                    .collect(),
            ))
        }
        SearchCategory::Playlists => {
            let page = core
                .search_playlists(query, PAGE_SIZE, offset)
                .await
                .map_err(|e| e.to_string())?;
            Ok(MoreRows::Playlists(
                page.items.into_iter().map(map_playlist).collect(),
            ))
        }
    }
}

/// Append fetched rows to the matching `SearchState` list. Pushes onto the
/// existing `VecModel` so already-loaded rows (and any resolved artwork)
/// are untouched. Runs on the Slint event loop.
pub fn append_results(window: &AppWindow, more: MoreRows) {
    let state = window.global::<SearchState>();
    match more {
        MoreRows::Albums(rows) => {
            if let Some(vm) = state
                .get_albums()
                .as_any()
                .downcast_ref::<VecModel<AlbumCardItem>>()
            {
                for row in rows {
                    vm.push(album_item(row));
                }
            }
        }
        MoreRows::Tracks(rows) => {
            if let Some(vm) = state
                .get_tracks()
                .as_any()
                .downcast_ref::<VecModel<TrackItem>>()
            {
                for row in rows {
                    vm.push(track_item(row));
                }
            }
        }
        MoreRows::Artists(rows) => {
            if let Some(vm) = state
                .get_artists()
                .as_any()
                .downcast_ref::<VecModel<SlimItem>>()
            {
                for row in rows {
                    vm.push(artist_item(row));
                }
            }
        }
        MoreRows::Playlists(rows) => {
            if let Some(vm) = state
                .get_playlists()
                .as_any()
                .downcast_ref::<VecModel<SearchPlaylistItem>>()
            {
                for row in rows {
                    vm.push(playlist_item(row));
                }
            }
        }
    }
}

/// Replace one category's `SearchState` list wholesale — used when the
/// searchType filter changes and the category is re-queried from offset 0.
pub fn replace_category(window: &AppWindow, more: MoreRows) {
    let state = window.global::<SearchState>();
    match more {
        MoreRows::Albums(rows) => {
            let items: Vec<AlbumCardItem> = rows.into_iter().map(album_item).collect();
            state.set_albums(ModelRc::new(VecModel::from(items)));
        }
        MoreRows::Tracks(rows) => {
            let items: Vec<TrackItem> = rows.into_iter().map(track_item).collect();
            state.set_tracks(ModelRc::new(VecModel::from(items)));
        }
        MoreRows::Artists(rows) => {
            // Rebuild both lists: the Artists tab keeps every result; the
            // All-tab carousel drops the duplicate next to the Most-popular
            // hero.
            let items: Vec<SlimItem> = rows.into_iter().map(artist_item).collect();
            let mp_id = if state.get_most_popular_kind().as_str() == "artist" {
                Some(state.get_most_popular_artist().id)
            } else {
                None
            };
            let carousel: Vec<SlimItem> = match (mp_id, items.first()) {
                (Some(id), Some(first)) if first.id == id.as_str() => items[1..].to_vec(),
                _ => items.clone(),
            };
            state.set_artists(ModelRc::new(VecModel::from(items)));
            state.set_artists_carousel(ModelRc::new(VecModel::from(carousel)));
        }
        MoreRows::Playlists(rows) => {
            let items: Vec<SearchPlaylistItem> = rows.into_iter().map(playlist_item).collect();
            state.set_playlists(ModelRc::new(VecModel::from(items)));
        }
    }
}

// ==================== Artwork jobs ====================

/// Cover-download jobs for an album/track/artist row at `idx`.
fn simple_job(target: ArtworkTarget, url: &str) -> Option<ArtworkJob> {
    (!url.is_empty()).then(|| ArtworkJob {
        target,
        url: url.to_string(),
    })
}

/// Playlist collage jobs — one per cover URL the row carries.
fn playlist_jobs(idx: usize, urls: &[String], jobs: &mut Vec<ArtworkJob>) {
    for (slot, url) in urls.iter().enumerate().take(4) {
        if !url.is_empty() {
            jobs.push(ArtworkJob {
                target: ArtworkTarget::SearchPlaylistCover { idx, slot },
                url: url.clone(),
            });
        }
    }
}

/// Build artwork download jobs for a freshly applied `SearchData`.
pub fn artwork_jobs(data: &SearchData) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    for (idx, row) in data.albums.iter().enumerate() {
        jobs.extend(simple_job(ArtworkTarget::SearchAlbum { idx }, &row.artwork_url));
    }
    for (idx, row) in data.tracks.iter().enumerate() {
        jobs.extend(simple_job(ArtworkTarget::SearchTrack { idx }, &row.artwork_url));
    }
    for (idx, row) in data.artists.iter().enumerate() {
        jobs.extend(simple_job(ArtworkTarget::SearchArtist { idx }, &row.artwork_url));
    }
    for (idx, row) in data.playlists.iter().enumerate() {
        playlist_jobs(idx, &row.cover_urls, &mut jobs);
    }
    let mp_url = match &data.most_popular {
        MostPopularRow::Album(r) => r.artwork_url.as_str(),
        MostPopularRow::Artist(r) => r.artwork_url.as_str(),
        MostPopularRow::Track(r) => r.artwork_url.as_str(),
        MostPopularRow::None => "",
    };
    jobs.extend(simple_job(ArtworkTarget::SearchMostPopular, mp_url));
    jobs
}

/// Build artwork jobs for a load-more page, targeting the rows that were
/// just appended (`start` is the index of the first appended row).
pub fn artwork_jobs_for_more(more: &MoreRows, start: usize) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    match more {
        MoreRows::Albums(rows) => {
            for (i, row) in rows.iter().enumerate() {
                jobs.extend(simple_job(
                    ArtworkTarget::SearchAlbum { idx: start + i },
                    &row.artwork_url,
                ));
            }
        }
        MoreRows::Tracks(rows) => {
            for (i, row) in rows.iter().enumerate() {
                jobs.extend(simple_job(
                    ArtworkTarget::SearchTrack { idx: start + i },
                    &row.artwork_url,
                ));
            }
        }
        MoreRows::Artists(rows) => {
            for (i, row) in rows.iter().enumerate() {
                jobs.extend(simple_job(
                    ArtworkTarget::SearchArtist { idx: start + i },
                    &row.artwork_url,
                ));
            }
        }
        MoreRows::Playlists(rows) => {
            for (i, row) in rows.iter().enumerate() {
                playlist_jobs(start + i, &row.cover_urls, &mut jobs);
            }
        }
    }
    jobs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_for_tab_maps_per_type_tabs() {
        assert_eq!(category_for_tab(0), None);
        assert_eq!(category_for_tab(1), Some(SearchCategory::Albums));
        assert_eq!(category_for_tab(2), Some(SearchCategory::Tracks));
        assert_eq!(category_for_tab(3), Some(SearchCategory::Artists));
        assert_eq!(category_for_tab(4), Some(SearchCategory::Playlists));
        assert_eq!(category_for_tab(9), None);
    }

    #[test]
    fn search_type_for_filter_maps_dropdown_index() {
        assert_eq!(search_type_for_filter(0), None);
        assert_eq!(search_type_for_filter(1), Some("MainArtist".to_string()));
        assert_eq!(search_type_for_filter(3), Some("Composer".to_string()));
        assert_eq!(search_type_for_filter(5), Some("ReleaseName".to_string()));
        assert_eq!(search_type_for_filter(99), None);
    }

    #[test]
    fn mmss_pads_seconds() {
        assert_eq!(mmss(5), "0:05");
        assert_eq!(mmss(65), "1:05");
        assert_eq!(mmss(225), "3:45");
    }

    #[test]
    fn tier_classifies_bit_depth() {
        assert_eq!(tier(Some(24)), "hires");
        assert_eq!(tier(Some(16)), "cd");
        assert_eq!(tier(None), "");
    }

    #[test]
    fn quality_label_formats_known_quality() {
        assert_eq!(quality_label(Some(24), Some(96.0)), "Hi-Res 24-bit / 96 kHz");
        assert_eq!(quality_label(Some(16), Some(44.1)), "CD 16-bit / 44.1 kHz");
        assert_eq!(quality_label(None, None), "");
    }

    #[test]
    fn map_artist_builds_album_count_subtitle() {
        let artist = Artist {
            id: 7,
            name: "Metallica".into(),
            image: None,
            albums_count: Some(12),
            biography: None,
            albums: None,
            tracks_appears_on: None,
            playlists: None,
        };
        let row = map_artist(&artist, true);
        assert_eq!(row.id, "7");
        assert_eq!(row.name, "Metallica");
        assert_eq!(row.subtitle, "12 albums");
        assert!(row.following);
    }
}
