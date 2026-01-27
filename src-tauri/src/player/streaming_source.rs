//! Buffered media source for streaming playback.
//!
//! Provides two main components:
//! 1. `BufferedMediaSource` - Wraps an async HTTP response to provide a synchronous
//!    `Read + Seek` interface required by symphonia decoders.
//! 2. `IncrementalStreamingSource` - A rodio Source that decodes audio packets
//!    incrementally as they become available, allowing playback to start before
//!    the entire file is downloaded.
//!
//! # Design
//!
//! The source uses a growing buffer that accumulates data from the HTTP response.
//! - Reads block if requesting data not yet buffered
//! - Seek forward blocks until data is available
//! - Seek backward works within buffered data
//! - Seek beyond current buffer position blocks until data arrives
//!
//! # Thread Safety
//!
//! The buffer state is shared between:
//! - The reader (audio thread, synchronous)
//! - The writer (download task, async)
//!
//! Communication uses `Mutex` + `Condvar` for blocking synchronization.

use std::collections::VecDeque;
use std::io::{Error as IoError, ErrorKind, Read, Result as IoResult, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use rodio::Source;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

/// Configuration for the streaming buffer
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// Minimum bytes to buffer before allowing reads (for format detection)
    pub initial_buffer_bytes: usize,
    /// Maximum buffer size before backpressure (not enforced, just for info)
    pub max_buffer_bytes: usize,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            // 512KB default - enough for format headers and ~2-5 seconds of audio
            // This allows playback to start quickly while still having enough
            // buffer to handle network jitter
            initial_buffer_bytes: 512 * 1024,
            // 100MB max buffer
            max_buffer_bytes: 100 * 1024 * 1024,
        }
    }
}

impl StreamingConfig {
    /// Create config from buffer seconds and approximate bitrate
    ///
    /// For Hi-Res FLAC at 192kHz/24bit stereo, bitrate is roughly 9.2 Mbps
    /// We estimate ~1MB per second as a conservative approximation
    pub fn from_seconds(seconds: u8) -> Self {
        // Minimum 256KB to ensure format detection works
        let bytes = ((seconds as usize) * 1024 * 1024).max(256 * 1024);
        Self {
            initial_buffer_bytes: bytes,
            max_buffer_bytes: 100 * 1024 * 1024,
        }
    }

    /// Create a minimal config for fastest startup
    /// Uses smallest buffer that still allows format detection (~256KB)
    pub fn fast_start() -> Self {
        Self {
            initial_buffer_bytes: 256 * 1024,
            max_buffer_bytes: 100 * 1024 * 1024,
        }
    }

    /// Create config dynamically based on measured download speed
    ///
    /// - Very fast (>10 MB/s): 256KB (instant start)
    /// - Fast (5-10 MB/s): 384KB
    /// - Normal (2-5 MB/s): 512KB
    /// - Slow (1-2 MB/s): 1MB (more buffer to prevent stutter)
    /// - Very slow (<1 MB/s): 2MB
    pub fn from_speed_mbps(speed_mbps: f64) -> Self {
        let initial_buffer = if speed_mbps >= 10.0 {
            256 * 1024 // 256KB - instant start for very fast connections
        } else if speed_mbps >= 5.0 {
            384 * 1024 // 384KB
        } else if speed_mbps >= 2.0 {
            512 * 1024 // 512KB - default
        } else if speed_mbps >= 1.0 {
            1024 * 1024 // 1MB - more buffer for slower connections
        } else {
            2 * 1024 * 1024 // 2MB - maximum buffer for very slow connections
        };

        log::info!(
            "📶 Dynamic buffer: {:.1} MB/s detected → {}KB initial buffer",
            speed_mbps,
            initial_buffer / 1024
        );

        Self {
            initial_buffer_bytes: initial_buffer,
            max_buffer_bytes: 100 * 1024 * 1024,
        }
    }
}

/// Internal state shared between reader and writer
struct BufferState {
    /// Accumulated data from HTTP response
    data: Vec<u8>,
    /// True when HTTP download is complete
    download_complete: bool,
    /// Error from download, if any
    download_error: Option<String>,
    /// Total expected size (from Content-Length), if known
    total_size: Option<u64>,
}

