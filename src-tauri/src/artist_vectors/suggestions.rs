//! Playlist suggestions engine
//!
//! Uses artist vectors to suggest new tracks for a playlist.
//! Algorithm:
//! 1. Extract unique artists from playlist tracks
//! 2. Compute combined playlist vector (sum + normalize)
//! 3. Find nearest artists not already in playlist
//! 4. Search Qobuz for top tracks by those artists
//! 5. Return suggested tracks with optional reasons

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::api::{QobuzClient, Track};

use super::builder::ArtistVectorBuilder;
use super::sparse_vector::SparseVector;
use super::store::ArtistVectorStore;

/// Configuration for suggestion generation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SuggestionConfig {
    /// Maximum number of artists to consider for suggestions
    pub max_artists: usize,
    /// Number of tracks to fetch per artist
    pub tracks_per_artist: usize,
    /// Maximum total tracks in the suggestion pool
    pub max_pool_size: usize,
    /// Maximum age (days) for vector freshness
    pub vector_max_age_days: i64,
    /// Minimum similarity score to include artist
    pub min_similarity: f32,
    /// Skip building vectors - only use existing cached vectors (faster but may have fewer results)
    pub skip_vector_build: bool,
}

impl Default for SuggestionConfig {
    fn default() -> Self {
        Self {
            max_artists: 30,           // Increased from 20 for more variety
            tracks_per_artist: 6,      // Increased from 5
            max_pool_size: 150,        // Increased from 100
            vector_max_age_days: 7,
            min_similarity: 0.1,
            skip_vector_build: false,
        }
    }
}

/// A suggested track with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedTrack {
    /// Qobuz track ID
    pub track_id: u64,
    /// Track title
    pub title: String,
    /// Artist name
    pub artist_name: String,
    /// Artist Qobuz ID (for navigation)
    pub artist_id: Option<u64>,
    /// Artist MBID (if known)
    pub artist_mbid: Option<String>,
    /// Album title
    pub album_title: String,
    /// Album ID for cover art
    pub album_id: String,
    /// Direct URL to album cover image
    pub album_image_url: Option<String>,
    /// Duration in seconds
    pub duration: u32,
    /// Similarity score (higher = more similar to playlist)
    pub similarity_score: f32,
    /// Reason for suggestion (for dev mode)
    pub reason: Option<String>,
}

/// Result of suggestion generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionResult {
    /// Pool of suggested tracks
    pub tracks: Vec<SuggestedTrack>,
    /// Artists that contributed to suggestions
    pub source_artists: Vec<String>,
    /// Number of playlist artists analyzed
    pub playlist_artists_count: usize,
    /// Number of similar artists found
    pub similar_artists_count: usize,
}

/// Playlist suggestions engine
pub struct SuggestionsEngine {
    /// Vector store for similarity lookups
    store: Arc<Mutex<Option<ArtistVectorStore>>>,
    /// Vector builder for lazy construction
    builder: Arc<ArtistVectorBuilder>,
    /// Qobuz client for track search
    qobuz_client: Arc<Mutex<QobuzClient>>,
    /// Configuration
    config: SuggestionConfig,
}

impl SuggestionsEngine {
    /// Create a new suggestions engine
    pub fn new(
        store: Arc<Mutex<Option<ArtistVectorStore>>>,
        builder: Arc<ArtistVectorBuilder>,
        qobuz_client: Arc<Mutex<QobuzClient>>,
        config: SuggestionConfig,
    ) -> Self {
        Self {
            store,
            builder,
            qobuz_client,
            config,
        }
    }

