//! Audio player module
//!
//! Handles audio playback with support for:
//! - HTTP streaming from Qobuz
//! - FLAC, MP3 decoding via symphonia
//! - Gapless playback
//! - Volume control
//! - Real-time position tracking via events
//!
//! Uses a dedicated audio thread since rodio's OutputStream is not Send.
//! Supports both rodio (PipeWire/Pulse) and direct ALSA (hw: devices).

mod playback_engine;
mod streaming_source;

pub use streaming_source::{BufferedMediaSource, BufferWriter, StreamingConfig, IncrementalStreamingSource};

use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use rodio::{Decoder, OutputStream, Source};
use rodio::buffer::SamplesBuffer;
use rodio::decoder::Mp4Type;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::cpal::{StreamConfig, SampleRate, BufferSize, SupportedStreamConfig, SupportedBufferSize, SampleFormat};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

use crate::api::{client::QobuzClient, models::Quality};
use crate::audio::{
    AudioBackendType, AudioDiagnostic, BackendConfig, BackendManager, DiagnosticSource,
    extract_replaygain, calculate_gain_factor, db_to_linear,
    DynamicAmplify, AnalyzerTap, AnalyzerMessage, LoudnessCache, LoudnessAnalyzer,
};
use crate::config::audio_settings::AudioSettings;
use crate::visualizer::{VisualizerTap, TappedSource};
use playback_engine::PlaybackEngine;

/// Commands sent to the audio thread
enum AudioCommand {
    /// Play audio data with track ID, duration, and audio specs
    Play {
        data: Vec<u8>,
        track_id: u64,
        duration_secs: u64,
        sample_rate: u32,
        channels: u16,
    },
    /// Play from streaming source (BufferedMediaSource)
    /// The download task should already be running and pushing to the source
    PlayStreaming {
        source: Arc<BufferedMediaSource>,
        track_id: u64,
        sample_rate: u32,
        channels: u16,
        duration_secs: u64,
    },
    /// Pause playback
    Pause,
    /// Resume playback
    Resume,
    /// Stop playback
    Stop,
    /// Set volume (0.0 - 1.0)
    SetVolume(f32),
    /// Seek to position in seconds
    Seek(u64),
    /// Reinitialize audio device (releases and re-acquires)
    ReinitDevice { device_name: Option<String> },
    /// Append next track to current engine for gapless playback (Rodio only)
    PlayNext {
        data: Vec<u8>,
        track_id: u64,
        sample_rate: u32,
        channels: u16,
    },
}

/// Pending gapless track data (queued for seamless transition)
struct GaplessPending {
    track_id: u64,
    duration_secs: u64,
    data: Vec<u8>,
    normalization_gain: Option<f32>,
}

struct CursorMediaSource {
    inner: Cursor<Vec<u8>>,
    len: u64,
}

impl CursorMediaSource {
    fn new(data: Vec<u8>) -> Self {
        let len = data.len() as u64;
        Self { inner: Cursor::new(data), len }
    }
}

impl MediaSource for CursorMediaSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

impl Read for CursorMediaSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Seek for CursorMediaSource {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// Audio specifications extracted from decoded audio
#[allow(dead_code)]
struct AudioSpecs {
    samples: SamplesBuffer<f32>,
    sample_rate: u32,
    channels: u16,
}

fn decode_with_symphonia(data: &[u8]) -> Result<AudioSpecs, String> {
    let source = Box::new(CursorMediaSource::new(data.to_vec())) as Box<dyn MediaSource>;
    let mss = MediaSourceStream::new(source, Default::default());

    let mut hint = Hint::new();
    hint.with_extension("m4a");

    let format_opts = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let metadata_opts: MetadataOptions = Default::default();
    let mut probed = get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|err| format!("Symphonia probe failed: {}", err))?;

    let track = probed
        .format
        .default_track()
        .ok_or_else(|| "Symphonia: no supported audio tracks".to_string())?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let mut decoder = get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|err| format!("Symphonia decoder init failed: {}", err))?;

    let mut sample_rate = 0;
    let mut channels = 0u16;
    let mut samples: Vec<f32> = Vec::new();

    loop {
        let packet = match probed.format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(_)) => break,
            Err(err) => return Err(format!("Symphonia read error: {}", err)),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                let spec = *audio_buf.spec();
                if sample_rate == 0 {
                    sample_rate = spec.rate;
                    channels = spec.channels.count() as u16;
                }

                let mut sample_buf = SampleBuffer::<f32>::new(audio_buf.frames() as u64, spec);
                sample_buf.copy_interleaved_ref(audio_buf);
                samples.extend_from_slice(sample_buf.samples());
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(err) => return Err(format!("Symphonia decode error: {}", err)),
        }
    }

    if samples.is_empty() || sample_rate == 0 || channels == 0 {
        return Err("Symphonia decode produced no audio".to_string());
    }

    Ok(AudioSpecs {
        samples: SamplesBuffer::new(channels, sample_rate, samples),
        sample_rate,
        channels,
    })
}

fn is_isomp4(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }

    &data[4..8] == b"ftyp"
}

/// Extract audio metadata (sample rate, channels) without full decode.
/// This is much faster than decode_with_symphonia as it only reads headers.
/// Audio metadata extracted from file headers
#[allow(dead_code)]
struct AudioMetadata {
    sample_rate: u32,
    channels: u16,
    bit_depth: Option<u32>,
}

#[allow(dead_code)]
fn extract_audio_metadata(data: &[u8]) -> Result<(u32, u16), String> {
    let meta = extract_audio_metadata_full(data)?;
    Ok((meta.sample_rate, meta.channels))
}

fn extract_audio_metadata_full(data: &[u8]) -> Result<AudioMetadata, String> {
    // For non-isomp4 files (FLAC, etc.), try symphonia directly to get all metadata
    // Symphonia gives us bits_per_sample which rodio doesn't expose

    // Use symphonia probe for codec params (no decode needed)
    let source = Box::new(CursorMediaSource::new(data.to_vec())) as Box<dyn MediaSource>;
    let mss = MediaSourceStream::new(source, Default::default());

    let mut hint = Hint::new();
    if is_isomp4(data) {
        hint.with_extension("m4a");
    }

    let format_opts = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let metadata_opts: MetadataOptions = Default::default();
    let probed = get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|err| format!("Symphonia probe failed: {}", err))?;

    let track = probed
        .format
        .default_track()
        .ok_or_else(|| "Symphonia: no supported audio tracks".to_string())?;

    let sample_rate = track.codec_params.sample_rate
        .ok_or_else(|| "No sample rate in codec params".to_string())?;

    // ALAC and some other formats don't include channel info in initial codec params
    // Default to stereo (2 channels) which is the most common case
    let channels = track.codec_params.channels
        .map(|c| c.count() as u16)
        .unwrap_or(2);

    // Get bits per sample for bit depth
    let bit_depth = track.codec_params.bits_per_sample;

    Ok(AudioMetadata {
        sample_rate,
        channels,
        bit_depth,
    })
}

fn decode_with_fallback(
    data: &[u8],
) -> Result<Box<dyn Source<Item = f32> + Send>, String> {
    if is_isomp4(data) {
        return decode_with_symphonia(data)
            .map(|specs| {
                log::info!("Decoded audio using symphonia fallback (isomp4)");
                Box::new(specs.samples) as Box<dyn Source<Item = f32> + Send>
            });
    }

    let primary = panic::catch_unwind(AssertUnwindSafe(|| {
        Decoder::new(BufReader::new(Cursor::new(data.to_vec())))
    }));

    match primary {
        Ok(Ok(decoder)) => return Ok(Box::new(decoder)),
        Ok(Err(err)) => {
            log::warn!("Primary decode failed, attempting mp4 fallback: {}", err);
        }
        Err(_) => {
            log::warn!("Primary decode panicked, attempting mp4 fallback");
        }
    }

    let mp4_attempts = [Mp4Type::M4a, Mp4Type::Mp4];
    for hint in mp4_attempts {
        let hint_label = format!("{:?}", hint);
        let attempt = panic::catch_unwind(AssertUnwindSafe(|| {
            Decoder::new_mp4(BufReader::new(Cursor::new(data.to_vec())), hint)
        }));

        match attempt {
            Ok(Ok(decoder)) => {
                log::info!("Decoded audio using mp4 fallback ({})", hint_label);
                return Ok(Box::new(decoder));
            }
            Ok(Err(err)) => {
                log::warn!("mp4 fallback ({}) failed: {}", hint_label, err);
            }
            Err(_) => {
                log::warn!("mp4 fallback ({}) panicked", hint_label);
            }
        }
    }

    match decode_with_symphonia(data) {
        Ok(specs) => {
            log::info!("Decoded audio using symphonia fallback");
            Ok(Box::new(specs.samples))
        }
        Err(err) => Err(err),
    }
}

/// Create OutputStream with custom sample rate configuration
fn create_output_stream_with_config(
    device: &rodio::cpal::Device,
    sample_rate: u32,
    channels: u16,
    exclusive_mode: bool,
) -> Result<(OutputStream, rodio::OutputStreamHandle), String> {
    log::info!(
        "Creating OutputStream: {}Hz, {} channels, exclusive: {}",
        sample_rate,
        channels,
        exclusive_mode
    );

    // Create StreamConfig with desired sample rate
    let config = StreamConfig {
        channels,
        sample_rate: SampleRate(sample_rate),
        buffer_size: if exclusive_mode {
            BufferSize::Fixed(512)  // Lower latency for exclusive mode
        } else {
            BufferSize::Default
        },
    };

    // Check if device supports this configuration
    let supported_configs = device
        .supported_output_configs()
        .map_err(|e| format!("Failed to get supported configs: {}", e))?;

    let mut found_matching = false;
    for range in supported_configs {
        if range.channels() == channels
            && sample_rate >= range.min_sample_rate().0
            && sample_rate <= range.max_sample_rate().0
        {
            found_matching = true;
            log::info!(
                "Device supports {}Hz (range: {}-{}Hz)",
                sample_rate,
                range.min_sample_rate().0,
                range.max_sample_rate().0
            );
            break;
        }
    }

    if !found_matching {
        log::warn!(
            "Device may not support {}Hz, attempting anyway",
            sample_rate
        );
    }

    // Create SupportedStreamConfig
    let supported_config = SupportedStreamConfig::new(
        config.channels,
        config.sample_rate,
        SupportedBufferSize::Range { min: 64, max: 8192 },
        SampleFormat::F32,
    );

    // Create OutputStream with custom config
    match OutputStream::try_from_device_config(device, supported_config) {
        Ok((stream, handle)) => {
            log::info!("OutputStream created successfully at {}Hz", sample_rate);
            Ok((stream, handle))
        }
        Err(e) => {
            log::error!("❌ Failed to create OutputStream at {}Hz: {}", sample_rate, e);
            Err(format!("Failed to create output stream: {}", e))
        }
    }
}

