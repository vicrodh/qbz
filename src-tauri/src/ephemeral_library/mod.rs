//! In-memory ephemeral library for ad-hoc folder playback.
//!
//! The user can point QBZ at a folder that lives outside their library
//! (a downloaded album they haven't decided to keep, an external drive,
//! etc.), browse it, and play tracks from it without anything landing
//! in `local_tracks`. The ephemeral session lives only in memory: a
//! `HashMap<i64, LocalTrack>` keyed by *synthetic ids in the high
//! range* (>= `EPHEMERAL_ID_FLOOR` = 2^48). Synthetic ids in this range
//! are how the rest of the playback pipeline distinguishes ephemeral
//! tracks from DB-resolvable ones — local_tracks autoincrement IDs are
//! orders of magnitude smaller, so any track_id arriving at
//! `v2_library_play_track` at or above the floor is unambiguously
//! ephemeral and gets routed here instead of the DB.
//!
//! The high-positive design (instead of the obvious "use negatives")
//! exists because the queue/playback-context commands serialize ids as
//! `u64` end-to-end (V2QueueTrack, v2_set_playback_context) and reject
//! negative numbers at the serde boundary. Positive ids above the DB
//! range and below 2^53 (JS Number safe limit) are valid u64 *and*
//! survive the JSON round-trip without precision loss.
//!
//! Only one folder is held at a time; opening a new folder replaces the
//! previous session. The state vanishes on app exit by virtue of being
//! in-memory — nothing persists, no migration, no cleanup logic needed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use qbz_library::{
    cue_to_tracks, CueParser, LibraryError, LibraryScanner, LocalTrack, MetadataExtractor,
};
use serde::Serialize;

/// Floor for synthetic ephemeral track ids. Any id at or above this
/// value is an ephemeral track; below it is a DB row id. Set high
/// enough to be impossible to collide with autoincrement DB ids in any
/// realistic library size, low enough to fit in JS Number's safe
/// integer range (2^53 - 1) so the JSON round-trip stays lossless.
pub const EPHEMERAL_ID_FLOOR: i64 = 1 << 48;

#[derive(Debug, Serialize, Clone)]
pub struct EphemeralFolderResult {
    pub folder_path: String,
    pub tracks: Vec<LocalTrack>,
    pub skipped_files: usize,
}

#[derive(Debug)]
pub enum EphemeralError {
    Lock,
    Library(String),
    Io(String),
}

impl std::fmt::Display for EphemeralError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lock => write!(f, "ephemeral library state lock poisoned"),
            Self::Library(msg) => write!(f, "{}", msg),
            Self::Io(msg) => write!(f, "{}", msg),
        }
    }
}

impl From<LibraryError> for EphemeralError {
    fn from(e: LibraryError) -> Self {
        EphemeralError::Library(e.to_string())
    }
}

struct EphemeralLibraryInner {
    tracks: HashMap<i64, LocalTrack>,
    next_id: i64,
    current_folder_path: Option<String>,
}

impl EphemeralLibraryInner {
    fn new() -> Self {
        Self {
            tracks: HashMap::new(),
            next_id: EPHEMERAL_ID_FLOOR,
            current_folder_path: None,
        }
    }

    fn reset(&mut self) {
        self.tracks.clear();
        self.next_id = EPHEMERAL_ID_FLOOR;
        self.current_folder_path = None;
    }
}

pub struct EphemeralLibraryState {
    inner: Mutex<EphemeralLibraryInner>,
}

