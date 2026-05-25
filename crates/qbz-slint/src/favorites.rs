//! Library > Favorites controller — fetches the user's saved
//! tracks / albums / artists via `QbzCore::get_favorites` and pushes
//! them into `FavoritesState`. Mirrors Tauri's FavoritesView.svelte
//! data flow: each tab is fetched lazily the first time it is opened.
//!
//! `get_favorites` returns a raw JSON value shaped
//! `{ <type>: { items: [...], total: N } }`; this module parses the
//! relevant branch into typed qbz-models items and maps them to the
//! Slint row/card structs.

use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{Album, Artist, Playlist, Track};
use serde::Deserialize;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{
    AlbumCardItem, AppWindow, FavoriteArtistItem, FavoriteLabelItem, FavoritePlaylistItem,
    FavoritesState, TrackItem,
};

/// Page size — matches Tauri's FAVORITES_PAGE_SIZE. We fetch one
/// page on tab open (favorites lists are typically small; full
/// pagination can come later).
pub const PAGE_SIZE: u32 = 500;

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
    Tracks { items: Vec<TrackCard>, total: usize },
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
    pub duration: String,
    pub quality_tier: String,
    pub explicit: bool,
    pub artwork_url: String,
}

#[derive(Clone)]
pub struct AlbumCard {
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

    let value = runtime
        .core()
        .get_favorites(tab.key(), PAGE_SIZE, 0)
        .await
        .map_err(|e| e.to_string())?;

    let branch = value.get(tab.key());
    let total = branch
        .and_then(|b| b.get("total"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0) as usize;
    let items = branch
        .and_then(|b| b.get("items"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    Ok(match tab {
        FavTab::Tracks => {
            let tracks: Vec<Track> = serde_json::from_value(items).unwrap_or_default();
            FavData::Tracks {
                items: tracks.into_iter().map(map_track).collect(),
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

fn tier(bit_depth: Option<u32>) -> &'static str {
    match bit_depth {
        Some(b) if b > 16 => "hires",
        Some(_) => "cd",
        None => "",
    }
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
    TrackCard {
        id: track.id.to_string(),
        title,
        artist: track.performer.map(|p| p.name).unwrap_or_default(),
        duration: mmss(track.duration),
        quality_tier: tier(track.maximum_bit_depth).to_string(),
        explicit: track.parental_warning,
        artwork_url,
    }
}

fn map_album(album: Album) -> AlbumCard {
    let year = crate::dates::release_label(album.release_date_original.as_deref());
    let genre = album.genre.map(|g| g.name).unwrap_or_default();
    let quality_label = match (album.maximum_bit_depth, album.maximum_sampling_rate) {
        (Some(bd), Some(sr)) => format!("{}-bit / {} kHz", bd, sr),
        _ => String::new(),
    };
    AlbumCard {
        id: album.id,
        title: album.title,
        artist: album.artist.name,
        artist_id: album.artist.id.to_string(),
        genre,
        year,
        quality_tier: tier(album.maximum_bit_depth).to_string(),
        quality_label,
        artwork_url: album.image.best().cloned().unwrap_or_default(),
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
        FavData::Tracks { items, total } => {
            let rows: Vec<TrackItem> = items
                .into_iter()
                .map(|t| TrackItem {
                    id: t.id.into(),
                    number: "".into(),
                    title: t.title.into(),
                    artist: t.artist.into(),
                    album: "".into(),
                    duration: t.duration.into(),
                    quality_tier: t.quality_tier.into(),
                    explicit: t.explicit,
                    selected: false,
                    artwork_url: t.artwork_url.into(),
                    artwork: slint::Image::default(),
                })
                .collect();
            state.set_tracks(ModelRc::new(VecModel::from(rows)));
            state.set_tracks_total(total as i32);
        }
        FavData::Albums { items, total } => {
            let cards: Vec<AlbumCardItem> = items
                .into_iter()
                .map(|a| AlbumCardItem {
                    id: a.id.into(),
                    title: a.title.into(),
                    artist: a.artist.into(),
                    artist_id: a.artist_id.into(),
                    genre: a.genre.into(),
                    year: a.year.into(),
                    quality_tier: a.quality_tier.into(),
                    quality_label: a.quality_label.into(),
                    ribbon: "".into(),
                    ribbon_kind: "".into(),
                    artwork_url: a.artwork_url.into(),
                    artwork: slint::Image::default(),
                })
                .collect();
            state.set_albums(ModelRc::new(VecModel::from(cards)));
            state.set_albums_total(total as i32);
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

pub fn reset_loading(window: &AppWindow) {
    window.global::<FavoritesState>().set_loading(true);
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
