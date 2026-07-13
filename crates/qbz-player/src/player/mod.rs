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

pub use streaming_source::{
    max_initial_buffer_bytes, set_max_initial_buffer_bytes, BufferWriter, BufferedMediaSource,
    InMemorySource, IncrementalStreamingSource, StreamingConfig,
};

use rodio::buffer::SamplesBuffer;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::cpal::{
    BufferSize, SampleFormat, StreamConfig, SupportedBufferSize, SupportedStreamConfig,
};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Source};
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

use playback_engine::PlaybackEngine;
use qbz_audio::{
    calculate_gain_factor, db_to_linear, extract_replaygain, AnalyzerMessage, AnalyzerTap,
    AudioBackendType, AudioDiagnostic, AudioSettings, BackendConfig, BackendManager,
    BitPerfectMode, DiagnosticSource, DynamicAmplify, LoudnessAnalyzer, LoudnessCache,
    TappedSource, VisualizerTap,
};
use qbz_models::{AssetOrigin, ExternalStreamAsset, Quality, StreamQualityInfo};
use qbz_qobuz::QobuzClient;

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
        /// Resume offset in seconds (#315). When > 0, the audio thread
        /// waits for enough buffer to cover the offset and pre-skips
        /// decoder output up to that point before engaging audio.
        start_position_secs: u64,
        /// Total content length in bytes. Combined with `duration_secs`
        /// to estimate bytes-per-second when sizing the resume buffer.
        content_length: u64,
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
    /// Release the output device WITHOUT reopening it: drops the active
    /// stream (freeing an exclusive ALSA `hw:` grab + its D-Bus reservation)
    /// and un-suspends / un-forces anything QBZ parked, so PipeWire can
    /// reclaim a device QBZ was holding. User-triggered from settings.
    ReleaseDevice,
    /// Append next track to current engine for gapless playback (Rodio only)
    PlayNext {
        data: Vec<u8>,
        track_id: u64,
        sample_rate: u32,
        channels: u16,
    },
    /// Play a local DSD file via DoP (DSD over PCM) on ALSA direct (DSD plan
    /// Phase 2). The audio thread opens the demuxer + an S32 stream at the
    /// DoP carrier rate and feeds pre-packed words through the DoP engine.
    PlayDsdDop { path: std::path::PathBuf, track_id: u64 },
    /// Play a local DSD file NATIVELY (ALSA DSD_U32, DSD plan Phase 3) —
    /// requires the kernel to grant the device a DSD format (quirk table).
    PlayDsdNative { path: std::path::PathBuf, track_id: u64 },
    /// Queue the next DSD track on the ACTIVE DoP engine (gapless DSD).
    /// Ignored (with gapless_ready reset) when the engine isn't DoP or the
    /// carrier rate differs — the normal track-end advance then handles it.
    PlayNextDsdDop { path: std::path::PathBuf, track_id: u64 },
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
        Self {
            inner: Cursor::new(data),
            len,
        }
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
    samples: SamplesBuffer,
    sample_rate: u32,
    channels: u16,
}

fn cpal_device_name(device: &rodio::cpal::Device) -> Option<String> {
    device
        .description()
        .ok()
        .map(|description| description.name().to_string())
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
        samples: SamplesBuffer::new(
            std::num::NonZero::new(channels).unwrap(),
            std::num::NonZero::new(sample_rate).unwrap(),
            samples,
        ),
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

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| "No sample rate in codec params".to_string())?;

    // ALAC and some other formats don't include channel info in initial codec params
    // Default to stereo (2 channels) which is the most common case
    let channels = track
        .codec_params
        .channels
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

/// True when a cached FLAC is a lower quality than `requested`, so the
/// cache entry should be bypassed and the track re-fetched. Ported from
/// the Tauri `cached_quality_below_requested` helper: an unparseable
/// buffer is assumed compatible.
fn cached_quality_below_requested(data: &[u8], requested: Quality) -> bool {
    let meta = match extract_audio_metadata_full(data) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let sample_rate = meta.sample_rate;
    let bit_depth = meta.bit_depth.unwrap_or(16);
    match requested {
        // Hi-Res+: expect 24-bit AND > 96 kHz.
        Quality::UltraHiRes => bit_depth < 24 || sample_rate <= 96000,
        // Hi-Res: expect 24-bit.
        Quality::HiRes => bit_depth < 24,
        // Lossless / Mp3: any FLAC satisfies the request.
        _ => false,
    }
}

