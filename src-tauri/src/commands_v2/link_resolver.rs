use tauri::State;

use qbz_models::QueueTrack as CoreQueueTrack;

use crate::api_cache::ApiCacheState;
use crate::artist_blacklist::BlacklistState;
use crate::artist_vectors::ArtistVectorStoreState;
use crate::config::developer_settings::{DeveloperSettings, DeveloperSettingsState};
use crate::config::graphics_settings::{
    GraphicsSettings, GraphicsSettingsState, GraphicsStartupStatus,
};
use crate::core_bridge::CoreBridgeState;
use crate::integrations_v2::MusicBrainzV2State;
use crate::library::{get_artwork_cache_dir, thumbnails, LibraryState};
use crate::playback_context::{ContentSource, ContextType, PlaybackContext};
use crate::plex::PlexServerInfo;
use crate::runtime::{CommandRequirement, RuntimeError, RuntimeManagerState};
use crate::AppState;
use md5::{Digest, Md5};

use super::V2PlaylistSuggestionsInput;

/// Result of resolving a cross-platform music link.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub enum MusicLinkResult {
    /// Successfully resolved to a Qobuz entity.
    Resolved {
        link: qbz_qobuz::ResolvedLink,
        provider: Option<String>,
    },
    /// The URL is a playlist — redirect to the Playlist Importer.
    PlaylistDetected { provider: String },
    /// The content exists on the source platform but is not available on Qobuz.
    NotOnQobuz { provider: Option<String> },
}

/// Resolve a cross-platform music link to a Qobuz navigation action.
///
/// Accepts URLs from Qobuz, Spotify, Apple Music, Tidal, Deezer, song.link, and album.link.
/// For non-Qobuz tracks/albums, uses the Odesli API to identify the content, then searches
/// Qobuz by title+artist to find the equivalent album.
/// For playlists, returns `PlaylistDetected` so the frontend can redirect to the importer.
#[tauri::command]
pub async fn v2_resolve_music_link(
    url: String,
    state: State<'_, AppState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<MusicLinkResult, RuntimeError> {
    use crate::playlist_import::providers::{detect_music_resource, MusicResource};

    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(RuntimeError::Internal("Empty URL".to_string()));
    }

    // 1. Try Qobuz native resolve first (sync, no network)
    if let Ok(resolved) = qbz_qobuz::resolve_link(&url) {
        return Ok(MusicLinkResult::Resolved {
            link: resolved,
            provider: None,
        });
    }

    // 2. Detect what kind of resource this is
    let resource = detect_music_resource(&url)
        .ok_or_else(|| RuntimeError::Internal("Unsupported or invalid music link".to_string()))?;

    match resource {
        MusicResource::Qobuz => {
            // Already handled above, but just in case
            let resolved =
                qbz_qobuz::resolve_link(&url).map_err(|e| RuntimeError::Internal(e.to_string()))?;
            Ok(MusicLinkResult::Resolved {
                link: resolved,
                provider: None,
            })
        }

        MusicResource::Playlist { provider } => Ok(MusicLinkResult::PlaylistDetected {
            provider: format!("{:?}", provider),
        }),

        MusicResource::Track {
            provider,
            url: source_url,
        } => {
            resolve_via_odesli_and_search(
                &state.songlink,
                &source_url,
                Some(&provider),
                true,
                &bridge,
                &runtime,
            )
            .await
        }

        MusicResource::Album {
            provider,
            url: source_url,
        } => {
            resolve_via_odesli_and_search(
                &state.songlink,
                &source_url,
                Some(&provider),
                false,
                &bridge,
                &runtime,
            )
            .await
        }

        MusicResource::SongLink { url: source_url } => {
            // song.link URLs: try to detect track vs album from the URL format
            let is_track_hint = source_url.contains("song.link/");
            resolve_via_odesli_and_search(
                &state.songlink,
                &source_url,
                None,
                is_track_hint,
                &bridge,
                &runtime,
            )
            .await
        }
    }
}

/// Identify a cross-platform music URL and search Qobuz for the equivalent.
///
/// Fast path: for Tidal/Deezer calls the platform API directly; for Spotify
/// scrapes the embed page to get title+artist. Fallback: uses Odesli API (~2-3s).
/// Then searches Qobuz with progressively simpler queries.
async fn resolve_via_odesli_and_search(
    songlink: &crate::share::SongLinkClient,
    url: &str,
    provider: Option<&crate::playlist_import::providers::MusicProvider>,
    is_track: bool,
    bridge: &State<'_, CoreBridgeState>,
    runtime: &State<'_, RuntimeManagerState>,
) -> Result<MusicLinkResult, RuntimeError> {
    let provider_name = provider.map(|p| format!("{:?}", p));

    // 1. Get title + artist: try direct platform API first (fast), fall back to Odesli
    let (title, artist) = if let Some(prov) = provider {
        match try_direct_platform_metadata(url, prov, is_track).await {
            Some(meta) => {
                log::info!(
                    "Link resolver: direct API resolved '{}' by '{}'",
                    meta.0,
                    meta.1
                );
                meta
            }
            None => {
                log::info!("Link resolver: direct API failed, falling back to Odesli");
                fetch_metadata_via_odesli(songlink, url).await?
            }
        }
    } else {
        // No provider (song.link URLs) — use Odesli
        fetch_metadata_via_odesli(songlink, url).await?
    };

    if title.is_empty() {
        return Ok(MusicLinkResult::NotOnQobuz {
            provider: provider_name,
        });
    }

    // 2. Search Qobuz with progressively simpler queries
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    let bridge_guard = bridge.get().await;

    if let Some(result) =
        search_qobuz_smart(&*bridge_guard, &title, &artist, is_track, &provider_name).await?
    {
        return Ok(result);
    }

    log::info!(
        "Link resolver: '{}' by '{}' not found on Qobuz",
        title,
        artist
    );
    Ok(MusicLinkResult::NotOnQobuz {
        provider: provider_name,
    })
}

