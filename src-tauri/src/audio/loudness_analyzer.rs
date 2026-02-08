//! Background loudness analyzer thread.
//!
//! Long-lived thread that receives decoded audio samples from `AnalyzerTap`,
//! computes EBU R128 integrated LUFS, and updates a shared `Arc<AtomicU32>`
//! gain value that `DynamicAmplify` reads.
//!
//! - First measurement after ~10s of audio (EBU R128 needs sufficient data)
//! - Refinement every ~5s thereafter (gain converges by ~30-60s)
//! - Cached results are used immediately on cache hit

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;

use ebur128::{EbuR128, Mode};

use super::analyzer_tap::AnalyzerMessage;
use super::loudness_cache::LoudnessCache;
use super::loudness::db_to_linear;

/// Maximum gain boost in dB (conservative clipping prevention)
const MAX_GAIN_DB: f32 = 6.0;


pub struct LoudnessAnalyzer;

impl LoudnessAnalyzer {
    /// Spawn the analyzer thread. Returns the join handle.
    ///
    /// The thread blocks on `rx.recv()` when idle — zero CPU usage between tracks.
    pub fn spawn(
        rx: Receiver<AnalyzerMessage>,
        cache: Arc<LoudnessCache>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("loudness-analyzer".into())
            .spawn(move || {
                log::info!("[LoudnessAnalyzer] Thread started");
                Self::run(rx, cache);
                log::info!("[LoudnessAnalyzer] Thread exiting");
            })
            .expect("Failed to spawn loudness analyzer thread")
    }

    fn run(rx: Receiver<AnalyzerMessage>, cache: Arc<LoudnessCache>) {
        let mut state: Option<AnalyzerState> = None;

        loop {
            let msg = match rx.recv() {
                Ok(msg) => msg,
                Err(_) => {
                    log::info!("[LoudnessAnalyzer] Channel closed, shutting down");
                    break;
                }
            };

            match msg {
                AnalyzerMessage::NewTrack { track_id, sample_rate, channels, target_lufs, gain_atomic } => {
                    log::info!(
                        "[LoudnessAnalyzer] New track {} ({}Hz, {}ch, target {:.1} LUFS)",
                        track_id, sample_rate, channels, target_lufs
                    );

                    // Check cache first
                    if let Some(cached) = cache.get(track_id) {
                        let gain = compute_gain_capped(cached.gain_db);
                        log::info!(
                            "[LoudnessAnalyzer] Cache hit for track {}: {:.2} dB (source: {}), gain {:.4}",
                            track_id, cached.gain_db, cached.source, gain
                        );

                        // Set gain immediately via the atomic
                        gain_atomic.store(gain.to_bits(), Ordering::Relaxed);

                        // Create state marked as cached — still accept samples for refinement
                        let mut s = AnalyzerState::new(track_id, sample_rate, channels, target_lufs);
                        s.gain_atomic = Some(gain_atomic);
                        s.initial_done = true;
                        state = Some(s);
                        continue;
                    }

                    // No cache — start fresh analysis
                    let mut s = AnalyzerState::new(track_id, sample_rate, channels, target_lufs);
                    s.gain_atomic = Some(gain_atomic);
                    state = Some(s);
                }
                AnalyzerMessage::Samples(samples) => {
                    if let Some(ref mut s) = state {
                        s.feed_samples(&samples, &cache);
                    }
                }
                AnalyzerMessage::Reset => {
                    if let Some(ref mut s) = state {
                        log::info!("[LoudnessAnalyzer] Reset (seek) — keeping current gain");
                        s.reset_analyzer();
                    }
                }
                AnalyzerMessage::Shutdown => {
                    log::info!("[LoudnessAnalyzer] Shutdown requested");
                    break;
                }
            }
        }
    }
}

