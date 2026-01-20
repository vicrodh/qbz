//! System notification commands

use notify_rust::Notification;
use md5::{Digest, Md5};
use std::path::PathBuf;
use std::fs;
use std::io::Write;

/// Cache directory for notification artwork
fn get_artwork_cache_dir() -> Result<PathBuf, String> {
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| "Could not find cache directory".to_string())?
        .join("qbz")
        .join("artwork");

    fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create artwork cache dir: {}", e))?;

    Ok(cache_dir)
}

/// Download artwork to cache and return the path
fn cache_artwork(url: &str) -> Result<PathBuf, String> {
    if let Some(local_path) = resolve_local_artwork(url) {
        if local_path.exists() {
            return Ok(local_path);
        }
    }

    // Create a hash of the URL for the filename
    let mut hasher = Md5::new();
    hasher.update(url.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let cache_dir = get_artwork_cache_dir()?;
    let cache_path = cache_dir.join(format!("{}.jpg", hash));

    // Return cached file if exists
    if cache_path.exists() {
        return Ok(cache_path);
    }

    // Download the image
    let response = reqwest::blocking::Client::new()
        .get(url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .map_err(|e| format!("Failed to download artwork: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Failed to download artwork: HTTP {}", response.status()));
    }

    let bytes = response.bytes()
        .map_err(|e| format!("Failed to read artwork bytes: {}", e))?;

    // Write to cache
    let mut file = fs::File::create(&cache_path)
        .map_err(|e| format!("Failed to create artwork cache file: {}", e))?;
    file.write_all(&bytes)
        .map_err(|e| format!("Failed to write artwork cache: {}", e))?;

    Ok(cache_path)
}

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

/// Format quality info for notification
fn format_quality(bit_depth: Option<u32>, sample_rate: Option<f64>) -> String {
    match (bit_depth, sample_rate) {
        (Some(bits), Some(rate)) if bits >= 24 || rate > 48.0 => {
            // Format sample rate: 44.1 → 44.1, 48.0 → 48, 192.0 → 192
            let rate_str = if rate.fract() == 0.0 {
                format!("{}", rate as u32)
            } else {
                format!("{}", rate)
            };
            format!("Hi-Res • {}-bit/{}kHz", bits, rate_str)
        }
        (Some(bits), Some(rate)) => {
            let rate_str = if rate.fract() == 0.0 {
                format!("{}", rate as u32)
            } else {
                format!("{}", rate)
            };
            format!("CD Quality • {}-bit/{}kHz", bits, rate_str)
        }
        _ => String::new()
    }
}

/// Show a track change notification with artwork
#[tauri::command]
pub fn show_track_notification(
    title: String,
    artist: String,
    album: String,
    artwork_url: Option<String>,
    bit_depth: Option<u32>,
    sample_rate: Option<f64>,  // Changed from u32 to f64 to support 44.1, 88.2, 176.4, etc.
) -> Result<(), String> {
    log::info!("Command: show_track_notification - {} by {}", title, artist);

    // Build body with 2-3 lines:
    // Line 1: Artist • Album
    // Line 2: Quality (if available)
    let mut lines = Vec::new();

    // Line 1: Artist and Album
    let mut line1_parts = Vec::new();
    if !artist.is_empty() {
        line1_parts.push(artist.clone());
    }
    if !album.is_empty() {
        line1_parts.push(album.clone());
    }
    if !line1_parts.is_empty() {
        lines.push(line1_parts.join(" • "));
    }

    // Line 2: Quality info
    let quality = format_quality(bit_depth, sample_rate);
    if !quality.is_empty() {
        lines.push(quality);
    }

    let body = lines.join("\n");

    let mut notification = Notification::new();
    notification
        .summary(&title)
        .body(&body)
        .appname("QBZ")
        .timeout(4000); // 4 seconds

    // Try to add artwork
    if let Some(url) = artwork_url {
        match cache_artwork(&url) {
            Ok(path) => {
                if let Some(path_str) = path.to_str() {
                    notification.image_path(path_str);
                    log::debug!("Notification artwork set: {}", path_str);
                }
            }
            Err(e) => {
                log::warn!("Could not cache artwork for notification: {}", e);
            }
        }
    }

    notification
        .show()
        .map_err(|e| format!("Failed to show notification: {}", e))?;

    Ok(())
}

/// Show a generic notification
#[tauri::command]
pub fn show_notification(
    title: String,
    body: Option<String>,
) -> Result<(), String> {
    log::info!("Command: show_notification - {}", title);

    let mut notification = Notification::new();
    notification.summary(&title).appname("QBZ").timeout(3000);

    if let Some(body_text) = body {
        notification.body(&body_text);
    }

    notification
        .show()
        .map_err(|e| format!("Failed to show notification: {}", e))?;

    Ok(())
}
