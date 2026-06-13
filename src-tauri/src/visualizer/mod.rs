//! Audio Visualizer Module
//!
//! Provides real-time FFT analysis for audio visualization without affecting bit-perfect playback.
//! Uses a lockless ring buffer to capture samples from the audio thread and processes them
//! on a dedicated thread.

mod fft_processor;
use fft_processor::TauriVizSink;
pub use qbz_audio::{RingBuffer, TappedSource, VisualizerTap};

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;

/// Number of frequency bins to send to frontend
/// 16 bins, mirrored on frontend for symmetric look
pub const NUM_BARS: usize = 16;

/// FFT size (must be power of 2)
pub const FFT_SIZE: usize = qbz_audio::visualizer::FFT_SIZE;

/// Target frames per second for visualization updates
pub const TARGET_FPS: u64 = 30;

/// Manages the audio visualizer lifecycle
pub struct Visualizer {
    /// Shared tap state (given to Player for sample capture)
    tap: VisualizerTap,
    /// Whether the FFT thread has been started (prevents double-start)
    started: AtomicBool,
}

impl Visualizer {
    /// Create a new visualizer instance
    pub fn new() -> Self {
        Self {
            tap: VisualizerTap::new(),
            started: AtomicBool::new(false),
        }
    }

    /// Get the tap to give to the Player
    pub fn get_tap(&self) -> VisualizerTap {
        self.tap.clone()
    }

    /// Start the FFT processing thread (idempotent — only starts once)
    pub fn start(&self, app_handle: AppHandle) {
        if self.started.swap(true, Ordering::SeqCst) {
            log::debug!("Visualizer FFT thread already started, skipping");
            return;
        }
        let sink = std::sync::Arc::new(TauriVizSink::new(app_handle));
        qbz_audio::visualizer::spawn_visualizer_thread(self.tap.clone(), sink);
        log::info!("Visualizer FFT thread started");
    }

    /// Enable or disable visualization
    pub fn set_enabled(&self, enabled: bool) {
        self.tap.set_enabled(enabled);
        log::info!(
            "Visualizer {}",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    /// Check if visualization is enabled
    pub fn is_enabled(&self) -> bool {
        self.tap.enabled.load(Ordering::Relaxed)
    }

    /// Update the sample rate (call when audio format changes)
    pub fn set_sample_rate(&self, rate: u32) {
        self.tap.set_sample_rate(rate);
    }
}

impl Default for Visualizer {
    fn default() -> Self {
        Self::new()
    }
}
