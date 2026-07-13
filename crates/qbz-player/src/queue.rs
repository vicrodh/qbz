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

use qbz_models::{QueueState, QueueTrack, RepeatMode};

#[derive(Debug, PartialEq, Eq)]
enum QueueMoveDirection {
    Up,
    Down,
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
    /// Track ID to stop after (optional)
    stop_after_track_id: Option<u64>,
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
                stop_after_track_id: None,
            }),
        }
    }

    /// The track at the current playback index, if any.
    pub fn current(&self) -> Option<QueueTrack> {
        let state = self.state.lock().unwrap();
        state
            .current_index
            .and_then(|idx| state.tracks.get(idx).cloned())
    }

    /// Patch the cached quality (bit depth + sample rate) of every queued track
    /// whose Plex `rating_key` matches one of `updates`. Used by the Plex
    /// quality-hydration path so a track that gets hydrated while it is already
    /// enqueued/playing has its frozen quality snapshot upgraded in place. Plex
    /// rows carry their `rating_key` in `source_item_id_hint`. `sample_rate` is
    /// in kHz to match the enqueue-time snapshot (`local_queue_track`). Returns
    /// true if the CURRENT track was among those patched (the caller then
    /// re-pushes the now-playing stamp).
    pub fn patch_plex_quality(&self, updates: &[(String, Option<u32>, Option<f64>)]) -> bool {
        if updates.is_empty() {
            return false;
        }
        let mut state = self.state.lock().unwrap();
        let current_idx = state.current_index;
        let mut current_patched = false;
        for (idx, track) in state.tracks.iter_mut().enumerate() {
            if track.source.as_deref() != Some("plex") {
                continue;
            }
            let Some(key) = track.source_item_id_hint.as_deref() else {
                continue;
            };
            if let Some((_, bit_depth, sample_rate)) =
                updates.iter().find(|(k, _, _)| k == key)
            {
                if let Some(bd) = *bit_depth {
                    track.bit_depth = Some(bd);
                    track.hires = bd > 16;
                }
                if let Some(sr) = *sample_rate {
                    track.sample_rate = Some(sr);
                }
                if current_idx == Some(idx) {
                    current_patched = true;
                }
            }
        }
        current_patched
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
        state.stop_after_track_id = None;
        // Remap history by track id BEFORE replacing tracks so that legitimate
        // plays survive queue version bumps / reorders. Entries whose track is
        // no longer present are dropped. See bug #316.
        Self::remap_history_by_track_id_internal(&mut state, &new_tracks);
        state.tracks = new_tracks;
        state.current_index = start_index;

        // Regenerate shuffle order
        Self::regenerate_shuffle_order_internal(&mut state);

        // CRITICAL FIX: When shuffle is enabled and we have a start_index,
        // ensure the start_index track is at the BEGINNING of shuffle order
        if state.shuffle {
            if let Some(start_idx) = start_index {
                if start_idx < state.tracks.len() {
                    if let Some(pos) = state.shuffle_order.iter().position(|&x| x == start_idx) {
                        state.shuffle_order.swap(0, pos);
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

    /// Replace the queue and playback order in a single atomic update.
    /// This avoids emitting an intermediate locally reshuffled state before an
    /// authoritative remote shuffle order has been applied.
    pub fn set_queue_with_order(
        &self,
        new_tracks: Vec<QueueTrack>,
        start_index: Option<usize>,
        shuffle_enabled: bool,
        shuffle_order: Option<Vec<usize>>,
    ) {
        let mut state = self.state.lock().unwrap();
        state.stop_after_track_id = None;
        // Remap history by track id BEFORE replacing tracks so that legitimate
        // plays survive queue version bumps / reorders. Entries whose track is
        // no longer present are dropped. See bug #316.
        Self::remap_history_by_track_id_internal(&mut state, &new_tracks);
        state.tracks = new_tracks;
        state.current_index = start_index;
        state.shuffle = shuffle_enabled;

        if !shuffle_enabled {
            state.shuffle_order.clear();
            state.shuffle_position = 0;
            return;
        }

        if let Some(order) =
            shuffle_order.filter(|order| Self::is_valid_shuffle_order(order, state.tracks.len()))
        {
            state.shuffle_order = order;
            if let Some(curr_idx) = state.current_index {
                if let Some(pos) = state.shuffle_order.iter().position(|&idx| idx == curr_idx) {
                    state.shuffle_position = pos;
                } else {
                    state.shuffle_position = 0;
                }
            } else {
                state.shuffle_position = 0;
            }
            return;
        }

        Self::set_identity_shuffle_order_internal(&mut state);
    }

    /// Clear the queue.
    ///
    /// When `keep_current` is true (default / historical behavior), the track
    /// at `current_index` is preserved as the sole remaining entry so the
    /// "now playing" slot doesn't go dark mid-song. Callers that know nothing
    /// is playing (or want to fully reset) can pass `false` to wipe everything,
    /// including the current track.
    pub fn clear(&self, keep_current: bool) {
        let mut state = self.state.lock().unwrap();
        state.stop_after_track_id = None;

        if keep_current {
            // Keep the track at `current_index`, not always `tracks[0]`.
            // `truncate(1)` was wrong mid-queue: clear while playing track N
            // would leave the first row as now-playing while audio kept N.
            if let Some(idx) = state.current_index {
                if idx < state.tracks.len() {
                    let kept = state.tracks[idx].clone();
                    // History stores indices into `tracks`. Remap by track id
                    // so entries for removed rows drop and any entry that still
                    // refers to the kept track points at index 0.
                    Self::remap_history_by_track_id_internal(&mut state, std::slice::from_ref(&kept));
                    state.tracks = vec![kept];
                    state.current_index = Some(0);
                } else {
                    state.tracks.clear();
                    state.current_index = None;
                    state.history.clear();
                }
            } else {
                state.tracks.clear();
                state.current_index = None;
                state.history.clear();
            }
        } else {
            state.tracks.clear();
            state.current_index = None;
            // Indices into an empty list are meaningless.
            state.history.clear();
        }

        state.shuffle_order.clear();
        state.shuffle_position = 0;
        // Playback history is remapped (or cleared) above. "Clear queue" only
        // affects current/upcoming queue rows, not an intentional history wipe
        // when the kept track still resolves — but removed tracks cannot stay
        // in index-based history.
    }

    /// Remove a track by index
    pub fn remove_track(&self, index: usize) -> Option<QueueTrack> {
        let mut state = self.state.lock().unwrap();
        if index >= state.tracks.len() {
            return None;
        }

        let removed = state.tracks.remove(index);

        // Invalidate marker if the removed track matches
        if state.stop_after_track_id == Some(removed.id) {
            state.stop_after_track_id = None;
        }

        // Adjust current index if needed
        if let Some(curr_idx) = state.current_index {
            if index < curr_idx {
                state.current_index = Some(curr_idx - 1);
            } else if index == curr_idx {
                if curr_idx >= state.tracks.len() {
                    state.current_index = if state.tracks.is_empty() {
                        None
                    } else {
                        Some(state.tracks.len() - 1)
                    };
                }
            }
        }

        // Keep history indices aligned with current track list after removal.
        state.history.retain(|&hist_idx| hist_idx != index);
        for hist_idx in state.history.iter_mut() {
            if *hist_idx > index {
                *hist_idx -= 1;
            }
        }

        if state.shuffle {
            Self::remove_index_from_shuffle_internal(&mut state, index);
        }
        Some(removed)
    }

    /// Remove a track by its position in the upcoming list
    pub fn remove_upcoming_track(&self, upcoming_index: usize) -> Option<QueueTrack> {
        let mut state = self.state.lock().unwrap();

        let actual_index = if state.shuffle {
            let shuffle_pos = state.shuffle_position + 1 + upcoming_index;
            if shuffle_pos >= state.shuffle_order.len() {
                return None;
            }
            state.shuffle_order[shuffle_pos]
        } else {
            match state.current_index {
                Some(curr_idx) => curr_idx + 1 + upcoming_index,
                None => upcoming_index,
            }
        };

        if actual_index >= state.tracks.len() {
            return None;
        }

        log::info!(
            "remove_upcoming_track: upcoming_index={} -> actual_index={}",
            upcoming_index,
            actual_index
        );

        let removed = state.tracks.remove(actual_index);

        // Invalidate marker if the removed track matches
        if state.stop_after_track_id == Some(removed.id) {
            state.stop_after_track_id = None;
        }

        if let Some(curr_idx) = state.current_index {
            if actual_index < curr_idx {
                state.current_index = Some(curr_idx - 1);
            } else if actual_index == curr_idx {
                if curr_idx >= state.tracks.len() {
                    state.current_index = if state.tracks.is_empty() {
                        None
                    } else {
                        Some(state.tracks.len() - 1)
                    };
                }
            }
        }

        // Keep history indices aligned with current track list after removal.
        state.history.retain(|&hist_idx| hist_idx != actual_index);
        for hist_idx in state.history.iter_mut() {
            if *hist_idx > actual_index {
                *hist_idx -= 1;
            }
        }

        if state.shuffle {
            Self::remove_index_from_shuffle_internal(&mut state, actual_index);
        }
        Some(removed)
    }

    /// Number of upcoming tracks (those after the current one) in the current
    /// play order. Mirrors `get_state_full`'s upcoming computation: shuffle-aware
    /// when shuffle is on, otherwise the tail of `tracks` after `current_index`.
    fn upcoming_len(state: &InternalState) -> usize {
        match state.current_index {
            Some(curr) => {
                if state.shuffle {
                    state
                        .shuffle_order
                        .len()
                        .saturating_sub(state.shuffle_position + 1)
                } else {
                    state.tracks.len().saturating_sub(curr + 1)
                }
            }
            None => state.tracks.len(),
        }
    }

    /// Remove every upcoming track positioned AFTER `upcoming_index` in the
    /// current play order; the track at `upcoming_index` is kept. Works in
    /// UPCOMING space (not absolute `tracks` indices), so it stays correct under
    /// shuffle by reusing `remove_upcoming_track`, which resolves upcoming
    /// positions through `shuffle_order`. Peels positions off the tail inward so
    /// the surviving positions never shift under it. Returns the count removed.
    ///
    /// This is the wired "Remove all after" queue action. (`remove_after`, below,
    /// truncates by absolute `tracks` index and is NOT play-order-aware under
    /// shuffle — it is kept only for its existing unit coverage.)
    pub fn remove_upcoming_after(&self, upcoming_index: usize) -> usize {
        let mut upcoming_len = {
            let state = self.state.lock().unwrap();
            Self::upcoming_len(&state)
        };
        if upcoming_index + 1 >= upcoming_len {
            return 0;
        }
        let mut removed = 0usize;
        while upcoming_len > upcoming_index + 1 {
            if self.remove_upcoming_track(upcoming_len - 1).is_none() {
                break;
            }
            removed += 1;
            upcoming_len -= 1;
        }
        removed
    }

    /// Remove all tracks at indices greater than `index`. The track at
    /// `index` is preserved. Returns the number of tracks removed.
    /// If the marker referenced a track in the removed range, the marker
    /// is cleared. No-op (returns 0) if `index` is the last position or
    /// out of bounds.
    pub fn remove_after(&self, index: usize) -> usize {
        let mut state = self.state.lock().unwrap();

        if index + 1 >= state.tracks.len() {
            return 0;
        }

        let cutoff = index + 1;
        let removed_ids: Vec<u64> = state.tracks[cutoff..].iter().map(|t| t.id).collect();
        let removed_count = removed_ids.len();

        // Drop the tail of `tracks`.
        state.tracks.truncate(cutoff);

        // If shuffle is active, also drop indices >= cutoff from shuffle_order
        // (preserve relative order of surviving indices).
        if state.shuffle {
            state.shuffle_order.retain(|&i| i < cutoff);
            // shuffle_position remains valid since we only dropped tracks AFTER
            // the current playing one (precondition: index >= current_index in
            // the typical UI flow; defensive clamp below handles edge cases).
            if state.shuffle_position >= state.shuffle_order.len() {
                state.shuffle_position = state.shuffle_order.len().saturating_sub(1);
            }
        }

        // Drop history entries pointing past the cutoff.
        state.history.retain(|&i| i < cutoff);

        // Invalidate marker if it pointed into the removed range.
        if let Some(marker_id) = state.stop_after_track_id {
            if removed_ids.contains(&marker_id) {
                state.stop_after_track_id = None;
            }
        }

        removed_count
    }

    /// Move a track from one position to another
    pub fn move_track(&self, from_index: usize, to_index: usize) -> bool {
        let mut state = self.state.lock().unwrap();

        if state.shuffle {
            // In shuffle mode, DnD indices come from the visible upcoming list,
            // so they must be applied to shuffle_order positions (not absolute
            // track indices in state.tracks).
            let base_pos = state
                .current_index
                .map(|_| state.shuffle_position + 1)
                .unwrap_or(0);
            let from_pos = base_pos + from_index;
            let to_pos = base_pos + to_index;

            if from_pos >= state.shuffle_order.len() || to_pos >= state.shuffle_order.len() {
                return false;
            }

            if from_pos == to_pos {
                return true;
            }

            let moved = state.shuffle_order.remove(from_pos);
            state.shuffle_order.insert(to_pos, moved);

            if let Some(curr_idx) = state.current_index {
                if let Some(pos) = state.shuffle_order.iter().position(|&x| x == curr_idx) {
                    state.shuffle_position = pos;
                }
            } else {
                state.shuffle_position = 0;
            }

            return true;
        }

        let direction: QueueMoveDirection = if from_index > to_index {
            QueueMoveDirection::Up
        } else {
            QueueMoveDirection::Down
        };

        let mut from_idx = from_index;
        let mut to_idx = to_index;

        if let Some(curr_idx) = state.current_index {
            from_idx = from_idx + curr_idx + 1;
            to_idx = to_idx + curr_idx + 1;
        }

        if direction == QueueMoveDirection::Down {
            to_idx = to_idx - 1;
        }

        log::info!(
            "Queue: move_track - {:?} from {} to {} (internal indices:{} -> {}). Tracks in queue: {}",
            direction,
            from_index,
            to_index,
            from_idx,
            to_idx,
            state.tracks.len()
        );

        if from_idx == to_idx {
            return true;
        }

        if from_idx >= state.tracks.len() || to_idx >= state.tracks.len() {
            return false;
        }

        let track = state.tracks.remove(from_idx);
        state.tracks.insert(to_idx, track);

        if let Some(curr_idx) = state.current_index {
            if from_idx == curr_idx {
                state.current_index = Some(to_idx);
            } else if from_idx < curr_idx && to_idx >= curr_idx {
                state.current_index = Some(curr_idx - 1);
            } else if from_idx > curr_idx && to_idx <= curr_idx {
                state.current_index = Some(curr_idx + 1);
            }
        }

        // Keep history aligned after reorder.
        for hist_idx in state.history.iter_mut() {
            *hist_idx = Self::remap_index_after_move(*hist_idx, from_idx, to_idx);
        }

        true
    }

    /// Get current track
    pub fn current_track(&self) -> Option<QueueTrack> {
        let state = self.state.lock().unwrap();
        state
            .current_index
            .and_then(|idx| state.tracks.get(idx).cloned())
    }

    /// Get next track without advancing
    pub fn peek_next(&self) -> Option<QueueTrack> {
        let state = self.state.lock().unwrap();
        if state.tracks.is_empty() {
            return None;
        }

        if state.repeat == RepeatMode::One {
            return state
                .current_index
                .and_then(|idx| state.tracks.get(idx).cloned());
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

    /// Get multiple upcoming tracks without advancing
    pub fn peek_upcoming(&self, count: usize) -> Vec<QueueTrack> {
        let state = self.state.lock().unwrap();
        if state.tracks.is_empty() || count == 0 {
            return Vec::new();
        }

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
            return state
                .current_index
                .and_then(|idx| state.tracks.get(idx).cloned());
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

    /// Move the current pointer to the track whose id matches `id`, WITHOUT
    /// starting playback. Used to reconcile the queue pointer to a track the
    /// audio engine already advanced to on its own (a gapless hand-off
    /// happens inside the player, not through `next`), so the now-playing
    /// card never goes stale while the seek bar keeps moving.
    ///
    /// Returns the matched track plus whether the pointer actually moved
    /// (`false` = it was already current). Returns `None` when no queue track
    /// has that id, leaving the pointer untouched.
    pub fn sync_current_to_id(&self, id: u64) -> Option<(QueueTrack, bool)> {
        let mut state = self.state.lock().unwrap();
        let target = state.tracks.iter().position(|t| t.id == id)?;
        let moved = state.current_index != Some(target);
        if moved {
            // Record the outgoing track so `previous` still walks back.
            if let Some(curr_idx) = state.current_index {
                state.history.push_back(curr_idx);
                while state.history.len() > 50 {
                    state.history.pop_front();
                }
            }
            state.current_index = Some(target);
            // Keep the shuffle cursor aligned with the new position.
            if state.shuffle {
                if let Some(pos) = state.shuffle_order.iter().position(|&x| x == target) {
                    state.shuffle_position = pos;
                }
            }
        }
        state.tracks.get(target).cloned().map(|t| (t, moved))
    }

    /// Jump to a track by its position in the `upcoming` list as returned by
    /// `get_state`. This is the position the user sees in the Queue sidebar;
    /// the method resolves it to the correct canonical index even when
    /// shuffle is active (where the display order differs from the canonical
    /// `tracks` order).
    ///
    /// Used by the "click a track in the queue panel" path — fixes issue
    /// #327 where shuffle mode caused a different track than the one
    /// clicked to be played.
    pub fn play_upcoming_at(&self, upcoming_index: usize) -> Option<QueueTrack> {
        let canonical_index = {
            let state = self.state.lock().unwrap();
            match state.current_index {
                Some(_) if state.shuffle => state
                    .shuffle_order
                    .get(state.shuffle_position + 1 + upcoming_index)
                    .copied(),
                Some(curr_idx) => Some(curr_idx + 1 + upcoming_index),
                None => Some(upcoming_index),
            }
        };
        canonical_index.and_then(|idx| self.play_index(idx))
    }

    /// Jump to a specific track by index
    pub fn play_index(&self, index: usize) -> Option<QueueTrack> {
        let mut state = self.state.lock().unwrap();
        if index >= state.tracks.len() {
            return None;
        }

        // Save current to history — ONLY when actually moving to a DIFFERENT
        // track. Jumping to the index already current (e.g. the QConnect
        // controller's `materialize_remote_queue` re-aligning the cursor to the
        // same index via `play_index`, since the stopped local player makes the
        // alignment fire unconditionally) must NOT record a spurious "previous"
        // entry, or the current track shows up duplicated in the History tab.
        // Matches `sync_current_to_id`'s `moved` guard.
        if let Some(curr_idx) = state.current_index {
            if curr_idx != index {
                state.history.push_back(curr_idx);
                while state.history.len() > 50 {
                    state.history.pop_front();
                }
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

            // Enabling shuffle during active playback must keep current track
            // as the first item in the shuffled timeline. Otherwise, indices
            // before current are interpreted as already played.
            if let Some(curr_idx) = state.current_index {
                if let Some(pos) = state.shuffle_order.iter().position(|&idx| idx == curr_idx) {
                    if pos != 0 {
                        state.shuffle_order.swap(0, pos);
                    }
                    state.shuffle_position = 0;
                }
            }
        }
    }

    /// Set shuffle mode using an authoritative order produced elsewhere.
    /// Used by QConnect so the local queue follows the remote session order
    /// instead of generating a second independent shuffle.
    pub fn set_shuffle_with_order(&self, enabled: bool, shuffle_order: Option<Vec<usize>>) {
        let mut state = self.state.lock().unwrap();
        state.shuffle = enabled;

        if !enabled {
            state.shuffle_order.clear();
            state.shuffle_position = 0;
            return;
        }

        if let Some(order) =
            shuffle_order.filter(|order| Self::is_valid_shuffle_order(order, state.tracks.len()))
        {
            state.shuffle_order = order;
            if let Some(curr_idx) = state.current_index {
                if let Some(pos) = state.shuffle_order.iter().position(|&idx| idx == curr_idx) {
                    state.shuffle_position = pos;
                } else {
                    state.shuffle_position = 0;
                }
            } else {
                state.shuffle_position = 0;
            }
            return;
        }

        Self::set_identity_shuffle_order_internal(&mut state);
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

    /// Set the "stop after" marker on a specific track ID. Replaces any
    /// previous marker. Silent no-op if the track ID is not currently in
    /// the queue (defensive check — frontend should only ever pass IDs
    /// from the current queue).
    pub fn set_stop_after(&self, track_id: u64) {
        let mut state = self.state.lock().unwrap();
        if state.tracks.iter().any(|t| t.id == track_id) {
            state.stop_after_track_id = Some(track_id);
        }
    }

    /// Clear the marker (user cancellation from UI).
    pub fn clear_stop_after(&self) {
        let mut state = self.state.lock().unwrap();
        state.stop_after_track_id = None;
    }

    /// Read current marker (used by `get_state()` for serialization).
    pub fn get_stop_after(&self) -> Option<u64> {
        self.state.lock().unwrap().stop_after_track_id
    }

    /// One-shot consume: if the finished track ID matches the marker,
    /// clear it and return true. Otherwise return false. The
    /// auto-advance driver calls this on every natural track-end and
    /// pauses (instead of advancing) when it returns true. Manual skip
    /// paths must NOT call this.
    pub fn consume_stop_after_if(&self, finished_track_id: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.stop_after_track_id == Some(finished_track_id) {
            state.stop_after_track_id = None;
            true
        } else {
            false
        }
    }

    /// Get queue state for frontend
    pub fn get_state(&self) -> QueueState {
        let state = self.state.lock().unwrap();

        let current_track = state
            .current_index
            .and_then(|idx| state.tracks.get(idx).cloned());

        // Get upcoming tracks (after current)
        let upcoming: Vec<QueueTrack> = if let Some(curr_idx) = state.current_index {
            if state.shuffle {
                state
                    .shuffle_order
                    .iter()
                    .skip(state.shuffle_position + 1)
                    .take(20)
                    .filter_map(|&idx| state.tracks.get(idx).cloned())
                    .collect()
            } else {
                state
                    .tracks
                    .iter()
                    .skip(curr_idx + 1)
                    .take(20)
                    .cloned()
                    .collect()
            }
        } else {
            state.tracks.iter().take(20).cloned().collect()
        };

        // Get history tracks (recent first)
        let history_tracks: Vec<QueueTrack> = state
            .history
            .iter()
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
            stop_after_track_id: state.stop_after_track_id,
        }
    }

    /// Get all tracks in the queue plus the current index (for session persistence).
    /// Unlike get_state() which caps upcoming/history, this returns the full track list.
    pub fn get_all_tracks(&self) -> (Vec<QueueTrack>, Option<usize>) {
        let state = self.state.lock().unwrap();
        (state.tracks.clone(), state.current_index)
    }

    /// Get the full queue state without the upcoming/history caps applied by
    /// `get_state()`. Used by clients that paginate the upcoming list (e.g.
    /// the Queue sidebar's "UP NEXT" paginator) and need the complete history.
    /// The `upcoming` ordering is shuffle-aware, matching `get_state()`.
    pub fn get_state_full(&self) -> QueueState {
        let state = self.state.lock().unwrap();

        let current_track = state
            .current_index
            .and_then(|idx| state.tracks.get(idx).cloned());

        // Full upcoming list (after current), shuffle-aware. No `take` cap.
        let upcoming: Vec<QueueTrack> = if let Some(curr_idx) = state.current_index {
            if state.shuffle {
                state
                    .shuffle_order
                    .iter()
                    .skip(state.shuffle_position + 1)
                    .filter_map(|&idx| state.tracks.get(idx).cloned())
                    .collect()
            } else {
                state
                    .tracks
                    .iter()
                    .skip(curr_idx + 1)
                    .cloned()
                    .collect()
            }
        } else {
            state.tracks.clone()
        };

        // Full history (recent first). No `take` cap.
        let history_tracks: Vec<QueueTrack> = state
            .history
            .iter()
            .rev()
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
            stop_after_track_id: state.stop_after_track_id,
        }
    }

    /// Regenerate shuffle order (internal, must be called with lock held)
    fn regenerate_shuffle_order_internal(state: &mut InternalState) {
        let mut order: Vec<usize> = (0..state.tracks.len()).collect();

        // Fisher-Yates shuffle with proper PRNG
        use rand::{Rng, SeedableRng};
        use std::time::{SystemTime, UNIX_EPOCH};

        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        for i in (1..order.len()).rev() {
            let j = rng.random_range(0..=i);
            order.swap(i, j);
        }

        state.shuffle_order = order;

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

    /// Preserve the existing queue order when shuffle is remote-controlled but
    /// no authoritative remote order has arrived yet.
    fn set_identity_shuffle_order_internal(state: &mut InternalState) {
        state.shuffle_order = (0..state.tracks.len()).collect();

        if let Some(curr_idx) = state.current_index {
            state.shuffle_position = curr_idx.min(state.shuffle_order.len().saturating_sub(1));
        } else {
            state.shuffle_position = 0;
        }
    }

    /// Remap history entries from `state.tracks` indices to indices into
    /// `new_tracks`, looking up by track id. Entries whose track id is no
    /// longer present in `new_tracks` are dropped. Must be called with the
    /// lock held and BEFORE `state.tracks` is replaced.
    ///
    /// This preserves history across queue version bumps that don't change
    /// track identity (e.g. pure reorder, shuffle toggle, or an authoritative
    /// remote echo of the current local queue). Bug #316.
    fn remap_history_by_track_id_internal(state: &mut InternalState, new_tracks: &[QueueTrack]) {
        if state.history.is_empty() || new_tracks.is_empty() || state.tracks.is_empty() {
            state.history.clear();
            return;
        }

        // Build lookup: track_id -> new index. If duplicate ids exist (rare),
        // last occurrence wins; history will still resolve to a valid track.
        let mut new_id_to_idx: std::collections::HashMap<u64, usize> =
            std::collections::HashMap::with_capacity(new_tracks.len());
        for (idx, track) in new_tracks.iter().enumerate() {
            new_id_to_idx.insert(track.id, idx);
        }

        let mut remapped: VecDeque<usize> = VecDeque::with_capacity(state.history.len());
        for &old_idx in state.history.iter() {
            let Some(old_track) = state.tracks.get(old_idx) else {
                continue;
            };
            if let Some(&new_idx) = new_id_to_idx.get(&old_track.id) {
                remapped.push_back(new_idx);
            }
        }
        state.history = remapped;
    }

    /// Remap an index after remove+insert move operation.
    fn remap_index_after_move(idx: usize, from_idx: usize, to_idx: usize) -> usize {
        if idx == from_idx {
            return to_idx;
        }

        if from_idx < to_idx {
            // Moved down: [from+1 ..= to] shift left
            if idx > from_idx && idx <= to_idx {
                idx - 1
            } else {
                idx
            }
        } else {
            // Moved up: [to .. from-1] shift right
            if idx >= to_idx && idx < from_idx {
                idx + 1
            } else {
                idx
            }
        }
    }

    /// Remove one absolute track index from shuffle order and rebase remaining indices.
    fn remove_index_from_shuffle_internal(state: &mut InternalState, removed_idx: usize) {
        if let Some(pos) = state
            .shuffle_order
            .iter()
            .position(|&idx| idx == removed_idx)
        {
            state.shuffle_order.remove(pos);

            if pos < state.shuffle_position && state.shuffle_position > 0 {
                state.shuffle_position -= 1;
            } else if pos == state.shuffle_position
                && state.shuffle_position >= state.shuffle_order.len()
            {
                state.shuffle_position = state.shuffle_order.len().saturating_sub(1);
            }
        }

        for idx in state.shuffle_order.iter_mut() {
            if *idx > removed_idx {
                *idx -= 1;
            }
        }

        if let Some(curr_idx) = state.current_index {
            if let Some(pos) = state.shuffle_order.iter().position(|&idx| idx == curr_idx) {
                state.shuffle_position = pos;
            } else {
                state.shuffle_position = 0;
            }
        } else {
            state.shuffle_position = 0;
        }
    }

    fn is_valid_shuffle_order(order: &[usize], track_count: usize) -> bool {
        if order.len() != track_count {
            return false;
        }

        let mut seen = vec![false; track_count];
        for &idx in order {
            if idx >= track_count || seen[idx] {
                return false;
            }
            seen[idx] = true;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_track(id: u64) -> QueueTrack {
        QueueTrack {
            id,
            title: format!("Track {}", id),
            version: None,
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            album_version: None,
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
            parental_warning: false,
            source_item_id_hint: None,
            context_kind: None,
            context_id: None,
        }
    }

    #[test]
    fn test_clear_without_current_track() {
        let queue = QueueManager::new();

        queue.add_track(create_test_track(123));
        queue.add_track(create_test_track(124));
        queue.add_track(create_test_track(125));

        queue.clear(true);

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

        queue.clear(true);

        let state = queue.get_state();
        assert!(state.current_track.is_some());
        assert_eq!(state.current_track.unwrap().id, 123);
        assert!(state.upcoming.is_empty());
        assert_eq!(state.total_tracks, 1);
    }

    /// Regression: clear(true) must keep the track at `current_index`, not
    /// always `tracks[0]`. Mid-album "Clear queue" previously left the first
    /// row as now-playing while audio kept the real current track.
    #[test]
    fn test_clear_keeps_mid_queue_current_track() {
        let queue = QueueManager::new();

        queue.add_track(create_test_track(100));
        queue.add_track(create_test_track(200));
        queue.add_track(create_test_track(300));
        queue.play_index(1); // current = 200

        queue.clear(true);

        let state = queue.get_state();
        assert!(state.current_track.is_some());
        assert_eq!(state.current_track.unwrap().id, 200);
        assert!(state.upcoming.is_empty());
        assert_eq!(state.total_tracks, 1);
    }

    #[test]
    fn test_clear_wipes_current_track_when_not_kept() {
        let queue = QueueManager::new();

        queue.add_track(create_test_track(123));
        queue.add_track(create_test_track(124));
        queue.play_index(0);

        // keep_current: false — user pressed Clear Queue while nothing was
        // actively playing, so the stale "now playing" slot should go too.
        queue.clear(false);

        let state = queue.get_state();
        assert!(state.current_track.is_none());
        assert!(state.upcoming.is_empty());
        assert_eq!(state.total_tracks, 0);
    }

    // ===================================================================
    // LANE C DIAGNOSTIC TESTS (B1 — sync_current_to_id pointer robustness)
    // These reproduce concrete states where the QueueManager pointer ends
    // up at a DIFFERENT track than the audio engine is actually playing.
    // ===================================================================

    /// B1 ROOT-CAUSE (a): live track id NOT present in `state.tracks`.
    /// When the audio engine has gaplessly advanced into a track that is no
    /// longer in the queue (e.g. the queue was replaced by a brand-new list
    /// while the OLD track was still draining, or an infinite-radio / single-
    /// track-replace), `sync_current_to_id` returns None and the pointer is
    /// left untouched — pointing at the STALE old track. Both NPB and sidebar
    /// then read the wrong `current_track`, and the poll loop never retries
    /// (it updates `last_track_id` and `continue`s). This is the "cleared the
    /// queue and it did NOT fix it" symptom.
    #[test]
    fn test_sync_current_to_id_unknown_id_leaves_pointer_stale() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(100)); // "Enjoy the Silence"
        queue.add_track(create_test_track(101));
        queue.play_index(0); // pointer = 0 -> track 100

        // The audio engine is actually playing track 999 ("Beetlebum"), which
        // is NOT in the queue (queue was replaced / radio handoff).
        let result = queue.sync_current_to_id(999);

        // Sync fails — pointer cannot be corrected.
        assert!(result.is_none(), "sync should return None for unknown id");

        // The pointer is STILL on the stale track 100, so the now-playing
        // surfaces report the WRONG track while audio plays 999.
        let state = queue.get_state_full();
        assert_eq!(
            state.current_track.unwrap().id,
            100,
            "pointer stays stale at the OLD track when sync_current_to_id can't find the live id"
        );
    }

    /// B1 ROOT-CAUSE (a'): clear(keep_current) when the kept track is NOT the
    /// audible one. The user clears the queue to "fix" the stale now-playing,
    /// but clear keeps the track at the (already stale) `current_index`, so the
    /// wrong track is now the SOLE entry — and because its id != the live audio
    /// id, a subsequent sync_current_to_id STILL can't correct it. Reproduces
    /// the user's "cleared the queue and it did NOT fix it".
    #[test]
    fn test_clear_keep_current_preserves_the_wrong_track() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(100)); // stale "now playing"
        queue.add_track(create_test_track(101));
        queue.play_index(0); // pointer stuck on 100 (audio actually plays 999)

        queue.clear(true); // user hits "Clear Queue" to fix the stale row

        let state = queue.get_state_full();
        // The kept track is the WRONG one (100), not the audible 999.
        assert_eq!(state.current_track.unwrap().id, 100);

        // And sync still can't fix it: 999 isn't in the 1-track queue.
        assert!(queue.sync_current_to_id(999).is_none());
        assert_eq!(queue.get_state_full().current_track.unwrap().id, 100);
    }

    /// B1 ROOT-CAUSE (b): DUPLICATE track ids in the queue.
    /// `sync_current_to_id` uses `position(|t| t.id == id)`, which returns the
    /// FIRST match. If the same track id appears twice (very common: a track
    /// added to a playlist twice, an album with a repeated bonus track, or a
    /// QConnect queue echo), advancing the audio into the SECOND occurrence
    /// resyncs the pointer to the FIRST occurrence — wrong index. The
    /// now-playing track id is "right" but the upcoming list, position, and any
    /// index-derived state are computed from the wrong slot.
    #[test]
    fn test_sync_current_to_id_duplicate_id_resyncs_to_first_occurrence() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(100)); // idx 0
        queue.add_track(create_test_track(200)); // idx 1
        queue.add_track(create_test_track(100)); // idx 2  (DUPLICATE of idx 0)
        queue.add_track(create_test_track(300)); // idx 3
        queue.play_index(1); // pointer = 1 (track 200)

        // Audio gaplessly advanced into the SECOND copy of track 100 (idx 2).
        let (_, moved) = queue
            .sync_current_to_id(100)
            .expect("id 100 is present, so sync succeeds");
        assert!(moved, "pointer moved off track 200");

        let state = queue.get_state_full();
        // current_index resynced to the FIRST occurrence (idx 0), NOT idx 2.
        assert_eq!(
            state.current_index,
            Some(0),
            "sync_current_to_id resyncs to the FIRST id match (idx 0), not the actually-playing idx 2"
        );
        // Consequence: the UP-NEXT list is computed from the wrong slot. From
        // idx 0 the upcoming is [200, 100, 300]; from the real idx 2 it should
        // be [300]. The sidebar shows a wrong upcoming list.
        let upcoming_ids: Vec<u64> = state.upcoming.iter().map(|t| t.id).collect();
        assert_eq!(
            upcoming_ids,
            vec![200, 100, 300],
            "upcoming is derived from the wrong (first-occurrence) index"
        );
    }

    #[test]
    fn test_clear_preserves_history() {
        let queue = QueueManager::new();

        queue.add_track(create_test_track(123));
        queue.add_track(create_test_track(124));
        queue.add_track(create_test_track(125));
        queue.play_index(0);
        queue.next(); // push 123 into history, current becomes 124

        let before = queue.get_state();
        assert_eq!(before.history.len(), 1);
        assert_eq!(before.history[0].id, 123);
        assert_eq!(before.current_track.as_ref().map(|t| t.id), Some(124));

        queue.clear(true);

        let after = queue.get_state();
        // Kept track is the one that was current (124), not tracks[0] (123).
        assert_eq!(after.current_track.as_ref().map(|t| t.id), Some(124));
        assert_eq!(after.total_tracks, 1);
        // History is index-based: entries for tracks that left the queue are
        // dropped (same remap-by-id path as set_queue). Under the old
        // truncate(1) bug, 123 stayed as the sole row so history still
        // "resolved" — that was an accident of the bug, not the contract.
        assert!(
            after.history.is_empty(),
            "history must not resolve removed tracks after clear: {:?}",
            after.history.iter().map(|t| t.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn play_index_to_same_track_does_not_push_history() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(1));
        queue.add_track(create_test_track(2));
        queue.play_index(0); // current None -> 0, no push

        // Re-align to the SAME index — the QConnect controller materialize path
        // calls play_index with the index already current. It must NOT record a
        // spurious "previous" entry (the track-shows-twice-in-History bug).
        queue.play_index(0);
        assert!(
            queue.get_state().history.is_empty(),
            "re-aligning to the current track must not add a history entry"
        );

        // A genuine move still records the outgoing track.
        queue.play_index(1);
        let history = queue.get_state().history;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, 1);
    }

    #[test]
    fn test_move_track_down_without_current_track() {
        let queue = QueueManager::new();

        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }

        let result = queue.move_track(0, 3);

        assert!(result, "move_track should succeed");
        assert_eq!(
            queue
                .get_state()
                .upcoming
                .iter()
                .map(|track| track.id)
                .collect::<Vec<u64>>(),
            vec![2, 3, 1, 4, 5]
        );
    }

    #[test]
    fn test_move_track_down_with_current_track() {
        let queue = QueueManager::new();

        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }
        queue.play_index(0);

        let result = queue.move_track(0, 3);

        assert!(result, "move_track should succeed");
        assert_eq!(
            queue
                .get_state()
                .upcoming
                .iter()
                .map(|track| track.id)
                .collect::<Vec<u64>>(),
            vec![3, 4, 2, 5]
        );
    }

    #[test]
    fn test_move_track_up_without_current_track() {
        let queue = QueueManager::new();

        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }

        let result = queue.move_track(3, 0);

        assert!(result, "move_track should succeed");
        assert_eq!(
            queue
                .get_state()
                .upcoming
                .iter()
                .map(|track| track.id)
                .collect::<Vec<u64>>(),
            vec![4, 1, 2, 3, 5]
        );
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
        assert_eq!(
            queue
                .get_state()
                .upcoming
                .iter()
                .map(|track| track.id)
                .collect::<Vec<u64>>(),
            vec![5, 2, 3, 4]
        );
    }

    #[test]
    fn test_move_track_with_shuffle_reorders_shuffle_timeline() {
        let queue = QueueManager::new();
        for i in 1..=8 {
            queue.add_track(create_test_track(i));
        }

        queue.play_index(0);
        queue.set_shuffle(true);

        let before_shuffle = {
            let state = queue.state.lock().unwrap();
            state.shuffle_order.clone()
        };

        // With current_index=0 and shuffle_position=0:
        // upcoming move 2 -> 0 maps to shuffle positions 3 -> 1.
        assert!(queue.move_track(2, 0));

        let after_shuffle = {
            let state = queue.state.lock().unwrap();
            state.shuffle_order.clone()
        };

        let mut expected = before_shuffle.clone();
        let moved = expected.remove(3);
        expected.insert(1, moved);

        assert_eq!(after_shuffle, expected);
        assert_eq!(after_shuffle.len(), 8);
    }

    #[test]
    fn test_remove_track_with_shuffle_preserves_shuffle_order() {
        let queue = QueueManager::new();
        for i in 1..=8 {
            queue.add_track(create_test_track(i));
        }

        queue.play_index(0);
        queue.set_shuffle(true);

        let before_shuffle = {
            let state = queue.state.lock().unwrap();
            state.shuffle_order.clone()
        };

        assert!(queue.remove_track(2).is_some());

        let after_shuffle = {
            let state = queue.state.lock().unwrap();
            state.shuffle_order.clone()
        };

        let expected: Vec<usize> = before_shuffle
            .into_iter()
            .filter(|&idx| idx != 2)
            .map(|idx| if idx > 2 { idx - 1 } else { idx })
            .collect();

        assert_eq!(after_shuffle, expected);
        assert_eq!(after_shuffle.len(), 7);
    }

    #[test]
    fn test_enabling_shuffle_keeps_all_remaining_tracks_upcoming() {
        let queue = QueueManager::new();
        for i in 1..=11 {
            queue.add_track(create_test_track(i));
        }

        queue.play_index(0);
        queue.set_shuffle(true);

        let state = queue.get_state();
        assert_eq!(state.total_tracks, 11);
        assert_eq!(state.upcoming.len(), 10);
    }

    #[test]
    fn test_play_upcoming_at_without_shuffle_uses_linear_offset() {
        let queue = QueueManager::new();
        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }
        queue.play_index(1); // current = track id 2

        // upcoming list is [3, 4, 5]; clicking position 1 must play id 4
        let track = queue.play_upcoming_at(1).expect("track");
        assert_eq!(track.id, 4);
    }

    #[test]
    fn test_play_upcoming_at_with_shuffle_follows_shuffle_order() {
        let queue = QueueManager::new();
        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }

        // Authoritative shuffle: playing head is shuffle[0]=2 (id 3),
        // upcoming order becomes [5, 2, 4, 1] (track ids).
        queue.set_queue_with_order(
            (1..=5).map(create_test_track).collect(),
            Some(2),
            true,
            Some(vec![2, 4, 1, 3, 0]),
        );

        let state = queue.get_state();
        assert_eq!(
            state
                .upcoming
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            vec![5, 2, 4, 1]
        );

        // Clicking upcoming position 2 must play track id 4, not id 5
        // (which would be the "current_index + 2 + 1" = 5 broken path).
        let track = queue.play_upcoming_at(2).expect("track");
        assert_eq!(track.id, 4);
    }

    #[test]
    fn test_set_shuffle_with_order_uses_authoritative_order() {
        let queue = QueueManager::new();
        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }

        queue.play_index(2);
        queue.set_shuffle_with_order(true, Some(vec![2, 4, 1, 3, 0]));

        let state = queue.get_state();
        assert!(state.shuffle);
        assert_eq!(
            state
                .upcoming
                .iter()
                .map(|track| track.id)
                .collect::<Vec<_>>(),
            vec![5, 2, 4, 1]
        );
    }

    #[test]
    fn test_set_shuffle_with_order_preserves_current_order_when_invalid() {
        let queue = QueueManager::new();
        for i in 1..=4 {
            queue.add_track(create_test_track(i));
        }

        queue.play_index(1);
        queue.set_shuffle_with_order(true, Some(vec![1, 1, 2, 3]));

        let state = queue.get_state();
        assert!(state.shuffle);
        assert_eq!(
            state
                .upcoming
                .iter()
                .map(|track| track.id)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
    }

    #[test]
    fn test_set_queue_with_order_applies_authoritative_shuffle_before_snapshot() {
        let queue = QueueManager::new();
        let tracks = (1..=5).map(create_test_track).collect::<Vec<_>>();

        queue.set_queue_with_order(tracks, Some(0), true, Some(vec![0, 3, 1, 4, 2]));

        let state = queue.get_state();
        assert!(state.shuffle);
        assert_eq!(
            state
                .upcoming
                .iter()
                .map(|track| track.id)
                .collect::<Vec<_>>(),
            vec![4, 2, 5, 3]
        );
    }

    #[test]
    fn test_set_queue_with_order_preserves_queue_order_when_authoritative_order_missing() {
        let queue = QueueManager::new();
        let tracks = (1..=5).map(create_test_track).collect::<Vec<_>>();

        queue.set_queue_with_order(tracks, Some(1), true, None);

        let state = queue.get_state();
        assert!(state.shuffle);
        assert_eq!(
            state
                .upcoming
                .iter()
                .map(|track| track.id)
                .collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
    }

    // --- Bug #316 history-preservation regression tests ---

    /// Helper: build a queue with N tracks, play track 0, advance through
    /// `advance_count` to populate history, returning the queue.
    fn queue_with_played_history(track_count: u64, advance_count: usize) -> QueueManager {
        let queue = QueueManager::new();
        for i in 1..=track_count {
            queue.add_track(create_test_track(i));
        }
        queue.play_index(0);
        for _ in 0..advance_count {
            queue.next();
        }
        queue
    }

    #[test]
    fn test_set_queue_with_order_preserves_history_on_pure_reorder() {
        // Played 3 tracks, current is on track 4 (id=4).
        let queue = queue_with_played_history(5, 3);
        let before = queue.get_state();
        assert_eq!(
            before.history.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );

        // Same tracks, completely reordered. Current track (id=4) at new index 0.
        let reordered = vec![
            create_test_track(4),
            create_test_track(2),
            create_test_track(5),
            create_test_track(1),
            create_test_track(3),
        ];
        queue.set_queue_with_order(reordered, Some(0), false, None);

        let after = queue.get_state();
        // History rendered newest-first; ids must survive the reorder identically.
        assert_eq!(
            after.history.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
    }

    #[test]
    fn test_set_queue_with_order_preserves_history_when_tracks_added() {
        // Played track 1, then 2; current is track 3.
        let queue = queue_with_played_history(3, 2);

        // Same tracks plus 2 new ones (4, 5). Current still on track 3 (new index 2).
        let expanded = vec![
            create_test_track(1),
            create_test_track(2),
            create_test_track(3),
            create_test_track(4),
            create_test_track(5),
        ];
        queue.set_queue_with_order(expanded, Some(2), false, None);

        let after = queue.get_state();
        assert_eq!(
            after.history.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![2, 1]
        );
    }

    #[test]
    fn test_set_queue_with_order_drops_only_removed_tracks_from_history() {
        // Played tracks 1, 2, 3; current on track 4 (id=4).
        let queue = queue_with_played_history(5, 3);
        let before = queue.get_state();
        assert_eq!(
            before.history.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );

        // Remove track id=2 from queue; tracks 1 and 3 survive in history.
        let trimmed = vec![
            create_test_track(1),
            create_test_track(3),
            create_test_track(4),
            create_test_track(5),
        ];
        queue.set_queue_with_order(trimmed, Some(2), false, None);

        let after = queue.get_state();
        assert_eq!(
            after.history.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![3, 1]
        );
    }

    #[test]
    fn test_set_queue_with_order_clears_history_when_tracks_completely_different() {
        let queue = queue_with_played_history(5, 3);
        assert_eq!(queue.get_state().history.len(), 3);

        // No overlap with the previous queue; history must drop entirely.
        let fresh = vec![
            create_test_track(100),
            create_test_track(101),
            create_test_track(102),
        ];
        queue.set_queue_with_order(fresh, Some(0), false, None);

        let after = queue.get_state();
        assert!(after.history.is_empty());
    }

    #[test]
    fn test_set_queue_preserves_history_on_pure_reorder() {
        // Mirror test for set_queue (non-with-order variant).
        let queue = queue_with_played_history(5, 3);
        let before = queue.get_state();
        assert_eq!(
            before.history.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );

        let reordered = vec![
            create_test_track(5),
            create_test_track(4),
            create_test_track(3),
            create_test_track(2),
            create_test_track(1),
        ];
        queue.set_queue(reordered, Some(1)); // current track 4 now at idx 1

        let after = queue.get_state();
        assert_eq!(
            after.history.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
    }

    #[test]
    fn test_set_queue_with_order_remaps_history_indices_after_reorder() {
        // Verify that after a reorder, the internal indices stored in history
        // actually point to the right new tracks (not just that ids match
        // through the get_state() projection accidentally).
        let queue = queue_with_played_history(4, 3);

        // Reverse order. Old tracks 1,2,3,4 -> new tracks 4,3,2,1.
        // Old history: indices [0, 1, 2] -> ids [1, 2, 3].
        // New mapping: id=1->idx 3, id=2->idx 2, id=3->idx 1.
        // Expected new history indices: [3, 2, 1] (front-to-back).
        let reversed = vec![
            create_test_track(4),
            create_test_track(3),
            create_test_track(2),
            create_test_track(1),
        ];
        queue.set_queue_with_order(reversed, Some(0), false, None);

        // Inspect internal state to verify the indices, not just rendered ids.
        let state = queue.state.lock().unwrap();
        assert_eq!(
            state.history.iter().copied().collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
    }

    // ============ Stop-After Marker — Basic API ============

    #[test]
    fn test_set_stop_after_stores_marker() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.add_track(create_test_track(102));
        queue.add_track(create_test_track(103));

        queue.set_stop_after(102);

        assert_eq!(queue.get_stop_after(), Some(102));
    }

    #[test]
    fn test_set_stop_after_replaces_previous_marker() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.add_track(create_test_track(102));

        queue.set_stop_after(101);
        queue.set_stop_after(102);

        assert_eq!(queue.get_stop_after(), Some(102));
    }

    #[test]
    fn test_clear_stop_after_resets_marker() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.set_stop_after(101);

        queue.clear_stop_after();

        assert_eq!(queue.get_stop_after(), None);
    }

    #[test]
    fn test_set_stop_after_silently_ignores_unknown_id() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.add_track(create_test_track(102));

        queue.set_stop_after(999); // not in queue

        assert_eq!(queue.get_stop_after(), None);
    }

    #[test]
    fn test_set_stop_after_on_empty_queue_is_noop() {
        let queue = QueueManager::new();

        queue.set_stop_after(101);

        assert_eq!(queue.get_stop_after(), None);
    }

    // ============ Stop-After Marker — Consume (Firing Path) ============

    #[test]
    fn test_consume_stop_after_if_fires_on_match() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.add_track(create_test_track(102));
        queue.set_stop_after(102);

        let fired = queue.consume_stop_after_if(102);

        assert!(fired, "consume should return true on match");
        assert_eq!(queue.get_stop_after(), None, "marker should be cleared after firing");
    }

    #[test]
    fn test_consume_stop_after_if_does_not_fire_on_mismatch() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.add_track(create_test_track(102));
        queue.set_stop_after(102);

        let fired = queue.consume_stop_after_if(101);

        assert!(!fired, "consume should return false on mismatch");
        assert_eq!(queue.get_stop_after(), Some(102), "marker should remain on mismatch");
    }

    #[test]
    fn test_consume_stop_after_if_with_no_marker_returns_false() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));

        let fired = queue.consume_stop_after_if(101);

        assert!(!fired);
    }

    // ============ Stop-After Marker — Invalidation on Queue Mutations ============

    #[test]
    fn test_set_queue_invalidates_marker() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.add_track(create_test_track(102));
        queue.set_stop_after(102);

        queue.set_queue(vec![create_test_track(201), create_test_track(202)], None);

        assert_eq!(queue.get_stop_after(), None);
    }

    #[test]
    fn test_clear_invalidates_marker() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.add_track(create_test_track(102));
        queue.set_stop_after(102);

        queue.clear(true);

        assert_eq!(queue.get_stop_after(), None);
    }

    #[test]
    fn test_remove_track_invalidates_marker_when_marked_track_removed() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.add_track(create_test_track(102));
        queue.add_track(create_test_track(103));
        queue.set_stop_after(102);

        queue.remove_track(1); // removes track 102

        assert_eq!(queue.get_stop_after(), None);
    }

    #[test]
    fn test_remove_track_keeps_marker_when_other_track_removed() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.add_track(create_test_track(102));
        queue.add_track(create_test_track(103));
        queue.set_stop_after(102);

        queue.remove_track(0); // removes track 101

        assert_eq!(queue.get_stop_after(), Some(102));
    }

    #[test]
    fn test_move_track_does_not_invalidate_marker() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.add_track(create_test_track(102));
        queue.add_track(create_test_track(103));
        queue.set_stop_after(102);

        queue.move_track(1, 0); // 102 moves to position 0

        assert_eq!(queue.get_stop_after(), Some(102));
    }

    #[test]
    fn test_remove_after_returns_count() {
        let queue = QueueManager::new();
        for id in [101, 102, 103, 104, 105] {
            queue.add_track(create_test_track(id));
        }

        let removed = queue.remove_after(1);

        assert_eq!(removed, 3, "should remove indices 2, 3, 4");
        let state = queue.get_state();
        assert_eq!(state.total_tracks, 2);
    }

    #[test]
    fn test_remove_after_on_last_index_is_noop() {
        let queue = QueueManager::new();
        for id in [101, 102, 103] {
            queue.add_track(create_test_track(id));
        }

        let removed = queue.remove_after(2);

        assert_eq!(removed, 0);
        assert_eq!(queue.get_state().total_tracks, 3);
    }

    #[test]
    fn test_remove_after_with_index_out_of_bounds_is_noop() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.add_track(create_test_track(102));

        let removed = queue.remove_after(99);

        assert_eq!(removed, 0);
        assert_eq!(queue.get_state().total_tracks, 2);
    }

    #[test]
    fn test_remove_after_invalidates_marker_when_in_removed_range() {
        let queue = QueueManager::new();
        for id in [101, 102, 103, 104] {
            queue.add_track(create_test_track(id));
        }
        queue.set_stop_after(103);

        queue.remove_after(1); // removes 103, 104

        assert_eq!(queue.get_stop_after(), None);
    }

    #[test]
    fn test_remove_after_keeps_marker_when_before_range() {
        let queue = QueueManager::new();
        for id in [101, 102, 103, 104] {
            queue.add_track(create_test_track(id));
        }
        queue.set_stop_after(101);

        queue.remove_after(2); // removes 104 only (index 3)

        assert_eq!(queue.get_stop_after(), Some(101));
    }

    #[test]
    fn test_remove_after_keeps_marker_when_at_pivot_index() {
        let queue = QueueManager::new();
        for id in [101, 102, 103, 104] {
            queue.add_track(create_test_track(id));
        }
        queue.set_stop_after(102);

        queue.remove_after(1); // removes indices 2, 3 — track 102 (at index 1) stays

        assert_eq!(queue.get_stop_after(), Some(102));
    }

    #[test]
    fn test_remove_upcoming_after_linear() {
        let queue = QueueManager::new();
        // 101 playing, upcoming = [102, 103, 104, 105].
        queue.set_queue(
            (101..=105).map(create_test_track).collect(),
            Some(0),
        );

        // Keep upcoming positions 0..=1 (102, 103); drop 2, 3 (104, 105).
        let removed = queue.remove_upcoming_after(1);

        assert_eq!(removed, 2);
        let state = queue.get_state_full();
        assert_eq!(state.current_track.map(|t| t.id), Some(101));
        assert_eq!(
            state.upcoming.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![102, 103]
        );
    }

    #[test]
    fn test_remove_upcoming_after_on_last_upcoming_is_noop() {
        let queue = QueueManager::new();
        queue.set_queue((101..=103).map(create_test_track).collect(), Some(0));
        // Upcoming = [102, 103]; position 1 is the last, nothing after it.
        let removed = queue.remove_upcoming_after(1);
        assert_eq!(removed, 0);
        assert_eq!(queue.get_state().total_tracks, 3);
    }

    #[test]
    fn test_remove_upcoming_after_no_current_track() {
        let queue = QueueManager::new();
        // No current index -> every track is upcoming.
        queue.set_queue((101..=104).map(create_test_track).collect(), None);
        let removed = queue.remove_upcoming_after(0);
        assert_eq!(removed, 3);
        let state = queue.get_state_full();
        assert_eq!(
            state.upcoming.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![101]
        );
    }

    #[test]
    fn test_remove_upcoming_after_respects_shuffle_play_order() {
        let queue = QueueManager::new();
        // tracks: 101..105 at indices 0..4. Shuffle play order = [2,0,4,1,3]
        // -> 103(current), then upcoming 101, 105, 102, 104.
        queue.set_queue_with_order(
            (101..=105).map(create_test_track).collect(),
            Some(2),
            true,
            Some(vec![2, 0, 4, 1, 3]),
        );

        // Keep upcoming positions 0..=1 (101, 105); drop 2, 3 (102, 104).
        let removed = queue.remove_upcoming_after(1);

        assert_eq!(removed, 2);
        let state = queue.get_state_full();
        assert_eq!(state.current_track.map(|t| t.id), Some(103));
        assert_eq!(
            state.upcoming.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![101, 105]
        );
    }

    #[test]
    fn test_remove_upcoming_after_invalidates_marker_in_removed_range() {
        let queue = QueueManager::new();
        queue.set_queue((101..=105).map(create_test_track).collect(), Some(0));
        queue.set_stop_after(104); // upcoming position 2 -> removed by after(1)

        queue.remove_upcoming_after(1);

        assert_eq!(queue.get_stop_after(), None);
    }

    #[test]
    fn test_get_state_includes_stop_after() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));
        queue.set_stop_after(101);

        let state = queue.get_state();

        assert_eq!(state.stop_after_track_id, Some(101));
    }

    #[test]
    fn test_get_state_full_returns_uncapped_upcoming() {
        let queue = QueueManager::new();
        // 50 tracks — more than get_state()'s 20-track upcoming cap.
        for i in 1..=50 {
            queue.add_track(create_test_track(i));
        }
        queue.play_index(0);

        let capped = queue.get_state();
        assert_eq!(capped.upcoming.len(), 20, "get_state caps upcoming at 20");

        let full = queue.get_state_full();
        assert_eq!(full.upcoming.len(), 49, "get_state_full returns all upcoming");
        assert_eq!(full.total_tracks, 50);
        assert_eq!(full.upcoming.first().unwrap().id, 2);
        assert_eq!(full.upcoming.last().unwrap().id, 50);
    }

    #[test]
    fn test_get_state_full_returns_uncapped_history() {
        let queue = QueueManager::new();
        for i in 1..=30 {
            queue.add_track(create_test_track(i));
        }
        queue.play_index(0);
        // Advance through 25 tracks — more than get_state()'s 10-entry cap.
        for _ in 0..25 {
            queue.next();
        }

        let capped = queue.get_state();
        assert_eq!(capped.history.len(), 10, "get_state caps history at 10");

        let full = queue.get_state_full();
        assert_eq!(full.history.len(), 25, "get_state_full returns all history");
        // Newest-first ordering: most recently played sits at the front.
        assert_eq!(full.history.first().unwrap().id, 25);
    }

    #[test]
    fn test_get_state_full_no_current_track_returns_all_as_upcoming() {
        let queue = QueueManager::new();
        for i in 1..=5 {
            queue.add_track(create_test_track(i));
        }

        let full = queue.get_state_full();
        assert!(full.current_track.is_none());
        assert_eq!(full.upcoming.len(), 5);
    }

    #[test]
    fn test_get_state_returns_none_when_no_marker() {
        let queue = QueueManager::new();
        queue.add_track(create_test_track(101));

        let state = queue.get_state();

        assert_eq!(state.stop_after_track_id, None);
    }
}
