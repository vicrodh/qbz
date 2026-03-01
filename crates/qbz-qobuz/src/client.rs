//! Qobuz API client implementation

use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::auth::{get_timestamp, parse_login_response, sign_get_favorites, sign_get_file_url};
use super::bundle::{extract_bundle_tokens, BundleTokens};
use super::endpoints::{self, paths};
use super::error::{ApiError, Result};
use qbz_models::*;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0";

/// Qobuz API client
pub struct QobuzClient {
    http: Client,
    tokens: Arc<RwLock<Option<BundleTokens>>>,
    session: Arc<RwLock<Option<UserSession>>>,
    validated_secret: Arc<RwLock<Option<String>>>,
    locale: Arc<RwLock<String>>,
}

impl Clone for QobuzClient {
    fn clone(&self) -> Self {
        Self {
            http: self.http.clone(),
            tokens: Arc::clone(&self.tokens),
            session: Arc::clone(&self.session),
            validated_secret: Arc::clone(&self.validated_secret),
            locale: Arc::clone(&self.locale),
        }
    }
}

impl QobuzClient {
    /// Create a new client
    pub fn new() -> Result<Self> {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .cookie_store(true)
            .build()?;

        Ok(Self {
            http,
            tokens: Arc::new(RwLock::new(None)),
            session: Arc::new(RwLock::new(None)),
            validated_secret: Arc::new(RwLock::new(None)),
            locale: Arc::new(RwLock::new("en".to_string())),
        })
    }

    /// Initialize client by extracting bundle tokens
    pub async fn init(&self) -> Result<()> {
        let tokens = extract_bundle_tokens(&self.http).await?;
        *self.tokens.write().await = Some(tokens);
        Ok(())
    }

    /// Set the locale for API requests
    pub async fn set_locale(&self, locale: String) {
        *self.locale.write().await = locale;
    }

    /// Get the current locale (public for cache key generation)
    pub async fn get_locale(&self) -> String {
        self.locale.read().await.clone()
    }

    /// Get the current locale (internal use)
    async fn locale(&self) -> String {
        self.locale.read().await.clone()
    }

    /// Get app ID (public for catalog search)
    pub async fn app_id(&self) -> Result<String> {
        self.tokens
            .read()
            .await
            .as_ref()
            .map(|t| t.app_id.clone())
            .ok_or_else(|| ApiError::BundleExtractionError("Client not initialized".to_string()))
    }

    /// Get HTTP client reference (public for catalog search)
    pub fn get_http(&self) -> &Client {
        &self.http
    }

    /// Get validated secret (validates on first use)
    async fn secret(&self) -> Result<String> {
        // Check if we already have a validated secret
        if let Some(secret) = self.validated_secret.read().await.clone() {
            return Ok(secret);
        }

        // Need to validate secrets
        let tokens = self.tokens.read().await;
        let tokens = tokens
            .as_ref()
            .ok_or_else(|| ApiError::BundleExtractionError("Client not initialized".to_string()))?;

        for secret in &tokens.secrets {
            if self.test_secret(secret).await? {
                *self.validated_secret.write().await = Some(secret.clone());
                return Ok(secret.clone());
            }
        }

        Err(ApiError::InvalidAppSecret)
    }

    /// Test if a secret is valid using a known track
    async fn test_secret(&self, secret: &str) -> Result<bool> {
        let test_track_id = 5966783u64; // Known test track
        let timestamp = get_timestamp();
        let signature = sign_get_file_url(test_track_id, 5, timestamp, secret);

        let url = endpoints::build_url(paths::TRACK_GET_FILE_URL);
        let response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[
                ("track_id", test_track_id.to_string()),
                ("format_id", "5".to_string()),
                ("intent", "stream".to_string()),
                ("request_ts", timestamp.to_string()),
                ("request_sig", signature),
            ])
            .send()
            .await?;

