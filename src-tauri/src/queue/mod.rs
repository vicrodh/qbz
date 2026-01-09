//! Queue management module
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

/// Queue manager for handling playback queue
pub struct QueueManager {
    /// All tracks in the queue (original order)
    tracks: Mutex<Vec<QueueTrack>>,
    /// Current playback index
    current_index: Mutex<Option<usize>>,
    /// Shuffle mode enabled
    shuffle: Mutex<bool>,
    /// Shuffled indices (when shuffle is on)
    shuffle_order: Mutex<Vec<usize>>,
    /// Position in shuffle order
    shuffle_position: Mutex<usize>,
    /// Repeat mode
    repeat: Mutex<RepeatMode>,
    /// History of played track indices (for going back)
    history: Mutex<VecDeque<usize>>,
}

impl Default for QueueManager {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueManager {
    pub fn new() -> Self {
        Self {
            tracks: Mutex::new(Vec::new()),
            current_index: Mutex::new(None),
            shuffle: Mutex::new(false),
            shuffle_order: Mutex::new(Vec::new()),
            shuffle_position: Mutex::new(0),
            repeat: Mutex::new(RepeatMode::Off),
            history: Mutex::new(VecDeque::with_capacity(50)),
        }
    }

    /// Add a track to the end of the queue
    pub fn add_track(&self, track: QueueTrack) {
        let mut tracks = self.tracks.lock().unwrap();
        tracks.push(track);

        // Update shuffle order if needed
        if *self.shuffle.lock().unwrap() {
            let mut order = self.shuffle_order.lock().unwrap();
            order.push(tracks.len() - 1);
        }
    }

    /// Add multiple tracks to the queue
    pub fn add_tracks(&self, new_tracks: Vec<QueueTrack>) {
        let mut tracks = self.tracks.lock().unwrap();
        let start_idx = tracks.len();
        tracks.extend(new_tracks);

        // Update shuffle order if needed
        if *self.shuffle.lock().unwrap() {
            let mut order = self.shuffle_order.lock().unwrap();
            for i in start_idx..tracks.len() {
                order.push(i);
            }
        }
    }

    /// Set the entire queue (replaces existing)
    pub fn set_queue(&self, new_tracks: Vec<QueueTrack>, start_index: Option<usize>) {
        let mut tracks = self.tracks.lock().unwrap();
        *tracks = new_tracks;

        let mut current = self.current_index.lock().unwrap();
        *current = start_index;

        // Reset shuffle order
        self.regenerate_shuffle_order();

        // Clear history
        self.history.lock().unwrap().clear();
    }

    /// Clear the queue
    pub fn clear(&self) {
        self.tracks.lock().unwrap().clear();
        *self.current_index.lock().unwrap() = None;
        self.shuffle_order.lock().unwrap().clear();
        *self.shuffle_position.lock().unwrap() = 0;
        self.history.lock().unwrap().clear();
    }

    /// Remove a track by index
    pub fn remove_track(&self, index: usize) -> Option<QueueTrack> {
        let mut tracks = self.tracks.lock().unwrap();
        if index >= tracks.len() {
            return None;
        }

        let removed = tracks.remove(index);

        // Adjust current index if needed
        let mut current = self.current_index.lock().unwrap();
        if let Some(curr_idx) = *current {
            if index < curr_idx {
                *current = Some(curr_idx - 1);
            } else if index == curr_idx {
                // Currently playing track was removed
                if curr_idx >= tracks.len() {
                    *current = if tracks.is_empty() { None } else { Some(tracks.len() - 1) };
                }
            }
        }

        // Regenerate shuffle order
        self.regenerate_shuffle_order();

        Some(removed)
    }

    /// Get current track
    pub fn current_track(&self) -> Option<QueueTrack> {
        let tracks = self.tracks.lock().unwrap();
        let current = self.current_index.lock().unwrap();

        current.and_then(|idx| tracks.get(idx).cloned())
    }

