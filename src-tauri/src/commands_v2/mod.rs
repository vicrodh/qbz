//! V2 Commands - Using the new multi-crate architecture
//!
//! These commands use QbzCore via CoreBridge instead of the old AppState.
//! Runtime contract ensures proper lifecycle (see ADR_RUNTIME_SESSION_CONTRACT.md).
//!
//! Playback flows through CoreBridge -> QbzCore -> Player (qbz-player crate).

use tauri::State;

use qbz_models::{
    Album, Artist, DiscoverAlbum, DiscoverData, DiscoverPlaylistsResponse, DiscoverResponse,
    GenreInfo, LabelDetail, LabelExploreResponse, LabelPageData, PageArtistResponse, Playlist,
    PlaylistTag, SearchResultsPage,
    Track, UserSession,
};

use crate::api::models::{
    PlaylistDuplicateResult, PlaylistWithTrackIds,
};
use crate::artist_blacklist::BlacklistState;
use crate::audio::{AlsaPlugin, AudioBackendType, AudioDevice, BackendManager};
use crate::cache::CacheStats;
use crate::config::audio_settings::{AudioSettings, AudioSettingsState};
use crate::config::developer_settings::DeveloperSettingsState;
use crate::config::favorites_preferences::FavoritesPreferences;
use crate::config::graphics_settings::GraphicsSettingsState;
use crate::config::legal_settings::LegalSettingsState;
use crate::config::playback_preferences::{
    AutoplayMode, PlaybackPreferences, PlaybackPreferencesState,
};
use crate::config::tray_settings::TraySettings;
use crate::config::tray_settings::TraySettingsState;
use crate::config::window_settings::WindowSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::library::LibraryState;
use crate::reco_store::RecoState;
use crate::runtime::{
    CommandRequirement, RuntimeError, RuntimeManagerState,
};
use crate::AppState;
use crate::integrations_v2::MusicBrainzV2State;
use std::collections::HashSet;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct V2SuggestionArtistInput {
    pub name: String,
    pub qobuz_id: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct V2PlaylistSuggestionsInput {
    pub artists: Vec<V2SuggestionArtistInput>,
    pub exclude_track_ids: Vec<u64>,
    #[serde(default)]
    pub include_reasons: bool,
    pub config: Option<crate::artist_vectors::SuggestionConfig>,
}

mod helpers;
pub use helpers::*;

mod runtime;
pub use runtime::*;

mod playback;
pub use playback::*;

mod auth;
pub use auth::*;

mod settings;
pub use settings::*;

mod library;
pub use library::*;

mod link_resolver;
pub use link_resolver::*;

mod queue;
pub use queue::*;

mod search;
pub use search::*;

mod favorites;
pub use favorites::*;

mod audio;
pub use audio::*;

mod playlists;
pub use playlists::*;

mod catalog;
pub use catalog::*;

mod integrations;
pub use integrations::*;

mod session;
pub use session::*;

mod legacy_compat;
pub use legacy_compat::*;

// ==================== Utility Commands ====================

/// Fetch a remote URL as bytes (bypasses WebView CORS restrictions).
/// Used for loading PDF booklets from Qobuz CDN.
#[tauri::command]
pub async fn v2_fetch_url_bytes(url: String) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch URL: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}: {}", response.status(), url));
    }

    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("Failed to read response: {}", e))
}

// ============ Image Cache Commands ============

/// Download an image via reqwest (rustls) and write to a temp file.
/// Returns a file:// URL that WebKit can load without needing system TLS.
/// Used as fallback when the image cache service is unavailable.
async fn download_image_to_temp(url: &str) -> Result<String, String> {
    let url_owned = url.to_string();
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let response = reqwest::blocking::Client::new()
            .get(&url_owned)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .map_err(|e| format!("Failed to download image: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }

        response
            .bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("Failed to read image bytes: {}", e))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    // Write to temp dir with a hash-based filename to avoid duplicates
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        hasher.finish()
    };
    let tmp_dir = std::env::temp_dir().join("qbz-img-proxy");
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("Failed to create temp dir: {}", e))?;
    let tmp_path = tmp_dir.join(format!("{:x}.img", hash));
    std::fs::write(&tmp_path, &bytes)
        .map_err(|e| format!("Failed to write temp image: {}", e))?;

    Ok(format!("file://{}", tmp_path.display()))
}