/// Fetch metadata from Odesli API (with one retry for transient errors).
async fn fetch_metadata_via_odesli(
    songlink: &crate::share::SongLinkClient,
    url: &str,
) -> Result<(String, String), RuntimeError> {
    let response = match songlink
        .get_by_url(url, crate::share::ContentType::Track)
        .await
    {
        Ok(r) => r,
        Err(first_err) => {
            log::warn!(
                "Link resolver: Odesli first attempt failed: {}, retrying...",
                first_err
            );
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            songlink
                .get_by_url(url, crate::share::ContentType::Track)
                .await
                .map_err(|e| RuntimeError::Internal(format!("Odesli API error: {}", e)))?
        }
    };

    let title = response.title.unwrap_or_default().trim().to_string();
    let artist = response.artist.unwrap_or_default().trim().to_string();
    Ok((title, artist))
}

/// Search Qobuz with progressively simpler queries until a match is found.
///
/// Strategy:
/// 1. "title artist" (exact)
/// 2. "cleaned_title artist" (remove parenthetical/bracket suffixes)
/// 3. "artist" only with album search (broad)
async fn search_qobuz_smart(
    bridge: &crate::core_bridge::CoreBridge,
    title: &str,
    artist: &str,
    is_track: bool,
    provider_name: &Option<String>,
) -> Result<Option<MusicLinkResult>, RuntimeError> {
    let full_query = if artist.is_empty() {
        title.to_string()
    } else {
        format!("{} {}", title, artist)
    };

    // Attempt 1: full query
    if is_track {
        let results = bridge
            .search_tracks(&full_query, 5, 0, None)
            .await
            .map_err(RuntimeError::Internal)?;
        if let Some(track) = results.items.first() {
            log::info!(
                "Link resolver: found Qobuz track id={} (full query)",
                track.id
            );
            return Ok(Some(MusicLinkResult::Resolved {
                link: qbz_qobuz::ResolvedLink::OpenTrack(track.id),
                provider: provider_name.clone(),
            }));
        }
    }

    let results = bridge
        .search_albums(&full_query, 5, 0, None)
        .await
        .map_err(RuntimeError::Internal)?;
    if let Some(album) = results.items.first() {
        log::info!(
            "Link resolver: found Qobuz album id={} (full query)",
            album.id
        );
        return Ok(Some(MusicLinkResult::Resolved {
            link: qbz_qobuz::ResolvedLink::OpenAlbum(album.id.clone()),
            provider: provider_name.clone(),
        }));
    }

    // Attempt 2: clean title (remove parenthetical/bracket suffixes like "Remastered", "Deluxe")
    let cleaned = clean_title(title);
    if cleaned != title && !cleaned.is_empty() {
        let clean_query = if artist.is_empty() {
            cleaned.clone()
        } else {
            format!("{} {}", cleaned, artist)
        };

        log::info!(
            "Link resolver: retrying with cleaned query '{}'",
            clean_query
        );
        let results = bridge
            .search_albums(&clean_query, 5, 0, None)
            .await
            .map_err(RuntimeError::Internal)?;
        if let Some(album) = results.items.first() {
            log::info!(
                "Link resolver: found Qobuz album id={} (cleaned query)",
                album.id
            );
            return Ok(Some(MusicLinkResult::Resolved {
                link: qbz_qobuz::ResolvedLink::OpenAlbum(album.id.clone()),
                provider: provider_name.clone(),
            }));
        }
    }

    // Attempt 3: search by artist name only (broad)
    if !artist.is_empty() && artist != title {
        log::info!(
            "Link resolver: retrying with artist-only query '{}'",
            artist
        );
        let results = bridge
            .search_albums(artist, 10, 0, None)
            .await
            .map_err(RuntimeError::Internal)?;
        let title_lower = title.to_ascii_lowercase();
        let cleaned_lower = clean_title(title).to_ascii_lowercase();
        for album in &results.items {
            let album_title_lower = album.title.to_ascii_lowercase();
            if album_title_lower.contains(&cleaned_lower)
                || cleaned_lower.contains(&album_title_lower)
                || album_title_lower.contains(&title_lower)
            {
                log::info!(
                    "Link resolver: found Qobuz album id={} (artist-only + title match)",
                    album.id
                );
                return Ok(Some(MusicLinkResult::Resolved {
                    link: qbz_qobuz::ResolvedLink::OpenAlbum(album.id.clone()),
                    provider: provider_name.clone(),
                }));
            }
        }
    }

    Ok(None)
}

/// Remove parenthetical/bracket suffixes from a title.
/// "Senjutsu (2021 Remaster)" → "Senjutsu"
/// "The Number of the Beast [Deluxe Edition]" → "The Number of the Beast"
fn clean_title(title: &str) -> String {
    let mut result = title.to_string();
    // Remove trailing (...) and [...]
    while let Some(pos) = result.rfind('(') {
        if result[pos..].contains(')') {
            result = result[..pos].trim_end().to_string();
        } else {
            break;
        }
    }
    while let Some(pos) = result.rfind('[') {
        if result[pos..].contains(']') {
            result = result[..pos].trim_end().to_string();
        } else {
            break;
        }
    }
    result.trim().to_string()
}

// ── Direct platform metadata (bypass Odesli for speed) ──

const QBZ_PROXY_BASE: &str = "https://qbz-api-proxy.blitzkriegfc.workers.dev";

/// Try to get title+artist directly from the platform API.
/// Returns None if the platform isn't supported or the request fails.
async fn try_direct_platform_metadata(
    url: &str,
    provider: &crate::playlist_import::providers::MusicProvider,
    is_track: bool,
) -> Option<(String, String)> {
    use crate::playlist_import::providers::MusicProvider;

    match provider {
        MusicProvider::Deezer => try_deezer_metadata(url, is_track).await,
        MusicProvider::Spotify => try_spotify_metadata(url, is_track).await,
        MusicProvider::Tidal => try_tidal_metadata(url, is_track).await,
        MusicProvider::AppleMusic => None, // No direct API available
    }
}

