//! Playback Engine Abstraction
//!
//! Unified interface for different playback backends:
//! - Rodio (PipeWire, Pulse, ALSA via CPAL) - uses rodio::Sink
//! - Direct (hw: on Linux, /dev/dspX on FreeBSD) - bypasses rodio entirely
//!
//! The Direct variant uses a single long-lived writer thread with a source
//! queue to enable gapless playback.  When one source ends the next is picked
//! up seamlessly without interrupting the hardware stream.

use qbz_audio::DirectAudioStream;
use rodio::{mixer::Mixer, Player as RodioPlayer, Source};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

/// A boxed sample iterator that can be sent across threads
type BoxedSampleIter = Box<dyn Iterator<Item = f32> + Send>;

/// Thread-safe source queue for gapless playback.
/// The writer thread consumes sources; append() pushes new ones.
pub(crate) struct SourceQueue {
    queue: Mutex<VecDeque<BoxedSampleIter>>,
    /// Notifies the writer thread that a new source is available
    notify: Condvar,
}

impl SourceQueue {
    fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            notify: Condvar::new(),
        }
    }

    /// Push a new source to the back of the queue
    fn push(&self, source: BoxedSampleIter) {
        let mut q = self.queue.lock().unwrap();
        q.push_back(source);
        self.notify.notify_one();
    }

    /// Try to pop the next source (non-blocking)
    fn try_pop(&self) -> Option<BoxedSampleIter> {
        let mut q = self.queue.lock().unwrap();
        q.pop_front()
    }

    /// Wait for a source to become available (with timeout)
    /// Returns None on timeout (used to check stop/pause flags)
    fn wait_for_source(&self, timeout: Duration) -> Option<BoxedSampleIter> {
        let mut q = self.queue.lock().unwrap();
        if q.is_empty() {
            let (guard, _) = self.notify.wait_timeout(q, timeout).unwrap();
            q = guard;
        }
        q.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.queue.lock().unwrap().is_empty()
    }
}

/// Unified playback engine
pub enum PlaybackEngine {
    /// Rodio-based (PipeWire, Pulse, ALSA via CPAL)
    Rodio { sink: RodioPlayer },
    /// Direct hardware stream (AlsaDirectStream on Linux, OssDirectStream on FreeBSD).
    /// Bit-perfect, gapless via an internal source queue.
    Direct {
        stream: Arc<dyn DirectAudioStream>,
        is_playing: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        position_frames: Arc<AtomicU64>,
        duration_frames: Arc<AtomicU64>,
        source_queue: Arc<SourceQueue>,
        playback_thread: Option<thread::JoinHandle<()>>,
        /// Signals that the writer thread has consumed a source and moved to next
        source_transition: Arc<AtomicBool>,
        hardware_volume: bool,
    },
}

impl PlaybackEngine {
    /// Create Rodio engine
    pub fn new_rodio(mixer: &Mixer) -> Result<Self, String> {
        let sink = RodioPlayer::connect_new(mixer);
        Ok(Self::Rodio { sink })
    }

    /// Create a Direct hardware engine with gapless source queue.
    ///
    /// Works with any `DirectAudioStream` implementation (ALSA on Linux,
    /// OSS on FreeBSD).  Spawns a single writer thread for the engine's
    /// lifetime.
    pub fn new_direct(stream: Arc<dyn DirectAudioStream>, hardware_volume: bool) -> Self {
        let is_playing = Arc::new(AtomicBool::new(false));
        let should_stop = Arc::new(AtomicBool::new(false));
        let position_frames = Arc::new(AtomicU64::new(0));
        let duration_frames = Arc::new(AtomicU64::new(0));
        let source_queue = Arc::new(SourceQueue::new());
        let source_transition = Arc::new(AtomicBool::new(false));

        let handle = {
            let stream_c = stream.clone();
            let playing_c = is_playing.clone();
            let stop_c = should_stop.clone();
            let pos_c = position_frames.clone();
            let dur_c = duration_frames.clone();
            let queue_c = source_queue.clone();
            let transition_c = source_transition.clone();
            let channels = stream.channels();

            thread::spawn(move || {
                direct_writer_thread(
                    stream_c,
                    playing_c,
                    stop_c,
                    pos_c,
                    dur_c,
                    queue_c,
                    transition_c,
                    channels,
                );
            })
        };

        Self::Direct {
            stream,
            is_playing,
            should_stop,
            position_frames,
            duration_frames,
            source_queue,
            playback_thread: Some(handle),
            source_transition,
            hardware_volume,
        }
    }