#[tauri::command]
pub async fn v2_get_cached_image(
    url: String,
    cache_state: State<'_, crate::image_cache::ImageCacheState>,
    settings_state: State<'_, crate::config::ImageCacheSettingsState>,
) -> Result<String, String> {
    // Check if caching is enabled
    let settings = {
        let lock = settings_state
            .store
            .lock()
            .map_err(|e| format!("Settings lock error: {}", e))?;
        match lock.as_ref() {
            Some(store) => store.get_settings()?,
            None => crate::config::ImageCacheSettings::default(),
        }
    };

    if !settings.enabled {
        // Cache disabled — still proxy through reqwest so WebKit never
        // needs to resolve HTTPS (fixes AppImage TLS on some distros)
        return download_image_to_temp(&url).await;
    }

    // Check cache first
    {
        let lock = cache_state
            .service
            .lock()
            .map_err(|e| format!("Cache lock error: {}", e))?;
        if let Some(service) = lock.as_ref() {
            if let Some(path) = service.get(&url) {
                return Ok(format!("file://{}", path.display()));
            }
        }
    }

    // Download the image via reqwest (uses rustls — own CA bundle)
    let url_clone = url.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let response = reqwest::blocking::Client::new()
            .get(&url_clone)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .map_err(|e| format!("Failed to download image: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }

        response
            .bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("Failed to read image bytes: {}", e))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    // Store in cache and evict if needed
    let store_result = {
        let max_bytes = (settings.max_size_mb as u64) * 1024 * 1024;
        let lock = cache_state
            .service
            .lock()
            .map_err(|e| format!("Cache lock error: {}", e))?;
        if let Some(service) = lock.as_ref() {
            let path = service.store(&url, &bytes)?;
            let _ = service.evict(max_bytes);
            Some(format!("file://{}", path.display()))
        } else {
            None
        }
    }; // lock dropped here, before any .await

    match store_result {
        Some(path) => Ok(path),
        // Service not initialized — use temp file fallback
        None => download_image_to_temp(&url).await,
    }
}

#[tauri::command]
pub async fn v2_get_image_cache_settings(
    state: State<'_, crate::config::ImageCacheSettingsState>,
) -> Result<crate::config::ImageCacheSettings, String> {
    let lock = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    match lock.as_ref() {
        Some(store) => store.get_settings(),
        None => Ok(crate::config::ImageCacheSettings::default()),
    }
}

#[tauri::command]
pub async fn v2_set_image_cache_enabled(
    enabled: bool,
    state: State<'_, crate::config::ImageCacheSettingsState>,
) -> Result<(), String> {
    let lock = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    match lock.as_ref() {
        Some(store) => store.set_enabled(enabled),
        None => Err("Image cache settings not initialized".to_string()),
    }
}

#[tauri::command]
pub async fn v2_set_image_cache_max_size(
    max_size_mb: u32,
    state: State<'_, crate::config::ImageCacheSettingsState>,
    cache_state: State<'_, crate::image_cache::ImageCacheState>,
) -> Result<(), String> {
    {
        let lock = state
            .store
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        match lock.as_ref() {
            Some(store) => store.set_max_size_mb(max_size_mb)?,
            None => return Err("Image cache settings not initialized".to_string()),
        }
    }
    // Trigger eviction with new limit
    let max_bytes = (max_size_mb as u64) * 1024 * 1024;
    let lock = cache_state
        .service
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    if let Some(service) = lock.as_ref() {
        let _ = service.evict(max_bytes);
    }
    Ok(())
}

#[tauri::command]
pub async fn v2_get_image_cache_stats(
    state: State<'_, crate::image_cache::ImageCacheState>,
) -> Result<crate::image_cache::ImageCacheStats, String> {
    let lock = state
        .service
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    match lock.as_ref() {
        Some(service) => service.stats(),
        None => Ok(crate::image_cache::ImageCacheStats {
            total_bytes: 0,
            file_count: 0,
        }),
    }
}

#[tauri::command]
pub async fn v2_clear_image_cache(
    state: State<'_, crate::image_cache::ImageCacheState>,
    reco_state: State<'_, crate::reco_store::RecoState>,
) -> Result<u64, String> {
    let freed = {
        let lock = state
            .service
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        match lock.as_ref() {
            Some(service) => service.clear()?,
            None => 0,
        }
    };

    // Also clear reco meta image URLs so they re-resolve with correct sizes
    {
        let guard__ = reco_state.db.lock().await;
        if let Some(db) = guard__.as_ref() {
            let _ = db.clear_meta_caches();
        }
    }

    Ok(freed)
}

// ==================== ListenBrainz Discovery ====================

/// Normalize an artist name for dedup: trim, lowercase, collapse whitespace
pub(crate) fn normalize_artist_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Discover new artists via MusicBrainz tag-based search.
///
/// Pipeline: "Listeners also enjoy"
///
/// Uses MusicBrainz tag search to find artists that share the seed artist's
/// primary genre tag. This gives genre-accurate results (e.g., searching
/// "thrash metal" for Metallica returns Megadeth, Slayer, Anthrax — not
/// mainstream crossover like Led Zeppelin).
///
/// Pipeline:
/// 1. Fetch seed artist's tags from MusicBrainz (sorted by vote count)
/// 2. Search MB for artists tagged with the primary genre tag
/// 3. Filter: seed artist, known similar artists, local listening history
/// 4. Resolve on Qobuz (verify exact name match to avoid homonyms)
/// 5. Return top 8, minimum 5 (frontend shows 6, keeps 2 reserves)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryArtist {
    pub mbid: String,
    pub name: String,
    pub normalized_name: String,
    pub affinity_score: f64,
    pub similarity_percent: f64,
    pub qobuz_id: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryResponse {
    pub artists: Vec<DiscoveryArtist>,
    pub primary_tag: String,
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_discovery_artists(
    seedMbid: String,
    seedArtistName: String,
    similarArtistNames: Vec<String>,
    musicbrainz: State<'_, MusicBrainzV2State>,
    reco_state: State<'_, RecoState>,
    bridge: State<'_, CoreBridgeState>,
    blacklist_state: State<'_, BlacklistState>,
) -> Result<DiscoveryResponse, String> {
    log::info!(
        "[Discovery] Starting pipeline for {} (MBID: {})",
        seedArtistName,
        seedMbid
    );

    // Step 1: Check MB is enabled
    {
        let client = musicbrainz.client.lock().await;
        if !client.is_enabled().await {
            log::warn!("[Discovery] MusicBrainz is disabled, returning empty");
            return Ok(DiscoveryResponse {
                artists: Vec::new(),
                primary_tag: String::new(),
            });
        }
    }

    // Step 2: Get seed artist's primary genre tag
    let seed_tags = {
        let client = musicbrainz.client.lock().await;
        client.get_artist_tags(&seedMbid).await.unwrap_or_default()
    };

    if seed_tags.is_empty() {
        log::warn!("[Discovery] No tags found for seed artist, returning empty");
        return Ok(DiscoveryResponse {
            artists: Vec::new(),
            primary_tag: String::new(),
        });
    }

    let primary_tag = &seed_tags[0];
    log::info!(
        "[Discovery] Seed primary tag: '{}' (from {} tags total)",
        primary_tag,
        seed_tags.len()
    );

    // Step 3: Search MB for artists with the same primary tag
    // Request more than we need to account for filtering
    let mb_results = {
        let client = musicbrainz.client.lock().await;
        client
            .search_artists_by_tag(primary_tag, 50)
            .await
            .map_err(|e| format!("Tag search failed: {}", e))?
    };

    log::info!(
        "[Discovery] MB tag search returned {} artists for '{}'",
        mb_results.artists.len(),
        primary_tag
    );

    if mb_results.artists.is_empty() {
        return Ok(DiscoveryResponse {
            artists: Vec::new(),
            primary_tag: primary_tag.to_string(),
        });
    }

    // Step 4: Build exclusion sets
    let seed_name_normalized = normalize_artist_name(&seedArtistName);

    let similar_names_set: HashSet<String> = similarArtistNames
        .iter()
        .map(|name| normalize_artist_name(name))
        .collect();

    // Exclude any artist listened more than 2 times (user already knows them)
    let listen_threshold: u32 = 2;
    let (local_known_qobuz_ids, local_known_names): (HashSet<u64>, HashSet<String>) = {
        let guard = reco_state.db.lock().await;
        if let Some(db) = guard.as_ref() {
            let top_artists = db.get_top_artist_ids(500).unwrap_or_default();
            let qobuz_ids: HashSet<u64> = top_artists
                .iter()
                .filter(|a| a.play_count > listen_threshold)
                .map(|a| a.artist_id)
                .collect();

            let known_artists = db.get_known_artist_names(1000).unwrap_or_default();
            let known_ids: HashSet<u64> = qobuz_ids.clone();
            let names: HashSet<String> = known_artists
                .iter()
                .filter(|(id, _)| known_ids.contains(id))
                .map(|(_, name)| normalize_artist_name(name))
                .collect();

            log::debug!(
                "[Discovery] Exclusion: {} known artists (>{} plays)",
                qobuz_ids.len(),
                listen_threshold
            );

            (qobuz_ids, names)
        } else {
            (HashSet::new(), HashSet::new())
        }
    };

    // Step 4b: Load dismissed artists for this tag
    let dismissed_names: HashSet<String> = {
        let guard = reco_state.db.lock().await;
        if let Some(db) = guard.as_ref() {
            db.get_dismissed_artists_for_tag(&primary_tag.to_lowercase())
                .unwrap_or_default()
                .into_iter()
                .collect()
        } else {
            HashSet::new()
        }
    };

    if !dismissed_names.is_empty() {
        log::debug!(
            "[Discovery] {} dismissed artists for tag '{}'",
            dismissed_names.len(),
            primary_tag
        );
    }

    // Step 5: Filter MB results
    let mut candidates: Vec<(String, String)> = Vec::new(); // (mbid, name)

    for artist in &mb_results.artists {
        let normalized = normalize_artist_name(&artist.name);

        // Skip seed artist
        if normalized == seed_name_normalized || artist.id.to_lowercase() == seedMbid.to_lowercase()
        {
            continue;
        }
        // Skip artists already shown in the similar section
        if similar_names_set.contains(&normalized) {
            continue;
        }
        // Skip locally known artists
        if local_known_names.contains(&normalized) {
            continue;
        }
        // Skip dismissed artists for this tag
        if dismissed_names.contains(&normalized) {
            continue;
        }
        candidates.push((artist.id.clone(), artist.name.clone()));
    }

    // Step 6: Shuffle deterministically using seed MBID
    // This ensures: same artist page = same results, different artist = different results
    {
        use rand::seq::SliceRandom;
        use rand::SeedableRng;
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        seedMbid.hash(&mut hasher);
        let hash = hasher.finish();
        let mut rng = rand::rngs::StdRng::seed_from_u64(hash);
        candidates.shuffle(&mut rng);
    }

    log::info!(
        "[Discovery] {} candidates after filtering + shuffle (from {} MB results)",
        candidates.len(),
        mb_results.artists.len()
    );

    // Step 7: Resolve on Qobuz
    let bridge_guard = bridge.try_get().await;
    let mut results: Vec<DiscoveryArtist> = Vec::new();
    let min_results = 5;
    let max_results = 8;

    if let Some(ref core_bridge) = bridge_guard {
        for (mbid, name) in &candidates {
            if results.len() >= max_results {
                break;
            }

            let qobuz_artist = match core_bridge.search_artists(name, 1, 0, None).await {
                Ok(search_results) => {
                    if let Some(artist) = search_results.items.first() {
                        let qobuz_norm = normalize_artist_name(&artist.name);
                        let cand_norm = normalize_artist_name(name);
                        if qobuz_norm == cand_norm
                            && !local_known_qobuz_ids.contains(&artist.id)
                            && !blacklist_state.is_blacklisted(artist.id)
                        {
                            Some((artist.id, artist.name.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                Err(_) => None,
            };

            if let Some((qobuz_id, qobuz_name)) = qobuz_artist {
                results.push(DiscoveryArtist {
                    mbid: mbid.to_string(),
                    name: qobuz_name.clone(),
                    normalized_name: normalize_artist_name(&qobuz_name),
                    affinity_score: 0.0,
                    similarity_percent: 0.0,
                    qobuz_id: Some(qobuz_id),
                });
            }
        }
    } else {
        log::warn!("[Discovery] CoreBridge not available");
        return Ok(DiscoveryResponse {
            artists: Vec::new(),
            primary_tag: primary_tag.to_string(),
        });
    }

    // Step 7: If not enough results with primary tag, try secondary tag
    if results.len() < min_results && seed_tags.len() > 1 {
        let secondary_tag = &seed_tags[1];
        log::info!(
            "[Discovery] Only {} results, trying secondary tag: '{}'",
            results.len(),
            secondary_tag
        );

        // Load dismissals for secondary tag too
        let secondary_dismissed: HashSet<String> = {
            let guard = reco_state.db.lock().await;
            if let Some(db) = guard.as_ref() {
                db.get_dismissed_artists_for_tag(&secondary_tag.to_lowercase())
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            } else {
                HashSet::new()
            }
        };

        let secondary_search = {
            let client = musicbrainz.client.lock().await;
            client.search_artists_by_tag(secondary_tag, 30).await
        };
        if let Ok(secondary_results) = secondary_search {
            let existing_mbids: HashSet<String> = results.iter().map(|r| r.mbid.clone()).collect();

            // Filter and shuffle secondary candidates too
            let mut secondary_candidates: Vec<(String, String)> = Vec::new();
            for artist in &secondary_results.artists {
                let normalized = normalize_artist_name(&artist.name);
                if normalized == seed_name_normalized
                    || artist.id.to_lowercase() == seedMbid.to_lowercase()
                {
                    continue;
                }
                if similar_names_set.contains(&normalized)
                    || local_known_names.contains(&normalized)
                    || dismissed_names.contains(&normalized)
                    || secondary_dismissed.contains(&normalized)
                {
                    continue;
                }
                if existing_mbids.contains(&artist.id) {
                    continue;
                }
                secondary_candidates.push((artist.id.clone(), artist.name.clone()));
            }

            {
                use rand::seq::SliceRandom;
                use rand::SeedableRng;
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};

                let mut hasher = DefaultHasher::new();
                seedMbid.hash(&mut hasher);
                secondary_tag.hash(&mut hasher);
                let hash = hasher.finish();
                let mut rng = rand::rngs::StdRng::seed_from_u64(hash);
                secondary_candidates.shuffle(&mut rng);
            }

            if let Some(ref core_bridge) = bridge_guard {
                for (mbid, name) in &secondary_candidates {
                    if results.len() >= max_results {
                        break;
                    }

                    let qobuz_artist = match core_bridge.search_artists(name, 1, 0, None).await {
                        Ok(sr) => {
                            if let Some(qa) = sr.items.first() {
                                let qobuz_norm = normalize_artist_name(&qa.name);
                                let cand_norm = normalize_artist_name(name);
                                if qobuz_norm == cand_norm
                                    && !local_known_qobuz_ids.contains(&qa.id)
                                    && !blacklist_state.is_blacklisted(qa.id)
                                {
                                    Some((qa.id, qa.name.clone()))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    };

                    if let Some((qobuz_id, qobuz_name)) = qobuz_artist {
                        results.push(DiscoveryArtist {
                            mbid: mbid.clone(),
                            name: qobuz_name.clone(),
                            normalized_name: normalize_artist_name(&qobuz_name),
                            affinity_score: 0.0,
                            similarity_percent: 0.0,
                            qobuz_id: Some(qobuz_id),
                        });
                    }
                }
            }
        }
    }

    log::info!("[Discovery] Returning {} discovery artists", results.len());
    Ok(DiscoveryResponse {
        artists: results,
        primary_tag: primary_tag.to_string(),
    })
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_dismiss_discovery_artist(
    tag: String,
    artistName: String,
    reco_state: State<'_, RecoState>,
) -> Result<(), String> {
    let normalized = normalize_artist_name(&artistName);
    let tag_lower = tag.to_lowercase();

    log::info!(
        "[Discovery] Dismissing '{}' for tag '{}'",
        normalized,
        tag_lower
    );

    let guard = reco_state.db.lock().await;
    if let Some(db) = guard.as_ref() {
        db.dismiss_discovery_artist(&tag_lower, &normalized)?;
    }
    Ok(())
}

// ==================== Runtime Diagnostics ====================

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnostics {
    // Audio: saved settings
    pub audio_output_device: Option<String>,
    pub audio_backend_type: Option<String>,
    pub audio_exclusive_mode: bool,
    pub audio_dac_passthrough: bool,
    pub audio_preferred_sample_rate: Option<u32>,
    pub audio_alsa_plugin: Option<String>,
    pub audio_alsa_hardware_volume: bool,
    pub audio_normalization_enabled: bool,
    pub audio_normalization_target_lufs: f32,
    pub audio_gapless_enabled: bool,
    pub audio_pw_force_bitperfect: bool,
    pub audio_stream_buffer_seconds: u8,
    pub audio_streaming_only: bool,

    // Graphics: saved settings
    pub gfx_hardware_acceleration: bool,
    pub gfx_force_x11: bool,
    pub gfx_gdk_scale: Option<String>,
    pub gfx_gdk_dpi_scale: Option<String>,
    pub gfx_gsk_renderer: Option<String>,

    // Graphics: runtime (what actually applied at startup)
    pub runtime_using_fallback: bool,
    pub runtime_is_wayland: bool,
    pub runtime_has_nvidia: bool,
    pub runtime_has_amd: bool,
    pub runtime_has_intel: bool,
    pub runtime_is_vm: bool,
    pub runtime_hw_accel_enabled: bool,
    pub runtime_force_x11_active: bool,

    // Developer settings
    pub dev_force_dmabuf: bool,

    // Environment variables (what WebKit actually sees)
    pub env_webkit_disable_dmabuf: Option<String>,
    pub env_webkit_disable_compositing: Option<String>,
    pub env_gdk_backend: Option<String>,
    pub env_gsk_renderer: Option<String>,
    pub env_libgl_always_software: Option<String>,
    pub env_wayland_display: Option<String>,
    pub env_xdg_session_type: Option<String>,

    // App info
    pub app_version: String,
}

#[tauri::command]
pub fn v2_get_runtime_diagnostics(
    audio_state: State<'_, AudioSettingsState>,
    graphics_state: State<'_, GraphicsSettingsState>,
    developer_state: State<'_, DeveloperSettingsState>,
) -> Result<RuntimeDiagnostics, RuntimeError> {
    // Audio settings (may not be available before login)
    let audio = audio_state
        .store
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()));

    // Graphics settings
    let gfx = graphics_state
        .store
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()));

    // Graphics runtime status (static atomics — always available)
    let gfx_status = crate::config::graphics_settings::get_graphics_startup_status();

    // Developer settings
    let dev = developer_state
        .store
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.get_settings().ok()));

    let env_var = |name: &str| std::env::var(name).ok();

    let audio_defaults = crate::config::audio_settings::AudioSettings::default();
    let audio = audio.unwrap_or(audio_defaults);
    let gfx = gfx.unwrap_or_default();
    let dev = dev.unwrap_or_default();

    Ok(RuntimeDiagnostics {
        audio_output_device: audio.output_device,
        audio_backend_type: audio.backend_type.map(|b| format!("{:?}", b)),
        audio_exclusive_mode: audio.exclusive_mode,
        audio_dac_passthrough: audio.dac_passthrough,
        audio_preferred_sample_rate: audio.preferred_sample_rate,
        audio_alsa_plugin: audio.alsa_plugin.map(|p| format!("{:?}", p)),
        audio_alsa_hardware_volume: audio.alsa_hardware_volume,
        audio_normalization_enabled: audio.normalization_enabled,
        audio_normalization_target_lufs: audio.normalization_target_lufs,
        audio_gapless_enabled: audio.gapless_enabled,
        audio_pw_force_bitperfect: audio.pw_force_bitperfect,
        audio_stream_buffer_seconds: audio.stream_buffer_seconds,
        audio_streaming_only: audio.streaming_only,

        gfx_hardware_acceleration: gfx.hardware_acceleration,
        gfx_force_x11: gfx.force_x11,
        gfx_gdk_scale: gfx.gdk_scale,
        gfx_gdk_dpi_scale: gfx.gdk_dpi_scale,
        gfx_gsk_renderer: gfx.gsk_renderer,

        runtime_using_fallback: gfx_status.using_fallback,
        runtime_is_wayland: gfx_status.is_wayland,
        runtime_has_nvidia: gfx_status.has_nvidia,
        runtime_has_amd: gfx_status.has_amd,
        runtime_has_intel: gfx_status.has_intel,
        runtime_is_vm: gfx_status.is_vm,
        runtime_hw_accel_enabled: gfx_status.hardware_accel_enabled,
        runtime_force_x11_active: gfx_status.force_x11_active,

        dev_force_dmabuf: dev.force_dmabuf,

        env_webkit_disable_dmabuf: env_var("WEBKIT_DISABLE_DMABUF_RENDERER"),
        env_webkit_disable_compositing: env_var("WEBKIT_DISABLE_COMPOSITING_MODE"),
        env_gdk_backend: env_var("GDK_BACKEND"),
        env_gsk_renderer: env_var("GSK_RENDERER"),
        env_libgl_always_software: env_var("LIBGL_ALWAYS_SOFTWARE"),
        env_wayland_display: env_var("WAYLAND_DISPLAY"),
        env_xdg_session_type: env_var("XDG_SESSION_TYPE"),

        app_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