/// Extract a numeric or alphanumeric ID after /track/ or /album/ in a URL.
fn extract_entity_id(url: &str, entity_type: &str) -> Option<String> {
    let pattern = format!("/{}/", entity_type);
    let idx = url.find(&pattern)?;
    let rest = &url[idx + pattern.len()..];
    let id = rest.split(['?', '/', '#']).next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Extract Spotify ID from URL or URI.
fn extract_spotify_entity_id(url: &str, entity_type: &str) -> Option<String> {
    // URI format: spotify:track:abc123
    let uri_pattern = format!("spotify:{}:", entity_type);
    if let Some(rest) = url.strip_prefix(&uri_pattern) {
        let id = rest.split(['?', '/']).next()?;
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    extract_entity_id(url, entity_type)
}

async fn try_deezer_metadata(url: &str, is_track: bool) -> Option<(String, String)> {
    let entity = if is_track { "track" } else { "album" };
    let id = extract_entity_id(url, entity).or_else(|| {
        if is_track {
            None
        } else {
            extract_entity_id(url, "track")
        }
    })?;
    let api_url = format!("https://api.deezer.com/{}/{}", entity, id);

    log::debug!("Link resolver: Deezer direct API: {}", api_url);
    let data: serde_json::Value = reqwest::get(&api_url).await.ok()?.json().await.ok()?;
    if data.get("error").is_some() {
        return None;
    }

    let title = data.get("title")?.as_str()?.to_string();
    let artist = data
        .get("artist")
        .and_then(|a| a.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some((title, artist))
}

async fn try_spotify_metadata(url: &str, is_track: bool) -> Option<(String, String)> {
    let entity = if is_track { "track" } else { "album" };
    let id = extract_spotify_entity_id(url, entity)?;

    log::debug!("Link resolver: Spotify embed scrape for {} {}", entity, id);
    crate::playlist_import::providers::spotify::fetch_embed_metadata(entity, &id).await
}

async fn try_tidal_metadata(url: &str, is_track: bool) -> Option<(String, String)> {
    let entity = if is_track { "track" } else { "album" };
    let id = extract_entity_id(url, entity)
        // Also try /browse/track/ pattern
        .or_else(|| extract_entity_id(url, &format!("browse/{}", entity)))?;
    let token = get_proxy_token("tidal").await?;
    let api_url = format!(
        "https://openapi.tidal.com/v2/{}s/{}?countryCode=US&include=artists",
        entity, id
    );

    log::debug!("Link resolver: Tidal direct API: {}", api_url);
    let data: serde_json::Value = reqwest::Client::new()
        .get(&api_url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    let title = data
        .get("data")
        .and_then(|d| d.get("attributes"))
        .and_then(|a| a.get("title"))
        .and_then(|v| v.as_str())?
        .to_string();

    // Artist name is in the "included" array
    let artist = data
        .get("included")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|item| item.get("type").and_then(|v| v.as_str()) == Some("artists"))
        })
        .and_then(|item| item.get("attributes"))
        .and_then(|a| a.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some((title, artist))
}

/// Get an OAuth token from the QBZ proxy for the given platform.
async fn get_proxy_token(platform: &str) -> Option<String> {
    let url = format!("{}/{}/token", QBZ_PROXY_BASE, platform);
    let data: serde_json::Value = reqwest::Client::builder()
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::USER_AGENT,
                reqwest::header::HeaderValue::from_static("QBZ/1.0.0"),
            );
            h
        })
        .build()
        .ok()?
        .get(&url)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    data.get("access_token")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
}

#[tauri::command]
pub fn v2_resolve_qobuz_link(url: String) -> Result<qbz_qobuz::ResolvedLink, RuntimeError> {
    qbz_qobuz::resolve_link(&url).map_err(|e| RuntimeError::Internal(e.to_string()))
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_get_qobuz_track_url(trackId: u64) -> Result<String, RuntimeError> {
    Ok(format!("https://play.qobuz.com/track/{}", trackId))
}

/// Known .desktop filenames across packaging formats.
#[cfg(target_os = "linux")]
const QBZ_DESKTOP_CANDIDATES: &[&str] = &[
    "com.blitzfc.qbz.desktop", // Tauri deb, Flatpak
    "qbz.desktop",             // Arch, AUR, Snap
    "qbz-nix.desktop",         // Possible alternative
];

/// Search standard directories for the installed QBZ .desktop file.
#[cfg(target_os = "linux")]
fn find_qbz_desktop_file() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let search_dirs = [
        "/usr/share/applications".to_string(),
        "/usr/local/share/applications".to_string(),
        format!("{}/.local/share/applications", home),
        "/var/lib/flatpak/exports/share/applications".to_string(),
        format!("{}/.local/share/flatpak/exports/share/applications", home),
    ];

    for candidate in QBZ_DESKTOP_CANDIDATES {
        for dir in &search_dirs {
            let path = format!("{}/{}", dir, candidate);
            if std::path::Path::new(&path).exists() {
                log::info!("[URI Handler] Found desktop file: {}", path);
                return candidate.to_string();
            }
        }
    }

    log::warn!("[URI Handler] No desktop file found in standard dirs, using default");
    "com.blitzfc.qbz.desktop".to_string()
}

/// Refresh the desktop MIME database so xdg-open picks up changes.
#[cfg(target_os = "linux")]
fn refresh_desktop_database() {
    // User-level applications dir
    if let Some(data_dir) = dirs::data_dir() {
        let user_apps = data_dir.join("applications");
        if user_apps.exists() {
            let _ = std::process::Command::new("update-desktop-database")
                .arg(&user_apps)
                .status();
        }
    }
    // System-level (may fail without root, that's OK)
    let _ = std::process::Command::new("update-desktop-database")
        .arg("/usr/share/applications")
        .status();
}

/// Check if QBZ is the default handler for qobuzapp:// links.
#[tauri::command]
pub fn v2_check_qobuzapp_handler() -> Result<bool, RuntimeError> {
    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("xdg-mime")
            .args(["query", "default", "x-scheme-handler/qobuzapp"])
            .output()
            .map_err(|e| RuntimeError::Internal(format!("Failed to run xdg-mime: {}", e)))?;

        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(QBZ_DESKTOP_CANDIDATES.iter().any(|c| *c == result))
    }
    #[cfg(not(target_os = "linux"))]
    {
        // URI handler registration is Linux-only (xdg-mime)
        Ok(false)
    }
}

/// Register QBZ as the default handler for qobuzapp:// links.
#[tauri::command]
pub fn v2_register_qobuzapp_handler() -> Result<bool, RuntimeError> {
    #[cfg(target_os = "linux")]
    {
        let desktop_file = find_qbz_desktop_file();
        log::info!(
            "[URI Handler] Registering {} for x-scheme-handler/qobuzapp",
            desktop_file
        );

        let status = std::process::Command::new("xdg-mime")
            .args(["default", &desktop_file, "x-scheme-handler/qobuzapp"])
            .status()
            .map_err(|e| RuntimeError::Internal(format!("Failed to run xdg-mime: {}", e)))?;

        if !status.success() {
            log::error!("[URI Handler] xdg-mime default failed");
            return Ok(false);
        }

        refresh_desktop_database();
        log::info!("[URI Handler] Registration complete, desktop database refreshed");
        Ok(true)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(false)
    }
}

