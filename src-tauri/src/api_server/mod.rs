use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path, Query, State,
    },
    http::{header, Method, StatusCode},
    middleware,
    response::{
        sse::{Event, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use axum_server::{tls_rustls::RustlsConfig, Handle as AxumHandle};
use base64::Engine;
use futures_util::stream::Stream;
use rcgen::{CertificateParams, DistinguishedName, DnType, SanType};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::Once;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{broadcast, Mutex};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::{
    api::{Album, Artist, SearchResultsPage, Track},
    artist_blacklist::BlacklistState,
    commands::{self, search::SearchAllResults},
    config::{
        audio_settings::AudioSettingsState,
        playback_preferences::{AutoplayMode, PlaybackPreferences, PlaybackPreferencesState},
        remote_control_settings::{RemoteControlSettings, RemoteControlSettingsState},
    },
    offline_cache::OfflineCacheState,
    player::PlaybackEvent,
    queue::{QueueState as QueueStateData, QueueTrack},
    AppState,
};

#[derive(Clone)]
struct ApiContext {
    app_handle: AppHandle,
    token: String,
    playback_tx: broadcast::Sender<PlaybackEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteControlStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    pub local_url: String,
    pub secure: bool,
    pub cert_url: Option<String>,
    pub token: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteControlQr {
    pub qr_data_url: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NowPlayingResponse {
    playback: crate::player::PlaybackState,
    track: Option<QueueTrack>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PingResponse {
    ok: bool,
    version: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeekRequest {
    position: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VolumeRequest {
    volume: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayIndexRequest {
    index: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddToQueueRequest {
    track: QueueTrack,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShuffleRequest {
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepeatRequest {
    mode: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShuffleRepeatResponse {
    shuffle: bool,
    repeat: String,
}

#[derive(Debug)]
struct ApiServerHandle {
    handle: AxumHandle<SocketAddr>,
}

static TLS_PROVIDER_INIT: Once = Once::new();

fn ensure_tls_provider() -> Result<(), String> {
    let mut result = Ok(());
    TLS_PROVIDER_INIT.call_once(|| {
        result = rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .map_err(|e| format!("Failed to install TLS provider: {:?}", e));
    });
    result
}

struct ApiServerInner {
    current: Option<RemoteControlSettings>,
    server: Option<ApiServerHandle>,
    last_error: Option<String>,
}

pub struct ApiServerState {
    inner: Mutex<ApiServerInner>,
    playback_tx: broadcast::Sender<PlaybackEvent>,
}

impl ApiServerState {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            inner: Mutex::new(ApiServerInner {
                current: None,
                server: None,
                last_error: None,
            }),
            playback_tx: tx,
        }
    }

    pub async fn apply_settings(
        &self,
        app_handle: AppHandle,
        settings: RemoteControlSettings,
    ) -> Result<(), String> {
        let mut inner = self.inner.lock().await;

        if inner.current.as_ref() == Some(&settings) {
            return Ok(());
        }

        if let Some(handle) = inner.server.take() {
            handle
                .handle
                .graceful_shutdown(Some(Duration::from_secs(2)));
        }

        inner.current = Some(settings.clone());
        inner.last_error = None;

        if !settings.enabled {
            return Ok(());
        }

        let ctx = ApiContext {
            app_handle: app_handle.clone(),
            token: settings.token.clone(),
            playback_tx: self.playback_tx.clone(),
        };

        // Load allowed origins for CORS
        let allowed_origins = app_handle
            .try_state::<crate::config::AllowedOriginsState>()
            .map(|state| {
                state
                    .get_origins()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|o| o.origin)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let router = build_router(ctx, allowed_origins);
        let bind_addr = SocketAddr::from(([0, 0, 0, 0], settings.port));
        let handle = AxumHandle::<SocketAddr>::new();
        let make_service = router.into_make_service_with_connect_info::<SocketAddr>();

        if settings.secure {
            ensure_tls_provider().map_err(|err| {
                let msg = format!("Remote control TLS provider error: {}", err);
                inner.last_error = Some(msg.clone());
                msg
            })?;
            let (cert_path, key_path) = ensure_certificate().map_err(|err| {
                let msg = format!("Remote control certificate error: {}", err);
                inner.last_error = Some(msg.clone());
                msg
            })?;

            let tls_config = RustlsConfig::from_pem_file(cert_path, key_path)
                .await
                .map_err(|err| {
                    let msg = format!("Remote control TLS config failed: {}", err);
                    inner.last_error = Some(msg.clone());
                    msg
                })?;

            let handle_clone = handle.clone();
            tauri::async_runtime::spawn(async move {
                let server = axum_server::bind_rustls(bind_addr, tls_config)
                    .handle(handle_clone)
                    .serve(make_service);

                if let Err(err) = server.await {
                    log::error!("Remote control HTTPS server error: {}", err);
                }
            });
        } else {
            let handle_clone = handle.clone();
            tauri::async_runtime::spawn(async move {
                let server = axum_server::bind(bind_addr)
                    .handle(handle_clone)
                    .serve(make_service);

                if let Err(err) = server.await {
                    log::error!("Remote control HTTP server error: {}", err);
                }
            });
        }

        inner.server = Some(ApiServerHandle { handle });
        Ok(())
    }

    pub async fn status(
        &self,
        settings: &RemoteControlSettings,
        local_url: String,
    ) -> RemoteControlStatus {
        let inner = self.inner.lock().await;
        let cert_url = if settings.secure {
            Some(format!("{}/api/cert", local_url))
        } else {
            None
        };
        RemoteControlStatus {
            enabled: settings.enabled,
            running: inner.server.is_some(),
            port: settings.port,
            local_url,
            secure: settings.secure,
            cert_url,
            token: settings.token.clone(),
            last_error: inner.last_error.clone(),
        }
    }

    pub fn broadcast(&self, event: PlaybackEvent) {
        let _ = self.playback_tx.send(event);
    }
}

pub fn broadcast_playback_event(app_handle: &AppHandle, event: &PlaybackEvent) {
    if let Some(state) = app_handle.try_state::<ApiServerState>() {
        state.broadcast(event.clone());
    }
}

/// Emit playback event to both desktop UI (Tauri event) and PWA (WebSocket)
fn emit_playback_update(app_handle: &AppHandle) {
    let app_state = app_handle.state::<AppState>();
    let mut event = app_state.player.get_playback_event();
    // Add queue state to event for PWA sync
    event.shuffle = Some(app_state.queue.is_shuffle());
    event.repeat = Some(match app_state.queue.get_repeat() {
        crate::queue::RepeatMode::Off => "off".to_string(),
        crate::queue::RepeatMode::All => "all".to_string(),
        crate::queue::RepeatMode::One => "one".to_string(),
    });
    // Emit to desktop UI
    let _ = app_handle.emit("playback:state", &event);
    // Broadcast to WebSocket clients (PWA)
    broadcast_playback_event(app_handle, &event);
}

/// Emit queue state event to desktop UI when shuffle/repeat changes
fn emit_queue_state_update(app_handle: &AppHandle) {
    let app_state = app_handle.state::<AppState>();
    let shuffle = app_state.queue.is_shuffle();
    let repeat = match app_state.queue.get_repeat() {
        crate::queue::RepeatMode::Off => "off",
        crate::queue::RepeatMode::All => "all",
        crate::queue::RepeatMode::One => "one",
    };
    let _ = app_handle.emit(
        "queue:state",
        serde_json::json!({
            "shuffle": shuffle,
            "repeat": repeat
        }),
    );
}

#[tauri::command]
pub async fn remote_control_get_status(app: AppHandle) -> Result<RemoteControlStatus, String> {
    let settings_state = app.state::<RemoteControlSettingsState>();
    let settings = settings_state.get_settings()?;
    let local_url = get_local_url(settings.port, settings.secure);
    let api_state = app.state::<ApiServerState>();
    Ok(api_state.status(&settings, local_url).await)
}

#[tauri::command]
pub async fn remote_control_set_enabled(
    enabled: bool,
    app: AppHandle,
) -> Result<RemoteControlStatus, String> {
    let settings_state = app.state::<RemoteControlSettingsState>();
    settings_state.set_enabled(enabled)?;
    sync_server(&app).await?;
    remote_control_get_status(app).await
}

#[tauri::command]
pub async fn remote_control_set_port(
    port: u16,
    app: AppHandle,
) -> Result<RemoteControlStatus, String> {
    if port < 1024 {
        return Err("Port must be between 1024 and 65535".to_string());
    }
    let settings_state = app.state::<RemoteControlSettingsState>();
    settings_state.set_port(port)?;
    sync_server(&app).await?;
    remote_control_get_status(app).await
}

#[tauri::command]
pub async fn remote_control_set_secure(
    secure: bool,
    app: AppHandle,
) -> Result<RemoteControlStatus, String> {
    let settings_state = app.state::<RemoteControlSettingsState>();
    settings_state.set_secure(secure)?;
    sync_server(&app).await?;
    remote_control_get_status(app).await
}

#[tauri::command]
pub async fn remote_control_regenerate_token(app: AppHandle) -> Result<RemoteControlQr, String> {
    let settings_state = app.state::<RemoteControlSettingsState>();
    let _ = settings_state.regenerate_token()?;
    sync_server(&app).await?;
    remote_control_get_pairing_qr(app).await
}

#[tauri::command]
pub async fn remote_control_get_pairing_qr(app: AppHandle) -> Result<RemoteControlQr, String> {
    let settings_state = app.state::<RemoteControlSettingsState>();
    let settings = settings_state.get_settings()?;
    let url = get_local_url(settings.port, settings.secure);
    let payload = serde_json::json!({
        "url": url,
        "token": settings.token,
        "name": get_device_name(),
        "version": env!("CARGO_PKG_VERSION"),
    });

    let svg = qrcode_generator::to_svg_to_string(
        payload.to_string(),
        qrcode_generator::QrCodeEcc::Low,
        512,
        None::<&str>,
    )
    .map_err(|e| format!("QR generation failed: {}", e))?;

    let svg_base64 = base64::engine::general_purpose::STANDARD.encode(svg.as_bytes());
    let data_url = format!("data:image/svg+xml;base64,{}", svg_base64);

    Ok(RemoteControlQr {
        qr_data_url: data_url,
        url,
    })
}

pub async fn sync_server(app: &AppHandle) -> Result<(), String> {
    let settings_state = app.state::<RemoteControlSettingsState>();
    let settings = settings_state.get_settings()?;
    let api_state = app.state::<ApiServerState>();
    api_state.apply_settings(app.clone(), settings).await
}

// ============================================================================
// Allowed Origins Commands
// ============================================================================

#[tauri::command]
pub async fn remote_control_get_allowed_origins(
    app: AppHandle,
) -> Result<Vec<crate::config::AllowedOrigin>, String> {
    let state = app.state::<crate::config::AllowedOriginsState>();
    state.get_origins()
}

#[tauri::command]
pub async fn remote_control_add_allowed_origin(
    origin: String,
    app: AppHandle,
) -> Result<crate::config::AllowedOrigin, String> {
    let state = app.state::<crate::config::AllowedOriginsState>();
    let result = state.add_origin(&origin)?;
    // Restart server to apply new CORS settings
    sync_server(&app).await?;
    Ok(result)
}

#[tauri::command]
pub async fn remote_control_remove_allowed_origin(id: i64, app: AppHandle) -> Result<(), String> {
    let state = app.state::<crate::config::AllowedOriginsState>();
    state.remove_origin(id)?;
    // Restart server to apply new CORS settings
    sync_server(&app).await?;
    Ok(())
}

#[tauri::command]
pub async fn remote_control_restore_default_origins(
    app: AppHandle,
) -> Result<Vec<crate::config::AllowedOrigin>, String> {
    let state = app.state::<crate::config::AllowedOriginsState>();
    state.restore_defaults()?;
    // Restart server to apply new CORS settings
    sync_server(&app).await?;
    state.get_origins()
}

fn build_router(ctx: ApiContext, allowed_origins: Vec<String>) -> Router {
    let allowed_origins = std::sync::Arc::new(allowed_origins);
    let origins_clone = allowed_origins.clone();

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _| {
            is_local_origin(origin, &origins_clone)
        }))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::HeaderName::from_static("x-api-key"),
        ]);

    Router::new()
        .route("/api/ping", get(ping))
        .route("/api/cert", get(get_certificate))
        .route("/api/docs", get(get_openapi_spec))
        .route("/api/status", get(now_playing))
        .route("/api/now-playing", get(now_playing))
        .route("/api/playback/play", post(play))
        .route("/api/playback/pause", post(pause))
        .route("/api/playback/next", post(next_track))
        .route("/api/playback/previous", post(previous_track))
        .route("/api/playback/seek", post(seek))
        .route("/api/playback/volume", post(set_volume))
        .route("/api/search", get(search_tracks))
        .route("/api/search/all", get(search_all))
        .route("/api/queue", get(get_queue))
        .route("/api/queue/add", post(add_to_queue))
        .route("/api/queue/add-next", post(add_to_queue_next))
        .route("/api/queue/play", post(play_queue_index))
        .route("/api/queue/shuffle", post(set_shuffle))
        .route("/api/queue/repeat", post(set_repeat))
        .route("/api/favorites", get(get_favorites))
        .route("/api/favorites/add", post(add_favorite))
        .route("/api/favorites/remove", post(remove_favorite))
        .route("/api/album/play", post(play_album))
        .route("/api/album/:id", get(get_album))
        .route("/api/artist/:id", get(get_artist))
        .route("/api/playback/preferences", get(get_playback_preferences))
        .route("/api/playback/autoplay", post(set_autoplay))
        .route("/api/ws", get(ws_handler))
        .route("/api/events", get(sse_handler))
        .with_state(ctx.clone())
        .layer(middleware::from_fn(lan_only))
        .layer(middleware::from_fn_with_state(ctx, require_token))
        .layer(cors)
}

async fn ping(State(_ctx): State<ApiContext>) -> impl IntoResponse {
    let name = get_device_name();
    Json(PingResponse {
        ok: true,
        version: env!("CARGO_PKG_VERSION").to_string(),
        name,
    })
}

async fn get_certificate() -> Result<impl IntoResponse, StatusCode> {
    let (cert_path, _) = ensure_certificate().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let pem = std::fs::read_to_string(cert_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/x-pem-file"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_static("attachment; filename=\"qbz-remote-control.pem\""),
    );

    Ok((headers, pem))
}

async fn get_openapi_spec() -> impl IntoResponse {
    let spec = include_str!("../../../docs/openapi.yaml");
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/yaml; charset=utf-8"),
    );
    (headers, spec)
}

async fn now_playing(
    State(ctx): State<ApiContext>,
) -> Result<Json<NowPlayingResponse>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let playback = app_state
        .player
        .get_state()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let track = app_state.queue.current_track();
    Ok(Json(NowPlayingResponse { playback, track }))
}

async fn play(State(ctx): State<ApiContext>) -> Result<StatusCode, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    app_state.media_controls.set_playback(true);
    app_state
        .player
        .resume()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    emit_playback_update(&ctx.app_handle);
    Ok(StatusCode::NO_CONTENT)
}

async fn pause(State(ctx): State<ApiContext>) -> Result<StatusCode, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    app_state.media_controls.set_playback(false);
    app_state
        .player
        .pause()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    emit_playback_update(&ctx.app_handle);
    Ok(StatusCode::NO_CONTENT)
}

async fn next_track(State(ctx): State<ApiContext>) -> Result<Json<Option<QueueTrack>>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let next = app_state.queue.next();
    if let Some(track) = next {
        let played = play_queue_track(&ctx, track).await?;
        return Ok(Json(Some(played)));
    }
    Ok(Json(None))
}

async fn previous_track(
    State(ctx): State<ApiContext>,
) -> Result<Json<Option<QueueTrack>>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let prev = app_state.queue.previous();
    if let Some(track) = prev {
        let played = play_queue_track(&ctx, track).await?;
        return Ok(Json(Some(played)));
    }
    Ok(Json(None))
}

