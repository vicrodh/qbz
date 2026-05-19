//! Search results controller.
//!
//! Three stages, mirroring `album.rs`: `load_search` fetches a combined
//! search through `QbzCore` on a worker thread, `map_*` turns the domain
//! types into plain `Send` rows (the unit-tested layer), and
//! `apply_search` writes the `SearchState` global on the Slint event loop.

use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{Album, Artist, MostPopularItem, Playlist, SearchAllResults, Track};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{AlbumCardItem, AppWindow, SearchPlaylistItem, SearchState, SearchTrackItem, SlimItem};

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
    pub genre: String,
    pub year: String,
    pub quality_tier: String,
    pub quality_label: String,
    pub artwork_url: String,
}

/// A track result row, before it becomes a Slint `SearchTrackItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRow {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration: String,
    pub quality_tier: String,
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
}

/// A playlist result row, before it becomes a Slint `SearchPlaylistItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistRow {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub artwork_url: String,
}

/// The most-popular hero entry.
#[derive(Debug, Clone, PartialEq)]
pub enum MostPopularRow {
    None,
    Album(AlbumRow),
    Artist(ArtistRow),
    Track(TrackRow),
}

/// The full result of a combined search, as plain `Send` data.
pub struct SearchData {
    pub query: String,
    pub albums: Vec<AlbumRow>,
    pub tracks: Vec<TrackRow>,
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

// ==================== Mappers (unit-tested) ====================

pub fn map_album(album: Album) -> AlbumRow {
    AlbumRow {
        id: album.id,
        title: album.title,
        artist: album.artist.name,
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

pub fn map_track(track: Track) -> TrackRow {
    let mut title = track.title;
    if let Some(version) = track.version.as_ref().filter(|v| !v.is_empty()) {
        title = format!("{title} ({version})");
    }
    let artwork_url = track
        .album
        .as_ref()
        .and_then(|a| a.image.best().cloned())
        .unwrap_or_default();
    TrackRow {
        id: track.id.to_string(),
        title,
        artist: track.performer.map(|p| p.name).unwrap_or_default(),
        duration: mmss(track.duration),
        quality_tier: tier(track.maximum_bit_depth).to_string(),
        explicit: track.parental_warning,
        artwork_url,
    }
}

pub fn map_artist(artist: &Artist) -> ArtistRow {
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
    }
}

pub fn map_playlist(playlist: Playlist) -> PlaylistRow {
    let artwork_url = playlist
        .images150
        .as_ref()
        .or(playlist.images.as_ref())
        .or(playlist.images300.as_ref())
        .and_then(|imgs| imgs.first().cloned())
        .unwrap_or_default();
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
        artwork_url,
    }
}

fn map_most_popular(item: Option<MostPopularItem>) -> MostPopularRow {
    match item {
        Some(MostPopularItem::Albums(a)) => MostPopularRow::Album(map_album(a)),
        Some(MostPopularItem::Artists(a)) => MostPopularRow::Artist(map_artist(&a)),
        Some(MostPopularItem::Tracks(t)) => MostPopularRow::Track(map_track(t)),
        None => MostPopularRow::None,
    }
}

/// Map a combined-search result into plain `Send` data.
pub fn map_search_all(query: &str, results: SearchAllResults) -> SearchData {
    SearchData {
        query: query.to_string(),
        albums_total: results.albums.total,
        tracks_total: results.tracks.total,
        artists_total: results.artists.total,
        playlists_total: results.playlists.total,
        albums: results.albums.items.into_iter().map(map_album).collect(),
        tracks: results.tracks.items.into_iter().map(map_track).collect(),
        artists: results.artists.items.iter().map(map_artist).collect(),
        playlists: results.playlists.items.into_iter().map(map_playlist).collect(),
        most_popular: map_most_popular(results.most_popular),
    }
}

// ==================== Load (async, worker thread) ====================

/// Run a combined search and map it to plain `Send` data.
pub async fn load_search<A>(
    runtime: &Arc<AppRuntime<A>>,
    query: &str,
) -> Result<SearchData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    // Plan A passes an empty blacklist; the blacklist module is migrated
    // separately (roadmap task #9).
    let blacklist: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let results = runtime
        .core()
        .search_all(query, &blacklist)
        .await
        .map_err(|e| e.to_string())?;
    Ok(map_search_all(query, results))
}

// ==================== Apply (Slint event loop) ====================

fn album_item(row: AlbumRow) -> AlbumCardItem {
    AlbumCardItem {
        id: row.id.into(),
        title: row.title.into(),
        artist: row.artist.into(),
        genre: row.genre.into(),
        year: row.year.into(),
        quality_tier: row.quality_tier.into(),
        quality_label: row.quality_label.into(),
        ribbon: Default::default(),
        ribbon_kind: Default::default(),
        artwork_url: row.artwork_url.into(),
        artwork: slint::Image::default(),
    }
}

fn track_item(row: TrackRow) -> SearchTrackItem {
    SearchTrackItem {
        id: row.id.into(),
        title: row.title.into(),
        artist: row.artist.into(),
        duration: row.duration.into(),
        quality_tier: row.quality_tier.into(),
        explicit: row.explicit,
        artwork_url: row.artwork_url.into(),
        artwork: slint::Image::default(),
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
    }
}

fn playlist_item(row: PlaylistRow) -> SearchPlaylistItem {
    SearchPlaylistItem {
        id: row.id.into(),
        title: row.title.into(),
        subtitle: row.subtitle.into(),
        artwork_url: row.artwork_url.into(),
        artwork: slint::Image::default(),
    }
}