    /// Get next track without advancing
    pub fn peek_next(&self) -> Option<QueueTrack> {
        let tracks = self.tracks.lock().unwrap();
        if tracks.is_empty() {
            return None;
        }

        let current = self.current_index.lock().unwrap();
        let repeat = *self.repeat.lock().unwrap();
        let shuffle = *self.shuffle.lock().unwrap();

        // Handle repeat one - next is the same track
        if repeat == RepeatMode::One {
            return current.and_then(|idx| tracks.get(idx).cloned());
        }

        let next_idx = if shuffle {
            let order = self.shuffle_order.lock().unwrap();
            let pos = *self.shuffle_position.lock().unwrap();
            let next_pos = pos + 1;

            if next_pos < order.len() {
                Some(order[next_pos])
            } else if repeat == RepeatMode::All {
                order.first().copied()
            } else {
                None
            }
        } else {
            let curr_idx = current.unwrap_or(0);
            let next_idx = curr_idx + 1;

            if next_idx < tracks.len() {
                Some(next_idx)
            } else if repeat == RepeatMode::All {
                Some(0)
            } else {
                None
            }
        };

        next_idx.and_then(|idx| tracks.get(idx).cloned())
    }

    /// Advance to next track and return it
    pub fn next(&self) -> Option<QueueTrack> {
        let tracks = self.tracks.lock().unwrap();
        if tracks.is_empty() {
            return None;
        }

        let mut current = self.current_index.lock().unwrap();
        let repeat = *self.repeat.lock().unwrap();
        let shuffle = *self.shuffle.lock().unwrap();

        // Save current to history before moving
        if let Some(curr_idx) = *current {
            let mut history = self.history.lock().unwrap();
            history.push_back(curr_idx);
            // Keep history limited
            while history.len() > 50 {
                history.pop_front();
            }
        }

        // Handle repeat one
        if repeat == RepeatMode::One {
            return current.and_then(|idx| tracks.get(idx).cloned());
        }

        let next_idx = if shuffle {
            let order = self.shuffle_order.lock().unwrap();
            let mut pos = self.shuffle_position.lock().unwrap();
            *pos += 1;

            if *pos < order.len() {
                Some(order[*pos])
            } else if repeat == RepeatMode::All {
                *pos = 0;
                order.first().copied()
            } else {
                None
            }
        } else {
            let curr_idx = current.unwrap_or(0);
            let next_idx = curr_idx + 1;

            if next_idx < tracks.len() {
                Some(next_idx)
            } else if repeat == RepeatMode::All {
                Some(0)
            } else {
                None
            }
        };

        *current = next_idx;
        drop(current);
        drop(tracks);

        self.current_track()
    }

    /// Go to previous track and return it
    pub fn previous(&self) -> Option<QueueTrack> {
        let tracks = self.tracks.lock().unwrap();
        if tracks.is_empty() {
            return None;
        }

        let mut current = self.current_index.lock().unwrap();
        let repeat = *self.repeat.lock().unwrap();
        let shuffle = *self.shuffle.lock().unwrap();

        // Try to get from history first
        let mut history = self.history.lock().unwrap();
        if let Some(prev_idx) = history.pop_back() {
            *current = Some(prev_idx);

            // Adjust shuffle position if needed
            if shuffle {
                let order = self.shuffle_order.lock().unwrap();
                if let Some(pos) = order.iter().position(|&x| x == prev_idx) {
                    *self.shuffle_position.lock().unwrap() = pos;
                }
            }

            return tracks.get(prev_idx).cloned();
        }
        drop(history);

        // No history, go to previous in order
        let prev_idx = if shuffle {
            let order = self.shuffle_order.lock().unwrap();
            let mut pos = self.shuffle_position.lock().unwrap();

            if *pos > 0 {
                *pos -= 1;
                Some(order[*pos])
            } else if repeat == RepeatMode::All {
                *pos = order.len().saturating_sub(1);
                order.last().copied()
            } else {
                Some(order.first().copied().unwrap_or(0))
            }
        } else {
            let curr_idx = current.unwrap_or(0);

            if curr_idx > 0 {
                Some(curr_idx - 1)
            } else if repeat == RepeatMode::All {
                Some(tracks.len().saturating_sub(1))
            } else {
                Some(0) // Stay at beginning
            }
        };

        *current = prev_idx;
        drop(current);
        drop(tracks);

        self.current_track()
    }