/// Output stream type - either rodio or ALSA Direct
enum StreamType {
    Rodio(OutputStream, rodio::OutputStreamHandle),
    #[cfg(target_os = "linux")]
    AlsaDirect(Arc<crate::audio::AlsaDirectStream>),
}

/// Try to create output stream using the backend system (if configured)
/// Returns None if backend system is not configured (backend_type = None)
///
/// For ALSA backend with hw: devices, may return AlsaDirect instead of Rodio stream.
fn try_init_stream_with_backend(
    audio_settings: &AudioSettings,
    sample_rate: u32,
    channels: u16,
) -> Option<Result<StreamType, String>> {
    // Check if backend system is configured
    let backend_type = audio_settings.backend_type?;

    log::info!(
        "Using backend system: {:?} (device: {:?}, plugin: {:?})",
        backend_type,
        audio_settings.output_device,
        audio_settings.alsa_plugin
    );

    // Create backend
    let backend = match BackendManager::create_backend(backend_type) {
        Ok(b) => b,
        Err(e) => {
            log::error!("Failed to create backend {:?}: {}", backend_type, e);
            return Some(Err(e));
        }
    };

    // Check availability
    if !backend.is_available() {
        let msg = format!("Backend {:?} is not available on this system", backend_type);
        log::error!("{}", msg);
        return Some(Err(msg));
    }

    // Build backend config
    let config = BackendConfig {
        backend_type,
        device_id: audio_settings.output_device.clone(),
        sample_rate,
        channels,
        exclusive_mode: audio_settings.exclusive_mode,
        alsa_plugin: audio_settings.alsa_plugin,
    };

    // For ALSA backend with hw: devices, try direct ALSA first (Linux only)
    #[cfg(target_os = "linux")]
    if backend_type == AudioBackendType::Alsa {
        // Check if device is hw: or plughw:
        if let Some(ref device_id) = config.device_id {
            if crate::audio::AlsaDirectStream::is_hw_device(device_id) {
                log::info!("Detected hw: device, using ALSA Direct for bit-perfect playback");

                // Downcast backend to AlsaBackend to access try_create_direct_stream
                if let Some(alsa_backend) = backend.as_any().downcast_ref::<crate::audio::alsa_backend::AlsaBackend>() {
                    if let Some(result) = alsa_backend.try_create_direct_stream(&config) {
                        return Some(result.map(|(stream, mode)| {
                            log::info!("ALSA Direct stream created with mode: {:?}", mode);
                            StreamType::AlsaDirect(Arc::new(stream))
                        }));
                    }
                }
            }
        }
    }

    // Fallback to regular rodio stream (PipeWire, Pulse, ALSA via CPAL)
    match backend.create_output_stream(&config) {
        Ok(stream) => {
            log::info!("Stream created via {:?} backend at {}Hz", backend_type, sample_rate);
            Some(Ok(StreamType::Rodio(stream.0, stream.1)))
        }
        Err(e) => {
            log::error!("❌ Backend stream creation failed: {}", e);
            Some(Err(e))
        }
    }
}

/// Event payload for playback state updates
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaybackEvent {
    pub is_playing: bool,
    pub position: u64,
    pub duration: u64,
    pub track_id: u64,
    pub volume: f32,
    /// Actual sample rate of the current stream (Hz)
    pub sample_rate: Option<u32>,
    /// Actual bit depth of the current stream
    pub bit_depth: Option<u32>,
    /// Queue shuffle state
    pub shuffle: Option<bool>,
    /// Queue repeat mode ("off", "all", "one")
    pub repeat: Option<String>,
    /// Normalization gain factor being applied (None = normalization not active)
    pub normalization_gain: Option<f32>,
    /// True when backend wants the next track pre-queued for gapless playback
    #[serde(default)]
    pub gapless_ready: bool,
    /// Track ID of the gapless-queued next track (0 = none queued)
    #[serde(default)]
    pub gapless_next_track_id: u64,
}

/// Shared state between main thread and audio thread
#[derive(Clone)]
pub struct SharedState {
    /// Is currently playing
    is_playing: Arc<AtomicBool>,
    /// Current position in seconds
    position: Arc<AtomicU64>,
    /// Total duration in seconds
    duration: Arc<AtomicU64>,
    /// Current track ID
    current_track_id: Arc<AtomicU64>,
    /// Volume (0.0 - 1.0 stored as 0-100)
    volume: Arc<AtomicU64>,
    /// Playback start time (Unix timestamp millis when started/resumed)
    playback_start_millis: Arc<AtomicU64>,
    /// Position when playback was started/resumed (in seconds)
    position_at_start: Arc<AtomicU64>,
    /// Current output device name
    current_device: Arc<std::sync::RwLock<Option<String>>>,
    /// Stream error flag (set when ALSA/audio errors are detected)
    stream_error: Arc<AtomicBool>,
    /// Actual sample rate of the current stream (Hz)
    sample_rate: Arc<AtomicU32>,
    /// Actual bit depth of the current stream
    bit_depth: Arc<AtomicU32>,
    /// Current normalization gain factor (f32 stored as u32 bits, 0 = not applied)
    normalization_gain: Arc<AtomicU32>,
    /// True when the audio thread wants the next track pre-queued for gapless
    gapless_ready: Arc<AtomicBool>,
    /// Track ID of the gapless-queued next track (0 = none)
    gapless_next_track_id: Arc<AtomicU64>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            is_playing: Arc::new(AtomicBool::new(false)),
            position: Arc::new(AtomicU64::new(0)),
            duration: Arc::new(AtomicU64::new(0)),
            current_track_id: Arc::new(AtomicU64::new(0)),
            volume: Arc::new(AtomicU64::new(75)),
            playback_start_millis: Arc::new(AtomicU64::new(0)),
            position_at_start: Arc::new(AtomicU64::new(0)),
            current_device: Arc::new(std::sync::RwLock::new(None)),
            stream_error: Arc::new(AtomicBool::new(false)),
            sample_rate: Arc::new(AtomicU32::new(0)),
            bit_depth: Arc::new(AtomicU32::new(0)),
            normalization_gain: Arc::new(AtomicU32::new(0)),
            gapless_ready: Arc::new(AtomicBool::new(false)),
            gapless_next_track_id: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn set_stream_error(&self, error: bool) {
        self.stream_error.store(error, Ordering::SeqCst);
    }

    pub fn has_stream_error(&self) -> bool {
        self.stream_error.load(Ordering::SeqCst)
    }

    pub fn set_stream_quality(&self, sample_rate: u32, bit_depth: u32) {
        self.sample_rate.store(sample_rate, Ordering::SeqCst);
        self.bit_depth.store(bit_depth, Ordering::SeqCst);
    }

    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::SeqCst)
    }

    pub fn get_bit_depth(&self) -> u32 {
        self.bit_depth.load(Ordering::SeqCst)
    }

    /// Set the current normalization gain factor.
    /// Stores f32 as u32 bits. Pass None (or 0.0) to indicate no normalization.
    pub fn set_normalization_gain(&self, gain: Option<f32>) {
        let bits = gain.unwrap_or(0.0).to_bits();
        self.normalization_gain.store(bits, Ordering::SeqCst);
    }

    /// Get the current normalization gain factor.
    /// Returns None if normalization is not active (gain is 0.0).
    pub fn get_normalization_gain(&self) -> Option<f32> {
        let bits = self.normalization_gain.load(Ordering::SeqCst);
        let gain = f32::from_bits(bits);
        if gain == 0.0 { None } else { Some(gain) }
    }

    pub fn set_current_device(&self, device: Option<String>) {
        if let Ok(mut d) = self.current_device.write() {
            *d = device;
        }
    }

    pub fn current_device(&self) -> Option<String> {
        self.current_device.read().ok().and_then(|d| d.clone())
    }

    pub fn set_gapless_ready(&self, ready: bool) {
        self.gapless_ready.store(ready, Ordering::SeqCst);
    }

    pub fn is_gapless_ready(&self) -> bool {
        self.gapless_ready.load(Ordering::SeqCst)
    }

    pub fn set_gapless_next_track_id(&self, track_id: u64) {
        self.gapless_next_track_id.store(track_id, Ordering::SeqCst);
    }

    pub fn get_gapless_next_track_id(&self) -> u64 {
        self.gapless_next_track_id.load(Ordering::SeqCst)
    }

    /// Get current position based on elapsed time since playback started
    pub fn current_position(&self) -> u64 {
        if !self.is_playing.load(Ordering::SeqCst) {
            return self.position.load(Ordering::SeqCst);
        }

        let start_millis = self.playback_start_millis.load(Ordering::SeqCst);
        if start_millis == 0 {
            return self.position.load(Ordering::SeqCst);
        }

        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let elapsed_secs = (now_millis.saturating_sub(start_millis)) / 1000;
        let position_at_start = self.position_at_start.load(Ordering::SeqCst);
        let duration = self.duration.load(Ordering::SeqCst);

        // Clamp to duration
        (position_at_start + elapsed_secs).min(duration)
    }

    /// Mark playback as started/resumed at current position
    fn start_playback_timer(&self, position: u64) {
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.playback_start_millis.store(now_millis, Ordering::SeqCst);
        self.position_at_start.store(position, Ordering::SeqCst);
    }

    /// Mark playback as paused, saving current position
    fn pause_playback_timer(&self) {
        let current_pos = self.current_position();
        self.position.store(current_pos, Ordering::SeqCst);
        self.playback_start_millis.store(0, Ordering::SeqCst);
    }

    pub fn is_playing(&self) -> bool {
        self.is_playing.load(Ordering::SeqCst)
    }

    pub fn position(&self) -> u64 {
        self.position.load(Ordering::SeqCst)
    }

    pub fn duration(&self) -> u64 {
        self.duration.load(Ordering::SeqCst)
    }

    pub fn current_track_id(&self) -> u64 {
        self.current_track_id.load(Ordering::SeqCst)
    }

    pub fn volume(&self) -> f32 {
        self.volume.load(Ordering::SeqCst) as f32 / 100.0
    }
}

