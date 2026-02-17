//! FFT Processing for Audio Visualization
//!
//! Runs on a dedicated thread, completely separate from audio playback.
//! Uses spectrum-analyzer crate for efficient FFT computation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use qbz_audio::SpectralAnalyzer;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use spectrum_analyzer::scaling::divide_by_N_sqrt;
use spectrum_analyzer::windows::hann_window;
use tauri::{AppHandle, Emitter};

use super::{NUM_BARS, FFT_SIZE, TARGET_FPS, RingBuffer};

/// Shared state for the visualizer thread
pub struct VisualizerState {
    pub ring_buffer: Arc<RingBuffer>,
    pub enabled: Arc<AtomicBool>,
    pub sample_rate: Arc<AtomicU32>,
}

/// Start the FFT processing thread
pub fn start_visualizer_thread(state: VisualizerState, app_handle: AppHandle) {
    std::thread::Builder::new()
        .name("visualizer-fft".to_string())
        .spawn(move || {
            run_fft_loop(state, app_handle);
        })
        .expect("Failed to spawn visualizer thread");

    log::info!("Visualizer FFT thread started");
}

/// Number of energy bands for the Energy Bands visualizer
const NUM_ENERGY_BANDS: usize = 5;
const NUM_SPECTRAL_BANDS: usize = 190;
const SPECTRAL_UPDATE_RATE_HZ: u32 = 58;
const SPECTRAL_SMOOTHING: f32 = 0.80;

/// Energy band frequency ranges (Hz):
/// Sub-bass (20-60), Bass (60-250), Mids (250-2k), Presence (2k-6k), Air (6k-20k)
const ENERGY_BAND_RANGES: [(f32, f32); NUM_ENERGY_BANDS] = [
    (20.0, 60.0),
    (60.0, 250.0),
    (250.0, 2000.0),
    (2000.0, 6000.0),
    (6000.0, 20000.0),
];

