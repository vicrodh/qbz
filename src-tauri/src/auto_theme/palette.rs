//! K-means palette extraction from images.

use image::GenericImageView;
use std::path::Path;

use super::PaletteColor;
use super::ThemePalette;

/// Load an image, downsample, and extract dominant colors via k-means.
pub fn extract_palette(image_path: &str) -> Result<ThemePalette, String> {
    let path = Path::new(image_path);
    if !path.exists() {
        return Err(format!("Image not found: {}", image_path));
    }

    let img = image::open(path).map_err(|e| format!("Failed to open image: {}", e))?;

    // Downsample to ~100x100 for fast processing
    let thumb = img.resize_exact(100, 100, image::imageops::FilterType::Lanczos3);

    let mut pixels: Vec<[f64; 3]> = Vec::with_capacity(10_000);
    for (_x, _y, rgba) in thumb.pixels() {
        // Skip semi-transparent pixels
        if rgba[3] < 200 {
            continue;
        }
        pixels.push([rgba[0] as f64, rgba[1] as f64, rgba[2] as f64]);
    }

    if pixels.is_empty() {
        return Err("Image contains no opaque pixels".to_string());
    }

    let clusters = kmeans(&pixels, 6, 25);

    if clusters.is_empty() {
        return Err("K-means produced no clusters".to_string());
    }

    build_palette(clusters)
}

/// Extract palette directly from raw RGB pixel data (for testing or custom sources).
pub fn extract_palette_from_pixels(pixels: &[[f64; 3]]) -> Result<ThemePalette, String> {
    if pixels.is_empty() {
        return Err("No pixels provided".to_string());
    }
    let clusters = kmeans(pixels, 6, 25);
    if clusters.is_empty() {
        return Err("K-means produced no clusters".to_string());
    }
    build_palette(clusters)
}

/// A cluster result: centroid + pixel count.
#[derive(Debug, Clone)]
struct Cluster {
    centroid: [f64; 3],
    count: usize,
}

/// Simple k-means clustering on RGB values.
fn kmeans(pixels: &[[f64; 3]], k: usize, max_iters: usize) -> Vec<Cluster> {
    let n = pixels.len();
    if n == 0 || k == 0 {
        return Vec::new();
    }
    let k = k.min(n);

    // Initialize centroids by evenly sampling from the pixel list
    let mut centroids: Vec<[f64; 3]> = Vec::with_capacity(k);
    let step = n / k;
    for i in 0..k {
        centroids.push(pixels[i * step]);
    }

    let mut assignments = vec![0usize; n];

    for _ in 0..max_iters {
        let mut changed = false;

        // Assignment step
        for (idx, pixel) in pixels.iter().enumerate() {
            let mut best_cluster = 0;
            let mut best_dist = f64::MAX;
            for (ci, centroid) in centroids.iter().enumerate() {
                let dist = rgb_dist_sq(pixel, centroid);
                if dist < best_dist {
                    best_dist = dist;
                    best_cluster = ci;
                }
            }
            if assignments[idx] != best_cluster {
                assignments[idx] = best_cluster;
                changed = true;
            }
        }

        if !changed {
            break;
        }

        // Update step
        let mut sums = vec![[0.0f64; 3]; k];
        let mut counts = vec![0usize; k];
        for (idx, pixel) in pixels.iter().enumerate() {
            let ci = assignments[idx];
            sums[ci][0] += pixel[0];
            sums[ci][1] += pixel[1];
            sums[ci][2] += pixel[2];
            counts[ci] += 1;
        }

        for ci in 0..k {
            if counts[ci] > 0 {
                centroids[ci][0] = sums[ci][0] / counts[ci] as f64;
                centroids[ci][1] = sums[ci][1] / counts[ci] as f64;
                centroids[ci][2] = sums[ci][2] / counts[ci] as f64;
            }
        }
    }

    // Build final clusters
    let mut counts = vec![0usize; k];
    for &a in &assignments {
        counts[a] += 1;
    }

    centroids
        .into_iter()
        .zip(counts.into_iter())
        .filter(|(_, count)| *count > 0)
        .map(|(centroid, count)| Cluster { centroid, count })
        .collect()
}

fn rgb_dist_sq(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let dr = a[0] - b[0];
    let dg = a[1] - b[1];
    let db = a[2] - b[2];
    dr * dr + dg * dg + db * db
}

