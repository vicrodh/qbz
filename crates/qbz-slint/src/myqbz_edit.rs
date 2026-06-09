//! My QBZ — collection detail EDIT operations (Phase-2 Slice 7).
//!
//! Wires the hero overflow (⋯) menu + the Rename / Description / Delete-confirm
//! modals to the shared `qbz_mixtape::repo` setters, reached directly through
//! `crate::library_db::with_db` (ADR-005/006 — no Tauri command wrappers). Each
//! mutation mirrors its Tauri command (spec 40 §3.5/§3.6) and then RELOADS the
//! open detail view so the hero + state reflect the change (Tauri's
//! "-> reload"):
//!
//! - **Rename** (`v2_rename_mixtape_collection`): trim; empty -> no-op.
//! - **Description** (`v2_set_mixtape_description`): empty -> NULL (clear).
//! - **Play-mode toggle** (`v2_set_mixtape_play_mode`): in_order <-> album_shuffle.
//! - **Convert kind** (`v2_set_mixtape_kind`): mixtape <-> collection; the repo
//!   REJECTS any artist_collection conversion -> the "Cannot convert this kind"
//!   toast. Success -> "Converted".
//! - **Delete** (`v2_delete_mixtape_collection`, CASCADE): navigate BACK (which
//!   re-applies the previous grid entry, dropping the deleted row) on success;
//!   "Failed to delete" toast on error.
//!
//! All DB work runs synchronously inside `with_db` on a `spawn_blocking` worker
//! (no `&Connection` crosses an `.await`); the reload + toast hop back to the
//! event loop.

use qbz_models::mixtape::{CollectionKind, CollectionPlayMode};
use slint::ComponentHandle;

use crate::artwork::ImageCache;
use crate::{AppWindow, MyQbzEditState, NavState};

// ──────────────────────────── DB write helpers ────────────────────────

/// Run a repo mutation that returns `rusqlite::Result<()>` against the per-user
/// library.db. Returns `Ok(())` on success, `Err(message)` on any failure
/// (DB unavailable or the repo error). Synchronous (`with_db`).
fn with_repo<F>(f: F) -> Result<(), String>
where
    F: FnOnce(&rusqlite::Connection) -> rusqlite::Result<()> + Send,
{
    match crate::library_db::with_db(|db| Ok(db.with_connection(f))) {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("library database unavailable".to_string()),
    }
}

// ──────────────────────────── reload after mutation ───────────────────

/// Reload the open detail view for `id` (Tauri's "-> reload") so the hero +
/// toolbar reflect the mutation. Re-runs the detail navigator's
/// load/apply/artwork path; the inner `set_view` is harmless (already there).
fn reload(
    weak: &slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: &ImageCache,
    id: String,
) {
    let handle = handle.clone();
    let image_cache = image_cache.clone();
    let _ = weak.upgrade_in_event_loop(move |w| {
        crate::myqbz_detail::navigate(w.as_weak(), handle.clone(), image_cache.clone(), id);
    });
}

// ──────────────────────────── public entry points ─────────────────────

/// Rename modal submit: trim the draft; empty -> close without writing. Else
/// `repo::rename_collection` -> reload -> close. Caps at 80 chars (Tauri
/// `<input maxlength=80>`).
pub fn rename(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    id: String,
    raw_name: String,
) {
    let name: String = raw_name.trim().chars().take(80).collect();
    if id.is_empty() || name.is_empty() {
        close_modal(&weak);
        return;
    }
    set_busy(&weak, true);
    handle.clone().spawn(async move {
        let write_id = id.clone();
        let write_name = name.clone();
        let result = tokio::task::spawn_blocking(move || {
            with_repo(|conn| qbz_mixtape::repo::rename_collection(conn, &write_id, &write_name))
        })
        .await
        .unwrap_or_else(|e| Err(format!("rename task panicked: {e}")));

        finish(&weak, &handle, &image_cache, id, result, None, "Failed to rename");
    });
}

