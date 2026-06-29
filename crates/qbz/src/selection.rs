//! Shared keyboard-driven multi-select core (Excel-style Shift+Click range).
//!
//! Port of the Tauri selection model (`src/lib/utils/multiSelect.ts`):
//! - Shift+Click extends an **additive** range from an anchor (the last
//!   explicitly-clicked row) to the clicked row — it only ever ADDS, never
//!   deselects (`applyShiftRange`).
//! - Plain click / Ctrl+Click stay a single per-row toggle (Tauri reads only
//!   `shiftKey` at click time; Ctrl/Cmd never branch there).
//!
//! This module owns ONE per-surface anchor (a thread-local, UI thread only) and
//! the generic span-fill. The modifier state itself comes from
//! [`crate::keybindings::mods`] (fed by winit `ModifiersChanged`); the caller
//! reads `mods().2` (shift) and routes here. The helper is generic over the row
//! type via a setter closure so it works for both `TrackItem` and (later) the
//! album row struct — they are distinct Slint-generated types with no shared
//! trait.

use std::cell::RefCell;

use slint::{Model, VecModel};

// ---------------------------------------------------------------------------
// Surface ids — a stable, arbitrary discriminator so an anchor set on one
// surface never leaks into a Shift-range on another. These are NOT 1:1 with
// `ContentView`: surfaces like LocalLibrary Tracks / Offline / a future albums
// grid select via their own controllers, not the central toggle arm.
// ---------------------------------------------------------------------------
pub const SURFACE_ALBUM: u16 = 1;
pub const SURFACE_ARTIST: u16 = 2;
pub const SURFACE_PLAYLIST: u16 = 3;
pub const SURFACE_FAVORITES: u16 = 4;
pub const SURFACE_LABEL: u16 = 5;
pub const SURFACE_LOCAL_TRACKS: u16 = 6;
pub const SURFACE_OFFLINE: u16 = 7;

#[derive(Clone)]
struct Anchor {
    surface: u16,
    index: usize,
    /// The clicked row's id, kept so a Shift-range can re-resolve the anchor by
    /// id if the model was re-sorted/filtered under it (the index alone would
    /// go stale). Matches Tauri nulling `lastSelectedIndex` on model rebuild,
    /// but resilient without hooking every rebuild site.
    id: String,
}

thread_local! {
    static ANCHOR: RefCell<Option<Anchor>> = const { RefCell::new(None) };
}

/// Remember the clicked row as the new anchor for `surface`.
pub fn set_anchor(surface: u16, index: usize, id: &str) {
    ANCHOR.with(|a| {
        *a.borrow_mut() = Some(Anchor {
            surface,
            index,
            id: id.to_string(),
        });
    });
}

/// Drop the anchor (call on enter/leave select-mode and on model rebuild).
pub fn clear_anchor() {
    ANCHOR.with(|a| *a.borrow_mut() = None);
}

/// The stored anchor `(index, id)` for `surface`, if the current anchor belongs
/// to that surface. The caller verifies/re-resolves the index against the live
/// model before ranging.
pub fn anchor_for(surface: u16) -> Option<(usize, String)> {
    ANCHOR.with(|a| {
        a.borrow()
            .as_ref()
            .filter(|an| an.surface == surface)
            .map(|an| (an.index, an.id.clone()))
    })
}

/// Additive Shift-range over a `VecModel`: set selected = true for every row in
/// the inclusive index span `[min(anchor,clicked), max(anchor,clicked)]`. Never
/// deselects (1:1 with `applyShiftRange`). Generic over the row type via the
/// `set_selected` setter closure (e.g. `|t, v| t.selected = v`).
pub fn apply_shift_range<T: Clone + 'static>(
    model: &VecModel<T>,
    anchor: usize,
    clicked: usize,
    set_selected: impl Fn(&mut T, bool),
) {
    let lo = anchor.min(clicked);
    let hi = anchor.max(clicked);
    let n = model.row_count();
    for i in lo..=hi {
        if i < n {
            if let Some(mut item) = model.row_data(i) {
                set_selected(&mut item, true);
                model.set_row_data(i, item);
            }
        }
    }
}

/// Select-all-ONLY over a `VecModel`: set selected = true for every row (never
/// toggles to clear — 1:1 with Tauri's `isSelectAllShortcut`, which only ever
/// selects all; the toggling all-or-none lives on the bulk bar's button). The
/// caller recounts afterwards.
pub fn select_all<T: Clone + 'static>(model: &VecModel<T>, set_selected: impl Fn(&mut T, bool)) {
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            set_selected(&mut item, true);
            model.set_row_data(i, item);
        }
    }
}

/// Resolve the anchor index for `surface` against the live `model`, by id. Use
/// the stored index when the id at that index still matches; otherwise scan the
/// model for the stored id (it was re-sorted/filtered); `None` if it is gone.
/// `id_at` extracts a row's id (e.g. `|t| t.id.to_string()`).
pub fn resolve_anchor<T: Clone + 'static>(
    surface: u16,
    model: &VecModel<T>,
    id_at: impl Fn(&T) -> String,
) -> Option<usize> {
    let (idx, id) = anchor_for(surface)?;
    if let Some(item) = model.row_data(idx) {
        if id_at(&item) == id {
            return Some(idx);
        }
    }
    (0..model.row_count()).find(|&i| model.row_data(i).map(|t| id_at(&t) == id).unwrap_or(false))
}