async fn play_queue_track(ctx: &ApiContext, track: QueueTrack) -> Result<QueueTrack, StatusCode> {
    if track.is_local {
        return Err(StatusCode::NOT_IMPLEMENTED);
    }

    let app_state = ctx.app_handle.state::<AppState>();
    let offline_cache = ctx.app_handle.state::<OfflineCacheState>();
    let audio_settings = ctx.app_handle.state::<AudioSettingsState>();

    if let Err(err) = commands::playback::play_track(
        track.id,
        Some(track.duration_secs),
        None,
        app_state,
        offline_cache,
        audio_settings,
    )
    .await
    {
        log::error!("Remote control play_track failed: {}", err);
        return Err(StatusCode::BAD_GATEWAY);
    }

    // Emit update to both desktop UI and PWA
    emit_playback_update(&ctx.app_handle);

    Ok(track)
}

async fn seek(
    State(ctx): State<ApiContext>,
    Json(payload): Json<SeekRequest>,
) -> Result<StatusCode, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    commands::playback::seek(payload.position, app_state)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    emit_playback_update(&ctx.app_handle);
    Ok(StatusCode::NO_CONTENT)
}

async fn set_volume(
    State(ctx): State<ApiContext>,
    Json(payload): Json<VolumeRequest>,
) -> Result<StatusCode, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    commands::playback::set_volume(payload.volume, app_state)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    emit_playback_update(&ctx.app_handle);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Deserialize)]
struct FavoritesQuery {
    fav_type: String,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FavoriteRequest {
    fav_type: String,
    item_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayAlbumRequest {
    album_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetAutoplayRequest {
    mode: String,
}

async fn get_playback_preferences(
    State(ctx): State<ApiContext>,
) -> Result<Json<PlaybackPreferences>, StatusCode> {
    let prefs_state = ctx.app_handle.state::<PlaybackPreferencesState>();
    prefs_state
        .get_preferences()
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn set_autoplay(
    State(ctx): State<ApiContext>,
    Json(payload): Json<SetAutoplayRequest>,
) -> Result<StatusCode, StatusCode> {
    let prefs_state = ctx.app_handle.state::<PlaybackPreferencesState>();
    let mode = match payload.mode.as_str() {
        "continue" => AutoplayMode::ContinueWithinSource,
        "track_only" => AutoplayMode::PlayTrackOnly,
        "infinite" => AutoplayMode::InfiniteRadio,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    prefs_state
        .set_autoplay_mode(mode)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn search_tracks(
    State(ctx): State<ApiContext>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResultsPage<Track>>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let client = app_state.client.read().await;
    let results = client
        .search_tracks(
            &query.q,
            query.limit.unwrap_or(20),
            query.offset.unwrap_or(0),
            None,
        )
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(results))
}

async fn search_all(
    State(ctx): State<ApiContext>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchAllResults>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let blacklist_state = ctx.app_handle.state::<BlacklistState>();

    // Call the existing search_all command logic inline
    let client = app_state.client.read().await;

    // Parallel search for each type
    let (albums_result, tracks_result, artists_result) = tokio::join!(
        client.search_albums(
            &query.q,
            query.limit.unwrap_or(10),
            query.offset.unwrap_or(0),
            None
        ),
        client.search_tracks(
            &query.q,
            query.limit.unwrap_or(10),
            query.offset.unwrap_or(0),
            None
        ),
        client.search_artists(
            &query.q,
            query.limit.unwrap_or(10),
            query.offset.unwrap_or(0),
            None
        ),
    );

    let mut albums = albums_result.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let mut tracks = tracks_result.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let mut artists = artists_result.map_err(|_| StatusCode::BAD_GATEWAY)?;

    // Filter blacklisted artists
    albums
        .items
        .retain(|album| !blacklist_state.is_blacklisted(album.artist.id));
    tracks.items.retain(|track| {
        if let Some(ref performer) = track.performer {
            !blacklist_state.is_blacklisted(performer.id)
        } else {
            true
        }
    });
    artists
        .items
        .retain(|artist| !blacklist_state.is_blacklisted(artist.id));

    Ok(Json(SearchAllResults {
        albums,
        tracks,
        artists,
        playlists: SearchResultsPage {
            items: vec![],
            total: 0,
            offset: 0,
            limit: 10,
        },
        most_popular: None,
    }))
}

async fn get_queue(State(ctx): State<ApiContext>) -> Result<Json<QueueStateData>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    Ok(Json(app_state.queue.get_state()))
}

async fn add_to_queue(
    State(ctx): State<ApiContext>,
    Json(payload): Json<AddToQueueRequest>,
) -> Result<StatusCode, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    app_state.queue.add_track(payload.track);
    Ok(StatusCode::NO_CONTENT)
}

async fn add_to_queue_next(
    State(ctx): State<ApiContext>,
    Json(payload): Json<AddToQueueRequest>,
) -> Result<StatusCode, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    app_state.queue.add_track_next(payload.track);
    Ok(StatusCode::NO_CONTENT)
}

async fn play_queue_index(
    State(ctx): State<ApiContext>,
    Json(payload): Json<PlayIndexRequest>,
) -> Result<Json<Option<QueueTrack>>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let track = app_state.queue.play_index(payload.index);
    if let Some(ref t) = track {
        if !t.is_local {
            let offline_cache = ctx.app_handle.state::<OfflineCacheState>();
            let audio_settings = ctx.app_handle.state::<AudioSettingsState>();
            if let Err(err) = commands::playback::play_track(
                t.id,
                Some(t.duration_secs),
                None,
                app_state,
                offline_cache,
                audio_settings,
            )
            .await
            {
                log::error!("Remote control play_queue_index failed: {}", err);
                return Err(StatusCode::BAD_GATEWAY);
            }
            emit_playback_update(&ctx.app_handle);
        }
    }
    Ok(Json(track))
}

async fn set_shuffle(
    State(ctx): State<ApiContext>,
    Json(payload): Json<ShuffleRequest>,
) -> Result<Json<ShuffleRepeatResponse>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    app_state.queue.set_shuffle(payload.enabled);
    let repeat = match app_state.queue.get_repeat() {
        crate::queue::RepeatMode::Off => "off",
        crate::queue::RepeatMode::All => "all",
        crate::queue::RepeatMode::One => "one",
    };
    // Notify desktop UI of queue state change
    emit_queue_state_update(&ctx.app_handle);
    Ok(Json(ShuffleRepeatResponse {
        shuffle: app_state.queue.is_shuffle(),
        repeat: repeat.to_string(),
    }))
}