fn decode_with_fallback(data: &[u8]) -> Result<Box<dyn Source<Item = f32> + Send>, String> {
    if is_isomp4(data) {
        return decode_with_symphonia(data).map(|specs| {
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

    // Try mp4 fallback (rodio 0.22 removed Mp4Type hint)
    {
        let attempt = panic::catch_unwind(AssertUnwindSafe(|| {
            Decoder::new_mp4(BufReader::new(Cursor::new(data.to_vec())))
        }));

        match attempt {
            Ok(Ok(decoder)) => {
                log::info!("Decoded audio using mp4 fallback");
                return Ok(Box::new(decoder));
            }
            Ok(Err(err)) => {
                log::warn!("mp4 fallback failed: {}", err);
            }
            Err(_) => {
                log::warn!("mp4 fallback panicked");
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

/// Create MixerDeviceSink with custom sample rate configuration
fn create_output_stream_with_config(
    device: rodio::cpal::Device,
    sample_rate: u32,
    channels: u16,
    exclusive_mode: bool,
) -> Result<MixerDeviceSink, String> {
    log::info!(
        "Creating MixerDeviceSink: {}Hz, {} channels, exclusive: {}",
        sample_rate,
        channels,
        exclusive_mode
    );

    // Create StreamConfig with desired sample rate
    // Note: buffer_size here is unused — with_supported_config() resets it.
    // The actual buffer size is set via with_buffer_size() below.
    let config = StreamConfig {
        channels,
        sample_rate,
        buffer_size: BufferSize::Default,
    };

    // Check if device supports this configuration
    let supported_configs = device
        .supported_output_configs()
        .map_err(|e| format!("Failed to get supported configs: {}", e))?;

    let mut found_matching = false;
    for range in supported_configs {
        if range.channels() == channels
            && sample_rate >= range.min_sample_rate()
            && sample_rate <= range.max_sample_rate()
        {
            found_matching = true;
            log::info!(
                "Device supports {}Hz (range: {}-{}Hz)",
                sample_rate,
                range.min_sample_rate(),
                range.max_sample_rate()
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

    // Compute buffer size — must be applied AFTER with_supported_config()
    // because that method resets buffer_size to Default via ..Default::default().
    // MixerDeviceSink has zero internal buffering, so CPAL's buffer is the
    // ONLY buffer between the mixer and audio hardware.
    let cpal_buffer_size = if exclusive_mode {
        BufferSize::Fixed(512) // Low latency for exclusive mode
    } else {
        // ~100ms buffer, matching old vendored cpal period size.
        // Prevents underruns at high sample rates (192kHz = 19200 frames).
        BufferSize::Fixed(sample_rate / 10)
    };
    log::info!("Buffer size: {:?}", cpal_buffer_size);

    // Create MixerDeviceSink with custom config
    match DeviceSinkBuilder::from_device(device) {
        Ok(builder) => {
            match builder
                .with_supported_config(&supported_config)
                .with_buffer_size(cpal_buffer_size)
                .open_stream()
            {
                Ok(mixer_sink) => {
                    log::info!("MixerDeviceSink created successfully at {}Hz", sample_rate);
                    Ok(mixer_sink)
                }
                Err(e) => {
                    log::error!("Failed to open stream at {}Hz: {}", sample_rate, e);
                    Err(format!("Failed to create output stream: {}", e))
                }
            }
        }
        Err(e) => {
            log::error!("Failed to create device sink builder: {}", e);
            Err(format!("Failed to create output stream: {}", e))
        }
    }
}

/// Output stream type - either rodio or ALSA Direct.
///
/// On macOS, the Rodio variant carries an optional `exclusive_guard`
/// whose `Drop` releases CoreAudio Hog Mode and is otherwise inert —
/// so exclusive and shared modes share a single code path.
enum StreamType {
    Rodio {
        sink: MixerDeviceSink,
        /// Holds CoreAudio Hog Mode for the lifetime of the stream.
        /// Load-bearing via `Drop`; reads happen through pattern matches
        /// (e.g., `set_coreaudio_hardware_volume`).
        #[cfg(target_os = "macos")]
        exclusive_guard: Option<qbz_audio::CoreAudioExclusiveGuard>,
    },
    #[cfg(target_os = "linux")]
    AlsaDirect(Arc<qbz_audio::AlsaDirectStream>),
    /// Native JACK output (#263 Tier 3). QBZ as a JACK client with stable ports;
    /// NOT bit-perfect (resampled to the graph rate).
    #[cfg(target_os = "linux")]
    Jack(Arc<qbz_audio::JackStream>),
}

impl StreamType {
    /// Construct a shared-mode Rodio stream (no exclusive guard).
    fn rodio(sink: MixerDeviceSink) -> Self {
        StreamType::Rodio {
            sink,
            #[cfg(target_os = "macos")]
            exclusive_guard: None,
        }
    }

    /// Apply the volume to CoreAudio hardware if the device supports it.
    ///
    /// Returns `true` only when the hardware accepted the change so the
    /// caller can pin the software stream to unity gain. Returns `false`
    /// for shared mode and for knob-only DACs (no settable volume
    /// property), letting the caller fall back to software volume.
    #[cfg(target_os = "macos")]
    fn set_coreaudio_hardware_volume(&self, volume: f32) -> bool {
        match self {
            StreamType::Rodio {
                exclusive_guard: Some(guard),
                ..
            } => match guard.set_hardware_volume(volume) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!(
                        "[CoreAudio] Hardware volume failed; falling back to software: {}",
                        e
                    );
                    false
                }
            },
            _ => false,
        }
    }

    /// Actual output stream rate. On macOS shared mode this is the rate that
    /// must match CoreAudio's current nominal device rate; decoded track rates
    /// may differ and are resampled by Rodio.
    #[cfg(target_os = "macos")]
    fn output_sample_rate(&self) -> u32 {
        match self {
            StreamType::Rodio { sink, .. } => sink.config().sample_rate().get(),
        }
    }
}

fn apply_engine_volume(
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))] stream_opt: &Option<StreamType>,
    engine: &PlaybackEngine,
    volume: f32,
) {
    #[cfg(target_os = "macos")]
    if stream_opt
        .as_ref()
        .map(|stream| stream.set_coreaudio_hardware_volume(volume))
        .unwrap_or(false)
    {
        engine.set_volume(1.0);
        return;
    }

    engine.set_volume(volume);
}

#[cfg(target_os = "macos")]
fn uses_coreaudio_system_default(settings: &AudioSettings) -> bool {
    settings
        .backend_type
        .unwrap_or(AudioBackendType::SystemDefault)
        == AudioBackendType::SystemDefault
}

/// `evaluate_stream_recreate` runs every track change on the audio thread,
/// and `coreaudio_nominal_rate` issues two CoreAudio HAL queries per call
/// (`resolve_output_device_id` + `get_nominal_sample_rate`). A short-lived
/// cache absorbs the bulk of those repeats — long enough to skip duplicate
/// queries within an album, short enough to notice when the user changes the
/// system audio rate.
#[cfg(target_os = "macos")]
const COREAUDIO_RATE_CACHE_TTL: std::time::Duration = std::time::Duration::from_millis(750);

#[cfg(target_os = "macos")]
struct CachedNominalRate {
    cached_at: std::time::Instant,
    device_name: Option<String>,
    rate: Option<u32>,
}

#[cfg(target_os = "macos")]
std::thread_local! {
    static COREAUDIO_NOMINAL_RATE_CACHE: std::cell::RefCell<Option<CachedNominalRate>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "macos")]
fn query_macos_nominal_rate(device_name: Option<&str>) -> Option<u32> {
    let device_id = qbz_audio::coreaudio_direct::resolve_output_device_id(device_name)
        .inspect_err(|e| {
            log::warn!(
                "[CoreAudio] Failed to resolve output device for rate check: {}",
                e
            )
        })
        .ok()?;

    qbz_audio::coreaudio_direct::get_nominal_sample_rate(device_id)
        .inspect_err(|e| log::warn!("[CoreAudio] Failed to query nominal rate: {}", e))
        .ok()
}

#[cfg(target_os = "macos")]
fn coreaudio_nominal_rate(settings: &AudioSettings) -> Option<u32> {
    if !uses_coreaudio_system_default(settings) {
        return None;
    }

    let device_name = settings.output_device.clone();
    let now = std::time::Instant::now();

    COREAUDIO_NOMINAL_RATE_CACHE.with(|cell| {
        let mut cell = cell.borrow_mut();
        if let Some(cached) = cell.as_ref() {
            if cached.device_name == device_name
                && now.duration_since(cached.cached_at) < COREAUDIO_RATE_CACHE_TTL
            {
                return cached.rate;
            }
        }
        let rate = query_macos_nominal_rate(device_name.as_deref());
        *cell = Some(CachedNominalRate {
            cached_at: now,
            device_name,
            rate,
        });
        rate
    })
}

#[cfg(target_os = "macos")]
fn coreaudio_shared_rate_mismatch(
    settings: &AudioSettings,
    stream_opt: &Option<StreamType>,
) -> Option<(u32, u32)> {
    if settings.exclusive_mode || !uses_coreaudio_system_default(settings) {
        return None;
    }

    let stream_rate = stream_opt.as_ref().map(StreamType::output_sample_rate)?;
    let nominal_rate = coreaudio_nominal_rate(settings)?;

    (stream_rate != nominal_rate).then_some((stream_rate, nominal_rate))
}

/// Inputs needed by both `Play` and `Stream` handlers to decide whether the
/// current output stream must be torn down and recreated.
struct StreamRecreateDecision {
    needs_new_stream: bool,
    format_changed: bool,
    dac_passthrough: bool,
    using_alsa_direct: bool,
    using_coreaudio_exclusive: bool,
    coreaudio_shared_rate_mismatch: Option<(u32, u32)>,
}

/// Read settings once and evaluate every condition that forces a stream
/// rebuild: any decoded-format change (sample rate / channels — the output
/// stream must follow the track's native rate on every backend, #449) and
/// CoreAudio shared-mode rate drift (CPAL caches the device rate at open
/// time, so when the OS nominal rate moves we must rebuild or playback runs
/// at the wrong speed).
///
/// `current_track_sample_rate` / `current_track_channels` describe the last
/// *decoded source* format and are compared to the incoming `sample_rate` /
/// `channels` — never to the output stream's hardware rate, which on macOS
/// shared mode may differ and is tracked separately via Rodio's sink config.
fn evaluate_stream_recreate(
    thread_settings: &Arc<Mutex<AudioSettings>>,
    stream_opt: &Option<StreamType>,
    current_track_sample_rate: Option<u32>,
    current_track_channels: Option<u16>,
    sample_rate: u32,
    channels: u16,
    context: &str,
) -> StreamRecreateDecision {
    let format_changed =
        current_track_sample_rate != Some(sample_rate) || current_track_channels != Some(channels);

    let settings_guard = thread_settings.lock().ok();

    let dac_passthrough = settings_guard
        .as_ref()
        .map(|s| cfg!(target_os = "linux") && s.dac_passthrough)
        .unwrap_or(false);

    let using_alsa_direct = settings_guard
        .as_ref()
        .and_then(|s| s.backend_type)
        .map(|b| b == AudioBackendType::Alsa)
        .unwrap_or(false);

    let using_coreaudio_exclusive = settings_guard
        .as_ref()
        .map(|s| {
            cfg!(target_os = "macos")
                && s.backend_type.unwrap_or(AudioBackendType::SystemDefault)
                    == AudioBackendType::SystemDefault
                && s.exclusive_mode
        })
        .unwrap_or(false);

    #[cfg(target_os = "macos")]
    let coreaudio_shared_rate_mismatch = settings_guard
        .as_ref()
        .and_then(|s| coreaudio_shared_rate_mismatch(s, stream_opt))
        .inspect(|(stream_rate, nominal_rate)| {
            log::warn!(
                "[CoreAudio] {} shared-mode output rate changed: stream {}Hz, device nominal {}Hz. Recreating stream to avoid wrong-speed playback.",
                context,
                stream_rate,
                nominal_rate
            );
        });
    #[cfg(not(target_os = "macos"))]
    let coreaudio_shared_rate_mismatch: Option<(u32, u32)> = {
        let _ = (stream_opt, context);
        None
    };

    drop(settings_guard);

    let needs_new_stream = compute_needs_new_stream(
        stream_opt.is_some(),
        format_changed,
        dac_passthrough,
        using_alsa_direct,
        using_coreaudio_exclusive,
        coreaudio_shared_rate_mismatch.is_some(),
    );

    StreamRecreateDecision {
        needs_new_stream,
        format_changed,
        dac_passthrough,
        using_alsa_direct,
        using_coreaudio_exclusive,
        coreaudio_shared_rate_mismatch,
    }
}

/// Pure decision rule for whether the output stream must be rebuilt.
///
/// Split out so the truth table can be unit-tested without faking a real
/// `MixerDeviceSink` or `AudioSettings` mutex.
fn compute_needs_new_stream(
    has_stream: bool,
    format_changed: bool,
    _dac_passthrough: bool,
    _using_alsa_direct: bool,
    _using_coreaudio_exclusive: bool,
    coreaudio_shared_rate_mismatch: bool,
) -> bool {
    // A decoded-format change (sample rate or channel count) requires a fresh
    // output stream on EVERY backend so the device follows the track's native
    // rate (#449). The bit-perfect flags used to gate this, which was correct
    // only while Stop dropped the stream; once that drop was deferred to avoid
    // a track-change click (e93fcaec), the default/PipeWire path stopped
    // switching rates and stayed locked to the first track. Same-rate tracks
    // keep reusing the stream (format_changed == false), preserving the click
    // fix.
    !has_stream || format_changed || coreaudio_shared_rate_mismatch
}

/// Try to create output stream using the backend system (if configured)
/// Returns None if backend system is not configured (backend_type = None)
///
/// For ALSA backend with hw: devices, may return AlsaDirect instead of Rodio stream.
fn try_init_stream_with_backend(
    audio_settings: &AudioSettings,
    sample_rate: u32,
    channels: u16,
    state: &SharedState,
) -> Option<Result<StreamType, String>> {
    // A None backend_type means "Auto" / unset. Resolve it to SystemDefault on
    // every platform instead of returning None — returning None made the caller
    // fall through to the legacy CPAL path, which forced the track rate onto the
    // shared default device: that froze the seekbar with no audio AND left a
    // process-wide stuck audio handle that survived Reset (#470). "Auto" is
    // resolved to a concrete backend in the UI; this is the backend-side safety
    // net for any remaining None (legacy installs, headless callers).
    let backend_type = audio_settings
        .backend_type
        .unwrap_or(qbz_audio::AudioBackendType::SystemDefault);

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
        pw_force_bitperfect: audio_settings.pw_force_bitperfect,
        skip_sink_switch: audio_settings.skip_sink_switch,
    };

    // For ALSA backend with hw: devices, try direct ALSA first (Linux only)
    #[cfg(target_os = "linux")]
    if backend_type == AudioBackendType::Alsa {
        // Check if device is hw: or plughw:
        if let Some(ref device_id) = config.device_id {
            if qbz_audio::AlsaDirectStream::is_hw_device(device_id) {
                log::info!("Detected hw: device, using ALSA Direct for bit-perfect playback");

                // Downcast backend to AlsaBackend to access try_create_direct_stream
                if let Some(alsa_backend) = backend
                    .as_any()
                    .downcast_ref::<qbz_audio::alsa_backend::AlsaBackend>()
                {
                    if let Some(result) = alsa_backend.try_create_direct_stream(&config) {
                        return Some(result.map(|(stream, mode)| {
                            log::info!("ALSA Direct stream created with mode: {:?}", mode);
                            state.set_bit_perfect_mode(Some(mode));
                            StreamType::AlsaDirect(Arc::new(stream))
                        }));
                    }
                }
            }
        }
    }

    // JACK (#263 Tier 3): create the JACK client/stream directly (not via the
    // MixerDeviceSink trait). Opt-in routing-freedom mode, NOT bit-perfect.
    #[cfg(target_os = "linux")]
    if backend_type == AudioBackendType::Jack {
        match qbz_audio::JackStream::new(config.channels) {
            Ok(stream) => {
                state.set_bit_perfect_mode(Some(qbz_audio::BitPerfectMode::Disabled));
                return Some(Ok(StreamType::Jack(Arc::new(stream))));
            }
            Err(e) => return Some(Err(format!("JACK backend unavailable: {e}"))),
        }
    }

    // Fallback to regular rodio stream (PipeWire, Pulse, ALSA via CPAL)
    match backend.create_output_stream_with_exclusive_guard(&config) {
        Ok((mixer_sink, _exclusive_guard)) => {
            let output_sample_rate = mixer_sink.config().sample_rate().get();
            log::info!(
                "Stream created via {:?} backend (requested {}Hz, output {}Hz)",
                backend_type,
                sample_rate,
                output_sample_rate
            );
            state.set_bit_perfect_mode(Some(BitPerfectMode::Disabled));
            #[cfg(target_os = "macos")]
            let stream = if backend_type == AudioBackendType::SystemDefault {
                StreamType::Rodio {
                    sink: mixer_sink,
                    exclusive_guard: _exclusive_guard,
                }
            } else {
                StreamType::rodio(mixer_sink)
            };
            #[cfg(not(target_os = "macos"))]
            let stream = StreamType::rodio(mixer_sink);
            Some(Ok(stream))
        }
        Err(e) => {
            log::error!("Backend stream creation failed: {}", e);
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
    /// Bit-perfect mode of the current stream. None when no stream is active.
    /// Lets the UI show whether playback is direct-hardware bit-perfect, going
    /// through plughw software resample, or running on a shared system path
    /// (pipewire/pulse/cpal) where bit-perfect is not guaranteed.
    #[serde(default)]
    pub bit_perfect_mode: Option<BitPerfectMode>,
    /// Streaming buffer progress (0.0..1.0). `None` when not streaming or
    /// the track is fully buffered — drives the seek-bar cache overlay.
    #[serde(default)]
    pub buffer_progress: Option<f32>,
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
    /// DSD-direct mode: 0 = none, 1 = DoP, 2 = native DSD_U32_BE,
    /// 3 = native DSD_U32_LE. Non-zero means volume is fixed and seek is
    /// unsupported; the gapless arm uses it to build the matching packing.
    dsd_direct: Arc<std::sync::atomic::AtomicU8>,
    /// True when audio data/source is available for playback or resume
    has_loaded_audio: Arc<AtomicBool>,
    /// Volume (0.0 - 1.0, f32 stored as u32 bits — same idiom as
    /// `normalization_gain`; integer-percent storage quantized the volume
    /// to 1% on every re-apply)
    volume: Arc<AtomicU32>,
    /// Playback start time (Unix timestamp millis when started/resumed)
    playback_start_millis: Arc<AtomicU64>,
    /// Position when playback was started/resumed (in seconds)
    position_at_start: Arc<AtomicU64>,
    /// Current output device name
    current_device: Arc<std::sync::RwLock<Option<String>>>,
    /// Stream error flag (set when ALSA/audio errors are detected)
    stream_error: Arc<AtomicBool>,
    /// Optional user-readable explanation paired with `stream_error`.
    /// Drained by the Tauri polling loop to emit a frontend toast and then
    /// cleared, so the UI fires the notification exactly once per error.
    stream_error_message: Arc<std::sync::RwLock<Option<String>>>,
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
    /// Streaming buffer progress (0.0-1.0 stored as f32 bits, 0 = not streaming)
    buffer_progress: Arc<AtomicU32>,
    /// Current bit-perfect mode encoded as u8 (see `bit_perfect_mode_from_u8`).
    /// 0 = Unknown (no stream active yet), 1 = Disabled (CPAL/Rodio / shared
    /// system path), 2 = DirectHardware (ALSA hw:), 3 = PluginFallback (plughw:).
    bit_perfect_mode: Arc<AtomicU8>,
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
            dsd_direct: Arc::new(std::sync::atomic::AtomicU8::new(0)),
            has_loaded_audio: Arc::new(AtomicBool::new(false)),
            volume: Arc::new(AtomicU32::new(0.75f32.to_bits())),
            playback_start_millis: Arc::new(AtomicU64::new(0)),
            position_at_start: Arc::new(AtomicU64::new(0)),
            current_device: Arc::new(std::sync::RwLock::new(None)),
            stream_error: Arc::new(AtomicBool::new(false)),
            stream_error_message: Arc::new(std::sync::RwLock::new(None)),
            sample_rate: Arc::new(AtomicU32::new(0)),
            bit_depth: Arc::new(AtomicU32::new(0)),
            normalization_gain: Arc::new(AtomicU32::new(0)),
            gapless_ready: Arc::new(AtomicBool::new(false)),
            gapless_next_track_id: Arc::new(AtomicU64::new(0)),
            buffer_progress: Arc::new(AtomicU32::new(0)),
            bit_perfect_mode: Arc::new(AtomicU8::new(0)),
        }
    }

    /// Clearing (`error = false`) also drops any pending
    /// `stream_error_message`. This is intentional: if init recovers before
    /// the Tauri polling loop drains the message, we'd rather swallow the
    /// toast than surface a notification for a transient failure the user
    /// never perceived. The trade-off is that a fast record→clear→drain
    /// sequence loses the message — accepted because a recovered error is
    /// not a user-actionable event.
    /// 0 = none, 1 = DoP, 2 = native BE, 3 = native LE.
    pub fn set_dsd_mode(&self, mode: u8) {
        self.dsd_direct.store(mode, Ordering::SeqCst);
    }

    pub fn dsd_mode(&self) -> u8 {
        self.dsd_direct.load(Ordering::SeqCst)
    }

    pub fn is_dsd_direct(&self) -> bool {
        self.dsd_direct.load(Ordering::SeqCst) != 0
    }

    pub fn set_stream_error(&self, error: bool) {
        self.stream_error.store(error, Ordering::SeqCst);
        if !error {
            if let Ok(mut m) = self.stream_error_message.write() {
                *m = None;
            }
        }
    }

    pub fn has_stream_error(&self) -> bool {
        self.stream_error.load(Ordering::SeqCst)
    }

    /// Record a user-readable error explanation alongside `stream_error=true`.
    /// The message is drained once via `take_stream_error_message` so the UI
    /// fires the toast exactly once per error.
    pub fn record_stream_error(&self, message: impl Into<String>) {
        self.stream_error.store(true, Ordering::SeqCst);
        if let Ok(mut m) = self.stream_error_message.write() {
            *m = Some(message.into());
        }
    }

    /// Atomically take the pending stream-error message (if any). Returns
    /// `None` when no message is pending or has already been read.
    pub fn take_stream_error_message(&self) -> Option<String> {
        self.stream_error_message
            .write()
            .ok()
            .and_then(|mut m| m.take())
    }

    pub fn set_stream_quality(&self, sample_rate: u32, bit_depth: u32) {
        self.sample_rate.store(sample_rate, Ordering::SeqCst);
        self.bit_depth.store(bit_depth, Ordering::SeqCst);
    }

    /// Set the current bit-perfect mode for the active stream.
    /// Pass None when no stream is active (e.g., after stop).
    pub fn set_bit_perfect_mode(&self, mode: Option<BitPerfectMode>) {
        let code = match mode {
            None => 0,
            Some(BitPerfectMode::Disabled) => 1,
            Some(BitPerfectMode::DirectHardware) => 2,
            Some(BitPerfectMode::PluginFallback) => 3,
        };
        self.bit_perfect_mode.store(code, Ordering::SeqCst);
    }

    /// Get the current bit-perfect mode for the active stream.
    /// Returns None when no stream has been initialized yet.
    pub fn get_bit_perfect_mode(&self) -> Option<BitPerfectMode> {
        match self.bit_perfect_mode.load(Ordering::SeqCst) {
            0 => None,
            1 => Some(BitPerfectMode::Disabled),
            2 => Some(BitPerfectMode::DirectHardware),
            3 => Some(BitPerfectMode::PluginFallback),
            _ => None,
        }
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
        if gain == 0.0 {
            None
        } else {
            Some(gain)
        }
    }

    /// Set streaming buffer progress (0.0 to 1.0). Pass 0.0 when not streaming.
    pub fn set_buffer_progress(&self, progress: f32) {
        self.buffer_progress
            .store(progress.to_bits(), Ordering::SeqCst);
    }

    /// Get streaming buffer progress (0.0 to 1.0). Returns None if not streaming.
    pub fn get_buffer_progress(&self) -> Option<f32> {
        let bits = self.buffer_progress.load(Ordering::SeqCst);
        let progress = f32::from_bits(bits);
        if progress <= 0.0 || progress >= 1.0 {
            None
        } else {
            Some(progress)
        }
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

    /// Millisecond-precision companion to [`Self::current_position`] — the
    /// exact same derivation WITHOUT the whole-second truncation. READ-ONLY
    /// state derivation from the existing anchors (`playback_start_millis`
    /// is already epoch-ms; `position_at_start`/`position`/`duration` are
    /// seconds): no stream, seek, format or device path is touched.
    ///
    /// Added for the lyrics sync engine (karaoke needs sub-second
    /// resolution); semantics mirror `current_position` line by line:
    /// paused / no anchor → stored coarse position ×1000; playing →
    /// `position_at_start*1000 + (now_ms - start_millis)`, clamped to
    /// `duration*1000`.
    pub fn current_position_ms(&self) -> u64 {
        if !self.is_playing.load(Ordering::SeqCst) {
            return self.position.load(Ordering::SeqCst).saturating_mul(1000);
        }

        let start_millis = self.playback_start_millis.load(Ordering::SeqCst);
        if start_millis == 0 {
            return self.position.load(Ordering::SeqCst).saturating_mul(1000);
        }

        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let elapsed_ms = now_millis.saturating_sub(start_millis);
        let position_at_start_ms = self
            .position_at_start
            .load(Ordering::SeqCst)
            .saturating_mul(1000);
        let duration_ms = self.duration.load(Ordering::SeqCst).saturating_mul(1000);

        // Clamp to duration (same rule as current_position)
        position_at_start_ms.saturating_add(elapsed_ms).min(duration_ms)
    }

    /// Mark playback as started/resumed at current position
    fn start_playback_timer(&self, position: u64) {
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.playback_start_millis
            .store(now_millis, Ordering::SeqCst);
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

    pub fn set_loaded_audio(&self, loaded: bool) {
        self.has_loaded_audio.store(loaded, Ordering::SeqCst);
    }

    pub fn has_loaded_audio(&self) -> bool {
        self.has_loaded_audio.load(Ordering::SeqCst)
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::SeqCst))
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
    /// Two-level playback cache (L1 memory + optional L2 disk). A track is
    /// cached after its first play so replays start instantly.
    audio_cache: Arc<qbz_cache::AudioCache>,
    /// Monotonic play generation. Bumped on every new play intent so a slow
    /// `play_track` (network/CMAF) that finishes after a newer play cannot
    /// restart the previous track (last-writer race on `AudioCommand::Play`).
    play_generation: AtomicU64,
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
    pub fn new(
        device_name: Option<String>,
        audio_settings: AudioSettings,
        visualizer_tap: Option<VisualizerTap>,
        diagnostic: AudioDiagnostic,
    ) -> Self {
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
                               analyzer_enabled: &Arc<AtomicBool>|
             -> Box<dyn Source<Item = f32> + Send> {
                // Diagnostic tap (innermost — captures raw decoded samples)
                let source: Box<dyn Source<Item = f32> + Send> =
                    Box::new(DiagnosticSource::new(source, thread_diagnostic.clone()));

                // Normalization: dynamic (Phase 2) > static (Phase 1 fallback) > none (bit-perfect)
                let source: Box<dyn Source<Item = f32> + Send> =
                    if let Some(gain_atomic) = gain_atomic {
                        let initial_gain = normalization_gain.unwrap_or(1.0);
                        log::info!(
                            "Audio thread: dynamic normalization enabled (initial gain {:.4})",
                            initial_gain
                        );
                        analyzer_enabled.store(true, Ordering::SeqCst);
                        let source: Box<dyn Source<Item = f32> + Send> = Box::new(
                            AnalyzerTap::new(source, analyzer_tx.clone(), analyzer_enabled.clone()),
                        );
                        Box::new(DynamicAmplify::new(source, gain_atomic, initial_gain))
                    } else if let Some(gain) = normalization_gain {
                        log::info!(
                            "Audio thread: applying static normalization gain factor {:.4}",
                            gain
                        );
                        Box::new(source.amplify(gain))
                    } else {
                        source
                    };

                // Visualizer tap (outermost)
                if let Some(ref tap) = thread_viz_tap {
                    Box::new(TappedSource::new(
                        source,
                        tap.ring_buffer.clone(),
                        tap.enabled.clone(),
                    ))
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
            let init_device = |name: &Option<String>,
                               state: &SharedState,
                               sample_rate: u32,
                               channels: u16|
             -> Option<StreamType> {
                // Try backend system if configured
                if let Ok(settings) = thread_settings.lock() {
                    if settings.backend_type.is_some() || cfg!(target_os = "macos") {
                        // Use provided sample rate/channels to maintain DAC passthrough
                        log::info!(
                            "Initializing backend system with {}Hz/{}ch",
                            sample_rate,
                            channels
                        );
                        match try_init_stream_with_backend(&settings, sample_rate, channels, state)
                        {
                            Some(Ok(stream_type)) => {
                                // Set device name from settings for backend system
                                let device_name = settings
                                    .output_device
                                    .clone()
                                    .unwrap_or_else(|| "Default".to_string());
                                log::info!("Audio output initialized via backend system at {}Hz (device: {})", sample_rate, device_name);
                                state.set_current_device(Some(device_name));
                                return Some(stream_type);
                            }
                            Some(Err(e)) => {
                                // On macOS, the backend path is the only one that
                                // understands CoreAudio ownership and nominal-rate
                                // validation. Falling through to legacy CPAL can
                                // create a stream at a stale/source rate; in
                                // shared mode that can produce wrong-speed audio,
                                // and in Exclusive Mode it would silently drop Hog
                                // Mode. Surface the failure instead.
                                #[cfg(target_os = "macos")]
                                if settings
                                    .backend_type
                                    .unwrap_or(AudioBackendType::SystemDefault)
                                    == AudioBackendType::SystemDefault
                                {
                                    log::error!(
                                        "Could not start macOS audio output — {}. Not falling back to the legacy CPAL path because it would either play at the wrong speed (shared mode) or silently drop Exclusive Mode.",
                                        e
                                    );
                                    state.set_current_device(None);
                                    state.record_stream_error(e.clone());
                                    return None;
                                }
                                log::warn!(
                                    "Backend system init failed: {}, falling back to legacy",
                                    e
                                );
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
                    let found = host.output_devices().ok().and_then(|mut devices| {
                        devices.find(|d| cpal_device_name(d).as_deref() == Some(name.as_str()))
                    });

                    match found {
                        Some(d) if is_device_valid(&d) => {
                            log::info!("Found and validated device: {}", name);
                            Some(d)
                        }
                        Some(_) => {
                            log::warn!(
                                "Device '{}' found but has no valid output configs, using default",
                                name
                            );
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
                        if let Some(name) = cpal_device_name(&d) {
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

                match DeviceSinkBuilder::from_device(device).and_then(|b| b.open_sink_or_fallback())
                {
                    Ok(mixer_sink) => {
                        log::info!("Audio output initialized successfully");
                        Some(StreamType::rodio(mixer_sink))
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to create audio output on device: {}. Trying default...",
                            e
                        );
                        match DeviceSinkBuilder::open_default_sink() {
                            Ok(mixer_sink) => {
                                log::info!("Fallback to default audio output succeeded");
                                Some(StreamType::rodio(mixer_sink))
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
            let mut current_track_sample_rate: Option<u32> = None;
            let mut current_track_channels: Option<u16> = None;

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
            // Gapless request guard: once we request "next" for a track, do not re-arm
            // until track changes or playback state is reset.
            let mut gapless_request_armed = false;

            log::info!("Audio thread ready and waiting for commands");

            let handle_command =
                |command: AudioCommand,
                 current_engine: &mut Option<PlaybackEngine>,
                 current_audio_data: &mut Option<Vec<u8>>,
                 current_streaming_source: &mut Option<Arc<BufferedMediaSource>>,
                 stream_opt: &mut Option<StreamType>,
                 current_device_name: &mut Option<String>,
                 consecutive_sink_failures: &mut u32,
                 pause_suspend_deadline: &mut Option<Instant>,
                 current_track_sample_rate: &mut Option<u32>,
                 current_track_channels: &mut Option<u16>,
                 current_normalization_gain: &mut Option<f32>,
                 current_gain_atomic: &mut Option<Arc<AtomicU32>>,
                 gapless_pending: &mut Option<GaplessPending>,
                 gapless_request_armed: &mut bool| {
                    match command {
                        AudioCommand::Play {
                            data,
                            track_id,
                            duration_secs,
                            sample_rate,
                            channels,
                        } => {
                            log::info!(
                                "Audio thread: playing track {} ({}Hz, {} channels)",
                                track_id,
                                sample_rate,
                                channels
                            );
                            *pause_suspend_deadline = None;
                            thread_state.set_dsd_mode(0);
                            // Clear any pending gapless state (new Play supersedes queued gapless)
                            *gapless_pending = None;
                            *gapless_request_armed = false;
                            thread_state.set_gapless_ready(false);
                            thread_state.set_gapless_next_track_id(0);

                            let StreamRecreateDecision {
                                needs_new_stream,
                                format_changed,
                                dac_passthrough,
                                using_alsa_direct,
                                using_coreaudio_exclusive,
                                coreaudio_shared_rate_mismatch,
                            } = evaluate_stream_recreate(
                                &thread_settings,
                                stream_opt,
                                *current_track_sample_rate,
                                *current_track_channels,
                                sample_rate,
                                channels,
                                "Play",
                            );

                            if needs_new_stream {
                                if stream_opt.is_some() {
                                    // CoreAudio rate-mismatch case already
                                    // logged at warn level by
                                    // `evaluate_stream_recreate`.
                                    if coreaudio_shared_rate_mismatch.is_none()
                                        && (dac_passthrough
                                            || using_alsa_direct
                                            || using_coreaudio_exclusive)
                                        && format_changed
                                    {
                                        let mode = if using_coreaudio_exclusive {
                                            "CoreAudio exclusive"
                                        } else if using_alsa_direct {
                                            "ALSA Direct"
                                        } else {
                                            "DAC passthrough"
                                        };
                                        log::info!(
                                        "Sample rate/channels changed from {:?}Hz/{:?}ch to {}Hz/{}ch - recreating audio stream ({})",
                                        *current_track_sample_rate,
                                        *current_track_channels,
                                        sample_rate,
                                        channels,
                                        mode
                                    );
                                    }
                                    // Stop engine FIRST so its writer thread releases its
                                    // Arc<AlsaDirectStream> reference before we drop the stream.
                                    // Without this, snd_pcm_open() races against the old PCM
                                    // handle and fails with EBUSY.
                                    if let Some(engine) = current_engine.take() {
                                        engine.stop();
                                        std::thread::sleep(Duration::from_millis(50));
                                    }
                                    // Now this drop is the last Arc ref — PCM actually closes
                                    drop(stream_opt.take());
                                    // Give kernel time to fully release the ALSA device
                                    std::thread::sleep(Duration::from_millis(50));
                                }

                                log::info!(
                                    "DAC passthrough: {}, ALSA Direct: {}, CoreAudio exclusive: {}",
                                    dac_passthrough,
                                    using_alsa_direct,
                                    using_coreaudio_exclusive
                                );

                                // Try backend system first (if configured), then fall back to legacy CPAL
                                // This avoids unnecessary CPAL device enumeration for PipeWire DAC and ALSA Direct
                                let stream_result = if let Some(settings) =
                                    thread_settings.lock().ok()
                                {
                                    match try_init_stream_with_backend(
                                        &settings,
                                        sample_rate,
                                        channels,
                                        &thread_state,
                                    ) {
                                        Some(result) => {
                                            // Backend system handled it - set device name from settings
                                            if result.is_ok() {
                                                let device_name = settings
                                                    .output_device
                                                    .clone()
                                                    .unwrap_or_else(|| "Default".to_string());
                                                log::info!(
                                                    "Backend system using device: {}",
                                                    device_name
                                                );
                                                thread_state.set_current_device(Some(device_name));
                                            }
                                            result
                                        }
                                        None => {
                                            // Backend system not configured, use legacy CPAL path
                                            log::info!("Backend system not configured, using legacy CPAL path");

                                            // Get the audio device via CPAL
                                            let device = if let Some(ref name) =
                                                *current_device_name
                                            {
                                                log::info!("Looking for audio device: {}", name);
                                                let found = host.output_devices().ok().and_then(
                                                    |mut devices| {
                                                        devices.find(|d| {
                                                            cpal_device_name(d).as_deref()
                                                                == Some(name.as_str())
                                                        })
                                                    },
                                                );

                                                match found {
                                                    Some(d) if is_device_valid(&d) => {
                                                        log::info!(
                                                            "Found and validated device: {}",
                                                            name
                                                        );
                                                        Some(d)
                                                    }
                                                    Some(_) => {
                                                        log::warn!("Device '{}' found but has no valid output configs, using default", name);
                                                        host.default_output_device()
                                                    }
                                                    None => {
                                                        log::warn!(
                                                            "Device '{}' not found, using default",
                                                            name
                                                        );
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
                                            if let Some(name) = cpal_device_name(&device) {
                                                log::info!("Using audio device: {}", name);
                                                thread_state.set_current_device(Some(name));
                                            }

                                            create_output_stream_with_config(
                                                device,
                                                sample_rate,
                                                channels,
                                                dac_passthrough,
                                            )
                                            .map(StreamType::rodio)
                                        }
                                    }
                                } else {
                                    // Failed to lock settings, use legacy path with CPAL device search
                                    let device = if let Some(ref name) = *current_device_name {
                                        log::info!("Looking for audio device: {}", name);
                                        host.output_devices()
                                            .ok()
                                            .and_then(|mut devices| {
                                                devices.find(|d| {
                                                    cpal_device_name(d).as_deref()
                                                        == Some(name.as_str())
                                                })
                                            })
                                            .or_else(|| {
                                                log::warn!(
                                                    "Device '{}' not found, using default",
                                                    name
                                                );
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

                                    if let Some(name) = cpal_device_name(&device) {
                                        thread_state.set_current_device(Some(name));
                                    }

                                    create_output_stream_with_config(
                                        device,
                                        sample_rate,
                                        channels,
                                        dac_passthrough,
                                    )
                                    .map(StreamType::rodio)
                                };

                                // Handle stream creation result
                                match stream_result {
                                    Ok(stream) => {
                                        *stream_opt = Some(stream);
                                        thread_state.set_stream_error(false);

                                        // Set current device name from settings (for backend system)
                                        if let Some(settings) = thread_settings.lock().ok() {
                                            if let Some(ref device_name) = settings.output_device {
                                                thread_state
                                                    .set_current_device(Some(device_name.clone()));
                                                log::info!(
                                                    "Audio stream ready at {}Hz on device: {}",
                                                    sample_rate,
                                                    device_name
                                                );
                                            } else {
                                                thread_state.set_current_device(Some(
                                                    "Default".to_string(),
                                                ));
                                                log::info!(
                                                    "Audio stream ready at {}Hz on default device",
                                                    sample_rate
                                                );
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
                                        log::error!(
                                            "❌ Failed to create stream at {}Hz: {}",
                                            sample_rate,
                                            e
                                        );
                                        thread_state.set_stream_error(true);
                                        thread_state.set_current_device(None);
                                        return;
                                    }
                                }
                            } else if format_changed {
                                // Format changed but DAC passthrough is disabled - reuse existing stream
                                log::info!(
                                "Audio format changed from {:?}Hz/{:?}ch to {}Hz/{}ch - reusing audio stream (DAC passthrough disabled, gapless enabled)",
                                *current_track_sample_rate,
                                *current_track_channels,
                                sample_rate,
                                channels
                            );
                            }

                            // Track the decoded source format separately from
                            // the OS output stream rate. In shared mode the
                            // stream can stay at the CoreAudio nominal rate
                            // while each track has its own decoded rate.
                            *current_track_sample_rate = Some(sample_rate);
                            *current_track_channels = Some(channels);

                            let Some(ref stream) = *stream_opt else {
                                log::error!("Audio thread: no audio device available");
                                return;
                            };

                            // Stop previous engine and wait for sink to release resources.
                            // The 50ms sleep is an ALSA-only workaround for snd_pcm_open()
                            // racing the previous PCM handle's release. On CoreAudio (macOS)
                            // and WASAPI (Windows) the host stream stays open across track
                            // changes, so the sleep just feeds 50ms of silence into the
                            // mixer — audible as a click at each end of the gap.
                            if let Some(engine) = current_engine.take() {
                                engine.stop();
                                #[cfg(target_os = "linux")]
                                std::thread::sleep(Duration::from_millis(50));
                            }

                            *current_audio_data = Some(data.clone());
                            *current_streaming_source = None; // Clear streaming source for non-streaming playback
                            thread_state.set_loaded_audio(true);

                            // Create PlaybackEngine from StreamType
                            let mut engine = match stream {
                                StreamType::Rodio { sink: mixer_sink, .. } => {
                                    match PlaybackEngine::new_rodio(&mixer_sink.mixer()) {
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
                                                let sr = current_track_sample_rate.unwrap_or(48000);
                                                let ch = current_track_channels.unwrap_or(2);
                                                *stream_opt = init_device(
                                                    current_device_name,
                                                    &thread_state,
                                                    sr,
                                                    ch,
                                                );
                                                if stream_opt.is_some() {
                                                    log::info!("Audio stream auto-reinitialized successfully at {}Hz", sr);
                                                    *consecutive_sink_failures = 0;
                                                    thread_state.set_stream_error(false);
                                                } else {
                                                    log::error!("Auto-reinit failed. Audio device unavailable.");
                                                    thread_state
                                                        .is_playing
                                                        .store(false, Ordering::SeqCst);
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
                                    PlaybackEngine::new_alsa_direct(
                                        alsa_stream.clone(),
                                        hardware_volume,
                                    )
                                }
                                #[cfg(target_os = "linux")]
                                StreamType::Jack(jack_stream) => {
                                    *consecutive_sink_failures = 0;
                                    thread_state.set_stream_error(false);
                                    PlaybackEngine::new_jack(jack_stream.clone())
                                }
                            };

                            let volume = f32::from_bits(thread_state.volume.load(Ordering::SeqCst));
                            apply_engine_volume(&stream_opt, &engine, volume);

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
                            thread_state
                                .duration
                                .store(actual_duration, Ordering::SeqCst);

                            // Calculate normalization gain if enabled
                            let norm_settings = thread_settings
                                .lock()
                                .ok()
                                .filter(|s| s.normalization_enabled)
                                .map(|s| s.normalization_target_lufs);

                            let (normalization, gain_atomic) =
                                if let Some(target_lufs) = norm_settings {
                                    // Check for ReplayGain metadata first (initial gain hint)
                                    let rg_gain = extract_replaygain(&data)
                                        .map(|rg| calculate_gain_factor(&rg, target_lufs));

                                    // Create shared atomic for dynamic normalization
                                    let atomic =
                                        Arc::new(AtomicU32::new(rg_gain.unwrap_or(1.0).to_bits()));

                                    // Check loudness cache for pre-computed EBU R128 gain
                                    if let Some(cached) = loudness_cache.get(track_id) {
                                        let cached_gain = db_to_linear(cached.gain_db.min(6.0));
                                        atomic.store(cached_gain.to_bits(), Ordering::Relaxed);
                                        log::info!(
                                            "Normalization: cache hit for track {}, gain {:.4}",
                                            track_id,
                                            cached_gain
                                        );
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
                            let source = wrap_source(
                                source,
                                normalization,
                                gain_atomic,
                                &analyzer_tx,
                                &analyzer_enabled,
                            );
                            if let Err(e) = engine.append(source) {
                                log::error!("Failed to append source to engine: {}", e);
                                return;
                            }

                            thread_state.is_playing.store(true, Ordering::SeqCst);
                            thread_state.position.store(0, Ordering::SeqCst);
                            thread_state
                                .current_track_id
                                .store(track_id, Ordering::SeqCst);
                            thread_state.start_playback_timer(0);

                            *current_engine = Some(engine);
                            log::info!(
                                "Audio thread: playback started, duration: {}s, normalization: {}",
                                actual_duration,
                                normalization
                                    .map(|g| format!("{:.4}x", g))
                                    .unwrap_or_else(|| "off".to_string())
                            );
                        }
                        AudioCommand::PlayStreaming {
                            source,
                            track_id,
                            sample_rate,
                            channels,
                            duration_secs,
                            start_position_secs,
                            content_length,
                        } => {
                            log::info!(
                            "Audio thread: starting streaming playback for track {} ({}Hz, {} channels, {}s, start={}s)",
                            track_id,
                            sample_rate,
                            channels,
                            duration_secs,
                            start_position_secs
                        );
                            *pause_suspend_deadline = None;

                            // Store streaming source for resume capability
                            // When download completes, we can extract the data for resume
                            *current_streaming_source = Some(source.clone());
                            *current_audio_data = None; // Clear regular audio data
                            thread_state.set_loaded_audio(true);

                            let StreamRecreateDecision {
                                needs_new_stream,
                                format_changed,
                                dac_passthrough,
                                using_alsa_direct,
                                using_coreaudio_exclusive,
                                coreaudio_shared_rate_mismatch,
                            } = evaluate_stream_recreate(
                                &thread_settings,
                                stream_opt,
                                *current_track_sample_rate,
                                *current_track_channels,
                                sample_rate,
                                channels,
                                "Streaming",
                            );

                            if needs_new_stream {
                                if stream_opt.is_some() {
                                    // CoreAudio rate-mismatch case already
                                    // logged at warn level by
                                    // `evaluate_stream_recreate`.
                                    if coreaudio_shared_rate_mismatch.is_none()
                                        && (dac_passthrough
                                            || using_alsa_direct
                                            || using_coreaudio_exclusive)
                                        && format_changed
                                    {
                                        let mode = if using_coreaudio_exclusive {
                                            "CoreAudio exclusive"
                                        } else if using_alsa_direct {
                                            "ALSA Direct"
                                        } else {
                                            "DAC passthrough"
                                        };
                                        log::info!(
                                        "Streaming: Sample rate/channels changed to {}Hz/{}ch - recreating audio stream ({})",
                                        sample_rate,
                                        channels,
                                        mode
                                    );
                                    }
                                    // Stop engine FIRST so its writer thread releases its
                                    // Arc<AlsaDirectStream> reference before we drop the stream.
                                    // Without this, snd_pcm_open() races against the old PCM
                                    // handle and fails with EBUSY.
                                    if let Some(engine) = current_engine.take() {
                                        engine.stop();
                                        std::thread::sleep(Duration::from_millis(50));
                                    }
                                    // Now this drop is the last Arc ref — PCM actually closes
                                    drop(stream_opt.take());
                                    // Give kernel time to fully release the ALSA device
                                    std::thread::sleep(Duration::from_millis(50));
                                }

                                let stream_result = if let Some(settings) =
                                    thread_settings.lock().ok()
                                {
                                    match try_init_stream_with_backend(
                                        &settings,
                                        sample_rate,
                                        channels,
                                        &thread_state,
                                    ) {
                                        Some(result) => {
                                            // Set device name from settings for backend system
                                            if result.is_ok() {
                                                let device_name = settings
                                                    .output_device
                                                    .clone()
                                                    .unwrap_or_else(|| "Default".to_string());
                                                log::info!(
                                                    "Streaming backend using device: {}",
                                                    device_name
                                                );
                                                thread_state.set_current_device(Some(device_name));
                                            }
                                            result
                                        }
                                        None => {
                                            log::info!("Backend system not configured, using legacy CPAL path");
                                            let device =
                                                if let Some(ref name) = *current_device_name {
                                                    host.output_devices()
                                                        .ok()
                                                        .and_then(|mut devices| {
                                                            devices.find(|d| {
                                                                cpal_device_name(d).as_deref()
                                                                    == Some(name.as_str())
                                                            })
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

                                            if let Some(name) = cpal_device_name(&device) {
                                                thread_state.set_current_device(Some(name));
                                            }

                                            create_output_stream_with_config(
                                                device,
                                                sample_rate,
                                                channels,
                                                dac_passthrough,
                                            )
                                            .map(StreamType::rodio)
                                        }
                                    }
                                } else {
                                    let device = host.default_output_device();
                                    let Some(device) = device else {
                                        log::error!(
                                            "No audio output device available for streaming"
                                        );
                                        thread_state.set_stream_error(true);
                                        return;
                                    };
                                    create_output_stream_with_config(
                                        device,
                                        sample_rate,
                                        channels,
                                        dac_passthrough,
                                    )
                                    .map(StreamType::rodio)
                                };

                                match stream_result {
                                    Ok(stream) => {
                                        *stream_opt = Some(stream);
                                        thread_state.set_stream_error(false);
                                        log::info!(
                                            "Streaming audio stream ready at {}Hz",
                                            sample_rate
                                        );
                                        std::thread::sleep(Duration::from_millis(150));
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "❌ Failed to create stream for streaming at {}Hz: {}",
                                            sample_rate,
                                            e
                                        );
                                        thread_state.set_stream_error(true);
                                        return;
                                    }
                                }
                            }

                            // Keep decoded source format current even when
                            // macOS shared mode reuses the same output stream.
                            *current_track_sample_rate = Some(sample_rate);
                            *current_track_channels = Some(channels);

                            let Some(ref stream) = *stream_opt else {
                                log::error!(
                                    "Audio thread: no audio device available for streaming"
                                );
                                return;
                            };

                            // Stop previous engine. ALSA-only sleep — see Play handler
                            // above for rationale (CoreAudio/WASAPI don't race here and the
                            // 50ms gap is audible as a click).
                            if let Some(engine) = current_engine.take() {
                                engine.stop();
                                #[cfg(target_os = "linux")]
                                std::thread::sleep(Duration::from_millis(50));
                            }

                            // Create PlaybackEngine
                            let mut engine = match stream {
                                StreamType::Rodio { sink: mixer_sink, .. } => {
                                    match PlaybackEngine::new_rodio(&mixer_sink.mixer()) {
                                        Ok(e) => {
                                            *consecutive_sink_failures = 0;
                                            thread_state.set_stream_error(false);
                                            e
                                        }
                                        Err(e) => {
                                            log::error!(
                                                "Failed to create engine for streaming: {}",
                                                e
                                            );
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
                                    PlaybackEngine::new_alsa_direct(
                                        alsa_stream.clone(),
                                        hardware_volume,
                                    )
                                }
                                #[cfg(target_os = "linux")]
                                StreamType::Jack(jack_stream) => {
                                    PlaybackEngine::new_jack(jack_stream.clone())
                                }
                            };

                            let volume = f32::from_bits(thread_state.volume.load(Ordering::SeqCst));
                            apply_engine_volume(&stream_opt, &engine, volume);

                            // Wait for minimum buffer before starting playback.
                            // When start_position_secs > 0 (session resume),
                            // also wait for enough buffer to cover the resume
                            // offset plus an 8s headroom — the eager pre-skip
                            // below decodes-and-discards up to the offset and
                            // needs the bytes available without blocking the
                            // audio device on the first pull.
                            log::info!("Streaming: waiting for initial buffer...");
                            let start_wait = Instant::now();
                            let max_wait = Duration::from_secs(60);

                            let bytes_per_sec_estimate: u64 = if duration_secs > 0
                                && content_length > 0
                            {
                                content_length / duration_secs
                            } else {
                                200_000
                            };
                            let resume_buffer_target: u64 = if start_position_secs > 0 {
                                bytes_per_sec_estimate
                                    .saturating_mul(start_position_secs.saturating_add(8))
                            } else {
                                0
                            };

                            let buffer_sufficient = |src: &Arc<BufferedMediaSource>| -> bool {
                                if !src.has_min_buffer() {
                                    return false;
                                }
                                if resume_buffer_target == 0 {
                                    return true;
                                }
                                (src.buffer_size() as u64) >= resume_buffer_target
                            };

                            while !buffer_sufficient(&source)
                                && start_wait.elapsed() < max_wait
                            {
                                std::thread::sleep(Duration::from_millis(50));
                            }

                            if !source.has_min_buffer() {
                                log::error!("Streaming: timeout waiting for initial buffer");
                                return;
                            }
                            if resume_buffer_target > 0
                                && (source.buffer_size() as u64) < resume_buffer_target
                            {
                                log::warn!(
                                    "Streaming: timed out waiting for resume buffer (got {} bytes, wanted {}); pre-skip may underrun briefly",
                                    source.buffer_size(),
                                    resume_buffer_target
                                );
                            }

                            let buffer_wait_ms = start_wait.elapsed().as_millis();
                            log::info!(
                            "Streaming: buffer ready in {}ms ({} bytes, target {}), creating incremental decoder...",
                            buffer_wait_ms, source.buffer_size(), resume_buffer_target
                        );

                            // Create incremental streaming source - this starts playback IMMEDIATELY
                            // while continuing to decode/download in background
                            let incremental_source =
                                match IncrementalStreamingSource::new(source.clone()) {
                                    Ok(s) => s,
                                    Err(e) => {
                                        log::error!(
                                            "Failed to create incremental streaming source: {}",
                                            e
                                        );
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

                            let (normalization, gain_atomic) = if let Some(target_lufs) =
                                norm_settings
                            {
                                // Try ReplayGain metadata from buffered data
                                let rg_gain = source.get_buffered_data().and_then(|data| {
                                    extract_replaygain(&data)
                                        .map(|rg| calculate_gain_factor(&rg, target_lufs))
                                });

                                // Create shared atomic for dynamic normalization
                                let atomic =
                                    Arc::new(AtomicU32::new(rg_gain.unwrap_or(1.0).to_bits()));

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
                            let mut source_to_play: Box<dyn Source<Item = f32> + Send> =
                                Box::new(incremental_source);

                            // Eager pre-skip for session resume. Decode and
                            // discard samples here so the engine's first pull
                            // doesn't have to do the work synchronously, which
                            // would underrun the audio device for multi-second
                            // offsets. The buffer wait above guarantees
                            // there's enough downloaded data to feed this loop.
                            if start_position_secs > 0 {
                                let target_samples: u64 = (start_position_secs)
                                    .saturating_mul(actual_sr as u64)
                                    .saturating_mul(actual_ch as u64);
                                let skip_start = Instant::now();
                                let mut skipped: u64 = 0;
                                while skipped < target_samples {
                                    if source_to_play.next().is_none() {
                                        log::warn!(
                                            "Resume: source ended before reaching {}s (pre-skipped {} samples)",
                                            start_position_secs,
                                            skipped
                                        );
                                        break;
                                    }
                                    skipped += 1;
                                }
                                log::info!(
                                    "Resume: pre-skipped {} samples ({}s) in {}ms",
                                    skipped,
                                    start_position_secs,
                                    skip_start.elapsed().as_millis()
                                );
                            }

                            // Wrap source with diagnostic, normalization, and visualizer
                            let source_to_play = wrap_source(
                                source_to_play,
                                normalization,
                                gain_atomic,
                                &analyzer_tx,
                                &analyzer_enabled,
                            );
                            if let Err(e) = engine.append(source_to_play) {
                                log::error!("Failed to append streaming source to engine: {}", e);
                                return;
                            }

                            thread_state.set_dsd_mode(0);
                            thread_state.is_playing.store(true, Ordering::SeqCst);
                            thread_state
                                .position
                                .store(start_position_secs, Ordering::SeqCst);
                            thread_state
                                .current_track_id
                                .store(track_id, Ordering::SeqCst);
                            thread_state.start_playback_timer(start_position_secs);

                            *current_engine = Some(engine);
                            log::info!(
                            "Audio thread: streaming playback STARTED in {}ms at {}s (incremental decode active)",
                            start_wait.elapsed().as_millis(),
                            start_position_secs
                        );
                        }
                        AudioCommand::PlayDsdDop { path, track_id } => {
                            #[cfg(target_os = "linux")]
                            {
                                log::info!(
                                    "Audio thread: DoP playback for track {} ({})",
                                    track_id,
                                    path.display()
                                );
                                *pause_suspend_deadline = None;
                                *gapless_pending = None;
                                *gapless_request_armed = false;
                                thread_state.set_gapless_ready(false);
                                thread_state.set_gapless_next_track_id(0);

                                let demux = match qbz_dsd::open_dsd(&path) {
                                    Ok(d) => d,
                                    Err(e) => {
                                        log::error!("DoP: cannot open DSD file: {}", e);
                                        thread_state.set_stream_error(true);
                                        return;
                                    }
                                };
                                let dop = match qbz_dsd::DopStream::new(demux) {
                                    Ok(d) => d,
                                    Err(e) => {
                                        log::error!("DoP: cannot build DoP stream: {}", e);
                                        thread_state.set_stream_error(true);
                                        return;
                                    }
                                };
                                let carrier = dop.carrier_rate();
                                let dsd_rate = dop.dsd_rate();
                                let duration =
                                    dop.total_frames() / (carrier.max(1) as u64);

                                // DoP always needs a fresh S32 stream at the
                                // carrier rate — tear down whatever is open.
                                if let Some(engine) = current_engine.take() {
                                    engine.stop();
                                    std::thread::sleep(Duration::from_millis(50));
                                }
                                drop(stream_opt.take());
                                std::thread::sleep(Duration::from_millis(50));

                                let device = thread_settings
                                    .lock()
                                    .ok()
                                    .and_then(|s| s.output_device.clone());
                                let Some(device) = device else {
                                    log::error!("DoP: no output device configured");
                                    thread_state.set_stream_error(true);
                                    return;
                                };
                                let stream = match qbz_audio::alsa_backend::create_dop_stream(
                                    &device, carrier, 2,
                                ) {
                                    Ok(st) => Arc::new(st),
                                    Err(e) => {
                                        log::error!("DoP: stream open failed: {}", e);
                                        thread_state.set_stream_error(true);
                                        thread_state.set_current_device(None);
                                        return;
                                    }
                                };
                                log::info!(
                                    "DoP: {} locked at {} Hz carrier on {}",
                                    qbz_dsd::dsd_label(dsd_rate),
                                    carrier,
                                    device
                                );
                                *stream_opt = Some(StreamType::AlsaDirect(stream.clone()));
                                thread_state.set_current_device(Some(device));
                                *current_track_sample_rate = Some(carrier);
                                *current_track_channels = Some(2);
                                *current_audio_data = None;
                                *current_streaming_source = None;

                                let mut engine = PlaybackEngine::new_alsa_dop(stream, false);
                                if let Err(e) = engine.append_dop(Box::new(dop)) {
                                    log::error!("DoP: append failed: {}", e);
                                    thread_state.set_stream_error(true);
                                    return;
                                }
                                *current_engine = Some(engine);
                                thread_state.set_loaded_audio(true);
                                thread_state.set_stream_error(false);
                                thread_state.set_stream_quality(dsd_rate, 1);
                                thread_state.duration.store(duration, Ordering::SeqCst);
                                thread_state.set_dsd_mode(1);
                                thread_state.is_playing.store(true, Ordering::SeqCst);
                                thread_state.position.store(0, Ordering::SeqCst);
                                thread_state
                                    .current_track_id
                                    .store(track_id, Ordering::SeqCst);
                                thread_state.start_playback_timer(0);
                                log::info!("Audio thread: DoP playback STARTED");
                            }
                            #[cfg(not(target_os = "linux"))]
                            {
                                let _ = (path, track_id);
                                log::error!("DoP playback is Linux-only");
                                thread_state.set_stream_error(true);
                            }
                        }
                        AudioCommand::PlayDsdNative { path, track_id } => {
                            #[cfg(target_os = "linux")]
                            {
                                log::info!(
                                    "Audio thread: native DSD playback for track {} ({})",
                                    track_id,
                                    path.display()
                                );
                                *pause_suspend_deadline = None;
                                *gapless_pending = None;
                                *gapless_request_armed = false;
                                thread_state.set_gapless_ready(false);
                                thread_state.set_gapless_next_track_id(0);

                                let info = match qbz_dsd::open_dsd(&path) {
                                    Ok(d) => d.info().clone(),
                                    Err(e) => {
                                        log::error!("Native DSD: cannot open file: {}", e);
                                        thread_state.set_stream_error(true);
                                        return;
                                    }
                                };
                                if info.channels != 2 {
                                    log::error!("Native DSD: stereo only");
                                    thread_state.set_stream_error(true);
                                    return;
                                }
                                let rate = qbz_dsd::native_u32_rate(info.dsd_rate);
                                let duration = (info.sample_count / 32) / (rate.max(1) as u64);

                                if let Some(engine) = current_engine.take() {
                                    engine.stop();
                                    std::thread::sleep(Duration::from_millis(50));
                                }
                                drop(stream_opt.take());
                                std::thread::sleep(Duration::from_millis(50));

                                let device = thread_settings
                                    .lock()
                                    .ok()
                                    .and_then(|s| s.output_device.clone());
                                let Some(device) = device else {
                                    log::error!("Native DSD: no output device configured");
                                    thread_state.set_stream_error(true);
                                    return;
                                };
                                let (stream, little_endian) =
                                    match qbz_audio::alsa_backend::create_native_dsd_stream(
                                        &device,
                                        info.dsd_rate,
                                        2,
                                    ) {
                                        Ok(pair) => pair,
                                        Err(e) => {
                                            log::error!("Native DSD: stream open failed: {}", e);
                                            thread_state.set_stream_error(true);
                                            thread_state.set_current_device(None);
                                            return;
                                        }
                                    };
                                let stream = Arc::new(stream);
                                let native_src = match qbz_dsd::open_dsd(&path)
                                    .map_err(|e| e.to_string())
                                    .and_then(|d| {
                                        qbz_dsd::NativeDsdStream::new(d, little_endian)
                                            .map_err(|e| e.to_string())
                                    }) {
                                    Ok(n) => n,
                                    Err(e) => {
                                        log::error!("Native DSD: source build failed: {}", e);
                                        thread_state.set_stream_error(true);
                                        return;
                                    }
                                };
                                log::info!(
                                    "Native DSD: {} locked at {} Hz U32 ({}) on {}",
                                    qbz_dsd::dsd_label(info.dsd_rate),
                                    rate,
                                    if little_endian { "LE" } else { "BE" },
                                    device
                                );
                                *stream_opt = Some(StreamType::AlsaDirect(stream.clone()));
                                thread_state.set_current_device(Some(device));
                                *current_track_sample_rate = Some(rate);
                                *current_track_channels = Some(2);
                                *current_audio_data = None;
                                *current_streaming_source = None;

                                let mut engine = PlaybackEngine::new_alsa_dop(stream, true);
                                if let Err(e) = engine.append_dop(Box::new(native_src)) {
                                    log::error!("Native DSD: append failed: {}", e);
                                    thread_state.set_stream_error(true);
                                    return;
                                }
                                *current_engine = Some(engine);
                                thread_state.set_loaded_audio(true);
                                thread_state.set_stream_error(false);
                                thread_state.set_stream_quality(info.dsd_rate, 1);
                                thread_state.duration.store(duration, Ordering::SeqCst);
                                thread_state.set_dsd_mode(if little_endian { 3 } else { 2 });
                                thread_state.is_playing.store(true, Ordering::SeqCst);
                                thread_state.position.store(0, Ordering::SeqCst);
                                thread_state
                                    .current_track_id
                                    .store(track_id, Ordering::SeqCst);
                                thread_state.start_playback_timer(0);
                                log::info!("Audio thread: native DSD playback STARTED");
                            }
                            #[cfg(not(target_os = "linux"))]
                            {
                                let _ = (path, track_id);
                                log::error!("Native DSD playback is Linux-only");
                                thread_state.set_stream_error(true);
                            }
                        }
                        AudioCommand::PlayNextDsdDop { path, track_id } => {
                            #[cfg(target_os = "linux")]
                            {
                                let Some(engine) = current_engine.as_mut() else {
                                    thread_state.set_gapless_ready(false);
                                    return;
                                };
                                if !engine.is_dop() {
                                    log::info!("Gapless DoP: engine is not DoP, ignoring");
                                    thread_state.set_gapless_ready(false);
                                    return;
                                }
                                // Build the packing matching the ACTIVE
                                // direct mode (1 = DoP, 2/3 = native BE/LE).
                                let mode = thread_state.dsd_mode();
                                let built: Result<
                                    (Box<dyn Iterator<Item = i32> + Send>, u32, u64),
                                    String,
                                > = qbz_dsd::open_dsd(&path)
                                    .map_err(|e| e.to_string())
                                    .and_then(|d| match mode {
                                        1 => qbz_dsd::DopStream::new(d)
                                            .map_err(|e| e.to_string())
                                            .map(|st| {
                                                let rate = st.carrier_rate();
                                                let frames = st.total_frames();
                                                (
                                                    Box::new(st)
                                                        as Box<
                                                            dyn Iterator<Item = i32> + Send,
                                                        >,
                                                    rate,
                                                    frames,
                                                )
                                            }),
                                        2 | 3 => qbz_dsd::NativeDsdStream::new(d, mode == 3)
                                            .map_err(|e| e.to_string())
                                            .map(|st| {
                                                let rate = st.rate();
                                                let frames = st.total_frames();
                                                (
                                                    Box::new(st)
                                                        as Box<
                                                            dyn Iterator<Item = i32> + Send,
                                                        >,
                                                    rate,
                                                    frames,
                                                )
                                            }),
                                        _ => Err("no DSD-direct mode active".to_string()),
                                    });
                                let (src, rate, total_frames) = match built {
                                    Ok(v) => v,
                                    Err(e) => {
                                        log::warn!("Gapless DSD: cannot open next track: {}", e);
                                        thread_state.set_gapless_ready(false);
                                        return;
                                    }
                                };
                                if *current_track_sample_rate != Some(rate) {
                                    log::info!(
                                        "Gapless DSD: rate mismatch ({:?} vs {}), ignoring",
                                        *current_track_sample_rate,
                                        rate
                                    );
                                    thread_state.set_gapless_ready(false);
                                    return;
                                }
                                let duration = total_frames / (rate.max(1) as u64);
                                match engine.append_dop(src) {
                                    Ok(()) => {
                                        // data stays empty: the DoP engine never
                                        // resumes from current_audio_data (the
                                        // pause-suspend teardown is gated off in
                                        // DoP mode).
                                        *gapless_pending = Some(GaplessPending {
                                            track_id,
                                            duration_secs: duration,
                                            data: Vec::new(),
                                            normalization_gain: None,
                                        });
                                        thread_state.set_gapless_next_track_id(track_id);
                                        thread_state.set_gapless_ready(false);
                                        log::info!(
                                            "Gapless DoP: queued track {} for seamless DSD transition",
                                            track_id
                                        );
                                    }
                                    Err(e) => {
                                        log::warn!("Gapless DoP: append failed: {}", e);
                                        thread_state.set_gapless_ready(false);
                                    }
                                }
                            }
                            #[cfg(not(target_os = "linux"))]
                            {
                                let _ = (path, track_id);
                                thread_state.set_gapless_ready(false);
                            }
                        }
                        AudioCommand::Pause => {
                            if let Some(ref engine) = *current_engine {
                                engine.pause();
                                thread_state.pause_playback_timer();
                                thread_state.is_playing.store(false, Ordering::SeqCst);
                                *pause_suspend_deadline = Some(
                                    Instant::now() + Duration::from_millis(PAUSE_SUSPEND_DELAY_MS),
                                );
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
                                let audio_data: Vec<u8> = if let Some(ref data) =
                                    *current_audio_data
                                {
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
                                    log::warn!(
                                        "Audio thread: cannot resume - no audio data available"
                                    );
                                    return;
                                };

                                if stream_opt.is_none() {
                                    // Use last known sample rate/channels to maintain DAC passthrough
                                    let sr = current_track_sample_rate.unwrap_or(48000);
                                    let ch = current_track_channels.unwrap_or(2);
                                    log::info!(
                                        "Resume: reinitializing stream at {}Hz/{}ch",
                                        sr,
                                        ch
                                    );
                                    *stream_opt =
                                        init_device(current_device_name, &thread_state, sr, ch);
                                }

                                let Some(ref stream) = *stream_opt else {
                                    log::error!(
                                        "Audio thread: cannot resume - no audio device available"
                                    );
                                    return;
                                };

                                let mut engine = match stream {
                                    StreamType::Rodio { sink: mixer_sink, .. } => {
                                        match PlaybackEngine::new_rodio(&mixer_sink.mixer()) {
                                            Ok(e) => e,
                                            Err(e) => {
                                                log::error!(
                                                    "Failed to create engine for resume: {}",
                                                    e
                                                );
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
                                        PlaybackEngine::new_alsa_direct(
                                            alsa_stream.clone(),
                                            hardware_volume,
                                        )
                                    }
                                    #[cfg(target_os = "linux")]
                                    StreamType::Jack(jack_stream) => {
                                        PlaybackEngine::new_jack(jack_stream.clone())
                                    }
                                };

                                let volume =
                                    f32::from_bits(thread_state.volume.load(Ordering::SeqCst));
                                apply_engine_volume(&stream_opt, &engine, volume);

                                let source = match decode_with_fallback(&audio_data) {
                                    Ok(s) => s,
                                    Err(e) => {
                                        log::error!("Failed to decode audio for resume: {}", e);
                                        return;
                                    }
                                };

                                let resume_pos = thread_state.position.load(Ordering::SeqCst);
                                let skipped_source: Box<dyn Source<Item = f32> + Send> =
                                    if resume_pos > 0 {
                                        Box::new(
                                            source.skip_duration(Duration::from_secs(resume_pos)),
                                        )
                                    } else {
                                        source
                                    };

                                // Wrap source with diagnostic, normalization, and visualizer
                                // Reuse the gain + atomic from the original Play
                                let skipped_source = wrap_source(
                                    skipped_source,
                                    *current_normalization_gain,
                                    current_gain_atomic.clone(),
                                    &analyzer_tx,
                                    &analyzer_enabled,
                                );
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
                            *gapless_request_armed = false;
                            thread_state.set_gapless_ready(false);
                            thread_state.set_gapless_next_track_id(0);
                            analyzer_enabled.store(false, Ordering::SeqCst);
                            thread_state.set_normalization_gain(None);
                            thread_state.is_playing.store(false, Ordering::SeqCst);
                            thread_state.position.store(0, Ordering::SeqCst);
                            thread_state.set_loaded_audio(false);
                            thread_state
                                .playback_start_millis
                                .store(0, Ordering::SeqCst);
                            thread_state.position_at_start.store(0, Ordering::SeqCst);
                            // Defer dropping the stream so a Play immediately following Stop
                            // (the frontend's track-change pattern is Stop → Play, not append)
                            // can reuse the open device. Tearing CoreAudio down between every
                            // track was producing the audible click on track change. The idle
                            // loop's pause-suspend handler (below) drops the stream when this
                            // deadline fires; Play / Resume / Seek / ReinitDevice all clear
                            // the deadline so they reuse or replace the stream as needed.
                            *pause_suspend_deadline = Some(
                                Instant::now() + Duration::from_millis(PAUSE_SUSPEND_DELAY_MS),
                            );
                            // Reset the PipeWire clock if WE forced it. The call is
                            // self-gating (reset_pipewire_clock no-ops unless QBZ set
                            // the force), so it is now unconditional: the previous
                            // pw_force_bitperfect gate missed plain (no-passthrough)
                            // PipeWire users and leaked a forced rate after stop (#263).
                            #[cfg(target_os = "linux")]
                            qbz_audio::pipewire_backend::PipeWireBackend::reset_pipewire_clock();
                            log::info!("Audio thread: stopped");
                        }
                        AudioCommand::SetVolume(volume) => {
                            thread_state
                                .volume
                                .store(volume.to_bits(), Ordering::SeqCst);
                            if let Some(ref engine) = *current_engine {
                                apply_engine_volume(&stream_opt, &engine, volume);
                            }
                            // debug: a slider drag delivers dozens of these per
                            // second — at info they dominated a field log (#555,
                            // 758 of 1025 lines) and each one is a formatted
                            // write from the AUDIO thread.
                            log::debug!("Audio thread: volume set to {}", volume);
                        }
                        AudioCommand::Seek(position_secs) => {
                            if current_engine.as_ref().map(|e| e.is_dop()).unwrap_or(false) {
                                // v1 limitation: no seek inside a DoP stream
                                // (demuxer-level seek + marker re-phase later).
                                log::info!("Seek ignored during DoP playback ({}s)", position_secs);
                                return;
                            }
                            *pause_suspend_deadline = None;
                            // Cancel any pending gapless — seek creates a new engine
                            *gapless_pending = None;
                            *gapless_request_armed = false;
                            thread_state.set_gapless_ready(false);
                            thread_state.set_gapless_next_track_id(0);

                            // Three cases reach this handler:
                            //   * full-file playback (current_audio_data set)
                            //   * CMAF streaming, download complete (buffered
                            //     source holds the full file)
                            //   * CMAF streaming, download IN PROGRESS — only
                            //     allowed if the target position falls inside
                            //     the already-buffered region. skip_duration
                            //     reads samples sequentially, so seeking past
                            //     the watermark would block the audio thread
                            //     waiting for the rest of the download.
                            //     Cache, offline-cache, and local-library
                            //     playback reach this handler with
                            //     current_audio_data Some and skip the
                            //     streaming branch entirely (issue #335).
                            if current_audio_data.is_none() && current_streaming_source.is_none() {
                                log::warn!("Audio thread: cannot seek - no audio data available");
                                return;
                            }
                            if let Some(ref stream_src) = *current_streaming_source {
                                if !stream_src.is_complete() {
                                    // Approximate bytes-to-seconds mapping via
                                    // download fraction × total duration. Exact
                                    // for CBR, close-enough for FLAC/VBR; the
                                    // 0.90 margin covers the error band so the
                                    // decoder never reads past the watermark.
                                    let duration_secs = thread_state.duration();
                                    let progress = stream_src.progress().unwrap_or(0.0);
                                    if duration_secs == 0 || progress <= 0.0 {
                                        log::warn!(
                                            "Audio thread: seek to {}s ignored — streaming progress unknown",
                                            position_secs
                                        );
                                        return;
                                    }
                                    let max_seekable_secs =
                                        (progress * 0.90 * duration_secs as f32) as u64;
                                    if position_secs > max_seekable_secs {
                                        log::warn!(
                                            "Audio thread: seek to {}s ignored — past buffered watermark ({}s, progress {:.1}%)",
                                            position_secs,
                                            max_seekable_secs,
                                            progress * 100.0
                                        );
                                        return;
                                    }
                                    log::info!(
                                        "Audio thread: seek to {}s within buffered zone (watermark {}s, progress {:.1}%)",
                                        position_secs,
                                        max_seekable_secs,
                                        progress * 100.0
                                    );
                                }
                            }

                            let Some(ref stream) = *stream_opt else {
                                log::error!(
                                    "Audio thread: cannot seek - no audio device available"
                                );
                                return;
                            };

                            log::info!("Audio thread: seeking to {}s", position_secs);

                            // After take(), every failure path must clear playing
                            // state — otherwise UI can show "playing" with no engine.
                            if let Some(engine) = current_engine.take() {
                                engine.stop();
                            }
                            let seek_abort = |thread_state: &SharedState, why: &str| {
                                log::error!("Audio thread: seek aborted: {why}");
                                thread_state.is_playing.store(false, Ordering::SeqCst);
                                thread_state.set_stream_error(true);
                            };

                            let mut engine = match stream {
                                StreamType::Rodio { sink: mixer_sink, .. } => {
                                    match PlaybackEngine::new_rodio(&mixer_sink.mixer()) {
                                        Ok(e) => e,
                                        Err(e) => {
                                            seek_abort(
                                                &thread_state,
                                                &format!("rodio engine create failed: {e}"),
                                            );
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
                                    PlaybackEngine::new_alsa_direct(
                                        alsa_stream.clone(),
                                        hardware_volume,
                                    )
                                }
                                #[cfg(target_os = "linux")]
                                StreamType::Jack(jack_stream) => {
                                    PlaybackEngine::new_jack(jack_stream.clone())
                                }
                            };

                            let volume = f32::from_bits(thread_state.volume.load(Ordering::SeqCst));
                            apply_engine_volume(&stream_opt, &engine, volume);

                            // Build the decoded source for the seek. Both
                            // streaming and cached paths use Symphonia's native
                            // seek — FLAC seek table / MP3 TOC jumps straight
                            // to the target byte, then decodes forward to the
                            // exact sample. Avoids skip_duration's
                            // decode-every-sample-from-zero loop, which stalls
                            // the audio thread on long seeks (especially FLAC
                            // Hi-Res). Cached path falls back to decode_with_
                            // fallback + skip_duration if Symphonia can't
                            // probe the format (e.g., rodio-only MP4/AAC),
                            // preserving existing behavior for those cases.
                            let skip_duration = Duration::from_secs(position_secs);
                            let skipped_source: Box<dyn Source<Item = f32> + Send> = if let Some(
                                ref stream_src,
                            ) =
                                *current_streaming_source
                            {
                                match IncrementalStreamingSource::new(stream_src.clone()) {
                                    Ok(mut s) => {
                                        if let Err(e) = s.seek_to(skip_duration) {
                                            seek_abort(
                                                &thread_state,
                                                &format!("streaming native seek failed: {e}"),
                                            );
                                            return;
                                        }
                                        Box::new(s)
                                    }
                                    Err(e) => {
                                        seek_abort(
                                            &thread_state,
                                            &format!("streaming source for seek failed: {e}"),
                                        );
                                        return;
                                    }
                                }
                            } else {
                                let audio_data = current_audio_data
                                    .as_ref()
                                    .expect("current_audio_data was checked Some above");
                                match InMemorySource::new(audio_data.clone()) {
                                    Ok(mut s) => match s.seek_to(skip_duration) {
                                        Ok(()) => Box::new(s),
                                        Err(e) => {
                                            log::warn!(
                                                    "Native seek on cached source failed ({}); falling back to skip_duration",
                                                    e
                                                );
                                            match decode_with_fallback(audio_data) {
                                                Ok(fb) => Box::new(fb.skip_duration(skip_duration)),
                                                Err(e) => {
                                                    seek_abort(
                                                        &thread_state,
                                                        &format!(
                                                            "decode for seek failed: {e}"
                                                        ),
                                                    );
                                                    return;
                                                }
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        log::warn!(
                                                "InMemorySource probe failed ({}); falling back to skip_duration",
                                                e
                                            );
                                        match decode_with_fallback(audio_data) {
                                            Ok(fb) => Box::new(fb.skip_duration(skip_duration)),
                                            Err(e) => {
                                                seek_abort(
                                                    &thread_state,
                                                    &format!(
                                                        "decode for seek failed: {e}"
                                                    ),
                                                );
                                                return;
                                            }
                                        }
                                    }
                                }
                            };

                            // Send Reset to analyzer (seek invalidates accumulated samples)
                            let _ = analyzer_tx.try_send(AnalyzerMessage::Reset);

                            // Wrap source with diagnostic, normalization, and visualizer
                            // Reuse the gain + atomic from the current track
                            let skipped_source = wrap_source(
                                skipped_source,
                                *current_normalization_gain,
                                current_gain_atomic.clone(),
                                &analyzer_tx,
                                &analyzer_enabled,
                            );
                            if let Err(e) = engine.append(skipped_source) {
                                seek_abort(
                                    &thread_state,
                                    &format!("append source for seek failed: {e}"),
                                );
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
                            thread_state.set_stream_error(false);
                            log::info!(
                                "Audio thread: seeked to {}s (was_playing: {})",
                                position_secs,
                                was_playing
                            );
                        }
                        AudioCommand::ReinitDevice {
                            device_name: new_device,
                        } => {
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
                            let sr = current_track_sample_rate.unwrap_or(48000);
                            let ch = current_track_channels.unwrap_or(2);
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
                        AudioCommand::ReleaseDevice => {
                            log::info!("Audio thread: releasing output device (user-requested)");
                            // Cancel any deferred drop and tear the stream down NOW so
                            // the device is freed immediately (no warm-stream lingering).
                            *pause_suspend_deadline = None;
                            if let Some(engine) = current_engine.take() {
                                engine.stop();
                            }
                            drop(stream_opt.take());
                            // Undo anything QBZ parked so PipeWire / WirePlumber can
                            // reclaim the device (e.g. a DAC left invisible to other
                            // apps after bit-perfect ALSA Direct held it exclusively).
                            // Both calls are self-gating no-ops if QBZ didn't set them.
                            #[cfg(target_os = "linux")]
                            {
                                qbz_audio::alsa_backend::resume_suspended_sink();
                                qbz_audio::pipewire_backend::PipeWireBackend::reset_pipewire_clock();
                            }
                            thread_state.pause_playback_timer();
                            thread_state.is_playing.store(false, Ordering::SeqCst);
                            // Keep current_audio_data / current_streaming_source intact
                            // so a later Play / Resume reopens and continues.
                            log::info!("Audio thread: output device released");
                        }
                        AudioCommand::PlayNext {
                            data,
                            track_id,
                            sample_rate,
                            channels,
                        } => {
                            // Gapless: append next track to existing Rodio Sink
                            let engine = match current_engine.as_mut() {
                                Some(e) => e,
                                None => {
                                    log::warn!(
                                        "Gapless: no engine, ignoring PlayNext for track {}",
                                        track_id
                                    );
                                    thread_state.set_gapless_ready(false);
                                    return;
                                }
                            };

                            // Verify format compatibility (same sample rate and channels)
                            if let (Some(cur_sr), Some(cur_ch)) =
                                (*current_track_sample_rate, *current_track_channels)
                            {
                                if sample_rate != cur_sr || channels != cur_ch {
                                    log::info!(
                                    "Gapless: format mismatch (current {}Hz/{}ch vs next {}Hz/{}ch), ignoring PlayNext for track {}",
                                    cur_sr, cur_ch, sample_rate, channels, track_id
                                );
                                    thread_state.set_gapless_ready(false);
                                    return;
                                }
                            }

                            // Don't queue if already streaming
                            if current_streaming_source.is_some() {
                                log::info!("Gapless: streaming source active, ignoring PlayNext for track {}", track_id);
                                thread_state.set_gapless_ready(false);
                                return;
                            }

                            // Decode the next track's audio
                            let source = match decode_with_fallback(&data) {
                                Ok(s) => s,
                                Err(e) => {
                                    log::error!(
                                        "Gapless: failed to decode track {}: {}",
                                        track_id,
                                        e
                                    );
                                    thread_state.set_gapless_ready(false);
                                    return;
                                }
                            };

                            let actual_duration =
                                source.total_duration().map(|d| d.as_secs()).unwrap_or(0);

                            // Calculate normalization for the next track
                            let norm_settings = thread_settings
                                .lock()
                                .ok()
                                .filter(|s| s.normalization_enabled)
                                .map(|s| s.normalization_target_lufs);

                            let (normalization, gain_atomic) =
                                if let Some(target_lufs) = norm_settings {
                                    let rg_gain = extract_replaygain(&data)
                                        .map(|rg| calculate_gain_factor(&rg, target_lufs));
                                    let atomic =
                                        Arc::new(AtomicU32::new(rg_gain.unwrap_or(1.0).to_bits()));
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
                            let source = wrap_source(
                                source,
                                normalization,
                                gain_atomic,
                                &analyzer_tx,
                                &analyzer_enabled,
                            );

                            // Append to existing Sink (gapless queue)
                            if let Err(e) = engine.append(source) {
                                log::error!(
                                    "Gapless: failed to append track {} to engine: {}",
                                    track_id,
                                    e
                                );
                                thread_state.set_gapless_ready(false);
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
                                track_id,
                                actual_duration
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
                            &mut current_track_sample_rate,
                            &mut current_track_channels,
                            &mut current_normalization_gain,
                            &mut current_gain_atomic,
                            &mut gapless_pending,
                            &mut gapless_request_armed,
                        ),
                        Err(RecvTimeoutError::Timeout) => {
                            let now = Instant::now();
                            if now.duration_since(last_empty_check) >= Duration::from_millis(500) {
                                last_empty_check = now;

                                // Update streaming buffer progress for UI seekbar
                                if let Some(streaming_src) = current_streaming_source.as_ref() {
                                    let progress = streaming_src.progress().unwrap_or(1.0);
                                    thread_state.set_buffer_progress(progress);
                                } else {
                                    thread_state.set_buffer_progress(0.0);
                                }

                                // Streaming -> cached promotion:
                                // once streaming download completes, persist full data and clear streaming marker.
                                // This unlocks normal gapless pre-queue for the current track's tail.
                                let mut clear_streaming_source = false;
                                if let Some(streaming_src) = current_streaming_source.as_ref() {
                                    if streaming_src.is_complete() {
                                        if current_audio_data.is_none() {
                                            if let Some(full_data) =
                                                streaming_src.take_complete_data()
                                            {
                                                log::info!(
                                                    "Streaming promotion: full track buffered ({} bytes), enabling cached transition path",
                                                    full_data.len()
                                                );
                                                current_audio_data = Some(full_data);
                                            }
                                        }
                                        clear_streaming_source = true;
                                    }
                                }
                                if clear_streaming_source {
                                    current_streaming_source = None;
                                }

                                let pos = thread_state.current_position();
                                let dur = thread_state.duration.load(Ordering::SeqCst);

                                // Track whether a gapless transition fired in this iteration
                                // so the "approaching end" check below can skip itself.
                                // Without this, the stale pos/dur snapshot above would arm
                                // gapless_request_armed=true for the new track immediately
                                // (because pos/dur still point at the outgoing track at the
                                // moment of swap), and the flag never resets during the new
                                // track's playback — so the real "approaching end" trigger
                                // for the new track never fires and gapless playback stalls
                                // out at engine-empty.
                                let mut transition_consumed_pending = false;

                                // Gapless transition detection: when position exceeds current
                                // track duration, the queued next track has started playing.
                                //
                                // ORDERING NOTE: the polling loop in lib.rs reads `track_id`,
                                // `gapless_next_track_id`, and other fields as separate atomic
                                // loads, so it can observe an inconsistent intermediate state
                                // if these stores aren't ordered carefully. The frontend's
                                // `isGaplessTransition` predicate requires
                                // `event.gapless_next_track_id === 0` AND
                                // `event.track_id !== currentTrack.id`. If the polling loop
                                // reads `track_id` post-swap but `gapless_next_track_id`
                                // pre-reset, both conditions can't be satisfied simultaneously
                                // and the frontend mis-classifies the gapless transition as an
                                // external track change — leaving the UI stuck on the previous
                                // title while the audio plays the new track.
                                //
                                // To eliminate that race, clear the "transition complete"
                                // markers (`gapless_next_track_id`, `gapless_ready`) BEFORE
                                // mutating `track_id`. Any racing reader either sees the old
                                // track_id with cleared slots (no transition observed yet, will
                                // catch up on next tick) or the new track_id with cleared slots
                                // (clean gapless transition), but never the inconsistent
                                // mid-swap mix.
                                if let Some(ref pending) = gapless_pending {
                                    if dur > 0 && pos >= dur {
                                        log::info!(
                                            "Gapless transition: track {} -> {} (pos {}s >= dur {}s)",
                                            thread_state.current_track_id.load(Ordering::SeqCst),
                                            pending.track_id, pos, dur
                                        );
                                        // Clear gapless slot markers FIRST so a racing reader
                                        // never sees the inconsistent track_id-changed +
                                        // slot-still-set combination.
                                        thread_state.set_gapless_next_track_id(0);
                                        thread_state.set_gapless_ready(false);
                                        // Now safe to swap the track identity.
                                        thread_state
                                            .current_track_id
                                            .store(pending.track_id, Ordering::SeqCst);
                                        thread_state
                                            .duration
                                            .store(pending.duration_secs, Ordering::SeqCst);
                                        thread_state.start_playback_timer(0);
                                        current_audio_data = Some(pending.data.clone());
                                        current_normalization_gain = pending.normalization_gain;
                                        thread_state
                                            .set_normalization_gain(pending.normalization_gain);
                                        gapless_pending = None;
                                        gapless_request_armed = false;
                                        transition_consumed_pending = true;
                                    }
                                }

                                // ALSA Direct gapless: the writer thread signals transitions
                                // via an atomic flag instead of position-based detection.
                                // Same ordering rationale as above.
                                if let Some(ref engine) = current_engine {
                                    if engine.take_source_transition() {
                                        if let Some(ref pending) = gapless_pending {
                                            log::info!(
                                                "ALSA Direct gapless transition: track {} -> {}",
                                                thread_state
                                                    .current_track_id
                                                    .load(Ordering::SeqCst),
                                                pending.track_id
                                            );
                                            thread_state.set_gapless_next_track_id(0);
                                            thread_state.set_gapless_ready(false);
                                            thread_state
                                                .current_track_id
                                                .store(pending.track_id, Ordering::SeqCst);
                                            thread_state
                                                .duration
                                                .store(pending.duration_secs, Ordering::SeqCst);
                                            thread_state.start_playback_timer(0);
                                            current_audio_data = Some(pending.data.clone());
                                            current_normalization_gain = pending.normalization_gain;
                                            thread_state
                                                .set_normalization_gain(pending.normalization_gain);
                                            gapless_pending = None;
                                            gapless_request_armed = false;
                                            transition_consumed_pending = true;
                                        }
                                    }
                                }

                                // Gapless readiness: signal frontend that it's
                                // time to prepare the next track.
                                //
                                // Lead time used to be 5s but that's too tight
                                // for offline-cache v2 bundles: the AES-CTR
                                // decrypt of a HiRes track on CPUs WITHOUT
                                // AES-NI runs at ~10 MB/s — a 58 MB track
                                // needs ~6s just to decrypt, which blows past
                                // a 5s window and misses the gapless handoff.
                                //
                                // 10s covers most HiRes tracks even on the
                                // software-AES fallback path, and is
                                // harmless when decrypt is fast (the bytes
                                // just land in L1 a few seconds earlier and
                                // sit there until the engine picks them up).
                                //
                                // If the frontend ever exposes a user setting
                                // for this, just plumb it through
                                // AudioSettings and read here.
                                const GAPLESS_LEAD_SECS: u64 = 10;
                                let gapless_enabled = thread_settings
                                    .lock()
                                    .ok()
                                    .map(|s| s.gapless_enabled)
                                    .unwrap_or(false);
                                if gapless_enabled
                                    && !transition_consumed_pending
                                    && dur > 0
                                    && pos + GAPLESS_LEAD_SECS >= dur
                                    && gapless_pending.is_none()
                                    && !gapless_request_armed
                                    && !thread_state.is_gapless_ready()
                                    && thread_state.get_gapless_next_track_id() == 0
                                    && current_streaming_source.is_none()
                                {
                                    log::info!("Gapless: approaching end of track ({}s/{}s), requesting next", pos, dur);
                                    thread_state.set_gapless_ready(true);
                                    gapless_request_armed = true;
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
                                        thread_state
                                            .playback_start_millis
                                            .store(0, Ordering::SeqCst);
                                        // Clear gapless state on track end
                                        thread_state.set_gapless_ready(false);
                                        thread_state.set_gapless_next_track_id(0);
                                        gapless_pending = None;
                                        gapless_request_armed = false;
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
                        // DoP streams are NEVER suspended on pause: the writer
                        // keeps the DAC locked in DSD mode with 0x69 silence,
                        // and there is no current_audio_data to resume from.
                        let dop_active = current_engine
                            .as_ref()
                            .map(|e| e.is_dop())
                            .unwrap_or(false);
                        if stream_opt.is_some() && !dop_active {
                            let now = Instant::now();
                            if now >= deadline {
                                if let Some(engine) = current_engine.take() {
                                    engine.stop();
                                }
                                drop(stream_opt.take());
                                pause_suspend_deadline = None;
                                // Reset the PipeWire clock if WE forced it (self-gating;
                                // see reset_pipewire_clock). Unconditional now — the
                                // previous pw_force_bitperfect gate leaked a forced rate
                                // for plain (no-passthrough) PipeWire users (#263).
                                #[cfg(target_os = "linux")]
                                qbz_audio::pipewire_backend::PipeWireBackend::reset_pipewire_clock();
                                // The exclusive device is now released (stream dropped
                                // above), so resume any PipeWire sink we suspended for
                                // exclusive access — self-gating no-op otherwise (#263).
                                #[cfg(target_os = "linux")]
                                qbz_audio::alsa_backend::resume_suspended_sink();
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
                                    &mut current_track_sample_rate,
                                    &mut current_track_channels,
                                    &mut current_normalization_gain,
                                    &mut current_gain_atomic,
                                    &mut gapless_pending,
                                    &mut gapless_request_armed,
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
                            &mut current_track_sample_rate,
                            &mut current_track_channels,
                            &mut current_normalization_gain,
                            &mut current_gain_atomic,
                            &mut gapless_pending,
                            &mut gapless_request_armed,
                        ),
                        Err(_) => {
                            log::info!("Audio thread: channel closed, exiting");
                            break;
                        }
                    }
                }
            }
        });

        // Two-level playback cache: L1 in memory (~400 MB), L2 on disk
        // (~800 MB). A disk-cache failure degrades to L1-only rather than
        // aborting player creation.
        let audio_cache = match qbz_cache::PlaybackCache::new(800 * 1024 * 1024) {
            Ok(pc) => Arc::new(qbz_cache::AudioCache::with_playback_cache(
                400 * 1024 * 1024,
                Arc::new(pc),
            )),
            Err(e) => {
                log::warn!("Playback disk cache unavailable: {e}; memory cache only");
                Arc::new(qbz_cache::AudioCache::new(400 * 1024 * 1024))
            }
        };

        Self {
            tx,
            state,
            audio_settings: settings,
            visualizer_tap,
            diagnostic,
            audio_cache,
            play_generation: AtomicU64::new(0),
        }
    }

    /// Start a new play intent; returns the generation token for this intent.
    /// Any earlier in-flight `play_track` whose generation no longer matches
    /// must not start audio.
    fn begin_play(&self) -> u64 {
        self.play_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn is_current_play(&self, gen: u64) -> bool {
        self.play_generation.load(Ordering::SeqCst) == gen
    }

    /// Play a track by ID.
    ///
    /// First attempts the CMAF streaming pipeline (Akamai CDN, encrypted
    /// segments): only the init segment is fetched synchronously to derive
    /// stream parameters, playback starts immediately, and audio segments are
    /// fetched + decrypted + pushed to the streaming buffer in a background
    /// task. If the CMAF setup fails for any reason, falls back to the legacy
    /// `/track/getFileUrl` path (full FLAC download, then `play_data`).
    pub async fn play_track(
        &self,
        client: &QobuzClient,
        track_id: u64,
        quality: Quality,
        start_position_secs: u64,
    ) -> Result<(), String> {
        // Supersede any earlier in-flight play_track for a different intent.
        let gen = self.begin_play();
        log::info!(
            "Player: Starting playback for track {} with quality {:?} (start {}s, gen {gen})",
            track_id,
            quality,
            start_position_secs
        );

        // Cache hit: replay instantly from L1/L2 unless the cached copy is
        // a lower quality than now requested.
        if let Some(cached) = self.audio_cache.get(track_id) {
            if cached_quality_below_requested(&cached.data, quality) {
                log::info!(
                    "[CACHE] Track {} cached below requested {:?} — re-fetching",
                    track_id,
                    quality
                );
            } else {
                if !self.is_current_play(gen) {
                    log::info!(
                        "Player: cache-hit play for track {track_id} superseded (gen {gen})"
                    );
                    return Ok(());
                }
                log::info!(
                    "[CACHE HIT] Track {} ({} bytes) — playing from cache",
                    track_id,
                    cached.size_bytes
                );
                // Use apply_play_data so we do not bump generation again.
                let r = self.apply_play_data(cached.data, track_id);
                // Cached tracks play from in-memory data (no streaming resume
                // offset); honor a session-resume position with a best-effort
                // seek once playback has been handed to the audio thread.
                if r.is_ok() && start_position_secs > 0 && self.is_current_play(gen) {
                    let _ = self.seek(start_position_secs);
                }
                return r;
            }
        }

        // `streaming_only` suppresses writing the track into the cache.
        let skip_cache = self
            .audio_settings
            .lock()
            .map(|s| s.streaming_only)
            .unwrap_or(false);

        // Try CMAF streaming pipeline first.
        // Only the init segment is fetched synchronously; audio segments
        // stream in a background task.
        log::info!("[CMAF] Attempting CMAF streaming for track {}", track_id);
        match qbz_qobuz::cmaf::setup_streaming(client, track_id, quality).await {
            Ok(cmaf_info) => {
                if !self.is_current_play(gen) {
                    log::info!(
                        "Player: CMAF setup for track {track_id} superseded (gen {gen})"
                    );
                    return Ok(());
                }
                // Derive stream parameters from init segment metadata.
                let sample_rate = cmaf_info.sampling_rate.unwrap_or(44100);
                let channels = 2u16; // FLAC from Qobuz is always stereo
                let bit_depth = cmaf_info.bit_depth.unwrap_or(16);
                let total_flac_size = cmaf_info.flac_header.len() as u64
                    + cmaf_info
                        .segment_table
                        .iter()
                        .map(|s| s.byte_len as u64)
                        .sum::<u64>();

                // Track duration from the CMAF segment table. The streaming
                // path's position timer clamps `current_position` to the
                // duration it was given, so a zero here freezes the seek bar
                // at 0:00 and blocks auto-advance — derive the real value
                // from the per-segment sample counts.
                let total_samples: u64 = cmaf_info
                    .segment_table
                    .iter()
                    .map(|s| s.sample_count as u64)
                    .sum();
                let duration_secs = if sample_rate > 0 {
                    total_samples / sample_rate as u64
                } else {
                    0
                };

                // Estimate speed from the init segment fetch (conservative:
                // assume ~10 MB/s if init was too fast to measure reliably).
                let speed_mbps = if cmaf_info.init_fetch_ms > 0 {
                    let init_bytes = cmaf_info.flac_header.len() as f64 + 4096.0; // rough init size
                    (init_bytes / (cmaf_info.init_fetch_ms as f64 / 1000.0))
                        / (1024.0 * 1024.0)
                } else {
                    10.0
                };

                log::info!(
                    "[CMAF] Streaming setup: {}Hz, {}-bit, {:.2} MB total, {:.1} MB/s est, {} segments",
                    sample_rate,
                    bit_depth,
                    total_flac_size as f64 / (1024.0 * 1024.0),
                    speed_mbps,
                    cmaf_info.n_segments
                );

                // Create the streaming buffer and start playback immediately.
                let buffer_writer = self.play_streaming_dynamic(
                    track_id,
                    sample_rate,
                    channels,
                    bit_depth,
                    total_flac_size,
                    speed_mbps,
                    duration_secs,
                    start_position_secs, // session-resume offset (0 = from start)
                )?;

                // Spawn the background task that fetches + decrypts + pushes
                // audio segments to the buffer.
                let url_template = cmaf_info.url_template.clone();
                let content_key = cmaf_info.content_key;
                let flac_header = cmaf_info.flac_header;
                let n_segments = cmaf_info.n_segments;
                let cache = self.audio_cache.clone();

                tokio::spawn(async move {
                    match Self::cmaf_stream_segments(
                        &url_template,
                        n_segments,
                        content_key,
                        flac_header,
                        buffer_writer,
                        track_id,
                        cache,
                        skip_cache,
                    )
                    .await
                    {
                        Ok(()) => log::info!(
                            "[CMAF-STREAM COMPLETE] Track {}",
                            track_id
                        ),
                        Err(e) => log::error!(
                            "[CMAF-STREAM ERROR] Track {}: {}",
                            track_id,
                            e
                        ),
                    }
                });

                return Ok(());
            }
            Err(e) => {
                log::warn!(
                    "[CMAF] Streaming setup failed: {}, falling back to legacy download",
                    e
                );
                // Fall through to the legacy download path.
            }
        }

        if !self.is_current_play(gen) {
            log::info!(
                "Player: legacy path for track {track_id} superseded before URL fetch (gen {gen})"
            );
            return Ok(());
        }

        // Legacy fallback: get the stream URL
        log::info!("Player: Getting stream URL...");
        let stream_url = client
            .get_stream_url_with_fallback(track_id, quality)
            .await
            .map_err(|e| {
                log::error!("Player: Failed to get stream URL: {}", e);
                format!("Failed to get stream URL: {}", e)
            })?;

        if !self.is_current_play(gen) {
            log::info!(
                "Player: legacy download for track {track_id} superseded after URL (gen {gen})"
            );
            return Ok(());
        }

        log::info!(
            "Player: Got stream URL: {} (format: {})",
            stream_url.url,
            stream_url.mime_type
        );

        // Download the audio data
        log::info!("Player: Starting audio caching...");
        let audio_data = self.download_audio(&stream_url.url).await.map_err(|e| {
            log::error!("Player: Caching failed: {}", e);
            e
        })?;
        log::info!("Player: Cached {} bytes of audio data", audio_data.len());

        if !self.is_current_play(gen) {
            log::info!(
                "Player: legacy play for track {track_id} superseded after download (gen {gen})"
            );
            return Ok(());
        }

        // Store the legacy download in the cache for instant replay.
        if !skip_cache {
            self.audio_cache.insert(track_id, audio_data.clone());
        }

        // Send to audio thread (do not re-bump generation)
        let r = self.apply_play_data(audio_data, track_id);
        if r.is_ok() && start_position_secs > 0 && self.is_current_play(gen) {
            let _ = self.seek(start_position_secs);
        }
        r
    }

    /// Download a track fully into the L1/L2 cache **without** starting
    /// playback.
    ///
    /// Gapless playback requires upcoming tracks to be cache hits so they
    /// play via `play_data` (fully in-memory) rather than the streaming
    /// path — the audio engine's `PlayNext` handler ignores gapless
    /// requests while a streaming source is active. This method is the
    /// prefetch primitive the controller drives for the next 1-2 queue
    /// tracks.
    ///
    /// Mirrors the Tauri V2 prefetch download: CMAF `download_full` first
    /// (Akamai CDN), legacy `/track/getFileUrl` full download as fallback.
    /// No-ops when `streaming_only` is set, when the track is already
    /// cached, or when another fetch for the same id is in flight.
    pub async fn prefetch_into_cache(
        &self,
        client: &QobuzClient,
        track_id: u64,
        quality: Quality,
    ) -> Result<(), String> {
        // Honor streaming_only — never warm the cache when the user has
        // opted out of caching.
        let skip_cache = self
            .audio_settings
            .lock()
            .map(|s| s.streaming_only)
            .unwrap_or(false);
        if skip_cache {
            log::debug!("[PREFETCH] Skipped track {track_id} — streaming_only mode active");
            return Ok(());
        }

        // Already cached, or another prefetch for this id is already
        // running — nothing to do.
        if self.audio_cache.contains(track_id) {
            log::debug!("[PREFETCH] Track {track_id} already cached");
            return Ok(());
        }
        if self.audio_cache.is_fetching(track_id) {
            log::debug!("[PREFETCH] Track {track_id} already being fetched");
            return Ok(());
        }

        self.audio_cache.mark_fetching(track_id);
        log::info!("[PREFETCH] Prefetching track {track_id} at {quality:?}");

        // Try CMAF full download first (Akamai CDN), legacy full download
        // as fallback (nginx CDN).
        let result = match qbz_qobuz::cmaf::download_full(client, track_id, quality).await {
            Ok(data) => Ok(data),
            Err(e) => {
                log::warn!(
                    "[PREFETCH] CMAF failed for track {track_id}: {e}, trying legacy"
                );
                match client.get_stream_url_with_fallback(track_id, quality).await {
                    Ok(stream_url) => self.download_audio(&stream_url.url).await,
                    Err(e) => Err(format!("Failed to get stream URL: {e}")),
                }
            }
        };

        match result {
            Ok(data) => {
                // Brief delay before the cache write to avoid racing the
                // audio thread, matching the Tauri prefetch path.
                tokio::time::sleep(Duration::from_millis(50)).await;
                let len = data.len();
                self.audio_cache.insert(track_id, data);
                self.audio_cache.unmark_fetching(track_id);
                log::info!("[PREFETCH] Complete for track {track_id} ({len} bytes)");
                Ok(())
            }
            Err(e) => {
                self.audio_cache.unmark_fetching(track_id);
                log::warn!("[PREFETCH] Failed for track {track_id}: {e}");
                Err(e)
            }
        }
    }

    /// True if `track_id` is present in the L1/L2 playback cache. Used by
    /// the gapless controller to decide whether a track can be queued for
    /// a seamless handoff.
    pub fn is_track_cached(&self, track_id: u64) -> bool {
        self.audio_cache.contains(track_id)
    }

    /// Fetch a track's audio bytes for a gapless handoff: L1 memory →
    /// L2 disk → CMAF `download_full` (legacy full download as fallback).
    /// Does not start playback — the caller passes the bytes to
    /// `play_next`. Returns `None` only when every tier fails.
    ///
    /// Ports the L1/L2/CMAF tiers of Tauri's `v2_play_next_gapless`; the
    /// ephemeral / offline-cache / local-library tiers are intentionally
    /// omitted as they do not exist in the Slint MVP.
    pub async fn fetch_for_gapless(
        &self,
        client: &QobuzClient,
        track_id: u64,
        quality: Quality,
    ) -> Option<Vec<u8>> {
        // L1: in-memory cache.
        if let Some(cached) = self.audio_cache.get(track_id) {
            log::info!(
                "[GAPLESS] Track {track_id} from MEMORY cache ({} bytes)",
                cached.size_bytes
            );
            return Some(cached.data);
        }

        // L2: on-disk plain-FLAC playback cache. Warm L1 on the way out.
        if let Some(playback_cache) = self.audio_cache.get_playback_cache() {
            if let Some(audio_data) = playback_cache.get(track_id) {
                log::info!(
                    "[GAPLESS] Track {track_id} from DISK cache ({} bytes)",
                    audio_data.len()
                );
                self.audio_cache.insert(track_id, audio_data.clone());
                return Some(audio_data);
            }
        }

        // CMAF full download (Akamai CDN), legacy full download as
        // fallback. Warm L1 so a re-gapless / replay skips the network.
        let downloaded = match qbz_qobuz::cmaf::download_full(client, track_id, quality).await {
            Ok(data) => Some(data),
            Err(e) => {
                log::warn!("[GAPLESS] CMAF failed for track {track_id}: {e}, trying legacy");
                match client.get_stream_url_with_fallback(track_id, quality).await {
                    Ok(stream_url) => match self.download_audio(&stream_url.url).await {
                        Ok(data) => Some(data),
                        Err(e) => {
                            log::warn!("[GAPLESS] Legacy download failed for {track_id}: {e}");
                            None
                        }
                    },
                    Err(e) => {
                        log::warn!("[GAPLESS] No stream URL for {track_id}: {e}");
                        None
                    }
                }
            }
        };

        if let Some(ref data) = downloaded {
            log::info!(
                "[GAPLESS] Track {track_id} downloaded for gapless ({} bytes)",
                data.len()
            );
            self.audio_cache.insert(track_id, data.clone());
        } else {
            log::info!("[GAPLESS] Track {track_id} not available, gapless not possible");
        }
        downloaded
    }

    /// Resolve a fully-materialized audio asset for an EXTERNAL renderer
    /// (Chromecast / DLNA), carrying the bytes verbatim plus the MIME and the
    /// quality. Used by the Cast path through the local media server.
    ///
    /// Cache-first (P1, matches the fast Tauri cast path + consumes the gapless
    /// prefetch): L1 in-memory -> L2 on-disk playback cache (both decrypted
    /// FLAC) -> network. A prefetched/replayed track is served instantly; only a
    /// cold track pays the CMAF download. On a cache hit the delivered quality is
    /// not known here (no metadata stored with the bytes) — the caller derives
    /// the quality label from the track's catalog metadata; the network path
    /// returns the precise resolved tier.
    pub async fn fetch_for_external_stream(
        &self,
        client: &QobuzClient,
        track_id: u64,
        quality: Quality,
    ) -> Option<ExternalStreamAsset> {
        // L1: in-memory cache (warmed by the gapless prefetch / a prior play).
        if let Some(cached) = self.audio_cache.get(track_id) {
            log::info!(
                "[CAST-FETCH] Track {track_id} from MEMORY cache ({} bytes)",
                cached.size_bytes
            );
            return Some(ExternalStreamAsset {
                bytes: cached.data,
                content_type: "audio/flac".to_string(),
                quality: StreamQualityInfo::from_raw(0, None, None),
                duration_secs: None,
                origin: AssetOrigin::Cache,
            });
        }
        // L2: on-disk plain-FLAC playback cache; warm L1 on the way out.
        if let Some(playback_cache) = self.audio_cache.get_playback_cache() {
            if let Some(audio_data) = playback_cache.get(track_id) {
                log::info!(
                    "[CAST-FETCH] Track {track_id} from DISK cache ({} bytes)",
                    audio_data.len()
                );
                self.audio_cache.insert(track_id, audio_data.clone());
                return Some(ExternalStreamAsset {
                    bytes: audio_data,
                    content_type: "audio/flac".to_string(),
                    quality: StreamQualityInfo::from_raw(0, None, None),
                    duration_secs: None,
                    origin: AssetOrigin::Cache,
                });
            }
        }

        // Cold: CMAF full download (Akamai CDN) -> decrypted FLAC.
        match qbz_qobuz::cmaf::download_full_with_quality(client, track_id, quality).await {
            Ok((bytes, q)) => {
                log::info!(
                    "[CAST-FETCH] Track {track_id} via CMAF: {} bytes, format_id={}, {:?} kHz/{:?}-bit",
                    bytes.len(),
                    q.format_id,
                    q.sampling_rate_khz,
                    q.bit_depth
                );
                // Warm L1 so a subsequent local replay skips the network.
                self.audio_cache.insert(track_id, bytes.clone());
                return Some(ExternalStreamAsset {
                    bytes,
                    content_type: "audio/flac".to_string(),
                    quality: q,
                    duration_secs: None,
                    origin: AssetOrigin::Network,
                });
            }
            Err(e) => {
                log::warn!("[CAST-FETCH] CMAF failed for track {track_id}: {e}, trying legacy");
            }
        }

        // Fallback: legacy stream URL + plain HTTP download. Quality and MIME
        // come from the resolved StreamUrl (which carries the granted tier).
        match client.get_stream_url_with_fallback(track_id, quality).await {
            Ok(stream_url) => {
                let content_type =
                    external_content_type(&stream_url.mime_type, stream_url.format_id);
                let q = StreamQualityInfo::from_raw(
                    stream_url.format_id,
                    Some(stream_url.sampling_rate),
                    stream_url.bit_depth,
                );
                match self.download_audio(&stream_url.url).await {
                    Ok(bytes) => {
                        log::info!(
                            "[CAST-FETCH] Track {track_id} via legacy: {} bytes, format_id={}, ct={}",
                            bytes.len(),
                            q.format_id,
                            content_type
                        );
                        self.audio_cache.insert(track_id, bytes.clone());
                        Some(ExternalStreamAsset {
                            bytes,
                            content_type,
                            quality: q,
                            duration_secs: None,
                            origin: AssetOrigin::Network,
                        })
                    }
                    Err(e) => {
                        log::warn!("[CAST-FETCH] Legacy download failed for {track_id}: {e}");
                        None
                    }
                }
            }
            Err(e) => {
                log::warn!("[CAST-FETCH] No stream URL for {track_id}: {e}");
                None
            }
        }
    }

    /// Stream CMAF segments to the player's buffer, decrypting on the fly.
    ///
    /// Writes the FLAC header first so the decoder can identify the format,
    /// then fetches each audio segment, decrypts encrypted frames, and pushes
    /// the resulting FLAC frame data to the streaming buffer. The player
    /// starts playing as soon as enough data is buffered.
    async fn cmaf_stream_segments(
        url_template: &str,
        n_segments: u8,
        content_key: [u8; 16],
        flac_header: Vec<u8>,
        writer: BufferWriter,
        track_id: u64,
        cache: Arc<qbz_cache::AudioCache>,
        skip_cache: bool,
    ) -> Result<(), String> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("CMAF client error: {}", e))?;

        // Write the FLAC header first so the decoder can identify the format.
        if let Err(e) = writer.push_chunk(&flac_header) {
            return Err(format!("Failed to write FLAC header to buffer: {}", e));
        }

        let mut total_written: u64 = flac_header.len() as u64;
        // Accumulate the assembled FLAC (header + decrypted frames) so the
        // finished track can be cached for instant replay. Empty when
        // `skip_cache` (streaming_only) is set.
        let mut cache_data: Vec<u8> = if skip_cache {
            Vec::new()
        } else {
            flac_header.clone()
        };
        let start = Instant::now();

        for seg_idx in 1..=n_segments {
            let seg_url = url_template.replace("$SEGMENT$", &seg_idx.to_string());
            let seg_data = client
                .get(&seg_url)
                .header("User-Agent", "Mozilla/5.0")
                .send()
                .await
                .map_err(|e| format!("CMAF segment {} fetch: {}", seg_idx, e))?
                .bytes()
                .await
                .map_err(|e| format!("CMAF segment {} read: {}", seg_idx, e))?;

            let crypto = qbz_cmaf::parse_segment_crypto(&seg_data)
                .map_err(|e| format!("CMAF segment {} parse: {}", seg_idx, e))?;

            let mut data_pos = crypto.data_offset;
            for entry in &crypto.entries {
                let frame_end = data_pos + entry.size as usize;
                if frame_end > seg_data.len() {
                    let _ = writer.error(format!("CMAF segment {} frame overflow", seg_idx));
                    return Err(format!("CMAF segment {} frame overflow", seg_idx));
                }
                let mut frame = seg_data[data_pos..frame_end].to_vec();
                if entry.flags != 0 {
                    qbz_cmaf::decrypt_frame(&content_key, &entry.iv, &mut frame);
                }
                // Write decrypted frame to the streaming buffer.
                if let Err(e) = writer.push_chunk(&frame) {
                    log::error!("[CMAF-STREAM] Failed to push frame: {}", e);
                }
                if !skip_cache {
                    cache_data.extend_from_slice(&frame);
                }
                total_written += frame.len() as u64;
                data_pos = frame_end;
            }

            // Trailing unencrypted data after all frame entries.
            if data_pos < crypto.mdat_end && crypto.mdat_end <= seg_data.len() {
                let trailing = &seg_data[data_pos..crypto.mdat_end];
                if let Err(e) = writer.push_chunk(trailing) {
                    log::error!("[CMAF-STREAM] Failed to push trailing data: {}", e);
                }
                if !skip_cache {
                    cache_data.extend_from_slice(trailing);
                }
                total_written += trailing.len() as u64;
            }

            // Progress logging every 5 segments or on the last segment.
            if seg_idx % 5 == 0 || seg_idx == n_segments {
                let elapsed = start.elapsed().as_secs_f64();
                log::info!(
                    "[CMAF-STREAM] Segment {}/{} ({:.1} MB, {:.1} MB/s)",
                    seg_idx,
                    n_segments - 1,
                    total_written as f64 / (1024.0 * 1024.0),
                    if elapsed > 0.0 {
                        total_written as f64 / (1024.0 * 1024.0) / elapsed
                    } else {
                        0.0
                    }
                );
            }
        }

        // Signal end of stream.
        if let Err(e) = writer.complete() {
            log::error!("[CMAF-STREAM] Failed to mark buffer complete: {}", e);
        }

        log::info!(
            "[CMAF-STREAM] Complete: {:.2} MB written in {:.1}s for track {}, segments fetched: 1..{}",
            total_written as f64 / (1024.0 * 1024.0),
            start.elapsed().as_secs_f64(),
            track_id,
            n_segments - 1
        );

        // Cache the assembled FLAC (header + decrypted frames) for instant
        // replay on the next play of this track.
        if !skip_cache && !cache_data.is_empty() {
            let bytes = cache_data.len();
            cache.insert(track_id, cache_data);
            log::info!("[CMAF-STREAM] Track {} cached ({} bytes)", track_id, bytes);
        }

        Ok(())
    }

    /// Play from raw audio data (for cached tracks)
    /// Play from raw audio data (for cached / offline tracks).
    ///
    /// Bumps play generation so this call supersedes any in-flight
    /// `play_track` that has not yet applied audio.
    pub fn play_data(&self, data: Vec<u8>, track_id: u64) -> Result<(), String> {
        let _gen = self.begin_play();
        self.apply_play_data(data, track_id)
    }

    /// Send `Play` without bumping generation (used by `play_track` after
    /// its own `begin_play` + supersede checks).
    fn apply_play_data(&self, data: Vec<u8>, track_id: u64) -> Result<(), String> {
        log::info!(
            "Player: Playing {} bytes of audio data for track {}",
            data.len(),
            track_id
        );

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
                format!(
                    "Failed to send play command (audio thread may have crashed): {}",
                    e
                )
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
            track_id,
            meta.sample_rate,
            meta.channels,
            data.len()
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

    /// Play a local DSD file (.dsf/.dff) by converting it on the fly to
    /// 176.4 kHz / 24-bit PCM (qbz-dsd, Phase 1 of the DSD plan — see
    /// qbz-nix-docs/dsd-support/). The converted stream rides the existing
    /// `play_streaming` path as an ordinary finite WAV: a background thread
    /// demuxes + decimates and pushes into the BufferWriter, so the whole
    /// PCM pipeline (engines, bit-perfect ALSA at 176.4 kHz, volume,
    /// normalization, seek-in-buffer) behaves exactly as for any hi-res
    /// track. DST-compressed DFF and >2ch files are rejected with a
    /// readable error before anything starts.
    pub fn play_dsd_file(&self, path: std::path::PathBuf, track_id: u64) -> Result<(), String> {
        let demux = qbz_dsd::open_dsd(&path).map_err(|e| e.to_string())?;
        let dsd_rate = demux.info().dsd_rate;

        // DoP resolution (Phase 2): user opt-in + ALSA direct backend +
        // stereo + carrier rate supported by the device. Anything else falls
        // through to the universal DSD→PCM conversion below.
        #[cfg(target_os = "linux")]
        {
            let info = demux.info().clone();
            let resolved = self
                .audio_settings
                .lock()
                .ok()
                .map(|s| {
                    (
                        s.dsd_mode.clone(),
                        matches!(s.backend_type, Some(qbz_audio::AudioBackendType::Alsa)),
                        s.output_device.clone(),
                    )
                })
                .unwrap_or(("convert".to_string(), false, None));
            if let (mode, true, Some(device)) = resolved {
                if info.channels == 2 && mode != "convert" {
                    if mode == "native" {
                        // The open itself validates (kernel quirk / rates);
                        // failure surfaces as a stream error toast.
                        log::info!(
                            "Player: DSD track {} — {} via NATIVE DSD",
                            track_id,
                            qbz_dsd::dsd_label(info.dsd_rate)
                        );
                        drop(demux);
                        return self
                            .tx
                            .send(AudioCommand::PlayDsdNative { path, track_id })
                            .map_err(|e| {
                                format!("Failed to send native DSD play command: {}", e)
                            });
                    }
                    let carrier = qbz_dsd::dop_carrier_rate(info.dsd_rate);
                    let rate_ok =
                        qbz_audio::alsa_backend::get_device_supported_rates(&device)
                            .map(|r| r.contains(&carrier))
                            .unwrap_or(true);
                    if rate_ok {
                        log::info!(
                            "Player: DSD track {} — {} via DoP ({} Hz carrier)",
                            track_id,
                            qbz_dsd::dsd_label(info.dsd_rate),
                            carrier
                        );
                        drop(demux);
                        return self
                            .tx
                            .send(AudioCommand::PlayDsdDop { path, track_id })
                            .map_err(|e| format!("Failed to send DoP play command: {}", e));
                    }
                    log::info!(
                        "Player: DoP selected but device lacks the {} Hz carrier — converting to PCM",
                        carrier
                    );
                } else if info.channels != 2 && mode != "convert" {
                    log::info!(
                        "Player: {} selected but track has {} channels — downmix-converting to PCM",
                        mode,
                        info.channels
                    );
                }
            }
        }
        let mut conv = qbz_dsd::DsdPcmConverter::new(demux, qbz_dsd::DEFAULT_GAIN_DB)
            .map_err(|e| e.to_string())?;
        let channels = conv.channels();
        let rate = conv.output_rate();
        let total_frames = conv.total_frames();
        let duration_secs = total_frames / rate as u64;
        let content_length = qbz_dsd::wav_total_size(total_frames, channels);
        log::info!(
            "Player: DSD track {} — {} ({} Hz) → PCM {} Hz/24-bit, {}s, {} bytes WAV",
            track_id,
            qbz_dsd::dsd_label(dsd_rate),
            dsd_rate,
            rate,
            duration_secs,
            content_length
        );
        self.state.set_stream_quality(rate, 24);

        let writer =
            self.play_streaming(track_id, rate, channels, content_length, 3, duration_secs, 0)?;

        std::thread::spawn(move || {
            if writer
                .push_chunk(&qbz_dsd::wav_header(total_frames, channels, rate))
                .is_err()
            {
                return;
            }
            let mut pcm = Vec::new();
            loop {
                match conv.next_block() {
                    Ok(Some(frames)) => {
                        pcm.clear();
                        qbz_dsd::frames_to_pcm24(&frames, &mut pcm);
                        if writer.push_chunk(&pcm).is_err() {
                            // Reader gone (track changed/stopped) — just stop.
                            return;
                        }
                    }
                    Ok(None) => {
                        let _ = writer.complete();
                        return;
                    }
                    Err(e) => {
                        log::error!("Player: DSD conversion failed mid-track: {}", e);
                        let _ = writer.error(format!("DSD conversion failed: {}", e));
                        return;
                    }
                }
            }
        });
        Ok(())
    }

    /// Whole-file DSD→PCM conversion into an in-memory WAV, for the gapless
    /// prefetch path: the result feeds `play_next` like any cached track, so
    /// consecutive converted-DSD tracks hand off seamlessly. CPU-bound
    /// (~10-30x realtime) — call from a blocking context.
    pub fn prepare_dsd_gapless_wav(path: &std::path::Path) -> Result<Vec<u8>, String> {
        let demux = qbz_dsd::open_dsd(path).map_err(|e| e.to_string())?;
        let mut conv = qbz_dsd::DsdPcmConverter::new(demux, qbz_dsd::DEFAULT_GAIN_DB)
            .map_err(|e| e.to_string())?;
        let channels = conv.channels();
        let rate = conv.output_rate();
        let total = conv.total_frames();
        let mut out = qbz_dsd::wav_header(total, channels, rate);
        out.reserve(total as usize * channels as usize * 3);
        while let Some(frames) = conv.next_block().map_err(|e| e.to_string())? {
            qbz_dsd::frames_to_pcm24(&frames, &mut out);
        }
        Ok(out)
    }

    /// Queue the next DSD track for a gapless transition: appends to the DoP
    /// engine when one is active (seamless native DSD), otherwise converts to
    /// an in-memory WAV and rides the normal `play_next` gapless path.
    pub fn play_next_dsd(&self, path: std::path::PathBuf, track_id: u64) -> Result<(), String> {
        if self.state.is_dsd_direct() {
            return self
                .tx
                .send(AudioCommand::PlayNextDsdDop { path, track_id })
                .map_err(|e| format!("Failed to send DoP gapless command: {}", e));
        }
        let wav = Self::prepare_dsd_gapless_wav(&path)?;
        self.play_next(wav, track_id)
    }

    /// True while a DoP stream is active (volume fixed, seek unsupported).
    pub fn is_dsd_direct_active(&self) -> bool {
        self.state.is_dsd_direct()
    }

    /// Play from streaming source (starts playback before full download).
    /// Returns the BufferWriter so caller can push data as it downloads.
    /// `start_position_secs` > 0 turns this into a session-resume play
    /// (#315): the audio thread waits for enough buffer to cover the
    /// offset and pre-skips decoder output up to that point.
    pub fn play_streaming(
        &self,
        track_id: u64,
        sample_rate: u32,
        channels: u16,
        content_length: u64,
        buffer_seconds: u8,
        duration_secs: u64,
        start_position_secs: u64,
    ) -> Result<BufferWriter, String> {
        log::info!(
            "Player: Starting streaming playback for track {} ({}Hz, {}ch, {} bytes total, {}s, start={}s)",
            track_id,
            sample_rate,
            channels,
            content_length,
            duration_secs,
            start_position_secs
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
                start_position_secs,
                content_length,
            })
            .map_err(|e| {
                log::error!("Player: Failed to send streaming command: {}", e);
                format!("Failed to send streaming play command: {}", e)
            })?;

        log::info!("Player: Streaming playback initiated");
        Ok(writer)
    }

    /// Play from streaming source with dynamic buffer based on measured speed.
    /// `start_position_secs` > 0 signals session resume (see `play_streaming`).
    pub fn play_streaming_dynamic(
        &self,
        track_id: u64,
        sample_rate: u32,
        channels: u16,
        bit_depth: u32,
        content_length: u64,
        speed_mbps: f64,
        duration_secs: u64,
        start_position_secs: u64,
    ) -> Result<BufferWriter, String> {
        log::info!(
            "Player: Starting dynamic streaming for track {} ({}Hz, {}ch, {}-bit, {:.2} MB, {:.1} MB/s, {}s, start={}s)",
            track_id,
            sample_rate,
            channels,
            bit_depth,
            content_length as f64 / (1024.0 * 1024.0),
            speed_mbps,
            duration_secs,
            start_position_secs
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
                start_position_secs,
                content_length,
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

    pub fn has_loaded_audio(&self) -> bool {
        self.state.has_loaded_audio()
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

    /// Release the output device without reopening it. Drops the active
    /// stream — freeing an exclusive ALSA `hw:` grab and its D-Bus
    /// reservation — and un-suspends / un-forces anything QBZ parked, so
    /// PipeWire/WirePlumber can reclaim a device QBZ was holding (e.g. a DAC
    /// left invisible to other apps after bit-perfect ALSA Direct). Pair
    /// with a device re-enumeration in the UI to surface a freed or
    /// hot-plugged DAC without restarting the app.
    pub fn release_device(&self) -> Result<(), String> {
        self.tx
            .send(AudioCommand::ReleaseDevice)
            .map_err(|e| format!("Failed to send release command: {}", e))
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
            sample_rate: if sample_rate > 0 {
                Some(sample_rate)
            } else {
                None
            },
            bit_depth: if bit_depth > 0 { Some(bit_depth) } else { None },
            shuffle: None, // Set by caller with access to queue state
            repeat: None,  // Set by caller with access to queue state
            normalization_gain: self.state.get_normalization_gain(),
            gapless_ready: self.state.is_gapless_ready(),
            gapless_next_track_id: self.state.get_gapless_next_track_id(),
            bit_perfect_mode: self.state.get_bit_perfect_mode(),
            buffer_progress: self.state.get_buffer_progress(),
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

/// Pick the MIME to advertise to an external renderer for a legacy stream-URL
/// download. Prefer the server-provided `mime_type`; when it is empty (Qobuz
/// can return `""`), fall back by format id so the renderer is never handed an
/// empty content type (which some Chromecast/DLNA renderers reject).
pub fn external_content_type(mime: &str, format_id: u32) -> String {
    let trimmed = mime.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    match qbz_models::Quality::from_id(format_id) {
        Some(qbz_models::Quality::Mp3) => "audio/mpeg".to_string(),
        // Lossless / HiRes / UltraHiRes are FLAC over the file/url path.
        Some(_) => "audio/flac".to_string(),
        None => "audio/flac".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::compute_needs_new_stream;
    use super::external_content_type;

    #[test]
    fn no_stream_always_needs_new() {
        // Without an existing stream there is nothing to reuse — every
        // other flag is irrelevant.
        assert!(compute_needs_new_stream(
            false, false, false, false, false, false
        ));
    }

    #[test]
    fn unchanged_format_on_default_backend_reuses_stream() {
        // Default rodio backend resamples internally, so an unchanged
        // decoded format on an existing stream needs no rebuild.
        assert!(!compute_needs_new_stream(
            true, false, false, false, false, false
        ));
    }

    #[test]
    fn format_change_on_default_backend_rebuilds_for_native_rate() {
        // #449 regression guard: a decoded sample-rate/channel change must
        // rebuild the output stream on EVERY backend, not just the bit-perfect
        // ones, so the device follows the track's native rate. 1.2.10 got this
        // "for free" because Stop dropped the stream; once that drop was
        // deferred to avoid a track-change click (e93fcaec), reusing the stream
        // on the default/PipeWire backend left the node locked to the first
        // track's rate for every subsequent track. Same-rate tracks still
        // reuse (format_changed == false), preserving the click fix.
        assert!(compute_needs_new_stream(
            true, true, false, false, false, false
        ));
    }

    #[test]
    fn format_change_with_dac_passthrough_rebuilds() {
        assert!(compute_needs_new_stream(
            true, true, true, false, false, false
        ));
    }

    #[test]
    fn format_change_with_alsa_direct_rebuilds() {
        assert!(compute_needs_new_stream(
            true, true, false, true, false, false
        ));
    }

    #[test]
    fn format_change_with_coreaudio_exclusive_rebuilds() {
        assert!(compute_needs_new_stream(
            true, true, false, false, true, false
        ));
    }

    #[test]
    fn bit_perfect_backends_without_format_change_reuse_stream() {
        // Bit-perfect flags only force a rebuild *together with* a format
        // change. On their own they should not.
        assert!(!compute_needs_new_stream(
            true, false, true, false, false, false
        ));
        assert!(!compute_needs_new_stream(
            true, false, false, true, false, false
        ));
        assert!(!compute_needs_new_stream(
            true, false, false, false, true, false
        ));
    }

    #[test]
    fn coreaudio_shared_rate_mismatch_rebuilds_regardless_of_format_change() {
        // The CoreAudio shared-mode rate-drift case has nothing to do with
        // track format; it must rebuild whenever detected.
        assert!(compute_needs_new_stream(
            true, false, false, false, false, true
        ));
    }

    #[test]
    fn current_position_ms_is_a_pure_anchor_derivation() {
        use std::sync::atomic::Ordering;

        let state = super::SharedState::new();
        state.duration.store(300, Ordering::SeqCst);

        // Paused: coarse stored position scaled to ms (current_position parity).
        state.position.store(12, Ordering::SeqCst);
        assert_eq!(state.current_position_ms(), 12_000);

        // Playing without an anchor yet: same coarse fallback.
        state.is_playing.store(true, Ordering::SeqCst);
        assert_eq!(state.current_position_ms(), 12_000);

        // Playing with anchors: position_at_start*1000 + wall-clock elapsed ms.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        state
            .playback_start_millis
            .store(now_ms - 1_500, Ordering::SeqCst);
        state.position_at_start.store(10, Ordering::SeqCst);
        let pos = state.current_position_ms();
        assert!(
            (11_450..=11_900).contains(&pos),
            "expected ~11500ms, got {pos}"
        );

        // Clamped to duration*1000 (same rule current_position applies).
        state
            .playback_start_millis
            .store(now_ms - 10_000, Ordering::SeqCst);
        state.position_at_start.store(299, Ordering::SeqCst);
        assert_eq!(state.current_position_ms(), 300_000);
    }

    #[test]
    fn external_content_type_prefers_server_mime() {
        assert_eq!(external_content_type("audio/flac", 7), "audio/flac");
        assert_eq!(external_content_type("audio/mpeg", 5), "audio/mpeg");
        // Whitespace-only is treated as empty.
        assert_eq!(external_content_type("  ", 6), "audio/flac");
    }

    #[test]
    fn external_content_type_falls_back_by_format_id() {
        // Empty MIME (Qobuz can return "") -> derive from format id.
        assert_eq!(external_content_type("", 5), "audio/mpeg"); // Mp3
        assert_eq!(external_content_type("", 6), "audio/flac"); // Lossless
        assert_eq!(external_content_type("", 7), "audio/flac"); // HiRes
        assert_eq!(external_content_type("", 27), "audio/flac"); // UltraHiRes
        assert_eq!(external_content_type("", 999), "audio/flac"); // unknown -> flac
    }

    #[test]
    fn stream_quality_normalizes_units() {
        use qbz_models::StreamQualityInfo;
        // kHz input stays kHz.
        let khz = StreamQualityInfo::from_raw(7, Some(96.0), Some(24));
        assert_eq!(khz.sampling_rate_khz, Some(96.0));
        // Hz input is converted to kHz.
        let hz = StreamQualityInfo::from_raw(27, Some(192000.0), Some(24));
        assert_eq!(hz.sampling_rate_khz, Some(192.0));
        // Zero / unknown -> None.
        let zero = StreamQualityInfo::from_raw(6, Some(0.0), Some(16));
        assert_eq!(zero.sampling_rate_khz, None);
        // Tier label from format id.
        assert_eq!(khz.tier_label(), "FLAC 24-bit/≤96kHz");
        assert_eq!(hz.tier_label(), "FLAC 24-bit/>96kHz");
    }
}
