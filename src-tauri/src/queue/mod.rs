//! Queue management module
//!
//! # DEPRECATED - LEGACY CODE
//!
//! **This module is deprecated and will be removed.**
//!
//! Use `qbz-player::QueueManager` instead. The V2 architecture routes queue ops through:
//! - `CoreBridge` -> `QbzCore` -> `qbz_player::QueueManager`
//!
//! This legacy module remains only as emergency fallback during migration.
//! Do NOT add new functionality here. All new queue code goes to `qbz-player`.
//!
//! ## Migration Status
//! - [x] QueueManager exists in qbz-player
//! - [x] QbzCore uses qbz_player::QueueManager
//! - [x] V2 queue commands use CoreBridge
//! - [ ] Full frontend migration to V2 commands
//!
//! ---
//!
//! Handles playback queue with:
//! - Queue manipulation (add, remove, reorder, clear)
//! - Current track tracking
//! - Shuffle mode
//! - Repeat modes (off, all, one)
//! - Play history for going back

use std::collections::VecDeque;
use std::sync::Mutex;

/// Track info stored in the queue
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueueTrack {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub hires: bool,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<f64>,
    /// Whether this is a local library track (not from Qobuz)
    #[serde(default)]
    pub is_local: bool,
    /// Album ID for navigation (Qobuz album ID)
    pub album_id: Option<String>,
    /// Artist ID for navigation (Qobuz artist ID)
    pub artist_id: Option<u64>,
    /// Whether the track is streamable on Qobuz (false = removed/unavailable)
    #[serde(default = "default_streamable")]
    pub streamable: bool,
    /// Optional origin source (e.g. "qobuz", "local", "plex")
    #[serde(default)]
    pub source: Option<String>,
}

fn default_streamable() -> bool {
    true // Default to true for backwards compatibility with existing queue data
}

/// Repeat mode options
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl Default for RepeatMode {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(Debug, PartialEq, Eq)]
enum QueueMoveDirection {
    Up,
    Down,
}

/// Queue state snapshot for frontend
#[derive(Debug, Clone, serde::Serialize)]
pub struct QueueState {
    pub current_track: Option<QueueTrack>,
    pub current_index: Option<usize>,
    pub upcoming: Vec<QueueTrack>,
    pub history: Vec<QueueTrack>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub total_tracks: usize,
}

/// Internal queue state - all in one struct to avoid deadlocks
struct InternalState {
    /// All tracks in the queue (original order)
    tracks: Vec<QueueTrack>,
    /// Current playback index
    current_index: Option<usize>,
    /// Shuffle mode enabled
    shuffle: bool,
    /// Shuffled indices (when shuffle is on)
    shuffle_order: Vec<usize>,
    /// Position in shuffle order
    shuffle_position: usize,
    /// Repeat mode
    repeat: RepeatMode,
    /// History of played track indices (for going back)
    history: VecDeque<usize>,
}

/// Queue manager for handling playback queue
pub struct QueueManager {
    state: Mutex<InternalState>,
}

