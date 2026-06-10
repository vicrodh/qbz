//! Settings > Local Library controller (Slint).
//!
//! Hosts the folder-management surface that Tauri renders inline in the
//! browse view's gear panel: the folder list (add / remove / edit / enable /
//! alias / network override), maintenance (cleanup missing files), and the
//! two-step danger-zone clear. The scan engine + progress live in Slice B.
//!
//! All DB access goes through the frontend-agnostic `qbz_library` crate via
//! `crate::library_db::with_db(|db| …)` on `spawn_blocking` (rusqlite is
//! blocking). The authoritative full folder set (with per-row selection) is
//! kept in a module static; the Slint `LibraryFoldersState.folders` model is
//! the filtered render set derived from it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use slint::{ComponentHandle, ModelRc, VecModel, Weak};

use crate::{
    AppWindow, LibFolderEditState, LibraryFolderItem, LibraryFoldersState, LibraryScanState,
};

/// One registered folder + its UI selection state. The authoritative copy;
/// the Slint model is derived (filtered) from this.
#[derive(Clone)]
struct FolderData {
    id: i64,
    path: String,
    alias: Option<String>,
    enabled: bool,
    is_network: bool,
    network_fs_type: Option<String>,
    user_override_network: bool,
    last_scan: Option<i64>,
    accessible: bool,
    selected: bool,
}

static FOLDERS: LazyLock<Mutex<Vec<FolderData>>> = LazyLock::new(|| Mutex::new(Vec::new()));
/// Bumped on every (re)load so a stale in-flight load is dropped on apply.
static FOLDERS_GEN: AtomicU64 = AtomicU64::new(0);

fn folders_lock() -> std::sync::MutexGuard<'static, Vec<FolderData>> {
    FOLDERS.lock().unwrap_or_else(|e| e.into_inner())
}

fn display_name(f: &FolderData) -> String {
    match f.alias.as_deref() {
        Some(a) if !a.is_empty() => a.to_string(),
        _ => f.path.clone(),
    }
}

/// Format a folder's `last_scan` (unix seconds, 0/None = never) for display.
fn last_scan_label(ts: Option<i64>) -> String {
    match ts {
        None | Some(0) => "Never".to_string(),
        Some(secs) => chrono::DateTime::from_timestamp(secs, 0)
            .map(|dt| {
                dt.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|| "Never".to_string()),
    }
}

/// fs-type label -> the modal's QbzSelect index (0=auto,1 cifs … 8 other).
fn fs_label_to_index(label: Option<&str>) -> i32 {
    match label.unwrap_or("") {
        "cifs" => 1,
        "nfs" => 2,
        "sshfs" => 3,
        "rclone" => 4,
        "webdav" => 5,
        "glusterfs" => 6,
        "ceph" => 7,
        "other" => 8,
        _ => 0,
    }
}

fn to_item(f: &FolderData) -> LibraryFolderItem {
    LibraryFolderItem {
        id: f.id as i32,
        path: f.path.clone().into(),
        alias: f.alias.clone().unwrap_or_default().into(),
        display_name: display_name(f).into(),
        enabled: f.enabled,
        is_network: f.is_network,
        network_fs_type: f.network_fs_type.clone().unwrap_or_default().into(),
        user_override_network: f.user_override_network,
        last_scan: f.last_scan.unwrap_or(0) as i32,
        last_scan_label: last_scan_label(f.last_scan).into(),
        accessible: f.accessible,
        selected: f.selected,
    }
}

/// Derive the filtered render model + selected-count from the static set.
fn derive(window: &AppWindow) {
    let s = window.global::<LibraryFoldersState>();
    let filter = s.get_filter().to_lowercase();
    let q = filter.trim();
    let guard = folders_lock();
    let items: Vec<LibraryFolderItem> = guard
        .iter()
        .filter(|f| {
            q.is_empty()
                || display_name(f).to_lowercase().contains(q)
                || f.path.to_lowercase().contains(q)
        })
        .map(to_item)
        .collect();
    let selected = guard.iter().filter(|f| f.selected).count() as i32;
    drop(guard);
    s.set_folders(ModelRc::new(VecModel::from(items)));
    s.set_selected_count(selected);
}

/// (Re)load the folder list. Pure read of the core `get_folders_with_metadata`
/// (the network re-detect + write the Tauri command does lives only in that
/// command, not the core fn — so this has no side effects). Selection is
/// preserved by id across the reload. Network folders get an async
/// accessibility check.
pub fn load_folders(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let gen = FOLDERS_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<LibraryFoldersState>().set_loading(true);
    });
    let weak2 = weak.clone();
    let check_handle = handle.clone();
    handle.spawn(async move {
        let rows = tokio::task::spawn_blocking(|| {
            crate::library_db::with_db(|db| db.get_folders_with_metadata())
        })
        .await
        .ok()
        .flatten();

        let Some(rows) = rows else {
            let _ = weak2.upgrade_in_event_loop(|w| {
                w.global::<LibraryFoldersState>().set_loading(false);
            });
            crate::toast::error_weak(&weak2, "Couldn't load library folders");
            return;
        };

        // Preserve selection across reloads.
        let prev_sel: std::collections::HashSet<i64> =
            folders_lock().iter().filter(|f| f.selected).map(|f| f.id).collect();

        let data: Vec<FolderData> = rows
            .into_iter()
            .map(|f| FolderData {
                selected: prev_sel.contains(&f.id),
                accessible: true,
                id: f.id,
                path: f.path,
                alias: f.alias,
                enabled: f.enabled,
                is_network: f.is_network,
                network_fs_type: f.network_fs_type,
                user_override_network: f.user_override_network,
                last_scan: f.last_scan,
            })
            .collect();

        let network: Vec<(i64, String)> = data
            .iter()
            .filter(|f| f.is_network)
            .map(|f| (f.id, f.path.clone()))
            .collect();

        *folders_lock() = data;

        let _ = weak2.upgrade_in_event_loop(move |w| {
            if FOLDERS_GEN.load(Ordering::SeqCst) != gen {
                return;
            }
            w.global::<LibraryFoldersState>().set_loading(false);
            derive(&w);
        });

        for (id, path) in network {
            check_accessible(weak2.clone(), check_handle.clone(), id, path);
        }
    });
}

