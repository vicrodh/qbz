//! Provider implementations

pub mod apple;
pub mod deezer;
pub mod spotify;
pub mod tidal;

use serde::{Deserialize, Serialize};

use crate::playlist_import::errors::PlaylistImportError;
use crate::playlist_import::models::ImportPlaylist;

/// Which streaming platform a music link belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MusicProvider {
    Spotify,
    AppleMusic,
    Tidal,
    Deezer,
}

/// The kind of resource a music URL points to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MusicResource {
    /// A native Qobuz URL — resolve directly.
    Qobuz,
    /// A single track on a third-party platform.
    Track { provider: MusicProvider, url: String },
    /// An album on a third-party platform.
    Album { provider: MusicProvider, url: String },
    /// A playlist — should be redirected to the Playlist Importer.
    Playlist { provider: MusicProvider },
    /// A song.link / album.link / odesli.co URL — resolve via Odesli API.
    SongLink { url: String },
}

/// Detect what kind of music resource a URL points to.
///
/// Returns `None` for URLs that don't match any supported platform.
pub fn detect_music_resource(url: &str) -> Option<MusicResource> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    // 1. Qobuz — resolve_link() handles this natively
    if qbz_qobuz::resolve_link(url).is_ok() {
        return Some(MusicResource::Qobuz);
    }

    // 2. song.link / album.link / odesli.co URLs
    let lower = url.to_ascii_lowercase();
    if lower.contains("song.link/")
        || lower.contains("album.link/")
        || lower.contains("odesli.co/")
    {
        return Some(MusicResource::SongLink { url: url.to_string() });
    }

    // 3. Per-provider detection (track/album/playlist)
    if let Some(resource) = spotify::detect_resource(url) {
        return Some(resource);
    }
    if let Some(resource) = apple::detect_resource(url) {
        return Some(resource);
    }
    if let Some(resource) = tidal::detect_resource(url) {
        return Some(resource);
    }
    if let Some(resource) = deezer::detect_resource(url) {
        return Some(resource);
    }

    None
}

/// User-provided credentials for a provider
#[derive(Debug, Clone, Default)]
pub struct ProviderCredentials {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Spotify {
        playlist_id: String,
    },
    AppleMusic {
        storefront: String,
        playlist_id: String,
    },
    Tidal {
        playlist_id: String,
    },
    Deezer {
        playlist_id: String,
    },
}

pub fn detect_provider(url: &str) -> Result<ProviderKind, PlaylistImportError> {
    if let Some(id) = spotify::parse_playlist_id(url) {
        return Ok(ProviderKind::Spotify { playlist_id: id });
    }
    if let Some((storefront, id)) = apple::parse_playlist_id(url) {
        return Ok(ProviderKind::AppleMusic {
            storefront,
            playlist_id: id,
        });
    }
    if let Some(id) = tidal::parse_playlist_id(url) {
        return Ok(ProviderKind::Tidal { playlist_id: id });
    }
    if let Some(id) = deezer::parse_playlist_id(url) {
        return Ok(ProviderKind::Deezer { playlist_id: id });
    }

    Err(PlaylistImportError::UnsupportedProvider(url.to_string()))
}

/// Fetch playlist (proxy handles credentials)
pub async fn fetch_playlist(kind: ProviderKind) -> Result<ImportPlaylist, PlaylistImportError> {
    match kind {
        ProviderKind::Spotify { playlist_id } => spotify::fetch_playlist(&playlist_id).await,
        ProviderKind::AppleMusic {
            storefront,
            playlist_id,
        } => apple::fetch_playlist(&storefront, &playlist_id).await,
        ProviderKind::Tidal { playlist_id } => tidal::fetch_playlist(&playlist_id).await,
        ProviderKind::Deezer { playlist_id } => deezer::fetch_playlist(&playlist_id).await,
    }
}
