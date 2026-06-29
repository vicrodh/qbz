//! Library > Favorites controller — fetches the user's saved
//! tracks / albums / artists via `QbzCore::get_favorites` and pushes
//! them into `FavoritesState`. Mirrors Tauri's FavoritesView.svelte
//! data flow: each tab is fetched lazily the first time it is opened.
//!
//! `get_favorites` returns a raw JSON value shaped
//! `{ <type>: { items: [...], total: N } }`; this module parses the
//! relevant branch into typed qbz-models items and maps them to the
//! Slint row/card structs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{Album, Artist, Playlist, Track};
use serde::Deserialize;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::album_map::{self, map_album, to_item, AlbumCard};
use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::search::{self, PlaylistRow};
use crate::{
    AlbumCardItem, AlphaJump, AppWindow, DiscoverSection, FavArtistSection, FavoriteArtistItem,
    FavoriteLabelItem, FavoritesState, SearchPlaylistItem, TrackItem,
};

/// Page size — matches Tauri's FAVORITES_PAGE_SIZE. We fetch one
/// page on tab open (favorites lists are typically small; full
/// pagination can come later).
pub const PAGE_SIZE: u32 = 500;

/// Hard ceiling on favorites pulled across all pages (mirrors Tauri's
/// FAVORITES_PAGE_SIZE * FAVORITES_MAX_PAGES ceiling).
const MAX_ITEMS: usize = 10_000;

/// The loaded favorite tracks as a play-ready queue source (Play all /
/// Shuffle). Set on the UI thread by `apply_favorites`.
static FAV_CURRENT: LazyLock<Mutex<Vec<Track>>> = LazyLock::new(|| Mutex::new(Vec::new()));

thread_local! {
    /// Track id -> genre name, for the favorites Tracks genre filter
    /// (TrackItem carries no genre). Set on the UI thread by apply.
    static FAV_TRACK_GENRE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Which favorites tab to load.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FavTab {
    Tracks,
    Albums,
    Artists,
    Playlists,
    Labels,
}

impl FavTab {
    pub fn from_route(route: &str) -> Option<Self> {
        Self::from_tab_id(route.strip_prefix("favorites-")?)
    }

    pub fn from_tab_id(id: &str) -> Option<Self> {
        match id {
            "tracks" => Some(Self::Tracks),
            "albums" => Some(Self::Albums),
            "artists" => Some(Self::Artists),
            "playlists" => Some(Self::Playlists),
            "labels" => Some(Self::Labels),
            _ => None,
        }
    }

    /// The Qobuz favType string + the JSON branch key (for the
    /// get_favorites-backed tabs).
    fn key(self) -> &'static str {
        match self {
            Self::Tracks => "tracks",
            Self::Albums => "albums",
            Self::Artists => "artists",
            Self::Playlists => "playlists",
            Self::Labels => "labels",
        }
    }
}

/// Favorites-labels response item — the qbz-models `Label` is just
/// {id, name}, but the favorites payload carries an image + count,
/// so parse into this richer local shape. `image` is a bare string on
/// the wire (LegacyLabelDto), but typed as Value to also tolerate the
/// `{mega|extralarge|large|thumbnail|small}` object form other label
/// surfaces return (resolved via `label::extract_label_image`).
#[derive(Deserialize)]
struct FavLabel {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    albums_count: Option<u32>,
    #[serde(default)]
    image: Option<serde_json::Value>,
}

pub enum FavData {
    Tracks { items: Vec<TrackCard>, play: Vec<Track>, total: usize },
    Albums { items: Vec<AlbumCard>, total: usize },
    Artists { items: Vec<ArtistCard>, total: usize },
    Playlists { favorites: Vec<PlaylistRow>, following: Vec<PlaylistRow> },
    Labels { items: Vec<LabelCard>, total: usize },
}

#[derive(Clone)]
pub struct TrackCard {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    /// Composer id for the blacklist row stamp (D-FEAT: performer OR composer);
    /// "" when the track carries no composer.
    pub composer_id: String,
    pub album: String,
    pub album_id: String,
    pub genre: String,
    pub duration: String,
    pub quality_tier: String,
    pub quality_detail: String,
    pub explicit: bool,
    pub artwork_url: String,
}

#[derive(Clone)]
pub struct ArtistCard {
    pub id: String,
    pub name: String,
    pub image_url: String,
}

#[derive(Clone)]
pub struct LabelCard {
    pub id: String,
    pub name: String,
    pub albums_line: String,
    pub image_url: String,
}

/// Fetch the full set of favorite ALBUM ids from the server (paginated),
/// for seeding `fav_cache` at login so the album header heart is correct
/// without first visiting the Favorites view. Mirrors Tauri's
/// `albumFavoritesStore.syncFromApi`.
pub async fn favorite_album_ids<A>(
    runtime: &Arc<AppRuntime<A>>,
) -> std::collections::HashSet<String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let mut ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut offset = 0u32;
    loop {
        let value = match runtime.core().get_favorites("albums", PAGE_SIZE, offset).await {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[qbz-slint] favorite album ids fetch failed: {e}");
                break;
            }
        };
        let branch = value.get("albums");
        let total = branch
            .and_then(|b| b.get("total"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as usize;
        let page: Vec<serde_json::Value> = branch
            .and_then(|b| b.get("items"))
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default();
        let page_len = page.len();
        for item in &page {
            match item.get("id") {
                Some(v) if v.is_string() => {
                    if let Some(s) = v.as_str() {
                        ids.insert(s.to_string());
                    }
                }
                Some(v) if v.is_u64() => {
                    if let Some(n) = v.as_u64() {
                        ids.insert(n.to_string());
                    }
                }
                _ => {}
            }
        }
        offset += page_len as u32;
        let exhausted = page_len < PAGE_SIZE as usize
            || (total > 0 && offset as usize >= total)
            || ids.len() >= MAX_ITEMS;
        if exhausted {
            break;
        }
    }
    ids
}