/// Update one folder's accessibility in the static + UI (and the open modal).
fn update_accessible(weak: &Weak<AppWindow>, id: i64, accessible: bool) {
    {
        let mut g = folders_lock();
        if let Some(f) = g.iter_mut().find(|f| f.id == id) {
            f.accessible = accessible;
        }
    }
    let _ = weak.upgrade_in_event_loop(move |w| {
        derive(&w);
        let es = w.global::<LibFolderEditState>();
        if es.get_open() && es.get_folder_id() as i64 == id {
            es.set_accessible(accessible);
            es.set_checking_accessible(false);
        }
    });
}

/// Check a (network) folder's accessibility. Mirrors Tauri: exists? then
/// read_dir under a 6s timeout; on timeout fall back to exists() so a
/// slow-but-mounted share isn't falsely flagged unavailable.
pub fn check_accessible(
    weak: Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    id: i64,
    path: String,
) {
    handle.spawn(async move {
        if !std::path::Path::new(&path).exists() {
            update_accessible(&weak, id, false);
            return;
        }
        let p = path.clone();
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(6),
            tokio::task::spawn_blocking(move || std::fs::read_dir(&p).is_ok()),
        )
        .await;
        let accessible = match res {
            Ok(Ok(ok)) => ok,
            Ok(Err(_)) => false,
            Err(_) => std::path::Path::new(&path).exists(),
        };
        update_accessible(&weak, id, accessible);
    });
}

/// Add a folder via the native directory picker, auto-detecting network type.
pub fn add_folder(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let h = handle.clone();
    handle.spawn(async move {
        let Some(dir) = rfd::AsyncFileDialog::new()
            .set_title("Select music folder")
            .pick_folder()
            .await
        else {
            return;
        };
        let path = dir.path().to_string_lossy().to_string();
        let p = path.clone();
        let (added, is_net) = tokio::task::spawn_blocking(move || {
            let pb = std::path::Path::new(&p);
            let is_net = qbz_library::is_network_path(pb);
            let fs = if is_net {
                qbz_library::network_fs_label(pb)
            } else {
                None
            };
            let ok = crate::library_db::with_db(|db| {
                db.add_folder_with_network_info(&p, is_net, fs.as_deref())
            })
            .is_some();
            (ok, is_net)
        })
        .await
        .unwrap_or((false, false));

        if !added {
            crate::toast::error_weak(&weak, "Couldn't add folder");
            return;
        }
        if is_net {
            crate::toast::success_weak(&weak, "Network folder detected");
        }
        load_folders(weak, h);
    });
}

