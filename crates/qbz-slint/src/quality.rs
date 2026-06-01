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
    let rate = if rate.fract().abs() < f64::EPSILON {
        format!("{}", rate as i64)
    } else {
        format!("{rate}")
    };
    format!("{depth}-bit / {rate} kHz")
}
