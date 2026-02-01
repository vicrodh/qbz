//! FFT Processing for Audio Visualization
//!
//! Runs on a dedicated thread, completely separate from audio playback.
//! Uses spectrum-analyzer crate for efficient FFT computation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use spectrum_analyzer::scaling::divide_by_N_sqrt;
use spectrum_analyzer::windows::hann_window;
use tauri::{AppHandle, Emitter};

use super::ring_buffer::RingBuffer;
use super::{NUM_BARS, FFT_SIZE, TARGET_FPS};

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

/// Main FFT processing loop
fn run_fft_loop(state: VisualizerState, app_handle: AppHandle) {
    // Pre-allocate all buffers to avoid allocations in the hot path
    let mut samples = vec![0.0f32; FFT_SIZE];
    let mut windowed = vec![0.0f32; FFT_SIZE];
    let mut output = vec![0.0f32; NUM_BARS];
    let mut smoothed = vec![0.0f32; NUM_BARS];

    // Smoothing factor: 0 = no smoothing, higher = more smoothing
    const SMOOTHING: f32 = 0.65;

    let frame_duration = Duration::from_micros(1_000_000 / TARGET_FPS);

    loop {
        let frame_start = Instant::now();

        if state.enabled.load(Ordering::Relaxed) {
            let sample_rate = state.sample_rate.load(Ordering::Relaxed);

            // Get samples from ring buffer
            state.ring_buffer.snapshot(&mut samples);

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
                }
                Err(e) => {
                    log::debug!("FFT error: {:?}", e);
                }
            }
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
        let num_bars = 64;
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

        // Middle bar (~32) should be around 630Hz (geometric mean of 20 and 20000)
        assert!(freqs[32] > 500.0 && freqs[32] < 800.0);

        // Last bar should approach 20000Hz
        assert!(freqs[num_bars - 1] > 15000.0);
    }
}
