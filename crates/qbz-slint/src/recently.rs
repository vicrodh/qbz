//! Recently-played store.
//!
//! A small JSON file at the shared QBZ data path holding the last few
//! played tracks, newest first. Discover Home renders two sections from
//! it — recently-played tracks (slim cards) and recently-played albums
//! (derived by de-duplicating the track history by album). The playback
//! session calls [`record`] when a track starts.
//!
//! Until playback is wired the store is simply empty and the Home
//! sections that read it hide themselves — the data path exists end to
//! end so playback only has to call `record`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How many recent tracks to keep.
const MAX_RECENT: usize = 24;

/// One recently-played track, with the album it belongs to.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecentTrack {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub artwork_url: String,
    #[serde(default)]
    pub album_id: String,
    #[serde(default)]
    pub album_title: String,
    #[serde(default)]
    pub album_artist: String,
    #[serde(default)]
    pub album_artwork_url: String,
}

/// One recently-played album, derived from the track history.
#[derive(Clone, Debug)]
pub struct RecentAlbum {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artwork_url: String,
}

fn store_path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("qbz").join("recently_played.json"))
}

/// Load the recently-played tracks, newest first. Returns an empty list
/// when the store does not exist yet or cannot be read.
pub fn load() -> Vec<RecentTrack> {
    let Some(path) = store_path() else {
        return Vec::new();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Recently-played albums, newest first, de-duplicated from the track
/// history. A track with no album id is skipped.
pub fn load_albums() -> Vec<RecentAlbum> {
    let mut albums: Vec<RecentAlbum> = Vec::new();
    for track in load() {
        if track.album_id.is_empty() || albums.iter().any(|a| a.id == track.album_id) {
            continue;
        }
        albums.push(RecentAlbum {
            id: track.album_id,
            title: track.album_title,
            artist: track.album_artist,
            artwork_url: track.album_artwork_url,
        });
    }
    albums
}

/// Record a played track at the front of the list. Deduplicates by id and
/// caps the list at [`MAX_RECENT`]. Called by the playback session when a
/// track starts.
#[allow(dead_code)] // wired by the playback session
pub fn record(track: RecentTrack) {
    let Some(path) = store_path() else {
        return;
    };
    let mut list = load();
    list.retain(|t| t.id != track.id);
    list.insert(0, track);
    list.truncate(MAX_RECENT);

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("[qbz-slint] recently-played store dir failed: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(&list) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::warn!("[qbz-slint] recently-played write failed: {e}");
            }
        }
        Err(e) => log::warn!("[qbz-slint] recently-played serialize failed: {e}"),
    }
}