/// Fetch + parse one favorites tab.
pub async fn load_favorites<A>(
    runtime: &Arc<AppRuntime<A>>,
    tab: FavTab,
) -> Result<FavData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    // Playlists has two sub-tabs from two different sources (mirror Tauri):
    //   - Following = playlists the user follows on Qobuz but does NOT own
    //     (`get_user_playlists` filtered by `owner.id != current_user_id`).
    //   - Library   = LOCALLY-favorited playlist ids (SQLite), in favorited
    //     order. We intersect the already-fetched `get_user_playlists` set
    //     (cheap, no extra fetch); for a favorited id not in that set we fall
    //     back to a single `get_playlist`.
    if tab == FavTab::Playlists {
        let all = runtime
            .core()
            .get_user_playlists()
            .await
            .map_err(|e| e.to_string())?;
        let uid = crate::library_db::current_user_id();
        let following: Vec<PlaylistRow> = match uid {
            Some(uid) => all
                .iter()
                .filter(|p| p.owner.id != uid)
                .cloned()
                .map(search::map_playlist)
                .collect(),
            None => Vec::new(),
        };
        let fav_ids =
            crate::library_db::with_db(|db| db.get_favorite_playlist_ids()).unwrap_or_default();
        let by_id: HashMap<u64, &Playlist> = all.iter().map(|p| (p.id, p)).collect();
        let mut favorites: Vec<PlaylistRow> = Vec::with_capacity(fav_ids.len());
        for fid in fav_ids {
            if let Some(p) = by_id.get(&fid) {
                favorites.push(search::map_playlist((**p).clone()));
            } else if let Ok(p) = runtime.core().get_playlist(fid).await {
                favorites.push(search::map_playlist(p));
            }
        }
        return Ok(FavData::Playlists { favorites, following });
    }

    // Page through the favorites until the API is exhausted (mirrors
    // Tauri's fetchAllFavorites: keep pulling until a short page or
    // offset >= total), capped at MAX_ITEMS so a pathological library
    // can't loop forever.
    let mut total: usize;
    let mut all_items: Vec<serde_json::Value> = Vec::new();
    let mut offset = 0u32;
    loop {
        let value = runtime
            .core()
            .get_favorites(tab.key(), PAGE_SIZE, offset)
            .await
            .map_err(|e| e.to_string())?;
        let branch = value.get(tab.key());
        total = branch
            .and_then(|b| b.get("total"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as usize;
        let page: Vec<serde_json::Value> = branch
            .and_then(|b| b.get("items"))
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default();
        let page_len = page.len();
        all_items.extend(page);
        offset += page_len as u32;
        let exhausted = page_len < PAGE_SIZE as usize
            || (total > 0 && offset as usize >= total)
            || all_items.len() >= MAX_ITEMS;
        if exhausted {
            break;
        }
    }
    let items = serde_json::Value::Array(all_items);

    Ok(match tab {
        FavTab::Tracks => {
            let tracks: Vec<Track> = serde_json::from_value(items).unwrap_or_default();
            let play = tracks.clone();
            FavData::Tracks {
                items: tracks.into_iter().map(map_track).collect(),
                play,
                total,
            }
        }
        FavTab::Albums => {
            let albums: Vec<Album> = serde_json::from_value(items).unwrap_or_default();
            FavData::Albums {
                items: albums.into_iter().map(map_album).collect(),
                total,
            }
        }
        FavTab::Artists => {
            let artists: Vec<Artist> = serde_json::from_value(items).unwrap_or_default();
            FavData::Artists {
                items: artists.into_iter().map(map_artist).collect(),
                total,
            }
        }
        FavTab::Labels => {
            let labels: Vec<FavLabel> = serde_json::from_value(items).unwrap_or_default();
            FavData::Labels {
                items: labels.into_iter().map(map_label).collect(),
                total,
            }
        }
        FavTab::Playlists => unreachable!("handled above"),
    })
}

/// All five favorites tab counts, seeded up front so the tab badges are
/// ready before the user opens a given tab.
pub struct FavCounts {
    pub tracks: i32,
    pub albums: i32,
    pub artists: i32,
    pub playlists: i32,
    pub labels: i32,
}

async fn total_for<A>(runtime: &Arc<AppRuntime<A>>, key: &str) -> i32
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    runtime
        .core()
        .get_favorites(key, 1, 0)
        .await
        .ok()
        .and_then(|v| {
            v.get(key)
                .and_then(|b| b.get("total"))
                .and_then(|t| t.as_u64())
        })
        .unwrap_or(0) as i32
}

/// Fetch the five favorites counts (cheap limit=1 probes + the playlist
/// count). Runs on a worker; apply with `apply_counts` on the UI thread.
pub async fn load_counts<A>(runtime: &Arc<AppRuntime<A>>) -> FavCounts
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let tracks = total_for(runtime, "tracks").await;
    let albums = total_for(runtime, "albums").await;
    let artists = total_for(runtime, "artists").await;
    let labels = total_for(runtime, "labels").await;
    // The tab badge counts the LOCAL Library (favorited) playlists, matching
    // the "favorites" semantics (cheap local read, no network).
    let playlists = crate::library_db::with_db(|db| db.get_favorite_playlist_ids())
        .map(|ids| ids.len() as i32)
        .unwrap_or(0);
    FavCounts {
        tracks,
        albums,
        artists,
        playlists,
        labels,
    }
}

