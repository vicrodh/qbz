//! ImmersiveView visual glue.
//!
//! The Tauri background's "full" mode is Kawarp (Kawase blur + domain warp).
//! Slint/femtovg cannot run that shader directly, so this module produces the
//! same source material as Tauri's atmosphere texture: a tiny artwork color
//! field scaled up, blurred, saturated, warmed, and vignetted. The Slint layer
//! animates two copies in opposite directions to approximate the warp.

use image::{imageops, Rgba, RgbaImage};
use slint::Color;

/// Generate a 128x128 atmospheric image from decoded RGBA artwork pixels.
/// Mirrors `src/lib/immersive/utils/texture-loader.ts::generateAtmosphere`.
pub fn generate_atmosphere(pixels: &[u8], width: u32, height: u32) -> Option<(Vec<u8>, u32, u32)> {
    let src = RgbaImage::from_raw(width, height, pixels.to_vec())?;
    let tiny = imageops::resize(&src, 8, 8, imageops::FilterType::Triangle);
    let scaled = imageops::resize(&tiny, 128, 128, imageops::FilterType::CatmullRom);
    let blurred = imageops::blur(&scaled, 16.0);
    let adjusted = color_adjust(blurred);
    let final_img = vignette(adjusted, 0.20);
    Some((final_img.into_raw(), 128, 128))
}

/// AlbumReactive glow color: the most saturated non-extreme 8x8 sample.
pub fn glow_color(pixels: &[u8], width: u32, height: u32) -> Color {
    let Some(src) = RgbaImage::from_raw(width, height, pixels.to_vec()) else {
        return Color::from_argb_u8(0x59, 100, 100, 255);
    };
    let tiny = imageops::resize(&src, 8, 8, imageops::FilterType::Triangle);
    let mut best_sat = 0.0f32;
    let mut best = (100u8, 100u8, 255u8);

    for px in tiny.pixels() {
        let r = px[0] as f32;
        let g = px[1] as f32;
        let b = px[2] as f32;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let lum = (max + min) / 2.0;
        let sat = if (max - min).abs() < f32::EPSILON {
            0.0
        } else if lum > 127.0 {
            (max - min) / (510.0 - max - min).max(1.0)
        } else {
            (max - min) / (max + min).max(1.0)
        };
        if lum > 50.0 && lum < 220.0 && sat > best_sat {
            best_sat = sat;
            best = (px[0], px[1], px[2]);
        }
    }

    Color::from_argb_u8(0x59, best.0, best.1, best.2)
}

/// Two vivid bar colors for the Spectrum visualizer, derived from the artwork's
/// PERCEIVED dominant tone. We bin chromatic pixels by hue and pick the most
/// ABUNDANT hue (one vote per pixel = coverage), NOT the most saturated — so a
/// metallic/dark cover resolves to the steel-blue you actually see, instead of
/// an amplified speck of an unseen magenta highlight. The picked hue is forced
/// vivid + mid-bright so it reads on the black bg; the secondary stop rotates
/// +55° for a clear gradient. A cover with essentially no chromatic pixels (a
/// true B&W cover) falls back to a default duotone.
/// Returns (primary at the base, secondary at the tip).
pub fn spectrum_colors(pixels: &[u8], width: u32, height: u32) -> (Color, Color) {
    let default = (
        Color::from_rgb_u8(0, 220, 200),
        Color::from_rgb_u8(150, 50, 255),
    );
    let Some(src) = RgbaImage::from_raw(width, height, pixels.to_vec()) else {
        return default;
    };
    let tiny = imageops::resize(&src, 16, 16, imageops::FilterType::Triangle);

    // Hue histogram over CHROMATIC pixels, weighted by COVERAGE (one vote per
    // pixel). 24 bins of 15°. Near-grey / near-black / near-white pixels carry
    // no usable hue and are skipped, so the grey mass never votes.
    const BINS: usize = 24;
    let mut hist = [0.0f32; BINS];
    let mut chromatic = 0u32;
    for px in tiny.pixels() {
        let (h, s, l) = rgb_to_hsl(px[0], px[1], px[2]);
        if !(0.10..=0.93).contains(&l) || s < 0.08 {
            continue;
        }
        let bin = ((h / 360.0 * BINS as f32) as usize).min(BINS - 1);
        hist[bin] += 1.0;
        chromatic += 1;
    }

    // Too few tinted pixels (effectively a B&W cover): no perceived tone.
    if chromatic < 4 {
        return default;
    }

    // Smoothed cluster score for a bin (peak + circular neighbours).
    let score_at = |i: usize| hist[i] + 0.5 * (hist[(i + BINS - 1) % BINS] + hist[(i + 1) % BINS]);

    // PRIMARY = most abundant hue cluster.
    let mut best_i = 0usize;
    let mut best = -1.0f32;
    for i in 0..BINS {
        let sc = score_at(i);
        if sc > best {
            best = sc;
            best_i = i;
        }
    }
    let primary_hue = (best_i as f32 + 0.5) * (360.0 / BINS as f32);

    // SECONDARY = a SECOND genuine hue cluster, at least ~45° away from the
    // primary and carrying real mass (>= 35% of the peak). If the cover is
    // essentially one colour (e.g. the mono pink/magenta Caifanes cover) there
    // is NO honest second hue — derive the secondary from the SAME hue, just
    // deeper + more saturated, instead of fabricating a hue-rotated colour the
    // album doesn't contain (the old `+55°` turned a pink cover into pink→orange).
    let mut sec_i: Option<usize> = None;
    let mut sec_best = 0.0f32;
    for i in 0..BINS {
        let circ = (i as i32 - best_i as i32).rem_euclid(BINS as i32);
        let dist = circ.min(BINS as i32 - circ); // circular distance in bins
        if dist < 3 {
            continue; // keep >= ~45° away from the primary
        }
        let sc = score_at(i);
        if sc > sec_best {
            sec_best = sc;
            sec_i = Some(i);
        }
    }

    let primary = hsl_to_rgb(primary_hue, 0.85, 0.58);
    let secondary = match sec_i.filter(|_| sec_best >= best * 0.35) {
        // Two genuinely different album colours → gradient between them.
        Some(si) => {
            let sec_hue = (si as f32 + 0.5) * (360.0 / BINS as f32);
            hsl_to_rgb(sec_hue, 0.88, 0.62)
        }
        // Single-colour cover → same hue, deeper (stays on-album).
        None => hsl_to_rgb(primary_hue, 0.95, 0.40),
    };
    (
        Color::from_rgb_u8(primary.0, primary.1, primary.2),
        Color::from_rgb_u8(secondary.0, secondary.1, secondary.2),
    )
}