/// A media source that buffers from an async HTTP stream.
///
/// Provides `Read + Seek` interface for decoders while data is still downloading.
/// The source is created with a `BufferWriter` that receives chunks from the
/// download task.
pub struct BufferedMediaSource {
    state: Arc<(Mutex<BufferState>, Condvar)>,
    config: StreamingConfig,
    /// Each reader has its own read position
    read_pos: std::sync::atomic::AtomicU64,
}

impl BufferedMediaSource {
    /// Create a new buffered source.
    ///
    /// Returns the source and a writer for pushing downloaded chunks.
    /// The writer should be used from the async download task.
    pub fn new(config: StreamingConfig, total_size: Option<u64>) -> (Self, BufferWriter) {
        let state = Arc::new((
            Mutex::new(BufferState {
                data: Vec::with_capacity(config.initial_buffer_bytes),
                download_complete: false,
                download_error: None,
                total_size,
            }),
            Condvar::new(),
        ));

        let source = Self {
            state: Arc::clone(&state),
            config: config.clone(),
            read_pos: std::sync::atomic::AtomicU64::new(0),
        };

        let writer = BufferWriter { state };

        (source, writer)
    }

    /// Create a new reader that shares the same buffer but has its own read position.
    /// This is used to pass to symphonia which needs ownership of the reader.
    pub fn create_reader(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            config: self.config.clone(),
            read_pos: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Wait until initial buffer is filled or download completes.
    ///
    /// This should be called before passing the source to the decoder,
    /// to ensure enough data is available for format detection.
    ///
    /// Returns error if download fails before initial buffer is filled.
    pub fn wait_for_initial_buffer(&self) -> IoResult<()> {
        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().map_err(|_| {
            IoError::new(ErrorKind::Other, "Failed to acquire buffer lock")
        })?;

        while state.data.len() < self.config.initial_buffer_bytes
            && !state.download_complete
            && state.download_error.is_none()
        {
            state = cvar.wait(state).map_err(|_| {
                IoError::new(ErrorKind::Other, "Condition variable wait failed")
            })?;
        }

        if let Some(ref err) = state.download_error {
            return Err(IoError::new(ErrorKind::Other, err.clone()));
        }

        Ok(())
    }

    /// Check if download is complete (full file in buffer)
    pub fn is_complete(&self) -> bool {
        let (lock, _) = &*self.state;
        if let Ok(state) = lock.lock() {
            state.download_complete && state.download_error.is_none()
        } else {
            false
        }
    }

    /// Get current buffer size in bytes
    pub fn buffer_size(&self) -> usize {
        let (lock, _) = &*self.state;
        if let Ok(state) = lock.lock() {
            state.data.len()
        } else {
            0
        }
    }

    /// Get the complete data if download finished successfully.
    ///
    /// Used to store in cache after streaming playback completes.
    /// Returns None if download is not complete or failed.
    pub fn take_complete_data(&self) -> Option<Vec<u8>> {
        let (lock, _) = &*self.state;
        if let Ok(state) = lock.lock() {
            if state.download_complete && state.download_error.is_none() {
                Some(state.data.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get download progress as a fraction (0.0 to 1.0)
    ///
    /// Returns None if total size is unknown
    pub fn progress(&self) -> Option<f32> {
        let (lock, _) = &*self.state;
        if let Ok(state) = lock.lock() {
            state.total_size.map(|total| {
                if total == 0 {
                    1.0
                } else {
                    state.data.len() as f32 / total as f32
                }
            })
        } else {
            None
        }
    }

    /// Check if minimum buffer for playback is available
    ///
    /// Returns true when initial_buffer_bytes have been buffered
    /// or the download is complete.
    pub fn has_min_buffer(&self) -> bool {
        let (lock, _) = &*self.state;
        if let Ok(state) = lock.lock() {
            state.data.len() >= self.config.initial_buffer_bytes || state.download_complete
        } else {
            false
        }
    }
}

impl Read for BufferedMediaSource {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        use std::sync::atomic::Ordering;

        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().map_err(|_| {
            IoError::new(ErrorKind::Other, "Failed to acquire buffer lock")
        })?;

        let read_pos = self.read_pos.load(Ordering::SeqCst) as usize;

        // Wait for data if we're ahead of buffer
        while read_pos >= state.data.len()
            && !state.download_complete
            && state.download_error.is_none()
        {
            state = cvar.wait(state).map_err(|_| {
                IoError::new(ErrorKind::Other, "Condition variable wait failed")
            })?;
        }

        // Check for errors
        if let Some(ref err) = state.download_error {
            return Err(IoError::new(ErrorKind::Other, err.clone()));
        }

        // EOF if at end and download complete
        if read_pos >= state.data.len() && state.download_complete {
            return Ok(0);
        }

        // Read available data
        let available = state.data.len() - read_pos;
        let to_read = buf.len().min(available);
        buf[..to_read].copy_from_slice(&state.data[read_pos..read_pos + to_read]);
        self.read_pos.store((read_pos + to_read) as u64, Ordering::SeqCst);

        Ok(to_read)
    }
}

impl Seek for BufferedMediaSource {
    fn seek(&mut self, pos: SeekFrom) -> IoResult<u64> {
        use std::sync::atomic::Ordering;

        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().map_err(|_| {
            IoError::new(ErrorKind::Other, "Failed to acquire buffer lock")
        })?;

        let current_pos = self.read_pos.load(Ordering::SeqCst) as i64;

        let new_pos = match pos {
            SeekFrom::Start(offset) => offset as i64,
            SeekFrom::Current(offset) => current_pos + offset,
            SeekFrom::End(offset) => {
                // For End seeks, we need to know total size or have complete download
                if let Some(total) = state.total_size {
                    total as i64 + offset
                } else if state.download_complete {
                    state.data.len() as i64 + offset
                } else {
                    // Can't seek from end without knowing size
                    return Err(IoError::new(
                        ErrorKind::Unsupported,
                        "Cannot seek from end while streaming without known size",
                    ));
                }
            }
        };

        if new_pos < 0 {
            return Err(IoError::new(
                ErrorKind::InvalidInput,
                "Seek position before start of stream",
            ));
        }

        let new_pos_usize = new_pos as usize;

        // If seeking forward beyond buffer, wait for data
        while new_pos_usize > state.data.len()
            && !state.download_complete
            && state.download_error.is_none()
        {
            state = cvar.wait(state).map_err(|_| {
                IoError::new(ErrorKind::Other, "Condition variable wait failed")
            })?;
        }

        if let Some(ref err) = state.download_error {
            return Err(IoError::new(ErrorKind::Other, err.clone()));
        }

        // After download complete, check bounds
        if state.download_complete && new_pos_usize > state.data.len() {
            return Err(IoError::new(
                ErrorKind::InvalidInput,
                "Seek position beyond end of stream",
            ));
        }

        self.read_pos.store(new_pos as u64, Ordering::SeqCst);
        Ok(new_pos as u64)
    }
}

// Required for symphonia MediaSource trait
impl MediaSource for BufferedMediaSource {
    fn is_seekable(&self) -> bool {
        // We support seeking within buffered data
        true
    }

    fn byte_len(&self) -> Option<u64> {
        let (lock, _) = &*self.state;
        if let Ok(state) = lock.lock() {
            state.total_size
        } else {
            None
        }
    }
}

/// Writer half for pushing downloaded chunks from the async download task.
///
/// This is the sender side that receives data from the HTTP response
/// and makes it available to the `BufferedMediaSource` reader.
#[derive(Clone)]
pub struct BufferWriter {
    state: Arc<(Mutex<BufferState>, Condvar)>,
}

impl BufferWriter {
    /// Push a chunk of downloaded data
    ///
    /// This wakes up any readers waiting for data.
    pub fn push_chunk(&self, chunk: &[u8]) -> Result<(), String> {
        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().map_err(|_| "Failed to acquire buffer lock")?;

        state.data.extend_from_slice(chunk);
        cvar.notify_all();

        Ok(())
    }

    /// Mark download as complete
    ///
    /// After this is called, readers will receive EOF after reading all buffered data.
    pub fn complete(&self) -> Result<(), String> {
        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().map_err(|_| "Failed to acquire buffer lock")?;

        state.download_complete = true;
        cvar.notify_all();

        Ok(())
    }

    /// Mark download as failed
    ///
    /// After this is called, readers will receive the error on next read.
    pub fn error(&self, err: String) -> Result<(), String> {
        let (lock, cvar) = &*self.state;
        let mut state = lock.lock().map_err(|_| "Failed to acquire buffer lock")?;

        state.download_error = Some(err);
        cvar.notify_all();

        Ok(())
    }

    /// Get current buffer size in bytes
    pub fn buffer_size(&self) -> usize {
        let (lock, _) = &*self.state;
        if let Ok(state) = lock.lock() {
            state.data.len()
        } else {
            0
        }
    }
}

// =============================================================================
// IncrementalStreamingSource - A rodio Source that decodes on-demand
// =============================================================================

/// A rodio Source that decodes audio packets incrementally from a BufferedMediaSource.
///
/// This allows playback to start immediately after the initial buffer is filled,
/// while the rest of the file continues downloading in the background.
///
/// The source maintains an internal queue of decoded samples and decodes more
/// packets on-demand as samples are consumed.
pub struct IncrementalStreamingSource {
    /// Sample rate of the audio
    sample_rate: u32,
    /// Number of channels
    channels: u16,
    /// Queue of decoded samples ready to play
    sample_queue: VecDeque<i16>,
    /// The format reader (demuxer)
    format: Box<dyn FormatReader>,
    /// The audio decoder
    decoder: Box<dyn Decoder>,
    /// Track ID we're decoding
    track_id: u32,
    /// Whether we've reached end of stream
    finished: bool,
    /// Number of packets decoded (for stats)
    packets_decoded: u64,
    /// Reference to the buffered source (for cache retrieval after playback)
    buffered_source: Arc<BufferedMediaSource>,
}

impl IncrementalStreamingSource {
    /// Create a new incremental streaming source.
    ///
    /// This initializes the symphonia decoder and prepares for incremental decoding.
    /// The BufferedMediaSource should already have its initial buffer filled.
    ///
    /// Returns the source along with detected sample_rate and channels.
    pub fn new(
        buffered_source: Arc<BufferedMediaSource>,
    ) -> Result<Self, String> {
        // Create a reader from the buffered source
        let reader = buffered_source.create_reader();
        let media_source = Box::new(reader) as Box<dyn MediaSource>;
        let mss = MediaSourceStream::new(media_source, Default::default());

        let mut hint = Hint::new();
        hint.with_extension("flac"); // Most Qobuz Hi-Res is FLAC

        let format_opts = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };
        let metadata_opts: MetadataOptions = Default::default();

        let probed = get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)
            .map_err(|err| format!("Symphonia probe failed for streaming: {}", err))?;

        let track = probed
            .format
            .default_track()
            .ok_or_else(|| "Symphonia: no supported audio tracks in stream".to_string())?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();

        // Extract sample rate and channels from codec params
        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| "No sample rate in codec params".to_string())?;
        let channels = codec_params
            .channels
            .map(|c| c.count() as u16)
            .unwrap_or(2);

        let decoder = get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|err| format!("Symphonia decoder init failed for streaming: {}", err))?;

        log::info!(
            "IncrementalStreamingSource initialized: {}Hz, {} channels",
            sample_rate,
            channels
        );

        Ok(Self {
            sample_rate,
            channels,
            sample_queue: VecDeque::with_capacity(sample_rate as usize * channels as usize), // ~1s buffer
            format: probed.format,
            decoder,
            track_id,
            finished: false,
            packets_decoded: 0,
            buffered_source,
        })
    }

