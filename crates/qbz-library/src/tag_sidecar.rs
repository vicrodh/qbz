//! LocalLibrary album tag sidecar support.
//!
//! Sidecar files live next to album folders (default `.qbz.json`) and store
//! album-level + per-track metadata overrides.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{LibraryError, LocalTrack};

const SIDECAR_FILE_NAME: &str = ".qbz.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AlbumMetadataOverride {
    pub album_title: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub catalog_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackMetadataOverride {
    pub file_path: String,
    pub cue_start_secs: Option<f64>,
    pub title: Option<String>,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumTagSidecar {
    pub version: u32,
    pub updated_at: i64,
    pub album: AlbumMetadataOverride,
    pub tracks: Vec<TrackMetadataOverride>,
}

impl AlbumTagSidecar {
    pub fn new(album: AlbumMetadataOverride, tracks: Vec<TrackMetadataOverride>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        Self {
            version: 1,
            updated_at: now,
            album,
            tracks,
        }
    }
}

pub fn sidecar_path(album_dir: &Path) -> PathBuf {
    album_dir.join(SIDECAR_FILE_NAME)
}

pub fn read_album_sidecar(album_dir: &Path) -> Result<Option<AlbumTagSidecar>, LibraryError> {
    let path = sidecar_path(album_dir);
    if !path.exists() {
        return Ok(None);
    }

    let bytes = fs::read(&path).map_err(LibraryError::Io)?;
    let sidecar: AlbumTagSidecar =
        serde_json::from_slice(&bytes).map_err(|e| LibraryError::Metadata(e.to_string()))?;
    Ok(Some(sidecar))
}

pub fn write_album_sidecar(album_dir: &Path, sidecar: &AlbumTagSidecar) -> Result<(), LibraryError> {
    fs::create_dir_all(album_dir).map_err(LibraryError::Io)?;

    let target = sidecar_path(album_dir);
    let tmp = album_dir.join(format!("{}.tmp", SIDECAR_FILE_NAME));
    let content =
        serde_json::to_vec_pretty(sidecar).map_err(|e| LibraryError::Metadata(e.to_string()))?;

    fs::write(&tmp, content).map_err(LibraryError::Io)?;
    fs::rename(&tmp, &target).map_err(LibraryError::Io)?;
    Ok(())
}

pub fn delete_album_sidecar(album_dir: &Path) -> Result<(), LibraryError> {
    let path = sidecar_path(album_dir);
    if !path.exists() {
        return Ok(());
    }
    fs::remove_file(&path).map_err(LibraryError::Io)?;
    Ok(())
}

pub fn apply_sidecar_to_track(track: &mut LocalTrack, sidecar: &AlbumTagSidecar) {
    if let Some(title) = sidecar.album.album_title.as_ref().and_then(|s| normalize(s)) {
        track.album = title.clone();
        track.album_group_title = title.clone();
    }

    if let Some(album_artist) = sidecar.album.album_artist.as_ref().and_then(|s| normalize(s)) {
        track.album_artist = Some(album_artist.clone());
    }

    if let Some(year) = sidecar.album.year {
        track.year = Some(year);
    }

    if let Some(genre) = sidecar.album.genre.as_ref().and_then(|s| normalize(s)) {
        track.genre = Some(genre.clone());
    }

    if let Some(cat) = sidecar
        .album
        .catalog_number
        .as_ref()
        .and_then(|s| normalize(s))
    {
        track.catalog_number = Some(cat.clone());
    }

    if let Some(entry) = sidecar.tracks.iter().find(|t| {
        t.file_path == track.file_path
            && match (t.cue_start_secs, track.cue_start_secs) {
                (Some(a), Some(b)) => (a - b).abs() < 0.001,
                (None, None) => true,
                _ => false,
            }
    }) {
        if let Some(title) = entry.title.as_ref().and_then(|s| normalize(s)) {
            track.title = title.clone();
        }
        if let Some(disc) = entry.disc_number {
            track.disc_number = Some(disc);
        }
        if let Some(no) = entry.track_number {
            track.track_number = Some(no);
        }
    }
}

fn normalize(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
