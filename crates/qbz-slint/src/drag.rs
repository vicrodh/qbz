//! Track drag-and-drop state — the ids currently being dragged onto a
//! sidebar playlist. The UI (DragState global) drives the ghost +
//! drop highlight; the actual track ids live here (the global only
//! carries a count + ghost text).

use std::sync::{LazyLock, Mutex};

static DRAGGED: LazyLock<Mutex<Vec<u64>>> = LazyLock::new(|| Mutex::new(Vec::new()));

pub fn set_dragged(ids: Vec<u64>) {
    if let Ok(mut d) = DRAGGED.lock() {
        *d = ids;
    }
}

pub fn dragged() -> Vec<u64> {
    DRAGGED.lock().map(|d| d.clone()).unwrap_or_default()
}

pub fn clear() {
    if let Ok(mut d) = DRAGGED.lock() {
        d.clear();
    }
}
