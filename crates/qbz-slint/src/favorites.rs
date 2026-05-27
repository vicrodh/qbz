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
use crate::{
    AlbumCardItem, AlphaJump, AppWindow, DiscoverSection, FavoriteArtistItem, FavoriteLabelItem,
    FavoritePlaylistItem, FavoritesState, TrackItem,
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
/// so parse into this richer local shape.
#[derive(Deserialize)]
struct FavLabel {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    albums_count: Option<u32>,
}

pub enum FavData {
    Tracks { items: Vec<TrackCard>, play: Vec<Track>, total: usize },
    Albums { items: Vec<AlbumCard>, total: usize },
    Artists { items: Vec<ArtistCard>, total: usize },
    Playlists { items: Vec<PlaylistCard>, total: usize },
    Labels { items: Vec<LabelCard>, total: usize },
}

#[derive(Clone)]
pub struct TrackCard {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    pub album: String,
    pub album_id: String,
    pub genre: String,
    pub duration: String,
    pub quality_tier: String,
    pub explicit: bool,
    pub artwork_url: String,
}

#[derive(Clone)]
pub struct ArtistCard {
    pub id: String,
    pub name: String,
    pub albums_line: String,
    pub image_url: String,
}

#[derive(Clone)]
pub struct PlaylistCard {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub tracks_line: String,
    pub cover_url: String,
}

#[derive(Clone)]
pub struct LabelCard {
    pub id: String,
    pub name: String,
    pub albums_line: String,
}

/// Fetch + parse one favorites tab.
pub async fn load_favorites<A>(
    runtime: &Arc<AppRuntime<A>>,
    tab: FavTab,
) -> Result<FavData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    // Playlists come from /playlist/getUserPlaylists, not the
    // getUserFavorites envelope — handle them first.
    if tab == FavTab::Playlists {
        let playlists = runtime
            .core()
            .get_user_playlists()
            .await
            .map_err(|e| e.to_string())?;
        let total = playlists.len();
        return Ok(FavData::Playlists {
            items: playlists.into_iter().map(map_playlist).collect(),
            total,
        });
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
    let playlists = runtime
        .core()
        .get_user_playlists()
        .await
        .map(|p| p.len() as i32)
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
    TrackCard {
        id: track.id.to_string(),
        title,
        artist,
        artist_id,
        album,
        album_id,
        genre,
        duration: mmss(track.duration),
        quality_tier: album_map::tier(track.maximum_bit_depth).to_string(),
        explicit: track.parental_warning,
        artwork_url,
    }
}

fn map_artist(artist: Artist) -> ArtistCard {
    let albums_line = match artist.albums_count {
        Some(n) if n > 0 => format!("{} albums", n),
        _ => String::new(),
    };
    ArtistCard {
        id: artist.id.to_string(),
        name: artist.name,
        albums_line,
        image_url: artist
            .image
            .and_then(|img| img.best().cloned())
            .unwrap_or_default(),
    }
}

fn map_playlist(playlist: Playlist) -> PlaylistCard {
    // The highest-resolution non-empty cover list wins (images300 >
    // images150 > images), first entry as the tile cover.
    let cover_url = [&playlist.images300, &playlist.images150, &playlist.images]
        .into_iter()
        .flatten()
        .find(|v| !v.is_empty())
        .and_then(|list| list.first().cloned())
        .unwrap_or_default();
    PlaylistCard {
        id: playlist.id.to_string(),
        name: playlist.name,
        owner: playlist.owner.name,
        tracks_line: format!("{} tracks", playlist.tracks_count),
        cover_url,
    }
}

fn map_label(label: FavLabel) -> LabelCard {
    let albums_line = match label.albums_count {
        Some(n) if n > 0 => format!("{} releases", n),
        _ => String::new(),
    };
    LabelCard {
        id: label.id.to_string(),
        name: label.name,
        albums_line,
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
                    id: t.id.into(),
                    number: "".into(),
                    title: t.title.into(),
                    artist: t.artist.into(),
                    album: t.album.into(),
                    duration: t.duration.into(),
                    quality_tier: t.quality_tier.into(),
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
                    albums_line: a.albums_line.into(),
                    image_url: a.image_url.into(),
                    image: slint::Image::default(),
                })
                .collect();
            state.set_artists(ModelRc::new(VecModel::from(cards)));
            state.set_artists_total(total as i32);
        }
        FavData::Playlists { items, total } => {
            let cards: Vec<FavoritePlaylistItem> = items
                .into_iter()
                .map(|p| FavoritePlaylistItem {
                    id: p.id.into(),
                    name: p.name.into(),
                    owner: p.owner.into(),
                    tracks_line: p.tracks_line.into(),
                    cover_url: p.cover_url.into(),
                    cover: slint::Image::default(),
                })
                .collect();
            state.set_playlists(ModelRc::new(VecModel::from(cards)));
            state.set_playlists_total(total as i32);
        }
        FavData::Labels { items, total } => {
            let rows: Vec<FavoriteLabelItem> = items
                .into_iter()
                .map(|l| FavoriteLabelItem {
                    id: l.id.into(),
                    name: l.name.into(),
                    albums_line: l.albums_line.into(),
                })
                .collect();
            state.set_labels(ModelRc::new(VecModel::from(rows)));
            state.set_labels_total(total as i32);
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

// ---- Tracks multi-select (mirrors playlist.rs) -------------------------
// Selection lives on TrackItem.selected in the rendered `tracks-visible`
// model (which shares `tracks` when no search filter is active).

/// Enter/leave multi-select mode; leaving clears the selection.
pub fn set_multi_select(window: &AppWindow, on: bool) {
    window.global::<FavoritesState>().set_tracks_multi_select(on);
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
        FavData::Playlists { items, .. } => items
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.cover_url.is_empty())
            .map(|(i, p)| ArtworkJob {
                url: p.cover_url.clone(),
                target: ArtworkTarget::FavoritePlaylist { index: i },
            })
            .collect(),
        // Labels render an icon, no remote artwork.
        FavData::Labels { .. } => Vec::new(),
    }
}
