//! Desktop "now playing" notifications for track changes (frontend-agnostic, ADR-006).
//!
//! 1:1 port of the Tauri notification path (`src-tauri/src/commands_v2/
//! legacy_compat.rs::v2_show_track_notification` + the artwork helpers in
//! `helpers.rs`), lifted out of the Tauri command layer so any frontend
//! (Slint / TUI) can fire it from native Rust instead of a webview `invoke`.
//!
//!   - **Linux** → XDG notification portal via `ashpd` (goes over D-Bus). The
//!     album art is passed as `Icon::Bytes(png)` — the portal rejects huge
//!     payloads, so the cover is center-cropped to a square, downscaled to
//!     <=512px, and re-encoded PNG (<=4 MiB).
//!   - **macOS** → `notify_rust` with `image_path` (it needs a file on disk, so
//!     the cover is cached but NOT resized).
//!   - **Windows** → not implemented (parity with Tauri).
//!
//! The whole thing is fire-and-forget: failures are logged, never surfaced, so
//! a missing portal or a slow CDN never blocks playback. The HTTP download +
//! image work run on `spawn_blocking` (a tokio runtime must be present — it is,
//! the app drives one).

use std::path::PathBuf;

/// Everything needed to render a track-change notification. The crate formats
/// the body + quality line itself so the output matches the Tauri notification
/// exactly, regardless of frontend.
#[derive(Debug, Clone, Default)]
pub struct NotificationMeta {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Bit depth (e.g. 16, 24). Drives the quality line.
    pub bit_depth: Option<u32>,
    /// Sample rate in kHz (e.g. 44.1, 96.0). Drives the quality line.
    pub sample_rate: Option<f64>,
    /// Album-art URL: http/https (downloaded + cached), `file://`, or
    /// `asset://localhost/...` (resolved to a local path). `None` = no art.
    pub art_url: Option<String>,
}

/// Format the quality line shown under the artist/album, identical to the Tauri
/// `v2_format_notification_quality`. Empty string = omit the line.
fn format_quality(bit_depth: Option<u32>, sample_rate: Option<f64>) -> String {
    match (bit_depth, sample_rate) {
        (Some(bits), Some(rate)) if bits >= 24 || rate > 48.0 => {
            let rate_str = if rate.fract() == 0.0 {
                format!("{}", rate as u32)
            } else {
                format!("{rate}")
            };
            format!("Hi-Res - {bits}-bit/{rate_str}kHz")
        }
        (Some(bits), Some(rate)) => {
            let rate_str = if rate.fract() == 0.0 {
                format!("{}", rate as u32)
            } else {
                format!("{rate}")
            };
            format!("CD Quality - {bits}-bit/{rate_str}kHz")
        }
        _ => String::new(),
    }
}

/// Build the notification body: "artist · album" then a quality line.
/// `·` (middle dot) on macOS, `•` (bullet) elsewhere — matches Tauri.
fn build_body(meta: &NotificationMeta) -> String {
    let separator = if cfg!(target_os = "macos") {
        " \u{00b7} "
    } else {
        " \u{2022} "
    };
    let mut lines = Vec::new();
    let mut line1 = Vec::new();
    if !meta.artist.is_empty() {
        line1.push(meta.artist.clone());
    }
    if !meta.album.is_empty() {
        line1.push(meta.album.clone());
    }
    if !line1.is_empty() {
        lines.push(line1.join(separator));
    }
    let quality = format_quality(meta.bit_depth, meta.sample_rate);
    if !quality.is_empty() {
        lines.push(quality);
    }
    lines.join("\n")
}

// --- artwork cache (Linux + macOS) ------------------------------------------

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn artwork_cache_dir() -> Result<PathBuf, String> {
    let dir = dirs::cache_dir()
        .ok_or_else(|| "Could not find cache directory".to_string())?
        .join("qbz")
        .join("artwork");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create artwork cache dir: {e}"))?;
    Ok(dir)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn resolve_local_artwork(url: &str) -> Option<PathBuf> {
    if let Some(path) = url.strip_prefix("file://") {
        return Some(PathBuf::from(path));
    }
    if let Some(path) = url.strip_prefix("asset://localhost/") {
        let decoded = urlencoding::decode(path).ok()?;
        return Some(PathBuf::from(decoded.into_owned()));
    }
    None
}

/// Shared blocking HTTP client (a fresh client per track leaks an fd → EMFILE
/// over a long session — same reasoning as the Tauri image cache).
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: std::sync::OnceLock<reqwest::blocking::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .pool_max_idle_per_host(2)
            .build()
            .expect("failed to build notification HTTP client")
    })
}