/// Audio player that handles streaming playback
/// Uses a dedicated thread for audio output
pub struct Player {
    /// Channel to send commands to the audio thread
    tx: Sender<AudioCommand>,
    /// Shared state accessible from any thread
    pub state: SharedState,
    /// Audio settings (exclusive mode, DAC passthrough, etc.)
    audio_settings: Arc<Mutex<AudioSettings>>,
    /// Visualizer tap for audio sample capture (optional)
    #[allow(dead_code)]
    visualizer_tap: Option<VisualizerTap>,
    /// Bit-depth diagnostic capture (always available, zero-cost when idle)
    pub diagnostic: AudioDiagnostic,
}

impl Default for Player {
    fn default() -> Self {
        Self::new(None, AudioSettings::default(), None, AudioDiagnostic::new())
    }
}

impl Player {
    /// Create a new player with an optional specific output device and audio settings
    /// If device_name is None, uses the system default device
    /// visualizer_tap is optional - if provided, audio samples are captured for visualization
    pub fn new(device_name: Option<String>, audio_settings: AudioSettings, visualizer_tap: Option<VisualizerTap>, diagnostic: AudioDiagnostic) -> Self {
        let (tx, rx) = mpsc::channel::<AudioCommand>();
        let state = SharedState::new();
        let thread_state = state.clone();

        // Clone settings for thread
        let settings = Arc::new(Mutex::new(audio_settings.clone()));
        let thread_settings = settings.clone();

        // Clone visualizer tap and diagnostic for audio thread
        let thread_viz_tap = visualizer_tap.clone();
        let thread_diagnostic = diagnostic.clone();

        // Spawn dedicated audio thread
        thread::spawn(move || {
            log::info!("Audio thread starting...");

            // Initialize loudness analysis system
            let (analyzer_tx, analyzer_rx) = mpsc::sync_channel::<AnalyzerMessage>(64);
            let loudness_cache = match LoudnessCache::new() {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    log::error!("Failed to create loudness cache: {}. Normalization will work without caching.", e);
                    // Create a fallback in-memory cache (will be lost on restart)
                    // For now, just panic — this should not fail in practice
                    panic!("LoudnessCache creation failed: {}", e);
                }
            };
            let _analyzer_handle = LoudnessAnalyzer::spawn(analyzer_rx, loudness_cache.clone());
            let analyzer_enabled = Arc::new(AtomicBool::new(false));

            // Helper to wrap source with visualizer tap, normalization, and diagnostic capture
            // Pipeline order (normalization ON):
            //   Diagnostic (raw) → AnalyzerTap → DynamicAmplify → Visualizer
            // Pipeline order (normalization OFF — bit-perfect):
            //   Diagnostic (raw) → Visualizer
            let wrap_source = |source: Box<dyn Source<Item = f32> + Send>,
                               normalization_gain: Option<f32>,
                               gain_atomic: Option<Arc<AtomicU32>>,
                               analyzer_tx: &SyncSender<AnalyzerMessage>,
                               analyzer_enabled: &Arc<AtomicBool>| -> Box<dyn Source<Item = f32> + Send> {
                // Diagnostic tap (innermost — captures raw decoded samples)
                let source: Box<dyn Source<Item = f32> + Send> =
                    Box::new(DiagnosticSource::new(source, thread_diagnostic.clone()));

                // Normalization: dynamic (Phase 2) > static (Phase 1 fallback) > none (bit-perfect)
                let source: Box<dyn Source<Item = f32> + Send> = if let Some(gain_atomic) = gain_atomic {
                    let initial_gain = normalization_gain.unwrap_or(1.0);
                    log::info!("Audio thread: dynamic normalization enabled (initial gain {:.4})", initial_gain);
                    analyzer_enabled.store(true, Ordering::SeqCst);
                    let source: Box<dyn Source<Item = f32> + Send> = Box::new(AnalyzerTap::new(source, analyzer_tx.clone(), analyzer_enabled.clone()));
                    Box::new(DynamicAmplify::new(source, gain_atomic, initial_gain))
                } else if let Some(gain) = normalization_gain {
                    log::info!("Audio thread: applying static normalization gain factor {:.4}", gain);
                    Box::new(source.amplify(gain))
                } else {
                    source
                };

                // Visualizer tap (outermost)
                if let Some(ref tap) = thread_viz_tap {
                    Box::new(TappedSource::new(source, tap.ring_buffer.clone(), tap.enabled.clone()))
                } else {
                    source
                }
            };

            // Get the audio host
            let host = rodio::cpal::default_host();

            // Helper to validate a device has supported output configs
            let is_device_valid = |d: &rodio::cpal::Device| -> bool {
                d.supported_output_configs()
                    .map(|configs| configs.count() > 0)
                    .unwrap_or(false)
            };

            // Helper to find and initialize audio device
            // Try backend system first, fall back to legacy CPAL
            // Takes desired sample_rate and channels to maintain DAC passthrough
            let init_device = |name: &Option<String>, state: &SharedState, sample_rate: u32, channels: u16| -> Option<StreamType> {
                // Try backend system if configured
                if let Ok(settings) = thread_settings.lock() {
                    if settings.backend_type.is_some() {
                        // Use provided sample rate/channels to maintain DAC passthrough
                        log::info!("Initializing backend system with {}Hz/{}ch", sample_rate, channels);
                        match try_init_stream_with_backend(&settings, sample_rate, channels) {
                            Some(Ok(stream_type)) => {
                                // Set device name from settings for backend system
                                let device_name = settings.output_device.clone()
                                    .unwrap_or_else(|| "Default".to_string());
                                log::info!("Audio output initialized via backend system at {}Hz (device: {})", sample_rate, device_name);
                                state.set_current_device(Some(device_name));
                                return Some(stream_type);
                            }
                            Some(Err(e)) => {
                                log::warn!("Backend system init failed: {}, falling back to legacy", e);
                            }
                            None => {
                                // Backend not configured, continue to legacy path
                            }
                        }
                    }
                }

                // Legacy CPAL path
                let device = if let Some(ref name) = name {
                    log::info!("Looking for audio device: {}", name);
                    let found = host.output_devices()
                        .ok()
                        .and_then(|mut devices| {
                            devices.find(|d| d.name().ok().as_ref() == Some(name))
                        });

                    match found {
                        Some(d) if is_device_valid(&d) => {
                            log::info!("Found and validated device: {}", name);
                            Some(d)
                        }
                        Some(_) => {
                            log::warn!("Device '{}' found but has no valid output configs, using default", name);
                            host.default_output_device()
                        }
                        None => {
                            log::warn!("Device '{}' not found, using default", name);
                            host.default_output_device()
                        }
                    }
                } else {
                    log::info!("Using default audio device");
                    host.default_output_device()
                };

                let device = match device {
                    Some(d) => {
                        if let Ok(name) = d.name() {
                            log::info!("Using audio device: {}", name);
                            state.set_current_device(Some(name));
                        }
                        d
                    }
                    None => {
                        log::error!("No audio output device available");
                        state.set_current_device(None);
                        return None;
                    }
                };

                match OutputStream::try_from_device(&device) {
                    Ok((stream, handle)) => {
                        log::info!("Audio output initialized successfully");
                        Some(StreamType::Rodio(stream, handle))
                    }
                    Err(e) => {
                        log::error!("Failed to create audio output on device: {}. Trying default...", e);
                        match OutputStream::try_default() {
                            Ok((stream, handle)) => {
                                log::info!("Fallback to default audio output succeeded");
                                Some(StreamType::Rodio(stream, handle))
                            }
                            Err(e2) => {
                                log::error!("Failed to create default audio output: {}", e2);
                                state.set_current_device(None);
                                None
                            }
                        }
                    }
                }
            };

            // Initialize audio device lazily on first playback to avoid idle CPU usage.
            let mut current_device_name = device_name.clone();
            let mut stream_opt: Option<StreamType> = None;
            let mut current_sample_rate: Option<u32> = None;
            let mut current_channels: Option<u16> = None;

            #[allow(dead_code)]
            const MAX_INIT_RETRIES: u32 = 5;
            #[allow(dead_code)]
            const RETRY_DELAY_MS: u64 = 500;

            let mut current_engine: Option<PlaybackEngine> = None;
            // Store audio data for seeking (we need to re-decode from the beginning)
            let mut current_audio_data: Option<Vec<u8>> = None;
            // Store streaming source for resume (when download completes, we can get the data)
            let mut current_streaming_source: Option<Arc<BufferedMediaSource>> = None;
            // Track consecutive sink creation failures to detect broken streams
            let mut consecutive_sink_failures: u32 = 0;
            const MAX_SINK_FAILURES: u32 = 3;
            // Delay dropping the audio stream after pause to reduce CPU usage.
            const PAUSE_SUSPEND_DELAY_MS: u64 = 2000;
            let mut pause_suspend_deadline: Option<Instant> = None;
            let mut last_empty_check = Instant::now();
            // Current track's normalization gain factor (stored for reuse on resume/seek)
            let mut current_normalization_gain: Option<f32> = None;
            // Current track's dynamic gain atomic (shared with DynamicAmplify + LoudnessAnalyzer)
            let mut current_gain_atomic: Option<Arc<AtomicU32>> = None;
            // Gapless: pending next track that has been appended to the Sink
            let mut gapless_pending: Option<GaplessPending> = None;

            log::info!("Audio thread ready and waiting for commands");

            let handle_command = |command: AudioCommand,
                                      current_engine: &mut Option<PlaybackEngine>,
                                      current_audio_data: &mut Option<Vec<u8>>,
                                      current_streaming_source: &mut Option<Arc<BufferedMediaSource>>,
                                      stream_opt: &mut Option<StreamType>,
                                      current_device_name: &mut Option<String>,
                                      consecutive_sink_failures: &mut u32,
                                      pause_suspend_deadline: &mut Option<Instant>,
                                      current_sample_rate: &mut Option<u32>,
                                      current_channels: &mut Option<u16>,
                                      current_normalization_gain: &mut Option<f32>,
                                      current_gain_atomic: &mut Option<Arc<AtomicU32>>,
                                      gapless_pending: &mut Option<GaplessPending>| {
                match command {
                    AudioCommand::Play { data, track_id, duration_secs, sample_rate, channels } => {
                        log::info!(
                            "Audio thread: playing track {} ({}Hz, {} channels)",
                            track_id,
                            sample_rate,
                            channels
                        );
                        *pause_suspend_deadline = None;
                        // Clear any pending gapless state (new Play supersedes queued gapless)
                        *gapless_pending = None;
                        thread_state.set_gapless_ready(false);
                        thread_state.set_gapless_next_track_id(0);

                        // Get DAC passthrough setting
                        let dac_passthrough = thread_settings
                            .lock()
                            .ok()
                            .map(|s| s.dac_passthrough)
                            .unwrap_or(false);

                        // Check if we need to recreate the stream
                        // Recreate on format change if DAC passthrough OR ALSA Direct is enabled (both require bit-perfect)
                        let format_changed = *current_sample_rate != Some(sample_rate)
                            || *current_channels != Some(channels);

                        // Check if using ALSA Direct backend
                        let using_alsa_direct = thread_settings
                            .lock()
                            .ok()
                            .and_then(|s| s.backend_type)
                            .map(|b| b == AudioBackendType::Alsa)
                            .unwrap_or(false);

                        let needs_new_stream = stream_opt.is_none()
                            || (dac_passthrough && format_changed)
                            || (using_alsa_direct && format_changed);

                        if needs_new_stream {
                            if stream_opt.is_some() {
                                if (dac_passthrough || using_alsa_direct) && format_changed {
                                    let mode = if using_alsa_direct { "ALSA Direct" } else { "DAC passthrough" };
                                    log::info!(
                                        "Sample rate/channels changed from {:?}Hz/{:?}ch to {}Hz/{}ch - recreating OutputStream ({})",
                                        *current_sample_rate,
                                        *current_channels,
                                        sample_rate,
                                        channels,
                                        mode
                                    );
                                } else {
                                    log::info!("Creating initial OutputStream");
                                }
                                // Drop old stream
                                drop(stream_opt.take());
                            }

                            log::info!("DAC passthrough: {}, ALSA Direct: {}", dac_passthrough, using_alsa_direct);

                            // Try backend system first (if configured), then fall back to legacy CPAL
                            // This avoids unnecessary CPAL device enumeration for PipeWire DAC and ALSA Direct
                            let stream_result = if let Some(settings) = thread_settings.lock().ok() {
                                match try_init_stream_with_backend(&settings, sample_rate, channels) {
                                    Some(result) => {
                                        // Backend system handled it - set device name from settings
                                        if result.is_ok() {
                                            let device_name = settings.output_device.clone()
                                                .unwrap_or_else(|| "Default".to_string());
                                            log::info!("Backend system using device: {}", device_name);
                                            thread_state.set_current_device(Some(device_name));
                                        }
                                        result
                                    }
                                    None => {
                                        // Backend system not configured, use legacy CPAL path
                                        log::info!("Backend system not configured, using legacy CPAL path");

                                        // Get the audio device via CPAL
                                        let device = if let Some(ref name) = *current_device_name {
                                            log::info!("Looking for audio device: {}", name);
                                            let found = host.output_devices()
                                                .ok()
                                                .and_then(|mut devices| {
                                                    devices.find(|d| d.name().ok().as_ref() == Some(name))
                                                });

                                            match found {
                                                Some(d) if is_device_valid(&d) => {
                                                    log::info!("Found and validated device: {}", name);
                                                    Some(d)
                                                }
                                                Some(_) => {
                                                    log::warn!("Device '{}' found but has no valid output configs, using default", name);
                                                    host.default_output_device()
                                                }
                                                None => {
                                                    log::warn!("Device '{}' not found, using default", name);
                                                    host.default_output_device()
                                                }
                                            }
                                        } else {
                                            log::info!("Using default audio device");
                                            host.default_output_device()
                                        };

                                        let Some(device) = device else {
                                            log::error!("No audio output device available");
                                            thread_state.set_current_device(None);
                                            thread_state.set_stream_error(true);
                                            return;
                                        };

                                        // Set current device name
                                        if let Ok(name) = device.name() {
                                            log::info!("Using audio device: {}", name);
                                            thread_state.set_current_device(Some(name));
                                        }

                                        create_output_stream_with_config(
                                            &device,
                                            sample_rate,
                                            channels,
                                            dac_passthrough,
                                        ).map(|(stream, handle)| StreamType::Rodio(stream, handle))
                                    }
                                }
                            } else {
                                // Failed to lock settings, use legacy path with CPAL device search
                                let device = if let Some(ref name) = *current_device_name {
                                    log::info!("Looking for audio device: {}", name);
                                    host.output_devices()
                                        .ok()
                                        .and_then(|mut devices| {
                                            devices.find(|d| d.name().ok().as_ref() == Some(name))
                                        })
                                        .or_else(|| {
                                            log::warn!("Device '{}' not found, using default", name);
                                            host.default_output_device()
                                        })
                                } else {
                                    host.default_output_device()
                                };

                                let Some(device) = device else {
                                    log::error!("No audio output device available");
                                    thread_state.set_current_device(None);
                                    thread_state.set_stream_error(true);
                                    return;
                                };

                                if let Ok(name) = device.name() {
                                    thread_state.set_current_device(Some(name));
                                }

                                create_output_stream_with_config(
                                    &device,
                                    sample_rate,
                                    channels,
                                    dac_passthrough,
                                ).map(|(stream, handle)| StreamType::Rodio(stream, handle))
                            };

                            // Handle stream creation result
                            match stream_result {
                                Ok(stream) => {
                                    *stream_opt = Some(stream);
                                    *current_sample_rate = Some(sample_rate);
                                    *current_channels = Some(channels);
                                    thread_state.set_stream_error(false);

                                    // Set current device name from settings (for backend system)
                                    if let Some(settings) = thread_settings.lock().ok() {
                                        if let Some(ref device_name) = settings.output_device {
                                            thread_state.set_current_device(Some(device_name.clone()));
                                            log::info!("Audio stream ready at {}Hz on device: {}", sample_rate, device_name);
                                        } else {
                                            thread_state.set_current_device(Some("Default".to_string()));
                                            log::info!("Audio stream ready at {}Hz on default device", sample_rate);
                                        }
                                    } else {
                                        log::info!("Audio stream ready at {}Hz", sample_rate);
                                    }

                                    // Delay to ensure stream is fully initialized before decoder starts
                                    // This prevents sync gaps and allows hardware to stabilize after sample rate changes
                                    // Extra time needed for large sample rate changes (e.g., 88.2kHz → 44.1kHz)
                                    std::thread::sleep(Duration::from_millis(150));
                                }
                                Err(e) => {
                                    log::error!("❌ Failed to create stream at {}Hz: {}", sample_rate, e);
                                    thread_state.set_stream_error(true);
                                    thread_state.set_current_device(None);
                                    return;
                                }
                            }
                        } else if format_changed {
                            // Format changed but DAC passthrough is disabled - reuse existing stream
                            log::info!(
                                "Audio format changed from {:?}Hz/{:?}ch to {}Hz/{}ch - reusing OutputStream (DAC passthrough disabled, gapless enabled)",
                                *current_sample_rate,
                                *current_channels,
                                sample_rate,
                                channels
                            );
                        }

                        let Some(ref stream) = *stream_opt else {
                            log::error!("Audio thread: no audio device available");
                            return;
                        };

                        // Stop previous engine and wait for sink to release resources
                        if let Some(engine) = current_engine.take() {
                            engine.stop();
                            // Small delay to allow the audio sink to fully release its
                            // reference to the OutputStreamHandle before creating a new sink.
                            // This prevents "resource busy" errors on rapid track switches.
                            std::thread::sleep(Duration::from_millis(50));
                        }

                        *current_audio_data = Some(data.clone());
                        *current_streaming_source = None; // Clear streaming source for non-streaming playback

                        // Create PlaybackEngine from StreamType
                        let mut engine = match stream {
                            StreamType::Rodio(_stream, handle) => {
                                match PlaybackEngine::new_rodio(handle) {
                                    Ok(e) => {
                                        *consecutive_sink_failures = 0;
                                        thread_state.set_stream_error(false);
                                        e
                                    }
                                    Err(e) => {
                                        *consecutive_sink_failures += 1;
                                        log::error!(
                                            "Failed to create engine (attempt {}): {}",
                                            *consecutive_sink_failures,
                                            e
                                        );

                                        if *consecutive_sink_failures >= MAX_SINK_FAILURES {
                                            log::warn!(
                                                "Audio stream appears broken after {} failures. Auto-reinitializing...",
                                                *consecutive_sink_failures
                                            );
                                            thread_state.set_stream_error(true);

                                            drop(stream_opt.take());
                                            std::thread::sleep(Duration::from_millis(200));

                                            // Use last known sample rate/channels to maintain DAC passthrough
                                            let sr = current_sample_rate.unwrap_or(48000);
                                            let ch = current_channels.unwrap_or(2);
                                            *stream_opt = init_device(current_device_name, &thread_state, sr, ch);
                                            if stream_opt.is_some() {
                                                log::info!("Audio stream auto-reinitialized successfully at {}Hz", sr);
                                                *consecutive_sink_failures = 0;
                                                thread_state.set_stream_error(false);
                                            } else {
                                                log::error!("Auto-reinit failed. Audio device unavailable.");
                                                thread_state.is_playing.store(false, Ordering::SeqCst);
                                                thread_state.set_current_device(None);
                                            }
                                        }
                                        return;
                                    }
                                }
                            }
                            #[cfg(target_os = "linux")]
                            StreamType::AlsaDirect(alsa_stream) => {
                                *consecutive_sink_failures = 0;
                                thread_state.set_stream_error(false);
                                let hardware_volume = thread_settings
                                    .lock()
                                    .ok()
                                    .map(|s| s.alsa_hardware_volume)
                                    .unwrap_or(false);
                                PlaybackEngine::new_alsa_direct(alsa_stream.clone(), hardware_volume)
                            }
                        };

                        let volume = thread_state.volume.load(Ordering::SeqCst) as f32 / 100.0;
                        engine.set_volume(volume);

                        let source = match decode_with_fallback(&data) {
                            Ok(s) => s,
                            Err(e) => {
                                log::error!("Failed to decode audio: {}", e);
                                return;
                            }
                        };

                        let actual_duration = source
                            .total_duration()
                            .map(|d| d.as_secs())
                            .unwrap_or(duration_secs);
                        thread_state.duration.store(actual_duration, Ordering::SeqCst);

                        // Calculate normalization gain if enabled
                        let norm_settings = thread_settings
                            .lock()
                            .ok()
                            .filter(|s| s.normalization_enabled)
                            .map(|s| s.normalization_target_lufs);

                        let (normalization, gain_atomic) = if let Some(target_lufs) = norm_settings {
                            // Check for ReplayGain metadata first (initial gain hint)
                            let rg_gain = extract_replaygain(&data).map(|rg| calculate_gain_factor(&rg, target_lufs));

                            // Create shared atomic for dynamic normalization
                            let atomic = Arc::new(AtomicU32::new(
                                rg_gain.unwrap_or(1.0).to_bits()
                            ));

                            // Check loudness cache for pre-computed EBU R128 gain
                            if let Some(cached) = loudness_cache.get(track_id) {
                                let cached_gain = db_to_linear(cached.gain_db.min(6.0));
                                atomic.store(cached_gain.to_bits(), Ordering::Relaxed);
                                log::info!("Normalization: cache hit for track {}, gain {:.4}", track_id, cached_gain);
                            }

                            // Notify analyzer of new track
                            let _ = analyzer_tx.try_send(AnalyzerMessage::NewTrack {
                                track_id,
                                sample_rate,
                                channels,
                                target_lufs,
                                gain_atomic: atomic.clone(),
                            });

                            (rg_gain, Some(atomic))
                        } else {
                            (None, None)
                        };

                        *current_normalization_gain = normalization;
                        *current_gain_atomic = gain_atomic.clone();
                        thread_state.set_normalization_gain(normalization);

                        // Wrap source with diagnostic, normalization, and visualizer
                        let source = wrap_source(source, normalization, gain_atomic, &analyzer_tx, &analyzer_enabled);
                        if let Err(e) = engine.append(source) {
                            log::error!("Failed to append source to engine: {}", e);
                            return;
                        }

                        thread_state.is_playing.store(true, Ordering::SeqCst);
                        thread_state.position.store(0, Ordering::SeqCst);
                        thread_state.current_track_id.store(track_id, Ordering::SeqCst);
                        thread_state.start_playback_timer(0);

                        *current_engine = Some(engine);
                        log::info!(
                            "Audio thread: playback started, duration: {}s, normalization: {}",
                            actual_duration,
                            normalization.map(|g| format!("{:.4}x", g)).unwrap_or_else(|| "off".to_string())
                        );
                    }
                    AudioCommand::PlayStreaming { source, track_id, sample_rate, channels, duration_secs } => {
                        log::info!(
                            "Audio thread: starting streaming playback for track {} ({}Hz, {} channels, {}s)",
                            track_id,
                            sample_rate,
                            channels,
                            duration_secs
                        );
                        *pause_suspend_deadline = None;

                        // Store streaming source for resume capability
                        // When download completes, we can extract the data for resume
                        *current_streaming_source = Some(source.clone());
                        *current_audio_data = None; // Clear regular audio data

                        // Get DAC passthrough setting
                        let dac_passthrough = thread_settings
                            .lock()
                            .ok()
                            .map(|s| s.dac_passthrough)
                            .unwrap_or(false);

                        // Check if we need to recreate the stream
                        let format_changed = *current_sample_rate != Some(sample_rate)
                            || *current_channels != Some(channels);

                        let using_alsa_direct = thread_settings
                            .lock()
                            .ok()
                            .and_then(|s| s.backend_type)
                            .map(|b| b == AudioBackendType::Alsa)
                            .unwrap_or(false);

                        let needs_new_stream = stream_opt.is_none()
                            || (dac_passthrough && format_changed)
                            || (using_alsa_direct && format_changed);

                        if needs_new_stream {
                            if stream_opt.is_some() {
                                if (dac_passthrough || using_alsa_direct) && format_changed {
                                    let mode = if using_alsa_direct { "ALSA Direct" } else { "DAC passthrough" };
                                    log::info!(
                                        "Streaming: Sample rate/channels changed to {}Hz/{}ch - recreating OutputStream ({})",
                                        sample_rate,
                                        channels,
                                        mode
                                    );
                                }
                                drop(stream_opt.take());
                            }

                            let stream_result = if let Some(settings) = thread_settings.lock().ok() {
                                match try_init_stream_with_backend(&settings, sample_rate, channels) {
                                    Some(result) => {
                                        // Set device name from settings for backend system
                                        if result.is_ok() {
                                            let device_name = settings.output_device.clone()
                                                .unwrap_or_else(|| "Default".to_string());
                                            log::info!("Streaming backend using device: {}", device_name);
                                            thread_state.set_current_device(Some(device_name));
                                        }
                                        result
                                    }
                                    None => {
                                        log::info!("Backend system not configured, using legacy CPAL path");
                                        let device = if let Some(ref name) = *current_device_name {
                                            host.output_devices()
                                                .ok()
                                                .and_then(|mut devices| {
                                                    devices.find(|d| d.name().ok().as_ref() == Some(name))
                                                })
                                                .or_else(|| host.default_output_device())
                                        } else {
                                            host.default_output_device()
                                        };

                                        let Some(device) = device else {
                                            log::error!("No audio output device available for streaming");
                                            thread_state.set_stream_error(true);
                                            return;
                                        };

                                        if let Ok(name) = device.name() {
                                            thread_state.set_current_device(Some(name));
                                        }

                                        create_output_stream_with_config(
                                            &device,
                                            sample_rate,
                                            channels,
                                            dac_passthrough,
                                        ).map(|(stream, handle)| StreamType::Rodio(stream, handle))
                                    }
                                }
                            } else {
                                let device = host.default_output_device();
                                let Some(device) = device else {
                                    log::error!("No audio output device available for streaming");
                                    thread_state.set_stream_error(true);
                                    return;
                                };
                                create_output_stream_with_config(&device, sample_rate, channels, dac_passthrough)
                                    .map(|(stream, handle)| StreamType::Rodio(stream, handle))
                            };

                            match stream_result {
                                Ok(stream) => {
                                    *stream_opt = Some(stream);
                                    *current_sample_rate = Some(sample_rate);
                                    *current_channels = Some(channels);
                                    thread_state.set_stream_error(false);
                                    log::info!("Streaming audio stream ready at {}Hz", sample_rate);
                                    std::thread::sleep(Duration::from_millis(150));
                                }
                                Err(e) => {
                                    log::error!("❌ Failed to create stream for streaming at {}Hz: {}", sample_rate, e);
                                    thread_state.set_stream_error(true);
                                    return;
                                }
                            }
                        }

                        let Some(ref stream) = *stream_opt else {
                            log::error!("Audio thread: no audio device available for streaming");
                            return;
                        };

                        // Stop previous engine
                        if let Some(engine) = current_engine.take() {
                            engine.stop();
                            std::thread::sleep(Duration::from_millis(50));
                        }

                        // Create PlaybackEngine
                        let mut engine = match stream {
                            StreamType::Rodio(_stream, handle) => {
                                match PlaybackEngine::new_rodio(handle) {
                                    Ok(e) => {
                                        *consecutive_sink_failures = 0;
                                        thread_state.set_stream_error(false);
                                        e
                                    }
                                    Err(e) => {
                                        log::error!("Failed to create engine for streaming: {}", e);
                                        return;
                                    }
                                }
                            }
                            #[cfg(target_os = "linux")]
                            StreamType::AlsaDirect(alsa_stream) => {
                                let hardware_volume = thread_settings
                                    .lock()
                                    .ok()
                                    .map(|s| s.alsa_hardware_volume)
                                    .unwrap_or(false);
                                PlaybackEngine::new_alsa_direct(alsa_stream.clone(), hardware_volume)
                            }
                        };

                        let volume = thread_state.volume.load(Ordering::SeqCst) as f32 / 100.0;
                        engine.set_volume(volume);

                        // Wait for minimum buffer before starting playback
                        log::info!("Streaming: waiting for initial buffer...");
                        let start_wait = Instant::now();
                        let max_wait = Duration::from_secs(30);

                        while !source.has_min_buffer() && start_wait.elapsed() < max_wait {
                            std::thread::sleep(Duration::from_millis(50));
                        }

                        if !source.has_min_buffer() {
                            log::error!("Streaming: timeout waiting for initial buffer");
                            return;
                        }

                        let buffer_wait_ms = start_wait.elapsed().as_millis();
                        log::info!(
                            "Streaming: initial buffer ready in {}ms, creating incremental decoder...",
                            buffer_wait_ms
                        );

                        // Create incremental streaming source - this starts playback IMMEDIATELY
                        // while continuing to decode/download in background
                        let incremental_source = match IncrementalStreamingSource::new(source.clone()) {
                            Ok(s) => s,
                            Err(e) => {
                                log::error!("Failed to create incremental streaming source: {}", e);
                                return;
                            }
                        };

                        // Verify sample rate/channels match what we expected
                        let actual_sr = incremental_source.get_sample_rate();
                        let actual_ch = incremental_source.get_channels();
                        if actual_sr != sample_rate || actual_ch != channels {
                            log::warn!(
                                "Streaming: detected format {}Hz/{}ch differs from expected {}Hz/{}ch",
                                actual_sr, actual_ch, sample_rate, channels
                            );
                        }

                        // Set duration from track metadata (passed from frontend)
                        // This allows the seekbar to show progress even during streaming
                        thread_state.duration.store(duration_secs, Ordering::SeqCst);

                        // Normalization for streaming: try ReplayGain from buffered data,
                        // then fall back to real-time EBU R128 analysis
                        let norm_settings = thread_settings
                            .lock()
                            .ok()
                            .filter(|s| s.normalization_enabled)
                            .map(|s| s.normalization_target_lufs);

                        let (normalization, gain_atomic) = if let Some(target_lufs) = norm_settings {
                            // Try ReplayGain metadata from buffered data
                            let rg_gain = source.get_buffered_data().and_then(|data| {
                                extract_replaygain(&data).map(|rg| calculate_gain_factor(&rg, target_lufs))
                            });

                            // Create shared atomic for dynamic normalization
                            let atomic = Arc::new(AtomicU32::new(
                                rg_gain.unwrap_or(1.0).to_bits()
                            ));

                            // Check loudness cache
                            if let Some(cached) = loudness_cache.get(track_id) {
                                let cached_gain = db_to_linear(cached.gain_db.min(6.0));
                                atomic.store(cached_gain.to_bits(), Ordering::Relaxed);
                                log::info!("Streaming normalization: cache hit for track {}, gain {:.4}", track_id, cached_gain);
                            }

                            // Notify analyzer of new track
                            let _ = analyzer_tx.try_send(AnalyzerMessage::NewTrack {
                                track_id,
                                sample_rate,
                                channels,
                                target_lufs,
                                gain_atomic: atomic.clone(),
                            });

                            (rg_gain, Some(atomic))
                        } else {
                            (None, None)
                        };

                        *current_normalization_gain = normalization;
                        *current_gain_atomic = gain_atomic.clone();
                        thread_state.set_normalization_gain(normalization);

                        // Box the incremental source to match the expected type
                        let source_to_play: Box<dyn Source<Item = f32> + Send> = Box::new(incremental_source);
                        // Wrap source with diagnostic, normalization, and visualizer
                        let source_to_play = wrap_source(source_to_play, normalization, gain_atomic, &analyzer_tx, &analyzer_enabled);
                        if let Err(e) = engine.append(source_to_play) {
                            log::error!("Failed to append streaming source to engine: {}", e);
                            return;
                        }

                        thread_state.is_playing.store(true, Ordering::SeqCst);
                        thread_state.position.store(0, Ordering::SeqCst);
                        thread_state.current_track_id.store(track_id, Ordering::SeqCst);
                        thread_state.start_playback_timer(0);

                        *current_engine = Some(engine);
                        log::info!(
                            "Audio thread: streaming playback STARTED in {}ms (incremental decode active)",
                            start_wait.elapsed().as_millis()
                        );
                    }
                    AudioCommand::Pause => {
                        if let Some(ref engine) = *current_engine {
                            engine.pause();
                            thread_state.pause_playback_timer();
                            thread_state.is_playing.store(false, Ordering::SeqCst);
                            *pause_suspend_deadline =
                                Some(Instant::now() + Duration::from_millis(PAUSE_SUSPEND_DELAY_MS));
                            log::info!(
                                "Audio thread: paused at {}s",
                                thread_state.position.load(Ordering::SeqCst)
                            );
                        }
                    }
                    AudioCommand::Resume => {
                        *pause_suspend_deadline = None;
                        if current_engine.is_none() {
                            // Try to get audio data from regular storage or streaming source
                            let audio_data: Vec<u8> = if let Some(ref data) = *current_audio_data {
                                data.clone()
                            } else if let Some(ref streaming_src) = *current_streaming_source {
                                // Try to get complete data from streaming source
                                if streaming_src.is_complete() {
                                    match streaming_src.take_complete_data() {
                                        Some(data) => {
                                            log::info!("Resume: using complete streaming data ({} bytes)", data.len());
                                            // Store it in current_audio_data for future use
                                            *current_audio_data = Some(data.clone());
                                            data
                                        }
                                        None => {
                                            log::warn!("Audio thread: cannot resume - streaming source complete but data unavailable");
                                            return;
                                        }
                                    }
                                } else {
                                    log::warn!("Audio thread: cannot resume - streaming not complete yet ({} bytes buffered)",
                                        streaming_src.buffer_size());
                                    return;
                                }
                            } else {
                                log::warn!("Audio thread: cannot resume - no audio data available");
                                return;
                            };

                            if stream_opt.is_none() {
                                // Use last known sample rate/channels to maintain DAC passthrough
                                let sr = current_sample_rate.unwrap_or(48000);
                                let ch = current_channels.unwrap_or(2);
                                log::info!("Resume: reinitializing stream at {}Hz/{}ch", sr, ch);
                                *stream_opt = init_device(current_device_name, &thread_state, sr, ch);
                            }

                            let Some(ref stream) = *stream_opt else {
                                log::error!("Audio thread: cannot resume - no audio device available");
                                return;
                            };

                            let mut engine = match stream {
                                StreamType::Rodio(_stream, handle) => {
                                    match PlaybackEngine::new_rodio(handle) {
                                        Ok(e) => e,
                                        Err(e) => {
                                            log::error!("Failed to create engine for resume: {}", e);
                                            return;
                                        }
                                    }
                                }
                                #[cfg(target_os = "linux")]
                                StreamType::AlsaDirect(alsa_stream) => {
                                    let hardware_volume = thread_settings
                                        .lock()
                                        .ok()
                                        .map(|s| s.alsa_hardware_volume)
                                        .unwrap_or(false);
                                    PlaybackEngine::new_alsa_direct(alsa_stream.clone(), hardware_volume)
                                }
                            };

                            let volume = thread_state.volume.load(Ordering::SeqCst) as f32 / 100.0;
                            engine.set_volume(volume);

                            let source = match decode_with_fallback(&audio_data) {
                                Ok(s) => s,
                                Err(e) => {
                                    log::error!("Failed to decode audio for resume: {}", e);
                                    return;
                                }
                            };

                            let resume_pos = thread_state.position.load(Ordering::SeqCst);
                            let skipped_source: Box<dyn Source<Item = f32> + Send> = if resume_pos > 0 {
                                Box::new(source.skip_duration(Duration::from_secs(resume_pos)))
                            } else {
                                source
                            };

                            // Wrap source with diagnostic, normalization, and visualizer
                            // Reuse the gain + atomic from the original Play
                            let skipped_source = wrap_source(skipped_source, *current_normalization_gain, current_gain_atomic.clone(), &analyzer_tx, &analyzer_enabled);
                            if let Err(e) = engine.append(skipped_source) {
                                log::error!("Failed to append source for resume: {}", e);
                                return;
                            }
                            thread_state.start_playback_timer(resume_pos);
                            thread_state.is_playing.store(true, Ordering::SeqCst);
                            *current_engine = Some(engine);

                            log::info!("Audio thread: resumed from {}s", resume_pos);
                            return;
                        }

                        if let Some(ref engine) = *current_engine {
                            engine.play();
                            let current_pos = thread_state.position.load(Ordering::SeqCst);
                            thread_state.start_playback_timer(current_pos);
                            thread_state.is_playing.store(true, Ordering::SeqCst);
                            log::info!("Audio thread: resumed");
                        }
                    }
                    AudioCommand::Stop => {
                        if let Some(engine) = current_engine.take() {
                            engine.stop();
                        }
                        *current_audio_data = None;
                        *current_streaming_source = None;
                        *current_normalization_gain = None;
                        *current_gain_atomic = None;
                        *gapless_pending = None;
                        thread_state.set_gapless_ready(false);
                        thread_state.set_gapless_next_track_id(0);
                        analyzer_enabled.store(false, Ordering::SeqCst);
                        thread_state.set_normalization_gain(None);
                        thread_state.is_playing.store(false, Ordering::SeqCst);
                        thread_state.position.store(0, Ordering::SeqCst);
                        thread_state.playback_start_millis.store(0, Ordering::SeqCst);
                        thread_state.position_at_start.store(0, Ordering::SeqCst);
                        // Drop the stream to release the device and stop background CPU use.
                        drop(stream_opt.take());
                        *pause_suspend_deadline = None;
                        log::info!("Audio thread: stopped");
                    }
                    AudioCommand::SetVolume(volume) => {
                        thread_state
                            .volume
                            .store((volume * 100.0) as u64, Ordering::SeqCst);
                        if let Some(ref engine) = *current_engine {
                            engine.set_volume(volume);
                        }
                        log::info!("Audio thread: volume set to {}", volume);
                    }
                    AudioCommand::Seek(position_secs) => {
                        *pause_suspend_deadline = None;
                        // Cancel any pending gapless — seek creates a new engine
                        *gapless_pending = None;
                        thread_state.set_gapless_ready(false);
                        thread_state.set_gapless_next_track_id(0);
                        let Some(ref audio_data) = *current_audio_data else {
                            log::warn!("Audio thread: cannot seek - no audio data available");
                            return;
                        };

                        let Some(ref stream) = *stream_opt else {
                            log::error!("Audio thread: cannot seek - no audio device available");
                            return;
                        };

                        log::info!("Audio thread: seeking to {}s", position_secs);

                        if let Some(engine) = current_engine.take() {
                            engine.stop();
                        }

                        let mut engine = match stream {
                            StreamType::Rodio(_stream, handle) => {
                                match PlaybackEngine::new_rodio(handle) {
                                    Ok(e) => e,
                                    Err(e) => {
                                        log::error!("Failed to create rodio engine for seek: {}", e);
                                        return;
                                    }
                                }
                            }
                            #[cfg(target_os = "linux")]
                            StreamType::AlsaDirect(alsa_stream) => {
                                let hardware_volume = thread_settings
                                    .lock()
                                    .ok()
                                    .map(|s| s.alsa_hardware_volume)
                                    .unwrap_or(false);
                                PlaybackEngine::new_alsa_direct(alsa_stream.clone(), hardware_volume)
                            }
                        };

                        let volume = thread_state.volume.load(Ordering::SeqCst) as f32 / 100.0;
                        engine.set_volume(volume);

                        let source = match decode_with_fallback(audio_data) {
                            Ok(s) => s,
                            Err(e) => {
                                log::error!("Failed to decode audio for seek: {}", e);
                                return;
                            }
                        };

                        let skip_duration = Duration::from_secs(position_secs);
                        let skipped_source: Box<dyn Source<Item = f32> + Send> = Box::new(source.skip_duration(skip_duration));

                        // Send Reset to analyzer (seek invalidates accumulated samples)
                        let _ = analyzer_tx.try_send(AnalyzerMessage::Reset);

                        // Wrap source with diagnostic, normalization, and visualizer
                        // Reuse the gain + atomic from the current track
                        let skipped_source = wrap_source(skipped_source, *current_normalization_gain, current_gain_atomic.clone(), &analyzer_tx, &analyzer_enabled);
                        if let Err(e) = engine.append(skipped_source) {
                            log::error!("Failed to append source for seek: {}", e);
                            return;
                        }

                        let was_playing = thread_state.is_playing.load(Ordering::SeqCst);
                        if !was_playing {
                            engine.pause();
                        }

                        thread_state.position.store(position_secs, Ordering::SeqCst);
                        if was_playing {
                            thread_state.start_playback_timer(position_secs);
                        }

                        *current_engine = Some(engine);
                        log::info!(
                            "Audio thread: seeked to {}s (was_playing: {})",
                            position_secs,
                            was_playing
                        );
                    }
                    AudioCommand::ReinitDevice { device_name: new_device } => {
                        log::info!(
                            "Audio thread: reinitializing device (new: {:?})",
                            new_device
                        );
                        *pause_suspend_deadline = None;

                        if let Some(engine) = current_engine.take() {
                            engine.stop();
                        }

                        drop(stream_opt.take());
                        log::info!("Audio thread: previous stream dropped, device released");

                        std::thread::sleep(Duration::from_millis(100));

                        *current_device_name = new_device;
                        // Use last known sample rate/channels to maintain DAC passthrough
                        let sr = current_sample_rate.unwrap_or(48000);
                        let ch = current_channels.unwrap_or(2);
                        log::info!("ReinitDevice: reinitializing at {}Hz/{}ch", sr, ch);
                        *stream_opt = init_device(current_device_name, &thread_state, sr, ch);

                        if stream_opt.is_some() {
                            log::info!("Audio thread: device reinitialized successfully");
                            *consecutive_sink_failures = 0;
                        } else {
                            log::error!("Audio thread: failed to reinitialize device");
                        }

                        // Preserve position so Resume can seek back to it.
                        // pause_playback_timer() captures the real-time position
                        // into thread_state.position before clearing the timer.
                        thread_state.pause_playback_timer();
                        thread_state.is_playing.store(false, Ordering::SeqCst);
                        // Keep current_audio_data and current_streaming_source
                        // intact so Resume can recreate the engine and seek.
                    }
                    AudioCommand::PlayNext { data, track_id, sample_rate, channels } => {
                        // Gapless: append next track to existing Rodio Sink
                        let engine = match current_engine.as_mut() {
                            Some(e) => e,
                            None => {
                                log::warn!("Gapless: no engine, ignoring PlayNext for track {}", track_id);
                                return;
                            }
                        };

                        // Only Rodio supports sink queueing
                        if engine.is_alsa_direct() {
                            log::info!("Gapless: ALSA Direct backend, cannot queue — ignoring PlayNext for track {}", track_id);
                            return;
                        }

                        // Verify format compatibility (same sample rate and channels)
                        if let (Some(cur_sr), Some(cur_ch)) = (*current_sample_rate, *current_channels) {
                            if sample_rate != cur_sr || channels != cur_ch {
                                log::info!(
                                    "Gapless: format mismatch (current {}Hz/{}ch vs next {}Hz/{}ch), ignoring PlayNext for track {}",
                                    cur_sr, cur_ch, sample_rate, channels, track_id
                                );
                                return;
                            }
                        }

                        // Don't queue if already streaming
                        if current_streaming_source.is_some() {
                            log::info!("Gapless: streaming source active, ignoring PlayNext for track {}", track_id);
                            return;
                        }

                        // Decode the next track's audio
                        let source = match decode_with_fallback(&data) {
                            Ok(s) => s,
                            Err(e) => {
                                log::error!("Gapless: failed to decode track {}: {}", track_id, e);
                                return;
                            }
                        };

                        let actual_duration = source
                            .total_duration()
                            .map(|d| d.as_secs())
                            .unwrap_or(0);

                        // Calculate normalization for the next track
                        let norm_settings = thread_settings
                            .lock()
                            .ok()
                            .filter(|s| s.normalization_enabled)
                            .map(|s| s.normalization_target_lufs);

                        let (normalization, gain_atomic) = if let Some(target_lufs) = norm_settings {
                            let rg_gain = extract_replaygain(&data).map(|rg| calculate_gain_factor(&rg, target_lufs));
                            let atomic = Arc::new(AtomicU32::new(
                                rg_gain.unwrap_or(1.0).to_bits()
                            ));
                            if let Some(cached) = loudness_cache.get(track_id) {
                                let cached_gain = db_to_linear(cached.gain_db.min(6.0));
                                atomic.store(cached_gain.to_bits(), Ordering::Relaxed);
                            }
                            let _ = analyzer_tx.try_send(AnalyzerMessage::NewTrack {
                                track_id,
                                sample_rate,
                                channels,
                                target_lufs,
                                gain_atomic: atomic.clone(),
                            });
                            (rg_gain, Some(atomic))
                        } else {
                            (None, None)
                        };

                        // Wrap source with normalization/visualizer pipeline
                        let source = wrap_source(source, normalization, gain_atomic, &analyzer_tx, &analyzer_enabled);

                        // Append to existing Sink (gapless queue)
                        if let Err(e) = engine.append(source) {
                            log::error!("Gapless: failed to append track {} to engine: {}", track_id, e);
                            return;
                        }

                        // Store pending gapless data for transition detection
                        *gapless_pending = Some(GaplessPending {
                            track_id,
                            duration_secs: actual_duration,
                            data,
                            normalization_gain: normalization,
                        });
                        thread_state.set_gapless_next_track_id(track_id);
                        thread_state.set_gapless_ready(false); // Request fulfilled

                        log::info!(
                            "Gapless: queued track {} (duration: {}s) for seamless transition",
                            track_id, actual_duration
                        );
                    }
                }
            };

            loop {
                if thread_state.is_playing.load(Ordering::SeqCst) {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(command) => handle_command(
                            command,
                            &mut current_engine,
                            &mut current_audio_data,
                            &mut current_streaming_source,
                            &mut stream_opt,
                            &mut current_device_name,
                            &mut consecutive_sink_failures,
                            &mut pause_suspend_deadline,
                            &mut current_sample_rate,
                            &mut current_channels,
                            &mut current_normalization_gain,
                            &mut current_gain_atomic,
                            &mut gapless_pending,
                        ),
                        Err(RecvTimeoutError::Timeout) => {
                            let now = Instant::now();
                            if now.duration_since(last_empty_check) >= Duration::from_millis(500) {
                                last_empty_check = now;

                                let pos = thread_state.current_position();
                                let dur = thread_state.duration.load(Ordering::SeqCst);

                                // Gapless transition detection: when position exceeds current
                                // track duration, the queued next track has started playing
                                if let Some(ref pending) = gapless_pending {
                                    if dur > 0 && pos >= dur {
                                        log::info!(
                                            "Gapless transition: track {} -> {} (pos {}s >= dur {}s)",
                                            thread_state.current_track_id.load(Ordering::SeqCst),
                                            pending.track_id, pos, dur
                                        );
                                        thread_state.current_track_id.store(pending.track_id, Ordering::SeqCst);
                                        thread_state.duration.store(pending.duration_secs, Ordering::SeqCst);
                                        thread_state.start_playback_timer(0);
                                        current_audio_data = Some(pending.data.clone());
                                        current_normalization_gain = pending.normalization_gain;
                                        thread_state.set_normalization_gain(pending.normalization_gain);
                                        thread_state.set_gapless_next_track_id(0);
                                        gapless_pending = None;
                                    }
                                }

                                // Gapless readiness: signal frontend when ~5s remain
                                // Only when: playing, not streaming, no gapless already pending, gapless enabled
                                let gapless_enabled = thread_settings
                                    .lock()
                                    .ok()
                                    .map(|s| s.gapless_enabled)
                                    .unwrap_or(false);
                                if gapless_enabled
                                    && dur > 0
                                    && pos + 5 >= dur
                                    && gapless_pending.is_none()
                                    && !thread_state.is_gapless_ready()
                                    && thread_state.get_gapless_next_track_id() == 0
                                    && current_streaming_source.is_none()
                                {
                                    log::info!("Gapless: approaching end of track ({}s/{}s), requesting next", pos, dur);
                                    thread_state.set_gapless_ready(true);
                                }

                                // Original: check if ALL sources are done (engine empty)
                                if let Some(ref engine) = current_engine {
                                    if engine.empty()
                                        && thread_state.is_playing.load(Ordering::SeqCst)
                                    {
                                        log::info!("Audio thread: track finished (engine empty)");
                                        thread_state.is_playing.store(false, Ordering::SeqCst);
                                        let duration = thread_state.duration.load(Ordering::SeqCst);
                                        thread_state.position.store(duration, Ordering::SeqCst);
                                        thread_state.playback_start_millis.store(0, Ordering::SeqCst);
                                        // Clear gapless state on track end
                                        thread_state.set_gapless_ready(false);
                                        thread_state.set_gapless_next_track_id(0);
                                        gapless_pending = None;
                                    }
                                }
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            log::info!("Audio thread: channel closed, exiting");
                            break;
                        }
                    }
                } else {
                    if let Some(deadline) = pause_suspend_deadline {
                        if stream_opt.is_some() {
                            let now = Instant::now();
                            if now >= deadline {
                                if let Some(engine) = current_engine.take() {
                                    engine.stop();
                                }
                                drop(stream_opt.take());
                                pause_suspend_deadline = None;
                                log::info!("Audio thread: suspended stream after pause");
                                continue;
                            }

                            let wait = deadline.saturating_duration_since(now);
                            let wait = std::cmp::min(wait, Duration::from_millis(250));
                            match rx.recv_timeout(wait) {
                                Ok(command) => handle_command(
                                    command,
                                    &mut current_engine,
                                    &mut current_audio_data,
                                    &mut current_streaming_source,
                                    &mut stream_opt,
                                    &mut current_device_name,
                                    &mut consecutive_sink_failures,
                                    &mut pause_suspend_deadline,
                                    &mut current_sample_rate,
                                    &mut current_channels,
                                    &mut current_normalization_gain,
                                    &mut current_gain_atomic,
                                    &mut gapless_pending,
                                ),
                                Err(RecvTimeoutError::Timeout) => {}
                                Err(RecvTimeoutError::Disconnected) => {
                                    log::info!("Audio thread: channel closed, exiting");
                                    break;
                                }
                            }
                            continue;
                        }
                        pause_suspend_deadline = None;
                    }

                    match rx.recv() {
                        Ok(command) => handle_command(
                            command,
                            &mut current_engine,
                            &mut current_audio_data,
                            &mut current_streaming_source,
                            &mut stream_opt,
                            &mut current_device_name,
                            &mut consecutive_sink_failures,
                            &mut pause_suspend_deadline,
                            &mut current_sample_rate,
                            &mut current_channels,
                            &mut current_normalization_gain,
                            &mut current_gain_atomic,
                            &mut gapless_pending,
                        ),
                        Err(_) => {
                            log::info!("Audio thread: channel closed, exiting");
                            break;
                        }
                    }
                }
            }
        });