/// Apply the seeded counts to `FavoritesState` (the tab badges).
pub fn apply_counts(window: &AppWindow, c: FavCounts) {
    let st = window.global::<FavoritesState>();
    st.set_tracks_total(c.tracks);
    st.set_albums_total(c.albums);
    st.set_artists_total(c.artists);
    st.set_playlists_total(c.playlists);
    st.set_labels_total(c.labels);
}

fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn map_track(track: Track) -> TrackCard {
    let mut title = track.title;
    if let Some(version) = track.version.as_ref().filter(|v| !v.is_empty()) {
        title = format!("{title} ({version})");
    }
    let artwork_url = track
        .album
        .as_ref()
        .and_then(|a| a.image.best().cloned())
        .unwrap_or_default();
    let album = track
        .album
        .as_ref()
        .map(|a| a.title.clone())
        .unwrap_or_default();
    let album_id = track.album.as_ref().map(|a| a.id.clone()).unwrap_or_default();
    let genre = track
        .album
        .as_ref()
        .and_then(|a| a.genre.as_ref())
        .map(|g| g.name.clone())
        .unwrap_or_default();
    let (artist, artist_id) = track
        .performer
        .map(|p| (p.name, p.id.to_string()))
        .unwrap_or_default();
    // Composer id for the blacklist row stamp (D-FEAT: performer OR composer).
    let composer_id = track
        .composer
        .map(|c| c.id.to_string())
        .unwrap_or_default();
    TrackCard {
        id: track.id.to_string(),
        title,
        artist,
        artist_id,
        composer_id,
        album,
        album_id,
        genre,
        duration: mmss(track.duration),
        quality_tier: album_map::tier(track.maximum_bit_depth).to_string(),
        quality_detail: crate::quality::detail(
            track.maximum_bit_depth,
            track.maximum_sampling_rate,
        ),
        explicit: track.parental_warning,
        artwork_url,
    }
}

fn map_artist(artist: Artist) -> ArtistCard {
    // albums_count is deliberately NOT shown (Tauri #169: Qobuz's count
    // includes compilations/tributes and is misleadingly high).
    ArtistCard {
        id: artist.id.to_string(),
        name: artist.name,
        image_url: artist
            .image
            .and_then(|img| img.best().cloned())
            .unwrap_or_default(),
    }
}

fn map_label(label: FavLabel) -> LabelCard {
    // Tauri's favorites label card says "{n} albums" (library.albumCount),
    // matching the sibling FavArtistCard's "{n} albums".
    let albums_line = match label.albums_count {
        Some(n) if n > 0 => format!("{} albums", n),
        _ => String::new(),
    };
    LabelCard {
        id: label.id.to_string(),
        name: label.name,
        albums_line,
        image_url: crate::label::extract_label_image(label.image.as_ref()),
    }
}

pub fn apply_favorites(window: &AppWindow, data: FavData) {
    let state = window.global::<FavoritesState>();
    match data {
        FavData::Tracks { items, play, total } => {
            if let Ok(mut current) = FAV_CURRENT.lock() {
                *current = play;
            }
            FAV_TRACK_GENRE.with(|m| {
                *m.borrow_mut() = items.iter().map(|t| (t.id.clone(), t.genre.clone())).collect();
            });
            let rows: Vec<TrackItem> = items
                .into_iter()
                .map(|t| TrackItem {
                    is_blacklisted: crate::artist_blacklist::stamp_row(
                        "qobuz",
                        &[t.artist_id.as_str(), t.composer_id.as_str()],
                    ),
                    id: t.id.clone().into(),
                    number: "".into(),
                    title: t.title.into(),
                    artist: t.artist.into(),
                    album: t.album.into(),
                    duration: t.duration.into(),
                    quality_tier: t.quality_tier.into(),
                    quality_detail: t.quality_detail.into(),
                    explicit: t.explicit,
                    selected: false,
                    artwork_url: t.artwork_url.into(),
                    artwork: slint::Image::default(),
                    // Everything in the Favorites > Tracks tab is, by
                    // definition, a favorite.
                    is_favorite: true,
                    artist_id: t.artist_id.into(),
                    album_id: t.album_id.into(),
                    removing: false,
                    cache_status: if crate::offline_cache::is_cached(&t.id) { 3 } else { 0 },
                    cache_progress: 0.0,
                    source: "qobuz".into(),
                    unlocking: false,
                    // Disc grouping is album-detail only; flat lists carry none.
                    disc_header_number: 0,
                })
                .collect();
            // `tracks` is the full set the artwork pipeline targets;
            // `tracks-visible` (what the list renders) shares the same
            // model until a search filter forks it, so artwork stays live.
            let model = ModelRc::new(VecModel::from(rows));
            state.set_tracks(model.clone());
            state.set_tracks_visible(model);
            state.set_tracks_total(total as i32);
            state.set_tracks_search("".into());
            // Apply the (persisted) group mode to the freshly loaded set.
            derive_tracks(window);
        }
        FavData::Albums { items, total } => {
            // Everything in the Albums tab is a favorite -> filled heart.
            let cards: Vec<AlbumCardItem> = items
                .into_iter()
                .map(|c| {
                    let mut it = to_item(c);
                    it.is_favorite = true;
                    it
                })
                .collect();
            // `albums` is the full set (artwork target); `albums-visible`
            // (what the grid/list renders) shares it until a search/sort
            // forks it, so artwork stays live.
            let model = ModelRc::new(VecModel::from(cards));
            let n = model.row_count() as i32;
            state.set_albums(model.clone());
            state.set_albums_visible(model);
            state.set_albums_total(total as i32);
            state.set_albums_shown(n);
            state.set_albums_search("".into());
            // Apply the (persisted) sort + group to the freshly loaded set.
            derive_albums(window);
        }
        FavData::Artists { items, total } => {
            let cards: Vec<FavoriteArtistItem> = items
                .into_iter()
                .map(|a| FavoriteArtistItem {
                    id: a.id.into(),
                    name: a.name.into(),
                    image_url: a.image_url.into(),
                    image: slint::Image::default(),
                })
                .collect();
            let model = ModelRc::new(VecModel::from(cards));
            state.set_artists(model.clone());
            state.set_artists_visible(model);
            state.set_artists_total(total as i32);
            state.set_artists_search("".into());
            derive_artists(window);
        }
        FavData::Playlists { favorites, following } => {
            let fav_items: Vec<SearchPlaylistItem> =
                favorites.into_iter().map(search::playlist_item).collect();
            let following_items: Vec<SearchPlaylistItem> =
                following.into_iter().map(search::playlist_item).collect();
            // Tab badge = Library (favorited) count; Following badge separate.
            state.set_playlists_total(fav_items.len() as i32);
            state.set_playlists_following_count(following_items.len() as i32);
            state.set_playlists_favorites(ModelRc::new(VecModel::from(fav_items)));
            state.set_playlists_following(ModelRc::new(VecModel::from(following_items)));
            state.set_playlists_search("".into());
            // Seed `playlists-visible` for the current sub-tab (shares the
            // source model until a search forks it, so collage artwork stays live).
            derive_playlists(window);
        }
        FavData::Labels { items, total } => {
            let rows: Vec<FavoriteLabelItem> = items
                .into_iter()
                .map(|l| FavoriteLabelItem {
                    id: l.id.into(),
                    name: l.name.into(),
                    albums_line: l.albums_line.into(),
                    image_url: l.image_url.into(),
                    image: slint::Image::default(),
                })
                .collect();
            // `labels` is the full set the artwork pipeline targets;
            // `labels-visible` (what the grid renders) shares it until a
            // search filter forks it, so artwork stays live.
            let model = ModelRc::new(VecModel::from(rows));
            state.set_labels(model);
            state.set_labels_total(total as i32);
            state.set_labels_search("".into());
            derive_labels(window);
        }
    }
    state.set_loading(false);
}

