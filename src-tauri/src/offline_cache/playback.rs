//! Offline-cache → audio-bytes bridge for playback.
//!
//! When a track is played back from the offline cache (either via the
//! main playback path or via the Local Library path when the track is a
//! Qobuz-cached offline entry), this module converts the stored row
//! into a `Vec<u8>` ready for `player.play_data`.
//!
//! For `cache_format = 2` (v2 CMAF bundle) this means:
//! 1. Read init.mp4 + segments.bin + manifest.json from disk
//! 2. Unwrap the content_key via the secret vault
//! 3. Decrypt the encrypted frames and prepend the FLAC header
//!
//! For `cache_format = 1` (legacy plain FLAC) the caller should just
//! `std::fs::read(file_path)` directly — this module doesn't handle v1
//! since v1 needs no extra work.

use std::path::Path;

use tauri::Emitter;

use super::cmaf_store::{self, BundleLayout};
use super::db::CmafBundleRow;
use super::secret_vault;

/// Run `load_cmaf_bundle` on the blocking pool and emit
/// `offline:unlock_start` / `offline:unlock_end` around it so the
/// frontend can show an "unlocking" animation on the track row.
///
/// `display_track_id` is what the frontend knows this track as — for
/// Qobuz flow it's the Qobuz track id, for Local Library it's the
/// library row id. The events carry THIS id so whatever UI is looking
/// at the track can key off it.
///
/// `cmaf_track_id` is the key used inside load_cmaf_bundle for logs
/// (always the Qobuz track id, since that's what the bundle is
/// identified by on disk + in the offline cache DB).
pub async fn load_cmaf_bundle_with_ui_events<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    display_track_id: u64,
    cmaf_track_id: u64,
    row: CmafBundleRow,
    cache_path: String,
) -> Option<Vec<u8>> {
    let _ = app.emit(
        "offline:unlock_start",
        serde_json::json!({ "trackId": display_track_id }),
    );
    let result = tokio::task::spawn_blocking(move || {
        load_cmaf_bundle(cmaf_track_id, &row, Path::new(&cache_path))
    })
    .await
    .ok()
    .flatten();
    let _ = app.emit(
        "offline:unlock_end",
        serde_json::json!({
            "trackId": display_track_id,
            "success": result.is_some(),
        }),
    );
    result
}

/// Decrypt a v2 CMAF bundle row into plain FLAC bytes ready for
/// `player.play_data`. Returns `None` on any failure (missing init,
/// wrong-size unwrapped key, corrupt manifest, decrypt error). The
/// caller should treat `None` as a cache miss — continue to the next
/// tier or the network.
///
/// `offline_root_path` is only used to locate the secret vault's
/// install UUID file; it must match the path used at download time.
/// Passing `OfflineCacheState::get_cache_path()` is correct.
pub fn load_cmaf_bundle(
    track_id: u64,
    row: &CmafBundleRow,
    offline_root_path: &Path,
) -> Option<Vec<u8>> {
    if row.cache_format != 2 {
        return None;
    }

    let init_path = row.init_path.as_ref().or_else(|| {
        log::warn!(
            "[OfflineCache/Play] Track {} cache_format=2 but init_path is null",
            track_id
        );
        None
    })?;
    let content_key_wrapped = row.content_key_wrapped.as_ref().or_else(|| {
        log::warn!(
            "[OfflineCache/Play] Track {} cache_format=2 but content_key_wrapped is null",
            track_id
        );
        None
    })?;

    let segments_path = std::path::PathBuf::from(&row.segments_path);
    let track_dir = segments_path.parent()?.to_path_buf();
    let layout = BundleLayout {
        track_dir,
        init_path: std::path::PathBuf::from(init_path),
        segments_path: segments_path.clone(),
        manifest_path: segments_path.with_file_name("manifest.json"),
    };

    let loaded = match cmaf_store::read_bundle(&layout) {
        Ok(lb) => lb,
        Err(e) => {
            log::warn!(
                "[OfflineCache/Play] Track {} failed to read CMAF bundle: {}",
                track_id,
                e
            );
            return None;
        }
    };

    let vault = match secret_vault::get_or_init(offline_root_path) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "[OfflineCache/Play] Track {} SecretBox init failed: {}",
                track_id,
                e
            );
            return None;
        }
    };
    let unwrapped = match vault.unwrap(content_key_wrapped) {
        Ok(k) => k,
        Err(e) => {
            log::warn!(
                "[OfflineCache/Play] Track {} content_key unwrap failed: {}",
                track_id,
                e
            );
            return None;
        }
    };
    if unwrapped.len() != 16 {
        log::warn!(
            "[OfflineCache/Play] Track {} unwrapped content_key wrong size ({} bytes)",
            track_id,
            unwrapped.len()
        );
        return None;
    }
    let mut content_key = [0u8; 16];
    content_key.copy_from_slice(&unwrapped);

    match loaded.decrypt_to_flac(&content_key) {
        Ok(flac_bytes) => {
            log::info!(
                "[OfflineCache/Play] Track {} unwrapped + decrypted ({:.2} MB FLAC)",
                track_id,
                flac_bytes.len() as f64 / (1024.0 * 1024.0)
            );
            Some(flac_bytes)
        }
        Err(e) => {
            log::warn!(
                "[OfflineCache/Play] Track {} CMAF decrypt failed: {}",
                track_id,
                e
            );
            None
        }
    }
}