/// Resolve `url` to a local image file: a `file://`/`asset://` URL maps
/// straight through, an http(s) URL is downloaded and cached by md5(url).
/// `offline` = local paths + md5 cache hits only, never the HTTP download —
/// the verdict is injected by the caller so this crate stays frontend-agnostic
/// (no dependency on the app's offline-mode engine).
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn cache_artwork(url: &str, offline: bool) -> Result<PathBuf, String> {
    use md5::{Digest, Md5};
    use std::io::Write;

    if let Some(local) = resolve_local_artwork(url) {
        if local.exists() {
            return Ok(local);
        }
    }

    let mut hasher = Md5::new();
    hasher.update(url.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let cache_path = artwork_cache_dir()?.join(format!("{hash}.jpg"));
    if cache_path.exists() {
        return Ok(cache_path);
    }

    if offline {
        return Err("offline: artwork not cached locally".to_string());
    }

    let response = http_client()
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .map_err(|e| format!("Failed to download artwork: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to download artwork: HTTP {} (url: {})",
            response.status(),
            url.split('?').next().unwrap_or(url)
        ));
    }
    let bytes = response
        .bytes()
        .map_err(|e| format!("Failed to read artwork bytes: {e}"))?;
    let mut file =
        std::fs::File::create(&cache_path).map_err(|e| format!("Failed to create cache file: {e}"))?;
    file.write_all(&bytes)
        .map_err(|e| format!("Failed to write artwork cache: {e}"))?;
    Ok(cache_path)
}

// --- Linux: portal icon bytes -----------------------------------------------

#[cfg(target_os = "linux")]
const PORTAL_ICON_MAX_EDGE: u32 = 512;
#[cfg(target_os = "linux")]
const PORTAL_ICON_MAX_BYTES: usize = 4 * 1024 * 1024;

/// Center-crop to a square, downscale to <=512px, re-encode PNG. Mirrors the
/// Tauri `v2_prepare_notification_icon_bytes`.
#[cfg(target_os = "linux")]
fn prepare_icon_bytes(path: &std::path::Path) -> Result<Vec<u8>, String> {
    use std::io::Cursor;

    let source = image::open(path).map_err(|e| format!("Failed to decode artwork {path:?}: {e}"))?;
    let (w, h) = (source.width(), source.height());
    let square = if w == h {
        source
    } else {
        let edge = w.min(h);
        source.crop_imm((w - edge) / 2, (h - edge) / 2, edge, edge)
    };
    let icon = if square.width() > PORTAL_ICON_MAX_EDGE {
        square.resize_exact(
            PORTAL_ICON_MAX_EDGE,
            PORTAL_ICON_MAX_EDGE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        square
    };
    let mut buf = Cursor::new(Vec::new());
    icon.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode notification PNG: {e}"))?;
    let bytes = buf.into_inner();
    if bytes.len() > PORTAL_ICON_MAX_BYTES {
        return Err(format!(
            "Notification icon too large after normalization: {} bytes (max {PORTAL_ICON_MAX_BYTES})",
            bytes.len()
        ));
    }
    Ok(bytes)
}

// --- public entry point -----------------------------------------------------

/// Show a track-change notification. Fire-and-forget: every failure is logged,
/// none propagated. Must be called from within a tokio runtime (it uses
/// `spawn_blocking` for the HTTP/image work). `offline` skips the artwork
/// HTTP download (local paths / disk-cache hits still render an icon).
pub async fn show_track_notification(meta: NotificationMeta, offline: bool) {
    let body = build_body(&meta);
    log::info!(
        "[notify] track notification: {} by {}",
        meta.title,
        meta.artist
    );

    #[cfg(target_os = "linux")]
    {
        use ashpd::desktop::notification::{Notification as PortalNotification, NotificationProxy};
        use ashpd::desktop::Icon;

        let mut notification =
            PortalNotification::new(&meta.title).body(Some(body.as_str()));

        if let Some(url) = meta.art_url.clone() {
            let prepared = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
                let path = cache_artwork(&url, offline)?;
                prepare_icon_bytes(&path)
            })
            .await;
            match prepared {
                Ok(Ok(bytes)) => {
                    log::debug!("[notify] artwork prepared: {} bytes", bytes.len());
                    notification = notification.icon(Icon::Bytes(bytes));
                }
                Ok(Err(e)) => log::warn!("[notify] could not prepare artwork: {e}"),
                Err(e) => log::warn!("[notify] artwork task failed: {e}"),
            }
        }

        match NotificationProxy::new().await {
            Ok(proxy) => {
                if let Err(e) = proxy
                    .add_notification("track-now-playing", notification)
                    .await
                {
                    log::warn!("[notify] XDG portal add_notification failed: {e}");
                }
            }
            Err(e) => log::warn!("[notify] XDG notification portal unavailable: {e}"),
        }
    }

    #[cfg(target_os = "macos")]
    {
        let _ = tokio::task::spawn_blocking(move || {
            let _ = notify_rust::set_application("com.blitzfc.qbz");
            let artwork_path = meta.art_url.as_deref().and_then(|url| match cache_artwork(url, offline) {
                Ok(path) => Some(path),
                Err(e) => {
                    log::debug!("[notify] could not cache artwork: {e}");
                    None
                }
            });
            let mut notification = notify_rust::Notification::new();
            notification.summary(&meta.title).body(&body);
            if let Some(path) = artwork_path.as_ref().and_then(|p| p.to_str()) {
                notification.image_path(path);
            }
            if let Err(e) = notification.show() {
                log::warn!("[notify] macOS notification failed: {e}");
            }
        })
        .await;
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = body;
        let _ = offline;
        log::info!("[notify] desktop notifications not implemented on this platform");
    }
}
