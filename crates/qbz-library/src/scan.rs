//! Progress-emitting library scan (frontend-agnostic).
//!
//! Lifts the scan-orchestration loop that lived in the Tauri command
//! `v2_library_scan` / `v2_library_scan_folder` into the core crate so any
//! frontend (Slint, TUI) can drive it. The Tauri side polled an
//! `Arc<Mutex<ScanProgress>>` over the IPC boundary; in-process callers get
//! the same information pushed through `on_event` and check `cancel` at every
//! file boundary. The per-file logic (CUE-first, sidecar override, embedded →
//! folder artwork, insert, missing-file cleanup) is replicated exactly.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    cue_to_tracks, AlbumTagSidecar, CueParser, LibraryDatabase, LibraryError, LibraryScanner,
    LocalTrack, MetadataExtractor, ScanError, ScanStatus,
};

/// One step of a scan, pushed to the caller. The caller maps these onto its
/// own progress surface (and may coalesce the per-file stream).
pub enum ScanEvent {
    /// Scan started (status = Scanning, counters reset).
    Started,
    /// A folder's file count was folded into the running total.
    TotalsAdded { total: u32 },
    /// A file is about to be processed (caller trims to a basename).
    FileStarted { path: String },
    /// A file finished (processed/total advanced).
    FileDone { processed: u32, total: u32 },
    /// Entering the missing-file cleanup phase.
    Cleanup,
    /// Terminal: Complete / Cancelled / Error, with any per-file errors.
    Finished {
        status: ScanStatus,
        errors: Vec<ScanError>,
    },
}

fn normalize_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Apply a per-album sidecar override to a scanned track, caching the sidecar
/// read per album group key. Mirrors Tauri's
/// `library_apply_sidecar_override_if_present`.
fn apply_sidecar_override(
    track: &mut LocalTrack,
    cache: &mut HashMap<String, Option<AlbumTagSidecar>>,
) {
    let group_key = track.album_group_key.trim();
    if group_key.is_empty() {
        return;
    }
    let cached = cache.entry(group_key.to_string()).or_insert_with(|| {
        let album_dir = Path::new(group_key);
        if !album_dir.is_dir() {
            return None;
        }
        crate::tag_sidecar::read_album_sidecar(album_dir).unwrap_or(None)
    });
    if let Some(sidecar) = cached.as_ref() {
        crate::tag_sidecar::apply_sidecar_to_track(track, sidecar);
    }
}