    /// Generate suggestions for a playlist
    ///
    /// # Arguments
    /// * `playlist_artists` - Artist info (MBID, name) from the playlist
    /// * `exclude_track_ids` - Track IDs to exclude (already in playlist)
    /// * `include_reasons` - Whether to include reason strings (dev mode)
    pub async fn generate_suggestions(
        &self,
        playlist_artists: &[(String, String)], // (mbid, name)
        exclude_track_ids: &HashSet<u64>,
        include_reasons: bool,
    ) -> Result<SuggestionResult, String> {
        use std::time::Instant;

        if playlist_artists.is_empty() {
            log::debug!("[SuggestionsEngine] Empty playlist, returning empty");
            return Ok(SuggestionResult {
                tracks: Vec::new(),
                source_artists: Vec::new(),
                playlist_artists_count: 0,
                similar_artists_count: 0,
            });
        }

        // Extract MBIDs for vector operations
        let playlist_artist_mbids: Vec<String> = playlist_artists.iter().map(|(mbid, _)| mbid.clone()).collect();

        // 1. Ensure vectors exist for playlist artists (skip if configured)
        let step1_start = Instant::now();
        if self.config.skip_vector_build {
            log::debug!("[SuggestionsEngine] Step 1: SKIPPED (skip_vector_build=true), using only cached vectors");
        } else {
            log::debug!("[SuggestionsEngine] Step 1: Ensuring vectors for {} artists", playlist_artists.len());
            for (i, (mbid, name)) in playlist_artists.iter().enumerate() {
                let artist_start = Instant::now();
                let _ = self
                    .builder
                    .ensure_vector(mbid, Some(name), None, self.config.vector_max_age_days)
                    .await;
                log::debug!("[SuggestionsEngine] ensure_vector {}/{} took {:?}", i + 1, playlist_artists.len(), artist_start.elapsed());
            }
            log::debug!("[SuggestionsEngine] Step 1 completed in {:?}", step1_start.elapsed());
        }

        // 2. Compute combined playlist vector
        log::debug!("[SuggestionsEngine] Step 2: Computing playlist vector");
        let step2_start = Instant::now();
        let playlist_vector = self.compute_playlist_vector(&playlist_artist_mbids).await?;
        log::debug!("[SuggestionsEngine] Step 2 completed in {:?}, vector empty={}", step2_start.elapsed(), playlist_vector.is_empty());

        if playlist_vector.is_empty() {
            log::warn!("[SuggestionsEngine] Playlist vector is empty, returning empty result");
            return Ok(SuggestionResult {
                tracks: Vec::new(),
                source_artists: Vec::new(),
                playlist_artists_count: playlist_artist_mbids.len(),
                similar_artists_count: 0,
            });
        }

        // 3. Find related artists (using direct relationships, not vector similarity)
        log::debug!("[SuggestionsEngine] Step 3: Finding related artists");
        let step3_start = Instant::now();
        let exclude_vec: Vec<String> = playlist_artist_mbids.to_vec();
        let similar_artists = {
            let guard__ = self.store.lock().await;
            let store = guard__.as_ref().ok_or("No active session - please log in")?;
            // Use direct relationship lookup instead of vector similarity
            // This finds members, collaborators, etc. from the MusicBrainz data
            store.get_all_related_artists(&playlist_artist_mbids, &exclude_vec, self.config.max_artists)?
        };
        log::debug!("[SuggestionsEngine] Step 3 completed in {:?}, found {} related artists", step3_start.elapsed(), similar_artists.len());

        let similar_artists_count = similar_artists.len();
        let mut source_artists = Vec::new();
        let mut all_tracks = Vec::new();

        // 4a. First, search for tracks by playlist artists themselves (highest relevance)
        log::info!("[SuggestionsEngine] Step 4a: Searching tracks for {} playlist artists", playlist_artists.len());
        let step4a_start = Instant::now();

        for (mbid, name) in playlist_artists {
            source_artists.push(name.clone());

            // Search Qobuz for tracks by this playlist artist (similarity = 1.0)
            // Fetch many more tracks since many might already be in playlist
            // For a playlist with 23 tracks, we need to search beyond those to find new ones
            let playlist_artist_limit = (self.config.tracks_per_artist * 5).max(30); // At least 30 tracks
            log::info!("[SuggestionsEngine] Step 4a: Searching for '{}' (MBID: {}) with limit {}", name, mbid, playlist_artist_limit);
            let tracks = self.search_artist_tracks_with_limit(mbid, Some(name), 1.0, playlist_artist_limit).await;
            log::info!("[SuggestionsEngine] Step 4a: Found {} tracks for '{}'", tracks.len(), name);

            let mut added = 0;
            let mut skipped = 0;
            for mut track in tracks {
                // Skip if already in playlist
                if exclude_track_ids.contains(&track.track_id) {
                    skipped += 1;
                    continue;
                }

                if include_reasons {
                    track.reason = Some(format!("More from {}", name));
                }

                all_tracks.push(track);
                added += 1;
            }
            log::info!("[SuggestionsEngine] Step 4a: Added {} tracks for '{}' ({} skipped as already in playlist)", added, name, skipped);
        }
        log::info!("[SuggestionsEngine] Step 4a completed in {:?}, got {} tracks from playlist artists", step4a_start.elapsed(), all_tracks.len());

        // 4b. Then search for tracks by related/similar artists
        log::debug!("[SuggestionsEngine] Step 4b: Searching tracks for {} related artists", similar_artists.len());
        let step4b_start = Instant::now();

        for (i, artist) in similar_artists.iter().enumerate() {
            if artist.similarity < self.config.min_similarity {
                continue;
            }

            if let Some(name) = &artist.name {
                if !source_artists.contains(name) {
                    source_artists.push(name.clone());
                }
            }

            // Search Qobuz for tracks by this related artist
            let tracks = self
                .search_artist_tracks(&artist.mbid, artist.name.as_deref(), artist.similarity)
                .await;

            for mut track in tracks {
                // Skip if already in playlist
                if exclude_track_ids.contains(&track.track_id) {
                    continue;
                }

                // Add reason if requested
                if include_reasons {
                    track.reason = Some(self.generate_reason(
                        &artist.mbid,
                        artist.name.as_deref(),
                        artist.similarity,
                        &playlist_artist_mbids,
                    ));
                }

                all_tracks.push(track);
            }

            // Stop if we have enough tracks
            if all_tracks.len() >= self.config.max_pool_size * 2 {
                log::debug!("[SuggestionsEngine] Reached extended pool size {} after {} related artists", all_tracks.len(), i + 1);
                break;
            }
        }
        log::debug!("[SuggestionsEngine] Step 4b completed in {:?}, got {} total tracks", step4b_start.elapsed(), all_tracks.len());

        // 4c. If pool is still small, use Qobuz's "similar artists" API as fallback
        // This gives us artists that definitely exist in Qobuz
        const MIN_TRACKS_BEFORE_QOBUZ_SIMILAR: usize = 20;
        if all_tracks.len() < MIN_TRACKS_BEFORE_QOBUZ_SIMILAR {
            log::info!(
                "[SuggestionsEngine] Step 4c: Pool too small ({}), fetching Qobuz similar artists",
                all_tracks.len()
            );
            let step4c_start = Instant::now();

            // Get client for Qobuz API calls
            let client = self.qobuz_client.lock().await;

            // For each playlist artist, get their Qobuz similar artists
            let mut qobuz_similar_ids: HashSet<u64> = HashSet::new();

            for (_mbid, name) in playlist_artists {
                // First, find the Qobuz artist ID for this playlist artist
                if let Some((qobuz_id, _)) = self.validate_qobuz_artist(&client, name).await {
                    // Get similar artists from Qobuz (up to 10 per playlist artist)
                    match client.get_similar_artists(qobuz_id, 10, 0).await {
                        Ok(similar_page) => {
                            for similar_artist in similar_page.items {
                                // Skip if we've already processed this artist
                                if qobuz_similar_ids.contains(&similar_artist.id) {
                                    continue;
                                }
                                qobuz_similar_ids.insert(similar_artist.id);

                                // Check genre compatibility
                                if self.has_incompatible_genre(&client, similar_artist.id, &similar_artist.name).await {
                                    log::debug!(
                                        "[SuggestionsEngine] Skipping Qobuz similar '{}' - incompatible genre",
                                        similar_artist.name
                                    );
                                    continue;
                                }

                                if !source_artists.contains(&similar_artist.name) {
                                    source_artists.push(similar_artist.name.clone());
                                }

                                // Search tracks for this similar artist (use empty MBID since we have Qobuz ID)
                                let tracks = self.search_artist_tracks_by_qobuz_id(
                                    &client,
                                    similar_artist.id,
                                    &similar_artist.name,
                                    0.8, // High similarity since Qobuz says they're similar
                                ).await;

                                for mut track in tracks {
                                    if exclude_track_ids.contains(&track.track_id) {
                                        continue;
                                    }

                                    if include_reasons {
                                        track.reason = Some(format!("Similar to {} (Qobuz)", name));
                                    }

                                    all_tracks.push(track);
                                }

                                // Stop if we have enough
                                if all_tracks.len() >= self.config.max_pool_size {
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!(
                                "[SuggestionsEngine] Failed to get Qobuz similar artists for '{}': {}",
                                name, e
                            );
                        }
                    }
                }

                if all_tracks.len() >= self.config.max_pool_size {
                    break;
                }
            }

            log::debug!(
                "[SuggestionsEngine] Step 4c completed in {:?}, now have {} tracks from {} Qobuz similar artists",
                step4c_start.elapsed(),
                all_tracks.len(),
                qobuz_similar_ids.len()
            );
        }

        // 5. Deduplicate by title+artist (keeps highest similarity version)
        let mut seen_titles: HashSet<String> = HashSet::new();
        all_tracks.retain(|track| {
            let key = format!("{}|{}", track.title.to_lowercase(), track.artist_name.to_lowercase());
            seen_titles.insert(key)
        });

        // 6. Shuffle tracks for variety (so same artist doesn't dominate)
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        all_tracks.shuffle(&mut rng);

        // 7. Limit pool size
        all_tracks.truncate(self.config.max_pool_size);

        Ok(SuggestionResult {
            tracks: all_tracks,
            source_artists,
            playlist_artists_count: playlist_artist_mbids.len(),
            similar_artists_count,
        })
    }

    /// Compute combined vector for playlist artists
    async fn compute_playlist_vector(
        &self,
        artist_mbids: &[String],
    ) -> Result<SparseVector, String> {
        let mut combined = SparseVector::new();
        let guard__ = self.store.lock().await;
        let store = guard__.as_ref().ok_or("No active session - please log in")?;

        for mbid in artist_mbids {
            if let Some(vector) = store.get_vector(mbid) {
                combined = combined.add(&vector);
            }
        }

        // Normalize to unit length
        Ok(combined.normalize())
    }

    /// Search Qobuz for tracks by an artist (uses default tracks_per_artist limit)
    async fn search_artist_tracks(
        &self,
        artist_mbid: &str,
        artist_name: Option<&str>,
        similarity: f32,
    ) -> Vec<SuggestedTrack> {
        self.search_artist_tracks_with_limit(artist_mbid, artist_name, similarity, self.config.tracks_per_artist).await
    }

    /// Search Qobuz for tracks by Qobuz artist ID (more reliable when we already validated the artist)
    /// Takes client reference to avoid deadlock when caller already holds the lock
    async fn search_artist_tracks_by_qobuz_id(
        &self,
        client: &QobuzClient,
        qobuz_artist_id: u64,
        artist_name: &str,
        similarity: f32,
    ) -> Vec<SuggestedTrack> {
        let limit = self.config.tracks_per_artist;

        // Search by artist name but verify tracks belong to this specific Qobuz artist ID
        match client.search_tracks(artist_name, (limit * 3) as u32, 0, None).await {
            Ok(results) => {
                let mut tracks = Vec::new();

                for item in results.items {
                    // Only accept tracks from this exact artist (by ID)
                    let performer_id = item.performer.as_ref().map(|p| p.id);
                    if performer_id != Some(qobuz_artist_id) {
                        continue;
                    }

                    tracks.push(self.track_to_suggested_with_qobuz_id(&item, qobuz_artist_id, similarity));
                    if tracks.len() >= limit {
                        break;
                    }
                }

                tracks
            }
            Err(e) => {
                log::warn!("Failed to search tracks for {} (Qobuz ID {}): {}", artist_name, qobuz_artist_id, e);
                Vec::new()
            }
        }
    }

    /// Convert a Track to a SuggestedTrack (using Qobuz artist ID instead of MBID)
    fn track_to_suggested_with_qobuz_id(&self, track: &Track, _qobuz_artist_id: u64, similarity: f32) -> SuggestedTrack {
        let (album_title, album_id, album_image_url) = match &track.album {
            Some(album) => {
                let image_url = album.image.thumbnail
                    .as_ref()
                    .or(album.image.small.as_ref())
                    .or(album.image.large.as_ref())
                    .cloned();
                (album.title.clone(), album.id.clone(), image_url)
            }
            None => (String::new(), String::new(), None),
        };

        let (track_artist, artist_id) = match &track.performer {
            Some(p) => (p.name.clone(), Some(p.id)),
            None => (String::new(), None),
        };

        SuggestedTrack {
            track_id: track.id,
            title: track.title.clone(),
            artist_name: track_artist,
            artist_id,
            artist_mbid: None, // No MBID for Qobuz-sourced similar artists
            album_title,
            album_id,
            album_image_url,
            duration: track.duration,
            similarity_score: similarity,
            reason: None,
        }
    }

    /// Search Qobuz for tracks by an artist with custom limit
    ///
    /// First validates that the artist EXISTS in Qobuz (has a dedicated artist page).
    /// This prevents false matches for session musicians who don't have their own catalog
    /// (e.g., "Martin Lopez" drummer returning tracks from unrelated "Martin Lopez" artists).
    async fn search_artist_tracks_with_limit(
        &self,
        artist_mbid: &str,
        artist_name: Option<&str>,
        similarity: f32,
        limit: usize,
    ) -> Vec<SuggestedTrack> {
        let search_query = match artist_name {
            Some(name) => name.to_string(),
            None => {
                // Try to get name from store
                let guard__ = self.store.lock().await;
                if let Some(store) = guard__.as_ref() {
                    store
                        .get_artist_name(artist_mbid)
                        .unwrap_or_else(|| artist_mbid.to_string())
                } else {
                    artist_mbid.to_string()
                }
            }
        };

        let client = self.qobuz_client.lock().await;

        // Step 1: Validate artist exists in Qobuz with their own catalog
        // This prevents searching for session musicians who don't have artist pages
        let validated_artist = self.validate_qobuz_artist(&client, &search_query).await;

        if validated_artist.is_none() {
            log::info!(
                "[SuggestionsEngine] Skipping '{}' - no Qobuz artist page found or incompatible genre",
                search_query
            );
            return Vec::new();
        }

        let (qobuz_artist_id, qobuz_artist_name) = validated_artist.unwrap();
        log::info!(
            "[SuggestionsEngine] Validated '{}' -> Qobuz artist '{}' (ID: {})",
            search_query, qobuz_artist_name, qobuz_artist_id
        );

        // Step 2: Search for tracks by artist name
        // Fetch many more since search results include tracks where the artist appears,
        // not just tracks BY the artist. We filter down to exact matches.
        let search_limit = ((limit * 5) as u32).max(100).min(500); // Between 100-500
        match client
            .search_tracks(&search_query, search_limit, 0, None)
            .await
        {
            Ok(results) => {
                let mut tracks = Vec::new();

                for item in results.items {
                    // Verify the track's performer matches the validated Qobuz artist
                    // Use both ID matching (best) and name matching (fallback)
                    let performer_id = item.performer.as_ref().map(|p| p.id);
                    let performer_name = item
                        .performer
                        .as_ref()
                        .map(|p| p.name.clone())
                        .unwrap_or_default();

                    // Prefer ID match (exact), fall back to name comparison
                    let is_match = performer_id == Some(qobuz_artist_id)
                        || names_similar(&performer_name, &qobuz_artist_name);

                    if is_match {
                        tracks.push(self.track_to_suggested(&item, artist_mbid, similarity));
                        if tracks.len() >= limit {
                            break;
                        }
                    }
                }

                tracks
            }
            Err(e) => {
                log::warn!("Failed to search tracks for {}: {}", search_query, e);
                Vec::new()
            }
        }
    }

    /// Validate that an artist exists in Qobuz with their own catalog AND compatible genre
    ///
    /// Returns Some((artist_id, artist_name)) if found, None otherwise.
    /// This prevents false matches for:
    /// - Session musicians without their own page (e.g., "Martin Lopez" drummer)
    /// - Names that match different artists (e.g., Latin "Martin Mendez" vs bassist)
    /// - Artists with incompatible genres (bachata/merengue artist vs metal drummer)
    async fn validate_qobuz_artist(
        &self,
        client: &QobuzClient,
        name: &str,
    ) -> Option<(u64, String)> {
        // Normalize name for comparison (removes accents: å→a, é→e, etc.)
        let name_normalized = normalize_name(name);

        // Search Qobuz for artist - try original name first
        let mut results = match client.search_artists(name, 10, 0, None).await {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[SuggestionsEngine] Artist search failed for '{}': {}", name, e);
                return None;
            }
        };

        // If no results and name has accents, also try normalized name
        // e.g., "Mikael Åkerfeldt" -> "Mikael Akerfeldt"
        if results.items.is_empty() && name != name_normalized {
            log::debug!(
                "[SuggestionsEngine] No results for '{}', trying normalized '{}'",
                name, name_normalized
            );
            if let Ok(r) = client.search_artists(&name_normalized, 10, 0, None).await {
                results = r;
            }
        }

        // Look for exact name match (comparing normalized versions)
        let mut candidate: Option<(u64, String)> = None;

        for artist in &results.items {
            let artist_normalized = normalize_name(&artist.name);

            // Exact match (after accent normalization)
            // This allows "Mikael Åkerfeldt" to match "Mikael Akerfeldt"
            if artist_normalized == name_normalized && artist.albums_count.unwrap_or(0) > 0 {
                candidate = Some((artist.id, artist.name.clone()));
                break;
            }
        }

        // Also try "The X" variant (e.g., "Beatles" -> "The Beatles")
        if candidate.is_none() {
            let the_name_normalized = format!("the {}", name_normalized);
            for artist in &results.items {
                let artist_normalized = normalize_name(&artist.name);
                if artist_normalized == the_name_normalized && artist.albums_count.unwrap_or(0) > 0 {
                    candidate = Some((artist.id, artist.name.clone()));
                    break;
                }
            }
        }

        // If we found a candidate, verify their genre is compatible
        if let Some((artist_id, artist_name)) = candidate {
            if self.has_incompatible_genre(client, artist_id, &artist_name).await {
                log::info!(
                    "[SuggestionsEngine] Rejecting '{}' (ID: {}) - incompatible genre detected",
                    artist_name, artist_id
                );
                return None;
            }
            return Some((artist_id, artist_name));
        }

        None
    }

    /// Check if an artist has incompatible genres (bachata, merengue, k-pop, etc.)
    ///
    /// Fetches a few albums and checks their genres against a blocklist.
    /// Returns true if incompatible, false if compatible or unknown.
    async fn has_incompatible_genre(&self, client: &QobuzClient, artist_id: u64, artist_name: &str) -> bool {
        // Incompatible genre keywords - these would never appear in a rock/metal context
        // NOTE: We force English locale when fetching, so only English names needed
        const INCOMPATIBLE_GENRES: &[&str] = &[
            // Latin/Tropical
            "bachata", "merengue", "reggaeton", "salsa", "cumbia", "vallenato",
            "latin pop", "latin music", "tropical", "urbano", "regional mexican",
            "latin",  // Generic Latin parent genre
            // Asian pop
            "k-pop", "kpop", "j-pop", "jpop", "mandopop", "cantopop", "c-pop",
            // European folk/schlager
            "schlager", "chanson", "french chanson", "volksmusik",
            // Religious
            "gospel", "christian", "worship", "religious", "spiritual",
            // Children/Family
            "children", "nursery", "lullaby", "kids",
            // Electronic/Dance (club-oriented)
            "trance", "techno", "house", "edm", "dubstep", "drum and bass",
            "hardstyle", "eurodance", "hands up", "happy hardcore", "dance",
            // Spoken word/Non-music
            "audiobook", "spoken word", "podcast", "meditation", "asmr",
            "relaxation", "sleep", "nature sounds", "white noise",
            "comedy", "stand-up",
            // Country (usually incompatible with metal)
            "country", "bluegrass", "americana",
            // New age/Wellness
            "new age", "healing", "spa", "yoga", "mindfulness", "wellness",
        ];

        // Fetch artist with a few albums (use English locale for consistent genre names)
        match client.get_artist_with_pagination_and_locale(artist_id, true, Some(5), None, Some("en")).await {
            Ok(artist) => {
                if let Some(albums) = &artist.albums {
                    for album in &albums.items {
                        if let Some(genre) = &album.genre {
                            let genre_lower = genre.name.to_lowercase();

                            // Check if genre matches any incompatible keyword
                            for incompatible in INCOMPATIBLE_GENRES {
                                if genre_lower.contains(incompatible) {
                                    log::debug!(
                                        "[SuggestionsEngine] Artist '{}' has incompatible genre: '{}' (album: {})",
                                        artist_name, genre.name, album.title
                                    );
                                    return true;
                                }
                            }
                        }

                        // Also check album title for genre hints (e.g., "Latino Bachata Amor")
                        let title_lower = album.title.to_lowercase();
                        for incompatible in INCOMPATIBLE_GENRES {
                            if title_lower.contains(incompatible) {
                                log::debug!(
                                    "[SuggestionsEngine] Artist '{}' has incompatible album title: '{}'",
                                    artist_name, album.title
                                );
                                return true;
                            }
                        }

                        // Additional title-based checks for non-music content
                        const INCOMPATIBLE_TITLE_KEYWORDS: &[&str] = &[
                            "audiobook", "hörbuch", "hörspiel", "gelesen von", "read by",
                            "narrated by", "lesung", "märchen", "fairy tale",
                            "meditation", "relaxation", "sleep music", "yoga music",
                            "trance mix", "club mix", "dance mix", "dj mix",
                        ];
                        for keyword in INCOMPATIBLE_TITLE_KEYWORDS {
                            if title_lower.contains(keyword) {
                                log::debug!(
                                    "[SuggestionsEngine] Artist '{}' has incompatible album title keyword '{}': '{}'",
                                    artist_name, keyword, album.title
                                );
                                return true;
                            }
                        }
                    }
                }
                false
            }
            Err(e) => {
                log::warn!(
                    "[SuggestionsEngine] Failed to fetch albums for genre check ({}): {}",
                    artist_name, e
                );
                // On error, don't block - let it through
                false
            }
        }
    }

    /// Convert a Track to a SuggestedTrack
    fn track_to_suggested(&self, track: &Track, artist_mbid: &str, similarity: f32) -> SuggestedTrack {
        // Extract album info including image URL
        let (album_title, album_id, album_image_url) = match &track.album {
            Some(album) => {
                let image_url = album.image.thumbnail
                    .as_ref()
                    .or(album.image.small.as_ref())
                    .or(album.image.large.as_ref())
                    .cloned();
                (album.title.clone(), album.id.clone(), image_url)
            }
            None => (String::new(), String::new(), None),
        };

        // Extract artist name and ID from track performer
        let (track_artist, artist_id) = match &track.performer {
            Some(p) => (p.name.clone(), Some(p.id)),
            None => (String::new(), None),
        };

        SuggestedTrack {
            track_id: track.id,
            title: track.title.clone(),
            artist_name: track_artist,
            artist_id,
            artist_mbid: Some(artist_mbid.to_string()),
            album_title,
            album_id,
            album_image_url,
            duration: track.duration,
            similarity_score: similarity,
            reason: None,
        }
    }

    /// Generate a human-readable reason for suggestion
    fn generate_reason(
        &self,
        _artist_mbid: &str,
        artist_name: Option<&str>,
        similarity: f32,
        _playlist_artists: &[String],
    ) -> String {
        let name = artist_name.unwrap_or("Unknown");
        let score_pct = (similarity * 100.0).round() as u32;

        format!("Similar to your playlist ({score_pct}% match) - {name}")
    }
}

/// Normalize a name for comparison (remove accents, lowercase)
fn normalize_name(name: &str) -> String {
    name.to_lowercase()
        .replace('á', "a").replace('é', "e").replace('í', "i").replace('ó', "o").replace('ú', "u")
        .replace('à', "a").replace('è', "e").replace('ì', "i").replace('ò', "o").replace('ù', "u")
        .replace('ä', "a").replace('ë', "e").replace('ï', "i").replace('ö', "o").replace('ü', "u")
        .replace('ñ', "n").replace('ç', "c")
}

/// Check if two artist names are similar enough to be considered a match
///
/// STRICT matching to prevent false positives like:
/// - "Martín Méndez" matching "Tomas Martin Lopez" (share "Martin")
/// - "Martín Méndez" matching "Martin Mendez" (different person, same name)
///
/// For person names (2-3 words), we require ALL words to match.
/// This handles "George Harrison" vs "Harrison, George" but rejects partial matches.
fn names_similar(name1: &str, name2: &str) -> bool {
    let norm1 = normalize_name(name1);
    let norm2 = normalize_name(name2);

    // Exact match after normalization
    if norm1 == norm2 {
        return true;
    }

    // Split into words
    let words1: HashSet<&str> = norm1.split_whitespace().collect();
    let words2: HashSet<&str> = norm2.split_whitespace().collect();

    if words1.is_empty() || words2.is_empty() {
        return false;
    }

    // Count matching words
    let matches = words1.intersection(&words2).count();
    let max_words = words1.len().max(words2.len());
    let min_words = words1.len().min(words2.len());

    // VERY STRICT for person names:
    // - For 2-word names: require EXACT same words (handles "George Harrison" vs "Harrison, George")
    // - For 3-word names: allow at most 1 extra word
    // - This rejects "Martin Lopez" vs "Tomas Martin Lopez" (different people)
    if min_words == 2 {
        // For 2-word names, require EXACTLY the same words (just different order allowed)
        // "Martin Lopez" vs "Tomas Martin Lopez" -> max_words=3, min_words=2 -> REJECT
        // "George Harrison" vs "Harrison, George" -> max_words=2, min_words=2 -> ACCEPT
        matches == min_words && max_words == min_words
    } else if min_words == 3 {
        // For 3-word names, allow at most 1 extra word
        matches >= min_words && (max_words - min_words) <= 1
    } else {
        // For longer names (bands, etc.), allow some flexibility
        matches as f32 / max_words as f32 >= 0.75
    }
}

/// Extract unique artist MBIDs from playlist tracks
///
/// This is a helper function that should be called from the Tauri command
/// to get artist MBIDs from track data.
pub fn extract_artist_mbids(
    tracks: &[(u64, Option<String>)], // (track_id, artist_mbid)
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut mbids = Vec::new();

    for (_, mbid) in tracks {
        if let Some(id) = mbid {
            if !id.is_empty() && seen.insert(id.clone()) {
                mbids.push(id.clone());
            }
        }
    }

    mbids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_artist_mbids() {
        let tracks = vec![
            (1, Some("mbid-1".to_string())),
            (2, Some("mbid-2".to_string())),
            (3, Some("mbid-1".to_string())), // Duplicate
            (4, None),                        // No MBID
            (5, Some("".to_string())),        // Empty MBID
            (6, Some("mbid-3".to_string())),
        ];

        let mbids = extract_artist_mbids(&tracks);

        assert_eq!(mbids.len(), 3);
        assert!(mbids.contains(&"mbid-1".to_string()));
        assert!(mbids.contains(&"mbid-2".to_string()));
        assert!(mbids.contains(&"mbid-3".to_string()));
    }

    #[test]
    fn test_suggestion_config_default() {
        let config = SuggestionConfig::default();

        assert_eq!(config.max_artists, 30);
        assert_eq!(config.tracks_per_artist, 6);
        assert_eq!(config.max_pool_size, 150);
        assert_eq!(config.vector_max_age_days, 7);
        assert!(config.min_similarity > 0.0);
    }
}
