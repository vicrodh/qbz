//! Audio Visualizer Module
//!
//! Provides real-time FFT analysis for audio visualization without affecting bit-perfect playback.
//! Uses a lockless ring buffer to capture samples from the audio thread and processes them
//! on a dedicated thread.

mod ring_buffer;
mod fft_processor;
mod tapped_source;

pub use ring_buffer::RingBuffer;
pub use fft_processor::{VisualizerState, start_visualizer_thread};
pub use tapped_source::TappedSource;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tauri::AppHandle;

/// Number of frequency bins to send to frontend
pub const NUM_BARS: usize = 64;

/// FFT size (must be power of 2)
/// 2048 gives good frequency resolution at ~21Hz per bin at 44.1kHz
pub const FFT_SIZE: usize = 2048;

/// Target frames per second for visualization updates
pub const TARGET_FPS: u64 = 60;

/// Shared state for visualization that can be passed to the audio thread
#[derive(Clone)]
pub struct VisualizerTap {
    /// Ring buffer for sample capture
    pub ring_buffer: Arc<RingBuffer>,
    /// Whether visualization is enabled
    pub enabled: Arc<AtomicBool>,
    /// Current sample rate
    pub sample_rate: Arc<AtomicU32>,
}

impl VisualizerTap {
    /// Create a new tap
    pub fn new() -> Self {
        Self {
            ring_buffer: Arc::new(RingBuffer::new(FFT_SIZE * 2)),
            enabled: Arc::new(AtomicBool::new(false)),
            sample_rate: Arc::new(AtomicU32::new(44100)),
        }
    }

    /// Check if visualization is enabled (fast atomic check)
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Push a sample (only if enabled)
    #[inline]
    pub fn push(&self, sample: f32) {
        if self.is_enabled() {
            self.ring_buffer.push(sample);
        }
    }
}

impl Default for VisualizerTap {
    fn default() -> Self {
        Self::new()
    }
}

/// Manages the audio visualizer lifecycle
pub struct Visualizer {
    /// Shared tap state (given to Player for sample capture)
    tap: VisualizerTap,
}

impl Visualizer {
    /// Create a new visualizer instance
    pub fn new() -> Self {
        Self {
            tap: VisualizerTap::new(),
        }
    }

    /// Get the tap to give to the Player
    pub fn get_tap(&self) -> VisualizerTap {
        self.tap.clone()
    }

    /// Start the FFT processing thread
    pub fn start(&self, app_handle: AppHandle) {
        let state = VisualizerState {
            ring_buffer: self.tap.ring_buffer.clone(),
            enabled: self.tap.enabled.clone(),
            sample_rate: self.tap.sample_rate.clone(),
        };
        start_visualizer_thread(state, app_handle);
    }

    /// Enable or disable visualization
    pub fn set_enabled(&self, enabled: bool) {
        self.tap.enabled.store(enabled, Ordering::Relaxed);
        log::info!("Visualizer {}", if enabled { "enabled" } else { "disabled" });
    }

    /// Check if visualization is enabled
    pub fn is_enabled(&self) -> bool {
        self.tap.enabled.load(Ordering::Relaxed)
    }

    /// Update the sample rate (call when audio format changes)
    pub fn set_sample_rate(&self, rate: u32) {
        self.tap.sample_rate.store(rate, Ordering::Relaxed);
    }
}

impl Default for Visualizer {
    fn default() -> Self {
        Self::new()
    }
}