/// Description modal submit: trimmed empty -> NULL (clear). Else set it.
/// `repo::set_description` -> reload -> close.
pub fn set_description(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    id: String,
    raw_description: String,
) {
    if id.is_empty() {
        close_modal(&weak);
        return;
    }
    let trimmed = raw_description.trim().to_string();
    let desc: Option<String> = if trimmed.is_empty() { None } else { Some(trimmed) };
    set_busy(&weak, true);
    handle.clone().spawn(async move {
        let write_id = id.clone();
        let write_desc = desc.clone();
        let result = tokio::task::spawn_blocking(move || {
            with_repo(|conn| {
                qbz_mixtape::repo::set_description(conn, &write_id, write_desc.as_deref())
            })
        })
        .await
        .unwrap_or_else(|e| Err(format!("description task panicked: {e}")));

        finish(&weak, &handle, &image_cache, id, result, None, "Failed to save description");
    });
}

/// Hero overflow play-mode toggle: flip in_order <-> album_shuffle. Reads the
/// current mode from `MyQbzDetailState.play_mode`, persists the OTHER, reloads.
pub fn toggle_play_mode(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    id: String,
    current_mode: String,
) {
    if id.is_empty() {
        return;
    }
    let next = if current_mode == "in_order" {
        CollectionPlayMode::AlbumShuffle
    } else {
        CollectionPlayMode::InOrder
    };
    handle.clone().spawn(async move {
        let write_id = id.clone();
        let result = tokio::task::spawn_blocking(move || {
            with_repo(|conn| qbz_mixtape::repo::set_play_mode(conn, &write_id, next))
        })
        .await
        .unwrap_or_else(|e| Err(format!("play-mode task panicked: {e}")));

        finish(&weak, &handle, &image_cache, id, result, None, "Failed to change play mode");
    });
}

/// Hero overflow convert-kind: flip mixtape <-> collection. The repo rejects
/// any artist_collection conversion -> "Cannot convert this kind"; success ->
/// "Converted". Reads the current kind from `MyQbzDetailState.kind`.
pub fn convert_kind(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    id: String,
    current_kind: String,
) {
    if id.is_empty() {
        return;
    }
    // artist_collection is non-convertible (the menu item is hidden for it, but
    // guard here too).
    let next = match current_kind.as_str() {
        "mixtape" => CollectionKind::Collection,
        "collection" => CollectionKind::Mixtape,
        _ => {
            crate::toast::error_weak(&weak, "Cannot convert this kind");
            return;
        }
    };
    handle.clone().spawn(async move {
        let write_id = id.clone();
        let result = tokio::task::spawn_blocking(move || {
            with_repo(|conn| qbz_mixtape::repo::set_kind(conn, &write_id, next))
        })
        .await
        .unwrap_or_else(|e| Err(format!("convert-kind task panicked: {e}")));

        match result {
            Ok(()) => {
                crate::toast::success_weak(&weak, "Converted");
                reload(&weak, &handle, &image_cache, id);
            }
            Err(_) => {
                // The repo's only rejection here is the artist_collection guard.
                crate::toast::error_weak(&weak, "Cannot convert this kind");
            }
        }
    });
}

/// Delete-confirm: `repo::delete_collection` (CASCADE) -> navigate BACK (which
/// re-applies the previous grid entry, so the deleted row is gone) -> close.
/// "Failed to delete" toast on error.
pub fn delete(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    id: String,
) {
    if id.is_empty() {
        close_modal(&weak);
        return;
    }
    set_busy(&weak, true);
    handle.spawn(async move {
        let write_id = id.clone();
        let result = tokio::task::spawn_blocking(move || {
            with_repo(|conn| qbz_mixtape::repo::delete_collection(conn, &write_id))
        })
        .await
        .unwrap_or_else(|e| Err(format!("delete task panicked: {e}")));

        let _ = weak.upgrade_in_event_loop(move |w| {
            let es = w.global::<MyQbzEditState>();
            es.set_busy(false);
            match result {
                Ok(()) => {
                    es.set_open(false);
                    es.set_mode("".into());
                    // Clean up the deleted collection's persisted view-prefs key
                    // so it doesn't orphan in the store (spec 12 §18 / §11.3).
                    crate::myqbz_view_prefs::remove(&id);
                    // Navigate back: re-applies the previous grid entry, which
                    // re-lists collections from the DB (the deleted one is gone).
                    w.global::<NavState>().invoke_request_back();
                }
                Err(e) => {
                    log::warn!("[qbz-slint] myqbz_edit delete failed: {e}");
                    crate::toast::error(&w, "Failed to delete");
                }
            }
        });
    });
}

