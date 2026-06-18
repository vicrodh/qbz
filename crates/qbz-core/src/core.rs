//! QBZ Core Orchestrator
//!
//! The main orchestrator that connects all QBZ subsystems and provides
//! a unified API for frontends.

use std::sync::Arc;
use tokio::sync::RwLock;

use qbz_models::{
    AssetOrigin, ExternalStreamAsset, StreamQualityInfo,
    Album, Artist, ArtistAlbums, CoreEvent, DiscoverAlbum, DiscoverData, DiscoverPlaylistsResponse,
    DiscoverResponse, FrontendAdapter, GenreInfo, LabelExploreResponse, LabelGetListResponse,
    LabelListPage, LabelPageData, LabelStoryResponse, PageArtistResponse,
    MostPopularItem, Playlist, PlaylistDuplicateResult, PlaylistTag, Quality, QueueState,
    QueueTrack, ReleasesGridResponse,
    RepeatMode, SearchAllResults, SearchResultsPage, StreamUrl, Track, TrackToAnalyse,
    TracksContainer, UserSession,
};
use qbz_integrations::musicbrainz::cache::MusicBrainzCache;
use qbz_integrations::musicbrainz::genre::{extract_affinity_seeds, genre_summary, is_broad_genre};
use qbz_integrations::musicbrainz::location::compute_affinity_score;
use qbz_integrations::musicbrainz::{
    location, AffinitySeeds, AlbumAppearance, ArtistMetadata, ArtistRelationships,
    DiscoveryArtist, DiscoveryResponse, LocationCandidate, LocationDiscoveryResponse,
    MusicBrainzClient, MusicianAppearances, MusicianConfidence, Period, RelatedArtist,
    ResolvedArtist, ResolvedMusician, Tag,
};
use qbz_player::{PlaybackState, Player, QueueManager};
use qbz_qobuz::QobuzClient;

use crate::error::CoreError;

/// Set of blacklisted artist ids. Empty until the blacklist module is
/// migrated out of src-tauri (roadmap task #9).
pub type BlacklistFilter = std::collections::HashSet<u64>;

fn parse_page<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    key: &str,
) -> SearchResultsPage<T> {
    value
        .get(key)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(SearchResultsPage {
            items: Vec::new(),
            total: 0,
            offset: 0,
            limit: 0,
        })
}