async fn set_repeat(
    State(ctx): State<ApiContext>,
    Json(payload): Json<RepeatRequest>,
) -> Result<Json<ShuffleRepeatResponse>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let mode = match payload.mode.to_lowercase().as_str() {
        "all" => crate::queue::RepeatMode::All,
        "one" => crate::queue::RepeatMode::One,
        _ => crate::queue::RepeatMode::Off,
    };
    app_state.queue.set_repeat(mode);
    // Notify desktop UI of queue state change
    emit_queue_state_update(&ctx.app_handle);
    Ok(Json(ShuffleRepeatResponse {
        shuffle: app_state.queue.is_shuffle(),
        repeat: payload.mode,
    }))
}

async fn get_favorites(
    State(ctx): State<ApiContext>,
    Query(query): Query<FavoritesQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let client = app_state.client.read().await;
    let result = client
        .get_favorites(
            &query.fav_type,
            query.limit.unwrap_or(50),
            query.offset.unwrap_or(0),
        )
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(result))
}

async fn add_favorite(
    State(ctx): State<ApiContext>,
    Json(payload): Json<FavoriteRequest>,
) -> Result<StatusCode, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let client = app_state.client.read().await;
    client
        .add_favorite(&payload.fav_type, &payload.item_id)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_favorite(
    State(ctx): State<ApiContext>,
    Json(payload): Json<FavoriteRequest>,
) -> Result<StatusCode, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let client = app_state.client.read().await;
    client
        .remove_favorite(&payload.fav_type, &payload.item_id)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn play_album(
    State(ctx): State<ApiContext>,
    Json(payload): Json<PlayAlbumRequest>,
) -> Result<StatusCode, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();

    // Get album with tracks (scope the client lock)
    let album = {
        let client = app_state.client.read().await;
        client
            .get_album(&payload.album_id)
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?
    };

    // Extract tracks and convert to QueueTrack
    let tracks: Vec<QueueTrack> = if let Some(tracks) = album.tracks {
        tracks
            .items
            .into_iter()
            .map(|t| {
                let artist_name = t
                    .performer
                    .as_ref()
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| album.artist.name.clone());
                let artwork = album.image.large.clone().or(album.image.small.clone());

                QueueTrack {
                    id: t.id,
                    title: t.title,
                    artist: artist_name,
                    album: album.title.clone(),
                    duration_secs: t.duration as u64,
                    artwork_url: artwork,
                    hires: t.hires,
                    bit_depth: t.maximum_bit_depth,
                    sample_rate: t.maximum_sampling_rate,
                    is_local: false,
                    album_id: Some(album.id.clone()),
                    artist_id: t.performer.as_ref().map(|p| p.id),
                    streamable: t.streamable,
                    source: Some("qobuz".to_string()),
                }
            })
            .collect()
    } else {
        return Err(StatusCode::NOT_FOUND);
    };

    if tracks.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Clear queue and add all tracks
    app_state.queue.clear();
    for track in tracks {
        app_state.queue.add_track(track);
    }

    // Play the first track
    if let Some(first_track) = app_state.queue.play_index(0) {
        if !first_track.is_local {
            let offline_cache = ctx.app_handle.state::<OfflineCacheState>();
            let audio_settings = ctx.app_handle.state::<AudioSettingsState>();
            if let Err(err) = commands::playback::play_track(
                first_track.id,
                Some(first_track.duration_secs),
                None,
                app_state,
                offline_cache,
                audio_settings,
            )
            .await
            {
                log::error!("Remote control play_album failed: {}", err);
                return Err(StatusCode::BAD_GATEWAY);
            }
            emit_playback_update(&ctx.app_handle);
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn get_album(
    State(ctx): State<ApiContext>,
    Path(album_id): Path<String>,
) -> Result<Json<Album>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let client = app_state.client.read().await;
    let album = client
        .get_album(&album_id)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(album))
}