    /// Append audio source.
    /// For ALSA Direct: pushes to the source queue for gapless transition.
    /// For Rodio: delegates to Sink's built-in queue.
    pub fn append<S>(&mut self, source: S) -> Result<(), String>
    where
        S: Source<Item = f32> + Send + 'static,
    {
        match self {
            Self::Rodio { sink } => {
                sink.append(source);
                Ok(())
            }
            Self::Direct {
                is_playing,
                should_stop,
                position_frames,
                source_queue,
                source_transition,
                ..
            } => {
                let is_first = source_queue.is_empty() && !is_playing.load(Ordering::SeqCst);

                // Box the source iterator and push to queue
                let boxed: BoxedSampleIter = Box::new(source.into_iter());
                source_queue.push(boxed);

                if is_first {
                    // First source: reset position, clear stop, start playing
                    position_frames.store(0, Ordering::SeqCst);
                    should_stop.store(false, Ordering::SeqCst);
                    source_transition.store(false, Ordering::SeqCst);
                    is_playing.store(true, Ordering::SeqCst);
                    log::info!("[Direct Engine] First source queued, playback starting");
                } else {
                    log::info!("[Direct Engine] Source queued for gapless transition");
                }

                Ok(())
            }
        }
    }

    /// Play (unpause)
    pub fn play(&self) {
        match self {
            Self::Rodio { sink } => sink.play(),
            Self::Direct { is_playing, .. } => {
                log::info!("[Direct Engine] Resume requested");
                is_playing.store(true, Ordering::SeqCst);
            }
        }
    }

    /// Pause
    pub fn pause(&self) {
        match self {
            Self::Rodio { sink } => sink.pause(),
            Self::Direct { is_playing, .. } => {
                log::info!("[Direct Engine] Pause requested");
                is_playing.store(false, Ordering::SeqCst);
            }
        }
    }

    /// Stop playback and release resources.
    /// For ALSA Direct, signals the writer thread and waits for it to exit.
    /// The Drop impl handles the same cleanup if stop() is not called explicitly.
    pub fn stop(mut self) {
        self.stop_inner();
    }