/// Re-derive the rendered Tracks list (`tracks-visible`) from the full
/// `tracks` set and the search query. An empty query shares the full
/// model so artwork keeps updating in place (the LabelState albums/visible
/// pattern); a query forks a filtered clone (each row carries its already
/// decoded artwork, so no re-fetch).
pub fn derive_tracks(window: &AppWindow) {
    let state = window.global::<FavoritesState>();
    let query_owned = state.get_tracks_search().to_lowercase();
    let query = query_owned.trim();
    let group = state.get_tracks_group_mode().to_string();
    let genre_names = crate::genre_filter::selected_names("favorites");
    let all = state.get_tracks();
    state.set_tracks_alpha(ModelRc::new(VecModel::from(Vec::<AlphaJump>::new())));
    // Fast path: no search + no grouping + no genre filter -> share model.
    if query.is_empty() && group == "off" && genre_names.is_empty() {
        state.set_tracks_visible(all);
        return;
    }
    let mut filtered: Vec<TrackItem> = (0..all.row_count())
        .filter_map(|i| all.row_data(i))
        .filter(|t| {
            (query.is_empty()
                || t.title.to_lowercase().contains(query)
                || t.artist.to_lowercase().contains(query)
                || t.album.to_lowercase().contains(query))
                && track_genre_matches(t.id.as_str(), &genre_names)
        })
        .collect();
    // Group-by reorders the rows so a group's tracks sit together (Tauri
    // adds visible headers; v1 here is group-ordering without header rows
    // until the list is virtualized).
    let lc = |s: &slint::SharedString| s.to_lowercase();
    match group.as_str() {
        "album" => {
            filtered.sort_by(|a, b| lc(&a.album).cmp(&lc(&b.album)).then(lc(&a.title).cmp(&lc(&b.title))))
        }
        "artist" => filtered.sort_by(|a, b| {
            lc(&a.artist)
                .cmp(&lc(&b.artist))
                .then(lc(&a.album).cmp(&lc(&b.album)))
                .then(lc(&a.title).cmp(&lc(&b.title)))
        }),
        "name" => filtered.sort_by(|a, b| lc(&a.title).cmp(&lc(&b.title))),
        _ => {}
    }
    // A-Z jump strip for name grouping: first row index per distinct initial.
    if group == "name" {
        let mut jumps: Vec<AlphaJump> = Vec::new();
        let mut last = String::new();
        for (i, t) in filtered.iter().enumerate() {
            let key = album_alpha_key(t.title.as_str());
            if key != last {
                jumps.push(AlphaJump {
                    letter: key.clone().into(),
                    index: i as i32,
                });
                last = key;
            }
        }
        state.set_tracks_alpha(ModelRc::new(VecModel::from(jumps)));
    }
    state.set_tracks_visible(ModelRc::new(VecModel::from(filtered)));
}

/// Re-derive the rendered Labels grid (`labels-visible`) from the full
/// `labels` set and the search query. An empty query shares the full
/// model so artwork keeps updating in place; a query forks a filtered
/// clone (name-only substring match, mirrors Tauri's filteredLabels).
pub fn derive_labels(window: &AppWindow) {
    let state = window.global::<FavoritesState>();
    let query_owned = state.get_labels_search().to_lowercase();
    let query = query_owned.trim();
    let all = state.get_labels();
    if query.is_empty() {
        state.set_labels_visible(all);
        return;
    }
    let filtered: Vec<FavoriteLabelItem> = (0..all.row_count())
        .filter_map(|i| all.row_data(i))
        .filter(|l| l.name.to_lowercase().contains(query))
        .collect();
    state.set_labels_visible(ModelRc::new(VecModel::from(filtered)));
}

