//! Playback Engine Abstraction
//!
//! Unified interface for different playback backends:
//! - Rodio (PipeWire, Pulse, ALSA via CPAL) - uses rodio::Sink
//! - ALSA Direct (hw: devices) - bypasses rodio, writes directly to ALSA PCM
//!
//! This abstraction allows the player to work with both approaches transparently.

use crate::audio::AlsaDirectStream;
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Unified playback engine
pub enum PlaybackEngine {
    /// Rodio-based (PipeWire, Pulse, ALSA via CPAL)
    Rodio {
        sink: Sink,
    },
    /// Direct ALSA (hw: devices, bit-perfect)
    AlsaDirect {
        stream: Arc<AlsaDirectStream>,
        is_playing: Arc<AtomicBool>,
        position_frames: Arc<AtomicU64>,
        duration_frames: Arc<AtomicU64>,
        playback_thread: Option<thread::JoinHandle<()>>,
    },
}

impl PlaybackEngine {
    /// Create Rodio engine
    pub fn new_rodio(stream_handle: &OutputStreamHandle) -> Result<Self, String> {
        let sink = Sink::try_new(stream_handle)
            .map_err(|e| format!("Failed to create Sink: {}", e))?;

        Ok(Self::Rodio { sink })
    }

    /// Create ALSA Direct engine
    pub fn new_alsa_direct(stream: AlsaDirectStream) -> Self {
        Self::AlsaDirect {
            stream: Arc::new(stream),
            is_playing: Arc::new(AtomicBool::new(false)),
            position_frames: Arc::new(AtomicU64::new(0)),
            duration_frames: Arc::new(AtomicU64::new(0)),
            playback_thread: None,
        }
    }

    /// Append audio source
    pub fn append<S>(&mut self, source: S) -> Result<(), String>
    where
        S: Source<Item = i16> + Send + 'static,
    {
        match self {
            Self::Rodio { sink } => {
                sink.append(source);
                Ok(())
            }
            Self::AlsaDirect {
                stream,
                is_playing,
                position_frames,
                duration_frames,
                playback_thread,
            } => {
                // For ALSA Direct, we need to spawn a thread that:
                // 1. Converts i16 samples to f32
                // 2. Writes to ALSA PCM
                // 3. Tracks position

                let stream_clone = stream.clone();
                let is_playing_clone = is_playing.clone();
                let position_clone = position_frames.clone();
                let duration_clone = duration_frames.clone();

                let sample_rate = stream.sample_rate();
                let channels = stream.channels();

                is_playing.store(true, Ordering::SeqCst);
                position_clone.store(0, Ordering::SeqCst);

                let handle = thread::spawn(move || {
                    // Collect all samples from source
                    let samples_i16: Vec<i16> = source.collect();

                    // Convert i16 to f32 (normalize to -1.0..1.0)
                    let samples_f32: Vec<f32> = samples_i16
                        .iter()
                        .map(|&s| s as f32 / 32768.0)
                        .collect();

                    // Store duration in frames
                    let total_frames = samples_f32.len() as u64 / channels as u64;
                    duration_clone.store(total_frames, Ordering::SeqCst);

                    // Write in chunks to allow for control (pause/stop)
                    const CHUNK_SIZE: usize = 4096; // frames
                    let chunk_samples = CHUNK_SIZE * channels as usize;

                    let mut offset = 0;
                    while offset < samples_f32.len() && is_playing_clone.load(Ordering::SeqCst) {
                        let end = (offset + chunk_samples).min(samples_f32.len());
                        let chunk = &samples_f32[offset..end];

                        if let Err(e) = stream_clone.write(chunk) {
                            log::error!("[ALSA Direct Engine] Write failed: {}", e);
                            break;
                        }

                        // Update position
                        let frames_written = (end - offset) / channels as usize;
                        position_clone.fetch_add(frames_written as u64, Ordering::SeqCst);

                        offset = end;
                    }

                    // Drain PCM buffer
                    if let Err(e) = stream_clone.drain() {
                        log::warn!("[ALSA Direct Engine] Drain failed: {}", e);
                    }

                    is_playing_clone.store(false, Ordering::SeqCst);
                    log::info!("[ALSA Direct Engine] Playback thread finished");
                });

                *playback_thread = Some(handle);
                Ok(())
            }
        }
    }

    /// Play (unpause)
    pub fn play(&self) {
        match self {
            Self::Rodio { sink } => sink.play(),
            Self::AlsaDirect { is_playing, .. } => {
                is_playing.store(true, Ordering::SeqCst);
            }
        }
    }

    /// Pause
    pub fn pause(&self) {
        match self {
            Self::Rodio { sink } => sink.pause(),
            Self::AlsaDirect { is_playing, .. } => {
                is_playing.store(false, Ordering::SeqCst);
            }
        }
    }

    /// Stop
    pub fn stop(self) {
        match self {
            Self::Rodio { sink } => {
                sink.stop();
            }
            Self::AlsaDirect {
                stream,
                is_playing,
                playback_thread,
                ..
            } => {
                is_playing.store(false, Ordering::SeqCst);

                // Wait for playback thread to finish
                if let Some(handle) = playback_thread {
                    let _ = handle.join();
                }

                // Stop PCM
                if let Err(e) = stream.stop() {
                    log::warn!("[ALSA Direct Engine] Stop failed: {}", e);
                }
            }
        }
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume(&self, volume: f32) {
        match self {
            Self::Rodio { sink } => sink.set_volume(volume),
            Self::AlsaDirect { .. } => {
                // TODO: Implement software volume or ALSA mixer control
                // For now, ALSA Direct uses hardware volume
                log::warn!("[ALSA Direct Engine] Software volume not yet implemented");
            }
        }
    }

    /// Check if playback queue is empty
    pub fn empty(&self) -> bool {
        match self {
            Self::Rodio { sink } => sink.empty(),
            Self::AlsaDirect {
                is_playing,
                position_frames,
                duration_frames,
                ..
            } => {
                if !is_playing.load(Ordering::SeqCst) {
                    let pos = position_frames.load(Ordering::SeqCst);
                    let dur = duration_frames.load(Ordering::SeqCst);
                    // Consider empty if stopped and reached the end
                    pos >= dur && dur > 0
                } else {
                    false
                }
            }
        }
    }

    /// Get current position in seconds (for ALSA Direct only)
    pub fn position_secs(&self) -> Option<u64> {
        match self {
            Self::Rodio { .. } => None, // Rodio doesn't expose position directly
            Self::AlsaDirect {
                position_frames,
                stream,
                ..
            } => {
                let frames = position_frames.load(Ordering::SeqCst);
                let sample_rate = stream.sample_rate() as u64;
                Some(frames / sample_rate)
            }
        }
    }

    /// Get duration in seconds (for ALSA Direct only)
    pub fn duration_secs(&self) -> Option<u64> {
        match self {
            Self::Rodio { .. } => None,
            Self::AlsaDirect {
                duration_frames,
                stream,
                ..
            } => {
                let frames = duration_frames.load(Ordering::SeqCst);
                let sample_rate = stream.sample_rate() as u64;
                Some(frames / sample_rate)
            }
        }
    }

    /// Check if using ALSA Direct engine
    pub fn is_alsa_direct(&self) -> bool {
        matches!(self, Self::AlsaDirect { .. })
    }
}