    /// Get the sample rate
    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get the number of channels
    pub fn get_channels(&self) -> u16 {
        self.channels
    }

    /// Get reference to the buffered source for cache retrieval
    pub fn buffered_source(&self) -> &Arc<BufferedMediaSource> {
        &self.buffered_source
    }

    /// Decode more packets to fill the sample queue.
    ///
    /// This is called when the sample queue is running low.
    /// It will decode packets until the queue has at least `min_samples` or EOF is reached.
    fn decode_more(&mut self, min_samples: usize) {
        if self.finished {
            return;
        }

        while self.sample_queue.len() < min_samples {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Not enough data buffered yet - wait briefly and retry
                    // This happens when playback catches up with download
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(SymphoniaError::IoError(_)) => {
                    // EOF or other IO error
                    log::info!(
                        "IncrementalStreamingSource: EOF reached after {} packets",
                        self.packets_decoded
                    );
                    self.finished = true;
                    return;
                }
                Err(err) => {
                    log::error!("Symphonia read error in stream: {}", err);
                    self.finished = true;
                    return;
                }
            };

            if packet.track_id() != self.track_id {
                continue;
            }

            match self.decoder.decode(&packet) {
                Ok(audio_buf) => {
                    let spec = *audio_buf.spec();
                    let mut sample_buf = SampleBuffer::<i16>::new(audio_buf.frames() as u64, spec);
                    sample_buf.copy_interleaved_ref(audio_buf);

                    // Add samples to queue
                    self.sample_queue.extend(sample_buf.samples().iter().copied());
                    self.packets_decoded += 1;
                }
                Err(SymphoniaError::DecodeError(e)) => {
                    log::warn!("Decode error (skipping packet): {}", e);
                    continue;
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(err) => {
                    log::error!("Symphonia decode error: {}", err);
                    self.finished = true;
                    return;
                }
            }
        }
    }
}