/// Re-derive the rendered Playlists grid/list (`playlists-visible`) from the
/// active sub-tab source (`playlists-favorites` / `playlists-following`) and
/// the search query. Empty query shares the source model so collage artwork
/// stays live; a query forks a name/owner-filtered clone (mirrors Tauri's
/// filteredPlaylists, which matches name OR owner — the owner is part of the
/// item subtitle). No sort, no group (Tauri has none).
pub fn derive_playlists(window: &AppWindow) {
    let state = window.global::<FavoritesState>();
    let source = if state.get_playlists_sub_tab().as_str() == "following" {
        state.get_playlists_following()
    } else {
        state.get_playlists_favorites()
    };
    let query_owned = state.get_playlists_search().to_lowercase();
    let query = query_owned.trim();
    if query.is_empty() {
        state.set_playlists_visible(source);
        return;
    }
    let filtered: Vec<SearchPlaylistItem> = (0..source.row_count())
        .filter_map(|i| source.row_data(i))
        .filter(|p| {
            p.title.to_lowercase().contains(query) || p.subtitle.to_lowercase().contains(query)
        })
        .collect();
    state.set_playlists_visible(ModelRc::new(VecModel::from(filtered)));
}

/// Re-derive the rendered Artists grid (`artists-visible`) from the full
/// `artists` set + the search query (name substring; mirrors Tauri's
/// filteredArtists). A-Z grouping + the alpha strip are layered on later.
pub fn derive_artists(window: &AppWindow) {
    let state = window.global::<FavoritesState>();
    let query_owned = state.get_artists_search().to_lowercase();
    let query = query_owned.trim();
    // The sidepanel left list is ALWAYS A-Z grouped (independent of the grid
    // group toggle), so it shows letter headers + the alpha jump strip.
    let group = state.get_artists_group_enabled()
        || state.get_artists_view_mode().as_str() == "sidepanel";
    let all = state.get_artists();

    // Flat (search-filtered) model. Share `all` when no query so artwork
    // keeps updating in place; a query forks a filtered clone.
    let filtered: Vec<FavoriteArtistItem> = if query.is_empty() {
        (0..all.row_count()).filter_map(|i| all.row_data(i)).collect()
    } else {
        (0..all.row_count())
            .filter_map(|i| all.row_data(i))
            .filter(|a| a.name.to_lowercase().contains(query))
            .collect()
    };
    state.set_artists_shown(filtered.len() as i32);
    if query.is_empty() {
        state.set_artists_visible(all);
    } else {
        state.set_artists_visible(ModelRc::new(VecModel::from(filtered.clone())));
    }

    // A-Z grouping (grid grouped mode): bucket by first letter, sections
    // ordered (# first then A-Z), with an alpha jump per section.
    if !group {
        state.set_artists_grouped(ModelRc::new(VecModel::from(Vec::<FavArtistSection>::new())));
        state.set_artists_alpha(ModelRc::new(VecModel::from(Vec::<AlphaJump>::new())));
        return;
    }
    let mut sorted = filtered;
    sorted.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let mut map: Vec<(String, Vec<FavoriteArtistItem>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for item in sorted {
        let key = album_alpha_key(item.name.as_str());
        let idx = *index.entry(key.clone()).or_insert_with(|| {
            map.push((key.clone(), Vec::new()));
            map.len() - 1
        });
        map[idx].1.push(item);
    }
    map.sort_by(|(a, _), (b, _)| match (a == "#", b == "#") {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    });
    let alpha: Vec<AlphaJump> = map
        .iter()
        .enumerate()
        .map(|(i, (k, _))| AlphaJump {
            letter: k.clone().into(),
            index: i as i32,
        })
        .collect();
    let sections: Vec<FavArtistSection> = map
        .into_iter()
        .map(|(key, artists)| FavArtistSection {
            key: key.clone().into(),
            title: key.into(),
            artists: ModelRc::new(VecModel::from(artists)),
        })
        .collect();
    state.set_artists_alpha(ModelRc::new(VecModel::from(alpha)));
    state.set_artists_grouped(ModelRc::new(VecModel::from(sections)));
}

/// Apply the selected artist's albums to the sidepanel right column. Reuses
/// the standalone artist page's `/artist/page` `release_type` classifier
/// (`artist::load_artist` → `ReleaseSection`s), per the user's decision, so
/// the sections are server-authoritative (Discography / EPs & Singles / Live
/// / Compilations / Others).
pub fn apply_selected_artist(window: &AppWindow, sections: Vec<crate::artist::ReleaseSection>) {
    let st = window.global::<FavoritesState>();
    let mut total = 0i32;
    let ds: Vec<DiscoverSection> = sections
        .into_iter()
        .map(|s| {
            let items: Vec<AlbumCardItem> =
                s.cards.into_iter().map(crate::artist::card_to_item).collect();
            total += items.len() as i32;
            DiscoverSection {
                title: s.title.into(),
                endpoint: "".into(),
                albums: ModelRc::new(VecModel::from(items)),
            }
        })
        .collect();
    st.set_selected_artist_sections(ModelRc::new(VecModel::from(ds)));
    st.set_selected_albums_total(total);
    st.set_selected_albums_loading(false);
    st.set_selected_albums_error("".into());
}

/// Artwork jobs for the selected artist's sidepanel album cards.
pub fn selected_artist_artwork_jobs(
    sections: &[crate::artist::ReleaseSection],
) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    for (section, sec) in sections.iter().enumerate() {
        for (index, card) in sec.cards.iter().enumerate() {
            if !card.artwork_url.is_empty() {
                jobs.push(ArtworkJob {
                    url: card.artwork_url.clone(),
                    target: ArtworkTarget::FavoriteArtistAlbum { section, index },
                });
            }
        }
    }
    jobs
}