struct AnalyzerState {
    track_id: u64,
    target_lufs: f32,
    ebur128: EbuR128,
    channels: u16,
    sample_rate: u32,
    /// Shared gain atomic — written by us, read by DynamicAmplify
    gain_atomic: Option<Arc<AtomicU32>>,
    /// Total samples fed since last reset
    samples_fed: u64,
    /// Total samples fed at last measurement
    samples_at_last_measure: u64,
    /// Whether initial measurement has been done
    initial_done: bool,
    /// Dynamic thresholds based on actual sample rate and channels
    initial_threshold: u64,
    refinement_interval: u64,
}

impl AnalyzerState {
    fn new(track_id: u64, sample_rate: u32, channels: u16, target_lufs: f32) -> Self {
        let ebur128 = EbuR128::new(channels as u32, sample_rate, Mode::I)
            .expect("Failed to create EbuR128 instance");

        // Scale thresholds to actual sample rate and channel count
        let samples_per_second = sample_rate as u64 * channels as u64;
        let initial_threshold = samples_per_second * 10; // 10 seconds
        let refinement_interval = samples_per_second * 5; // 5 seconds

        Self {
            track_id,
            target_lufs,
            ebur128,
            channels,
            sample_rate,
            gain_atomic: None,
            samples_fed: 0,
            samples_at_last_measure: 0,
            initial_done: false,
            initial_threshold,
            refinement_interval,
        }
    }

    /// Reset the EBU R128 analyzer (e.g., after seek) but keep the gain atomic.
    fn reset_analyzer(&mut self) {
        self.ebur128 = EbuR128::new(self.channels as u32, self.sample_rate, Mode::I)
            .expect("Failed to create EbuR128 instance");
        self.samples_fed = 0;
        self.samples_at_last_measure = 0;
        self.initial_done = false;
    }

    /// Feed samples to the EBU R128 analyzer and possibly update gain.
    fn feed_samples(&mut self, samples: &[f32], cache: &LoudnessCache) {
        // Feed interleaved samples as frames
        let frame_count = samples.len() / self.channels as usize;
        if frame_count == 0 {
            return;
        }

        if let Err(e) = self.ebur128.add_frames_f32(samples) {
            log::warn!("[LoudnessAnalyzer] Error feeding samples: {}", e);
            return;
        }

        self.samples_fed += samples.len() as u64;

        // Check if it's time to measure
        let should_measure = if !self.initial_done {
            self.samples_fed >= self.initial_threshold
        } else {
            self.samples_fed - self.samples_at_last_measure >= self.refinement_interval
        };

        if should_measure {
            self.measure_and_update(cache);
        }
    }

    fn measure_and_update(&mut self, cache: &LoudnessCache) {
        let loudness = match self.ebur128.loudness_global() {
            Ok(l) => l,
            Err(e) => {
                log::warn!("[LoudnessAnalyzer] Failed to get loudness: {}", e);
                return;
            }
        };

        // -inf means silence — don't adjust
        if loudness.is_infinite() || loudness.is_nan() {
            log::debug!("[LoudnessAnalyzer] Track {}: loudness is {:?}, skipping", self.track_id, loudness);
            return;
        }

        let measured_lufs = loudness as f32;
        let adjustment_db = self.target_lufs - measured_lufs;
        let gain = compute_gain_capped(adjustment_db);

        let phase = if self.initial_done { "refine" } else { "initial" };
        log::info!(
            "[LoudnessAnalyzer] Track {} ({}): measured {:.1} LUFS, target {:.1}, adjustment {:.2} dB, gain {:.4}",
            self.track_id, phase, measured_lufs, self.target_lufs, adjustment_db, gain
        );

        // Update shared atomic
        if let Some(ref atomic) = self.gain_atomic {
            atomic.store(gain.to_bits(), Ordering::Relaxed);
        }

        self.samples_at_last_measure = self.samples_fed;
        self.initial_done = true;

        // Cache the result (store the adjustment in dB, not the linear gain)
        cache.set(self.track_id, adjustment_db, 0.0, "ebur128");
    }
}

/// Convert a dB adjustment to a capped linear gain factor.
fn compute_gain_capped(adjustment_db: f32) -> f32 {
    let capped_db = adjustment_db.min(MAX_GAIN_DB);
    db_to_linear(capped_db)
}