impl Default for QueueManager {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(InternalState {
                tracks: Vec::new(),
                current_index: None,
                shuffle: false,
                shuffle_order: Vec::new(),
                shuffle_position: 0,
                repeat: RepeatMode::Off,
                history: VecDeque::with_capacity(50),
            }),
        }
    }

    /// Add a track to the end of the queue
    pub fn add_track(&self, track: QueueTrack) {
        let mut state = self.state.lock().unwrap();
        state.tracks.push(track);

        if state.shuffle {
            let new_idx = state.tracks.len() - 1;
            state.shuffle_order.push(new_idx);
        }
    }

    /// Add multiple tracks to the queue
    pub fn add_tracks(&self, new_tracks: Vec<QueueTrack>) {
        let mut state = self.state.lock().unwrap();
        let start_idx = state.tracks.len();
        state.tracks.extend(new_tracks);

        if state.shuffle {
            for i in start_idx..state.tracks.len() {
                state.shuffle_order.push(i);
            }
        }
    }

    /// Add a track to play next (after current index if set)
    pub fn add_track_next(&self, track: QueueTrack) {
        let mut state = self.state.lock().unwrap();
        let insert_index = state.current_index.map(|idx| idx + 1).unwrap_or(0);

        if insert_index >= state.tracks.len() {
            state.tracks.push(track);
        } else {
            state.tracks.insert(insert_index, track);
        }

        if state.shuffle {
            for idx in state.shuffle_order.iter_mut() {
                if *idx >= insert_index {
                    *idx += 1;
                }
            }

            let new_idx = insert_index;
            let next_pos = if state.current_index.is_some() {
                state.shuffle_position + 1
            } else {
                state.shuffle_order.len()
            };

            if next_pos >= state.shuffle_order.len() {
                state.shuffle_order.push(new_idx);
            } else {
                state.shuffle_order.insert(next_pos, new_idx);
            }
        }
    }

    /// Set the entire queue (replaces existing)
    pub fn set_queue(&self, new_tracks: Vec<QueueTrack>, start_index: Option<usize>) {
        let mut state = self.state.lock().unwrap();
        state.tracks = new_tracks;
        state.current_index = start_index;
        state.history.clear();

        // Regenerate shuffle order
        Self::regenerate_shuffle_order_internal(&mut state);

        // CRITICAL FIX: When shuffle is enabled and we have a start_index,
        // ensure the start_index track is at the BEGINNING of shuffle order
        // This fixes the bug where shuffle shows incomplete queue
        if state.shuffle {
            if let Some(start_idx) = start_index {
                if start_idx < state.tracks.len() {
                    // Find where start_idx is in the current shuffle_order
                    if let Some(pos) = state.shuffle_order.iter().position(|&x| x == start_idx) {
                        // Move it to the front by swapping
                        state.shuffle_order.swap(0, pos);
                        // Set shuffle position to 0 so we start from the beginning
                        state.shuffle_position = 0;

                        log::info!(
                            "Queue: Adjusted shuffle order to start with track index {} (was at position {})",
                            start_idx,
                            pos
                        );
                    }
                }
            }
        }
    }

    /// Clear the queue
    pub fn clear(&self) {
        let mut state = self.state.lock().unwrap();

        // A track is currently playing, keep it as the only item left.
        if let Some(_curr_idx) = state.current_index {
            state.tracks.truncate(1);
            state.current_index = Some(0);
        } else {
            state.tracks.clear();
            state.current_index = None;
        }

        state.shuffle_order.clear();
        state.shuffle_position = 0;
        // Keep playback history when clearing queue.
        // UX expectation: "Clear queue" only affects current/upcoming queue items.
    }

    /// Remove a track by index
    pub fn remove_track(&self, index: usize) -> Option<QueueTrack> {
        let mut state = self.state.lock().unwrap();
        if index >= state.tracks.len() {
            return None;
        }

        let removed = state.tracks.remove(index);

        // Adjust current index if needed
        if let Some(curr_idx) = state.current_index {
            if index < curr_idx {
                state.current_index = Some(curr_idx - 1);
            } else if index == curr_idx {
                if curr_idx >= state.tracks.len() {
                    state.current_index = if state.tracks.is_empty() { None } else { Some(state.tracks.len() - 1) };
                }
            }
        }

        Self::regenerate_shuffle_order_internal(&mut state);
        Some(removed)
    }

    /// Remove a track by its position in the upcoming list (0 = first upcoming track)
    /// This handles shuffle mode correctly by mapping the upcoming index to the actual track index
    pub fn remove_upcoming_track(&self, upcoming_index: usize) -> Option<QueueTrack> {
        let mut state = self.state.lock().unwrap();

        // Calculate the actual track index based on the upcoming index
        // Must match how get_state() builds the upcoming list
        let actual_index = if state.shuffle {
            // In shuffle mode, upcoming is built from shuffle_order starting at shuffle_position + 1
            let shuffle_pos = state.shuffle_position + 1 + upcoming_index;
            if shuffle_pos >= state.shuffle_order.len() {
                return None;
            }
            state.shuffle_order[shuffle_pos]
        } else {
            // In normal mode:
            // - If current_index exists: upcoming starts at current_index + 1
            // - If current_index is None: upcoming starts at 0 (full queue)
            match state.current_index {
                Some(curr_idx) => curr_idx + 1 + upcoming_index,
                None => upcoming_index,
            }
        };

        if actual_index >= state.tracks.len() {
            return None;
        }

        log::info!("remove_upcoming_track: upcoming_index={} -> actual_index={}", upcoming_index, actual_index);

        let removed = state.tracks.remove(actual_index);

        // Adjust current index if needed
        if let Some(curr_idx) = state.current_index {
            if actual_index < curr_idx {
                state.current_index = Some(curr_idx - 1);
            } else if actual_index == curr_idx {
                if curr_idx >= state.tracks.len() {
                    state.current_index = if state.tracks.is_empty() { None } else { Some(state.tracks.len() - 1) };
                }
            }
        }

        Self::regenerate_shuffle_order_internal(&mut state);
        Some(removed)
    }

    /// Move a track from one position to another
    pub fn move_track(&self, from_index: usize, to_index: usize) -> bool {
        let mut state = self.state.lock().unwrap();

        let direction: QueueMoveDirection = if from_index > to_index {
            QueueMoveDirection::Up
        } else {
            QueueMoveDirection::Down
        };

        let mut from_idx = from_index;
        let mut to_idx = to_index;

        if let Some(curr_idx) = state.current_index {
            // map to the internal state, which differs from the frontend's
            // representation because the here we also have the current playing
            // track part of the tracks.
            from_idx = from_idx + curr_idx + 1;
            to_idx = to_idx + curr_idx + 1;
        }

        if direction == QueueMoveDirection::Down {
            // When moving tracks down this makes the new position
            // more intuitive (since we usually drop tracks in between)
            to_idx = to_idx - 1;
        }

        log::info!("Queue: move_track - {:?} from {} to {} (internal indices:{} -> {}). Tracks in queue: {}", direction, from_index, to_index, from_idx, to_idx, state.tracks.len());

        // Moving a track to its already existing position, just ignore
        if from_idx == to_idx {
            return true;
        }

        if from_idx >= state.tracks.len() || to_idx >= state.tracks.len() {
            return false;
        }

        let track = state.tracks.remove(from_idx);
        state.tracks.insert(to_idx, track);

        // Adjust current index if needed
        if let Some(curr_idx) = state.current_index {
            if from_idx == curr_idx {
                // The current track was moved
                state.current_index = Some(to_idx);
            } else if from_idx < curr_idx && to_idx >= curr_idx {
                // Track moved from before current to at/after current
                state.current_index = Some(curr_idx - 1);
            } else if from_idx > curr_idx && to_idx <= curr_idx {
                // Track moved from after current to at/before current
                state.current_index = Some(curr_idx + 1);
            }
        }

        Self::regenerate_shuffle_order_internal(&mut state);
        true
    }

    /// Get current track
    pub fn current_track(&self) -> Option<QueueTrack> {
        let state = self.state.lock().unwrap();
        state.current_index.and_then(|idx| state.tracks.get(idx).cloned())
    }

    /// Get next track without advancing
    pub fn peek_next(&self) -> Option<QueueTrack> {
        let state = self.state.lock().unwrap();
        if state.tracks.is_empty() {
            return None;
        }

        if state.repeat == RepeatMode::One {
            return state.current_index.and_then(|idx| state.tracks.get(idx).cloned());
        }

        let next_idx = if state.shuffle {
            let next_pos = state.shuffle_position + 1;
            if next_pos < state.shuffle_order.len() {
                Some(state.shuffle_order[next_pos])
            } else if state.repeat == RepeatMode::All {
                state.shuffle_order.first().copied()
            } else {
                None
            }
        } else {
            let curr_idx = state.current_index.unwrap_or(0);
            let next_idx = curr_idx + 1;
            if next_idx < state.tracks.len() {
                Some(next_idx)
            } else if state.repeat == RepeatMode::All {
                Some(0)
            } else {
                None
            }
        };

        next_idx.and_then(|idx| state.tracks.get(idx).cloned())
    }

    /// Get multiple upcoming tracks without advancing (for prefetching)
    pub fn peek_upcoming(&self, count: usize) -> Vec<QueueTrack> {
        let state = self.state.lock().unwrap();
        if state.tracks.is_empty() || count == 0 {
            return Vec::new();
        }

        // Don't return upcoming if repeat one (same track always)
        if state.repeat == RepeatMode::One {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(count);

        if state.shuffle {
            let start_pos = state.shuffle_position + 1;
            for i in 0..count {
                let pos = start_pos + i;
                if pos < state.shuffle_order.len() {
                    if let Some(track) = state.tracks.get(state.shuffle_order[pos]) {
                        result.push(track.clone());
                    }
                } else if state.repeat == RepeatMode::All {
                    // Wrap around
                    let wrapped_pos = pos % state.shuffle_order.len();
                    if let Some(track) = state.tracks.get(state.shuffle_order[wrapped_pos]) {
                        result.push(track.clone());
                    }
                }
            }
        } else {
            let start_idx = state.current_index.map(|i| i + 1).unwrap_or(0);
            for i in 0..count {
                let idx = start_idx + i;
                if idx < state.tracks.len() {
                    result.push(state.tracks[idx].clone());
                } else if state.repeat == RepeatMode::All {
                    // Wrap around
                    let wrapped_idx = idx % state.tracks.len();
                    result.push(state.tracks[wrapped_idx].clone());
                }
            }
        }

        result
    }

    /// Advance to next track and return it
    pub fn next(&self) -> Option<QueueTrack> {
        let mut state = self.state.lock().unwrap();
        if state.tracks.is_empty() {
            return None;
        }

        // Save current to history before moving
        if let Some(curr_idx) = state.current_index {
            state.history.push_back(curr_idx);
            while state.history.len() > 50 {
                state.history.pop_front();
            }
        }

        if state.repeat == RepeatMode::One {
            return state.current_index.and_then(|idx| state.tracks.get(idx).cloned());
        }

        let next_idx = if state.shuffle {
            state.shuffle_position += 1;
            if state.shuffle_position < state.shuffle_order.len() {
                Some(state.shuffle_order[state.shuffle_position])
            } else if state.repeat == RepeatMode::All {
                state.shuffle_position = 0;
                state.shuffle_order.first().copied()
            } else {
                None
            }
        } else {
            let curr_idx = state.current_index.unwrap_or(0);
            let next_idx = curr_idx + 1;
            if next_idx < state.tracks.len() {
                Some(next_idx)
            } else if state.repeat == RepeatMode::All {
                Some(0)
            } else {
                None
            }
        };

        state.current_index = next_idx;
        next_idx.and_then(|idx| state.tracks.get(idx).cloned())
    }

    /// Go to previous track and return it
    pub fn previous(&self) -> Option<QueueTrack> {
        let mut state = self.state.lock().unwrap();
        if state.tracks.is_empty() {
            return None;
        }

        // Try to get from history first
        if let Some(prev_idx) = state.history.pop_back() {
            state.current_index = Some(prev_idx);

            if state.shuffle {
                if let Some(pos) = state.shuffle_order.iter().position(|&x| x == prev_idx) {
                    state.shuffle_position = pos;
                }
            }

            return state.tracks.get(prev_idx).cloned();
        }

        // No history, go to previous in order
        let prev_idx = if state.shuffle {
            if state.shuffle_position > 0 {
                state.shuffle_position -= 1;
                Some(state.shuffle_order[state.shuffle_position])
            } else if state.repeat == RepeatMode::All {
                state.shuffle_position = state.shuffle_order.len().saturating_sub(1);
                state.shuffle_order.last().copied()
            } else {
                state.shuffle_order.first().copied()
            }
        } else {
            let curr_idx = state.current_index.unwrap_or(0);
            if curr_idx > 0 {
                Some(curr_idx - 1)
            } else if state.repeat == RepeatMode::All {
                Some(state.tracks.len().saturating_sub(1))
            } else {
                Some(0)
            }
        };

        state.current_index = prev_idx;
        prev_idx.and_then(|idx| state.tracks.get(idx).cloned())
    }

    /// Jump to a specific track by index
    pub fn play_index(&self, index: usize) -> Option<QueueTrack> {
        let mut state = self.state.lock().unwrap();
        if index >= state.tracks.len() {
            return None;
        }

        // Save current to history
        if let Some(curr_idx) = state.current_index {
            state.history.push_back(curr_idx);
            while state.history.len() > 50 {
                state.history.pop_front();
            }
        }

        state.current_index = Some(index);

        if state.shuffle {
            if let Some(pos) = state.shuffle_order.iter().position(|&x| x == index) {
                state.shuffle_position = pos;
            }
        }

        state.tracks.get(index).cloned()
    }

    /// Toggle shuffle mode
    pub fn set_shuffle(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        if state.shuffle == enabled {
            return;
        }
        state.shuffle = enabled;

        if enabled {
            Self::regenerate_shuffle_order_internal(&mut state);
        }
    }

    /// Get shuffle status
    pub fn is_shuffle(&self) -> bool {
        self.state.lock().unwrap().shuffle
    }

    /// Set repeat mode
    pub fn set_repeat(&self, mode: RepeatMode) {
        self.state.lock().unwrap().repeat = mode;
    }

    /// Get repeat mode
    pub fn get_repeat(&self) -> RepeatMode {
        self.state.lock().unwrap().repeat
    }

    /// Get queue state for frontend
    pub fn get_state(&self) -> QueueState {
        let state = self.state.lock().unwrap();

        let current_track = state.current_index.and_then(|idx| state.tracks.get(idx).cloned());

        // Get upcoming tracks (after current)
        let upcoming: Vec<QueueTrack> = if let Some(curr_idx) = state.current_index {
            if state.shuffle {
                state.shuffle_order.iter()
                    .skip(state.shuffle_position + 1)
                    .take(20)
                    .filter_map(|&idx| state.tracks.get(idx).cloned())
                    .collect()
            } else {
                state.tracks.iter()
                    .skip(curr_idx + 1)
                    .take(20)
                    .cloned()
                    .collect()
            }
        } else {
            state.tracks.iter().take(20).cloned().collect()
        };

        // Get history tracks (recent first)
        let history_tracks: Vec<QueueTrack> = state.history.iter()
            .rev()
            .take(10)
            .filter_map(|&idx| state.tracks.get(idx).cloned())
            .collect();

        QueueState {
            current_track,
            current_index: state.current_index,
            upcoming,
            history: history_tracks,
            shuffle: state.shuffle,
            repeat: state.repeat,
            total_tracks: state.tracks.len(),
        }
    }

    /// Regenerate shuffle order (internal, must be called with lock held)
    fn regenerate_shuffle_order_internal(state: &mut InternalState) {
        let mut order: Vec<usize> = (0..state.tracks.len()).collect();

        // Fisher-Yates shuffle with proper PRNG
        use rand::{Rng, SeedableRng};
        use std::time::{SystemTime, UNIX_EPOCH};

        // Create seeded RNG from current timestamp
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        // Proper Fisher-Yates shuffle - each iteration gets a NEW random number
        for i in (1..order.len()).rev() {
            let j = rng.gen_range(0..=i);
            order.swap(i, j);
        }

        state.shuffle_order = order;

        // If there's a current track, find its position in the new shuffle order
        // (don't move it to front, just update our position in the shuffled list)
        if let Some(curr_idx) = state.current_index {
            if let Some(pos) = state.shuffle_order.iter().position(|&x| x == curr_idx) {
                state.shuffle_position = pos;
            } else {
                state.shuffle_position = 0;
            }
        } else {
            state.shuffle_position = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_track(id: u64) -> QueueTrack {
        QueueTrack {
            id,
            title: format!("Track {}", id),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration_secs: 180,
            artwork_url: None,
            hires: false,
            bit_depth: None,
            sample_rate: None,
            is_local: false,
            album_id: None,
            artist_id: None,
            streamable: true,
            source: Some("test".to_string()),
        }
    }

    #[test]
    fn test_clear_without_current_track() {
        let queue = QueueManager::new();

        queue.add_track(create_test_track(123));
        queue.add_track(create_test_track(124));
        queue.add_track(create_test_track(125));

        queue.clear();

        let state = queue.get_state();
        assert!(state.current_track.is_none());
        assert!(state.upcoming.is_empty());
        assert_eq!(state.total_tracks, 0);
    }

    #[test]
    fn test_clear_keeps_current_track() {
        let queue = QueueManager::new();

        queue.add_track(create_test_track(123));
        queue.add_track(create_test_track(124));
        queue.add_track(create_test_track(125));
        queue.play_index(0);

        queue.clear();

        let state = queue.get_state();
        assert!(state.current_track.is_some());
        assert_eq!(state.current_track.unwrap().id, 123);
        assert!(state.upcoming.is_empty());
        assert_eq!(state.total_tracks, 1);
    }

    #[test]
    fn test_move_track_down_without_current_track() {
        let queue = QueueManager::new();

        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }

        let result = queue.move_track(0, 3);

        assert!(result, "move_track should succeed");
        assert_eq!(queue.get_state().upcoming.iter().map(|track| track.id).collect::<Vec<u64>>(), vec![2, 3, 1, 4, 5]);
    }

    #[test]
    fn test_move_track_down_with_current_track() {
        let queue = QueueManager::new();

        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }
        queue.play_index(0);

        // Can't move the current playing track so this translates to "move from_index 1 -> 3"
        let result = queue.move_track(0, 3);

        assert!(result, "move_track should succeed");
        assert_eq!(queue.get_state().upcoming.iter().map(|track| track.id).collect::<Vec<u64>>(), vec![3, 4, 2, 5]);
    }

    #[test]
    fn test_move_track_up_without_current_track() {
        let queue = QueueManager::new();

        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }

        let result = queue.move_track(3, 0);

        assert!(result, "move_track should succeed");
        assert_eq!(queue.get_state().upcoming.iter().map(|track| track.id).collect::<Vec<u64>>(), vec![4, 1, 2, 3, 5]);
    }

    #[test]
    fn test_move_track_up_with_current_track() {
        let queue = QueueManager::new();

        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }
        queue.play_index(0);

        let result = queue.move_track(3, 0);

        assert!(result, "move_track should succeed");
        assert_eq!(queue.get_state().upcoming.iter().map(|track| track.id).collect::<Vec<u64>>(), vec![5, 2, 3, 4]);
    }

    #[test]
    fn test_move_track_to_the_same_position_without_current_track() {
        let queue = QueueManager::new();

        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }

        let result = queue.move_track(2, 3);

        assert!(result, "move_track should succeed");
        assert_eq!(queue.get_state().upcoming.iter().map(|track| track.id).collect::<Vec<u64>>(), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_move_track_to_the_same_position_with_current_track() {
        let queue = QueueManager::new();

        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }
        queue.play_index(0);

        let result = queue.move_track(0, 1);

        assert!(result, "move_track should succeed");
        assert_eq!(queue.get_state().upcoming.iter().map(|track| track.id).collect::<Vec<u64>>(), vec![2, 3, 4, 5]);
    }
}