        Ok(response.status() != StatusCode::BAD_REQUEST)
    }

    /// Login with email and password
    pub async fn login(&self, email: &str, password: &str) -> Result<UserSession> {
        let url = endpoints::build_url(paths::USER_LOGIN);
        let response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[("email", email), ("password", password)])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {
                let json: Value = response.json().await?;
                let session = parse_login_response(&json)?;
                *self.session.write().await = Some(session.clone());
                Ok(session)
            }
            StatusCode::UNAUTHORIZED => {
                Err(ApiError::AuthenticationError("Invalid credentials".to_string()))
            }
            StatusCode::BAD_REQUEST => Err(ApiError::InvalidAppId),
            status => Err(ApiError::ApiResponse(format!("Unexpected status: {}", status))),
        }
    }

    /// Check if logged in
    pub async fn is_logged_in(&self) -> bool {
        self.session.read().await.is_some()
    }

    /// Logout - clear the session
    pub async fn logout(&self) {
        *self.session.write().await = None;
    }

    /// Inject an already-authenticated session (e.g. from OAuth flow).
    /// Use this when the session was obtained outside this client instance.
    pub async fn set_session(&self, session: UserSession) {
        *self.session.write().await = Some(session);
    }

    /// Exchange an OAuth code for a full user session.
    ///
    /// This implements the new Qobuz OAuth flow:
    /// 1. GET /oauth/callback?code=CODE&private_key=KEY → { token }
    /// 2. POST /user/login with X-User-Auth-Token: token, body=extra=partner → UserSession
    pub async fn login_with_oauth_code(&self, code: &str) -> Result<UserSession> {
        use reqwest::header::{HeaderMap, HeaderValue};

        let tokens = self.tokens.read().await;
        let tokens = tokens.as_ref().ok_or_else(|| {
            ApiError::BundleExtractionError("Client not initialized".to_string())
        })?;
        let app_id = tokens.app_id.clone();
        let private_key = tokens.private_key.clone().ok_or_else(|| {
            ApiError::BundleExtractionError(
                "OAuth private key not available in bundle".to_string(),
            )
        })?;
        let _ = tokens; // drop read lock

        // Step 1: Exchange code for token
        let callback_url = endpoints::build_url(endpoints::paths::OAUTH_CALLBACK);
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-App-Id",
            HeaderValue::from_str(&app_id).map_err(|_| ApiError::InvalidAppId)?,
        );

        log::info!("[OAuth] Exchanging code for token via /oauth/callback");
        let callback_response = self
            .http
            .get(&callback_url)
            .headers(headers)
            .query(&[("code", code), ("private_key", &private_key), ("app_id", &app_id)])
            .send()
            .await?;

        if !callback_response.status().is_success() {
            return Err(ApiError::ApiResponse(format!(
                "OAuth callback failed with status {}",
                callback_response.status()
            )));
        }

        let callback_json: serde_json::Value = callback_response.json().await?;
        let token = callback_json["token"]
            .as_str()
            .ok_or_else(|| {
                ApiError::ApiResponse("OAuth callback: no token in response".to_string())
            })?
            .to_string();

        log::info!("[OAuth] Got token, fetching user session via /user/login");

        // Step 2: Fetch user session using the token
        let user_login_url = endpoints::build_url(endpoints::paths::USER_LOGIN);
        let mut auth_headers = HeaderMap::new();
        auth_headers.insert(
            "X-App-Id",
            HeaderValue::from_str(&app_id).map_err(|_| ApiError::InvalidAppId)?,
        );
        auth_headers.insert(
            "X-User-Auth-Token",
            HeaderValue::from_str(&token).map_err(|_| {
                ApiError::AuthenticationError("Invalid OAuth token format".into())
            })?,
        );

        let login_response = self
            .http
            .post(&user_login_url)
            .headers(auth_headers)
            .header("Content-Type", "text/plain;charset=UTF-8")
            .body("extra=partner")
            .send()
            .await?;

        match login_response.status() {
            StatusCode::OK => {
                let json: serde_json::Value = login_response.json().await?;
                let session = super::auth::parse_login_response(&json)?;
                *self.session.write().await = Some(session.clone());
                log::info!("[OAuth] Session established");
                Ok(session)
            }
            StatusCode::UNAUTHORIZED => Err(ApiError::AuthenticationError(
                "OAuth token rejected by user/login".to_string(),
            )),
            status => Err(ApiError::ApiResponse(format!(
                "user/login OAuth step failed with status {}",
                status
            ))),
        }
    }

    /// Get current user info (display name, subscription, and expiry if available)
    pub async fn get_user_info(&self) -> Option<(String, String, Option<String>)> {
        self.session.read().await.as_ref().map(|s| {
            (
                s.display_name.clone(),
                s.subscription_label.clone(),
                s.subscription_valid_until.clone(),
            )
        })
    }

    /// Get user auth token header value (public for catalog search)
    pub async fn auth_token(&self) -> Result<String> {
        self.session
            .read()
            .await
            .as_ref()
            .map(|s| s.user_auth_token.clone())
            .ok_or_else(|| ApiError::AuthenticationError("Not logged in".to_string()))
    }

    // === Header helpers ===

    /// Build standard API headers.
    /// Always includes X-App-Id. Includes X-User-Auth-Token when logged in.
    async fn api_headers(&self) -> Result<reqwest::header::HeaderMap> {
        use reqwest::header::{HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();

        let app_id = self.app_id().await?;
        headers.insert(
            "X-App-Id",
            HeaderValue::from_str(&app_id).map_err(|_| ApiError::InvalidAppId)?,
        );

        if let Ok(token) = self.auth_token().await {
            if let Ok(val) = HeaderValue::from_str(&token) {
                headers.insert("X-User-Auth-Token", val);
            }
        }

        Ok(headers)
    }

    /// Build headers that REQUIRE authentication. Fails if not logged in.
    async fn authenticated_headers(&self) -> Result<reqwest::header::HeaderMap> {
        use reqwest::header::{HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();

        let app_id = self.app_id().await?;
        headers.insert(
            "X-App-Id",
            HeaderValue::from_str(&app_id).map_err(|_| ApiError::InvalidAppId)?,
        );

        let token = self.auth_token().await?;
        headers.insert(
            "X-User-Auth-Token",
            HeaderValue::from_str(&token)
                .map_err(|_| ApiError::AuthenticationError("Invalid auth token format".into()))?,
        );

        Ok(headers)
    }

    // === Search endpoints ===

    /// Search for albums
    /// Optional search_type: "MainArtist", "Performer", "Composer", "Label", "ReleaseName"
    pub async fn search_albums(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Album>> {
        let url = endpoints::build_url(paths::ALBUM_SEARCH);
        let limit_str = limit.to_string();
        let offset_str = offset.to_string();

        let mut params: Vec<(&str, &str)> = vec![
            ("query", query),
            ("limit", &limit_str),
            ("offset", &offset_str),
        ];

        if let Some(st) = search_type {
            params.push(("type", st));
        }

        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&params)
            .send()
            .await?;
        log::debug!("[API] search_albums status={}", http_response.status());
        let response: Value = http_response.json().await?;

        let albums = response
            .get("albums")
            .ok_or_else(|| ApiError::ApiResponse("No albums in response".to_string()))?;

        Ok(serde_json::from_value(albums.clone())?)
    }

    /// Search for tracks
    /// Optional search_type: "MainArtist", "Performer", "Composer", "Label", "ReleaseName"
    pub async fn search_tracks(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Track>> {
        let url = endpoints::build_url(paths::TRACK_SEARCH);
        let limit_str = limit.to_string();
        let offset_str = offset.to_string();

        let mut params: Vec<(&str, &str)> = vec![
            ("query", query),
            ("limit", &limit_str),
            ("offset", &offset_str),
        ];

        if let Some(st) = search_type {
            params.push(("type", st));
        }

        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&params)
            .send()
            .await?;
        log::debug!("[API] search_tracks status={}", http_response.status());
        let response: Value = http_response.json().await?;

        let tracks = response
            .get("tracks")
            .ok_or_else(|| ApiError::ApiResponse("No tracks in response".to_string()))?;

        Ok(serde_json::from_value(tracks.clone())?)
    }

    /// Search for artists
    /// Optional search_type: "MainArtist", "Performer", "Composer", "Label", "ReleaseName"
    pub async fn search_artists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Artist>> {
        let url = endpoints::build_url(paths::ARTIST_SEARCH);
        let limit_str = limit.to_string();
        let offset_str = offset.to_string();

        let mut params: Vec<(&str, &str)> = vec![
            ("query", query),
            ("limit", &limit_str),
            ("offset", &offset_str),
        ];

        if let Some(st) = search_type {
            params.push(("type", st));
        }

        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&params)
            .send()
            .await?;
        log::debug!("[API] search_artists status={}", http_response.status());
        let response: Value = http_response.json().await?;

        let artists = response
            .get("artists")
            .ok_or_else(|| ApiError::ApiResponse("No artists in response".to_string()))?;

        Ok(serde_json::from_value(artists.clone())?)
    }

    /// Get similar artists for an artist ID
    pub async fn get_similar_artists(&self, artist_id: u64, limit: u32, offset: u32) -> Result<SearchResultsPage<Artist>> {
        let url = endpoints::build_url(paths::ARTIST_GET_SIMILAR);
        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[
                ("artist_id", artist_id.to_string()),
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ])
            .send()
            .await?;
        log::debug!("[API] get_similar_artists({}) status={}", artist_id, http_response.status());
        let response: Value = http_response.json().await?;

        let artists = response
            .get("artists")
            .ok_or_else(|| ApiError::ApiResponse("No artists in response".to_string()))?;

        Ok(serde_json::from_value(artists.clone())?)
    }

    /// Get an artist's tracks (public endpoint via artist/get?extra=tracks)
    pub async fn get_artist_tracks(&self, artist_id: u64, limit: u32, offset: u32) -> Result<TracksContainer> {
        let url = endpoints::build_url(paths::ARTIST_GET);
        let locale = self.locale().await;

        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[
                ("artist_id", artist_id.to_string()),
                ("extra", "tracks".to_string()),
                ("lang", locale),
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ])
            .send()
            .await?;
        log::debug!("[API] get_artist_tracks({}) status={}", artist_id, http_response.status());
        let response: Value = http_response.json().await?;

        let tracks = response
            .get("tracks")
            .ok_or_else(|| ApiError::ApiResponse("No tracks in artist response".to_string()))?;

        Ok(serde_json::from_value(tracks.clone())?)
    }

    // === Get endpoints ===

    /// Get album by ID
    pub async fn get_album(&self, album_id: &str) -> Result<Album> {
        let url = endpoints::build_url(paths::ALBUM_GET);
        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[("album_id", album_id)])
            .send()
            .await?;
        let status = http_response.status();
        log::debug!("[API] get_album({}) status={}", album_id, status);

        if status == StatusCode::NOT_FOUND {
            log::warn!("[API] get_album({}) returned 404 — album not found", album_id);
            return Err(ApiError::ApiResponse(format!("Album {} not found (404)", album_id)));
        }
        if !status.is_success() {
            log::error!("[API] get_album({}) unexpected status={}", album_id, status);
            return Err(ApiError::ApiResponse(format!("get_album({}) status {}", album_id, status)));
        }

        let response: Value = http_response.json().await?;
        Ok(serde_json::from_value(response)?)
    }

    /// Get featured albums by type (new-releases, press-awards, most-streamed)
    pub async fn get_featured_albums(
        &self,
        featured_type: &str,
        limit: u32,
        offset: u32,
        genre_id: Option<u64>,
    ) -> Result<SearchResultsPage<Album>> {
        let url = endpoints::build_url(paths::ALBUM_GET_FEATURED);
        let mut query = vec![
            ("type".to_string(), featured_type.to_string()),
            ("limit".to_string(), limit.to_string()),
            ("offset".to_string(), offset.to_string()),
        ];

        if let Some(gid) = genre_id {
            query.push(("genre_id".to_string(), gid.to_string()));
        }

        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&query)
            .send()
            .await?;
        log::debug!("[API] get_featured_albums({}) status={}", featured_type, http_response.status());
        let response: Value = http_response.json().await?;

        let albums = response
            .get("albums")
            .ok_or_else(|| ApiError::ApiResponse("No albums in response".to_string()))?;

        Ok(serde_json::from_value(albums.clone())?)
    }

    /// Get list of genres
    pub async fn get_genres(&self, parent_id: Option<u64>) -> Result<Vec<GenreInfo>> {
        let url = endpoints::build_url(paths::GENRE_LIST);
        // Force English for consistent genre names across all user regions
        let mut query: Vec<(&str, String)> = vec![
            ("lang", "en".to_string()),
        ];

        if let Some(pid) = parent_id {
            query.push(("parent_id", pid.to_string()));
        }

        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&query)
            .send()
            .await?;
        log::debug!("[API] get_genres(parent={:?}) status={}", parent_id, http_response.status());
        let response: Value = http_response.json().await?;

        let genres = response
            .get("genres")
            .and_then(|g| g.get("items"))
            .ok_or_else(|| ApiError::ApiResponse("No genres in response".to_string()))?;

        Ok(serde_json::from_value(genres.clone())?)
    }

    /// Get discover index (home page content: playlists, ideal discography, etc.)
    pub async fn get_discover_index(
        &self,
        genre_ids: Option<Vec<u64>>,
    ) -> Result<DiscoverResponse> {
        let url = endpoints::build_url(paths::DISCOVER_INDEX);
        let mut query: Vec<(&str, String)> = vec![];

        // Add genre_ids as comma-separated list if provided
        if let Some(gids) = genre_ids {
            if !gids.is_empty() {
                let ids_str = gids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
                query.push(("genre_ids", ids_str));
            }
        }

        let http_response = self
            .http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&query)
            .send()
            .await?;
        log::info!("[API] get_discover_index status={}", http_response.status());
        let response: Value = http_response.json().await?;

        // Debug: log the response structure
        if let Some(obj) = response.as_object() {
            log::info!("Discover API response keys: {:?}", obj.keys().collect::<Vec<_>>());
            if let Some(err) = obj.get("message") {
                log::error!("Discover API error: {:?}", err);
            }
            if let Some(code) = obj.get("code") {
                log::error!("Discover API error code: {:?}", code);
            }
        }

        Ok(serde_json::from_value(response)?)
    }

    /// Get discover albums from a specific browse endpoint (newReleases, idealDiscography, mostStreamed)
    pub async fn get_discover_albums(
        &self,
        endpoint: &str,
        genre_ids: Option<Vec<u64>>,
        offset: u32,
        limit: u32,
    ) -> Result<DiscoverData<DiscoverAlbum>> {
        let url = endpoints::build_url(endpoint);
        let mut query: Vec<(&str, String)> = vec![];

        if let Some(gids) = genre_ids {
            if !gids.is_empty() {
                let ids_str = gids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
                query.push(("genre_ids", ids_str));
            }
        }

        query.push(("offset", offset.to_string()));
        query.push(("limit", limit.to_string()));

        let http_response = self
            .http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&query)
            .send()
            .await?;
        log::info!("[API] get_discover_albums({}) status={}", endpoint, http_response.status());
        let response: serde_json::Value = http_response.json().await?;

        Ok(serde_json::from_value(response)?)
    }

    /// Get discover playlists with optional tag and genre filters
    /// Example: tags=label, genre_ids=112,119
    pub async fn get_discover_playlists(
        &self,
        tag: Option<String>,
        genre_ids: Option<Vec<u64>>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<DiscoverPlaylistsResponse> {
        let url = endpoints::build_url(paths::DISCOVER_PLAYLISTS);
        let mut query: Vec<(&str, String)> = vec![];

        // Add tag filter if provided (e.g., "label", "partner")
        if let Some(ref t) = tag {
            query.push(("tags", t.clone()));
        }

        // Add genre_ids as comma-separated list if provided
        if let Some(gids) = genre_ids {
            if !gids.is_empty() {
                let ids_str = gids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
                query.push(("genre_ids", ids_str));
            }
        }

        // Add limit (default 20)
        let lim = limit.unwrap_or(20);
        query.push(("limit", lim.to_string()));

        // Add offset (default 0)
        let off = offset.unwrap_or(0);
        query.push(("offset", off.to_string()));

        log::debug!("[API] get_discover_playlists URL: {} query: {:?}", url, query);

        // First get raw JSON to debug structure
        let raw_response: serde_json::Value = self
            .http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&query)
            .send()
            .await?
            .json()
            .await?;

        log::debug!("[API] get_discover_playlists raw response keys: {:?}", raw_response.as_object().map(|o| o.keys().collect::<Vec<_>>()));

        // Try to parse as expected structure
        let response: DiscoverPlaylistsResponse = serde_json::from_value(raw_response.clone())
            .map_err(|e| {
                log::error!("[API] Failed to parse discover playlists response: {}", e);
                log::error!("[API] Raw response: {}", serde_json::to_string_pretty(&raw_response).unwrap_or_default());
                e
            })?;

        log::debug!("[API] get_discover_playlists response: {} playlists", response.items.len());

        Ok(response)
    }

    /// Get playlist tags with localized names
    pub async fn get_playlist_tags(&self) -> Result<Vec<PlaylistTag>> {
        let url = endpoints::build_url(paths::PLAYLIST_GET_TAGS);

        let http_response = self
            .http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .send()
            .await?;
        log::info!("[API] get_playlist_tags status={}", http_response.status());

        let raw: PlaylistTagsResponse = http_response.json().await?;

        // Get current locale (e.g., "en", "es", "fr", "de")
        let locale = self.locale().await;
        let lang = locale.split('-').next().unwrap_or("en");

        // Convert raw tags to PlaylistTag with localized name
        let tags: Vec<PlaylistTag> = raw
            .tags
            .into_iter()
            .filter(|tag| tag.is_discover.as_deref() == Some("true"))
            .filter_map(|tag| {
                // Parse name_json to get localized name
                let name_map: serde_json::Value =
                    serde_json::from_str(&tag.name_json).ok()?;
                let name = name_map
                    .get(lang)
                    .or_else(|| name_map.get("en"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                let id = tag.featured_tag_id
                    .as_ref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                Some(PlaylistTag { id, slug: tag.slug, name })
            })
            .collect();

        log::debug!("[API] get_playlist_tags: {} tags", tags.len());
        Ok(tags)
    }

    /// Get track by ID
    pub async fn get_track(&self, track_id: u64) -> Result<Track> {
        let url = endpoints::build_url(paths::TRACK_GET);
        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[("track_id", track_id.to_string())])
            .send()
            .await?;
        let status = http_response.status();
        log::debug!("[API] get_track({}) status={}", track_id, status);

        if status == StatusCode::NOT_FOUND {
            log::warn!("[API] get_track({}) returned 404 — track no longer available", track_id);
            return Err(ApiError::TrackUnavailable(track_id));
        }
        if !status.is_success() {
            log::error!("[API] get_track({}) unexpected status={}", track_id, status);
            return Err(ApiError::ApiResponse(format!("get_track({}) status {}", track_id, status)));
        }

        let response: Value = http_response.json().await?;
        Ok(serde_json::from_value(response)?)
    }

    /// Get artist by ID (basic info only - no albums, faster response)
    pub async fn get_artist_basic(&self, artist_id: u64) -> Result<Artist> {
        let url = endpoints::build_url(paths::ARTIST_GET);
        let locale = self.locale().await;
        let query = vec![
            ("artist_id", artist_id.to_string()),
            ("lang", locale),
            // No "extra" parameter = only basic info (id, name, image)
        ];

        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&query)
            .send()
            .await?;
        log::debug!("[API] get_artist_basic({}) status={}", artist_id, http_response.status());
        let response: Value = http_response.json().await?;

        Ok(serde_json::from_value(response)?)
    }

    /// Get artist by ID
    pub async fn get_artist(
        &self,
        artist_id: u64,
        with_albums: bool,
    ) -> Result<Artist> {
        self.get_artist_with_pagination(artist_id, with_albums, None, None).await
    }

    /// Get artist detail by ID with albums, playlists, and appears-on tracks
    pub async fn get_artist_detail(
        &self,
        artist_id: u64,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Artist> {
        let url = endpoints::build_url(paths::ARTIST_GET);
        let locale = self.locale().await;
        let mut query = vec![
            ("artist_id", artist_id.to_string()),
            ("extra", "albums,tracks_appears_on,playlists".to_string()),
            ("lang", locale),
        ];
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }
        if let Some(o) = offset {
            query.push(("offset", o.to_string()));
        }

        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&query)
            .send()
            .await?;
        log::debug!("[API] get_artist_detail({}) status={}", artist_id, http_response.status());
        let response: Value = http_response.json().await?;

        Ok(serde_json::from_value(response)?)
    }

    /// Get artist by ID with album pagination
    pub async fn get_artist_with_pagination(
        &self,
        artist_id: u64,
        with_albums: bool,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Artist> {
        self.get_artist_with_pagination_and_locale(artist_id, with_albums, limit, offset, None).await
    }

    /// Get artist by ID with album pagination and optional locale override
    /// Use locale_override to force a specific language (e.g., "en" for genre checking)
    pub async fn get_artist_with_pagination_and_locale(
        &self,
        artist_id: u64,
        with_albums: bool,
        limit: Option<u32>,
        offset: Option<u32>,
        locale_override: Option<&str>,
    ) -> Result<Artist> {
        let url = endpoints::build_url(paths::ARTIST_GET);
        let locale = match locale_override {
            Some(l) => l.to_string(),
            None => self.locale().await,
        };
        let mut query = vec![
            ("artist_id", artist_id.to_string()),
            ("lang", locale),
        ];
        if with_albums {
            query.push(("extra", "albums".to_string()));
        }
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }
        if let Some(o) = offset {
            query.push(("offset", o.to_string()));
        }

        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&query)
            .send()
            .await?;
        log::debug!("[API] get_artist({}, albums={}) status={}", artist_id, with_albums, http_response.status());
        let response: Value = http_response.json().await?;

        Ok(serde_json::from_value(response)?)
    }

    /// Get playlist metadata + ordered track IDs (lightweight, no full Track objects).
    /// Uses `playlist/get?extra=track_ids` which returns the playlist with a flat
    /// array of track IDs instead of nested Track objects.
    pub async fn get_playlist_track_ids(&self, playlist_id: u64) -> Result<PlaylistWithTrackIds> {
        let url = endpoints::build_url(paths::PLAYLIST_GET);
        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[
                ("playlist_id", playlist_id.to_string()),
                ("extra", "track_ids".to_string()),
            ])
            .send()
            .await?;
        log::debug!("[API] get_playlist_track_ids({}) status={}", playlist_id, http_response.status());
        let response: Value = http_response.json().await?;
        let result: PlaylistWithTrackIds = serde_json::from_value(response)?;
        log::info!(
            "[API] get_playlist_track_ids({}) — {} track IDs",
            playlist_id,
            result.track_ids.len()
        );
        Ok(result)
    }

    /// Fetch full Track objects for a batch of track IDs (max 50 per call).
    /// Uses the `track/getList` endpoint.
    ///
    /// Tries multiple API call strategies:
    /// POST to track/getList with JSON body {"tracks_id": [...]}
    /// Returns full Track objects for the given IDs (max 50 per call).
    pub async fn get_tracks_batch(&self, track_ids: &[u64]) -> Result<Vec<Track>> {
        let url = endpoints::build_url(paths::TRACK_GET_LIST);
        let headers = self.api_headers().await?;

        let body = serde_json::json!({ "tracks_id": track_ids });
        log::debug!("[API] get_tracks_batch POST ({} IDs)", track_ids.len());

        let http_response = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        let status = http_response.status();
        log::debug!("[API] get_tracks_batch POST status={}", status);

        let value: Value = http_response.json().await?;

        // Response: { "tracks": { "total": N, "items": [...] } }
        let items = value
            .get("tracks")
            .and_then(|t| t.get("items"))
            .ok_or_else(|| {
                let preview = serde_json::to_string(&value)
                    .unwrap_or_default()
                    .chars()
                    .take(500)
                    .collect::<String>();
                ApiError::ApiResponse(format!(
                    "Missing tracks.items in getList response: {}",
                    preview
                ))
            })?;

        let tracks: Vec<Track> = serde_json::from_value(items.clone())?;
        log::debug!("[API] get_tracks_batch returned {} tracks", tracks.len());
        Ok(tracks)
    }

    /// Get playlist by ID (paginates automatically to fetch all tracks)
    ///
    /// After the first page, remaining pages are fetched concurrently
    /// since we know the total track count from the first response.
    pub async fn get_playlist(&self, playlist_id: u64) -> Result<Playlist> {
        let url = endpoints::build_url(paths::PLAYLIST_GET);
        const PAGE_SIZE: u32 = 500;

        let start = std::time::Instant::now();

        // First page — gives us metadata + total track count
        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[
                ("playlist_id", playlist_id.to_string()),
                ("limit", PAGE_SIZE.to_string()),
                ("offset", "0".to_string()),
                ("extra", "tracks".to_string()),
            ])
            .send()
            .await?;
        log::debug!("[API] get_playlist({}) status={}", playlist_id, http_response.status());
        let response: Value = http_response.json().await?;
        let mut playlist: Playlist = serde_json::from_value(response)?;

        // Fetch remaining pages concurrently
        if let Some(ref mut container) = playlist.tracks {
            let total = container.total;
            let fetched = container.items.len() as u32;

            if fetched < total {
                // Build all remaining page offsets
                let offsets: Vec<u32> = (fetched..total).step_by(PAGE_SIZE as usize).collect();
                log::debug!(
                    "[API] get_playlist({}) fetching {} remaining pages concurrently ({}/{})",
                    playlist_id, offsets.len(), fetched, total
                );

                // Prepare headers once for all concurrent requests
                let headers = self.api_headers().await?;

                // Launch all page requests concurrently
                let futures: Vec<_> = offsets.iter().map(|&offset| {
                    let http = &self.http;
                    let url = &url;
                    let headers = headers.clone();
                    let pid = playlist_id.to_string();
                    let limit = PAGE_SIZE.to_string();
                    let offset_str = offset.to_string();
                    async move {
                        let resp = http
                            .get(url)
                            .headers(headers)
                            .query(&[
                                ("playlist_id", pid.as_str()),
                                ("limit", limit.as_str()),
                                ("offset", offset_str.as_str()),
                                ("extra", "tracks"),
                            ])
                            .send()
                            .await?;
                        let value: Value = resp.json().await?;
                        let page: Playlist = serde_json::from_value(value)?;
                        Ok::<_, anyhow::Error>((offset, page))
                    }
                }).collect();

                let results = futures_util::future::join_all(futures).await;

                // Collect results sorted by offset to maintain track order
                let mut pages: Vec<(u32, Playlist)> = Vec::new();
                for result in results {
                    match result {
                        Ok(page) => pages.push(page),
                        Err(e) => {
                            log::warn!("[API] get_playlist({}) page fetch failed: {}", playlist_id, e);
                            // Continue with what we have
                        }
                    }
                }
                pages.sort_by_key(|(offset, _)| *offset);

                // Append tracks in order
                for (_, page_playlist) in pages {
                    if let Some(page_tracks) = page_playlist.tracks {
                        if !page_tracks.items.is_empty() {
                            container.items.extend(page_tracks.items);
                        }
                    }
                }
            }
        }

        let elapsed = start.elapsed();
        log::info!(
            "[API] get_playlist({}) complete: {} tracks in {:.2}s",
            playlist_id,
            playlist.tracks.as_ref().map(|t| t.items.len()).unwrap_or(0),
            elapsed.as_secs_f64()
        );

        Ok(playlist)
    }

    /// Get label page (aggregated: top tracks, releases, playlists, artists)
    pub async fn get_label_page(&self, label_id: u64) -> Result<LabelPageData> {
        let url = endpoints::build_url(paths::LABEL_PAGE);

        log::debug!("[API] get_label_page({})", label_id);
        let response: serde_json::Value = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[("label_id", label_id.to_string())])
            .send()
            .await?
            .json()
            .await?;

        Ok(serde_json::from_value(response)?)
    }

    /// Get label explore (discover more labels)
    pub async fn get_label_explore(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<LabelExploreResponse> {
        let url = endpoints::build_url(paths::LABEL_EXPLORE);

        log::debug!("[API] get_label_explore(limit={}, offset={})", limit, offset);
        let response: serde_json::Value = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ])
            .send()
            .await?
            .json()
            .await?;

        Ok(serde_json::from_value(response)?)
    }

    /// Get label by ID with albums
    pub async fn get_label(&self, label_id: u64, limit: u32, offset: u32) -> Result<LabelDetail> {
        let url = endpoints::build_url(paths::LABEL_GET);
        let locale = self.locale().await;

        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[
                ("label_id", label_id.to_string()),
                ("extra", "albums".to_string()),
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
                ("lang", locale),
            ])
            .send()
            .await?;
        log::debug!("[API] get_label({}) status={}", label_id, http_response.status());
        let response: Value = http_response.json().await?;

        Ok(serde_json::from_value(response)?)
    }

    // === Authenticated endpoints ===

    /// Get stream URL for a track (requires auth + signature)
    pub async fn get_stream_url(&self, track_id: u64, quality: Quality) -> Result<StreamUrl> {
        log::info!("Getting stream URL for track {} with quality {:?}", track_id, quality);
        let url = endpoints::build_url(paths::TRACK_GET_FILE_URL);
        let timestamp = get_timestamp();
        log::debug!("Getting secret for signing...");
        let secret = self.secret().await?;
        log::debug!("Secret obtained, signing request...");
        let signature = sign_get_file_url(track_id, quality.id(), timestamp, &secret);

        log::debug!("Sending stream URL request...");
        let response = self
            .http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&[
                ("track_id", track_id.to_string()),
                ("format_id", quality.id().to_string()),
                ("intent", "stream".to_string()),
                ("request_ts", timestamp.to_string()),
                ("request_sig", signature),
            ])
            .send()
            .await?;

        log::info!("Stream URL response status: {}", response.status());
        match response.status() {
            StatusCode::OK => {
                let json: Value = response.json().await?;
                log::debug!("Stream URL response JSON keys: {:?}", json.as_object().map(|o| o.keys().collect::<Vec<_>>()));

                // Check for restrictions
                let restrictions: Vec<StreamRestriction> = json
                    .get("restrictions")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

// Validate that we got an actual URL (track may be unavailable)
                let url = json["url"].as_str().unwrap_or("").to_string();
                if url.is_empty() {
                    // Log the restriction codes for debugging
                    let restriction_codes: Vec<&str> = restrictions.iter().map(|r| r.code.as_str()).collect();
                    log::warn!("Stream URL missing for track {} - restrictions: {:?}", track_id, restriction_codes);
                    return Err(ApiError::TrackUnavailable(track_id));
                }

                Ok(StreamUrl {
                    url,
                    format_id: json["format_id"].as_u64().unwrap_or(0) as u32,
                    mime_type: json["mime_type"].as_str().unwrap_or("").to_string(),
                    sampling_rate: json["sampling_rate"].as_f64().unwrap_or(0.0),
                    bit_depth: json["bit_depth"].as_u64().map(|v| v as u32),
                    track_id,
                    restrictions,
                })
            }
            StatusCode::BAD_REQUEST => Err(ApiError::InvalidAppSecret),
            status => Err(ApiError::ApiResponse(format!("Unexpected status: {}", status))),
        }
    }

    /// Get stream URL with quality fallback
    pub async fn get_stream_url_with_fallback(
        &self,
        track_id: u64,
        preferred: Quality,
    ) -> Result<StreamUrl> {
        log::info!("Getting stream URL with fallback for track {}, preferred quality: {:?}", track_id, preferred);
        let qualities = Quality::fallback_order();
        let start_idx = qualities.iter().position(|q| *q == preferred).unwrap_or(0);

        let mut track_unavailable = false;

        for quality in &qualities[start_idx..] {
            log::info!("Trying quality: {:?}", quality);
            match self.get_stream_url(track_id, *quality).await {
                Ok(url) if !url.has_restrictions() => {
                    log::info!(
                        "Got stream URL for requested quality format_id={}",
                        quality.id()
                    );
                    return Ok(url);
                },
                Ok(_) => {
                    log::info!("Quality {:?} has restrictions, trying next", quality);
                    continue;
                },
                Err(ApiError::InvalidAppSecret) => {
                    log::error!("Invalid app secret");
                    return Err(ApiError::InvalidAppSecret);
                },
                Err(ApiError::TrackUnavailable(_)) => {
                    // Track is completely unavailable on Qobuz
                    track_unavailable = true;
                    continue;
                },
                Err(e) => {
                    log::warn!("Quality {:?} failed: {}, trying next", quality, e);
                    continue;
                },
            }
        }

        // If all quality levels reported track unavailable, return that specific error
        if track_unavailable {
            log::error!("Track {} is no longer available on Qobuz", track_id);
            return Err(ApiError::TrackUnavailable(track_id));
        }

        log::error!("No quality available for track {}", track_id);
        Err(ApiError::NoQualityAvailable)
    }

    /// Get user favorites (requires auth + signature)
    pub async fn get_favorites(&self, fav_type: &str, limit: u32, offset: u32) -> Result<Value> {
        let url = endpoints::build_url(paths::FAVORITE_GET_USER_FAVORITES);
        let timestamp = get_timestamp();
        let secret = self.secret().await?;
        let signature = sign_get_favorites(timestamp, &secret);

        let http_response = self
            .http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&[
                ("type", fav_type),
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
                ("request_ts", &timestamp.to_string()),
                ("request_sig", &signature),
            ])
            .send()
            .await?;
        log::debug!("[API] get_favorites({}) status={}", fav_type, http_response.status());
        let response: Value = http_response.json().await?;

        Ok(response)
    }

    /// Get user's playlists
    pub async fn get_user_playlists(&self) -> Result<Vec<Playlist>> {
        let url = endpoints::build_url(paths::PLAYLIST_GET_USER_PLAYLISTS);
        let http_response = self
            .http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .send()
            .await?;
        log::debug!("[API] get_user_playlists status={}", http_response.status());
        let response: Value = http_response.json().await?;

        let playlists = response
            .get("playlists")
            .and_then(|p| p.get("items"))
            .ok_or_else(|| ApiError::ApiResponse("No playlists in response".to_string()))?;

        Ok(serde_json::from_value(playlists.clone())?)
    }

    /// Search playlists
    pub async fn search_playlists(&self, query: &str, limit: u32, offset: u32) -> Result<SearchResultsPage<Playlist>> {
        let url = endpoints::build_url(paths::PLAYLIST_SEARCH);
        let http_response = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&[
                ("query", query),
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
            ])
            .send()
            .await?;
        log::debug!("[API] search_playlists status={}", http_response.status());
        let response: Value = http_response.json().await?;

        let playlists = response
            .get("playlists")
            .ok_or_else(|| ApiError::ApiResponse("No playlists in response".to_string()))?;

        Ok(serde_json::from_value(playlists.clone())?)
    }

    /// Create a new playlist
    pub async fn create_playlist(&self, name: &str, description: Option<&str>, is_public: bool) -> Result<Playlist> {
        let url = endpoints::build_url(paths::PLAYLIST_CREATE);

        let mut params = vec![
            ("name", name.to_string()),
            ("is_public", is_public.to_string()),
        ];
        if let Some(desc) = description {
            params.push(("description", desc.to_string()));
        }

        let response: Playlist = self
            .http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&params)
            .send()
            .await?
            .json()
            .await?;

        Ok(response)
    }

    /// Delete a playlist
    pub async fn delete_playlist(&self, playlist_id: u64) -> Result<()> {
        let url = endpoints::build_url(paths::PLAYLIST_DELETE);

        self.http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&[("playlist_id", playlist_id.to_string())])
            .send()
            .await?;

        Ok(())
    }

    /// Add tracks to a playlist
    pub async fn add_tracks_to_playlist(&self, playlist_id: u64, track_ids: &[u64]) -> Result<()> {
        let url = endpoints::build_url(paths::PLAYLIST_ADD_TRACKS);
        let track_ids_str = track_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");

        self.http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&[
                ("playlist_id", playlist_id.to_string()),
                ("track_ids", track_ids_str),
            ])
            .send()
            .await?;

        Ok(())
    }

    /// Remove tracks from a playlist
    pub async fn remove_tracks_from_playlist(&self, playlist_id: u64, playlist_track_ids: &[u64]) -> Result<()> {
        let url = endpoints::build_url(paths::PLAYLIST_DELETE_TRACKS);
        let track_ids_str = playlist_track_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");

        self.http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&[
                ("playlist_id", playlist_id.to_string()),
                ("playlist_track_ids", track_ids_str),
            ])
            .send()
            .await?;

        Ok(())
    }

    /// Update playlist metadata
    pub async fn update_playlist(&self, playlist_id: u64, name: Option<&str>, description: Option<&str>, is_public: Option<bool>) -> Result<Playlist> {
        let url = endpoints::build_url(paths::PLAYLIST_UPDATE);

        let mut params = vec![("playlist_id", playlist_id.to_string())];
        if let Some(n) = name {
            params.push(("name", n.to_string()));
        }
        if let Some(d) = description {
            params.push(("description", d.to_string()));
        }
        if let Some(p) = is_public {
            params.push(("is_public", p.to_string()));
        }

        let response: Playlist = self
            .http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&params)
            .send()
            .await?
            .json()
            .await?;

        Ok(response)
    }

    /// Add item to favorites
    pub async fn add_favorite(&self, fav_type: &str, item_id: &str) -> Result<()> {
        let url = endpoints::build_url(paths::FAVORITE_CREATE);
        let type_key = format!("{}_ids", fav_type); // album_ids, track_ids, artist_ids

        let response = self
            .http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&[(&type_key, item_id)])
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ApiError::ApiResponse(format!("Failed to add favorite: {}", response.status())))
        }
    }

    /// Remove item from favorites
    pub async fn remove_favorite(&self, fav_type: &str, item_id: &str) -> Result<()> {
        let url = endpoints::build_url(paths::FAVORITE_DELETE);
        let type_key = format!("{}_ids", fav_type);

        let response = self
            .http
            .get(&url)
            .headers(self.authenticated_headers().await?)
            .query(&[(&type_key, item_id)])
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ApiError::ApiResponse(format!("Failed to remove favorite: {}", response.status())))
        }
    }

    // ============ Artist Page Endpoints ============

    /// Get artist page (aggregated: bio, top tracks, releases, similar, playlists)
    pub async fn get_artist_page(
        &self,
        artist_id: u64,
        sort: Option<&str>,
    ) -> Result<PageArtistResponse> {
        let url = endpoints::build_url(paths::ARTIST_PAGE);
        let mut query = vec![
            ("artist_id", artist_id.to_string()),
        ];
        if let Some(s) = sort {
            query.push(("sort", s.to_string()));
        }

        log::debug!("[API] get_artist_page({}) sort={:?}", artist_id, sort);
        let response: serde_json::Value = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&query)
            .send()
            .await?
            .json()
            .await?;

        Ok(serde_json::from_value(response)?)
    }

    /// Get artist releases grid (paginated by release_type)
    pub async fn get_releases_grid(
        &self,
        artist_id: u64,
        release_type: &str,
        limit: u32,
        offset: u32,
        sort: Option<&str>,
    ) -> Result<ReleasesGridResponse> {
        let url = endpoints::build_url(paths::ARTIST_RELEASES_GRID);
        let mut query = vec![
            ("artist_id", artist_id.to_string()),
            ("release_type", release_type.to_string()),
            ("limit", limit.to_string()),
            ("offset", offset.to_string()),
        ];
        if let Some(s) = sort {
            query.push(("sort", s.to_string()));
        }

        log::debug!("[API] get_releases_grid({}) type={} limit={} offset={}", artist_id, release_type, limit, offset);
        let response: serde_json::Value = self
            .http
            .get(&url)
            .headers(self.api_headers().await?)
            .query(&query)
            .send()
            .await?
            .json()
            .await?;

        Ok(serde_json::from_value(response)?)
    }
}

impl Default for QobuzClient {
    fn default() -> Self {
        Self::new().expect("Failed to create client")
    }
}
