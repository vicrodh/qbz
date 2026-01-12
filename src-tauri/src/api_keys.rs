//! User-provided API credentials store
//!
//! Manages runtime credentials that override embedded (build-time) credentials.
//! Users can provide their own API keys if the embedded ones stop working.

use std::sync::Arc;
use tokio::sync::Mutex;

/// Credentials for a specific provider
#[derive(Debug, Clone, Default)]
pub struct ProviderCredentials {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

impl ProviderCredentials {
    pub fn is_set(&self) -> bool {
        self.client_id.is_some() && self.client_secret.is_some()
    }
}

/// Store for all user-provided API credentials
#[derive(Debug, Default)]
pub struct ApiKeysStore {
    pub spotify: ProviderCredentials,
    pub tidal: ProviderCredentials,
    pub discogs: ProviderCredentials,
}

impl ApiKeysStore {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Thread-safe wrapper for the API keys store
pub type ApiKeysState = Arc<Mutex<ApiKeysStore>>;

pub fn create_api_keys_state() -> ApiKeysState {
    Arc::new(Mutex::new(ApiKeysStore::new()))
}

// Tauri commands

/// Set Spotify credentials
#[tauri::command]
pub async fn set_spotify_credentials(
    client_id: String,
    client_secret: String,
    state: tauri::State<'_, ApiKeysState>,
) -> Result<(), String> {
    log::info!("Setting user-provided Spotify credentials");
    let mut store = state.lock().await;
    store.spotify = ProviderCredentials {
        client_id: Some(client_id),
        client_secret: Some(client_secret),
    };
    Ok(())
}

/// Clear Spotify credentials
#[tauri::command]
pub async fn clear_spotify_credentials(
    state: tauri::State<'_, ApiKeysState>,
) -> Result<(), String> {
    log::info!("Clearing user-provided Spotify credentials");
    let mut store = state.lock().await;
    store.spotify = ProviderCredentials::default();
    Ok(())
}

/// Check if Spotify has user-provided credentials
#[tauri::command]
pub async fn has_spotify_user_credentials(
    state: tauri::State<'_, ApiKeysState>,
) -> Result<bool, String> {
    let store = state.lock().await;
    Ok(store.spotify.is_set())
}

/// Set Tidal credentials
#[tauri::command]
pub async fn set_tidal_credentials(
    client_id: String,
    client_secret: String,
    state: tauri::State<'_, ApiKeysState>,
) -> Result<(), String> {
    log::info!("Setting user-provided Tidal credentials");
    let mut store = state.lock().await;
    store.tidal = ProviderCredentials {
        client_id: Some(client_id),
        client_secret: Some(client_secret),
    };
    Ok(())
}

/// Clear Tidal credentials
#[tauri::command]
pub async fn clear_tidal_credentials(
    state: tauri::State<'_, ApiKeysState>,
) -> Result<(), String> {
    log::info!("Clearing user-provided Tidal credentials");
    let mut store = state.lock().await;
    store.tidal = ProviderCredentials::default();
    Ok(())
}

/// Check if Tidal has user-provided credentials
#[tauri::command]
pub async fn has_tidal_user_credentials(
    state: tauri::State<'_, ApiKeysState>,
) -> Result<bool, String> {
    let store = state.lock().await;
    Ok(store.tidal.is_set())
}

/// Set Discogs credentials
#[tauri::command]
pub async fn set_discogs_credentials(
    consumer_key: String,
    consumer_secret: String,
    state: tauri::State<'_, ApiKeysState>,
) -> Result<(), String> {
    log::info!("Setting user-provided Discogs credentials");
    let mut store = state.lock().await;
    store.discogs = ProviderCredentials {
        client_id: Some(consumer_key),
        client_secret: Some(consumer_secret),
    };
    Ok(())
}

/// Clear Discogs credentials
#[tauri::command]
pub async fn clear_discogs_credentials(
    state: tauri::State<'_, ApiKeysState>,
) -> Result<(), String> {
    log::info!("Clearing user-provided Discogs credentials");
    let mut store = state.lock().await;
    store.discogs = ProviderCredentials::default();
    Ok(())
}

/// Check if Discogs has user-provided credentials
#[tauri::command]
pub async fn has_discogs_user_credentials(
    state: tauri::State<'_, ApiKeysState>,
) -> Result<bool, String> {
    let store = state.lock().await;
    Ok(store.discogs.is_set())
}

/// Check which providers have embedded credentials available
#[tauri::command]
pub fn get_embedded_credentials_status() -> EmbeddedCredentialsStatus {
    EmbeddedCredentialsStatus {
        spotify: option_env!("SPOTIFY_API_CLIENT_ID").is_some()
            && option_env!("SPOTIFY_API_CLIENT_SECRET").is_some(),
        tidal: option_env!("TIDAL_API_CLIENT_ID").is_some()
            && option_env!("TIDAL_API_CLIENT_SECRET").is_some(),
        discogs: option_env!("DISCOGS_API_CLIENT_KEY").is_some()
            && option_env!("DISCOGS_API_CLIENT_SECRET").is_some(),
        lastfm: option_env!("LAST_FM_API_KEY").is_some()
            && option_env!("LAST_FM_API_SHARED_SECRET").is_some(),
    }
}

#[derive(serde::Serialize)]
pub struct EmbeddedCredentialsStatus {
    pub spotify: bool,
    pub tidal: bool,
    pub discogs: bool,
    pub lastfm: bool,
}