impl EphemeralLibraryState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(EphemeralLibraryInner::new()),
        }
    }

    /// Scan a folder, extract metadata for every supported audio file
    /// found, assign synthetic negative ids and stash the result. The
    /// previous ephemeral session, if any, is dropped.
    pub fn open_folder(&self, path: &Path) -> Result<EphemeralFolderResult, EphemeralError> {
        if !path.exists() {
            return Err(EphemeralError::Io(format!(
                "Folder does not exist: {}",
                path.display()
            )));
        }
        if !path.is_dir() {
            return Err(EphemeralError::Io(format!(
                "Not a directory: {}",
                path.display()
            )));
        }

        let scanner = LibraryScanner::new();
        let scan = scanner.scan_directory(path)?;

        let mut tracks_out: Vec<LocalTrack> = Vec::with_capacity(scan.audio_files.len());
        let mut skipped_files: usize = 0;

        let mut inner = self.inner.lock().map_err(|_| EphemeralError::Lock)?;
        inner.reset();

        // Cache directory for artwork thumbnails. Same one the regular
        // index uses, so ephemeral artwork piggy-backs on the existing
        // thumbnail pipeline (and gets evicted by the same housekeeping).
        let artwork_cache = crate::library::get_artwork_cache_dir();

        // Two artwork caches keyed at different granularities. The bigger
        // win is the album-level cache: embedded covers are usually
        // identical across every track of an album, so doing extract_artwork
        // (Probe::open + thumbnail encode) 155 times for a 155-track album
        // is wasted I/O. The folder-level cache is a smaller secondary
        // saver for find_folder_artwork (cover.jpg lookup) when albums
        // share the same parent directory.
        let mut album_artwork_cache: HashMap<String, Option<String>> = HashMap::new();
        let mut folder_artwork_cache: HashMap<PathBuf, Option<String>> = HashMap::new();

        // Audio files referenced by CUE sheets. We index those audio files
        // through the CUE path (one logical "album" file gets exploded
        // into N virtual tracks) and skip them in the regular audio loop
        // below — otherwise the user would see both the CUE-derived
        // tracks and a single-row entry for the underlying FLAC/WAV.
        let mut cue_referenced_audio: HashSet<PathBuf> = HashSet::new();

        for cue_path in &scan.cue_files {
            match CueParser::parse(cue_path) {
                Ok(mut cue) => {
                    let audio_path_raw = Path::new(&cue.audio_file).to_path_buf();
                    let canonical = std::fs::canonicalize(&audio_path_raw)
                        .unwrap_or_else(|_| audio_path_raw.clone());
                    if !canonical.exists() {
                        log::warn!(
                            "[ephemeral] CUE references missing audio: {} -> {}",
                            cue_path.display(),
                            audio_path_raw.display()
                        );
                        skipped_files += 1;
                        continue;
                    }
                    cue.audio_file = canonical.to_string_lossy().to_string();

                    // The decoder behind play_data is Symphonia, which
                    // covers FLAC / MP3 / M4A (AAC + ALAC) / ALAC /
                    // WAV / AIFF out of the box (`features = ["all"]`).
                    // APE (Monkey's Audio) and raw BIN (CD-DA dumps
                    // without headers) aren't in that list: playback
                    // either errors out or produces white noise as
                    // Symphonia mis-probes the stream. Skip CUE files
                    // that point at those — better an empty pane than
                    // a track row that explodes on click.
                    let ext_lower = canonical
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_lowercase());
                    let playable_via_cue = matches!(
                        ext_lower.as_deref(),
                        Some("flac" | "mp3" | "m4a" | "alac" | "wav" | "aiff" | "aif")
                    );
                    if !playable_via_cue {
                        log::warn!(
                            "[ephemeral] CUE references unsupported audio format ({:?}) — skipping: {}",
                            ext_lower,
                            canonical.display()
                        );
                        skipped_files += 1;
                        continue;
                    }

                    let properties = match MetadataExtractor::extract_properties(&canonical) {
                        Ok(p) => p,
                        Err(e) => {
                            log::warn!(
                                "[ephemeral] failed to read audio properties for {}: {}",
                                canonical.display(),
                                e
                            );
                            skipped_files += 1;
                            continue;
                        }
                    };
                    let format = MetadataExtractor::detect_format(&canonical);

                    let mut cue_tracks =
                        cue_to_tracks(&cue, properties.duration_secs, format, &properties);
                    if cue_tracks.is_empty() {
                        log::warn!(
                            "[ephemeral] CUE produced no tracks: {}",
                            cue_path.display()
                        );
                        skipped_files += 1;
                        continue;
                    }

                    // CUE = single album: resolve cover once, share across
                    // every CUE-derived track. Use a key derived from the
                    // CUE path so the cache survives even when the
                    // album_group_key field is empty (rare but possible
                    // for CUE files without explicit TITLE/PERFORMER).
                    let album_key = if !cue_tracks[0].album_group_key.is_empty() {
                        cue_tracks[0].album_group_key.clone()
                    } else {
                        format!("cue:{}", cue.file_path)
                    };
                    let artwork = if let Some(cached) = album_artwork_cache.get(&album_key) {
                        cached.clone()
                    } else {
                        let mut found =
                            MetadataExtractor::extract_artwork(&canonical, &artwork_cache);
                        if found.is_none() {
                            if let Some(folder_art) = MetadataExtractor::find_folder_artwork(
                                &canonical,
                                cue.title.as_deref(),
                            ) {
                                found = MetadataExtractor::cache_artwork_file(
                                    Path::new(&folder_art),
                                    &artwork_cache,
                                );
                            }
                        }
                        album_artwork_cache.insert(album_key, found.clone());
                        found
                    };

                    for mut track in cue_tracks.drain(..) {
                        track.id = inner.next_id;
                        inner.next_id += 1;
                        track.source = Some("ephemeral".to_string());
                        track.artwork_path = artwork.clone();
                        inner.tracks.insert(track.id, track.clone());
                        tracks_out.push(track);
                    }
                    cue_referenced_audio.insert(canonical);
                }
                Err(e) => {
                    log::warn!(
                        "[ephemeral] failed to parse CUE {}: {}",
                        cue_path.display(),
                        e
                    );
                    skipped_files += 1;
                }
            }
        }

        for audio_file in &scan.audio_files {
            // Skip audio files that were already exploded into tracks via
            // a CUE sheet — listing them again as a single row would
            // duplicate the album and confuse playback (the CUE-derived
            // track ids are the canonical ones).
            let canonical_audio = std::fs::canonicalize(audio_file)
                .unwrap_or_else(|_| audio_file.clone());
            if cue_referenced_audio.contains(&canonical_audio) {
                continue;
            }

            // The scanner accepts APE because the regular library tracks
            // them for tag/metadata purposes, but Symphonia can't decode
            // Monkey's Audio. In ephemeral mode there is no value in
            // surfacing rows that explode on click — skip them so the
            // pane only shows tracks the user can actually play.
            let ext_lower = audio_file
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_lowercase());
            if matches!(ext_lower.as_deref(), Some("ape")) {
                log::info!(
                    "[ephemeral] skipping APE (no Symphonia decoder): {}",
                    audio_file.display()
                );
                skipped_files += 1;
                continue;
            }
            match MetadataExtractor::extract(audio_file) {
                Ok(mut track) => {
                    track.id = inner.next_id;
                    inner.next_id += 1;
                    track.source = Some("ephemeral".to_string());

                    let album_key = if !track.album_group_key.is_empty() {
                        track.album_group_key.clone()
                    } else {
                        format!(
                            "{}|||{}",
                            track.album,
                            track.album_artist.as_deref().unwrap_or(&track.artist)
                        )
                    };

                    let artwork = if let Some(cached) = album_artwork_cache.get(&album_key) {
                        cached.clone()
                    } else {
                        let mut found =
                            MetadataExtractor::extract_artwork(audio_file, &artwork_cache);
                        if found.is_none() {
                            let folder_key = audio_file
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| audio_file.to_path_buf());
                            let folder_art = folder_artwork_cache
                                .entry(folder_key)
                                .or_insert_with(|| {
                                    MetadataExtractor::find_folder_artwork(
                                        audio_file,
                                        Some(track.album.as_str()),
                                    )
                                })
                                .clone();
                            if let Some(folder_art) = folder_art {
                                found = MetadataExtractor::cache_artwork_file(
                                    std::path::Path::new(&folder_art),
                                    &artwork_cache,
                                );
                            }
                        }
                        album_artwork_cache.insert(album_key, found.clone());
                        found
                    };
                    track.artwork_path = artwork;

                    inner.tracks.insert(track.id, track.clone());
                    tracks_out.push(track);
                }
                Err(e) => {
                    log::warn!(
                        "[ephemeral] failed to extract metadata from {}: {}",
                        audio_file.display(),
                        e
                    );
                    skipped_files += 1;
                }
            }
        }

        let folder_path = path.display().to_string();
        inner.current_folder_path = Some(folder_path.clone());

        log::info!(
            "[ephemeral] opened {} ({} tracks, {} skipped)",
            folder_path,
            tracks_out.len(),
            skipped_files
        );

        Ok(EphemeralFolderResult {
            folder_path,
            tracks: tracks_out,
            skipped_files,
        })
    }

    pub fn clear(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.reset();
        }
    }

    /// Resolve a synthetic negative id to the cached `LocalTrack`. Returns
    /// `None` if the id is unknown (stale queue entry from a previous
    /// session, race against `clear`, etc.).
    pub fn get_track(&self, id: i64) -> Option<LocalTrack> {
        let inner = self.inner.lock().ok()?;
        inner.tracks.get(&id).cloned()
    }
}

impl Default for EphemeralLibraryState {
    fn default() -> Self {
        Self::new()
    }
}
