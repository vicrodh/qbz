//! Playback Engine Abstraction
//!
//! Unified interface for different playback backends:
//! - Rodio (PipeWire, Pulse, ALSA via CPAL) - uses rodio::Sink
//! - ALSA Direct (hw: devices) - bypasses rodio, writes directly to ALSA PCM
//!
//! ALSA Direct uses a single long-lived writer thread with a source queue
//! to enable gapless playback. When one source ends, the next is picked up
//! seamlessly without interrupting the PCM stream.

#[cfg(target_os = "linux")]
use qbz_audio::AlsaDirectStream;
#[cfg(target_os = "freebsd")]
use qbz_audio::{DirectAudioStream, OssDirectStream};
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
    /// Direct ALSA (hw: devices, bit-perfect) with gapless source queue
    #[cfg(target_os = "linux")]
    AlsaDirect {
        stream: Arc<AlsaDirectStream>,
        is_playing: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        position_frames: Arc<AtomicU64>,
        duration_frames: Arc<AtomicU64>,
        source_queue: Arc<SourceQueue>,
        playback_thread: Option<thread::JoinHandle<()>>,
        source_transition: Arc<AtomicBool>,
        hardware_volume: bool,
    },
    /// Direct OSS (/dev/dspX, bit-perfect) with gapless source queue
    #[cfg(target_os = "freebsd")]
    OssDirect {
        stream: Arc<OssDirectStream>,
        is_playing: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        position_frames: Arc<AtomicU64>,
        duration_frames: Arc<AtomicU64>,
        source_queue: Arc<SourceQueue>,
        playback_thread: Option<thread::JoinHandle<()>>,
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

    /// Create ALSA Direct engine with gapless source queue.
    /// Spawns a single writer thread that lives for the engine's lifetime.
    #[cfg(target_os = "linux")]
    pub fn new_alsa_direct(stream: Arc<AlsaDirectStream>, hardware_volume: bool) -> Self {
        let is_playing = Arc::new(AtomicBool::new(false));
        let should_stop = Arc::new(AtomicBool::new(false));
        let position_frames = Arc::new(AtomicU64::new(0));
        let duration_frames = Arc::new(AtomicU64::new(0));
        let source_queue = Arc::new(SourceQueue::new());
        let source_transition = Arc::new(AtomicBool::new(false));

        // Spawn the single long-lived writer thread
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
                alsa_writer_thread(
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

        Self::AlsaDirect {
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

    /// Create OSS Direct engine with gapless source queue (FreeBSD).
    #[cfg(target_os = "freebsd")]
    pub fn new_oss_direct(stream: Arc<OssDirectStream>, hardware_volume: bool) -> Self {
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
                oss_writer_thread(
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

        Self::OssDirect {
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
            #[cfg(target_os = "linux")]
            Self::AlsaDirect {
                is_playing,
                should_stop,
                position_frames,
                source_queue,
                source_transition,
                ..
            } => {
                let is_first = source_queue.is_empty() && !is_playing.load(Ordering::SeqCst);
                let boxed: BoxedSampleIter = Box::new(source.into_iter());
                source_queue.push(boxed);

                if is_first {
                    position_frames.store(0, Ordering::SeqCst);
                    should_stop.store(false, Ordering::SeqCst);
                    source_transition.store(false, Ordering::SeqCst);
                    is_playing.store(true, Ordering::SeqCst);
                    log::info!("[ALSA Direct Engine] First source queued, playback starting");
                } else {
                    log::info!("[ALSA Direct Engine] Source queued for gapless transition");
                }

                Ok(())
            }
            #[cfg(target_os = "freebsd")]
            Self::OssDirect {
                is_playing,
                should_stop,
                position_frames,
                source_queue,
                source_transition,
                ..
            } => {
                let is_first = source_queue.is_empty() && !is_playing.load(Ordering::SeqCst);
                let boxed: BoxedSampleIter = Box::new(source.into_iter());
                source_queue.push(boxed);

                if is_first {
                    position_frames.store(0, Ordering::SeqCst);
                    should_stop.store(false, Ordering::SeqCst);
                    source_transition.store(false, Ordering::SeqCst);
                    is_playing.store(true, Ordering::SeqCst);
                    log::info!("[OSS Direct Engine] First source queued, playback starting");
                } else {
                    log::info!("[OSS Direct Engine] Source queued for gapless transition");
                }

                Ok(())
            }
        }
    }

    /// Play (unpause)
    pub fn play(&self) {
        match self {
            Self::Rodio { sink } => sink.play(),
            #[cfg(target_os = "linux")]
            Self::AlsaDirect { is_playing, .. } => {
                log::info!("[ALSA Direct Engine] Resume requested");
                is_playing.store(true, Ordering::SeqCst);
            }
            #[cfg(target_os = "freebsd")]
            Self::OssDirect { is_playing, .. } => {
                log::info!("[OSS Direct Engine] Resume requested");
                is_playing.store(true, Ordering::SeqCst);
            }
        }
    }

    /// Pause
    pub fn pause(&self) {
        match self {
            Self::Rodio { sink } => sink.pause(),
            #[cfg(target_os = "linux")]
            Self::AlsaDirect { is_playing, .. } => {
                log::info!("[ALSA Direct Engine] Pause requested");
                is_playing.store(false, Ordering::SeqCst);
            }
            #[cfg(target_os = "freebsd")]
            Self::OssDirect { is_playing, .. } => {
                log::info!("[OSS Direct Engine] Pause requested");
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
            #[cfg(target_os = "linux")]
            Self::AlsaDirect {
                stream,
                is_playing,
                should_stop,
                playback_thread,
                ..
            } => {
                if should_stop.load(Ordering::SeqCst) {
                    return;
                }
                log::info!("[ALSA Direct Engine] Stop requested");
                should_stop.store(true, Ordering::SeqCst);
                is_playing.store(false, Ordering::SeqCst);

                if let Some(handle) = playback_thread.take() {
                    let _ = handle.join();
                }

                if let Err(e) = stream.stop() {
                    log::warn!("[ALSA Direct Engine] Stop failed: {}", e);
                }
            }
            #[cfg(target_os = "freebsd")]
            Self::OssDirect {
                stream,
                is_playing,
                should_stop,
                playback_thread,
                ..
            } => {
                if should_stop.load(Ordering::SeqCst) {
                    return;
                }
                log::info!("[OSS Direct Engine] Stop requested");
                should_stop.store(true, Ordering::SeqCst);
                is_playing.store(false, Ordering::SeqCst);

                if let Some(handle) = playback_thread.take() {
                    let _ = handle.join();
                }

                if let Err(e) = stream.stop() {
                    log::warn!("[OSS Direct Engine] Stop failed: {}", e);
                }
            }
        }
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume(&self, volume: f32) {
        match self {
            Self::Rodio { sink } => sink.set_volume(volume),
            #[cfg(target_os = "linux")]
            Self::AlsaDirect {
                stream,
                hardware_volume,
                ..
            } => {
                if *hardware_volume {
                    if let Err(e) = stream.set_hardware_volume(volume) {
                        log::warn!("[ALSA Direct Engine] Hardware volume failed: {}", e);
                    }
                }
            }
            #[cfg(target_os = "freebsd")]
            Self::OssDirect { .. } => {
                log::debug!("[OSS Direct Engine] Hardware volume not supported");
            }
        }
    }

    /// Check if playback queue is empty (all sources consumed, not playing)
    pub fn empty(&self) -> bool {
        match self {
            Self::Rodio { sink } => sink.empty(),
            #[cfg(target_os = "linux")]
            Self::AlsaDirect {
                is_playing,
                source_queue,
                ..
            } => !is_playing.load(Ordering::SeqCst) && source_queue.is_empty(),
            #[cfg(target_os = "freebsd")]
            Self::OssDirect {
                is_playing,
                source_queue,
                ..
            } => !is_playing.load(Ordering::SeqCst) && source_queue.is_empty(),
        }
    }

    /// Check if a gapless source transition just happened.
    pub fn take_source_transition(&self) -> bool {
        match self {
            Self::Rodio { .. } => false,
            #[cfg(target_os = "linux")]
            Self::AlsaDirect {
                source_transition, ..
            } => source_transition
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
            #[cfg(target_os = "freebsd")]
            Self::OssDirect {
                source_transition, ..
            } => source_transition
                .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
        }
    }

    /// Get current position in seconds (direct engine only)
    #[allow(dead_code)]
    pub fn position_secs(&self) -> Option<u64> {
        match self {
            Self::Rodio { .. } => None,
            #[cfg(target_os = "linux")]
            Self::AlsaDirect {
                position_frames,
                stream,
                ..
            } => {
                let frames = position_frames.load(Ordering::SeqCst);
                Some(frames / stream.sample_rate() as u64)
            }
            #[cfg(target_os = "freebsd")]
            Self::OssDirect {
                position_frames,
                stream,
                ..
            } => {
                let frames = position_frames.load(Ordering::SeqCst);
                Some(frames / stream.sample_rate() as u64)
            }
        }
    }

    /// Get duration in seconds (direct engine only)
    #[allow(dead_code)]
    pub fn duration_secs(&self) -> Option<u64> {
        match self {
            Self::Rodio { .. } => None,
            #[cfg(target_os = "linux")]
            Self::AlsaDirect {
                duration_frames,
                stream,
                ..
            } => {
                let frames = duration_frames.load(Ordering::SeqCst);
                Some(frames / stream.sample_rate() as u64)
            }
            #[cfg(target_os = "freebsd")]
            Self::OssDirect {
                duration_frames,
                stream,
                ..
            } => {
                let frames = duration_frames.load(Ordering::SeqCst);
                Some(frames / stream.sample_rate() as u64)
            }
        }
    }

    /// Check if using a direct hardware engine (ALSA or OSS)
    #[allow(dead_code)]
    pub fn is_direct(&self) -> bool {
        !matches!(self, Self::Rodio { .. })
    }
}

/// Single long-lived writer thread for ALSA Direct.
#[cfg(target_os = "linux")]
fn alsa_writer_thread(
    stream: Arc<AlsaDirectStream>,
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

    log::info!("[ALSA Direct Engine] Writer thread started (gapless-capable)");

    'thread: loop {
        // Check global stop
        if should_stop.load(Ordering::SeqCst) {
            log::info!("[ALSA Direct Engine] Stop signal, writer thread exiting");
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
                    log::info!("[ALSA Direct Engine] Acquired new source from queue");
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
                log::error!("[ALSA Direct Engine] Write failed: {}", e);
                break 'thread;
            }

            let frames_written = buffer_f32.len() / channels as usize;
            total_frames += frames_written as u64;
            position_frames.store(total_frames, Ordering::SeqCst);
            duration_frames.store(total_frames, Ordering::SeqCst);
        }

        if source_ended {
            log::info!(
                "[ALSA Direct Engine] Source ended (total frames: {})",
                total_frames
            );

            // Try to get next source immediately (gapless transition)
            match source_queue.try_pop() {
                Some(next_src) => {
                    log::info!("[ALSA Direct Engine] Gapless transition to next source");
                    current_source = Some(next_src);
                    total_frames = 0;
                    position_frames.store(0, Ordering::SeqCst);
                    // Signal that a transition happened
                    source_transition.store(true, Ordering::SeqCst);
                    // Continue immediately — no drain, no gap
                }
                None => {
                    // No next source — this is a natural end of playback
                    log::info!("[ALSA Direct Engine] No next source, draining ALSA buffer");
                    if let Err(e) = stream.drain() {
                        log::warn!("[ALSA Direct Engine] Drain failed: {}", e);
                    }
                    current_source = None;
                    is_playing.store(false, Ordering::SeqCst);
                    // Don't break — stay alive waiting for next append()
                }
            }
        }
    }

    is_playing.store(false, Ordering::SeqCst);
    log::info!("[ALSA Direct Engine] Writer thread finished");
}

/// Single long-lived writer thread for OSS Direct (FreeBSD).
/// Identical logic to alsa_writer_thread but uses OssDirectStream.
#[cfg(target_os = "freebsd")]
fn oss_writer_thread(
    stream: Arc<OssDirectStream>,
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

    log::info!("[OSS Direct Engine] Writer thread started (gapless-capable)");

    'thread: loop {
        if should_stop.load(Ordering::SeqCst) {
            log::info!("[OSS Direct Engine] Stop signal, writer thread exiting");
            break 'thread;
        }

        if current_source.is_none() {
            match source_queue.wait_for_source(Duration::from_millis(100)) {
                Some(src) => {
                    current_source = Some(src);
                    total_frames = 0;
                    position_frames.store(0, Ordering::SeqCst);
                    log::info!("[OSS Direct Engine] Acquired new source from queue");
                }
                None => continue 'thread,
            }
        }

        while !is_playing.load(Ordering::SeqCst) {
            if should_stop.load(Ordering::SeqCst) {
                break 'thread;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

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

        if !buffer_f32.is_empty() {
            if let Err(e) = stream.write_f32(&buffer_f32) {
                log::error!("[OSS Direct Engine] Write failed: {}", e);
                break 'thread;
            }

            let frames_written = buffer_f32.len() / channels as usize;
            total_frames += frames_written as u64;
            position_frames.store(total_frames, Ordering::SeqCst);
            duration_frames.store(total_frames, Ordering::SeqCst);
        }

        if source_ended {
            log::info!("[OSS Direct Engine] Source ended (total frames: {})", total_frames);

            match source_queue.try_pop() {
                Some(next_src) => {
                    log::info!("[OSS Direct Engine] Gapless transition to next source");
                    current_source = Some(next_src);
                    total_frames = 0;
                    position_frames.store(0, Ordering::SeqCst);
                    source_transition.store(true, Ordering::SeqCst);
                }
                None => {
                    log::info!("[OSS Direct Engine] No next source, draining OSS buffer");
                    if let Err(e) = stream.drain() {
                        log::warn!("[OSS Direct Engine] Drain failed: {}", e);
                    }
                    current_source = None;
                    is_playing.store(false, Ordering::SeqCst);
                }
            }
        }
    }

    is_playing.store(false, Ordering::SeqCst);
    log::info!("[OSS Direct Engine] Writer thread finished");
}

impl Drop for PlaybackEngine {
    fn drop(&mut self) {
        self.stop_inner();
    }
}