/// Bulk-remove the selected items from the collection (spec 12 §13.3). The
/// positions are removed **highest-first** so each `repo::remove_item`'s
/// position-compaction (spec 40 §3.10) never shifts a position we still have to
/// delete. After the batch: reload the detail (re-fetches the now-compacted
/// list), clear the selection, and toast "Removed {n}". A repo error per item is
/// logged and the batch continues; only a hard DB-unavailable surfaces the
/// "Failed to remove items" error.
pub fn remove_selected(
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: ImageCache,
    id: String,
    mut positions: Vec<i32>,
) {
    if id.is_empty() || positions.is_empty() {
        return;
    }
    // Highest position first (descending) so compaction is harmless.
    positions.sort_unstable_by(|a, b| b.cmp(a));
    let count = positions.len();
    handle.clone().spawn(async move {
        let write_id = id.clone();
        let result = tokio::task::spawn_blocking(move || {
            with_repo(|conn| {
                for pos in &positions {
                    if let Err(e) = qbz_mixtape::repo::remove_item(conn, &write_id, *pos) {
                        log::warn!(
                            "[qbz-slint] myqbz_edit remove_item({write_id}, {pos}) failed: {e}"
                        );
                    }
                }
                Ok(())
            })
        })
        .await
        .unwrap_or_else(|e| Err(format!("bulk-remove task panicked: {e}")));

        match result {
            Ok(()) => {
                let _ = weak.upgrade_in_event_loop(|w| {
                    crate::myqbz_detail::clear_selection(&w);
                });
                crate::toast::info_weak(&weak, format!("Removed {count}"));
                reload(&weak, &handle, &image_cache, id);
            }
            Err(e) => {
                log::warn!("[qbz-slint] myqbz_edit bulk-remove failed: {e}");
                crate::toast::error_weak(&weak, "Failed to remove items");
            }
        }
    });
}

// ──────────────────────────── modal-state helpers ─────────────────────

/// On a successful mutation: close the modal (if any), reload, and (when given)
/// toast a success message. On failure: clear busy + toast `err_msg`.
fn finish(
    weak: &slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: &ImageCache,
    id: String,
    result: Result<(), String>,
    success_toast: Option<&'static str>,
    err_msg: &'static str,
) {
    match result {
        Ok(()) => {
            if let Some(msg) = success_toast {
                crate::toast::success_weak(weak, msg);
            }
            close_modal(weak);
            reload(weak, handle, image_cache, id);
        }
        Err(e) => {
            log::warn!("[qbz-slint] myqbz_edit mutation failed: {e}");
            set_busy(weak, false);
            crate::toast::error_weak(weak, err_msg);
        }
    }
}

/// Close the edit modal (clears mode + busy). UI thread hop.
fn close_modal(weak: &slint::Weak<AppWindow>) {
    let _ = weak.upgrade_in_event_loop(|w| {
        let es = w.global::<MyQbzEditState>();
        es.set_open(false);
        es.set_mode("".into());
        es.set_busy(false);
    });
}

/// Set the modal busy flag (disables submit) from any thread.
fn set_busy(weak: &slint::Weak<AppWindow>, busy: bool) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        w.global::<MyQbzEditState>().set_busy(busy);
    });
}