/// Bulk-remove the selected folders (with confirm + cascade track delete).
pub fn remove_folders(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    // Removes only the LocalLibrary DB entries + their indexed tracks — never
    // the files on disk. Reversible (re-add + re-scan reindexes), so we skip a
    // confirm dialog: rfd's message dialog needs `zenity`, which isn't present
    // on every Linux box, and silently fails-closed there (the original "delete
    // does nothing" bug). A Toast gives feedback instead.
    let paths: Vec<String> = folders_lock()
        .iter()
        .filter(|f| f.selected)
        .map(|f| f.path.clone())
        .collect();
    if paths.is_empty() {
        return;
    }
    let count = paths.len();
    let h = handle.clone();
    handle.spawn(async move {
        let paths2 = paths.clone();
        let keys = tokio::task::spawn_blocking(move || {
            crate::library_db::with_db(|db| {
                let mut keys: Vec<String> = Vec::new();
                for p in &paths2 {
                    keys.extend(db.album_keys_in_folder(p).unwrap_or_default());
                    db.remove_folder_with_tracks(p)?;
                }
                Ok(keys)
            })
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        crate::recently::prune_albums(&keys);
        crate::toast::success_weak(&weak, format!("Removed {count} folder(s)"));
        load_folders(weak, h);
    });
}

/// Remove ONE folder by id (confirm + cascade track delete). The per-row
/// delete button; independent of the multi-select state, so a previously
/// added folder can be removed without first selecting it + the toolbar trash.
pub fn remove_folder(weak: Weak<AppWindow>, handle: tokio::runtime::Handle, id: i64) {
    // DB-only removal (entry + indexed tracks), never the files. No confirm
    // dialog — see remove_folders for why (zenity-less boxes fail-closed).
    let (path, name) = {
        let g = folders_lock();
        match g.iter().find(|f| f.id == id) {
            Some(f) => (f.path.clone(), display_name(f)),
            None => return,
        }
    };
    let h = handle.clone();
    handle.spawn(async move {
        let p = path.clone();
        let result = tokio::task::spawn_blocking(move || {
            crate::library_db::with_db(|db| {
                // Capture album keys BEFORE the delete so we can prune them out
                // of Recently Played too (not just the DB rows).
                let keys = db.album_keys_in_folder(&p).unwrap_or_default();
                let n = db.remove_folder_with_tracks(&p)?;
                Ok((n, keys))
            })
        })
        .await
        .ok()
        .flatten();
        let (n, keys) = result.unwrap_or((0, Vec::new()));
        crate::recently::prune_albums(&keys);
        crate::toast::success_weak(&weak, format!("Removed \"{name}\" ({n} tracks)"));
        load_folders(weak, h);
    });
}

/// Toggle one folder's selection (UI state in the static), then re-derive.
pub fn toggle_select(weak: Weak<AppWindow>, id: i64) {
    {
        let mut g = folders_lock();
        if let Some(f) = g.iter_mut().find(|f| f.id == id) {
            f.selected = !f.selected;
        }
    }
    let _ = weak.upgrade_in_event_loop(|w| derive(&w));
}

/// Open the folder-settings modal for `id` (or the single selected folder
/// when `id == 0`, as the toolbar Edit button passes).
pub fn edit_folder(weak: Weak<AppWindow>, handle: tokio::runtime::Handle, id: i64) {
    let f = {
        let g = folders_lock();
        if id > 0 {
            g.iter().find(|f| f.id == id).cloned()
        } else {
            let sel: Vec<FolderData> = g.iter().filter(|f| f.selected).cloned().collect();
            if sel.len() == 1 {
                Some(sel[0].clone())
            } else {
                None
            }
        }
    };
    let Some(f) = f else {
        return;
    };
    let is_network = f.is_network;
    let fid = f.id;
    let path = f.path.clone();
    let _ = weak.upgrade_in_event_loop(move |w| {
        let es = w.global::<LibFolderEditState>();
        es.set_folder_id(f.id as i32);
        es.set_path(f.path.clone().into());
        es.set_alias(f.alias.clone().unwrap_or_default().into());
        es.set_enabled(f.enabled);
        es.set_is_network(f.is_network);
        es.set_user_override_network(f.user_override_network);
        es.set_fs_type_index(fs_label_to_index(f.network_fs_type.as_deref()));
        es.set_accessible(f.accessible);
        es.set_checking_accessible(f.is_network);
        es.set_last_scan_label(last_scan_label(f.last_scan).into());
        es.set_open(true);
    });
    if is_network {
        check_accessible(weak, handle, fid, path);
    }
}

/// Persist folder settings from the modal. fs-type "auto" re-detects (network
/// only); a non-auto label is stored verbatim.
#[allow(clippy::too_many_arguments)]
pub fn save_folder_settings(
    weak: Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    id: i64,
    alias: String,
    enabled: bool,
    is_network: bool,
    fs_type: String,
    user_override: bool,
) {
    let path = folders_lock()
        .iter()
        .find(|f| f.id == id)
        .map(|f| f.path.clone())
        .unwrap_or_default();
    let h = handle.clone();
    handle.spawn(async move {
        let ok = tokio::task::spawn_blocking(move || {
            let fs_opt: Option<String> = if !is_network {
                None
            } else if fs_type == "auto" {
                qbz_library::network_fs_label(std::path::Path::new(&path))
            } else {
                Some(fs_type)
            };
            let alias_opt = if alias.trim().is_empty() {
                None
            } else {
                Some(alias)
            };
            crate::library_db::with_db(|db| {
                db.update_folder_settings(
                    id,
                    alias_opt.as_deref(),
                    enabled,
                    is_network,
                    fs_opt.as_deref(),
                    user_override,
                )
            })
            .is_some()
        })
        .await
        .unwrap_or(false);

        if !ok {
            crate::toast::error_weak(&weak, "Couldn't save folder settings");
            return;
        }
        let _ = weak.upgrade_in_event_loop(|w| {
            w.global::<LibFolderEditState>().set_open(false);
        });
        load_folders(weak, h);
    });
}

/// Change a folder's path via the picker (resets its last_scan; rejects dups).
pub fn change_folder_path(weak: Weak<AppWindow>, handle: tokio::runtime::Handle, id: i64) {
    let h = handle.clone();
    handle.spawn(async move {
        let Some(dir) = rfd::AsyncFileDialog::new()
            .set_title("Select music folder")
            .pick_folder()
            .await
        else {
            return;
        };
        let new_path = dir.path().to_string_lossy().to_string();
        let np = new_path.clone();
        let ok = tokio::task::spawn_blocking(move || {
            crate::library_db::with_db(|db| db.update_folder_path(id, &np)).is_some()
        })
        .await
        .unwrap_or(false);

        if !ok {
            crate::toast::error_weak(&weak, "Couldn't change folder location (path may already exist)");
            return;
        }
        let np2 = new_path.clone();
        let _ = weak.upgrade_in_event_loop(move |w| {
            let es = w.global::<LibFolderEditState>();
            if es.get_folder_id() as i64 == id {
                es.set_path(np2.into());
                es.set_last_scan_label("Never".into());
            }
        });
        load_folders(weak, h);
    });
}

/// Remove tracks whose files no longer exist. Reads paths, drops the DB lock,
/// stats outside the lock, then deletes in chunks (avoids the Tauri
/// under-lock stat stall). Inline status auto-clears after 3s.
pub fn cleanup_missing(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    {
        // Re-entry guard.
        if let Some(w) = weak.upgrade() {
            let s = w.global::<LibraryFoldersState>();
            if s.get_cleaning_missing() {
                return;
            }
            s.set_cleaning_missing(true);
            s.set_cleanup_status("Scanning track paths...".into());
        }
    }
    let h = handle.clone();
    handle.spawn(async move {
        let result = tokio::task::spawn_blocking(|| {
            let paths = crate::library_db::with_db(|db| db.get_all_track_paths())?;
            // Same guard as the scan's cleanup phase: a network folder whose
            // mount is DOWN right now stats as missing for every file — that
            // is "unreachable", not "deleted". Skip those subtrees so a
            // maintenance click while a share is unmounted can't wipe its
            // index.
            let skip: Vec<String> =
                crate::library_db::with_db(|db| db.get_folders_with_metadata())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|f| f.is_network && std::fs::read_dir(&f.path).is_err())
                    .map(|f| {
                        if f.path.ends_with('/') {
                            f.path
                        } else {
                            format!("{}/", f.path)
                        }
                    })
                    .collect();
            let checked = paths.len();
            let missing: Vec<i64> = paths
                .into_iter()
                .filter(|(_, p)| {
                    !skip.iter().any(|pre| p.starts_with(pre.as_str()))
                        && !std::path::Path::new(p).exists()
                })
                .map(|(id, _)| id)
                .collect();
            let mut removed = 0usize;
            if !missing.is_empty() {
                for chunk in missing.chunks(500) {
                    removed += crate::library_db::with_db(|db| db.delete_tracks_by_ids(chunk))
                        .unwrap_or(0);
                }
            }
            Some((checked, removed))
        })
        .await
        .ok()
        .flatten();

        let (status, toast_ok) = match result {
            Some((checked, removed)) if removed > 0 => {
                (format!("Removed {removed} of {checked} tracks"), true)
            }
            Some((checked, _)) => (format!("Checked {checked} tracks - all OK"), true),
            None => ("Cleanup failed".to_string(), false),
        };

        if let Some(w) = weak.upgrade() {
            let s = w.global::<LibraryFoldersState>();
            s.set_cleaning_missing(false);
            s.set_cleanup_status(status.clone().into());
        }
        if toast_ok {
            crate::toast::success_weak(&weak, status);
        } else {
            crate::toast::error_weak(&weak, "Couldn't clean up missing files");
        }

        // Auto-clear the inline status after 3s.
        let weak_clear = weak.clone();
        h.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let _ = weak_clear.upgrade_in_event_loop(|w| {
                w.global::<LibraryFoldersState>().set_cleanup_status("".into());
            });
        });
        load_folders(weak, h);
    });
}