/// Pick the first `most_popular` entry that survives the blacklist.
fn pick_most_popular(
    value: &serde_json::Value,
    blacklist: &BlacklistFilter,
) -> Option<MostPopularItem> {
    let items = value.get("most_popular")?.get("items")?.as_array()?;
    for entry in items {
        let Some(kind) = entry.get("type").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(content) = entry.get("content") else {
            continue;
        };
        match kind {
            "artists" => {
                if let Ok(a) = serde_json::from_value::<Artist>(content.clone()) {
                    if !blacklist.contains(&a.id) {
                        return Some(MostPopularItem::Artists(a));
                    }
                }
            }
            "albums" => {
                if let Ok(al) = serde_json::from_value::<Album>(content.clone()) {
                    if !blacklist.contains(&al.artist.id) {
                        return Some(MostPopularItem::Albums(al));
                    }
                }
            }
            "tracks" => {
                if let Ok(t) = serde_json::from_value::<Track>(content.clone()) {
                    let blocked = t
                        .performer
                        .as_ref()
                        .map_or(false, |p| blacklist.contains(&p.id));
                    if !blocked {
                        return Some(MostPopularItem::Tracks(t));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// D-FEAT: returns true if the album should be hidden by the blacklist.
///
/// Extends the historical Tauri rule (which blocked only the PRIMARY
/// `album.artist`) to also block when ANY contributor in `album.artists[]`
/// (featured artists included) is blacklisted. Centralizing this here keeps
/// every call site (search, discovery, queue-build) on ONE consistent rule.
///
/// Fail-open: an empty filter never blocks; an album with no matching id is
/// kept.
pub fn album_blacklisted(album: &Album, bl: &BlacklistFilter) -> bool {
    if bl.is_empty() {
        return false;
    }
    if bl.contains(&album.artist.id) {
        return true;
    }
    album
        .artists
        .as_ref()
        .is_some_and(|v| v.iter().any(|a| bl.contains(&a.id)))
}

/// D-FEAT: returns true if the track should be hidden by the blacklist.
///
/// Blocks on the track's structured `performer` OR `composer` id. Extends the
/// historical Tauri rule (performer only) to also cover the composer.
///
/// Fail-open: an empty filter never blocks; a track with neither a performer
/// nor a composer id is kept (no id to match against).
///
/// D-FEAT limitation: the model exposes no structured per-track *featured
/// performer id* — only `performer`, `composer`, and a free-text `performers`
/// string. We deliberately do NOT name-match the free-text string; this rule
/// is strictly id-based.
pub fn track_blacklisted(track: &Track, bl: &BlacklistFilter) -> bool {
    if bl.is_empty() {
        return false;
    }
    track
        .performer
        .as_ref()
        .is_some_and(|a| bl.contains(&a.id))
        || track
            .composer
            .as_ref()
            .is_some_and(|a| bl.contains(&a.id))
}

/// D-FEAT: returns true if a discover-shaped album should be hidden.
///
/// Discover albums expose only a flat `artists[]` vec (no separate primary
/// `artist`), so any matching contributor id — primary or featured — blocks
/// the album. Fail-open: an empty filter never blocks.
pub fn discover_album_blacklisted(album: &DiscoverAlbum, bl: &BlacklistFilter) -> bool {
    if bl.is_empty() {
        return false;
    }
    album.artists.iter().any(|a| bl.contains(&a.id))
}

/// Parse a `catalog_search` JSON payload into typed category pages,
/// dropping any item whose artist id is blacklisted and adjusting totals.
pub(crate) fn parse_search_all(
    value: &serde_json::Value,
    blacklist: &BlacklistFilter,
) -> SearchAllResults {
    let mut albums = parse_page::<Album>(value, "albums");
    let mut tracks = parse_page::<Track>(value, "tracks");
    let mut artists = parse_page::<Artist>(value, "artists");
    let playlists = parse_page::<Playlist>(value, "playlists");

    let before = artists.items.len();
    artists.items.retain(|a| !blacklist.contains(&a.id));
    artists.total = artists
        .total
        .saturating_sub((before - artists.items.len()) as u32);

    let before = albums.items.len();
    albums.items.retain(|al| !album_blacklisted(al, blacklist));
    albums.total = albums
        .total
        .saturating_sub((before - albums.items.len()) as u32);

    let before = tracks.items.len();
    tracks
        .items
        .retain(|track| !track_blacklisted(track, blacklist));
    tracks.total = tracks
        .total
        .saturating_sub((before - tracks.items.len()) as u32);

    SearchAllResults {
        albums,
        tracks,
        artists,
        playlists,
        most_popular: pick_most_popular(value, blacklist),
    }
}

/// Core orchestrator for QBZ
///
/// This is the main entry point for any frontend (Tauri, Slint, Iced, CLI, etc.)
/// It provides a unified API and emits events through the FrontendAdapter.
pub struct QbzCore<A: FrontendAdapter> {
    /// Frontend adapter for event emission
    adapter: Arc<A>,
    /// Qobuz API client
    client: Arc<RwLock<Option<QobuzClient>>>,
    /// Queue manager
    queue: Arc<RwLock<QueueManager>>,
    /// Audio player
    player: Arc<Player>,
    /// MusicBrainz client (always present; enable/disable toggle lives inside)
    musicbrainz: Arc<MusicBrainzClient>,
    /// Persistent MB cache. Opened by the frontend (which owns the
    /// data-dir path) via `set_musicbrainz_cache`. Methods read the
    /// cache before hitting the network and persist on miss.
    musicbrainz_cache: Arc<std::sync::Mutex<Option<MusicBrainzCache>>>,
    /// Whether the core is initialized
    initialized: Arc<RwLock<bool>>,
    /// D8 guard: true when the current queue was built from an OFFLINE-ONLY
    /// local playlist — such a queue must never be pushed to the Qobuz
    /// Connect cloud. Cleared by every queue REPLACEMENT (`set_queue` /
    /// `set_queue_with_order` / `clear_queue`); append-style ops preserve it.
    /// Set explicitly by the frontend's local-playlist play path right after
    /// its `set_queue`.
    queue_offline_only: Arc<std::sync::atomic::AtomicBool>,
}

impl<A: FrontendAdapter + Send + Sync + 'static> QbzCore<A> {
    /// Create a new QbzCore instance with the given frontend adapter and player
    ///
    /// The Player must be created by the frontend with appropriate audio settings.
    /// QbzCore orchestrates playback through this player.
    pub fn new(adapter: A, player: Player) -> Self {
        Self {
            adapter: Arc::new(adapter),
            client: Arc::new(RwLock::new(None)),
            queue: Arc::new(RwLock::new(QueueManager::new())),
            player: Arc::new(player),
            musicbrainz: Arc::new(MusicBrainzClient::new()),
            musicbrainz_cache: Arc::new(std::sync::Mutex::new(None)),
            initialized: Arc::new(RwLock::new(false)),
            queue_offline_only: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Mark (or unmark) the current queue as built from an OFFLINE-ONLY local
    /// playlist (D8). Call right after the `set_queue` that loaded it.
    pub fn set_queue_offline_only(&self, on: bool) {
        self.queue_offline_only
            .store(on, std::sync::atomic::Ordering::Relaxed);
    }

    /// True when the current queue originates from an offline-only local
    /// playlist — QConnect must skip its cloud queue push.
    pub fn queue_is_offline_only(&self) -> bool {
        self.queue_offline_only
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Install a MusicBrainz cache. The frontend owns the data path
    /// and opens the cache; QbzCore just stores the handle and uses
    /// it transparently in `musicbrainz_get_artist_metadata` and
    /// `musicbrainz_get_artist_relationships`.
    pub fn set_musicbrainz_cache(&self, cache: MusicBrainzCache) {
        if let Ok(mut guard) = self.musicbrainz_cache.lock() {
            *guard = Some(cache);
        }
    }

    /// Initialize the core
    ///
    /// This should be called once at startup to set up all subsystems.
    /// Best-effort: if bundle token extraction fails (e.g. no network), the
    /// core still finishes initialization so queue manager and player remain
    /// usable for offline/local playback. API calls will then return
    /// `CoreError::NotInitialized` until the client is rebuilt with tokens.
    pub async fn init(&self) -> Result<(), CoreError> {
        let mut initialized = self.initialized.write().await;
        if *initialized {
            return Ok(());
        }

        let client = QobuzClient::new().map_err(|e| CoreError::Internal(e.to_string()))?;

        match client.init().await {
            Ok(_) => {
                *self.client.write().await = Some(client);
                log::info!("QbzCore initialized with bundle tokens");
            }
            Err(e) => {
                log::warn!(
                    "QbzCore: bundle token extraction failed ({}). Starting in offline-tolerant mode; API calls will be unavailable until next online start.",
                    e
                );
            }
        }

        *initialized = true;
        Ok(())
    }

    /// Whether the Qobuz API client is initialized (bundle tokens extracted).
    /// Returns false when the core is running in offline-tolerant mode after
    /// a failed bundle extraction at startup.
    pub async fn is_api_initialized(&self) -> bool {
        self.client.read().await.is_some()
    }

    /// Best-effort attempt to rebuild the Qobuz API client. Useful when the
    /// initial `init()` ran offline and the host has since regained network.
    /// No-op when the client is already initialized.
    pub async fn try_init_api(&self) -> Result<(), CoreError> {
        {
            let guard = self.client.read().await;
            if guard.is_some() {
                return Ok(());
            }
        }

        let client = QobuzClient::new().map_err(|e| CoreError::Internal(e.to_string()))?;
        client
            .init()
            .await
            .map_err(|e| CoreError::Internal(format!("Failed to extract bundle tokens: {}", e)))?;
        *self.client.write().await = Some(client);
        log::info!("QbzCore: API client initialized lazily");
        Ok(())
    }

    /// Check if a user session exists
    pub async fn has_session(&self) -> bool {
        let client = self.client.read().await;
        if let Some(c) = client.as_ref() {
            c.is_logged_in().await
        } else {
            false
        }
    }

    /// Login with email and password
    pub async fn login(&self, email: &str, password: &str) -> Result<UserSession, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        match client.login(email, password).await {
            Ok(session) => {
                self.emit(CoreEvent::LoggedIn {
                    session: session.clone(),
                })
                .await;
                Ok(session)
            }
            Err(e) => {
                self.emit(CoreEvent::Error {
                    code: "AUTH_FAILED".to_string(),
                    message: e.to_string(),
                    recoverable: true,
                })
                .await;
                Err(CoreError::AuthFailed(e.to_string()))
            }
        }
    }

    /// Restore a session from a saved OAuth user_auth_token.
    pub async fn login_with_token(&self, token: &str) -> Result<UserSession, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        match client.login_with_token(token).await {
            Ok(session) => {
                self.emit(CoreEvent::LoggedIn {
                    session: session.clone(),
                })
                .await;
                Ok(session)
            }
            Err(e) => {
                self.emit(CoreEvent::Error {
                    code: "OAUTH_TOKEN_FAILED".to_string(),
                    message: e.to_string(),
                    recoverable: true,
                })
                .await;
                // Preserve the typed ApiError: callers must distinguish an
                // explicit auth rejection (clear the saved token) from a
                // network-class failure (keep it) — stringifying here made
                // that impossible and caused the token-clearing-on-boot bug.
                Err(CoreError::Api(e))
            }
        }
    }

    /// Inject an already-authenticated session (e.g. from OAuth flow).
    /// Emits a LoggedIn event so the rest of the system knows auth state changed.
    pub async fn set_session(&self, session: UserSession) -> Result<(), CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client.set_session(session.clone()).await;
        self.emit(CoreEvent::LoggedIn { session }).await;
        Ok(())
    }

    /// Logout the current user
    pub async fn logout(&self) -> Result<(), CoreError> {
        let client = self.client.read().await;
        if let Some(c) = client.as_ref() {
            c.logout().await;
            self.emit(CoreEvent::LoggedOut).await;
        }
        Ok(())
    }

    // ==================== Queue Operations ====================

    /// Get current queue state
    pub async fn get_queue_state(&self) -> QueueState {
        let queue = self.queue.read().await;
        queue.get_state()
    }

    /// Get all queue tracks and current index (for session persistence)
    pub async fn get_all_queue_tracks(&self) -> (Vec<QueueTrack>, Option<usize>) {
        let queue = self.queue.read().await;
        queue.get_all_tracks()
    }

    /// Get the full queue state without the upcoming/history caps that
    /// `get_queue_state` applies. Used by clients that paginate the
    /// upcoming list and need the complete play history (Queue sidebar).
    pub async fn get_queue_state_full(&self) -> QueueState {
        let queue = self.queue.read().await;
        queue.get_state_full()
    }

    /// Set repeat mode
    pub async fn set_repeat_mode(&self, mode: RepeatMode) {
        let queue = self.queue.write().await;
        queue.set_repeat(mode.clone());
        self.emit(CoreEvent::RepeatModeChanged { mode }).await;
    }

    /// Set shuffle
    pub async fn set_shuffle(&self, enabled: bool) {
        let queue = self.queue.write().await;
        queue.set_shuffle(enabled);
        self.emit(CoreEvent::ShuffleChanged { enabled }).await;
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Set shuffle mode using an authoritative order.
    pub async fn set_shuffle_with_order(&self, enabled: bool, shuffle_order: Option<Vec<usize>>) {
        let queue = self.queue.write().await;
        queue.set_shuffle_with_order(enabled, shuffle_order);
        self.emit(CoreEvent::ShuffleChanged { enabled }).await;
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Toggle shuffle and return new state
    pub async fn toggle_shuffle(&self) -> bool {
        let queue = self.queue.write().await;
        let was_enabled = queue.is_shuffle();
        let new_enabled = !was_enabled;
        queue.set_shuffle(new_enabled);
        self.emit(CoreEvent::ShuffleChanged {
            enabled: new_enabled,
        })
        .await;
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        new_enabled
    }

    /// Clear the queue. `keep_current=true` preserves the now-playing track
    /// (historical behavior); `false` wipes everything including the current
    /// slot — use when nothing is actively playing and the user wants a full
    /// reset.
    pub async fn clear_queue(&self, keep_current: bool) {
        self.set_queue_offline_only(false);
        let queue = self.queue.write().await;
        queue.clear(keep_current);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Add a track to the end of the queue
    pub async fn add_track(&self, track: QueueTrack) {
        let queue = self.queue.write().await;
        queue.add_track(track);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Add multiple tracks to the queue
    pub async fn add_tracks(&self, tracks: Vec<QueueTrack>) {
        let queue = self.queue.write().await;
        queue.add_tracks(tracks);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Add a track to play next (after current)
    pub async fn add_track_next(&self, track: QueueTrack) {
        let queue = self.queue.write().await;
        queue.add_track_next(track);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Set the entire queue (replaces existing)
    pub async fn set_queue(&self, tracks: Vec<QueueTrack>, start_index: Option<usize>) {
        // Any queue replacement drops the offline-only-playlist stamp; the
        // local-playlist play path re-sets it right after when it applies.
        self.set_queue_offline_only(false);
        let queue = self.queue.write().await;
        queue.set_queue(tracks, start_index);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Replace queue contents and playback order atomically.
    pub async fn set_queue_with_order(
        &self,
        tracks: Vec<QueueTrack>,
        start_index: Option<usize>,
        shuffle_enabled: bool,
        shuffle_order: Option<Vec<usize>>,
    ) {
        self.set_queue_offline_only(false);
        let queue = self.queue.write().await;
        queue.set_queue_with_order(tracks, start_index, shuffle_enabled, shuffle_order);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
    }

    /// Remove a track by index
    pub async fn remove_track(&self, index: usize) -> Option<QueueTrack> {
        let queue = self.queue.write().await;
        let removed = queue.remove_track(index);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        removed
    }

    /// Remove a track from the upcoming list by position
    pub async fn remove_upcoming_track(&self, upcoming_index: usize) -> Option<QueueTrack> {
        let queue = self.queue.write().await;
        let removed = queue.remove_upcoming_track(upcoming_index);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        removed
    }

    /// Move a track from one position to another
    pub async fn move_track(&self, from_index: usize, to_index: usize) -> bool {
        let queue = self.queue.write().await;
        let success = queue.move_track(from_index, to_index);
        if success {
            self.emit(CoreEvent::QueueUpdated {
                state: queue.get_state(),
            })
            .await;
        }
        success
    }

    /// Jump to a specific track by index
    pub async fn play_index(&self, index: usize) -> Option<QueueTrack> {
        let queue = self.queue.write().await;
        let track = queue.play_index(index);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        track
    }

    /// Jump to a track by its position in the upcoming list (as shown in the
    /// Queue sidebar). Shuffle-aware: resolves through `shuffle_order` when
    /// shuffle is active.
    pub async fn play_upcoming_at(&self, upcoming_index: usize) -> Option<QueueTrack> {
        let queue = self.queue.write().await;
        let track = queue.play_upcoming_at(upcoming_index);
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        track
    }

    /// Play `track_id` preferring an offline-cached copy (the offline tier of
    /// the shared playback tier-walk) before the player's own L1/L2 → network
    /// path. `offline` is the frontend's open `OfflineCacheState` (None = no
    /// offline tier); `sink` optionally drives the unlock animation.
    ///
    /// The player is untouched: an offline hit is handed to `play_data` (which
    /// warms L1), a miss falls through to `Player::play_track`.
    pub async fn play_track_resolved(
        &self,
        track_id: u64,
        quality: Quality,
        offline: Option<&qbz_offline_cache::OfflineCacheState>,
        sink: Option<&qbz_offline_cache::CacheEventSink>,
    ) -> Result<(), String> {
        if let Some(off) = offline {
            if let Some(bytes) =
                crate::offline_resolve::resolve_offline_bytes(track_id, off, sink).await
            {
                log::info!("[Core] track {} served from OFFLINE cache", track_id);
                return self.player.play_data(bytes, track_id);
            }
        }
        let guard = self.client.read().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| "No Qobuz client available".to_string())?;
        self.player.play_track(client, track_id, quality).await
    }

    /// Resolve the bytes for a GAPLESS successor. Tier order L1/L2 → offline →
    /// network: the offline tier is checked only when the track is NOT already
    /// in the player's cache (the CMAF decrypt is slow, ~5-7 s, so a cached
    /// copy wins). Returns bytes to hand to `Player::play_next`, or None.
    pub async fn fetch_for_gapless_resolved(
        &self,
        track_id: u64,
        quality: Quality,
        offline: Option<&qbz_offline_cache::OfflineCacheState>,
        sink: Option<&qbz_offline_cache::CacheEventSink>,
    ) -> Option<Vec<u8>> {
        if !self.player.is_track_cached(track_id) {
            if let Some(off) = offline {
                if let Some(bytes) =
                    crate::offline_resolve::resolve_offline_bytes(track_id, off, sink).await
                {
                    return Some(bytes);
                }
            }
        }
        let guard = self.client.read().await;
        let client = guard.as_ref()?;
        self.player.fetch_for_gapless(client, track_id, quality).await
    }

    /// Resolve a fully-materialized audio asset (bytes + MIME + quality) for an
    /// EXTERNAL renderer (Chromecast / DLNA). Tier order mirrors
    /// `fetch_for_gapless_resolved`: L1/L2 player cache -> OFFLINE (local CMAF
    /// decrypt, no network) -> network. The offline tier is what makes a
    /// downloaded track cast fast with no connection (the same "local segments,
    /// decrypt on demand" path the offline cache uses for playback). On a cache
    /// or offline hit the precise delivered quality isn't known here — the Cast
    /// service derives the quality label from the track's catalog metadata.
    pub async fn fetch_for_external_stream_resolved(
        &self,
        track_id: u64,
        quality: Quality,
        offline: Option<&qbz_offline_cache::OfflineCacheState>,
        sink: Option<&qbz_offline_cache::CacheEventSink>,
    ) -> Option<ExternalStreamAsset> {
        // L1/L2 player cache (decrypted FLAC) is handled inside
        // fetch_for_external_stream; only reach for the offline tier when the
        // track is not already cached (the CMAF decrypt is slow).
        if !self.player.is_track_cached(track_id) {
            if let Some(off) = offline {
                if let Some(bytes) =
                    crate::offline_resolve::resolve_offline_bytes(track_id, off, sink).await
                {
                    log::info!("[CAST-FETCH] track {track_id} served from OFFLINE cache");
                    return Some(ExternalStreamAsset {
                        bytes,
                        content_type: "audio/flac".to_string(),
                        quality: StreamQualityInfo::from_raw(0, None, None),
                        duration_secs: None,
                        origin: AssetOrigin::Offline,
                    });
                }
            }
        }
        let guard = self.client.read().await;
        let client = guard.as_ref()?;
        self.player
            .fetch_for_external_stream(client, track_id, quality)
            .await
    }

    /// Advance to next track in queue
    pub async fn next_track(&self) -> Option<QueueTrack> {
        let queue = self.queue.write().await;
        let track = queue.next();
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        track
    }

    /// Go to previous track in queue
    pub async fn previous_track(&self) -> Option<QueueTrack> {
        let queue = self.queue.write().await;
        let track = queue.previous();
        self.emit(CoreEvent::QueueUpdated {
            state: queue.get_state(),
        })
        .await;
        track
    }

    /// Get multiple upcoming tracks without advancing (for prefetching)
    pub async fn peek_upcoming(&self, count: usize) -> Vec<QueueTrack> {
        let queue = self.queue.read().await;
        queue.peek_upcoming(count)
    }

    /// The current queue track, if any (source-aware playback routing).
    pub async fn current_track(&self) -> Option<QueueTrack> {
        let queue = self.queue.read().await;
        queue.current()
    }

    /// Reconcile the queue pointer to the track the audio engine is actually
    /// playing. A gapless hand-off advances inside the player without going
    /// through `next_track`, so the core pointer can lag the live track and
    /// the now-playing card goes stale. This moves the pointer to the track
    /// with `id` and returns it plus whether the pointer moved; a queue
    /// update is emitted only when it did. Frontend-agnostic — the playback
    /// poll loop calls this to keep now-playing in sync (ADR-006).
    pub async fn sync_current_to_id(&self, id: u64) -> Option<(QueueTrack, bool)> {
        let queue = self.queue.write().await;
        let result = queue.sync_current_to_id(id);
        if matches!(result, Some((_, true))) {
            self.emit(CoreEvent::QueueUpdated {
                state: queue.get_state(),
            })
            .await;
        }
        result
    }

    /// Patch the cached quality of any queued Plex track whose `rating_key`
    /// matches one of `updates` (`(rating_key, bit_depth, sample_rate_khz)`).
    /// Frontend-agnostic hook for the Plex quality-hydration path: a track
    /// hydrated while it is already enqueued/playing carries a frozen quality
    /// snapshot, so this upgrades it in place. Returns true if the CURRENT
    /// track was patched — the caller then re-pushes the now-playing stamp.
    pub async fn patch_plex_queue_quality(
        &self,
        updates: &[(String, Option<u32>, Option<f64>)],
    ) -> bool {
        let queue = self.queue.write().await;
        let current_patched = queue.patch_plex_quality(updates);
        if current_patched {
            self.emit(CoreEvent::QueueUpdated {
                state: queue.get_state(),
            })
            .await;
        }
        current_patched
    }

    // ==================== Search & Catalog ====================

    /// Search for albums
    pub async fn search_albums(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Album>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .search_albums(query, limit, offset, search_type)
            .await
            .map_err(CoreError::Api)
    }

    /// Search for tracks
    pub async fn search_tracks(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Track>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .search_tracks(query, limit, offset, search_type)
            .await
            .map_err(CoreError::Api)
    }

    /// Search for artists
    pub async fn search_artists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
        search_type: Option<&str>,
    ) -> Result<SearchResultsPage<Artist>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .search_artists(query, limit, offset, search_type)
            .await
            .map_err(CoreError::Api)
    }

    /// Catalog search (combined: albums, tracks, artists, playlists, most_popular).
    /// Returns raw JSON for the caller to parse.
    pub async fn catalog_search(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<serde_json::Value, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .catalog_search(query, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Combined search: `catalog_search` plus parsing of the four category
    /// pages and the `most_popular` hero, with blacklist filtering applied.
    /// The blacklist is a parameter so Search does not depend on the
    /// un-migrated `artist_blacklist` module.
    pub async fn search_all(
        &self,
        query: &str,
        blacklist: &BlacklistFilter,
    ) -> Result<SearchAllResults, CoreError> {
        let json = self.catalog_search(query, 30, 0).await?;
        Ok(parse_search_all(&json, blacklist))
    }

    /// Get album by ID
    pub async fn get_album(&self, album_id: &str) -> Result<Album, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client.get_album(album_id).await.map_err(CoreError::Api)
    }

    /// Get track by ID
    pub async fn get_track(&self, track_id: u64) -> Result<Track, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client.get_track(track_id).await.map_err(CoreError::Api)
    }

    /// Get artist by ID
    pub async fn get_artist(&self, artist_id: u64) -> Result<Artist, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_artist_basic(artist_id)
            .await
            .map_err(CoreError::Api)
    }

    // ==================== Streaming ====================

    /// Get stream URL for a track with quality fallback
    pub async fn get_stream_url(
        &self,
        track_id: u64,
        quality: Quality,
    ) -> Result<StreamUrl, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_stream_url_with_fallback(track_id, quality)
            .await
            .map_err(CoreError::Api)
    }

    // ==================== Playback Operations ====================

    /// Pause playback
    pub fn pause(&self) -> Result<(), CoreError> {
        self.player.pause().map_err(|e| CoreError::Playback(e))
    }

    /// Resume playback
    pub fn resume(&self) -> Result<(), CoreError> {
        self.player.resume().map_err(|e| CoreError::Playback(e))
    }

    /// Stop playback
    pub fn stop(&self) -> Result<(), CoreError> {
        self.player.stop().map_err(|e| CoreError::Playback(e))
    }

    /// Seek to position in seconds
    pub fn seek(&self, position: u64) -> Result<(), CoreError> {
        self.player
            .seek(position)
            .map_err(|e| CoreError::Playback(e))
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume(&self, volume: f32) -> Result<(), CoreError> {
        self.player
            .set_volume(volume)
            .map_err(|e| CoreError::Playback(e))
    }

    /// Get current playback state
    pub fn get_playback_state(&self) -> PlaybackState {
        let state = &self.player.state;
        PlaybackState {
            is_playing: state.is_playing(),
            position: state.current_position(),
            duration: state.duration(),
            track_id: state.current_track_id(),
            volume: state.volume(),
        }
    }

    /// Get the player (for advanced usage)
    pub fn player(&self) -> Arc<Player> {
        Arc::clone(&self.player)
    }

    // ==================== Favorites ====================

    /// Get favorites (albums, tracks, or artists)
    pub async fn get_favorites(
        &self,
        fav_type: &str,
        limit: u32,
        offset: u32,
    ) -> Result<serde_json::Value, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_favorites(fav_type, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Add item to favorites
    pub async fn add_favorite(&self, fav_type: &str, item_id: &str) -> Result<(), CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .add_favorite(fav_type, item_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Remove item from favorites
    pub async fn remove_favorite(&self, fav_type: &str, item_id: &str) -> Result<(), CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .remove_favorite(fav_type, item_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Fetch the set of the user's favorite track IDs. Pages through the
    /// favorites endpoint until exhausted. Used by clients that need to
    /// reflect favorite state on individual tracks (e.g. the Queue
    /// sidebar's now-playing heart).
    pub async fn favorite_track_ids(&self) -> Result<std::collections::HashSet<u64>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        let mut ids = std::collections::HashSet::new();
        let page_size: u32 = 500;
        let mut offset: u32 = 0;
        loop {
            let value = client
                .get_favorites("tracks", page_size, offset)
                .await
                .map_err(CoreError::Api)?;
            let items = value
                .get("tracks")
                .and_then(|t| t.get("items"))
                .and_then(|i| i.as_array())
                .cloned()
                .unwrap_or_default();
            let count = items.len() as u32;
            for item in &items {
                if let Some(id) = item.get("id").and_then(|v| v.as_u64()) {
                    ids.insert(id);
                }
            }
            if count < page_size {
                break;
            }
            offset += page_size;
        }
        Ok(ids)
    }

    /// Fetch the set of the user's favorite (followed) artist IDs. Pages
    /// through the favorites endpoint until exhausted. Used to reflect
    /// follow state on artist cards.
    pub async fn favorite_artist_ids(&self) -> Result<std::collections::HashSet<u64>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        let mut ids = std::collections::HashSet::new();
        let page_size: u32 = 500;
        let mut offset: u32 = 0;
        loop {
            let value = client
                .get_favorites("artists", page_size, offset)
                .await
                .map_err(CoreError::Api)?;
            let items = value
                .get("artists")
                .and_then(|a| a.get("items"))
                .and_then(|i| i.as_array())
                .cloned()
                .unwrap_or_default();
            let count = items.len() as u32;
            for item in &items {
                if let Some(id) = item.get("id").and_then(|v| v.as_u64()) {
                    ids.insert(id);
                }
            }
            if count < page_size {
                break;
            }
            offset += page_size;
        }
        Ok(ids)
    }

    /// Toggle the favorite state of a track. `make_favorite = true` adds it,
    /// `false` removes it. Thin convenience over `add_favorite` /
    /// `remove_favorite` so callers do not duplicate the type string.
    pub async fn set_track_favorite(
        &self,
        track_id: u64,
        make_favorite: bool,
    ) -> Result<(), CoreError> {
        let id = track_id.to_string();
        if make_favorite {
            self.add_favorite("track", &id).await
        } else {
            self.remove_favorite("track", &id).await
        }
    }

    // ==================== Playlists ====================

    /// Get user playlists
    pub async fn get_user_playlists(&self) -> Result<Vec<Playlist>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client.get_user_playlists().await.map_err(CoreError::Api)
    }

    /// Get playlist by ID
    pub async fn get_playlist(&self, playlist_id: u64) -> Result<Playlist, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_playlist(playlist_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Add tracks to playlist
    pub async fn add_tracks_to_playlist(
        &self,
        playlist_id: u64,
        track_ids: &[u64],
    ) -> Result<(), CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .add_tracks_to_playlist(playlist_id, track_ids)
            .await
            .map_err(CoreError::Api)
    }

    /// Check how many of `track_ids` are already in the Qobuz playlist
    /// `playlist_id`. Mirrors Tauri's `v2_check_playlist_duplicates`
    /// (commands_v2/playlists.rs): fetch the playlist's existing track ids and
    /// set-intersect with the input. This is Qobuz-tracks-into-Qobuz-playlist
    /// only — callers gate out local / Plex / local-playlist targets before
    /// calling (those never duplicate-check, mirroring the Tauri flow).
    pub async fn check_playlist_duplicates(
        &self,
        playlist_id: u64,
        track_ids: &[u64],
    ) -> Result<PlaylistDuplicateResult, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        let playlist = client
            .get_playlist_track_ids(playlist_id)
            .await
            .map_err(CoreError::Api)?;
        Ok(compute_playlist_duplicates(&playlist.track_ids, track_ids))
    }

    /// Remove tracks from playlist
    pub async fn remove_tracks_from_playlist(
        &self,
        playlist_id: u64,
        playlist_track_ids: &[u64],
    ) -> Result<(), CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .remove_tracks_from_playlist(playlist_id, playlist_track_ids)
            .await
            .map_err(CoreError::Api)
    }

    /// Create a new playlist
    pub async fn create_playlist(
        &self,
        name: &str,
        description: Option<&str>,
        is_public: bool,
    ) -> Result<Playlist, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .create_playlist(name, description, is_public)
            .await
            .map_err(CoreError::Api)
    }

    /// Delete a playlist
    pub async fn delete_playlist(&self, playlist_id: u64) -> Result<(), CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .delete_playlist(playlist_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Update a playlist
    pub async fn update_playlist(
        &self,
        playlist_id: u64,
        name: Option<&str>,
        description: Option<&str>,
        is_public: Option<bool>,
    ) -> Result<Playlist, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .update_playlist(playlist_id, name, description, is_public)
            .await
            .map_err(CoreError::Api)
    }

    /// Search playlists
    pub async fn search_playlists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResultsPage<Playlist>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .search_playlists(query, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get tracks batch by IDs
    pub async fn get_tracks_batch(&self, track_ids: &[u64]) -> Result<Vec<Track>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_tracks_batch(track_ids)
            .await
            .map_err(CoreError::Api)
    }

    /// Get genres
    pub async fn get_genres(&self, parent_id: Option<u64>) -> Result<Vec<GenreInfo>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client.get_genres(parent_id).await.map_err(CoreError::Api)
    }

    /// Get discover index
    pub async fn get_discover_index(
        &self,
        genre_ids: Option<Vec<u64>>,
    ) -> Result<DiscoverResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_discover_index(genre_ids)
            .await
            .map_err(CoreError::Api)
    }

    /// Get discover playlists
    pub async fn get_discover_playlists(
        &self,
        tag: Option<String>,
        genre_ids: Option<Vec<u64>>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<DiscoverPlaylistsResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_discover_playlists(tag, genre_ids, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get playlist tags
    pub async fn get_playlist_tags(&self) -> Result<Vec<PlaylistTag>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client.get_playlist_tags().await.map_err(CoreError::Api)
    }

    /// Get discover albums from a specific browse endpoint
    pub async fn get_discover_albums(
        &self,
        endpoint: &str,
        genre_ids: Option<Vec<u64>>,
        offset: u32,
        limit: u32,
    ) -> Result<DiscoverData<DiscoverAlbum>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_discover_albums(endpoint, genre_ids, offset, limit)
            .await
            .map_err(CoreError::Api)
    }

    /// Get featured albums
    pub async fn get_featured_albums(
        &self,
        featured_type: &str,
        limit: u32,
        offset: u32,
        genre_id: Option<u64>,
    ) -> Result<SearchResultsPage<Album>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_featured_albums(featured_type, limit, offset, genre_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Get Release Watch — new releases from followed artists/labels/awards.
    /// `release_type` must be one of "artists" | "labels" | "awards".
    pub async fn get_release_watch(
        &self,
        release_type: &str,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResultsPage<Album>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_release_watch(release_type, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get artist page (full artist details with albums, tracks, similar)
    pub async fn get_artist_page(
        &self,
        artist_id: u64,
        sort: Option<&str>,
    ) -> Result<PageArtistResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_artist_page(artist_id, sort)
            .await
            .map_err(CoreError::Api)
    }

    /// Get similar artists
    pub async fn get_similar_artists(
        &self,
        artist_id: u64,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResultsPage<Artist>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_similar_artists(artist_id, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Albums similar to a seed album (`/album/suggest`).
    pub async fn get_album_suggest(
        &self,
        album_id: &str,
    ) -> Result<qbz_models::AlbumSuggestResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_album_suggest(album_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Qobuz artist radio (`/radio/artist`) — a generated track list.
    pub async fn get_radio_artist(
        &self,
        artist_id: &str,
    ) -> Result<qbz_models::RadioResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_radio_artist(artist_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Qobuz album radio (`/radio/album`).
    pub async fn get_radio_album(
        &self,
        album_id: &str,
    ) -> Result<qbz_models::RadioResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_radio_album(album_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Qobuz track radio (`/radio/track`).
    pub async fn get_radio_track(
        &self,
        track_id: &str,
    ) -> Result<qbz_models::RadioResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_radio_track(track_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Dynamic mix suggestions (`/dynamic/suggest`) seeded from
    /// recently-listened track ids. Returns the suggested tracks.
    pub async fn get_dynamic_suggest(
        &self,
        listened_track_ids: &[u64],
        limit: u32,
    ) -> Result<Vec<Track>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_dynamic_suggest(listened_track_ids, limit)
            .await
            .map_err(CoreError::Api)
    }

    /// Dynamic mix suggestions with the `track_to_analysed` payload — the
    /// PRIMARY DailyQ/WeeklyQ path (see `QobuzClient::get_dynamic_suggest_full`).
    pub async fn get_dynamic_suggest_full(
        &self,
        listened_track_ids: &[u64],
        tracks_to_analyse: &[TrackToAnalyse],
        limit: u32,
    ) -> Result<Vec<Track>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_dynamic_suggest_full(listened_track_ids, tracks_to_analyse, limit)
            .await
            .map_err(CoreError::Api)
    }

    /// Smart artist radio via the local `qbz-radio` pool builder — a
    /// richer alternative to the Qobuz `/radio/artist` endpoint. Builds
    /// a session pool (seed tracks + similar artists + second-degree)
    /// in the local radio DB, pulls up to 50 track ids from the engine,
    /// and resolves them to full tracks.
    ///
    /// The radio DB uses a `!Send` rusqlite connection, so the pool
    /// build + pull run on blocking threads (mirroring the Tauri
    /// command); the async builder is driven there via `block_on`.
    pub async fn create_smart_artist_radio(
        &self,
        artist_id: u64,
    ) -> Result<Vec<Track>, CoreError> {
        let client = {
            let guard = self.client.read().await;
            guard.as_ref().ok_or(CoreError::NotInitialized)?.clone()
        };

        let build_client = client.clone();
        let session_id = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let db = qbz_radio::RadioDb::open_default()?;
            let builder = qbz_radio::RadioPoolBuilder::new(
                &db,
                &build_client,
                qbz_radio::BuildRadioOptions::default(),
            );
            let session = tokio::runtime::Handle::current()
                .block_on(builder.create_artist_radio(artist_id))?;
            Ok(session.id)
        })
        .await
        .map_err(|e| CoreError::Internal(format!("radio build task: {e}")))?
        .map_err(CoreError::Internal)?;

        let ids = tokio::task::spawn_blocking(move || -> Result<Vec<u64>, String> {
            let db = qbz_radio::RadioDb::open_default()?;
            let engine = qbz_radio::RadioEngine::new(db);
            let mut ids = Vec::new();
            for _ in 0..60 {
                match engine.next_track(&session_id) {
                    Ok(track) => ids.push(track.track_id),
                    Err(_) => break,
                }
            }
            Ok(ids.into_iter().take(50).collect())
        })
        .await
        .map_err(|e| CoreError::Internal(format!("radio pull task: {e}")))?
        .map_err(CoreError::Internal)?;

        let mut tracks = Vec::new();
        for id in ids {
            if let Ok(track) = client.get_track(id).await {
                tracks.push(track);
            }
        }
        Ok(tracks)
    }

    /// Get artist with albums (for album pagination)
    pub async fn get_artist_with_albums(
        &self,
        artist_id: u64,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Artist, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_artist_with_pagination(artist_id, true, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get an artist's albums collection (paginated `ArtistAlbums` only).
    ///
    /// Equivalent to `get_artist_with_albums` but projects only the `albums`
    /// field for callers that don't need the full artist envelope.
    pub async fn get_artist_albums(
        &self,
        artist_id: u64,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<ArtistAlbums, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        let artist = client
            .get_artist_with_pagination(artist_id, true, limit, offset)
            .await
            .map_err(CoreError::Api)?;

        artist
            .albums
            .ok_or_else(|| CoreError::Api(qbz_qobuz::ApiError::ApiResponse(
                "No albums in artist response".to_string(),
            )))
    }

    /// Get artist detail with albums, playlists and appears-on tracks.
    ///
    /// Backs the suggestions panel: requests `extra=albums,tracks_appears_on,playlists`
    /// from `/artist/get` so callers can read `playlists` and `tracks_appears_on`
    /// without a second round-trip.
    pub async fn get_artist_detail(
        &self,
        artist_id: u64,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Artist, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_artist_detail(artist_id, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get an artist's popular/top tracks (`/artist/get?extra=tracks`).
    pub async fn get_artist_tracks(
        &self,
        artist_id: u64,
        limit: u32,
        offset: u32,
    ) -> Result<TracksContainer, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_artist_tracks(artist_id, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get an artist's releases grid (paginated by `release_type`).
    pub async fn get_releases_grid(
        &self,
        artist_id: u64,
        release_type: &str,
        limit: u32,
        offset: u32,
        sort: Option<&str>,
    ) -> Result<ReleasesGridResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_releases_grid(artist_id, release_type, limit, offset, sort)
            .await
            .map_err(CoreError::Api)
    }

    /// Get label page (aggregated: top tracks, releases, playlists, artists)
    pub async fn get_label_page(&self, label_id: u64) -> Result<LabelPageData, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_label_page(label_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Enumerate award catalog (/award/explore).
    pub async fn get_award_explore(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<serde_json::Value, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_award_explore(limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get award page — hero info + award-winning releases.
    pub async fn get_award_page(
        &self,
        award_id: &str,
    ) -> Result<qbz_models::AwardPageData, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_award_page(award_id)
            .await
            .map_err(CoreError::Api)
    }

    /// Get paginated albums for an award (/award/getAlbums).
    pub async fn get_award_albums(
        &self,
        award_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResultsPage<Album>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_award_albums(award_id, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get label explore (discover more labels)
    pub async fn get_label_explore(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<LabelExploreResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;

        client
            .get_label_explore(limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get a label's album catalog (paginated, replaces legacy /label/get).
    #[allow(clippy::too_many_arguments)]
    pub async fn get_label_albums(
        &self,
        label_id: u64,
        limit: u32,
        offset: u32,
        sort: Option<String>,
        order: Option<String>,
        genre_ids: Option<String>,
        from_date: Option<String>,
        to_date: Option<String>,
    ) -> Result<LabelListPage<Album>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_label_albums(
                label_id,
                limit,
                offset,
                sort.as_deref(),
                order.as_deref(),
                genre_ids.as_deref(),
                from_date.as_deref(),
                to_date.as_deref(),
            )
            .await
            .map_err(CoreError::Api)
    }

    /// Get a label's upcoming releases.
    pub async fn get_label_next_releases(
        &self,
        label_id: u64,
        limit: u32,
        offset: u32,
        genre_ids: Option<String>,
    ) -> Result<LabelListPage<Album>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_label_next_releases(label_id, limit, offset, genre_ids.as_deref())
            .await
            .map_err(CoreError::Api)
    }

    /// Get a label's press-awarded releases.
    pub async fn get_label_awarded_releases(
        &self,
        label_id: u64,
        limit: u32,
        offset: u32,
        sort: Option<String>,
        order: Option<String>,
        genre_ids: Option<String>,
    ) -> Result<LabelListPage<Album>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_label_awarded_releases(
                label_id,
                limit,
                offset,
                sort.as_deref(),
                order.as_deref(),
                genre_ids.as_deref(),
            )
            .await
            .map_err(CoreError::Api)
    }

    /// Get a label's curated playlists.
    pub async fn get_label_playlists(
        &self,
        label_id: u64,
        limit: u32,
        offset: u32,
    ) -> Result<LabelListPage<Playlist>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_label_playlists(label_id, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get a label's top artists.
    pub async fn get_label_top_artists(
        &self,
        label_id: u64,
        limit: u32,
        offset: u32,
    ) -> Result<LabelListPage<Artist>, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_label_top_artists(label_id, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Get a label's editorial story.
    pub async fn get_label_story(
        &self,
        label_id: u64,
        limit: u32,
        offset: u32,
    ) -> Result<LabelStoryResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_label_story(label_id, limit, offset)
            .await
            .map_err(CoreError::Api)
    }

    /// Bulk hydrate labels by ID list.
    pub async fn get_label_list(
        &self,
        label_ids: Vec<u64>,
    ) -> Result<LabelGetListResponse, CoreError> {
        let client = self.client.read().await;
        let client = client.as_ref().ok_or(CoreError::NotInitialized)?;
        client
            .get_label_list(&label_ids)
            .await
            .map_err(CoreError::Api)
    }

    // ==================== Event Emission ====================

    /// Emit an event to the frontend adapter
    async fn emit(&self, event: CoreEvent) {
        self.adapter.on_event(event).await;
    }

    /// Get the frontend adapter (for external event emission)
    pub fn adapter(&self) -> Arc<A> {
        Arc::clone(&self.adapter)
    }

    /// Get the Qobuz client (for advanced usage)
    pub fn client(&self) -> Arc<RwLock<Option<QobuzClient>>> {
        Arc::clone(&self.client)
    }

    /// Get the queue manager (for advanced usage)
    pub fn queue(&self) -> Arc<RwLock<QueueManager>> {
        Arc::clone(&self.queue)
    }

    // ----- MusicBrainz -------------------------------------------------------

    /// Whether MusicBrainz integration is currently enabled.
    pub async fn musicbrainz_is_enabled(&self) -> bool {
        self.musicbrainz.is_enabled().await
    }

    /// Enable or disable MusicBrainz integration.
    pub async fn musicbrainz_set_enabled(&self, enabled: bool) {
        self.musicbrainz.set_enabled(enabled).await;
    }

    /// Resolve an artist name to a MusicBrainz id. Returns `None` if no
    /// confident match is found.
    pub async fn musicbrainz_resolve_artist(
        &self,
        name: &str,
    ) -> Result<Option<ResolvedArtist>, CoreError> {
        self.musicbrainz
            .resolve_artist(name)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))
    }

    /// Fetch the artist metadata (location, life_span, genre seeds) for
    /// the Origin section of the artist network sidebar. Resolves the
    /// real country from the begin_area hierarchy when a city-level
    /// location is found, because MB's `country` field is where the
    /// artist is active, not where they were born/formed.
    pub async fn musicbrainz_get_artist_metadata(
        &self,
        mbid: &str,
    ) -> Result<ArtistMetadata, CoreError> {
        // Cache lookup — same behavior as Tauri's v2 command.
        if let Ok(guard) = self.musicbrainz_cache.lock() {
            if let Some(cache) = guard.as_ref() {
                if let Ok(Some(cached)) = cache.get_artist_metadata(mbid) {
                    return Ok(cached);
                }
            }
        }

        let artist = self
            .musicbrainz
            .get_artist_with_relations(mbid)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        let mut metadata = location::extract_metadata(&artist);

        if let Some(ref mut loc) = metadata.location {
            if loc.city.is_some() {
                if let Some(ref area_id) = loc.area_id {
                    if let Ok(Some((country_name, country_code))) =
                        self.musicbrainz.resolve_area_country(area_id).await
                    {
                        loc.display_name = format!("{}, {}", loc.display_name, country_name);
                        loc.country = Some(country_name);
                        loc.country_code = country_code;
                    }
                }
            }
        }

        if let Ok(guard) = self.musicbrainz_cache.lock() {
            if let Some(cache) = guard.as_ref() {
                let _ = cache.set_artist_metadata(mbid, &metadata);
            }
        }

        Ok(metadata)
    }

    /// Fetch the artist relationships (band members, member-of groups,
    /// collaborators) for the Relationships section of the sidebar.
    /// Splits `member of band` by direction: backward direction lists
    /// members of *this* artist (still-active vs ended -> past), forward
    /// direction lists groups this artist is a member of.
    pub async fn musicbrainz_get_artist_relationships(
        &self,
        mbid: &str,
    ) -> Result<ArtistRelationships, CoreError> {
        if let Ok(guard) = self.musicbrainz_cache.lock() {
            if let Some(cache) = guard.as_ref() {
                if let Ok(Some(cached)) = cache.get_artist_relations(mbid) {
                    return Ok(cached);
                }
            }
        }

        let artist = self
            .musicbrainz
            .get_artist_with_relations(mbid)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        let mut members = Vec::new();
        let mut past_members = Vec::new();
        let mut groups = Vec::new();
        let mut collaborators = Vec::new();

        if let Some(relations) = &artist.relations {
            for relation in relations {
                let Some(related_artist) = &relation.artist else {
                    continue;
                };

                let related = RelatedArtist {
                    mbid: related_artist.id.clone(),
                    name: related_artist.name.clone(),
                    role: relation
                        .attributes
                        .as_ref()
                        .and_then(|a| a.first().cloned()),
                    period: Some(Period {
                        begin: relation.begin.clone(),
                        end: relation.end.clone(),
                    }),
                    ended: relation.ended.unwrap_or(false),
                };

                match relation.relation_type.as_str() {
                    "member of band" => {
                        if relation.direction.as_deref() == Some("backward") {
                            if related.ended {
                                past_members.push(related);
                            } else {
                                members.push(related);
                            }
                        } else {
                            groups.push(related);
                        }
                    }
                    "collaboration" => {
                        collaborators.push(related);
                    }
                    _ => {}
                }
            }
        }

        let result = ArtistRelationships {
            members,
            past_members,
            groups,
            collaborators,
        };

        if let Ok(guard) = self.musicbrainz_cache.lock() {
            if let Some(cache) = guard.as_ref() {
                let _ = cache.set_artist_relations(mbid, &result);
            }
        }

        Ok(result)
    }

    /// "You may also like" tag-based discovery — finds artists that
    /// share the seed artist's primary genre tag on MusicBrainz, then
    /// validates exact name matches on Qobuz so the row can actually
    /// open the artist page. Filters out the seed itself, the artists
    /// already shown in the Similar section, and any dismissed
    /// artists (passed in by the caller; the dismiss store lives at
    /// the frontend layer).
    ///
    /// When the primary tag does not return enough validated results,
    /// the pipeline falls back to the secondary tag, dedupes against
    /// the primary's results, and tops up. Result ordering is
    /// deterministic per seed_mbid (same artist page = same shuffle).
    pub async fn musicbrainz_discover_artists(
        &self,
        seed_mbid: &str,
        seed_name: &str,
        similar_names: &[String],
        dismissed_per_tag: &(dyn Fn(&str) -> std::collections::HashSet<String> + Send + Sync),
        known_artists: &(dyn Fn() -> (
            std::collections::HashSet<u64>,
            std::collections::HashSet<String>,
        )
                       + Send
                       + Sync),
    ) -> Result<DiscoveryResponse, CoreError> {
        use std::collections::HashSet;

        if !self.musicbrainz.is_enabled().await {
            return Ok(DiscoveryResponse {
                artists: Vec::new(),
                primary_tag: String::new(),
            });
        }

        let seed_tags = self
            .musicbrainz
            .get_artist_tags(seed_mbid)
            .await
            .unwrap_or_default();
        if seed_tags.is_empty() {
            return Ok(DiscoveryResponse {
                artists: Vec::new(),
                primary_tag: String::new(),
            });
        }

        let primary_tag = seed_tags[0].clone();
        let mb_results = self
            .musicbrainz
            .search_artists_by_tag(&primary_tag, 50)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        if mb_results.artists.is_empty() {
            return Ok(DiscoveryResponse {
                artists: Vec::new(),
                primary_tag,
            });
        }

        let seed_norm = normalize_artist_name(seed_name);
        let similar_norm: HashSet<String> =
            similar_names.iter().map(|n| normalize_artist_name(n)).collect();
        let dismissed_primary = dismissed_per_tag(&primary_tag.to_lowercase());
        let (known_ids, known_names) = known_artists();

        let mut candidates: Vec<(String, String)> = Vec::new();
        for artist in &mb_results.artists {
            let n = normalize_artist_name(&artist.name);
            if n == seed_norm
                || artist.id.eq_ignore_ascii_case(seed_mbid)
                || similar_norm.contains(&n)
                || dismissed_primary.contains(&n)
                || known_names.contains(&n)
            {
                continue;
            }
            candidates.push((artist.id.clone(), artist.name.clone()));
        }

        shuffle_with_seed(&mut candidates, seed_mbid, None);

        let max_results = 8;
        let min_results = 5;
        let mut results = self
            .validate_discovery_on_qobuz(&candidates, max_results, &known_ids)
            .await;

        if results.len() < min_results && seed_tags.len() > 1 {
            let secondary_tag = seed_tags[1].clone();
            let dismissed_secondary = dismissed_per_tag(&secondary_tag.to_lowercase());
            let existing_mbids: HashSet<String> =
                results.iter().map(|r| r.mbid.clone()).collect();
            if let Ok(secondary) = self
                .musicbrainz
                .search_artists_by_tag(&secondary_tag, 30)
                .await
            {
                let mut secondary_candidates: Vec<(String, String)> = Vec::new();
                for a in &secondary.artists {
                    let n = normalize_artist_name(&a.name);
                    if n == seed_norm
                        || a.id.eq_ignore_ascii_case(seed_mbid)
                        || similar_norm.contains(&n)
                        || dismissed_primary.contains(&n)
                        || dismissed_secondary.contains(&n)
                        || known_names.contains(&n)
                        || existing_mbids.contains(&a.id)
                    {
                        continue;
                    }
                    secondary_candidates.push((a.id.clone(), a.name.clone()));
                }
                shuffle_with_seed(&mut secondary_candidates, seed_mbid, Some(&secondary_tag));
                let remaining = max_results.saturating_sub(results.len());
                let mut more = self
                    .validate_discovery_on_qobuz(
                        &secondary_candidates,
                        remaining,
                        &known_ids,
                    )
                    .await;
                results.append(&mut more);
            }
        }

        Ok(DiscoveryResponse {
            artists: results,
            primary_tag,
        })
    }

    async fn validate_discovery_on_qobuz(
        &self,
        candidates: &[(String, String)],
        max: usize,
        known_ids: &std::collections::HashSet<u64>,
    ) -> Vec<DiscoveryArtist> {
        let mut out: Vec<DiscoveryArtist> = Vec::new();
        for (mbid, name) in candidates {
            if out.len() >= max {
                break;
            }
            let Ok(page) = self.search_artists(name, 1, 0, None).await else {
                continue;
            };
            let Some(first) = page.items.first() else {
                continue;
            };
            if normalize_artist_name(&first.name) != normalize_artist_name(name) {
                continue;
            }
            // Tauri's `!local_known_qobuz_ids.contains(&artist.id)`
            // gate — never suggest an artist the user has already
            // listened to >2 times.
            if known_ids.contains(&first.id) {
                continue;
            }
            out.push(DiscoveryArtist {
                mbid: mbid.clone(),
                name: first.name.clone(),
                qobuz_id: Some(first.id),
            });
        }
        out
    }
}

// ----- Location / scene discovery -------------------------------------------

impl<A: FrontendAdapter + Send + Sync + 'static> QbzCore<A> {
    /// "Artists from the same place" — given a source artist's MBID,
    /// area and genre/tag seeds, find other artists from that area
    /// who share the genres, validated against Qobuz. Ports
    /// v2_discover_artists_by_location's core pipeline (the scene
    /// cache + progress events are omitted; subdivision resolution
    /// and affinity scoring are kept).
    #[allow(clippy::too_many_arguments)]
    pub async fn discover_artists_by_location(
        &self,
        source_mbid: &str,
        area_id: Option<&str>,
        area_name: &str,
        country: Option<&str>,
        genres: Vec<String>,
        tags: Vec<String>,
        limit: usize,
        offset: usize,
    ) -> Result<LocationDiscoveryResponse, CoreError> {
        use std::collections::HashMap;

        // Step 0: smart area resolution — city → parent subdivision
        // for broader results (Leyton → England, Seattle →
        // Washington).
        let (search_name, display_name) = match area_id {
            Some(aid) => match self.musicbrainz.resolve_parent_subdivision(aid).await {
                Ok(Some((subdivision, _))) => {
                    let display = country
                        .map(|c| format!("{}, {}", c, subdivision))
                        .unwrap_or_else(|| subdivision.clone());
                    (subdivision, display)
                }
                _ => {
                    let display = country
                        .map(|c| format!("{}, {}", c, area_name))
                        .unwrap_or_else(|| area_name.to_string());
                    (area_name.to_string(), display)
                }
            },
            None => {
                let display = country
                    .map(|c| format!("{}, {}", c, area_name))
                    .unwrap_or_else(|| area_name.to_string());
                (area_name.to_string(), display)
            }
        };

        let source_seeds = AffinitySeeds {
            genres: genres.clone(),
            tags: tags.clone(),
            normalized_seeds: genres.iter().chain(tags.iter()).cloned().collect(),
        };

        // Step 2: pick search genres, dropping overly broad tags that
        // would return the whole country's catalog.
        let mut search_genres: Vec<String> = if genres.is_empty() {
            tags.iter()
                .filter(|s| !is_broad_genre(s))
                .take(3)
                .cloned()
                .collect()
        } else {
            genres
                .iter()
                .chain(tags.iter().take(2))
                .filter(|s| !is_broad_genre(s))
                .cloned()
                .collect()
        };
        if search_genres.is_empty() {
            // Everything was broad — fall back to the raw list.
            search_genres = if genres.is_empty() {
                tags.iter().take(3).cloned().collect()
            } else {
                genres.iter().take(3).cloned().collect()
            };
        }
        if search_genres.is_empty() {
            return Ok(LocationDiscoveryResponse {
                artists: Vec::new(),
                scene_label: format!("{} scene", display_name),
                genre_summary: String::new(),
                total_candidates: 0,
                has_more: false,
                next_offset: 0,
            });
        }

        // Step 2/3: MB tag+area search per genre, dedupe + score.
        // candidate_map: mbid -> (name, score_sum, genre_hits, tags)
        let mut candidate_map: HashMap<String, (String, i32, usize, Vec<String>)> =
            HashMap::new();
        let per_genre_limit = 200usize;
        for genre in &search_genres {
            let result = self
                .musicbrainz
                .search_artists_by_tag_and_area(genre, &search_name, country, per_genre_limit, 0)
                .await;
            let Ok(response) = result else {
                continue;
            };
            for artist in &response.artists {
                if artist.id == source_mbid {
                    continue;
                }
                let candidate_tags: Vec<String> = artist
                    .tags
                    .as_ref()
                    .map(|list| {
                        list.iter()
                            .filter(|t| t.count.unwrap_or(0) > 0)
                            .map(|t| t.name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let same_city = artist
                    .begin_area
                    .as_ref()
                    .map(|ba| {
                        ba.name.eq_ignore_ascii_case(&search_name)
                            || area_id.map(|aid| ba.id == aid).unwrap_or(false)
                    })
                    .unwrap_or(false);
                let same_country = artist
                    .area
                    .as_ref()
                    .map(|a| a.name.eq_ignore_ascii_case(&search_name))
                    .unwrap_or(false);
                let score =
                    compute_affinity_score(&candidate_tags, &source_seeds, same_city, same_country);
                let entry = candidate_map
                    .entry(artist.id.clone())
                    .or_insert_with(|| (artist.name.clone(), 0, 0, Vec::new()));
                entry.1 += score;
                entry.2 += 1;
                for tag in &candidate_tags {
                    if !entry.3.contains(tag) {
                        entry.3.push(tag.clone());
                    }
                }
            }
        }

        // Step 3: score + sort. Final = affinity + (genre_hits-1)*15.
        let mut scored: Vec<(String, String, Vec<String>, i32)> = candidate_map
            .into_iter()
            .map(|(mbid, (name, score, genre_hits, tag_list))| {
                let candidate_seeds = extract_affinity_seeds(
                    &tag_list
                        .iter()
                        .map(|name| Tag {
                            name: name.clone(),
                            count: Some(1),
                        })
                        .collect::<Vec<_>>(),
                );
                let multi_genre_bonus = ((genre_hits as i32) - 1) * 15;
                (mbid, name, candidate_seeds.genres, score + multi_genre_bonus)
            })
            .collect();
        scored.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)));

        let total_candidates = scored.len();
        let to_validate: Vec<_> = scored.into_iter().skip(offset).take(limit).collect();

        // Step 4: validate against Qobuz (exact normalized-name match,
        // pick the one with the most albums as a popularity proxy).
        let mut validated: Vec<LocationCandidate> = Vec::new();
        for (mbid, mb_name, candidate_genres, score) in &to_validate {
            let Ok(results) = self.search_artists(mb_name, 5, 0, None).await else {
                continue;
            };
            let mb_norm = normalize_artist_name(mb_name);
            let best = results
                .items
                .iter()
                .filter(|a| normalize_artist_name(&a.name) == mb_norm)
                .max_by_key(|a| a.albums_count.unwrap_or(0));
            if let Some(qobuz_artist) = best {
                let image_url = qobuz_artist
                    .image
                    .as_ref()
                    .and_then(|img| img.small.as_ref().or(img.thumbnail.as_ref()).cloned());
                validated.push(LocationCandidate {
                    mbid: mbid.clone(),
                    mb_name: mb_name.clone(),
                    qobuz_id: Some(qobuz_artist.id as i64),
                    qobuz_name: Some(qobuz_artist.name.clone()),
                    qobuz_image: image_url,
                    score: *score,
                    genres: candidate_genres.clone(),
                    qobuz_albums_count: qobuz_artist.albums_count,
                });
            }
        }

        let scene_label = country
            .map(|c| c.to_string())
            .unwrap_or_else(|| display_name.clone());
        let next_offset = offset + to_validate.len();
        Ok(LocationDiscoveryResponse {
            artists: validated,
            scene_label,
            genre_summary: genre_summary(&source_seeds),
            total_candidates,
            has_more: next_offset < total_candidates,
            next_offset,
        })
    }
}

// ----- Musician resolution + appearances -----------------------------------

impl<A: FrontendAdapter + Send + Sync + 'static> QbzCore<A> {
    /// Resolve a musician (band member / collaborator) to the
    /// strongest available identity: MBID via MusicBrainz, exact-
    /// name Qobuz artist match, or just an appears-on count. Ports
    /// v2_resolve_musician end to end so the artist-network sidebar
    /// click can route the user to the right view (artist page when
    /// confident, MusicianPageView otherwise).
    pub async fn musicbrainz_resolve_musician(
        &self,
        name: &str,
        role: &str,
    ) -> Result<ResolvedMusician, CoreError> {
        let resolved_artist = self
            .musicbrainz
            .resolve_artist(name)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        let normalized_target = name.trim().to_lowercase();
        let artist_results = self.search_artists(name, 10, 0, None).await?;
        let exact = artist_results
            .items
            .iter()
            .find(|artist| artist.name.trim().to_lowercase() == normalized_target);

        if let Some(artist) = exact {
            let qobuz_artist_id = i64::try_from(artist.id).ok();
            return Ok(ResolvedMusician {
                name: name.to_string(),
                role: role.to_string(),
                mbid: None,
                qobuz_artist_id,
                confidence: MusicianConfidence::Confirmed,
                bands: Vec::new(),
                appears_on_count: 0,
            });
        }

        let album_results = self.search_albums(name, 20, 0, None).await?;
        let appears_on_count = album_results.total as usize;

        let confidence = if appears_on_count > 0 {
            MusicianConfidence::Contextual
        } else if resolved_artist.is_some() {
            MusicianConfidence::Weak
        } else {
            MusicianConfidence::None
        };

        Ok(ResolvedMusician {
            name: name.to_string(),
            role: role.to_string(),
            mbid: resolved_artist.as_ref().map(|a| a.mbid.clone()),
            qobuz_artist_id: None,
            confidence,
            bands: Vec::new(),
            appears_on_count,
        })
    }

    /// Fetch the paginated list of albums on which `name` appears,
    /// for the Appears On grid in MusicianPageView. Ports
    /// v2_get_musician_appearances — same Qobuz album search +
    /// per-row mapping with `role_on_album = role`.
    pub async fn musicbrainz_get_musician_appearances(
        &self,
        name: &str,
        role: &str,
        limit: u32,
        offset: u32,
    ) -> Result<MusicianAppearances, CoreError> {
        let results = self.search_albums(name, limit, offset, None).await?;
        let albums = results
            .items
            .into_iter()
            .map(|album| AlbumAppearance {
                album_id: album.id,
                album_title: album.title,
                album_artwork: album.image.large.or(album.image.small).unwrap_or_default(),
                artist_name: album.artist.name,
                year: album.release_date_original,
                role_on_album: role.to_string(),
            })
            .collect::<Vec<_>>();
        Ok(MusicianAppearances {
            albums,
            total: results.total as usize,
        })
    }
}

/// Normalize an artist name for dedupe: trim, lowercase, collapse
/// whitespace. Used by the discovery pipeline so "Iron  Maiden" and
/// "iron maiden" hash to the same key in the dismiss store.
pub fn normalize_artist_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Deterministic shuffle keyed by `seed_mbid` (and optionally a tag).
/// Same artist page produces the same order across runs; different
/// artist or different fallback tag produces a different order.
fn shuffle_with_seed<T>(items: &mut Vec<T>, seed_mbid: &str, secondary_tag: Option<&str>) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    seed_mbid.hash(&mut hasher);
    if let Some(t) = secondary_tag {
        t.hash(&mut hasher);
    }
    let mut h = hasher.finish();
    // Fisher-Yates with a simple xorshift PRNG seeded from the hash —
    // keeps qbz-core free of the rand dep.
    let n = items.len();
    for i in (1..n).rev() {
        h ^= h << 13;
        h ^= h >> 7;
        h ^= h << 17;
        let j = (h % ((i + 1) as u64)) as usize;
        items.swap(i, j);
    }
}

/// Pure set-intersection behind [`QbzCore::check_playlist_duplicates`] — split
/// out so the duplicate logic is unit-testable without a live Qobuz client.
/// `existing` = the playlist's current track ids; `track_ids` = the ids the
/// user wants to add. Returns the Tauri-shaped result (total checked, how many
/// are already present, and the set of those duplicate ids).
pub(crate) fn compute_playlist_duplicates(
    existing: &[u64],
    track_ids: &[u64],
) -> PlaylistDuplicateResult {
    let existing_set: std::collections::HashSet<u64> = existing.iter().copied().collect();
    let duplicate_track_ids: std::collections::HashSet<u64> = track_ids
        .iter()
        .copied()
        .filter(|id| existing_set.contains(id))
        .collect();
    PlaylistDuplicateResult {
        total_tracks: track_ids.len(),
        duplicate_count: duplicate_track_ids.len(),
        duplicate_track_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_search_all_filters_blacklisted_artist() {
        let json = serde_json::json!({
            "albums":  { "items": [], "total": 0, "offset": 0, "limit": 30 },
            "tracks":  { "items": [], "total": 0, "offset": 0, "limit": 30 },
            "artists": {
                "items": [
                    { "id": 1, "name": "Keep" },
                    { "id": 999, "name": "Blocked" }
                ],
                "total": 2, "offset": 0, "limit": 30
            },
            "playlists": { "items": [], "total": 0, "offset": 0, "limit": 30 }
        });
        let blocked: BlacklistFilter = [999].into_iter().collect();
        let out = parse_search_all(&json, &blocked);
        assert_eq!(out.artists.items.len(), 1);
        assert_eq!(out.artists.items[0].name, "Keep");
        assert_eq!(out.artists.total, 1);
        assert!(out.most_popular.is_none());
    }

    // --- D-FEAT featured-aware blacklist helpers ---

    use qbz_models::types::AlbumArtist;
    use qbz_models::{DiscoverAlbumImage, DiscoverArtist};

    // Album and Track do not derive Default in qbz-models, so test fixtures
    // construct full struct literals. Only the artist-id fields are meaningful
    // to the blacklist helpers; everything else is zero/None filler.
    fn album_with_artists(primary_id: u64, featured_ids: &[u64]) -> Album {
        let artists = std::iter::once(AlbumArtist {
            id: primary_id,
            name: String::new(),
            roles: Some(vec!["main-artist".to_string()]),
        })
        .chain(featured_ids.iter().map(|&id| AlbumArtist {
            id,
            name: String::new(),
            roles: Some(vec!["featured-artist".to_string()]),
        }))
        .collect();
        Album {
            id: String::new(),
            title: String::new(),
            artist: Artist {
                id: primary_id,
                ..Default::default()
            },
            image: Default::default(),
            release_date_original: None,
            release_date_stream: None,
            streamable: None,
            label: None,
            genre: None,
            tracks_count: None,
            duration: None,
            hires: false,
            hires_streamable: false,
            maximum_sampling_rate: None,
            maximum_bit_depth: None,
            audio_info: None,
            dates: None,
            track_count: None,
            release_type: None,
            tracks: None,
            upc: None,
            description: None,
            goodies: None,
            awards: None,
            parental_warning: None,
            artists: Some(artists),
        }
    }

    fn track_with(performer_id: Option<u64>, composer_id: Option<u64>) -> Track {
        Track {
            id: 0,
            title: String::new(),
            version: None,
            isrc: None,
            duration: 0,
            track_number: 0,
            media_number: None,
            performer: performer_id.map(|id| Artist {
                id,
                ..Default::default()
            }),
            album: None,
            hires: false,
            hires_streamable: false,
            maximum_sampling_rate: None,
            maximum_bit_depth: None,
            streamable: false,
            parental_warning: false,
            playlist_track_id: None,
            performers: None,
            composer: composer_id.map(|id| Artist {
                id,
                ..Default::default()
            }),
            copyright: None,
        }
    }

    #[test]
    fn album_blacklisted_blocks_on_primary_artist() {
        let album = album_with_artists(1, &[]);
        let bl: BlacklistFilter = [1].into_iter().collect();
        assert!(album_blacklisted(&album, &bl));
    }

    #[test]
    fn album_blacklisted_blocks_on_featured_not_primary() {
        // Primary is 1 (kept), featured 999 is blocked.
        let album = album_with_artists(1, &[999]);
        let bl: BlacklistFilter = [999].into_iter().collect();
        assert!(album_blacklisted(&album, &bl));
    }

    #[test]
    fn album_blacklisted_keeps_when_no_match() {
        let album = album_with_artists(1, &[2, 3]);
        let bl: BlacklistFilter = [999].into_iter().collect();
        assert!(!album_blacklisted(&album, &bl));
    }

    #[test]
    fn album_blacklisted_empty_filter_is_false() {
        let album = album_with_artists(1, &[999]);
        let bl: BlacklistFilter = BlacklistFilter::new();
        assert!(!album_blacklisted(&album, &bl));
    }

    #[test]
    fn track_blacklisted_blocks_on_performer() {
        let track = track_with(Some(5), None);
        let bl: BlacklistFilter = [5].into_iter().collect();
        assert!(track_blacklisted(&track, &bl));
    }

    #[test]
    fn track_blacklisted_blocks_on_composer() {
        let track = track_with(Some(1), Some(7));
        let bl: BlacklistFilter = [7].into_iter().collect();
        assert!(track_blacklisted(&track, &bl));
    }

    #[test]
    fn track_blacklisted_keeps_when_no_match() {
        let track = track_with(Some(1), Some(2));
        let bl: BlacklistFilter = [999].into_iter().collect();
        assert!(!track_blacklisted(&track, &bl));
    }

    #[test]
    fn track_blacklisted_fail_open_when_no_ids() {
        // No performer + no composer => kept (fail-open).
        let track = track_with(None, None);
        let bl: BlacklistFilter = [1, 2, 3].into_iter().collect();
        assert!(!track_blacklisted(&track, &bl));
    }

    #[test]
    fn track_blacklisted_empty_filter_is_false() {
        let track = track_with(Some(5), Some(7));
        let bl: BlacklistFilter = BlacklistFilter::new();
        assert!(!track_blacklisted(&track, &bl));
    }

    #[test]
    fn discover_album_blacklisted_blocks_on_any_artist() {
        let album = DiscoverAlbum {
            id: String::new(),
            title: String::new(),
            version: None,
            track_count: None,
            duration: None,
            parental_warning: None,
            image: DiscoverAlbumImage {
                small: None,
                thumbnail: None,
                large: None,
            },
            artists: vec![
                DiscoverArtist {
                    id: 1,
                    name: String::new(),
                    roles: None,
                },
                DiscoverArtist {
                    id: 999,
                    name: String::new(),
                    roles: None,
                },
            ],
            label: None,
            genre: None,
            dates: None,
            audio_info: None,
            awards: None,
        };
        let blocked: BlacklistFilter = [999].into_iter().collect();
        assert!(discover_album_blacklisted(&album, &blocked));
        let kept: BlacklistFilter = [555].into_iter().collect();
        assert!(!discover_album_blacklisted(&album, &kept));
    }

    #[test]
    fn compute_playlist_duplicates_intersects_input_with_existing() {
        // Existing playlist has 10, 20, 30. Adding 20, 30, 40, 50:
        // 20 and 30 are duplicates; 40 and 50 are new.
        let existing = [10u64, 20, 30];
        let to_add = [20u64, 30, 40, 50];
        let r = compute_playlist_duplicates(&existing, &to_add);
        assert_eq!(r.total_tracks, 4);
        assert_eq!(r.duplicate_count, 2);
        assert!(r.duplicate_track_ids.contains(&20));
        assert!(r.duplicate_track_ids.contains(&30));
        assert!(!r.duplicate_track_ids.contains(&40));
    }

    #[test]
    fn compute_playlist_duplicates_none_when_disjoint() {
        let r = compute_playlist_duplicates(&[1u64, 2, 3], &[4u64, 5]);
        assert_eq!(r.total_tracks, 2);
        assert_eq!(r.duplicate_count, 0);
        assert!(r.duplicate_track_ids.is_empty());
    }

    #[test]
    fn compute_playlist_duplicates_empty_input() {
        let r = compute_playlist_duplicates(&[1u64, 2, 3], &[]);
        assert_eq!(r.total_tracks, 0);
        assert_eq!(r.duplicate_count, 0);
    }
}
