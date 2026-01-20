//! Playback context module
//!
//! Manages the semantic origin of playback to determine what "next" means.
//! A playback context is NOT the queue - it's the source that defines
//! the boundaries and continuation behavior for playback.
//!
//! Examples:
//! - Album: next track in the album
//! - Playlist: next track in the playlist
//! - Artist Top Songs: next ranked track
//! - Single track: no context, playback ends when track finishes

use std::sync::Mutex;
use serde::{Deserialize, Serialize};

/// Type of playback context
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextType {
    Album,
    Playlist,
    ArtistTop,
    HomeList,
    Favorites,
    LocalLibrary,
}

/// Source of the content (Qobuz or Local Library)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentSource {
    Qobuz,
    Local,
}

/// Playback context - the semantic origin of playback
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackContext {
    /// Type of context (album, playlist, etc.)
    #[serde(rename = "type")]
    pub context_type: ContextType,
    /// Unique identifier for this context
    pub id: String,
    /// Human-readable label
    pub label: String,
    /// Source of content
    pub source: ContentSource,
    /// Original track list for this context (track IDs)
    pub track_ids: Vec<u64>,
    /// Current position in the context
    pub current_position: usize,
}

impl PlaybackContext {
    /// Create a new playback context
    pub fn new(
        context_type: ContextType,
        id: String,
        label: String,
        source: ContentSource,
        track_ids: Vec<u64>,
        start_position: usize,
    ) -> Self {
        Self {
            context_type,
            id,
            label,
            source,
            track_ids,
            current_position: start_position,
        }
    }

    /// Get the next track ID in this context (if any)
    pub fn next_track_id(&self) -> Option<u64> {
        let next_pos = self.current_position + 1;
        if next_pos < self.track_ids.len() {
            self.track_ids.get(next_pos).copied()
        } else {
            None
        }
    }

    /// Get multiple upcoming track IDs in this context
    pub fn upcoming_track_ids(&self, count: usize) -> Vec<u64> {
        let start_pos = self.current_position + 1;
        self.track_ids
            .iter()
            .skip(start_pos)
            .take(count)
            .copied()
            .collect()
    }

    /// Advance to the next track in context
    pub fn advance(&mut self) -> bool {
        let next_pos = self.current_position + 1;
        if next_pos < self.track_ids.len() {
            self.current_position = next_pos;
            true
        } else {
            false
        }
    }

    /// Check if context has more tracks
    pub fn has_next(&self) -> bool {
        self.current_position + 1 < self.track_ids.len()
    }

    /// Get total tracks in context
    pub fn total_tracks(&self) -> usize {
        self.track_ids.len()
    }

    /// Get context info for display
    pub fn display_info(&self) -> String {
        let type_str = match self.context_type {
            ContextType::Album => "Album",
            ContextType::Playlist => "Playlist",
            ContextType::ArtistTop => "Artist Top Songs",
            ContextType::HomeList => "Home List",
            ContextType::Favorites => "Favorites",
            ContextType::LocalLibrary => "Local Library",
        };
        format!("{} · {}", type_str, self.label)
    }
}

/// Manager for the current playback context
pub struct ContextManager {
    current: Mutex<Option<PlaybackContext>>,
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextManager {
    pub fn new() -> Self {
        Self {
            current: Mutex::new(None),
        }
    }

    /// Set the current playback context
    pub fn set_context(&self, context: PlaybackContext) {
        let mut current = self.current.lock().unwrap();
        *current = Some(context);
        log::info!("Playback context set: {:?}", current.as_ref().map(|c| &c.label));
    }

    /// Clear the current context (for single track playback)
    pub fn clear_context(&self) {
        let mut current = self.current.lock().unwrap();
        *current = None;
        log::info!("Playback context cleared");
    }

    /// Get the current context (cloned)
    pub fn get_context(&self) -> Option<PlaybackContext> {
        self.current.lock().unwrap().clone()
    }

    /// Check if a context is active
    pub fn has_context(&self) -> bool {
        self.current.lock().unwrap().is_some()
    }

    /// Get the next track ID from the current context
    pub fn next_track_id(&self) -> Option<u64> {
        self.current
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|ctx| ctx.next_track_id())
        }

    /// Get multiple upcoming track IDs from the current context
    pub fn upcoming_track_ids(&self, count: usize) -> Vec<u64> {
        self.current
            .lock()
            .unwrap()
            .as_ref()
            .map(|ctx| ctx.upcoming_track_ids(count))
            .unwrap_or_default()
    }

    /// Advance the context to the next track
    pub fn advance_context(&self) -> bool {
        let mut current = self.current.lock().unwrap();
        if let Some(ctx) = current.as_mut() {
            let advanced = ctx.advance();
            if !advanced {
                log::info!("Playback context ended (no more tracks)");
            }
            advanced
        } else {
            false
        }
    }

    /// Update current track position in context (when user seeks within context)
    pub fn set_position(&self, track_id: u64) {
        let mut current = self.current.lock().unwrap();
        if let Some(ctx) = current.as_mut() {
            if let Some(pos) = ctx.track_ids.iter().position(|&id| id == track_id) {
                ctx.current_position = pos;
                log::debug!("Context position updated to {}", pos);
            }
        }
    }
}