/// Build a ThemePalette from k-means clusters.
fn build_palette(mut clusters: Vec<Cluster>) -> Result<ThemePalette, String> {
    // Sort by pixel count (dominant first)
    clusters.sort_by(|a, b| b.count.cmp(&a.count));

    let all_colors: Vec<PaletteColor> = clusters
        .iter()
        .map(|c| {
            PaletteColor::new(
                c.centroid[0].round() as u8,
                c.centroid[1].round() as u8,
                c.centroid[2].round() as u8,
            )
        })
        .collect();

    let dominant = all_colors[0];
    let is_dark = dominant.luminance() < 0.5;

    // Check if the image is monochromatic (all clusters very similar)
    let max_distance = all_colors
        .iter()
        .skip(1)
        .map(|c| dominant.distance(c))
        .fold(0.0f64, f64::max);

    let is_monochrome = max_distance < 40.0;

    // Assign background roles from dominant color with lightness shifts
    let (bg_primary, bg_secondary, bg_tertiary, bg_hover) = if is_dark {
        (
            dominant.shift_lightness(-0.05), // slightly darker
            dominant.shift_lightness(0.03),
            dominant.shift_lightness(0.08),
            dominant.shift_lightness(0.02),
        )
    } else {
        (
            dominant.shift_lightness(0.05), // slightly lighter
            dominant.shift_lightness(-0.03),
            dominant.shift_lightness(-0.08),
            dominant.shift_lightness(-0.02),
        )
    };

    // Find best accent: most saturated cluster with WCAG AA contrast (4.5:1) against bg_primary
    let accent = if is_monochrome {
        // Monochrome fallback: use a standard accent
        if is_dark {
            PaletteColor::new(66, 133, 244) // blue
        } else {
            PaletteColor::new(26, 115, 232) // darker blue
        }
    } else {
        find_best_accent(&all_colors, &bg_primary, is_dark)
    };

    Ok(ThemePalette {
        bg_primary,
        bg_secondary,
        bg_tertiary,
        bg_hover,
        accent,
        is_dark,
        all_colors,
    })
}

/// Find the best accent color: prioritize saturation, then ensure WCAG AA contrast.
fn find_best_accent(
    colors: &[PaletteColor],
    bg: &PaletteColor,
    is_dark: bool,
) -> PaletteColor {
    // Sort candidates by saturation (highest first), skip the dominant (index 0)
    let mut candidates: Vec<(usize, f64)> = colors
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, c)| (i, c.saturation()))
        .collect();
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (idx, _sat) in &candidates {
        let color = colors[*idx];
        let ratio = color.contrast_ratio(bg);
        if ratio >= 4.5 {
            return color;
        }
        // Try adjusting lightness to meet contrast
        let adjusted = adjust_for_contrast(&color, bg, is_dark);
        if adjusted.contrast_ratio(bg) >= 4.5 {
            return adjusted;
        }
    }

    // Fallback: use the most saturated and force-adjust
    if let Some((idx, _)) = candidates.first() {
        let color = colors[*idx];
        adjust_for_contrast(&color, bg, is_dark)
    } else {
        // Ultimate fallback
        if is_dark {
            PaletteColor::new(66, 133, 244)
        } else {
            PaletteColor::new(26, 115, 232)
        }
    }
}

/// Adjust a color's lightness to achieve WCAG AA contrast against a background.
fn adjust_for_contrast(
    color: &PaletteColor,
    bg: &PaletteColor,
    is_dark: bool,
) -> PaletteColor {
    let (h, s, l) = color.to_hsl();
    let direction = if is_dark { 0.05 } else { -0.05 };

    let mut new_l = l;
    for _ in 0..20 {
        new_l = (new_l + direction).clamp(0.0, 1.0);
        let candidate = PaletteColor::from_hsl(h, s, new_l);
        if candidate.contrast_ratio(bg) >= 4.5 {
            return candidate;
        }
    }

    PaletteColor::from_hsl(h, s, new_l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kmeans_basic() {
        let pixels: Vec<[f64; 3]> = vec![
            [0.0, 0.0, 0.0],
            [10.0, 10.0, 10.0],
            [5.0, 5.0, 5.0],
            [250.0, 250.0, 250.0],
            [240.0, 240.0, 240.0],
            [245.0, 245.0, 245.0],
        ];
        let clusters = kmeans(&pixels, 2, 20);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn test_extract_from_pixels() {
        // A mix of dark blues and bright oranges
        let mut pixels = Vec::new();
        for _ in 0..500 {
            pixels.push([20.0, 30.0, 80.0]); // dark blue
        }
        for _ in 0..300 {
            pixels.push([240.0, 160.0, 40.0]); // orange
        }
        for _ in 0..200 {
            pixels.push([10.0, 10.0, 15.0]); // near-black
        }

        let palette = extract_palette_from_pixels(&pixels).unwrap();
        assert!(palette.is_dark); // dominant is dark
        assert!(palette.accent.saturation() > 0.1); // accent should be saturated
    }

    #[test]
    fn test_monochrome_fallback() {
        // All pixels are very similar grays
        let pixels: Vec<[f64; 3]> = (0..1000).map(|_| [50.0, 52.0, 48.0]).collect();
        let palette = extract_palette_from_pixels(&pixels).unwrap();
        // Should still produce a valid palette with fallback accent
        assert!(palette.accent.saturation() > 0.1 || palette.accent.luminance() > 0.3);
    }

    #[test]
    fn test_accent_has_contrast() {
        let mut pixels = Vec::new();
        for _ in 0..700 {
            pixels.push([15.0, 15.0, 20.0]);
        }
        for _ in 0..300 {
            pixels.push([200.0, 50.0, 80.0]);
        }

        let palette = extract_palette_from_pixels(&pixels).unwrap();
        let ratio = palette.accent.contrast_ratio(&palette.bg_primary);
        // Should meet or approach WCAG AA
        assert!(ratio >= 3.0, "Accent contrast ratio too low: {}", ratio);
    }
}
