//! Share-link helpers — Qobuz track URL + Song.link (Odesli) resolution
//! + clipboard copy. Used by the track context menu's Share actions.

/// Canonical Qobuz track URL — the `open.qobuz.com` share form (#514).
pub fn qobuz_track_url(track_id: &str) -> String {
    format!("https://open.qobuz.com/track/{track_id}")
}

/// Qobuz web-player playlist URL (matches Tauri's share-playlist link).
pub fn qobuz_playlist_url(playlist_id: &str) -> String {
    format!("https://play.qobuz.com/playlist/{playlist_id}")
}

/// Qobuz album URL — the `open.qobuz.com` form (#514; Tauri's
/// `shareAlbumQobuzLink` used `https://play.qobuz.com/album/{id}`). Also
/// the source URL fed to Song.link for the album-level "Album.link".
pub fn qobuz_album_url(album_id: &str) -> String {
    format!("https://open.qobuz.com/album/{album_id}")
}

/// Qobuz web-player artist URL (header Share action).
pub fn qobuz_artist_url(artist_id: &str) -> String {
    format!("https://play.qobuz.com/artist/{artist_id}")
}

/// Qobuz web-player label URL (label-page header Share action). There is no
/// Song.link/Album.link equivalent for labels — Qobuz-link only.
pub fn qobuz_label_url(label_id: &str) -> String {
    format!("https://play.qobuz.com/label/{label_id}")
}

/// Long-lived clipboard instance. arboard ties the offer's lifetime to the
/// LAST live `Clipboard` object: dropping it destroys the X11 selection
/// window (contents survive only when a clipboard MANAGER accepts the
/// handoff — KDE ships one, stock GNOME/XFCE/Cinnamon do not) and ends the
/// Wayland offer with the same rule. The old create-per-copy pattern
/// therefore worked on KDE and silently lost the text everywhere else
/// (HiFi-wizard copy report, #514). One instance kept alive for the whole
/// process serves the offer like any normal app.
static CLIPBOARD: std::sync::OnceLock<std::sync::Mutex<Option<arboard::Clipboard>>> =
    std::sync::OnceLock::new();

/// Copy `text` to the system clipboard. Runs on a blocking thread —
/// clipboard backends (X11/Wayland) can block.
pub fn copy_to_clipboard(text: String) {
    tokio::task::spawn_blocking(move || {
        let cell = CLIPBOARD.get_or_init(|| std::sync::Mutex::new(None));
        let mut guard = match cell.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if guard.is_none() {
            match arboard::Clipboard::new() {
                Ok(c) => *guard = Some(c),
                Err(e) => {
                    log::warn!("[qbz-slint] clipboard unavailable: {e}");
                    return;
                }
            }
        }
        if let Some(clipboard) = guard.as_mut() {
            if let Err(e) = clipboard.set_text(text) {
                log::warn!("[qbz-slint] clipboard set failed: {e}");
                // Drop the instance so the next copy reconnects — the
                // display connection may have gone away.
                *guard = None;
            }
        }
    });
}

/// Shared HTTP client settings for the share resolvers (Tauri parity:
/// 10 s request / 5 s connect timeouts).
fn share_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default()
}

/// Resolve an ISRC/UPC code to a Deezer catalog id. `path` is
/// `track/isrc:{code}` or `album/upc:{code}`. Deezer answers misses with
/// HTTP 200 + an `{"error": ...}` body, so both shapes are checked.
async fn deezer_lookup(path: &str) -> Option<u64> {
    let url = format!("https://api.deezer.com/2.0/{path}");
    let resp = match share_http_client().get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[qbz-slint] deezer lookup {path}: request failed: {e}");
            return None;
        }
    };
    if !resp.status().is_success() {
        log::warn!("[qbz-slint] deezer lookup {path}: HTTP {}", resp.status());
        return None;
    }
    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[qbz-slint] deezer lookup {path}: bad JSON: {e}");
            return None;
        }
    };
    if let Some(err) = body.get("error") {
        log::info!("[qbz-slint] deezer lookup {path}: no match ({err})");
        return None;
    }
    body.get("id").and_then(|v| v.as_u64())
}

