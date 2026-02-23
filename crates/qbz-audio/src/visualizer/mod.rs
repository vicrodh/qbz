//! Audio Visualizer Module
//!
//! Provides real-time FFT analysis for audio visualization without affecting bit-perfect playback.
//! Uses a lockless ring buffer to capture samples from the audio thread.
//!
//! This module contains the core types needed by the player:
//! - RingBuffer: Lockless ring buffer for sample capture
//! - TappedSource: Audio source wrapper that taps samples
//! - VisualizerTap: Shared state for visualization
//!
//! The Tauri-specific FFT thread and event emission remain in qbz-nix.

mod ring_buffer;
mod tapped_source;

pub use ring_buffer::RingBuffer;
pub use tapped_source::TappedSource;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Number of frequency bins to send to frontend
pub const NUM_BARS: usize = 16;

/// FFT size (must be power of 2)
pub const FFT_SIZE: usize = 1024;

/// Target frames per second for visualization updates
pub const TARGET_FPS: u64 = 30;

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

    /// Enable or disable visualization
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Update the sample rate
    pub fn set_sample_rate(&self, rate: u32) {
        self.sample_rate.store(rate, Ordering::Relaxed);
    }
}

impl Default for VisualizerTap {
    fn default() -> Self {
        Self::new()
    }
}