/// The loaded favorite tracks as a play-ready queue (Play all).
pub fn play_tracks() -> Vec<Track> {
    FAV_CURRENT.lock().map(|c| c.clone()).unwrap_or_default()
}

/// The favorite tracks in a fresh random order (Shuffle). Mirrors
/// playlist::shuffled_tracks (time-seeded xorshift Fisher-Yates).
pub fn shuffled_tracks() -> Vec<Track> {
    let mut tracks = play_tracks();
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
    tracks
}

/// Re-derive the rendered Albums list (`albums-visible`) from the full
/// `albums` set + the search query and sort key. Empty query + default
/// order shares the full model so artwork stays live; otherwise forks a
/// filtered + sorted clone (mirrors label.rs::derive_releases).
pub fn derive_albums(window: &AppWindow) {
    let state = window.global::<FavoritesState>();
    let query_owned = state.get_albums_search().to_lowercase();
    let query = query_owned.trim();
    let sort = state.get_albums_sort_by().to_string();
    let group = state.get_albums_group_mode().to_string();
    let genre_names = crate::genre_filter::selected_names("favorites");
    let all = state.get_albums();
    state.set_albums_alpha(ModelRc::new(VecModel::from(Vec::<AlphaJump>::new())));
    let empty_sections = || ModelRc::new(VecModel::from(Vec::<DiscoverSection>::new()));

    // Fast path: no filter, default order, no grouping, no genre -> share.
    if query.is_empty() && sort == "default" && group == "off" && genre_names.is_empty() {
        let n = all.row_count() as i32;
        state.set_albums_visible(all);
        state.set_albums_grouped(empty_sections());
        state.set_albums_shown(n);
        return;
    }

    let mut filtered: Vec<AlbumCardItem> = (0..all.row_count())
        .filter_map(|i| all.row_data(i))
        .filter(|a| {
            (query.is_empty()
                || a.title.to_lowercase().contains(query)
                || a.artist.to_lowercase().contains(query))
                && album_genre_matches(a.genre.as_str(), &genre_names)
        })
        .collect();
    album_map::sort_album_items(&mut filtered, &sort);
    state.set_albums_shown(filtered.len() as i32);

    if group == "off" {
        state.set_albums_visible(ModelRc::new(VecModel::from(filtered)));
        state.set_albums_grouped(empty_sections());
        return;
    }

    // Grouped: bucket by artist name, or by the title's first letter
    // (# bucket for non-alphabetic), sections ordered alphabetically.
    let mut map: Vec<(String, Vec<AlbumCardItem>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for item in filtered {
        let key = if group == "artist" {
            let a = item.artist.to_string();
            if a.is_empty() {
                "Unknown".to_string()
            } else {
                a
            }
        } else {
            album_alpha_key(item.title.as_str())
        };
        let idx = *index.entry(key.clone()).or_insert_with(|| {
            map.push((key.clone(), Vec::new()));
            map.len() - 1
        });
        map[idx].1.push(item);
    }
    map.sort_by(|(a, _), (b, _)| match (a == "#", b == "#") {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    });
    // A-Z jump strip for alpha grouping: the section letters in order.
    if group == "alpha" {
        let alpha: Vec<AlphaJump> = map
            .iter()
            .enumerate()
            .map(|(i, (k, _))| AlphaJump {
                letter: k.clone().into(),
                index: i as i32,
            })
            .collect();
        state.set_albums_alpha(ModelRc::new(VecModel::from(alpha)));
    }
    let sections: Vec<DiscoverSection> = map
        .into_iter()
        .map(|(key, items)| DiscoverSection {
            title: key.into(),
            endpoint: "".into(),
            albums: ModelRc::new(VecModel::from(items)),
        })
        .collect();
    state.set_albums_grouped(ModelRc::new(VecModel::from(sections)));
    state.set_albums_visible(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
}

/// First-letter bucket key for alpha grouping (# for non-alphabetic).
fn album_alpha_key(title: &str) -> String {
    match title.trim().chars().next() {
        Some(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase().to_string(),
        Some(c) if c.is_alphabetic() => c.to_uppercase().to_string(),
        _ => "#".to_string(),
    }
}

/// True if `genre` matches any selected genre name (favorites filter).
fn album_genre_matches(genre: &str, names: &[String]) -> bool {
    if names.is_empty() {
        return true;
    }
    let g = genre.to_lowercase();
    names.iter().any(|n| g.contains(&n.to_lowercase()))
}

/// Same, looking the track's genre up in the id->genre map.
fn track_genre_matches(id: &str, names: &[String]) -> bool {
    if names.is_empty() {
        return true;
    }
    FAV_TRACK_GENRE.with(|m| {
        m.borrow()
            .get(id)
            .map(|g| {
                let gl = g.to_lowercase();
                names.iter().any(|n| gl.contains(&n.to_lowercase()))
            })
            .unwrap_or(false)
    })
}

/// A random album id from the currently-visible set (Shuffle / random).
pub fn random_visible_album(window: &AppWindow) -> Option<String> {
    let model = window.global::<FavoritesState>().get_albums_visible();
    let n = model.row_count();
    if n == 0 {
        return None;
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let idx = (seed % n as u64) as usize;
    model.row_data(idx).map(|a| a.id.to_string())
}

/// A random artist id from the currently-visible favorites set. Tauri's
/// Artists header Shuffle opens a random ARTIST (not a random album).
pub fn random_visible_artist(window: &AppWindow) -> Option<String> {
    let model = window.global::<FavoritesState>().get_artists_visible();
    let n = model.row_count();
    if n == 0 {
        return None;
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let idx = (seed % n as u64) as usize;
    model.row_data(idx).map(|a| a.id.to_string())
}

/// A random playlist id from the currently-visible Playlists set (for the
/// Playlists "random" button — play a random playlist).
pub fn random_visible_playlist(window: &AppWindow) -> Option<String> {
    let model = window.global::<FavoritesState>().get_playlists_visible();
    let n = model.row_count();
    if n == 0 {
        return None;
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let idx = (seed % n as u64) as usize;
    model.row_data(idx).map(|p| p.id.to_string())
}

/// A random label (id, name) from the currently-visible Labels set (for the
/// Labels "random" button — open a random label's landing).
pub fn random_visible_label(window: &AppWindow) -> Option<(String, String)> {
    let model = window.global::<FavoritesState>().get_labels_visible();
    let n = model.row_count();
    if n == 0 {
        return None;
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let idx = (seed % n as u64) as usize;
    model
        .row_data(idx)
        .map(|l| (l.id.to_string(), l.name.to_string()))
}

// ---- Un-favorite in place: fade (set `removing`) then remove -----------

/// Flag the matching track row(s) as removing so they fade out.
pub fn mark_track_removing(window: &AppWindow, id: &str) {
    let state = window.global::<FavoritesState>();
    for model in [state.get_tracks_visible(), state.get_tracks()] {
        for i in 0..model.row_count() {
            if let Some(mut item) = model.row_data(i) {
                if item.id == id && !item.removing {
                    item.removing = true;
                    model.set_row_data(i, item);
                }
            }
        }
    }
}

/// Remove the track row from both the rendered + full models (after fade).
pub fn remove_track_row(window: &AppWindow, id: &str) {
    let state = window.global::<FavoritesState>();
    for model in [state.get_tracks_visible(), state.get_tracks()] {
        if let Some(vm) = model.as_any().downcast_ref::<VecModel<TrackItem>>() {
            for i in 0..vm.row_count() {
                if vm.row_data(i).map(|t| t.id == id).unwrap_or(false) {
                    vm.remove(i);
                    break;
                }
            }
        }
    }
}

/// Flag the matching album card(s) as removing so they fade out.
pub fn mark_album_removing(window: &AppWindow, id: &str) {
    let state = window.global::<FavoritesState>();
    for model in [state.get_albums_visible(), state.get_albums()] {
        for i in 0..model.row_count() {
            if let Some(mut item) = model.row_data(i) {
                if item.id == id && !item.removing {
                    item.removing = true;
                    model.set_row_data(i, item);
                }
            }
        }
    }
}

/// Remove the album card from both the rendered + full models (after fade).
pub fn remove_album_row(window: &AppWindow, id: &str) {
    let state = window.global::<FavoritesState>();
    for model in [state.get_albums_visible(), state.get_albums()] {
        if let Some(vm) = model.as_any().downcast_ref::<VecModel<AlbumCardItem>>() {
            for i in 0..vm.row_count() {
                if vm.row_data(i).map(|a| a.id == id).unwrap_or(false) {
                    vm.remove(i);
                    break;
                }
            }
        }
    }
}

/// Remove a playlist from the Library (favorites) source after a local
/// un-favorite, then re-derive the rendered model + update the tab badge.
/// Following is untouched (a followed playlist stays followed).
pub fn remove_playlist_row(window: &AppWindow, id: &str) {
    let state = window.global::<FavoritesState>();
    let model = state.get_playlists_favorites();
    if let Some(vm) = model.as_any().downcast_ref::<VecModel<SearchPlaylistItem>>() {
        for i in 0..vm.row_count() {
            if vm.row_data(i).map(|p| p.id == id).unwrap_or(false) {
                vm.remove(i);
                break;
            }
        }
    }
    state.set_playlists_total(model.row_count() as i32);
    derive_playlists(window);
}

// ---- Artwork propagation -----------------------------------------------
// The artwork pipeline writes a decoded cover into the SOURCE model
// (`albums` / `tracks`) by index, but the views render the derived
// `albums-visible` / `albums-grouped` / `tracks-visible` models, which are
// independent clones whenever a sort / group / search is active. So a
// late-arriving cover never reached the rendered card (it stayed grey
// until a re-derive). Propagate it into the rendered model(s) by id too.

fn set_artwork_in_albums(model: &ModelRc<AlbumCardItem>, id: &str, image: &slint::Image) {
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.id.as_str() == id {
                item.artwork = image.clone();
                model.set_row_data(i, item);
                break;
            }
        }
    }
}

/// Set a freshly-decoded album cover (by id) on the rendered favorites
/// album models (flat `albums-visible` + every `albums-grouped` section).
pub fn set_album_artwork(window: &AppWindow, id: &str, image: slint::Image) {
    let st = window.global::<FavoritesState>();
    set_artwork_in_albums(&st.get_albums_visible(), id, &image);
    let grouped = st.get_albums_grouped();
    for s in 0..grouped.row_count() {
        if let Some(section) = grouped.row_data(s) {
            set_artwork_in_albums(&section.albums, id, &image);
        }
    }
}

/// Set a freshly-decoded artist photo (by id) on the rendered favorites
/// artist models (flat `artists-visible` + every `artists-grouped` section,
/// which backs both the grouped grid and the sidepanel list). Without this
/// the photo only lands on the source `artists` model and the grouped/
/// sidepanel views show it only after a re-derive (revisit).
pub fn set_artist_image(window: &AppWindow, id: &str, image: slint::Image) {
    let st = window.global::<FavoritesState>();
    let vis = st.get_artists_visible();
    for i in 0..vis.row_count() {
        if let Some(mut item) = vis.row_data(i) {
            if item.id.as_str() == id {
                item.image = image.clone();
                vis.set_row_data(i, item);
                break;
            }
        }
    }
    let grouped = st.get_artists_grouped();
    for s in 0..grouped.row_count() {
        if let Some(section) = grouped.row_data(s) {
            let arts = section.artists;
            for i in 0..arts.row_count() {
                if let Some(mut item) = arts.row_data(i) {
                    if item.id.as_str() == id {
                        item.image = image.clone();
                        arts.set_row_data(i, item);
                        break;
                    }
                }
            }
        }
    }
}

/// Set a freshly-decoded collage cover (by id + slot) on the rendered
/// favorites playlists model (`playlists-visible`), which is a clone of the
/// active sub-tab source whenever a search filter is active.
pub fn set_playlist_cover(window: &AppWindow, id: &str, slot: usize, image: slint::Image) {
    let model = window.global::<FavoritesState>().get_playlists_visible();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.id.as_str() == id {
                match slot {
                    0 => item.cover1 = image,
                    1 => item.cover2 = image,
                    2 => item.cover3 = image,
                    _ => item.cover4 = image,
                }
                model.set_row_data(i, item);
                break;
            }
        }
    }
}

/// Same for the rendered favorites tracks model (`tracks-visible`).
pub fn set_track_artwork(window: &AppWindow, id: &str, image: slint::Image) {
    let model = window.global::<FavoritesState>().get_tracks_visible();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.id.as_str() == id {
                item.artwork = image.clone();
                model.set_row_data(i, item);
                break;
            }
        }
    }
}

// ---- Tracks multi-select (mirrors playlist.rs) -------------------------
// Selection lives on TrackItem.selected in the rendered `tracks-visible`
// model (which shares `tracks` when no search filter is active).

/// Enter/leave multi-select mode; leaving clears the selection.
pub fn set_multi_select(window: &AppWindow, on: bool) {
    window.global::<FavoritesState>().set_tracks_multi_select(on);
    crate::selection::clear_anchor();
    if !on {
        clear_selection(window);
    }
}

/// Recount selected rows into `tracks-selected-count`.
pub fn recount_selected(window: &AppWindow) {
    let state = window.global::<FavoritesState>();
    let model = state.get_tracks_visible();
    let count = (0..model.row_count())
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count();
    state.set_tracks_selected_count(count as i32);
}

/// Select-all toggle: select every visible row, or clear if all selected.
pub fn select_all(window: &AppWindow) {
    let model = window.global::<FavoritesState>().get_tracks_visible();
    let total = model.row_count();
    let selected = (0..total)
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count();
    let target = selected != total;
    for i in 0..total {
        if let Some(mut item) = model.row_data(i) {
            if item.selected != target {
                item.selected = target;
                model.set_row_data(i, item);
            }
        }
    }
    recount_selected(window);
}

/// Deselect every visible row.
pub fn clear_selection(window: &AppWindow) {
    let state = window.global::<FavoritesState>();
    let model = state.get_tracks_visible();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.selected {
                item.selected = false;
                model.set_row_data(i, item);
            }
        }
    }
    state.set_tracks_selected_count(0);
}