    /// Jump to a specific track by index
    pub fn play_index(&self, index: usize) -> Option<QueueTrack> {
        let tracks = self.tracks.lock().unwrap();
        if index >= tracks.len() {
            return None;
        }

        // Save current to history
        let mut current = self.current_index.lock().unwrap();
        if let Some(curr_idx) = *current {
            let mut history = self.history.lock().unwrap();
            history.push_back(curr_idx);
            while history.len() > 50 {
                history.pop_front();
            }
        }

        *current = Some(index);

        // Update shuffle position if needed
        if *self.shuffle.lock().unwrap() {
            let order = self.shuffle_order.lock().unwrap();
            if let Some(pos) = order.iter().position(|&x| x == index) {
                *self.shuffle_position.lock().unwrap() = pos;
            }
        }

        tracks.get(index).cloned()
    }

    /// Toggle shuffle mode
    pub fn set_shuffle(&self, enabled: bool) {
        let mut shuffle = self.shuffle.lock().unwrap();
        if *shuffle == enabled {
            return;
        }
        *shuffle = enabled;
        drop(shuffle);

        if enabled {
            self.regenerate_shuffle_order();
        }
    }

    /// Get shuffle status
    pub fn is_shuffle(&self) -> bool {
        *self.shuffle.lock().unwrap()
    }

    /// Set repeat mode
    pub fn set_repeat(&self, mode: RepeatMode) {
        *self.repeat.lock().unwrap() = mode;
    }

    /// Get repeat mode
    pub fn get_repeat(&self) -> RepeatMode {
        *self.repeat.lock().unwrap()
    }

    /// Get queue state for frontend
    pub fn get_state(&self) -> QueueState {
        let tracks = self.tracks.lock().unwrap();
        let current_index = *self.current_index.lock().unwrap();
        let shuffle = *self.shuffle.lock().unwrap();
        let repeat = *self.repeat.lock().unwrap();
        let history = self.history.lock().unwrap();

        let current_track = current_index.and_then(|idx| tracks.get(idx).cloned());

        // Get upcoming tracks (after current)
        let upcoming: Vec<QueueTrack> = if let Some(curr_idx) = current_index {
            if shuffle {
                let order = self.shuffle_order.lock().unwrap();
                let pos = *self.shuffle_position.lock().unwrap();
                order.iter()
                    .skip(pos + 1)
                    .take(20)
                    .filter_map(|&idx| tracks.get(idx).cloned())
                    .collect()
            } else {
                tracks.iter()
                    .skip(curr_idx + 1)
                    .take(20)
                    .cloned()
                    .collect()
            }
        } else {
            tracks.iter().take(20).cloned().collect()
        };

        // Get history tracks (recent first)
        let history_tracks: Vec<QueueTrack> = history.iter()
            .rev()
            .take(10)
            .filter_map(|&idx| tracks.get(idx).cloned())
            .collect();

        QueueState {
            current_track,
            current_index,
            upcoming,
            history: history_tracks,
            shuffle,
            repeat,
            total_tracks: tracks.len(),
        }
    }

    /// Regenerate shuffle order
    fn regenerate_shuffle_order(&self) {
        let tracks = self.tracks.lock().unwrap();
        let current_index = *self.current_index.lock().unwrap();

        let mut order: Vec<usize> = (0..tracks.len()).collect();

        // Fisher-Yates shuffle
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as usize;

        for i in (1..order.len()).rev() {
            let j = (seed.wrapping_mul(i + 1)) % (i + 1);
            order.swap(i, j);
        }

        // If there's a current track, move it to the front
        if let Some(curr_idx) = current_index {
            if let Some(pos) = order.iter().position(|&x| x == curr_idx) {
                order.remove(pos);
                order.insert(0, curr_idx);
            }
        }

        *self.shuffle_order.lock().unwrap() = order;
        *self.shuffle_position.lock().unwrap() = 0;
    }
}
