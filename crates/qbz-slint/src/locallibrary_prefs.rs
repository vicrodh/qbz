//! Persistence for the LocalLibrary toolbar choices (Tracks group mode) across
//! restarts. A small json under `<data-dir>/qbz/locallibrary_ui.json`, mirroring
//! `favorites_prefs.rs`. Search queries are transient, not persisted.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use slint::ComponentHandle;

use crate::{AppWindow, LocalLibraryState};

#[derive(Serialize, Deserialize)]
struct Prefs {
    #[serde(default = "d_off")]
    tracks_group: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            tracks_group: d_off(),
        }
    }
}

fn d_off() -> String {
    "off".to_string()
}

fn store_path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("qbz").join("locallibrary_ui.json"))
}

fn read() -> Prefs {
    let Some(path) = store_path() else {
        return Prefs::default();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Prefs::default(),
    }
}

/// Apply persisted toolbar choices to LocalLibraryState. UI thread.
pub fn load(window: &AppWindow) {
    let p = read();
    window
        .global::<LocalLibraryState>()
        .set_tracks_group_mode(p.tracks_group.into());
}

/// Persist the current toolbar choices read from LocalLibraryState.
pub fn save(window: &AppWindow) {
    let Some(path) = store_path() else {
        return;
    };
    let p = Prefs {
        tracks_group: window
            .global::<LocalLibraryState>()
            .get_tracks_group_mode()
            .into(),
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec_pretty(&p) {
        let _ = std::fs::write(&path, json);
    }
}