/// Two-step danger-zone clear of all indexed tracks (audio files untouched).
pub fn clear_library(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let h = handle.clone();
    handle.spawn(async move {
        let step1 = rfd::AsyncMessageDialog::new()
            .set_title("Clear library database?")
            .set_description(
                "This removes ALL indexed tracks from the database. Your audio files are NOT deleted. You will need to re-scan your folders afterward.",
            )
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            .await;
        if step1 != rfd::MessageDialogResult::Yes {
            return;
        }
        let step2 = rfd::AsyncMessageDialog::new()
            .set_title("Are you absolutely sure?")
            .set_description("This action cannot be undone.")
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            .await;
        if step2 != rfd::MessageDialogResult::Yes {
            return;
        }

        let _ = weak.upgrade_in_event_loop(|w| {
            w.global::<LibraryFoldersState>().set_clearing_library(true);
        });
        let ok = tokio::task::spawn_blocking(|| {
            crate::library_db::with_db(|db| db.clear_all_tracks()).is_some()
        })
        .await
        .unwrap_or(false);

        let _ = weak.upgrade_in_event_loop(|w| {
            w.global::<LibraryFoldersState>().set_clearing_library(false);
            // Reset the browse models so the tabs re-fetch on next visit.
            crate::local_library::reset_browse_models(&w);
        });
        if ok {
            crate::toast::success_weak(&weak, "Library database cleared");
        } else {
            crate::toast::error_weak(&weak, "Couldn't clear the library database");
        }
        load_folders(weak, h);
    });
}

