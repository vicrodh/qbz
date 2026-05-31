//! Native JACK output backend (#263 Tier 3).
//!
//! QBZ appears as a first-class JACK client (`qbz`) with stable output ports
//! `qbz:out_FL` / `qbz:out_FR`, patchable in qjackctl / qpwgraph / Reaper. The
//! routing survives track changes because the client + ports are created ONCE
//! and live for the whole session.
//!
//! **NOT bit-perfect.** A JACK graph runs at ONE fixed sample rate, so audio is
//! resampled to the graph rate (by the player's feeder) before it reaches us.
//! This is the documented opt-in trade: routing freedom over per-track
//! bit-perfect. The ALSA-exclusive / DAC-passthrough paths are untouched.
//!
//! Architecture: a lock-free SPSC ring buffer sits between the player's feeder
//! thread (push, via [`JackStream::write_f32`]) and the JACK `process` callback
//! (pop, runs in JACK's real-time thread). The callback never allocates or
//! locks beyond the lock-free ring read.

use jack::{AudioOut, Client, ClientOptions, Control, Port, ProcessScope};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Ring buffer headroom in stereo frames (~1.5 s at 44.1 kHz). Sized generously
/// so the feeder never blocks the audio decode under normal scheduling.
const RING_CAPACITY_FRAMES: usize = 1 << 16; // 65536

/// Max stereo frames a single `process` cycle will ever request; the reusable
/// de-interleave scratch is pre-sized to this so the RT callback never allocates.
const MAX_NFRAMES: usize = 16384;

/// JACK `process` handler (runs in JACK's RT thread). Pops interleaved stereo
/// f32 from the ring buffer and de-interleaves into the two output ports.
struct JackProcess {
    reader: jack::RingBufferReader,
    out_l: Port<AudioOut>,
    out_r: Port<AudioOut>,
    /// Reusable interleaved scratch (pre-sized; no RT allocation).
    scratch: Vec<f32>,
    underruns: Arc<AtomicU64>,
}

impl jack::ProcessHandler for JackProcess {
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        let nframes = (ps.n_frames() as usize).min(MAX_NFRAMES);
        let need_samples = nframes * 2; // stereo interleaved

        // Read interleaved f32 (as bytes) from the lock-free ring buffer into the
        // pre-allocated scratch. No allocation: scratch is sized to MAX_NFRAMES*2.
        let scratch_bytes: &mut [u8] = unsafe {
            std::slice::from_raw_parts_mut(
                self.scratch.as_mut_ptr() as *mut u8,
                need_samples * std::mem::size_of::<f32>(),
            )
        };
        let got_bytes = self.reader.read_buffer(scratch_bytes);
        let got_samples = got_bytes / std::mem::size_of::<f32>();

        let l = self.out_l.as_mut_slice(ps);
        let r = self.out_r.as_mut_slice(ps);
        for i in 0..nframes {
            let li = i * 2;
            let ri = li + 1;
            l[i] = if li < got_samples { self.scratch[li] } else { 0.0 };
            r[i] = if ri < got_samples { self.scratch[ri] } else { 0.0 };
        }

        if got_samples < need_samples {
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
        Control::Continue
    }
}

/// An active JACK client plus the producer side of the audio ring buffer.
///
/// Mirrors the role of `AlsaDirectStream`: the player feeds interleaved f32 via
/// [`write_f32`](Self::write_f32); a long-lived feeder thread paces the writes.
/// Dropping this deactivates + closes the JACK client (ports disappear).
pub struct JackStream {
    /// Activated async client; kept alive for the stream's lifetime. Its `Drop`
    /// deactivates the client and unregisters the ports.
    _async_client: jack::AsyncClient<(), JackProcess>,
    writer: Mutex<jack::RingBufferWriter>,
    sample_rate: u32,
    channels: u16,
    underruns: Arc<AtomicU64>,
}

impl JackStream {
    /// Open the JACK client, register stable stereo ports, and activate.
    ///
    /// `channels` is the player's channel count; JACK output is stereo here
    /// (FL/FR). The player resamples + downmixes to stereo at the graph rate
    /// before feeding, so the ring carries interleaved stereo f32.
    pub fn new(channels: u16) -> Result<Self, String> {
        let (client, _status) = Client::new("qbz", ClientOptions::NO_START_SERVER)
            .map_err(|e| format!("JACK client open failed (is a JACK/pipewire-jack server running?): {e}"))?;

        let sample_rate = client.sample_rate() as u32;

        let out_l = client
            .register_port("out_FL", AudioOut::default())
            .map_err(|e| format!("JACK register out_FL failed: {e}"))?;
        let out_r = client
            .register_port("out_FR", AudioOut::default())
            .map_err(|e| format!("JACK register out_FR failed: {e}"))?;

        // Lock-free SPSC ring buffer (bytes). Holds interleaved stereo f32.
        let rb = jack::RingBuffer::new(RING_CAPACITY_FRAMES * 2 * std::mem::size_of::<f32>())
            .map_err(|e| format!("JACK ring buffer alloc failed: {e}"))?;
        let (reader, writer) = rb.into_reader_writer();

        let underruns = Arc::new(AtomicU64::new(0));
        let process = JackProcess {
            reader,
            out_l,
            out_r,
            scratch: vec![0.0f32; MAX_NFRAMES * 2],
            underruns: underruns.clone(),
        };

        let async_client = client
            .activate_async((), process)
            .map_err(|e| format!("JACK activate failed: {e}"))?;

        log::info!(
            "[JACK] client 'qbz' active at {} Hz — ports qbz:out_FL / qbz:out_FR (NOT bit-perfect: resampled to the graph rate)",
            sample_rate
        );

        Ok(Self {
            _async_client: async_client,
            writer: Mutex::new(writer),
            sample_rate,
            channels,
            underruns,
        })
    }

    /// The JACK graph sample rate. The player resamples each track to this rate
    /// before feeding (the graph rate can change at runtime — the player should
    /// re-read this on a `buffer_size`/`sample_rate` change; TODO Slice 3.2).
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Push interleaved stereo f32 (already at the graph rate) into the ring
    /// buffer. Called from the player's feeder thread. Returns the number of
    /// *frames* accepted; fewer than requested means the ring is full and the
    /// feeder should retry (it paces itself against real time).
    pub fn write_f32(&self, samples: &[f32]) -> usize {
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                samples.as_ptr() as *const u8,
                std::mem::size_of_val(samples),
            )
        };
        let written = {
            let mut w = self.writer.lock().unwrap();
            w.write_buffer(bytes)
        };
        (written / std::mem::size_of::<f32>()) / 2
    }

    /// Total underrun events since open (diagnostic).
    #[allow(dead_code)]
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }
}
