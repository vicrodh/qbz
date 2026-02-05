//! Playback Engine Abstraction
//!
//! Unified interface for different playback backends:
//! - Rodio (PipeWire, Pulse, ALSA via CPAL) - uses rodio::Sink
//! - ALSA Direct (hw: devices) - bypasses rodio, writes directly to ALSA PCM
//!
//! This abstraction allows the player to work with both approaches transparently.

use crate::audio::AlsaDirectStream;
use rodio::{OutputStreamHandle, Sink, Source};
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
        should_stop: Arc<AtomicBool>,  // Separate flag for full stop vs pause
        position_frames: Arc<AtomicU64>,
        duration_frames: Arc<AtomicU64>,
        playback_thread: Option<thread::JoinHandle<()>>,
        hardware_volume: bool,  // Use ALSA mixer for volume control
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
    pub fn new_alsa_direct(stream: Arc<AlsaDirectStream>, hardware_volume: bool) -> Self {
        Self::AlsaDirect {
            stream,
            is_playing: Arc::new(AtomicBool::new(false)),
            should_stop: Arc::new(AtomicBool::new(false)),
            position_frames: Arc::new(AtomicU64::new(0)),
            duration_frames: Arc::new(AtomicU64::new(0)),
            playback_thread: None,
            hardware_volume,
        }
    }

    /// Append audio source
    pub fn append<S>(&mut self, source: S) -> Result<(), String>
    where
        S: Source<Item = f32> + Send + 'static,
    {
        match self {
            Self::Rodio { sink } => {
                sink.append(source);
                Ok(())
            }
            Self::AlsaDirect {
                stream,
                is_playing,
                should_stop,
                position_frames,
                duration_frames,
                playback_thread,
                hardware_volume: _,
            } => {
                // For ALSA Direct, we need to spawn a thread that:
                // 1. Streams f32 samples from source (no buffering entire file)
                // 2. Writes to ALSA PCM (converts f32 to hardware format)
                // 3. Tracks position
                // 4. Supports pause/resume without terminating

                let stream_clone = stream.clone();
                let is_playing_clone = is_playing.clone();
                let should_stop_clone = should_stop.clone();
                let position_clone = position_frames.clone();
                let duration_clone = duration_frames.clone();

                let channels = stream.channels();

                is_playing.store(true, Ordering::SeqCst);
                should_stop.store(false, Ordering::SeqCst);
                position_clone.store(0, Ordering::SeqCst);

                log::info!("[ALSA Direct Engine] Starting streaming playback thread");

                let handle = thread::spawn(move || {
                    // Stream samples in chunks (no pre-buffering entire file)
                    const CHUNK_SIZE: usize = 8192; // frames per chunk
                    let chunk_samples = CHUNK_SIZE * channels as usize;

                    let mut buffer_f32 = Vec::with_capacity(chunk_samples);

                    let mut total_frames: u64 = 0;
                    let mut source_iter = source.into_iter();
                    let mut natural_end = false;

                    'playback: loop {
                        // Check if we should stop completely (not just pause)
                        if should_stop_clone.load(Ordering::SeqCst) {
                            log::info!("[ALSA Direct Engine] Stop requested, terminating thread");
                            break 'playback;
                        }

                        // Check if paused - wait instead of terminating
                        while !is_playing_clone.load(Ordering::SeqCst) {
                            // Still check for stop while paused
                            if should_stop_clone.load(Ordering::SeqCst) {
                                log::info!("[ALSA Direct Engine] Stop requested while paused");
                                break 'playback;
                            }
                            // Sleep briefly to avoid busy-waiting
                            std::thread::sleep(Duration::from_millis(50));
                        }

                        // Fill buffer from source
                        buffer_f32.clear();
                        for _ in 0..chunk_samples {
                            match source_iter.next() {
                                Some(sample) => buffer_f32.push(sample),
                                None => break, // End of stream
                            }
                        }

                        if buffer_f32.is_empty() {
                            // End of stream
                            log::info!("[ALSA Direct Engine] Stream ended (total frames: {})", total_frames);
                            natural_end = true;
                            break 'playback;
                        }

                        // Write to ALSA (auto-converts based on detected format)
                        // This is bit-perfect: no resampling, no mixing, direct to hardware
                        if let Err(e) = stream_clone.write_f32(&buffer_f32) {
                            log::error!("[ALSA Direct Engine] Write failed: {}", e);
                            break 'playback;
                        }

                        // Update position
                        let frames_written = buffer_f32.len() / channels as usize;
                        total_frames += frames_written as u64;
                        position_clone.store(total_frames, Ordering::SeqCst);
                        duration_clone.store(total_frames, Ordering::SeqCst);
                    }

                    // Only drain if song ended naturally (not skipped/stopped)
                    // This prevents 2-5s delay when rapidly changing tracks
                    if natural_end {
                        log::info!("[ALSA Direct Engine] Song ended naturally, draining buffer");
                        if let Err(e) = stream_clone.drain() {
                            log::warn!("[ALSA Direct Engine] Drain failed: {}", e);
                        }
                    } else {
                        log::info!("[ALSA Direct Engine] Playback interrupted, skipping drain for faster response");
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
                log::info!("[ALSA Direct Engine] Resume requested");
                is_playing.store(true, Ordering::SeqCst);
            }
        }
    }

    /// Pause
    pub fn pause(&self) {
        match self {
            Self::Rodio { sink } => sink.pause(),
            Self::AlsaDirect { is_playing, .. } => {
                log::info!("[ALSA Direct Engine] Pause requested");
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
                should_stop,
                playback_thread,
                ..
            } => {
                log::info!("[ALSA Direct Engine] Stop requested");
                // Signal thread to stop completely
                should_stop.store(true, Ordering::SeqCst);
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
            Self::AlsaDirect { stream, hardware_volume, .. } => {
                if *hardware_volume {
                    // Try hardware mixer control
                    #[cfg(target_os = "linux")]
                    {
                        if let Err(e) = stream.set_hardware_volume(volume) {
                            log::warn!("[ALSA Direct Engine] Hardware volume failed: {}. Volume slider may not work.", e);
                        }
                    }
                } else {
                    // Hardware volume disabled - volume control is handled by DAC/amplifier
                    log::debug!("[ALSA Direct Engine] Hardware volume control disabled (use DAC/amplifier)");
                }
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub fn is_alsa_direct(&self) -> bool {
        matches!(self, Self::AlsaDirect { .. })
    }
}
