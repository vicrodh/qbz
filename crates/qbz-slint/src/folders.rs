//! Playlist folders — local-only organization stored in library.db
//! (shared with the Tauri app). Folders are flat (no nesting); a
//! playlist belongs to at most one folder via
//! `playlist_settings.folder_id`. All ops are blocking (they open the
//! DB), so async callers wrap them in `tokio::task::spawn_blocking`.

use std::collections::HashMap;

use crate::library_db;

#[derive(Clone)]
pub struct FolderInfo {
    pub id: String,
    pub name: String,
}

/// All folders, ordered by their stored position. The sidebar now uses
/// `load_folders_full` (it needs the hidden flag to exclude hidden
/// folders); kept as a lightweight id+name helper for other callers.
#[allow(dead_code)]
pub fn load_folders() -> Vec<FolderInfo> {
    library_db::with_db(|db| db.get_all_playlist_folders())
        .unwrap_or_default()
        .into_iter()
        .map(|f| FolderInfo {
            id: f.id,
            name: f.name,
        })
        .collect()
}

/// playlist id -> folder id, for grouping playlists under folders.
pub fn playlist_folder_map() -> HashMap<u64, String> {
    library_db::with_db(|db| db.get_all_playlist_settings())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|s| s.folder_id.map(|fid| (s.qobuz_playlist_id, fid)))
        .collect()
}

/// playlist id -> custom-sort position, for the sidebar "Custom" sort.
pub fn playlist_positions() -> HashMap<u64, i32> {
    library_db::with_db(|db| db.get_all_playlist_settings())
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.qobuz_playlist_id, s.position))
        .collect()
}

pub fn create_folder(name: &str) -> Option<FolderInfo> {
    library_db::with_db(|db| db.create_playlist_folder(name, None, None, None)).map(|f| {
        FolderInfo {
            id: f.id,
            name: f.name,
        }
    })
}

pub fn delete_folder(id: &str) {
    library_db::with_db(|db| db.delete_playlist_folder(id));
}

/// Move a playlist into `folder_id`, or to root when None.
pub fn move_playlist(playlist_id: u64, folder_id: Option<&str>) {
    library_db::with_db(|db| db.move_playlist_to_folder(playlist_id, folder_id));
}

// === Playlist Manager — richer folder + settings/stats access ==========
// The sidebar only needs id+name; the Playlist Manager needs the full
// folder record (icon preset/color/custom-image/hidden) plus the
// per-playlist settings/stats and local track counts.

/// Full folder record for the Playlist Manager (icon + color + hidden).
#[derive(Clone, Default)]
pub struct FolderFull {
    pub id: String,
    pub name: String,
    pub icon_type: String,
    pub icon_preset: String,
    pub icon_color: String,
    pub custom_image_path: Option<String>,
    pub is_hidden: bool,
}

/// Per-playlist local settings the manager merges onto the remote list.
#[derive(Clone, Default)]
pub struct PlaylistSettingsLite {
    pub hidden: bool,
    pub is_favorite: bool,
    pub position: i32,
    pub folder_id: Option<String>,
}

/// All folders with their full icon/color records, ordered by position.
pub fn load_folders_full() -> Vec<FolderFull> {
    library_db::with_db(|db| db.get_all_playlist_folders())
        .unwrap_or_default()
        .into_iter()
        .map(|f| FolderFull {
            id: f.id,
            name: f.name,
            icon_type: f.icon_type,
            icon_preset: f.icon_preset,
            icon_color: f.icon_color,
            custom_image_path: f.custom_image_path,
            is_hidden: f.is_hidden,
        })
        .collect()
}

/// playlist id -> its local settings (hidden/favorite/position/folder).
pub fn playlist_settings_map() -> HashMap<u64, PlaylistSettingsLite> {
    library_db::with_db(|db| db.get_all_playlist_settings())
        .unwrap_or_default()
        .into_iter()
        .map(|s| {
            (
                s.qobuz_playlist_id,
                PlaylistSettingsLite {
                    hidden: s.hidden,
                    is_favorite: s.is_favorite,
                    position: s.position,
                    folder_id: s.folder_id,
                },
            )
        })
        .collect()
}

/// playlist id -> play count (for the "Play Count" sort + the list badge).
pub fn playlist_play_counts() -> HashMap<u64, u32> {
    library_db::with_db(|db| db.get_all_playlist_stats())
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.qobuz_playlist_id, s.play_count))
        .collect()
}

/// playlist id -> local (non-Qobuz) track count.
pub fn playlist_local_counts() -> HashMap<u64, u32> {
    library_db::with_db(|db| db.get_all_playlist_local_track_counts()).unwrap_or_default()
}

/// Create a folder with icon preset + color (manager create path).
pub fn create_folder_full(
    name: &str,
    icon_preset: &str,
    icon_color: &str,
) -> Option<FolderFull> {
    let preset = Some(icon_preset);
    let color = if icon_color.is_empty() {
        None
    } else {
        Some(icon_color)
    };
    library_db::with_db(|db| db.create_playlist_folder(name, Some("preset"), preset, color)).map(
        |f| FolderFull {
            id: f.id,
            name: f.name,
            icon_type: f.icon_type,
            icon_preset: f.icon_preset,
            icon_color: f.icon_color,
            custom_image_path: f.custom_image_path,
            is_hidden: f.is_hidden,
        },
    )
}

/// Update a folder (name, icon preset/type, color, custom image, hidden).
/// `custom_image_path` is `Some(Some(p))` to set, `Some(None)` to clear,
/// `None` to leave unchanged (mirrors the DB signature).
#[allow(clippy::too_many_arguments)]
pub fn update_folder_full(
    id: &str,
    name: &str,
    icon_type: &str,
    icon_preset: &str,
    icon_color: &str,
    custom_image_path: Option<Option<&str>>,
    is_hidden: bool,
) {
    let color = if icon_color.is_empty() {
        None
    } else {
        Some(icon_color)
    };
    library_db::with_db(|db| {
        db.update_playlist_folder(
            id,
            Some(name),
            Some(icon_type),
            Some(icon_preset),
            color,
            custom_image_path,
            Some(is_hidden),
        )
    });
}

/// Set a playlist's favorite flag.
pub fn set_favorite(playlist_id: u64, favorite: bool) {
    library_db::with_db(|db| db.set_playlist_favorite(playlist_id, favorite));
}

/// Set a playlist's hidden flag.
pub fn set_hidden(playlist_id: u64, hidden: bool) {
    library_db::with_db(|db| db.set_playlist_hidden(playlist_id, hidden));
}

/// Set a folder's hidden flag (leaves all other fields unchanged).
pub fn set_folder_hidden(id: &str, hidden: bool) {
    library_db::with_db(|db| {
        db.update_playlist_folder(id, None, None, None, None, None, Some(hidden))
    });
}

/// Persist a custom playlist order (custom-sort positions).
pub fn reorder_playlists(playlist_ids: &[u64]) {
    library_db::with_db(|db| db.reorder_playlists(playlist_ids));
}