/// Song.link page URL for a track — ISRC-first via Deezer (#514).
///
/// The Odesli API does not know Qobuz as an input platform: every
/// `qobuz.com` URL form answers 400 `could_not_resolve_entity` (verified
/// 2026-07-16), so the URL-only path the 2.0 port shipped never worked.
/// Tauri's working path resolved the track's ISRC to a Deezer id (one GET)
/// and built `https://song.link/d/{id}` directly — restored here. The
/// Odesli URL fallback is kept as a last resort for the no-ISRC case.
pub async fn songlink_for_track(track_id: &str, isrc: Option<&str>) -> Option<String> {
    if let Some(code) = isrc.map(str::trim).filter(|c| !c.is_empty()) {
        if let Some(deezer_id) = deezer_lookup(&format!("track/isrc:{code}")).await {
            log::info!("[qbz-slint] song.link via ISRC {code} -> deezer track {deezer_id}");
            return Some(format!("https://song.link/d/{deezer_id}"));
        }
    } else {
        log::info!("[qbz-slint] track {track_id} has no ISRC; trying Odesli URL fallback");
    }
    songlink_url(&qobuz_track_url(track_id)).await
}

/// Album.link page URL for an album — UPC-first via Deezer (#514).
///
/// Same story as [`songlink_for_track`]: Odesli cannot resolve Qobuz URLs,
/// so the UPC -> Deezer -> `https://album.link/d/{id}` path (Tauri's
/// `get_by_upc`) is the one that works. Qobuz UPCs often carry a leading
/// zero (13-digit EAN) while Deezer stores the 12-digit form and does NOT
/// match zero-padded input (verified), so a trimmed retry is attempted.
pub async fn albumlink_for_album(album_id: &str, upc: Option<&str>) -> Option<String> {
    if let Some(code) = upc.map(str::trim).filter(|c| !c.is_empty()) {
        if let Some(deezer_id) = deezer_lookup(&format!("album/upc:{code}")).await {
            log::info!("[qbz-slint] album.link via UPC {code} -> deezer album {deezer_id}");
            return Some(format!("https://album.link/d/{deezer_id}"));
        }
        let trimmed = code.trim_start_matches('0');
        if trimmed != code && !trimmed.is_empty() {
            if let Some(deezer_id) = deezer_lookup(&format!("album/upc:{trimmed}")).await {
                log::info!(
                    "[qbz-slint] album.link via UPC {trimmed} (leading zeros trimmed) -> deezer album {deezer_id}"
                );
                return Some(format!("https://album.link/d/{deezer_id}"));
            }
        }
    } else {
        log::info!("[qbz-slint] album {album_id} has no UPC; trying Odesli URL fallback");
    }
    songlink_url(&qobuz_album_url(album_id)).await
}

/// Resolve a source URL to its universal Song.link (Odesli) page URL.
/// One GET to the Odesli API; returns the `pageUrl` field. NOTE: Odesli
/// cannot resolve Qobuz URLs (400 `could_not_resolve_entity`) — for Qobuz
/// content use the ISRC/UPC resolvers above; this remains as their last
/// resort and for any non-Qobuz source URL.
pub async fn songlink_url(source_url: &str) -> Option<String> {
    let resp = share_http_client()
        .get("https://api.song.link/v1-alpha.1/links")
        .query(&[("url", source_url), ("userCountry", "US")])
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(200).collect();
        log::warn!("[qbz-slint] song.link status {status} for {source_url}: {snippet}");
        return None;
    }
    let value: serde_json::Value = resp.json().await.ok()?;
    value
        .get("pageUrl")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string())
}