/// Main FFT processing loop
fn run_fft_loop(state: VisualizerState, app_handle: AppHandle) {
    // Pre-allocate all buffers to avoid allocations in the hot path
    let mut samples = vec![0.0f32; FFT_SIZE];
    let mut windowed = vec![0.0f32; FFT_SIZE];
    let mut output = vec![0.0f32; NUM_BARS];
    let mut smoothed = vec![0.0f32; NUM_BARS];

    // Waveform buffer: 256 L + 256 R = 512 floats
    const WAVEFORM_POINTS: usize = 256;
    let mut waveform_buf = vec![0.0f32; WAVEFORM_POINTS * 2];

    // Energy bands state
    let mut energy_bands = [0.0f32; NUM_ENERGY_BANDS];
    let mut smoothed_energy = [0.0f32; NUM_ENERGY_BANDS];
    let mut spectral_analyzer = SpectralAnalyzer::new(
        state.sample_rate.load(Ordering::Relaxed),
        FFT_SIZE,
        NUM_SPECTRAL_BANDS,
        SPECTRAL_UPDATE_RATE_HZ,
        SPECTRAL_SMOOTHING,
    );
    let mut spectral_bytes = vec![0u8; NUM_SPECTRAL_BANDS * std::mem::size_of::<f32>()];

    // Transient detection state
    let mut prev_rms = 0.0f32;
    let mut transient_cooldown = 0u32; // frames remaining in cooldown
    const TRANSIENT_THRESHOLD: f32 = 0.04; // RMS jump threshold (sensitive)
    const TRANSIENT_COOLDOWN_FRAMES: u32 = 3; // ~100ms at 30fps

    // Smoothing factor: 0 = no smoothing, higher = more smoothing
    const SMOOTHING: f32 = 0.65;

    let frame_duration = Duration::from_micros(1_000_000 / TARGET_FPS);

    loop {
        let frame_start = Instant::now();

        if state.enabled.load(Ordering::Relaxed) {
            let sample_rate = state.sample_rate.load(Ordering::Relaxed);

            // Get samples from ring buffer
            state.ring_buffer.snapshot(&mut samples);

            // Emit compact, progressive spectrogram bands for Spectral Ribbon.
            if spectral_analyzer.process_audio_frame(&samples, sample_rate) {
                let spectral = spectral_analyzer.get_latest_bands();
                for (idx, value) in spectral.iter().enumerate() {
                    let offset = idx * 4;
                    spectral_bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                }
                let _ = app_handle.emit("viz:spectral", spectral_bytes.clone());
            }

            // Apply Hann window to reduce spectral leakage
            let window = hann_window(&samples);
            for (i, (sample, win)) in samples.iter().zip(window.iter()).enumerate() {
                windowed[i] = sample * win;
            }

            // Compute FFT spectrum
            match samples_fft_to_spectrum(
                &windowed,
                sample_rate,
                FrequencyLimit::Range(20.0, 20000.0),
                Some(&divide_by_N_sqrt),
            ) {
                Ok(spectrum) => {
                    // Map spectrum to logarithmic frequency bars
                    map_to_log_bars(&spectrum, &mut output);

                    // Apply smoothing for visual continuity
                    for i in 0..NUM_BARS {
                        let new = output[i];
                        // Faster attack, slower decay for punchy visuals
                        if new > smoothed[i] {
                            smoothed[i] = smoothed[i] * 0.3 + new * 0.7; // Fast attack
                        } else {
                            smoothed[i] = smoothed[i] * SMOOTHING + new * (1.0 - SMOOTHING); // Slow decay
                        }
                        output[i] = smoothed[i];
                    }

                    // Send to frontend as binary data
                    let bytes: Vec<u8> = output
                        .iter()
                        .flat_map(|f| f.to_le_bytes())
                        .collect();

                    let _ = app_handle.emit("viz:data", bytes);

                    // --- Energy Bands: compute RMS per frequency band from spectrum ---
                    let data = spectrum.data();
                    for (band_idx, &(lo, hi)) in ENERGY_BAND_RANGES.iter().enumerate() {
                        let mut sum_sq = 0.0f32;
                        let mut count = 0u32;
                        for (freq, magnitude) in data.iter() {
                            let f = freq.val();
                            if f >= lo && f < hi {
                                let mag = magnitude.val();
                                sum_sq += mag * mag;
                                count += 1;
                            }
                        }
                        let rms = if count > 0 {
                            (sum_sq / count as f32).sqrt()
                        } else {
                            0.0
                        };
                        // Compress and normalize
                        let compressed = (rms * 6.0).powf(0.5).clamp(0.0, 1.0);
                        // Smooth: fast attack, slow decay
                        if compressed > smoothed_energy[band_idx] {
                            smoothed_energy[band_idx] = smoothed_energy[band_idx] * 0.2 + compressed * 0.8;
                        } else {
                            smoothed_energy[band_idx] = smoothed_energy[band_idx] * 0.85 + compressed * 0.15;
                        }
                        energy_bands[band_idx] = smoothed_energy[band_idx];
                    }

                    let energy_bytes: Vec<u8> = energy_bands
                        .iter()
                        .flat_map(|f| f.to_le_bytes())
                        .collect();
                    let _ = app_handle.emit("viz:energy", energy_bytes);

                    // --- Transient Detection: detect sharp RMS jumps ---
                    // Use raw (pre-smoothed) RMS for transient sensitivity
                    // Weight bass/sub-bass more heavily for beat detection
                    let raw_rms = {
                        let mut raw_sum = 0.0f32;
                        for (band_idx, &(lo, hi)) in ENERGY_BAND_RANGES.iter().enumerate() {
                            let mut sum_sq = 0.0f32;
                            let mut cnt = 0u32;
                            for (freq, magnitude) in data.iter() {
                                let f = freq.val();
                                if f >= lo && f < hi {
                                    let mag = magnitude.val();
                                    sum_sq += mag * mag;
                                    cnt += 1;
                                }
                            }
                            let band_rms = if cnt > 0 { (sum_sq / cnt as f32).sqrt() } else { 0.0 };
                            // Bass/sub-bass weighted 2x for beat detection
                            let weight = if band_idx < 2 { 2.0 } else { 1.0 };
                            raw_sum += (band_rms * 6.0).powf(0.5).clamp(0.0, 1.0) * weight;
                        }
                        raw_sum / (NUM_ENERGY_BANDS as f32 + 2.0) // account for extra bass weight
                    };
                    let rms_delta = raw_rms - prev_rms;

                    if transient_cooldown > 0 {
                        transient_cooldown -= 1;
                    }

                    if rms_delta > TRANSIENT_THRESHOLD && transient_cooldown == 0 {
                        // Transient detected! Emit intensity (0.0 - 1.0)
                        let intensity = (rms_delta * 5.0).clamp(0.0, 1.0);
                        let transient_bytes: Vec<u8> = intensity.to_le_bytes().to_vec();
                        let _ = app_handle.emit("viz:transient", transient_bytes);
                        transient_cooldown = TRANSIENT_COOLDOWN_FRAMES;
                    }

                    prev_rms = raw_rms;
                }
                Err(e) => {
                    log::debug!("FFT error: {:?}", e);
                }
            }

            // Emit raw waveform data for oscilloscope (stereo L/R)
            // samples[] is interleaved: L0, R0, L1, R1, ...
            // 1024 samples = 512 stereo pairs → downsample to 256 per channel
            let stereo_pairs = FFT_SIZE / 2; // 512
            let step = stereo_pairs / WAVEFORM_POINTS; // 512/256 = 2
            for i in 0..WAVEFORM_POINTS {
                let base = i * step * 2; // index into interleaved buffer
                waveform_buf[i] = samples[base];                          // L
                waveform_buf[WAVEFORM_POINTS + i] = samples[base + 1];    // R
            }
            let waveform_bytes: Vec<u8> = waveform_buf
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();
            let _ = app_handle.emit("viz:waveform", waveform_bytes);
        }

        // Maintain target FPS
        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }
}