impl Source for IncrementalStreamingSource {
    fn current_frame_len(&self) -> Option<usize> {
        // We don't know frame boundaries in the queue
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        // We don't know total duration until download completes
        // Could estimate from content-length if available
        None
    }
}

impl Iterator for IncrementalStreamingSource {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        // If queue is running low, decode more
        // Keep at least 0.5 seconds of audio buffered
        let min_buffer = (self.sample_rate as usize * self.channels as usize) / 2;
        if self.sample_queue.len() < min_buffer {
            self.decode_more(min_buffer);
        }

        self.sample_queue.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_basic_read_write() {
        let config = StreamingConfig {
            initial_buffer_bytes: 10,
            max_buffer_bytes: 100,
        };
        let (mut source, writer) = BufferedMediaSource::new(config, Some(20));

        // Write some data
        writer.push_chunk(b"Hello").unwrap();
        writer.push_chunk(b"World").unwrap();

        // Read it back
        let mut buf = [0u8; 5];
        assert_eq!(source.read(&mut buf).unwrap(), 5);
        assert_eq!(&buf, b"Hello");

        assert_eq!(source.read(&mut buf).unwrap(), 5);
        assert_eq!(&buf, b"World");
    }

    #[test]
    fn test_seek_within_buffer() {
        let config = StreamingConfig {
            initial_buffer_bytes: 5,
            max_buffer_bytes: 100,
        };
        let (mut source, writer) = BufferedMediaSource::new(config, Some(10));

        writer.push_chunk(b"0123456789").unwrap();
        writer.complete().unwrap();

        // Read first 5 bytes
        let mut buf = [0u8; 5];
        source.read(&mut buf).unwrap();
        assert_eq!(&buf, b"01234");

        // Seek back to start
        source.seek(SeekFrom::Start(0)).unwrap();
        source.read(&mut buf).unwrap();
        assert_eq!(&buf, b"01234");

        // Seek to middle
        source.seek(SeekFrom::Start(3)).unwrap();
        source.read(&mut buf).unwrap();
        assert_eq!(&buf, b"34567");
    }

    #[test]
    fn test_complete_data_retrieval() {
        let config = StreamingConfig {
            initial_buffer_bytes: 5,
            max_buffer_bytes: 100,
        };
        let (source, writer) = BufferedMediaSource::new(config, Some(10));

        writer.push_chunk(b"Hello").unwrap();
        assert!(source.take_complete_data().is_none()); // Not complete yet

        writer.push_chunk(b"World").unwrap();
        writer.complete().unwrap();

        let data = source.take_complete_data().unwrap();
        assert_eq!(&data, b"HelloWorld");
    }

    #[test]
    fn test_blocking_read() {
        let config = StreamingConfig {
            initial_buffer_bytes: 5,
            max_buffer_bytes: 100,
        };
        let (mut source, writer) = BufferedMediaSource::new(config, None);

        // Spawn thread to write after delay
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            writer.push_chunk(b"Delayed").unwrap();
            writer.complete().unwrap();
        });

        // This should block until data arrives
        let mut buf = [0u8; 7];
        let n = source.read(&mut buf).unwrap();
        assert_eq!(n, 7);
        assert_eq!(&buf, b"Delayed");
    }
}