        Self { tx, state, audio_settings: settings, visualizer_tap, diagnostic }
    }

    /// Play a track by ID (downloads audio)
    pub async fn play_track(
        &self,
        client: &QobuzClient,
        track_id: u64,
        quality: Quality,
    ) -> Result<(), String> {
        log::info!("Player: Starting playback for track {} with quality {:?}", track_id, quality);

        // Get the stream URL
        log::info!("Player: Getting stream URL...");
        let stream_url = client
            .get_stream_url_with_fallback(track_id, quality)
            .await
            .map_err(|e| {
                log::error!("Player: Failed to get stream URL: {}", e);
                format!("Failed to get stream URL: {}", e)
            })?;

        log::info!("Player: Got stream URL: {} (format: {})", stream_url.url, stream_url.mime_type);

        // Download the audio data
        log::info!("Player: Starting audio caching...");
        let audio_data = self.download_audio(&stream_url.url).await.map_err(|e| {
            log::error!("Player: Caching failed: {}", e);
            e
        })?;
        log::info!("Player: Cached {} bytes of audio data", audio_data.len());

        // Send to audio thread
        self.play_data(audio_data, track_id)
    }

    /// Play from raw audio data (for cached tracks)
    pub fn play_data(&self, data: Vec<u8>, track_id: u64) -> Result<(), String> {
        log::info!("Player: Playing {} bytes of audio data for track {}", data.len(), track_id);

        // Extract audio metadata (sample rate, channels, bit depth) - fast header-only read
        let meta = extract_audio_metadata_full(&data)
            .map_err(|e| format!("Failed to extract audio metadata: {}", e))?;

        let sample_rate = meta.sample_rate;
        let channels = meta.channels;
        let bit_depth = meta.bit_depth.unwrap_or(16);

        log::info!(
            "Player: Detected audio format - {}Hz, {} channels, {}-bit",
            sample_rate,
            channels,
            bit_depth
        );

        // Update shared state with actual stream quality
        self.state.set_stream_quality(sample_rate, bit_depth);

        self.tx
            .send(AudioCommand::Play {
                data,
                track_id,
                duration_secs: 0, // Will be determined by decoder
                sample_rate,
                channels,
            })
            .map_err(|e| {
                log::error!("Player: Failed to send to audio thread: {}", e);
                format!("Failed to send play command (audio thread may have crashed): {}", e)
            })?;

        log::info!("Player: Playback initiated successfully");
        Ok(())
    }

    /// Queue next track for gapless playback (appends to current Sink without stopping)
    pub fn play_next(&self, data: Vec<u8>, track_id: u64) -> Result<(), String> {
        let meta = extract_audio_metadata_full(&data)
            .map_err(|e| format!("Failed to extract audio metadata for gapless: {}", e))?;

        log::info!(
            "Player: Queueing gapless track {} ({}Hz, {}ch, {} bytes)",
            track_id, meta.sample_rate, meta.channels, data.len()
        );

        self.tx
            .send(AudioCommand::PlayNext {
                data,
                track_id,
                sample_rate: meta.sample_rate,
                channels: meta.channels,
            })
            .map_err(|e| {
                log::error!("Player: Failed to send PlayNext to audio thread: {}", e);
                format!("Failed to send gapless command: {}", e)
            })
    }

    /// Play from streaming source (starts playback before full download)
    /// Returns the BufferWriter so caller can push data as it downloads
    pub fn play_streaming(
        &self,
        track_id: u64,
        sample_rate: u32,
        channels: u16,
        content_length: u64,
        buffer_seconds: u8,
        duration_secs: u64,
    ) -> Result<BufferWriter, String> {
        log::info!(
            "Player: Starting streaming playback for track {} ({}Hz, {}ch, {} bytes total, {}s)",
            track_id,
            sample_rate,
            channels,
            content_length,
            duration_secs
        );

        // Use StreamingConfig::from_seconds for proper buffer sizing
        let config = StreamingConfig::from_seconds(buffer_seconds);

        let (source, writer) = BufferedMediaSource::new(config, Some(content_length));
        let source = Arc::new(source);

        self.tx
            .send(AudioCommand::PlayStreaming {
                source: source.clone(),
                track_id,
                sample_rate,
                channels,
                duration_secs,
            })
            .map_err(|e| {
                log::error!("Player: Failed to send streaming command: {}", e);
                format!("Failed to send streaming play command: {}", e)
            })?;

        log::info!("Player: Streaming playback initiated");
        Ok(writer)
    }

    /// Play from streaming source with dynamic buffer based on measured speed
    /// Returns the BufferWriter so caller can push data as it downloads
    pub fn play_streaming_dynamic(
        &self,
        track_id: u64,
        sample_rate: u32,
        channels: u16,
        bit_depth: u32,
        content_length: u64,
        speed_mbps: f64,
        duration_secs: u64,
    ) -> Result<BufferWriter, String> {
        log::info!(
            "Player: Starting dynamic streaming for track {} ({}Hz, {}ch, {}-bit, {:.2} MB, {:.1} MB/s, {}s)",
            track_id,
            sample_rate,
            channels,
            bit_depth,
            content_length as f64 / (1024.0 * 1024.0),
            speed_mbps,
            duration_secs
        );

        // Update shared state with actual stream quality
        self.state.set_stream_quality(sample_rate, bit_depth);

        // Use StreamingConfig::from_speed_mbps for dynamic buffer sizing
        let config = StreamingConfig::from_speed_mbps(speed_mbps);

        let (source, writer) = BufferedMediaSource::new(config, Some(content_length));
        let source = Arc::new(source);

        self.tx
            .send(AudioCommand::PlayStreaming {
                source: source.clone(),
                track_id,
                sample_rate,
                channels,
                duration_secs,
            })
            .map_err(|e| {
                log::error!("Player: Failed to send streaming command: {}", e);
                format!("Failed to send streaming play command: {}", e)
            })?;

        log::info!("Player: Dynamic streaming playback initiated");
        Ok(writer)
    }

    /// Download audio from URL with timeout
    async fn download_audio(&self, url: &str) -> Result<Vec<u8>, String> {
        use std::time::Duration;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        log::info!("Caching audio from URL...");

        let response = client
            .get(url)
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
            .map_err(|e| format!("Failed to fetch audio: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        log::info!("Response received, reading bytes...");

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read audio bytes: {}", e))?;

        log::info!("Cached {} bytes", bytes.len());
        Ok(bytes.to_vec())
    }

    /// Pause playback
    pub fn pause(&self) -> Result<(), String> {
        self.tx
            .send(AudioCommand::Pause)
            .map_err(|e| format!("Failed to send pause command: {}", e))
    }

    /// Resume playback
    pub fn resume(&self) -> Result<(), String> {
        self.tx
            .send(AudioCommand::Resume)
            .map_err(|e| format!("Failed to send resume command: {}", e))
    }

    /// Stop playback
    pub fn stop(&self) -> Result<(), String> {
        self.tx
            .send(AudioCommand::Stop)
            .map_err(|e| format!("Failed to send stop command: {}", e))
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume(&self, volume: f32) -> Result<(), String> {
        let clamped = volume.clamp(0.0, 1.0);

        // Skip if volume is already at this value (prevents MPRIS/PipeWire feedback loop)
        let current = self.state.volume();
        if (clamped - current).abs() < 0.001 {
            return Ok(());
        }

        self.tx
            .send(AudioCommand::SetVolume(clamped))
            .map_err(|e| format!("Failed to send volume command: {}", e))
    }

    /// Seek to position in seconds
    pub fn seek(&self, position: u64) -> Result<(), String> {
        // Clamp to duration if known
        let duration = self.state.duration();
        let clamped_position = if duration > 0 {
            position.min(duration)
        } else {
            position
        };

        self.tx
            .send(AudioCommand::Seek(clamped_position))
            .map_err(|e| format!("Failed to send seek command: {}", e))
    }

    /// Reinitialize audio device (releases and re-acquires the device)
    /// Use this when changing audio settings like exclusive mode
    pub fn reinit_device(&self, device_name: Option<String>) -> Result<(), String> {
        self.tx
            .send(AudioCommand::ReinitDevice { device_name })
            .map_err(|e| format!("Failed to send reinit command: {}", e))
    }

    /// Reload audio settings from fresh config (e.g., after database update)
    /// Call this before reinit_device() to ensure Player uses latest settings
    pub fn reload_settings(&self, settings: AudioSettings) -> Result<(), String> {
        if let Ok(mut current_settings) = self.audio_settings.lock() {
            *current_settings = settings;
            Ok(())
        } else {
            Err("Failed to lock audio settings".to_string())
        }
    }

    /// Get current playback state with real-time position
    pub fn get_state(&self) -> Result<PlaybackState, String> {
        Ok(PlaybackState {
            is_playing: self.state.is_playing(),
            position: self.state.current_position(),
            duration: self.state.duration(),
            track_id: self.state.current_track_id(),
            volume: self.state.volume(),
        })
    }

    /// Get playback event for emitting to frontend
    pub fn get_playback_event(&self) -> PlaybackEvent {
        let sample_rate = self.state.get_sample_rate();
        let bit_depth = self.state.get_bit_depth();
        PlaybackEvent {
            is_playing: self.state.is_playing(),
            position: self.state.current_position(),
            duration: self.state.duration(),
            track_id: self.state.current_track_id(),
            volume: self.state.volume(),
            sample_rate: if sample_rate > 0 { Some(sample_rate) } else { None },
            bit_depth: if bit_depth > 0 { Some(bit_depth) } else { None },
            shuffle: None,  // Set by caller with access to queue state
            repeat: None,   // Set by caller with access to queue state
            normalization_gain: self.state.get_normalization_gain(),
            gapless_ready: self.state.is_gapless_ready(),
            gapless_next_track_id: self.state.get_gapless_next_track_id(),
        }
    }
}

/// Playback state snapshot
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PlaybackState {
    pub is_playing: bool,
    pub position: u64,
    pub duration: u64,
    pub track_id: u64,
    pub volume: f32,
}
