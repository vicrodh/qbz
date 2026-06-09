//! Per-user persistence for the "My QBZ" navigation branding (custom label
//! + custom icon path).
//!
//! Mirrors the Tauri `myQbzNavStore` contract (spec 20 §0.1), re-homed so the
//! ENTRY POINT is Settings > Appearance (DQ3) rather than a sidebar context
//! menu. The persisted shape is `{ label, icon_path }`:
//!
//!  - `label`     : the custom label. A trimmed-empty value is coerced to the
//!                  default `"My QBZ"` and that default string is what gets
//!                  persisted (matching `setMyQbzLabel`).
//!  - `icon_path` : an absolute filesystem path to a user-chosen image, or the
//!                  empty string for "default" (the branded `my-qbz.svg`). The
//!                  reset action stores the empty string — i.e. removes the
//!                  custom icon — rather than persisting a default path.
//!
//! Storage is per-user JSON, scoped the same way as the per-user Plex / tray
//! DBs so different Qobuz accounts keep independent branding:
//!
//!   <data_dir>/qbz/users/<user_id>/myqbz_branding.json
//!
//! The store is intentionally minimal: read-modify-write the whole tiny file
//! on every set. `init_for_user` binds it on shell entry; the Slint-facing
//! `seed` / `apply_*` helpers bridge it to `MyQbzBrandingState`.

use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use slint::ComponentHandle;

use crate::{AppWindow, MyQbzBrandingState};

/// The default "My QBZ" label (spec 20 §0.1 `DEFAULT_LABEL`).
pub const DEFAULT_LABEL: &str = "My QBZ";

/// The active user id, set by `init_for_user`. `None` before login (the
/// store degrades to defaults — there is no pre-login branding surface).
static USER_ID: LazyLock<Mutex<Option<u64>>> = LazyLock::new(|| Mutex::new(None));

/// Persisted branding. Missing fields default sanely so an older file still
/// deserializes.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Branding {
    #[serde(default = "default_label")]
    label: String,
    /// Absolute path to a custom icon, or empty for the default glyph.
    #[serde(default)]
    icon_path: String,
}

fn default_label() -> String {
    DEFAULT_LABEL.to_string()
}

impl Default for Branding {
    fn default() -> Self {
        Self {
            label: default_label(),
            icon_path: String::new(),
        }
    }
}

/// `<data_dir>/qbz/users/<user_id>/myqbz_branding.json` for the active user.
/// `None` before login or when the data dir is unavailable.
fn store_path() -> Option<PathBuf> {
    let user_id = (*USER_ID.lock().ok()?)?;
    Some(
        dirs::data_dir()?
            .join("qbz")
            .join("users")
            .join(user_id.to_string())
            .join("myqbz_branding.json"),
    )
}

/// Load the active user's branding. A missing / unreadable / unparseable file
/// degrades to defaults.
fn read() -> Branding {
    let Some(path) = store_path() else {
        return Branding::default();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Branding::default(),
    }
}

/// Persist the branding (best-effort — failures are logged).
fn write(b: &Branding) {
    let Some(path) = store_path() else {
        log::warn!("[qbz-slint] myqbz branding: no active user, not saving");
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::error!("[qbz-slint] myqbz branding: create dir failed: {e}");
            return;
        }
    }
    match serde_json::to_vec_pretty(b) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("[qbz-slint] myqbz branding: write failed: {e}");
            }
        }
        Err(e) => log::error!("[qbz-slint] myqbz branding: serialize failed: {e}"),
    }
}

/// Bind the store to `user_id` on shell entry. Subsequent reads/writes target
/// that user's JSON file.
pub fn init_for_user(user_id: u64) {
    if let Ok(mut guard) = USER_ID.lock() {
        *guard = Some(user_id);
    }
}