async fn get_artist(
    State(ctx): State<ApiContext>,
    Path(artist_id): Path<String>,
) -> Result<Json<Artist>, StatusCode> {
    let app_state = ctx.app_handle.state::<AppState>();
    let client = app_state.client.read().await;
    let id: u64 = artist_id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let artist = client
        .get_artist(id, true) // Include albums
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(artist))
}

async fn ws_handler(State(ctx): State<ApiContext>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, ctx.playback_tx.subscribe()))
}

async fn handle_ws(mut socket: WebSocket, mut rx: broadcast::Receiver<PlaybackEvent>) {
    loop {
        match rx.recv().await {
            Ok(event) => {
                let payload = match serde_json::to_string(&event) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if socket.send(Message::Text(payload)).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

async fn sse_handler(
    State(ctx): State<ApiContext>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = ctx.playback_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        match result {
            Ok(event) => {
                let json = serde_json::to_string(&event).ok()?;
                Some(Ok(Event::default().data(json)))
            }
            Err(_) => None, // Skip lagged messages
        }
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

async fn require_token(
    State(ctx): State<ApiContext>,
    req: axum::http::Request<Body>,
    next: middleware::Next,
) -> impl IntoResponse {
    if req.method() == Method::OPTIONS {
        return StatusCode::NO_CONTENT.into_response();
    }

    // Public endpoints (no auth required)
    let path = req.uri().path();
    if path == "/api/cert" || path == "/api/docs" {
        return next.run(req).await;
    }

    if ctx.token.is_empty() {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Some(value) = req.headers().get("x-api-key") {
        if value.as_bytes() == ctx.token.as_bytes() {
            return next.run(req).await;
        }
    }

    if let Some(value) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(auth) = value.to_str() {
            if let Some(token) = auth.strip_prefix("Bearer ") {
                if token.as_bytes() == ctx.token.as_bytes() {
                    return next.run(req).await;
                }
            }
        }
    }

    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            let mut parts = pair.splitn(2, '=');
            if let (Some("token"), Some(value)) = (parts.next(), parts.next()) {
                if value.as_bytes() == ctx.token.as_bytes() {
                    return next.run(req).await;
                }
            }
        }
    }

    StatusCode::UNAUTHORIZED.into_response()
}

async fn lan_only(req: axum::http::Request<Body>, next: middleware::Next) -> impl IntoResponse {
    let addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);

    if let Some(addr) = addr {
        if is_local_addr(addr.ip()) {
            return next.run(req).await;
        }
    }

    StatusCode::FORBIDDEN.into_response()
}

fn is_local_addr(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(addr) => addr.is_private() || addr.is_loopback(),
        IpAddr::V6(addr) => {
            addr.is_loopback() || addr.is_unique_local() || is_ipv6_link_local(addr)
        }
    }
}

fn is_local_origin(origin: &header::HeaderValue, allowed_origins: &[String]) -> bool {
    let origin_str = match origin.to_str() {
        Ok(value) => value,
        Err(_) => return false,
    };

    if origin_str == "null" {
        return false;
    }

    let uri: axum::http::Uri = match origin_str.parse() {
        Ok(value) => value,
        Err(_) => return false,
    };

    let host = match uri.host() {
        Some(value) => value,
        None => return false,
    };

    // Always allow localhost
    if host == "localhost" {
        return true;
    }

    // Check against configured allowed origins
    if allowed_origins.iter().any(|o| o == host) {
        return true;
    }

    // Allow local network IPs
    match host.parse::<IpAddr>() {
        Ok(ip) => is_local_addr(ip),
        Err(_) => false,
    }
}

fn is_ipv6_link_local(addr: std::net::Ipv6Addr) -> bool {
    (addr.segments()[0] & 0xffc0) == 0xfe80
}

fn get_local_url(port: u16, secure: bool) -> String {
    let ip = local_ip().unwrap_or_else(|| IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    let scheme = if secure { "https" } else { "http" };
    match ip {
        IpAddr::V4(addr) => format!("{}://{}:{}", scheme, addr, port),
        IpAddr::V6(addr) => format!("{}://[{}]:{}", scheme, addr, port),
    }
}

fn local_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let local_addr = socket.local_addr().ok()?;
    Some(local_addr.ip())
}

fn remote_control_data_dir() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir()
        .ok_or("Could not determine data directory")?
        .join("qbz");
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create data directory: {}", e))?;
    Ok(data_dir)
}