/// Remove QBZ as the default handler for qobuzapp:// links.
#[tauri::command]
pub fn v2_deregister_qobuzapp_handler() -> Result<bool, RuntimeError> {
    #[cfg(target_os = "linux")]
    {
        let mimeapps = dirs::config_dir()
            .ok_or_else(|| RuntimeError::Internal("No config dir found".to_string()))?
            .join("mimeapps.list");

        if !mimeapps.exists() {
            return Ok(true); // Nothing to remove
        }

        let content = std::fs::read_to_string(&mimeapps)
            .map_err(|e| RuntimeError::Internal(format!("Failed to read mimeapps.list: {}", e)))?;

        let filtered: String = content
            .lines()
            .filter(|line| !line.starts_with("x-scheme-handler/qobuzapp="))
            .collect::<Vec<_>>()
            .join("\n");

        // Preserve trailing newline if original had one
        let filtered = if content.ends_with('\n') && !filtered.ends_with('\n') {
            format!("{}\n", filtered)
        } else {
            filtered
        };

        std::fs::write(&mimeapps, filtered)
            .map_err(|e| RuntimeError::Internal(format!("Failed to write mimeapps.list: {}", e)))?;

        refresh_desktop_database();
        Ok(true)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(true)
    }
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_ping(baseUrl: String, token: String) -> Result<PlexServerInfo, String> {
    crate::plex::plex_ping(baseUrl, token).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_get_track_metadata(
    baseUrl: String,
    token: String,
    ratingKey: String,
) -> Result<crate::plex::PlexTrack, String> {
    crate::plex::plex_get_track_metadata(baseUrl, token, ratingKey).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_auth_pin_start(
    clientIdentifier: String,
) -> Result<crate::plex::PlexPinStartResult, String> {
    crate::plex::plex_auth_pin_start(clientIdentifier).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_auth_pin_check(
    clientIdentifier: String,
    pinId: u64,
    code: Option<String>,
) -> Result<crate::plex::PlexPinCheckResult, String> {
    crate::plex::plex_auth_pin_check(clientIdentifier, pinId, code).await
}

#[tauri::command]
pub fn v2_set_visualizer_enabled(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.visualizer.set_enabled(enabled);
    Ok(())
}

#[tauri::command]
pub fn v2_get_developer_settings(
    state: State<'_, DeveloperSettingsState>,
) -> Result<DeveloperSettings, String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Developer settings store not initialized")?;
    store.get_settings()
}

#[tauri::command]
pub fn v2_set_developer_force_dmabuf(
    enabled: bool,
    state: State<'_, DeveloperSettingsState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Developer settings store not initialized")?;
    store.set_force_dmabuf(enabled)
}

#[tauri::command]
pub fn v2_get_graphics_settings(
    state: State<'_, GraphicsSettingsState>,
) -> Result<GraphicsSettings, String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.get_settings()
}

#[tauri::command]
pub fn v2_get_graphics_startup_status() -> GraphicsStartupStatus {
    crate::config::graphics_settings::get_graphics_startup_status()
}

#[tauri::command]
pub fn v2_set_hardware_acceleration(
    enabled: bool,
    state: State<'_, GraphicsSettingsState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.set_hardware_acceleration(enabled)
}

#[tauri::command]
pub fn v2_set_gdk_scale(
    value: Option<String>,
    state: State<'_, GraphicsSettingsState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.set_gdk_scale(value)
}

#[tauri::command]
pub fn v2_set_gdk_dpi_scale(
    value: Option<String>,
    state: State<'_, GraphicsSettingsState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.set_gdk_dpi_scale(value)
}

#[tauri::command]
pub fn v2_set_gsk_renderer(
    value: Option<String>,
    state: State<'_, GraphicsSettingsState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard
        .as_ref()
        .ok_or("Graphics settings store not initialized")?;
    store.set_gsk_renderer(value)
}

#[tauri::command]
pub fn v2_clear_cache(state: State<'_, AppState>) -> Result<(), String> {
    state.audio_cache.clear();
    Ok(())
}

#[tauri::command]
pub async fn v2_clear_artist_cache(cache_state: State<'_, ApiCacheState>) -> Result<usize, String> {
    let guard = cache_state.cache.lock().await;
    let cache = guard.as_ref().ok_or("No active session - please log in")?;
    cache.clear_all_artists()
}

#[tauri::command]
pub async fn v2_get_vector_store_stats(
    store_state: State<'_, ArtistVectorStoreState>,
) -> Result<crate::artist_vectors::StoreStats, String> {
    let guard = store_state.store.lock().await;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.get_stats()
}

#[tauri::command]
pub async fn v2_clear_vector_store(
    store_state: State<'_, ArtistVectorStoreState>,
) -> Result<usize, String> {
    let mut guard = store_state.store.lock().await;
    let store = guard.as_mut().ok_or("No active session - please log in")?;
    store.clear_all()
}

#[tauri::command]
pub async fn v2_get_playlist_suggestions(
    input: V2PlaylistSuggestionsInput,
    store_state: State<'_, ArtistVectorStoreState>,
    musicbrainz: State<'_, MusicBrainzV2State>,
    app_state: State<'_, AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<crate::artist_vectors::SuggestionResult, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await?;

    if input.artists.is_empty() {
        return Ok(crate::artist_vectors::SuggestionResult {
            tracks: Vec::new(),
            source_artists: Vec::new(),
            playlist_artists_count: 0,
            similar_artists_count: 0,
        });
    }

    let mut resolved_artists: Vec<(String, String)> = Vec::new();
    let mut seen_mbids = std::collections::HashSet::new();

    for artist in &input.artists {
        let mbid_from_qobuz = if let Some(qobuz_id) = artist.qobuz_id {
            let qobuz_artist_name = {
                let client = app_state.client.read().await;
                match client.get_artist(qobuz_id, false).await {
                    Ok(qobuz_artist) => Some(qobuz_artist.name),
                    Err(err) => {
                        log::warn!(
                            "[V2/Suggestions] Failed to fetch Qobuz artist {} for MBID resolution: {}",
                            qobuz_id,
                            err
                        );
                        None
                    }
                }
            };

            if let Some(artist_name) = qobuz_artist_name {
                let client = musicbrainz.client.lock().await;
                match client.search_artist(&artist_name, 10).await {
                    Ok(search) => search
                        .artists
                        .into_iter()
                        .find(|candidate| candidate.score.unwrap_or(0) >= 80)
                        .map(|candidate| candidate.id),
                    Err(err) => {
                        log::warn!(
                            "[V2/Suggestions] Failed Qobuz->MBID search for {} ({}): {}",
                            artist.name,
                            qobuz_id,
                            err
                        );
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        let resolved_mbid = if mbid_from_qobuz.is_some() {
            mbid_from_qobuz
        } else {
            let client = musicbrainz.client.lock().await;
            match client.search_artist(&artist.name, 10).await {
                Ok(search) => search
                    .artists
                    .into_iter()
                    .find(|candidate| candidate.score.unwrap_or(0) >= 80)
                    .map(|candidate| candidate.id),
                Err(err) => {
                    log::warn!(
                        "[V2/Suggestions] Failed name->MBID resolution for {}: {}",
                        artist.name,
                        err
                    );
                    None
                }
            }
        };

        if let Some(mbid) = resolved_mbid {
            if seen_mbids.insert(mbid.clone()) {
                resolved_artists.push((mbid, artist.name.clone()));
            }
        }
    }

    if resolved_artists.is_empty() {
        log::warn!("[V2/Suggestions] No artists could be resolved to MusicBrainz IDs");
        return Ok(crate::artist_vectors::SuggestionResult {
            tracks: Vec::new(),
            source_artists: Vec::new(),
            playlist_artists_count: input.artists.len(),
            similar_artists_count: 0,
        });
    }

    let config = input.config.unwrap_or_default();
    let builder = std::sync::Arc::new(crate::artist_vectors::ArtistVectorBuilder::new(
        store_state.store.clone(),
        musicbrainz.client.clone(),
        musicbrainz.cache.clone(),
        app_state.client.clone(),
        crate::artist_vectors::RelationshipWeights::default(),
    ));

    let engine = crate::artist_vectors::SuggestionsEngine::new(
        store_state.store.clone(),
        builder,
        app_state.client.clone(),
        config,
    );

    let exclude_track_ids: std::collections::HashSet<u64> =
        input.exclude_track_ids.into_iter().collect();

    engine
        .generate_suggestions(&resolved_artists, &exclude_track_ids, input.include_reasons)
        .await
        .map_err(RuntimeError::Internal)
}

#[tauri::command]
pub fn v2_add_to_artist_blacklist(
    artist_id: u64,
    artist_name: String,
    notes: Option<String>,
    state: State<'_, BlacklistState>,
) -> Result<(), String> {
    state.add(artist_id, &artist_name, notes.as_deref())
}

#[tauri::command]
pub fn v2_remove_from_artist_blacklist(
    artist_id: u64,
    state: State<'_, BlacklistState>,
) -> Result<(), String> {
    state.remove(artist_id)
}

#[tauri::command]
pub fn v2_set_blacklist_enabled(
    enabled: bool,
    state: State<'_, BlacklistState>,
) -> Result<(), String> {
    state.set_enabled(enabled)
}

#[tauri::command]
pub fn v2_clear_artist_blacklist(state: State<'_, BlacklistState>) -> Result<(), String> {
    state.clear_all()
}

#[tauri::command]
pub fn v2_get_artist_blacklist(
    state: State<'_, BlacklistState>,
) -> Result<Vec<crate::artist_blacklist::BlacklistedArtist>, String> {
    state.get_all()
}

#[tauri::command]
pub fn v2_get_blacklist_settings(
    state: State<'_, BlacklistState>,
) -> Result<crate::artist_blacklist::BlacklistSettings, String> {
    state.get_settings()
}

#[tauri::command]
pub fn v2_save_credentials(email: String, password: String) -> Result<(), String> {
    crate::credentials::save_qobuz_credentials(&email, &password)
}

#[tauri::command]
pub fn v2_clear_saved_credentials() -> Result<(), String> {
    crate::credentials::clear_qobuz_credentials()?;
    crate::credentials::clear_oauth_token()
}

#[tauri::command]
pub async fn v2_plex_open_auth_url(url: String) -> Result<(), String> {
    crate::plex::plex_open_auth_url(url).await
}

#[tauri::command]
pub fn v2_plex_cache_save_sections(
    server_id: Option<String>,
    sections: Vec<crate::plex::PlexMusicSection>,
) -> Result<usize, String> {
    crate::plex::plex_cache_save_sections(server_id, sections)
}

#[tauri::command]
pub fn v2_plex_cache_get_sections() -> Result<Vec<crate::plex::PlexMusicSection>, String> {
    crate::plex::plex_cache_get_sections()
}

#[tauri::command]
pub fn v2_plex_cache_save_tracks(
    server_id: Option<String>,
    section_key: String,
    tracks: Vec<crate::plex::PlexTrack>,
) -> Result<usize, String> {
    crate::plex::plex_cache_save_tracks(server_id, section_key, tracks)
}

#[tauri::command]
pub fn v2_plex_cache_clear() -> Result<(), String> {
    crate::plex::plex_cache_clear()
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_plex_cache_get_tracks(
    sectionKey: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<crate::plex::PlexTrack>, String> {
    crate::plex::plex_cache_get_tracks(sectionKey, limit)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_plex_cache_get_tracks_by_keys(
    ratingKeys: Vec<String>,
) -> Result<Vec<crate::plex::PlexTrack>, String> {
    crate::plex::plex_cache_get_tracks_by_keys(&ratingKeys)
}

#[tauri::command]
pub fn v2_plex_cache_get_albums() -> Result<Vec<crate::plex::PlexCachedAlbum>, String> {
    crate::plex::plex_cache_get_albums()
}

#[tauri::command]
pub fn v2_plex_cache_search_tracks(
    query: String,
    limit: Option<u32>,
) -> Result<Vec<crate::plex::PlexCachedTrack>, String> {
    crate::plex::plex_cache_search_tracks(query, limit)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn v2_plex_cache_get_album_tracks(
    albumKey: String,
) -> Result<Vec<crate::plex::PlexCachedTrack>, String> {
    crate::plex::plex_cache_get_album_tracks(albumKey)
}

#[tauri::command]
pub fn v2_plex_cache_update_track_quality(
    updates: Vec<crate::plex::PlexTrackQualityUpdate>,
) -> Result<usize, String> {
    crate::plex::plex_cache_update_track_quality(updates)
}

#[tauri::command]
pub fn v2_plex_cache_get_tracks_needing_hydration(
    limit: Option<u32>,
) -> Result<Vec<String>, String> {
    crate::plex::plex_cache_get_tracks_needing_hydration(limit)
}

// === Custom Album Covers ===

#[derive(Debug, Clone, serde::Serialize)]
pub struct CustomAlbumCoverResult {
    pub image_path: String,
    pub thumbnail_path: String,
}

#[tauri::command]
pub async fn v2_library_set_custom_album_cover(
    album_id: String,
    custom_image_path: String,
    state: State<'_, LibraryState>,
) -> Result<CustomAlbumCoverResult, String> {
    let artwork_dir = get_artwork_cache_dir();
    let source = std::path::Path::new(&custom_image_path);
    if !source.exists() {
        return Err(format!(
            "Source image does not exist: {}",
            custom_image_path
        ));
    }

    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if !["png", "jpg", "jpeg", "webp"].contains(&extension.as_str()) {
        return Err(format!(
            "Unsupported image format: {}. Use png, jpg, jpeg, or webp.",
            extension
        ));
    }

    let mut hasher = Md5::new();
    hasher.update(album_id.as_bytes());
    let album_hash = format!("{:x}", hasher.finalize());
    let timestamp = chrono::Utc::now().timestamp();
    let filename = format!("album_custom_{}_{}.jpg", album_hash, timestamp);
    let dest_path = artwork_dir.join(&filename);

    let img = image::ImageReader::open(source)
        .map_err(|e| format!("Failed to open image: {}", e))?
        .decode()
        .map_err(|e| format!("Failed to decode image: {}", e))?;
    let resized = img.resize(1000, 1000, image::imageops::FilterType::Lanczos3);
    resized
        .save(&dest_path)
        .map_err(|e| format!("Failed to save resized image: {}", e))?;

    let thumbnail_path = thumbnails::generate_thumbnail(&dest_path)
        .map_err(|e| format!("Failed to generate thumbnail: {}", e))?;

    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.set_custom_album_cover(&album_id, &dest_path.to_string_lossy())
        .map_err(|e| e.to_string())?;

    Ok(CustomAlbumCoverResult {
        image_path: dest_path.to_string_lossy().into_owned(),
        thumbnail_path: thumbnail_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn v2_library_remove_custom_album_cover(
    album_id: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;

    let existing = db
        .get_custom_album_cover(&album_id)
        .map_err(|e| e.to_string())?;
    if let Some(path) = existing {
        let p = std::path::Path::new(&path);
        if p.exists() {
            if let Ok(thumb) = thumbnails::get_thumbnail_path(p) {
                let _ = std::fs::remove_file(thumb);
            }
            let _ = std::fs::remove_file(p);
        }
    }

    db.remove_custom_album_cover(&album_id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn v2_library_get_all_custom_album_covers(
    state: State<'_, LibraryState>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_all_custom_album_covers().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_save_image_url_to_file(url: String, dest_path: String) -> Result<(), String> {
    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to download image: {}", e))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read image data: {}", e))?;
    std::fs::write(&dest_path, &bytes).map_err(|e| format!("Failed to save image: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn v2_create_artist_radio(
    artist_id: u64,
    artist_name: String,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<String, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

    let client = state.client.read().await.clone();

    let session_id = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
        let builder = crate::radio_engine::RadioPoolBuilder::new(
            &radio_db,
            &client,
            crate::radio_engine::BuildRadioOptions::default(),
        );
        let rt = tokio::runtime::Handle::current();
        let session = rt.block_on(builder.create_artist_radio(artist_id))?;
        Ok(session.id)
    })
    .await
    .map_err(|e| format!("Radio task failed: {}", e))??;

    let client = state.client.read().await;
    let track_ids = tokio::task::spawn_blocking({
        let session_id = session_id.clone();
        move || -> Result<Vec<u64>, String> {
            let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
            let radio_engine = crate::radio_engine::RadioEngine::new(radio_db);
            let mut ids = Vec::new();
            for _ in 0..60 {
                match radio_engine.next_track(&session_id) {
                    Ok(radio_track) => ids.push(radio_track.track_id),
                    Err(_) => break,
                }
            }
            Ok(ids.into_iter().take(50).collect())
        }
    })
    .await
    .map_err(|e| format!("Track generation task failed: {}", e))??;

    let mut tracks = Vec::new();
    for next_track_id in track_ids {
        if let Ok(track) = client.get_track(next_track_id).await {
            if let Some(ref performer) = track.performer {
                if blacklist_state.is_blacklisted(performer.id) {
                    continue;
                }
            }
            tracks.push(track);
        }
    }

    if tracks.is_empty() {
        return Err("Failed to generate any radio tracks".to_string());
    }

    let queue_tracks: Vec<CoreQueueTrack> =
        tracks.iter().map(track_to_queue_track_from_api).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(0)).await;
    bridge.play_index(0).await;

    let queue_track_ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
    let context = PlaybackContext::new(
        ContextType::Radio,
        session_id.clone(),
        artist_name,
        ContentSource::Qobuz,
        queue_track_ids,
        0,
    );
    state.context.set_context(context);

    Ok(session_id)
}

#[tauri::command]
pub async fn v2_create_track_radio(
    track_id: u64,
    track_name: String,
    artist_id: u64,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<String, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

    let client = state.client.read().await.clone();

    let session_id = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
        let builder = crate::radio_engine::RadioPoolBuilder::new(
            &radio_db,
            &client,
            crate::radio_engine::BuildRadioOptions::default(),
        );
        let rt = tokio::runtime::Handle::current();
        let session = rt.block_on(builder.create_track_radio(track_id, artist_id))?;
        Ok(session.id)
    })
    .await
    .map_err(|e| format!("Radio task failed: {}", e))??;

    let client = state.client.read().await;
    let track_ids = tokio::task::spawn_blocking({
        let session_id = session_id.clone();
        let seed_track_id = track_id;
        move || -> Result<Vec<u64>, String> {
            let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
            let radio_engine = crate::radio_engine::RadioEngine::new(radio_db);
            let mut tracks_with_source = Vec::new();
            for _ in 0..60 {
                match radio_engine.next_track(&session_id) {
                    Ok(radio_track) => {
                        tracks_with_source.push((radio_track.track_id, radio_track.source.clone()));
                    }
                    Err(_) => break,
                }
            }
            if let Some(seed_idx) = tracks_with_source
                .iter()
                .position(|(id, source)| *id == seed_track_id && source == "seed_track")
            {
                if seed_idx != 0 {
                    tracks_with_source.swap(0, seed_idx);
                }
            }
            Ok(tracks_with_source
                .into_iter()
                .take(50)
                .map(|(id, _)| id)
                .collect())
        }
    })
    .await
    .map_err(|e| format!("Track generation task failed: {}", e))??;

    let mut tracks = Vec::new();
    for next_track_id in track_ids {
        if let Ok(track) = client.get_track(next_track_id).await {
            if let Some(ref performer) = track.performer {
                if blacklist_state.is_blacklisted(performer.id) {
                    continue;
                }
            }
            tracks.push(track);
        }
    }

    if tracks.is_empty() {
        return Err("Failed to generate any radio tracks".to_string());
    }

    let queue_tracks: Vec<CoreQueueTrack> =
        tracks.iter().map(track_to_queue_track_from_api).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(0)).await;
    bridge.play_index(0).await;

    let queue_track_ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
    let context = PlaybackContext::new(
        ContextType::Radio,
        session_id.clone(),
        track_name,
        ContentSource::Qobuz,
        queue_track_ids,
        0,
    );
    state.context.set_context(context);

    Ok(session_id)
}

/// Create album radio using the Qobuz `/radio/album` API endpoint.
///
/// Unlike artist/track radio (which uses RadioPoolBuilder + RadioDb),
/// album radio calls the Qobuz API directly — the endpoint returns
/// recommended tracks in a single GET response.
#[tauri::command]
pub async fn v2_create_album_radio(
    album_id: String,
    album_name: String,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<String, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

    let client = state.client.read().await.clone();
    let radio_response = client
        .get_radio_album(&album_id)
        .await
        .map_err(|e| format!("Failed to fetch album radio: {}", e))?;

    // The radio endpoint returns partial track objects (missing performer, etc.).
    // Extract IDs and fetch full track data individually, same as artist/track radio.
    let track_ids: Vec<u64> = radio_response.tracks.items.iter().map(|t| t.id).collect();

    if track_ids.is_empty() {
        return Err("No radio tracks returned for this album".to_string());
    }

    let mut tracks = Vec::new();
    for track_id in track_ids {
        if let Ok(track) = client.get_track(track_id).await {
            if let Some(ref performer) = track.performer {
                if blacklist_state.is_blacklisted(performer.id) {
                    continue;
                }
            }
            tracks.push(track);
        }
    }

    if tracks.is_empty() {
        return Err("Failed to fetch any radio tracks".to_string());
    }

    let queue_tracks: Vec<CoreQueueTrack> =
        tracks.iter().map(track_to_queue_track_from_api).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(0)).await;
    bridge.play_index(0).await;

    let queue_track_ids: Vec<u64> = tracks.iter().map(|track| track.id).collect();
    let context_id = format!(
        "album_radio_{}_{}",
        album_id,
        chrono::Utc::now().timestamp()
    );
    let context = PlaybackContext::new(
        ContextType::Radio,
        context_id.clone(),
        album_name,
        ContentSource::Qobuz,
        queue_track_ids,
        0,
    );
    state.context.set_context(context);

    Ok(context_id)
}

/// Create artist radio using the Qobuz `/radio/artist` API endpoint.
///
/// Like album radio, this calls the Qobuz API directly — the endpoint returns
/// recommended tracks in a single GET response.
#[tauri::command]
pub async fn v2_create_qobuz_artist_radio(
    artist_id: u64,
    artist_name: String,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<String, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

    let client = state.client.read().await.clone();
    let radio_response = client
        .get_radio_artist(&artist_id.to_string())
        .await
        .map_err(|e| format!("Failed to fetch artist radio: {}", e))?;

    let track_ids: Vec<u64> = radio_response
        .tracks
        .items
        .iter()
        .map(|track| track.id)
        .collect();

    if track_ids.is_empty() {
        return Err("No radio tracks returned for this artist".to_string());
    }

    let mut tracks = Vec::new();
    for track_id in track_ids {
        if let Ok(track) = client.get_track(track_id).await {
            if let Some(ref performer) = track.performer {
                if blacklist_state.is_blacklisted(performer.id) {
                    continue;
                }
            }
            tracks.push(track);
        }
    }

    if tracks.is_empty() {
        return Err("Failed to fetch any radio tracks".to_string());
    }

    let queue_tracks: Vec<CoreQueueTrack> =
        tracks.iter().map(track_to_queue_track_from_api).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(0)).await;

    let queue_track_ids: Vec<u64> = tracks.iter().map(|track| track.id).collect();
    let context_id = format!(
        "qobuz_artist_radio_{}_{}",
        artist_id,
        chrono::Utc::now().timestamp()
    );
    let context = PlaybackContext::new(
        ContextType::Radio,
        context_id.clone(),
        artist_name,
        ContentSource::Qobuz,
        queue_track_ids,
        0,
    );
    state.context.set_context(context);

    Ok(context_id)
}

/// Create track radio using the Qobuz `/radio/track` API endpoint.
///
/// Like album radio, this calls the Qobuz API directly — the endpoint returns
/// recommended tracks in a single GET response.
#[tauri::command]
pub async fn v2_create_qobuz_track_radio(
    track_id: u64,
    track_name: String,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<String, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

    let client = state.client.read().await.clone();
    let radio_response = client
        .get_radio_track(&track_id.to_string())
        .await
        .map_err(|e| format!("Failed to fetch track radio: {}", e))?;

    let fetched_ids: Vec<u64> = radio_response
        .tracks
        .items
        .iter()
        .map(|track| track.id)
        .collect();

    if fetched_ids.is_empty() {
        return Err("No radio tracks returned for this track".to_string());
    }

    let mut tracks = Vec::new();
    for next_id in fetched_ids {
        if let Ok(track) = client.get_track(next_id).await {
            if let Some(ref performer) = track.performer {
                if blacklist_state.is_blacklisted(performer.id) {
                    continue;
                }
            }
            tracks.push(track);
        }
    }

    if tracks.is_empty() {
        return Err("Failed to fetch any radio tracks".to_string());
    }

    let queue_tracks: Vec<CoreQueueTrack> =
        tracks.iter().map(track_to_queue_track_from_api).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(0)).await;

    let queue_track_ids: Vec<u64> = tracks.iter().map(|track| track.id).collect();
    let context_id = format!(
        "qobuz_track_radio_{}_{}",
        track_id,
        chrono::Utc::now().timestamp()
    );
    let context = PlaybackContext::new(
        ContextType::Radio,
        context_id.clone(),
        track_name,
        ContentSource::Qobuz,
        queue_track_ids,
        0,
    );
    state.context.set_context(context);

    Ok(context_id)
}

/// Create an infinite radio session based on recent tracks.
///
/// V2 reimplementation of the legacy `create_infinite_radio` command.
/// Uses the most recent track's artist as the radio seed (via Qobuz `/radio/artist`
/// through `RadioPoolBuilder`) and returns 50 generated tracks WITHOUT touching
/// the queue or playback context. The frontend appends them to the existing
/// queue so the user's current listening session is preserved.
///
/// Called when the queue is about to end and `AutoplayMode::InfiniteRadio` is on.
#[tauri::command]
pub async fn v2_create_infinite_radio(
    recent_track_ids: Vec<u64>,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<super::queue::V2QueueTrack>, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

    if recent_track_ids.is_empty() {
        return Err("No recent tracks provided for infinite radio".to_string());
    }

    log::info!(
        "[V2 Radio] Creating infinite radio from {} recent tracks: {:?}",
        recent_track_ids.len(),
        recent_track_ids
    );

    let client = state.client.read().await.clone();

    let primary_track = client
        .get_track(recent_track_ids[0])
        .await
        .map_err(|e| format!("Failed to fetch primary track: {}", e))?;

    let artist_id = primary_track
        .performer
        .as_ref()
        .map(|p| p.id)
        .ok_or("Primary track has no artist")?;

    let session_id = tokio::task::spawn_blocking({
        let client = client.clone();
        move || -> Result<String, String> {
            let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
            let builder = crate::radio_engine::RadioPoolBuilder::new(
                &radio_db,
                &client,
                crate::radio_engine::BuildRadioOptions::default(),
            );
            let rt = tokio::runtime::Handle::current();
            let session = rt.block_on(builder.create_artist_radio(artist_id))?;
            Ok(session.id)
        }
    })
    .await
    .map_err(|e| format!("Infinite radio task failed: {}", e))??;

    // Demoted to debug to keep the radio session id out of default-level
    // logs (CodeQL rust/cleartext-logging). Set RUST_LOG=debug to see.
    log::debug!("[V2 Radio] Infinite radio session created: {}", session_id);

    let track_ids = tokio::task::spawn_blocking({
        let session_id = session_id.clone();
        move || -> Result<Vec<u64>, String> {
            let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
            let radio_engine = crate::radio_engine::RadioEngine::new(radio_db);
            let mut ids = Vec::new();
            for _ in 0..60 {
                match radio_engine.next_track(&session_id) {
                    Ok(radio_track) => ids.push(radio_track.track_id),
                    Err(_) => break,
                }
            }
            Ok(ids.into_iter().take(50).collect())
        }
    })
    .await
    .map_err(|e| format!("Track generation task failed: {}", e))??;

    let client = state.client.read().await;
    let mut tracks: Vec<super::queue::V2QueueTrack> = Vec::new();
    let recent_ids_set: std::collections::HashSet<u64> =
        recent_track_ids.iter().copied().collect();
    for next_track_id in track_ids {
        if recent_ids_set.contains(&next_track_id) {
            continue;
        }
        if let Ok(track) = client.get_track(next_track_id).await {
            if let Some(ref performer) = track.performer {
                if blacklist_state.is_blacklisted(performer.id) {
                    continue;
                }
            }
            let core: CoreQueueTrack = track_to_queue_track_from_api(&track);
            tracks.push(super::queue::V2QueueTrack {
                id: core.id,
                title: core.title,
                artist: core.artist,
                album: core.album,
                duration_secs: core.duration_secs,
                artwork_url: core.artwork_url,
                hires: core.hires,
                bit_depth: core.bit_depth,
                sample_rate: core.sample_rate,
                is_local: core.is_local,
                album_id: core.album_id.clone(),
                artist_id: core.artist_id,
                streamable: core.streamable,
                source: core.source,
                parental_warning: core.parental_warning,
                source_item_id_hint: core.source_item_id_hint.or(core.album_id),
            });
        }
    }

    if tracks.is_empty() {
        return Err("Failed to generate any infinite radio tracks".to_string());
    }

    log::info!("[V2 Radio] Returning {} infinite radio tracks", tracks.len());
    Ok(tracks)
}

fn track_to_queue_track_from_api(track: &crate::api::Track) -> CoreQueueTrack {
    let artwork_url = track
        .album
        .as_ref()
        .and_then(|a| a.image.large.clone())
        .or_else(|| track.album.as_ref().and_then(|a| a.image.thumbnail.clone()))
        .or_else(|| track.album.as_ref().and_then(|a| a.image.small.clone()));
    let artist = track
        .performer
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album = track
        .album
        .as_ref()
        .map(|a| a.title.clone())
        .unwrap_or_else(|| "Unknown Album".to_string());
    let album_id = track.album.as_ref().map(|a| a.id.clone());
    let artist_id = track.performer.as_ref().map(|p| p.id);

    CoreQueueTrack {
        id: track.id,
        title: track.title.clone(),
        artist,
        album,
        duration_secs: track.duration as u64,
        artwork_url,
        hires: track.hires,
        bit_depth: track.maximum_bit_depth,
        sample_rate: track.maximum_sampling_rate,
        is_local: false,
        album_id: album_id.clone(),
        artist_id,
        streamable: track.streamable,
        source: Some("qobuz".to_string()),
        parental_warning: track.parental_warning,
        source_item_id_hint: album_id,
    }
}