/// Apply search results to the `SearchState` global. Runs on the Slint
/// event loop.
pub fn apply_search(window: &AppWindow, data: SearchData) {
    let state = window.global::<SearchState>();
    state.set_query(data.query.into());

    let albums: Vec<AlbumCardItem> = data.albums.into_iter().map(album_item).collect();
    let tracks: Vec<SearchTrackItem> = data.tracks.into_iter().map(track_item).collect();
    let artists: Vec<SlimItem> = data.artists.into_iter().map(artist_item).collect();
    let playlists: Vec<SearchPlaylistItem> =
        data.playlists.into_iter().map(playlist_item).collect();

    state.set_albums(ModelRc::new(VecModel::from(albums)));
    state.set_tracks(ModelRc::new(VecModel::from(tracks)));
    state.set_artists(ModelRc::new(VecModel::from(artists)));
    state.set_playlists(ModelRc::new(VecModel::from(playlists)));

    state.set_albums_total(data.albums_total as i32);
    state.set_tracks_total(data.tracks_total as i32);
    state.set_artists_total(data.artists_total as i32);
    state.set_playlists_total(data.playlists_total as i32);

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
    state.set_tracks(ModelRc::new(VecModel::from(Vec::<SearchTrackItem>::new())));
    state.set_artists(ModelRc::new(VecModel::from(Vec::<SlimItem>::new())));
    state.set_playlists(ModelRc::new(VecModel::from(Vec::<SearchPlaylistItem>::new())));
    state.set_albums_total(0);
    state.set_tracks_total(0);
    state.set_artists_total(0);
    state.set_playlists_total(0);
    state.set_most_popular_kind("".into());
    state.set_filter_index(0);
    state.set_loading(true);
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

/// Map a filter-dropdown index to the Qobuz `search_type` value.
/// Index 0 (No filter) maps to `None`.
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
    Tracks(Vec<TrackRow>),
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
            let page = core
                .search_artists(query, PAGE_SIZE, offset, search_type)
                .await
                .map_err(|e| e.to_string())?;
            Ok(MoreRows::Artists(
                page.items.iter().map(map_artist).collect(),
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
                .downcast_ref::<VecModel<SearchTrackItem>>()
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
            let items: Vec<SearchTrackItem> = rows.into_iter().map(track_item).collect();
            state.set_tracks(ModelRc::new(VecModel::from(items)));
        }
        MoreRows::Artists(rows) => {
            let items: Vec<SlimItem> = rows.into_iter().map(artist_item).collect();
            state.set_artists(ModelRc::new(VecModel::from(items)));
        }
        MoreRows::Playlists(rows) => {
            let items: Vec<SearchPlaylistItem> = rows.into_iter().map(playlist_item).collect();
            state.set_playlists(ModelRc::new(VecModel::from(items)));
        }
    }
}

// ==================== Artwork jobs ====================

/// Build artwork download jobs for a freshly applied `SearchData` — one
/// per result row that carries a cover URL.
pub fn artwork_jobs(data: &SearchData) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    for (idx, row) in data.albums.iter().enumerate() {
        if !row.artwork_url.is_empty() {
            jobs.push(ArtworkJob {
                target: ArtworkTarget::SearchAlbum { idx },
                url: row.artwork_url.clone(),
            });
        }
    }
    for (idx, row) in data.tracks.iter().enumerate() {
        if !row.artwork_url.is_empty() {
            jobs.push(ArtworkJob {
                target: ArtworkTarget::SearchTrack { idx },
                url: row.artwork_url.clone(),
            });
        }
    }
    for (idx, row) in data.artists.iter().enumerate() {
        if !row.artwork_url.is_empty() {
            jobs.push(ArtworkJob {
                target: ArtworkTarget::SearchArtist { idx },
                url: row.artwork_url.clone(),
            });
        }
    }
    for (idx, row) in data.playlists.iter().enumerate() {
        if !row.artwork_url.is_empty() {
            jobs.push(ArtworkJob {
                target: ArtworkTarget::SearchPlaylist { idx },
                url: row.artwork_url.clone(),
            });
        }
    }
    jobs
}

/// Build artwork jobs for a load-more page, targeting the rows that were
/// just appended (`start` is the index of the first appended row).
pub fn artwork_jobs_for_more(more: &MoreRows, start: usize) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    match more {
        MoreRows::Albums(rows) => {
            for (i, row) in rows.iter().enumerate() {
                if !row.artwork_url.is_empty() {
                    jobs.push(ArtworkJob {
                        target: ArtworkTarget::SearchAlbum { idx: start + i },
                        url: row.artwork_url.clone(),
                    });
                }
            }
        }
        MoreRows::Tracks(rows) => {
            for (i, row) in rows.iter().enumerate() {
                if !row.artwork_url.is_empty() {
                    jobs.push(ArtworkJob {
                        target: ArtworkTarget::SearchTrack { idx: start + i },
                        url: row.artwork_url.clone(),
                    });
                }
            }
        }
        MoreRows::Artists(rows) => {
            for (i, row) in rows.iter().enumerate() {
                if !row.artwork_url.is_empty() {
                    jobs.push(ArtworkJob {
                        target: ArtworkTarget::SearchArtist { idx: start + i },
                        url: row.artwork_url.clone(),
                    });
                }
            }
        }
        MoreRows::Playlists(rows) => {
            for (i, row) in rows.iter().enumerate() {
                if !row.artwork_url.is_empty() {
                    jobs.push(ArtworkJob {
                        target: ArtworkTarget::SearchPlaylist { idx: start + i },
                        url: row.artwork_url.clone(),
                    });
                }
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
        let row = map_artist(&artist);
        assert_eq!(row.id, "7");
        assert_eq!(row.name, "Metallica");
        assert_eq!(row.subtitle, "12 albums");
    }
}