/// RGB(0..255) -> HSL with H in [0,360), S and L in [0,1].
fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1.0e-6 {
        return (0.0, 0.0, l);
    }
    let s = (d / (1.0 - (2.0 * l - 1.0).abs())).clamp(0.0, 1.0);
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h.rem_euclid(360.0), s, l)
}

/// HSL (H in degrees, S and L in [0,1]) -> RGB(0..255).
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

fn color_adjust(mut img: RgbaImage) -> RgbaImage {
    let mut min_r = 255.0f32;
    let mut min_g = 255.0f32;
    let mut min_b = 255.0f32;
    let mut max_r = 0.0f32;
    let mut max_g = 0.0f32;
    let mut max_b = 0.0f32;
    let mut total_brightness = 0.0f32;
    let count = (img.width() * img.height()).max(1) as f32;

    for px in img.pixels_mut() {
        let (r, g, b) = saturate_brightness_contrast(px[0], px[1], px[2]);
        px[0] = r;
        px[1] = g;
        px[2] = b;

        let rf = r as f32;
        let gf = g as f32;
        let bf = b as f32;
        min_r = min_r.min(rf);
        min_g = min_g.min(gf);
        min_b = min_b.min(bf);
        max_r = max_r.max(rf);
        max_g = max_g.max(gf);
        max_b = max_b.max(bf);
        total_brightness += (rf + gf + bf) / 3.0;
    }

    let norm_strength = (total_brightness / count / 80.0).min(1.0);
    let target_min = 18.0f32;
    let target_range = 232.0f32;
    let range_r = (max_r - min_r).max(1.0);
    let range_g = (max_g - min_g).max(1.0);
    let range_b = (max_b - min_b).max(1.0);

    for px in img.pixels_mut() {
        let r = px[0] as f32;
        let g = px[1] as f32;
        let b = px[2] as f32;
        let norm_r = target_min + ((r - min_r) / range_r) * target_range;
        let norm_g = target_min + ((g - min_g) / range_g) * target_range;
        let norm_b = target_min + ((b - min_b) / range_b) * target_range;
        let lift_r = (r * 1.5).min(255.0);
        let lift_g = (g * 1.5).min(255.0);
        let lift_b = (b * 1.5).min(255.0);

        px[0] = (lift_r + (norm_r - lift_r) * norm_strength).mul_add(1.08, 0.0).min(255.0) as u8;
        px[1] = (lift_g + (norm_g - lift_g) * norm_strength).min(255.0) as u8;
        px[2] = (lift_b + (norm_b - lift_b) * norm_strength).min(255.0) as u8;
    }

    img
}

fn saturate_brightness_contrast(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let mut rf = r as f32;
    let mut gf = g as f32;
    let mut bf = b as f32;
    let gray = (rf + gf + bf) / 3.0;
    let sat = 2.45;
    rf = gray + (rf - gray) * sat;
    gf = gray + (gf - gray) * sat;
    bf = gray + (bf - gray) * sat;

    let brightness = 0.92;
    let contrast = 1.18;
    let adjust = |v: f32| ((v * brightness - 128.0) * contrast + 128.0).clamp(0.0, 255.0) as u8;
    (adjust(rf), adjust(gf), adjust(bf))
}

fn vignette(mut img: RgbaImage, intensity: f32) -> RgbaImage {
    let w = img.width() as f32;
    let h = img.height() as f32;
    let cx = w / 2.0;
    let cy = h / 2.0;
    let inner = w.min(h) * 0.20;
    let outer = w.min(h) * 0.70;

    for (x, y, px) in img.enumerate_pixels_mut() {
        let dx = x as f32 - cx;
        let dy = y as f32 - cy;
        let d = (dx * dx + dy * dy).sqrt();
        let t = ((d - inner) / (outer - inner)).clamp(0.0, 1.0);
        let factor = 1.0 - intensity * t;
        *px = Rgba([
            (px[0] as f32 * factor) as u8,
            (px[1] as f32 * factor) as u8,
            (px[2] as f32 * factor) as u8,
            px[3],
        ]);
    }
    img
}
