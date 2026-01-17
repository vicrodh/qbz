//! Provider implementations

pub mod apple;
pub mod deezer;
pub mod spotify;
pub mod tidal;

use crate::playlist_import::errors::PlaylistImportError;
use crate::playlist_import::models::ImportPlaylist;

/// User-provided credentials for a provider
#[derive(Debug, Clone, Default)]
pub struct ProviderCredentials {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Spotify { playlist_id: String },
    AppleMusic { storefront: String, playlist_id: String },
    Tidal { playlist_id: String },
    Deezer { playlist_id: String },
}

pub fn detect_provider(url: &str) -> Result<ProviderKind, PlaylistImportError> {
    if let Some(id) = spotify::parse_playlist_id(url) {
        return Ok(ProviderKind::Spotify { playlist_id: id });
    }
    if let Some((storefront, id)) = apple::parse_playlist_id(url) {
        return Ok(ProviderKind::AppleMusic { storefront, playlist_id: id });
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
pub async fn fetch_playlist(
    kind: ProviderKind,
) -> Result<ImportPlaylist, PlaylistImportError> {
    match kind {
        ProviderKind::Spotify { playlist_id } => {
            spotify::fetch_playlist(&playlist_id).await
        }
        ProviderKind::AppleMusic { storefront, playlist_id } => {
            apple::fetch_playlist(&storefront, &playlist_id).await
        }
        ProviderKind::Tidal { playlist_id } => {
            tidal::fetch_playlist(&playlist_id).await
        }
        ProviderKind::Deezer { playlist_id } => deezer::fetch_playlist(&playlist_id).await,
    }
}