/// Parse a CUE sheet into its virtual tracks and insert them. Mirrors Tauri's
/// `library_process_cue_file` (the per-track artwork loop already covers what
/// the deprecated `update_album_group_artwork` shortcut did, so it is omitted).
fn process_cue_file(
    db: &LibraryDatabase,
    cue_path: &Path,
    artwork_cache: &Path,
) -> Result<(), String> {
    let mut cue = CueParser::parse(cue_path).map_err(|e| e.to_string())?;
    let audio_path = normalize_path(Path::new(&cue.audio_file));
    if !audio_path.exists() {
        return Err(format!("Audio file not found: {}", cue.audio_file));
    }
    cue.audio_file = audio_path.to_string_lossy().to_string();

    let properties = MetadataExtractor::extract_properties(&audio_path).map_err(|e| e.to_string())?;
    let format = MetadataExtractor::detect_format(&audio_path);
    let mut tracks = cue_to_tracks(&cue, properties.duration_secs, format, &properties);

    if let Some(group_key) = tracks
        .first()
        .map(|t| t.album_group_key.trim().to_string())
        .filter(|k| !k.is_empty())
    {
        let album_dir = Path::new(&group_key);
        if album_dir.is_dir() {
            if let Ok(Some(sidecar)) = crate::tag_sidecar::read_album_sidecar(album_dir) {
                for t in tracks.iter_mut() {
                    crate::tag_sidecar::apply_sidecar_to_track(t, &sidecar);
                }
            }
        }
    }

    let mut artwork = MetadataExtractor::extract_artwork(&audio_path, artwork_cache);
    if artwork.is_none() {
        if let Some(folder_art) =
            MetadataExtractor::find_folder_artwork(&audio_path, cue.title.as_deref())
        {
            artwork =
                MetadataExtractor::cache_artwork_file(Path::new(&folder_art), artwork_cache);
        }
    }
    if let Some(p) = artwork.as_ref() {
        for t in tracks.iter_mut() {
            t.artwork_path = Some(p.clone());
        }
    }

    for track in &tracks {
        db.insert_track(track).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Scan the library (or a single folder set) with progress + cancellation.
///
/// `folder_ids = None` scans every ENABLED folder (full scan); `Some(&[id])`
/// scans only those enabled folders (single/per-folder parity). `artwork_cache`
/// is the cache dir for extracted/copied covers. `cancel` is checked at every
/// file boundary; on cancel the scan returns early with `Finished{Cancelled}`
/// and does NOT run cleanup (Tauri parity). `on_event` receives each step.
///
/// Improvements over the Tauri loop (both verified against source):
/// - the full scan updates each folder's `last_scan` on success (Tauri only
///   did this for single-folder scans);
/// - network status is re-detected for every scanned folder that is not
///   user-overridden (Tauri only did this for single-folder scans).
pub fn scan_with_progress(
    db: &LibraryDatabase,
    folder_ids: Option<&[i64]>,
    artwork_cache: &Path,
    cancel: &AtomicBool,
    on_event: &(dyn Fn(ScanEvent) + Send + Sync),
) -> Result<(), LibraryError> {
    let all = db.get_folders_with_metadata()?;
    let targets: Vec<crate::LibraryFolder> = match folder_ids {
        None => all.into_iter().filter(|f| f.enabled).collect(),
        Some(ids) => all
            .into_iter()
            .filter(|f| f.enabled && ids.contains(&f.id))
            .collect(),
    };
    if targets.is_empty() {
        return Err(LibraryError::Other("No library folders to scan".to_string()));
    }
    let single = folder_ids.is_some();

    // Refresh network detection for non-overridden folders being scanned.
    for f in &targets {
        if f.user_override_network {
            continue;
        }
        let p = Path::new(&f.path);
        let is_net = crate::mount_info::is_network_path(p);
        if is_net != f.is_network {
            let fs = if is_net {
                crate::mount_info::network_fs_label(p)
            } else {
                None
            };
            let _ = db.update_folder_settings(
                f.id,
                f.alias.as_deref(),
                f.enabled,
                is_net,
                fs.as_deref(),
                false,
            );
        }
    }

    on_event(ScanEvent::Started);

    let scanner = LibraryScanner::new();
    let mut all_errors: Vec<ScanError> = Vec::new();
    let mut sidecar_cache: HashMap<String, Option<AlbumTagSidecar>> = HashMap::new();
    let mut total: u32 = 0;
    let mut processed: u32 = 0;

    for folder in &targets {
        let scan_result = match scanner.scan_directory(Path::new(&folder.path)) {
            Ok(r) => r,
            Err(e) => {
                all_errors.push(ScanError {
                    file_path: folder.path.clone(),
                    error: e.to_string(),
                });
                continue;
            }
        };

        total += (scan_result.audio_files.len() + scan_result.cue_files.len()) as u32;
        on_event(ScanEvent::TotalsAdded { total });

        // CUE files first (one file -> several virtual tracks).
        for cue_path in &scan_result.cue_files {
            if cancel.load(Ordering::Relaxed) {
                on_event(ScanEvent::Finished {
                    status: ScanStatus::Cancelled,
                    errors: std::mem::take(&mut all_errors),
                });
                return Ok(());
            }
            on_event(ScanEvent::FileStarted {
                path: cue_path.to_string_lossy().to_string(),
            });
            if let Err(e) = process_cue_file(db, cue_path, artwork_cache) {
                all_errors.push(ScanError {
                    file_path: cue_path.to_string_lossy().to_string(),
                    error: e,
                });
            }
            processed += 1;
            on_event(ScanEvent::FileDone { processed, total });
        }

        // Audio files (skipping any referenced by a CUE sheet).
        let cue_audio_files: HashSet<String> = scan_result
            .cue_files
            .iter()
            .filter_map(|p| {
                CueParser::parse(p).ok().map(|cue| {
                    normalize_path(Path::new(&cue.audio_file))
                        .to_string_lossy()
                        .to_string()
                })
            })
            .collect();

        let mut folder_artwork_cache: HashMap<PathBuf, Option<String>> = HashMap::new();

        for audio_path in &scan_result.audio_files {
            if cancel.load(Ordering::Relaxed) {
                on_event(ScanEvent::Finished {
                    status: ScanStatus::Cancelled,
                    errors: std::mem::take(&mut all_errors),
                });
                return Ok(());
            }

            let canonical = normalize_path(audio_path);
            let path_str = canonical.to_string_lossy().to_string();
            if cue_audio_files.contains(&path_str) {
                processed += 1;
                on_event(ScanEvent::FileDone { processed, total });
                continue;
            }

            on_event(ScanEvent::FileStarted {
                path: path_str.clone(),
            });

            match MetadataExtractor::extract(&canonical) {
                Ok(mut track) => {
                    apply_sidecar_override(&mut track, &mut sidecar_cache);
                    let mut artwork = MetadataExtractor::extract_artwork(&canonical, artwork_cache);
                    if artwork.is_none() {
                        let album_hint: Option<String> = if !track.album_group_title.is_empty() {
                            Some(track.album_group_title.clone())
                        } else {
                            Some(track.album.clone())
                        };
                        let folder_dir = canonical
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|| canonical.clone());
                        let cached = folder_artwork_cache
                            .entry(folder_dir)
                            .or_insert_with(|| {
                                MetadataExtractor::find_folder_artwork(
                                    &canonical,
                                    album_hint.as_deref(),
                                )
                            })
                            .clone();
                        if let Some(folder_art) = cached {
                            artwork = MetadataExtractor::cache_artwork_file(
                                Path::new(&folder_art),
                                artwork_cache,
                            );
                        }
                    }
                    track.artwork_path = artwork;

                    if let Err(e) = db.insert_track(&track) {
                        all_errors.push(ScanError {
                            file_path: path_str,
                            error: e.to_string(),
                        });
                    }
                }
                Err(e) => all_errors.push(ScanError {
                    file_path: path_str,
                    error: e.to_string(),
                }),
            }

            processed += 1;
            on_event(ScanEvent::FileDone { processed, total });
        }
    }

    // Cleanup: remove tracks whose files no longer exist. Full scan checks the
    // whole DB; single-folder scan only the scanned folders' subtrees.
    //
    // GUARD: a network folder whose mount is currently DOWN must not have its
    // subtree treated as missing — while unmounted every stat fails, and
    // deleting the rows would wipe a whole folder's index over a transient
    // condition (e.g. a reboot where the share didn't auto-mount). Those
    // subtrees are skipped; they rehabilitate on the next scan after remount.
    on_event(ScanEvent::Cleanup);
    let folder_prefix = |path: &str| {
        if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{}/", path)
        }
    };
    let unavailable_prefixes: Vec<String> = db
        .get_folders_with_metadata()
        .map(|folders| {
            folders
                .iter()
                .filter(|f| f.is_network && std::fs::read_dir(&f.path).is_err())
                .map(|f| folder_prefix(&f.path))
                .collect()
        })
        .unwrap_or_default();
    let under_unavailable =
        |p: &str| unavailable_prefixes.iter().any(|pre| p.starts_with(pre.as_str()));
    if let Ok(tracks) = db.get_all_track_paths() {
        let missing: Vec<i64> = if single {
            let prefixes: Vec<String> =
                targets.iter().map(|f| folder_prefix(&f.path)).collect();
            tracks
                .iter()
                .filter(|(_, p)| {
                    prefixes.iter().any(|pre| p.starts_with(pre))
                        && !under_unavailable(p)
                        && !Path::new(p).exists()
                })
                .map(|(id, _)| *id)
                .collect()
        } else {
            tracks
                .iter()
                .filter(|(_, p)| !under_unavailable(p) && !Path::new(p).exists())
                .map(|(id, _)| *id)
                .collect()
        };
        for chunk in missing.chunks(500) {
            let _ = db.delete_tracks_by_ids(chunk);
        }
    }

    // Stamp each scanned folder's last_scan (improvement: full scan too).
    let now = now_secs();
    for f in &targets {
        let _ = db.update_folder_scan_time(&f.path, now);
    }

    on_event(ScanEvent::Finished {
        status: ScanStatus::Complete,
        errors: all_errors,
    });
    Ok(())
}