/// Re-derive after the filter changed (the text is two-way bound already).
pub fn set_filter(weak: Weak<AppWindow>) {
    let _ = weak.upgrade_in_event_loop(|w| derive(&w));
}

// ================================ Scan ====================================

/// Cancel token for the running scan (the port's equivalent of Tauri's
/// `LibraryState.scan_cancel`). `stop_scan` sets it; the core loop checks it
/// at every file boundary.
static SCAN_CANCEL: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));

fn basename(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

/// ~100ms coalescing for the per-file event stream (a 16K-track scan would
/// otherwise flood the event loop). Terminal/phase events bypass this.
fn throttle_ok(last: &Mutex<std::time::Instant>) -> bool {
    let mut g = last.lock().unwrap_or_else(|e| e.into_inner());
    if g.elapsed() >= std::time::Duration::from_millis(100) {
        *g = std::time::Instant::now();
        true
    } else {
        false
    }
}

/// Run a scan (full when `ids` is None, else the given enabled folders) on a
/// blocking thread, pushing throttled progress to `LibraryScanState`. On
/// finish: reload the folder list, reset the browse models so the tabs
/// re-fetch, and toast the outcome.
fn run_scan(weak: Weak<AppWindow>, handle: tokio::runtime::Handle, ids: Option<Vec<i64>>) {
    SCAN_CANCEL.store(false, Ordering::SeqCst);
    let _ = weak.upgrade_in_event_loop(|w| {
        let s = w.global::<LibraryScanState>();
        s.set_scanning(true);
        s.set_scan_status(1);
        s.set_total_files(0);
        s.set_processed_files(0);
        s.set_progress(0.0);
        s.set_current_file("".into());
        s.set_error_count(0);
    });

    let h = handle.clone();
    handle.spawn_blocking(move || {
        let artwork_cache = crate::library_db::artwork_cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let cancel = SCAN_CANCEL.clone();
        let weak_sink = weak.clone();
        let last = Mutex::new(std::time::Instant::now());

        let sink = move |ev: qbz_library::ScanEvent| {
            use qbz_library::ScanEvent::*;
            match ev {
                Started => {}
                TotalsAdded { total } => {
                    let _ = weak_sink.upgrade_in_event_loop(move |w| {
                        w.global::<LibraryScanState>().set_total_files(total as i32);
                    });
                }
                FileStarted { path } => {
                    if throttle_ok(&last) {
                        let base = basename(&path);
                        let _ = weak_sink.upgrade_in_event_loop(move |w| {
                            w.global::<LibraryScanState>().set_current_file(base.into());
                        });
                    }
                }
                FileDone { processed, total } => {
                    if throttle_ok(&last) {
                        let _ = weak_sink.upgrade_in_event_loop(move |w| {
                            let s = w.global::<LibraryScanState>();
                            s.set_processed_files(processed as i32);
                            s.set_total_files(total as i32);
                            s.set_progress(if total > 0 {
                                (processed as f32 / total as f32).min(1.0)
                            } else {
                                0.0
                            });
                        });
                    }
                }
                Cleanup => {
                    let _ = weak_sink.upgrade_in_event_loop(|w| {
                        w.global::<LibraryScanState>()
                            .set_current_file("Cleaning up missing files...".into());
                    });
                }
                Finished { status, errors } => {
                    let st = match status {
                        qbz_library::ScanStatus::Complete => 2,
                        qbz_library::ScanStatus::Cancelled => 3,
                        qbz_library::ScanStatus::Error => 4,
                        _ => 0,
                    };
                    let ec = errors.len() as i32;
                    let _ = weak_sink.upgrade_in_event_loop(move |w| {
                        let s = w.global::<LibraryScanState>();
                        s.set_scanning(false);
                        s.set_scan_status(st);
                        s.set_error_count(ec);
                        s.set_current_file("".into());
                        if st == 2 {
                            s.set_progress(1.0);
                        }
                    });
                    match st {
                        2 if ec > 0 => crate::toast::success_weak(
                            &weak_sink,
                            format!("Scan complete ({ec} file(s) skipped)"),
                        ),
                        2 => crate::toast::success_weak(&weak_sink, "Scan complete"),
                        3 => crate::toast::success_weak(&weak_sink, "Scan cancelled"),
                        _ => crate::toast::error_weak(&weak_sink, "Scan failed"),
                    }
                }
            }
        };

        let ids_ref = ids.as_deref();
        let _ = crate::library_db::with_db(|db| {
            qbz_library::scan_with_progress(db, ids_ref, &artwork_cache, &cancel, &sink)
        });

        // Post-scan: refresh the folder list (last_scan labels) + reset the
        // browse models so the tabs re-fetch the new index on next visit.
        let _ = weak.upgrade_in_event_loop(|w| {
            crate::local_library::reset_browse_models(&w);
        });
        load_folders(weak, h);
    });
}

/// Scan every enabled folder. Guards on an empty list.
pub fn scan_all(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let empty = folders_lock().is_empty();
    if empty {
        crate::toast::error_weak(&weak, "Add a folder before scanning");
        return;
    }
    run_scan(weak, handle, None);
}

/// Scan a single folder (from the settings modal). Closes the modal first.
pub fn scan_folder(weak: Weak<AppWindow>, handle: tokio::runtime::Handle, id: i64) {
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<LibFolderEditState>().set_open(false);
    });
    run_scan(weak, handle, Some(vec![id]));
}

/// Request cancellation of the running scan.
pub fn stop_scan() {
    SCAN_CANCEL.store(true, Ordering::SeqCst);
}