/// The ids of the currently-selected visible rows.
pub fn selected_ids(window: &AppWindow) -> Vec<String> {
    let model = window.global::<FavoritesState>().get_tracks_visible();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|t| t.selected)
        .map(|t| t.id.to_string())
        .collect()
}

/// The selected favorite tracks as full Track objects (for bulk enqueue),
/// in favorites order.
pub fn selected_tracks(window: &AppWindow) -> Vec<Track> {
    let ids = selected_ids(window);
    if ids.is_empty() {
        return Vec::new();
    }
    FAV_CURRENT
        .lock()
        .map(|c| {
            c.iter()
                .filter(|t| ids.contains(&t.id.to_string()))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub fn reset_loading(window: &AppWindow) {
    let state = window.global::<FavoritesState>();
    state.set_loading(true);
    state.set_load_error("".into());
}

/// Artwork jobs for the freshly loaded tab.
pub fn artwork_jobs(data: &FavData) -> Vec<ArtworkJob> {
    match data {
        FavData::Tracks { items, .. } => items
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.artwork_url.is_empty())
            .map(|(i, t)| ArtworkJob {
                url: t.artwork_url.clone(),
                target: ArtworkTarget::FavoriteTrack { index: i },
            })
            .collect(),
        FavData::Albums { items, .. } => items
            .iter()
            .enumerate()
            .filter(|(_, a)| !a.artwork_url.is_empty())
            .map(|(i, a)| ArtworkJob {
                url: a.artwork_url.clone(),
                target: ArtworkTarget::FavoriteAlbum { index: i },
            })
            .collect(),
        FavData::Artists { items, .. } => items
            .iter()
            .enumerate()
            .filter(|(_, a)| !a.image_url.is_empty())
            .map(|(i, a)| ArtworkJob {
                url: a.image_url.clone(),
                target: ArtworkTarget::FavoriteArtist { index: i },
            })
            .collect(),
        FavData::Playlists { favorites, following } => {
            fn push(rows: &[PlaylistRow], following: bool, jobs: &mut Vec<ArtworkJob>) {
                for (index, row) in rows.iter().enumerate() {
                    for (slot, url) in row.cover_urls.iter().enumerate().take(4) {
                        if !url.is_empty() {
                            jobs.push(ArtworkJob {
                                url: url.clone(),
                                target: ArtworkTarget::FavPlaylistCover { following, index, slot },
                            });
                        }
                    }
                }
            }
            let mut jobs: Vec<ArtworkJob> = Vec::new();
            push(favorites, false, &mut jobs);
            push(following, true, &mut jobs);
            jobs
        }
        FavData::Labels { items, .. } => items
            .iter()
            .enumerate()
            .filter(|(_, l)| !l.image_url.is_empty())
            .map(|(i, l)| ArtworkJob {
                url: l.image_url.clone(),
                target: ArtworkTarget::FavoriteLabel { index: i },
            })
            .collect(),
    }
}
