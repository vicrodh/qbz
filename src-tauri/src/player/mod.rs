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

use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use rodio::{Decoder, OutputStream, Sink, Source};
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
use crate::audio::{AudioBackendType, BackendConfig, BackendManager};
use crate::config::audio_settings::AudioSettings;
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
struct AudioSpecs {
    samples: SamplesBuffer<i16>,
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
    let mut samples: Vec<i16> = Vec::new();

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

                let mut sample_buf = SampleBuffer::<i16>::new(audio_buf.frames() as u64, spec);
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
fn extract_audio_metadata(data: &[u8]) -> Result<(u32, u16), String> {
    // For non-isomp4 files (FLAC, etc.), try rodio's decoder first - it reads headers quickly
    if !is_isomp4(data) {
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            Decoder::new(BufReader::new(Cursor::new(data.to_vec())))
        }));

        if let Ok(Ok(decoder)) = result {
            return Ok((decoder.sample_rate(), decoder.channels()));
        }
    }

    // Fallback to symphonia probe for codec params (no decode needed)
    let source = Box::new(CursorMediaSource::new(data.to_vec())) as Box<dyn MediaSource>;
    let mss = MediaSourceStream::new(source, Default::default());

    let mut hint = Hint::new();
    hint.with_extension("m4a");

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
    let channels = track.codec_params.channels
        .map(|c| c.count() as u16)
        .ok_or_else(|| "No channel info in codec params".to_string())?;

    Ok((sample_rate, channels))
}