fn certificate_paths() -> Result<(PathBuf, PathBuf), String> {
    let dir = remote_control_data_dir()?;
    Ok((
        dir.join("remote_control_cert.pem"),
        dir.join("remote_control_key.pem"),
    ))
}

fn ensure_certificate() -> Result<(PathBuf, PathBuf), String> {
    let (cert_path, key_path) = certificate_paths()?;
    if cert_path.exists() && key_path.exists() {
        return Ok((cert_path, key_path));
    }

    let mut params = CertificateParams::new(Vec::new());
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "QBZ Remote Control");
    params.distinguished_name = dn;

    let mut sans = Vec::new();
    sans.push(SanType::DnsName("localhost".into()));
    sans.push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    if let Some(ip) = local_ip() {
        sans.push(SanType::IpAddress(ip));
    }
    if let Ok(hostname) = std::env::var("HOSTNAME") {
        if !hostname.is_empty() {
            sans.push(SanType::DnsName(hostname));
        }
    }
    params.subject_alt_names = sans;

    let cert = rcgen::Certificate::from_params(params)
        .map_err(|e| format!("Failed to generate certificate: {}", e))?;
    let cert_pem = cert
        .serialize_pem()
        .map_err(|e| format!("Failed to serialize certificate: {}", e))?;
    let key_pem = cert.serialize_private_key_pem();

    std::fs::write(&cert_path, cert_pem)
        .map_err(|e| format!("Failed to write certificate: {}", e))?;
    std::fs::write(&key_path, key_pem)
        .map_err(|e| format!("Failed to write certificate key: {}", e))?;

    Ok((cert_path, key_path))
}

fn get_device_name() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| "QBZ".to_string())
}