    /// Internal stop logic shared by stop() and Drop
    fn stop_inner(&mut self) {
        match self {
            Self::Rodio { sink } => {
                sink.stop();
            }
            Self::Direct {
                stream,
                is_playing,
                should_stop,
                playback_thread,
                ..
            } => {
                if should_stop.load(Ordering::SeqCst) {
                    return; // Already stopped
                }
                log::info!("[Direct Engine] Stop requested");
                should_stop.store(true, Ordering::SeqCst);
                is_playing.store(false, Ordering::SeqCst);

                if let Some(handle) = playback_thread.take() {
                    let _ = handle.join();
                }

                if let Err(e) = stream.stop() {
                    log::warn!("[Direct Engine] Stop failed: {}", e);
                }
            }
        }
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume(&self, volume: f32) {
        match self {
            Self::Rodio { sink } => sink.set_volume(volume),
            Self::Direct {
                stream,
                hardware_volume,
                ..
            } => {
                if *hardware_volume {
                    if let Err(e) = stream.set_hardware_volume(volume) {
                        log::warn!("[Direct Engine] Hardware volume failed: {}", e);
                    }
                } else {
                    log::debug!(
                        "[Direct Engine] Hardware volume disabled (use DAC/amplifier)"
                    );
                }
            }
        }
    }

    /// Check if playback queue is empty (all sources consumed, not playing)
    pub fn empty(&self) -> bool {
        match self {
            Self::Rodio { sink } => sink.empty(),
            Self::Direct {
                is_playing,
                source_queue,
                ..
            } => !is_playing.load(Ordering::SeqCst) && source_queue.is_empty(),
        }
    }

    /// Check if a gapless source transition just happened.
    /// Returns true once, then resets the flag.
    pub fn take_source_transition(&self) -> bool {
        match self {
            Self::Rodio { .. } => false,
            Self::Direct {
                source_transition, ..
            } => source_transition
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
        }
    }

    /// Get current position in seconds (for ALSA Direct only)
    #[allow(dead_code)]
    pub fn position_secs(&self) -> Option<u64> {
        match self {
            Self::Rodio { .. } => None,
            Self::Direct {
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
            Self::Direct {
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

    /// Check if using the direct hardware engine (ALSA or OSS)
    #[allow(dead_code)]
    pub fn is_direct(&self) -> bool {
        matches!(self, Self::Direct { .. })
    }
}

/// Single long-lived writer thread for direct hardware streams.
///
/// Continuously reads samples from the current source and writes to the
/// device.  When a source ends, seamlessly picks up the next one from the
/// queue (gapless transition).  If no next source is available, drains the
/// hardware buffer and waits for the next source or a stop signal.
fn direct_writer_thread(
    stream: Arc<dyn DirectAudioStream>,
    is_playing: Arc<AtomicBool>,
    should_stop: Arc<AtomicBool>,
    position_frames: Arc<AtomicU64>,
    duration_frames: Arc<AtomicU64>,
    source_queue: Arc<SourceQueue>,
    source_transition: Arc<AtomicBool>,
    channels: u16,
) {
    const CHUNK_FRAMES: usize = 8192;
    let chunk_samples = CHUNK_FRAMES * channels as usize;
    let mut buffer_f32 = Vec::with_capacity(chunk_samples);
    let mut current_source: Option<BoxedSampleIter> = None;
    let mut total_frames: u64 = 0;

    log::info!("[Direct Engine] Writer thread started (gapless-capable)");

    'thread: loop {
        // Check global stop
        if should_stop.load(Ordering::SeqCst) {
            log::info!("[Direct Engine] Stop signal, writer thread exiting");
            break 'thread;
        }

        // If no current source, try to get one
        if current_source.is_none() {
            // Wait for a source (with 100ms timeout to recheck stop flag)
            match source_queue.wait_for_source(Duration::from_millis(100)) {
                Some(src) => {
                    current_source = Some(src);
                    total_frames = 0;
                    position_frames.store(0, Ordering::SeqCst);
                    log::info!("[Direct Engine] Acquired new source from queue");
                }
                None => {
                    // No source available, loop back to check stop
                    continue 'thread;
                }
            }
        }

        // Wait while paused
        while !is_playing.load(Ordering::SeqCst) {
            if should_stop.load(Ordering::SeqCst) {
                break 'thread;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        // Fill buffer from current source
        buffer_f32.clear();
        let source = current_source.as_mut().unwrap();
        let mut source_ended = false;

        for _ in 0..chunk_samples {
            match source.next() {
                Some(sample) => buffer_f32.push(sample),
                None => {
                    source_ended = true;
                    break;
                }
            }
        }

        // Write whatever we have to ALSA (even partial chunks on source end)
        if !buffer_f32.is_empty() {
            if let Err(e) = stream.write_f32(&buffer_f32) {
                log::error!("[Direct Engine] Write failed: {}", e);
                break 'thread;
            }

            let frames_written = buffer_f32.len() / channels as usize;
            total_frames += frames_written as u64;
            position_frames.store(total_frames, Ordering::SeqCst);
            duration_frames.store(total_frames, Ordering::SeqCst);
        }

        if source_ended {
            log::info!(
                "[Direct Engine] Source ended (total frames: {})",
                total_frames
            );

            // Try to get next source immediately (gapless transition)
            match source_queue.try_pop() {
                Some(next_src) => {
                    log::info!("[Direct Engine] Gapless transition to next source");
                    current_source = Some(next_src);
                    total_frames = 0;
                    position_frames.store(0, Ordering::SeqCst);
                    // Signal that a transition happened
                    source_transition.store(true, Ordering::SeqCst);
                    // Continue immediately — no drain, no gap
                }
                None => {
                    // No next source — this is a natural end of playback
                    log::info!("[Direct Engine] No next source, draining ALSA buffer");
                    if let Err(e) = stream.drain() {
                        log::warn!("[Direct Engine] Drain failed: {}", e);
                    }
                    current_source = None;
                    is_playing.store(false, Ordering::SeqCst);
                    // Don't break — stay alive waiting for next append()
                }
            }
        }
    }

    is_playing.store(false, Ordering::SeqCst);
    log::info!("[Direct Engine] Writer thread finished");
}

impl Drop for PlaybackEngine {
    fn drop(&mut self) {
        self.stop_inner();
    }
}