fn decode_with_fallback(
    data: &[u8],
) -> Result<Box<dyn Source<Item = i16> + Send>, String> {
    if is_isomp4(data) {
        return decode_with_symphonia(data)
            .map(|specs| {
                log::info!("Decoded audio using symphonia fallback (isomp4)");
                Box::new(specs.samples) as Box<dyn Source<Item = i16> + Send>
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
            log::info!("✅ OutputStream created successfully at {}Hz", sample_rate);
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
                log::info!("🎵 Detected hw: device, using ALSA Direct for bit-perfect playback");

                // Downcast backend to AlsaBackend to access try_create_direct_stream
                if let Some(alsa_backend) = backend.as_any().downcast_ref::<crate::audio::alsa_backend::AlsaBackend>() {
                    if let Some(result) = alsa_backend.try_create_direct_stream(&config) {
                        return Some(result.map(|stream| StreamType::AlsaDirect(Arc::new(stream))));
                    }
                }
            }
        }
    }

    // Fallback to regular rodio stream (PipeWire, Pulse, ALSA via CPAL)
    match backend.create_output_stream(&config) {
        Ok(stream) => {
            log::info!("✅ Stream created via {:?} backend at {}Hz", backend_type, sample_rate);
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
        }
    }

    pub fn set_stream_error(&self, error: bool) {
        self.stream_error.store(error, Ordering::SeqCst);
    }

    pub fn has_stream_error(&self) -> bool {
        self.stream_error.load(Ordering::SeqCst)
    }

    pub fn set_current_device(&self, device: Option<String>) {
        if let Ok(mut d) = self.current_device.write() {
            *d = device;
        }
    }

    pub fn current_device(&self) -> Option<String> {
        self.current_device.read().ok().and_then(|d| d.clone())
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
}

impl Default for Player {
    fn default() -> Self {
        Self::new(None, AudioSettings::default())
    }
}

impl Player {
    /// Create a new player with an optional specific output device and audio settings
    /// If device_name is None, uses the system default device
    pub fn new(device_name: Option<String>, audio_settings: AudioSettings) -> Self {
        let (tx, rx) = mpsc::channel::<AudioCommand>();
        let state = SharedState::new();
        let thread_state = state.clone();

        // Clone settings for thread
        let settings = Arc::new(Mutex::new(audio_settings.clone()));
        let thread_settings = settings.clone();

        // Spawn dedicated audio thread
        thread::spawn(move || {
            log::info!("Audio thread starting...");

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
                                log::info!("Audio output initialized via backend system at {}Hz", sample_rate);
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

            const MAX_INIT_RETRIES: u32 = 5;
            const RETRY_DELAY_MS: u64 = 500;

            let mut current_engine: Option<PlaybackEngine> = None;
            // Store audio data for seeking (we need to re-decode from the beginning)
            let mut current_audio_data: Option<Vec<u8>> = None;
            // Track consecutive sink creation failures to detect broken streams
            let mut consecutive_sink_failures: u32 = 0;
            const MAX_SINK_FAILURES: u32 = 3;
            // Delay dropping the audio stream after pause to reduce CPU usage.
            const PAUSE_SUSPEND_DELAY_MS: u64 = 2000;
            let mut pause_suspend_deadline: Option<Instant> = None;
            let mut last_empty_check = Instant::now();

            log::info!("Audio thread ready and waiting for commands");

            let mut handle_command = |command: AudioCommand,
                                      current_engine: &mut Option<PlaybackEngine>,
                                      current_audio_data: &mut Option<Vec<u8>>,
                                      stream_opt: &mut Option<StreamType>,
                                      current_device_name: &mut Option<String>,
                                      consecutive_sink_failures: &mut u32,
                                      pause_suspend_deadline: &mut Option<Instant>,
                                      current_sample_rate: &mut Option<u32>,
                                      current_channels: &mut Option<u16>| {
                match command {
                    AudioCommand::Play { data, track_id, duration_secs, sample_rate, channels } => {
                        log::info!(
                            "Audio thread: playing track {} ({}Hz, {} channels)",
                            track_id,
                            sample_rate,
                            channels
                        );
                        *pause_suspend_deadline = None;

                        // Get DAC passthrough setting
                        let dac_passthrough = thread_settings
                            .lock()
                            .ok()
                            .map(|s| s.dac_passthrough)
                            .unwrap_or(false);

                        // Check if we need to recreate the stream
                        // Only recreate on format change if DAC passthrough is enabled
                        let format_changed = *current_sample_rate != Some(sample_rate)
                            || *current_channels != Some(channels);
                        let needs_new_stream = stream_opt.is_none()
                            || (dac_passthrough && format_changed);

                        if needs_new_stream {
                            if stream_opt.is_some() {
                                if dac_passthrough && format_changed {
                                    log::info!(
                                        "Sample rate/channels changed from {:?}Hz/{:?}ch to {}Hz/{}ch - recreating OutputStream (DAC passthrough)",
                                        *current_sample_rate,
                                        *current_channels,
                                        sample_rate,
                                        channels
                                    );
                                } else {
                                    log::info!("Creating initial OutputStream");
                                }
                                // Drop old stream
                                drop(stream_opt.take());
                            }

                            log::info!("DAC passthrough enabled: {}", dac_passthrough);

                            // Get the audio device
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

                            // Try backend system first (if configured), then fall back to legacy CPAL
                            let stream_result = if let Some(settings) = thread_settings.lock().ok() {
                                match try_init_stream_with_backend(&settings, sample_rate, channels) {
                                    Some(result) => result,
                                    None => {
                                        // Backend system not configured, use legacy CPAL path
                                        log::info!("Backend system not configured, using legacy CPAL path");
                                        create_output_stream_with_config(
                                            &device,
                                            sample_rate,
                                            channels,
                                            dac_passthrough,
                                        ).map(|(stream, handle)| StreamType::Rodio(stream, handle))
                                    }
                                }
                            } else {
                                // Failed to lock settings, use legacy path
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
                                    log::info!("✅ Audio stream ready at {}Hz", sample_rate);
                                }
                                Err(e) => {
                                    log::error!("❌ Failed to create stream at {}Hz: {}", sample_rate, e);
                                    log::warn!("Attempting fallback to default device config...");

                                    // Try fallback to default config
                                    match OutputStream::try_from_device(&device) {
                                        Ok((stream, handle)) => {
                                            log::warn!("Using default device config as fallback");
                                            *stream_opt = Some(StreamType::Rodio(stream, handle));
                                            *current_sample_rate = Some(sample_rate);
                                            *current_channels = Some(channels);
                                            thread_state.set_stream_error(false);
                                        }
                                        Err(e2) => {
                                            log::error!("Fallback also failed: {}", e2);
                                            thread_state.set_stream_error(true);
                                            thread_state.set_current_device(None);
                                            return;
                                        }
                                    }
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

                        // Stop previous engine
                        if let Some(engine) = current_engine.take() {
                            engine.stop();
                        }

                        *current_audio_data = Some(data.clone());

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
                            "Audio thread: playback started, duration: {}s",
                            actual_duration
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
                            let Some(ref audio_data) = *current_audio_data else {
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

                            let source = match decode_with_fallback(audio_data) {
                                Ok(s) => s,
                                Err(e) => {
                                    log::error!("Failed to decode audio for resume: {}", e);
                                    return;
                                }
                            };

                            let resume_pos = thread_state.position.load(Ordering::SeqCst);
                            let skipped_source: Box<dyn Source<Item = i16> + Send> = if resume_pos > 0 {
                                Box::new(source.skip_duration(Duration::from_secs(resume_pos)))
                            } else {
                                source
                            };

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
                        let skipped_source = source.skip_duration(skip_duration);

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

                        thread_state.is_playing.store(false, Ordering::SeqCst);
                        thread_state.position.store(0, Ordering::SeqCst);
                        thread_state.playback_start_millis.store(0, Ordering::SeqCst);
                        *current_audio_data = None;
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
                            &mut stream_opt,
                            &mut current_device_name,
                            &mut consecutive_sink_failures,
                            &mut pause_suspend_deadline,
                            &mut current_sample_rate,
                            &mut current_channels,
                        ),
                        Err(RecvTimeoutError::Timeout) => {
                            let now = Instant::now();
                            if now.duration_since(last_empty_check) >= Duration::from_millis(500) {
                                last_empty_check = now;
                                if let Some(ref engine) = current_engine {
                                    if engine.empty()
                                        && thread_state.is_playing.load(Ordering::SeqCst)
                                    {
                                        log::info!("Audio thread: track finished (engine empty)");
                                        thread_state.is_playing.store(false, Ordering::SeqCst);
                                        let duration = thread_state.duration.load(Ordering::SeqCst);
                                        thread_state.position.store(duration, Ordering::SeqCst);
                                        thread_state.playback_start_millis.store(0, Ordering::SeqCst);
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
                                    &mut stream_opt,
                                    &mut current_device_name,
                                    &mut consecutive_sink_failures,
                                    &mut pause_suspend_deadline,
                                    &mut current_sample_rate,
                                    &mut current_channels,
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
                            &mut stream_opt,
                            &mut current_device_name,
                            &mut consecutive_sink_failures,
                            &mut pause_suspend_deadline,
                            &mut current_sample_rate,
                            &mut current_channels,
                        ),
                        Err(_) => {
                            log::info!("Audio thread: channel closed, exiting");
                            break;
                        }
                    }
                }
            }
        });

        Self { tx, state, audio_settings: settings }
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
        log::info!("Player: Starting audio download...");
        let audio_data = self.download_audio(&stream_url.url).await.map_err(|e| {
            log::error!("Player: Download failed: {}", e);
            e
        })?;
        log::info!("Player: Downloaded {} bytes of audio data", audio_data.len());

        // Send to audio thread
        self.play_data(audio_data, track_id)
    }

    /// Play from raw audio data (for cached tracks)
    pub fn play_data(&self, data: Vec<u8>, track_id: u64) -> Result<(), String> {
        log::info!("Player: Playing {} bytes of audio data for track {}", data.len(), track_id);

        // Extract audio metadata (sample rate and channels) - fast header-only read
        let (sample_rate, channels) = extract_audio_metadata(&data)
            .map_err(|e| format!("Failed to extract audio metadata: {}", e))?;

        log::info!(
            "Player: Detected audio format - {}Hz, {} channels",
            sample_rate,
            channels
        );

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

    /// Download audio from URL with timeout
    async fn download_audio(&self, url: &str) -> Result<Vec<u8>, String> {
        use std::time::Duration;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        log::info!("Downloading audio from URL...");

        let response = client
            .get(url)
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
            .map_err(|e| format!("Failed to fetch audio: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        log::info!("Download response received, reading bytes...");

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read audio bytes: {}", e))?;

        log::info!("Downloaded {} bytes", bytes.len());
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
        PlaybackEvent {
            is_playing: self.state.is_playing(),
            position: self.state.current_position(),
            duration: self.state.duration(),
            track_id: self.state.current_track_id(),
            volume: self.state.volume(),
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