/// Map spectrum data to logarithmically-spaced frequency bars
///
/// Human hearing is logarithmic, so we use log-spaced bars to match
/// how we perceive frequency. This gives equal visual weight to
/// bass, mids, and treble.
fn map_to_log_bars(spectrum: &spectrum_analyzer::FrequencySpectrum, output: &mut [f32]) {
    let num_bars = output.len();

    // Frequency range (Hz)
    const MIN_FREQ: f32 = 20.0;
    const MAX_FREQ: f32 = 20000.0;

    let min_log = MIN_FREQ.ln();
    let max_log = MAX_FREQ.ln();

    // Get spectrum data
    let data = spectrum.data();

    for (i, bar) in output.iter_mut().enumerate() {
        // Calculate logarithmic frequency bounds for this bar
        let t_low = i as f32 / num_bars as f32;
        let t_high = (i + 1) as f32 / num_bars as f32;

        let freq_low = (min_log + (max_log - min_log) * t_low).exp();
        let freq_high = (min_log + (max_log - min_log) * t_high).exp();

        // Find all frequency bins that fall within this bar's range
        let mut sum = 0.0f32;
        let mut count = 0u32;

        for (freq, magnitude) in data.iter() {
            let f = freq.val();
            if f >= freq_low && f < freq_high {
                // Apply perceptual weighting (boost bass slightly)
                let weight = if f < 200.0 {
                    1.5 // Bass boost
                } else if f < 2000.0 {
                    1.0 // Mids
                } else {
                    0.8 // Reduce harsh highs
                };

                sum += magnitude.val() * weight;
                count += 1;
            }
        }

        // Average magnitude for this bar
        let avg = if count > 0 {
            sum / count as f32
        } else {
            0.0
        };

        // Apply dynamic range compression and normalize
        // This makes quiet passages more visible while preventing clipping
        let compressed = (avg * 4.0).powf(0.6);
        *bar = compressed.clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_frequency_distribution() {
        // Verify that frequency bars are logarithmically distributed
        let num_bars = NUM_BARS; // Use the actual constant
        let min_log = 20.0_f32.ln();
        let max_log = 20000.0_f32.ln();

        let mut freqs = Vec::new();
        for i in 0..num_bars {
            let t = i as f32 / num_bars as f32;
            let freq = (min_log + (max_log - min_log) * t).exp();
            freqs.push(freq);
        }

        // First bar should be around 20Hz
        assert!(freqs[0] > 19.0 && freqs[0] < 25.0);

        // Middle bar (~16 for 32 bars) should be around 630Hz (geometric mean of 20 and 20000)
        let mid = num_bars / 2;
        assert!(freqs[mid] > 500.0 && freqs[mid] < 800.0);

        // Last bar should approach 20000Hz
        assert!(freqs[num_bars - 1] > 15000.0);
    }
}