/// Coerce a raw label input to the persisted value: trimmed-empty → the
/// default "My QBZ" (and the default string is what's stored).
fn coerce_label(label: &str) -> String {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        DEFAULT_LABEL.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Persist a new label (empty/whitespace coerces to the default) and return
/// the coerced value the sidebar should display.
pub fn set_label(label: &str) -> String {
    let mut b = read();
    b.label = coerce_label(label);
    write(&b);
    b.label
}

/// Persist a custom icon path. An empty / whitespace path clears the custom
/// icon (reset to default), mirroring `setMyQbzIconPath(null)`.
pub fn set_icon_path(path: &str) {
    let mut b = read();
    b.icon_path = path.trim().to_string();
    write(&b);
}

/// Reset the icon to the default branded glyph (clears the persisted path).
pub fn reset_icon() {
    set_icon_path("");
}

/// Resolve a (label, custom_icon) pair for the UI from the persisted
/// branding. `custom_icon` is `Some` only when a custom path is set AND the
/// file loads; a missing path or a stale / deleted file yields `None` (the
/// markup then falls back to the default branded glyph). A load failure does
/// NOT mutate the store — the user can re-pick, and the path is preserved in
/// case the file returns.
fn resolve() -> (String, Option<slint::Image>) {
    let b = read();
    if b.icon_path.is_empty() {
        return (b.label, None);
    }
    match slint::Image::load_from_path(std::path::Path::new(&b.icon_path)) {
        Ok(img) => (b.label, Some(img)),
        Err(e) => {
            log::warn!(
                "[qbz-slint] myqbz branding: custom icon '{}' failed to load, using default: {e}",
                b.icon_path
            );
            (b.label, None)
        }
    }
}

/// Push the persisted branding onto `MyQbzBrandingState`. Runs on the UI
/// thread (it touches the Slint global + decodes an image). Call on shell
/// entry and after every set/reset so the sidebar row reflects the change.
///
/// The default glyph stays a compile-time `@image-url` in the markup; Rust
/// only supplies the custom image (and the flag that selects it).
pub fn seed(window: &AppWindow) {
    let (label, custom_icon) = resolve();
    let st = window.global::<MyQbzBrandingState>();
    st.set_label(label.into());
    match custom_icon {
        Some(img) => {
            st.set_custom_icon(img);
            st.set_has_custom_icon(true);
        }
        None => {
            // Clear any stale custom image and fall back to the default glyph.
            st.set_custom_icon(slint::Image::default());
            st.set_has_custom_icon(false);
        }
    }
}

/// Re-seed the branding state on the UI thread via a weak handle. Used by the
/// async icon picker once it has persisted a new path.
pub fn reseed_weak(weak: &slint::Weak<AppWindow>) {
    let _ = weak.upgrade_in_event_loop(|w| seed(&w));
}

/// Open the native image picker; on pick, persist the chosen path and re-seed
/// the branding state (sidebar row + Settings preview reflect it). No-op on
/// cancel. The filter matches the Tauri modal's set (`svg, png, jpg, jpeg,
/// webp`).
pub fn pick_icon(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    handle.spawn(async move {
        let Some(file) = rfd::AsyncFileDialog::new()
            .set_title("Choose a My QBZ icon")
            .add_filter("Image", &["svg", "png", "jpg", "jpeg", "webp"])
            .pick_file()
            .await
        else {
            return; // cancelled — leave branding unchanged.
        };
        let path = file.path().to_string_lossy().to_string();
        set_icon_path(&path);
        reseed_weak(&weak);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerce_blank_label_yields_default() {
        assert_eq!(coerce_label(""), "My QBZ");
        assert_eq!(coerce_label("   "), "My QBZ");
        assert_eq!(coerce_label("  Tapes  "), "Tapes");
        assert_eq!(coerce_label("Tapes"), "Tapes");
    }

    #[test]
    fn branding_defaults() {
        let b = Branding::default();
        assert_eq!(b.label, "My QBZ");
        assert!(b.icon_path.is_empty());
    }

    #[test]
    fn legacy_json_without_fields_deserializes() {
        let b: Branding = serde_json::from_str("{}").expect("empty object deserializes");
        assert_eq!(b.label, "My QBZ");
        assert!(b.icon_path.is_empty());
    }

    #[test]
    fn missing_icon_path_field_keeps_label() {
        let b: Branding =
            serde_json::from_str(r#"{"label":"Tapes"}"#).expect("partial object deserializes");
        assert_eq!(b.label, "Tapes");
        assert!(b.icon_path.is_empty());
    }
}
