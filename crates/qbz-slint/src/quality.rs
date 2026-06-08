//! Shared quality-label formatting for the track / album quality badges.
//!
//! `detail` is the exact bit-depth / sample-rate line shown under the tier
//! label, e.g. `"24-bit / 96 kHz"` — no `Hi-Res` / `CD` prefix (the badge
//! renders the tier separately). It matches the AlbumView header badge so a
//! track row and an album header advertise quality identically.

/// Format the quality detail string from a track's max bit depth and sample
/// rate. Defaults to CD (16-bit / 44.1 kHz) / Hi-Res (24-bit / 96 kHz) values
/// when a field is missing, mirroring the AlbumView badge. The badge itself is
/// hidden when the tier is empty, so the value is irrelevant for unknown-quality
/// rows.
pub fn detail(bit_depth: Option<u32>, sample_rate: Option<f64>) -> String {
    let hi_res = matches!(bit_depth, Some(depth) if depth >= 24);
    let depth = bit_depth.unwrap_or(if hi_res { 24 } else { 16 });
    let rate = sample_rate.unwrap_or(if hi_res { 96.0 } else { 44.1 });
    // Accept Hz or kHz: Qobuz passes kHz (e.g. 96.0), while local/Plex track
    // rows pass raw Hz (e.g. 96000). Normalize so every surface — album-card,
    // album-detail header, track row, now-playing stamp — shows "96 kHz", not
    // a mix of "96 kHz" and "96000 kHz".
    let rate = if rate >= 1000.0 { rate / 1000.0 } else { rate };
    let rate = if rate.fract().abs() < f64::EPSILON {
        format!("{}", rate as i64)
    } else {
        format!("{rate}")
    };
    format!("{depth}-bit / {rate} kHz")
}

/// Lossless container/codec formats — the file IS lossless even when its exact
/// bit depth / sample rate isn't known yet (e.g. an un-hydrated Plex track).
pub fn is_lossless_format(format: &str) -> bool {
    matches!(
        format.trim().to_ascii_lowercase().as_str(),
        "flac" | "wav" | "wave" | "aiff" | "aif" | "alac" | "ape" | "dsd" | "dsf" | "dff"
    )
}

/// Badge tier from a container/codec + (possibly-unknown) max bit depth:
/// - `"mp3"`      lossy
/// - `"hires"`    >= 24-bit
/// - `"cd"`       known bit depth < 24
/// - `"lossless"` bit depth UNKNOWN but the container IS lossless (un-hydrated
///   Plex FLAC etc.) — show the filetype; better than no badge at all
/// - `""`         unknown format AND unknown bit depth → badge hidden
pub fn tier(format: &str, bit_depth: Option<u32>) -> &'static str {
    if format.trim().eq_ignore_ascii_case("mp3") {
        return "mp3";
    }
    match bit_depth {
        Some(b) if b >= 24 => "hires",
        Some(_) => "cd",
        None if is_lossless_format(format) => "lossless",
        None => "",
    }
}

/// The ONE source of truth for a quality badge: `(tier, detail, tooltip)`.
/// Every local/Plex surface — album card, album-detail header, track row,
/// now-playing stamp — goes through this so they can never disagree (the
/// earlier per-surface duplication is exactly what produced "96 kHz" on one
/// surface and "96000 kHz" on another). `sample_rate` accepts Hz OR kHz: the
/// shared [`detail`] normalizes via a `>= 1000` guard, so Qobuz's already-kHz
/// values are never re-divided (no "0.041 kHz") and Hz values become kHz once.
/// For an un-hydrated lossless track the detail is the bare filetype (`"FLAC"`);
/// after hydration it becomes the usual `"24-bit / 96 kHz"`, matching Qobuz.
pub fn badge(
    format: &str,
    bit_depth: Option<u32>,
    sample_rate: Option<f64>,
) -> (&'static str, String, String) {
    let t = tier(format, bit_depth);
    match t {
        "" | "mp3" => (t, String::new(), String::new()),
        "lossless" => {
            let f = format.trim().to_uppercase();
            (t, f.clone(), format!("Lossless: {f}"))
        }
        _ => {
            let d = detail(bit_depth, sample_rate);
            let prefix = if t == "hires" { "Hi-Res" } else { "CD" };
            (t, d.clone(), format!("{prefix}: {d}"))
        }
    }
}
