//! ImmersiveView audio-visualizer glue.
//!
//! Spawns the frontend-agnostic FFT producer (`qbz_audio::visualizer`) against
//! the runtime's [`VisualizerTap`], latches each [`VizFrame`] into a single-slot
//! cell, and drains the latest frames into the `VisualizerState` Slint global on
//! a ~30 fps UI-thread timer. Persistent `VecModel`s are mutated in place
//! (`set_row_data`) so the Slint side keeps the same model identity — no
//! per-frame re-instantiation of the bound views.
//!
//! The tap starts disabled: nothing is captured and the FFT loop idles until the
//! immersive view calls `VisualizerState::set-enabled(true)` on open. There is no
//! Tauri command here — `set-enabled` drives `tap.set_enabled` directly, the same
//! pattern `playback.rs` uses for the rest of the runtime controls.
//!
//! Protected-audio note: this lives entirely downstream of the read-only ring
//! buffer. It touches none of the device/stream init (see CLAUDE.md "Audio
//! Backend System").

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_audio::visualizer::{spawn_visualizer_thread, VizFrame, VizSink};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::adapter::SlintAdapter;
use crate::{AppWindow, ImmersiveState, NowPlayingState, VisualizerState};

/// Single-slot, latest-wins frame store shared with the FFT producer thread.
/// Each cell holds at most the most recent frame for that stream; the UI drain
/// `take()`s it. A stalled UI therefore drops intermediate frames instead of
/// growing an unbounded queue.
#[derive(Default)]
struct VizCells {
    bars: Mutex<Option<[f32; 16]>>,
    spectral: Mutex<Option<Vec<f32>>>,
    energy: Mutex<Option<[f32; 5]>>,
    waveform: Mutex<Option<Box<[f32; 512]>>>,
    transient: Mutex<Option<f32>>,
}

/// The producer-side sink: latches frames into the shared cells (no Slint access
/// from the FFT thread).
struct SlintVizSink {
    cells: Arc<VizCells>,
}

impl VizSink for SlintVizSink {
    fn submit(&self, frame: VizFrame) {
        match frame {
            VizFrame::Viz16(b) => *self.cells.bars.lock().unwrap() = Some(b),
            VizFrame::Spectral512(b) => *self.cells.spectral.lock().unwrap() = Some(b),
            VizFrame::Energy5(b) => *self.cells.energy.lock().unwrap() = Some(b),
            VizFrame::Wave256x2(b) => *self.cells.waveform.lock().unwrap() = Some(b),
            VizFrame::Transient1(x) => *self.cells.transient.lock().unwrap() = Some(x),
        }
    }
}

thread_local! {
    /// Keeps the drain timer alive for the app lifetime (a dropped `Timer` stops
    /// firing) and reachable from the set-enabled handler, which restarts/stops
    /// it with the tap. Lives on the UI thread, like the models it writes.
    static DRAIN_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

/// Wire the visualizer. Call once, on the UI thread, after the runtime is built
/// and before `window.run()`. No-op when the runtime carries no tap (i.e. it was
/// built without [`AppRuntime::with_visualizer`]).
pub fn install(window: &AppWindow, runtime: &Arc<AppRuntime<SlintAdapter>>) {
    let Some(tap) = runtime.visualizer_tap().cloned() else {
        log::warn!("[viz] runtime has no visualizer tap; immersive visualizers disabled");
        return;
    };

    // Persistent models — created once, set on the global once, then mutated per
    // frame so the bound views never see a new model identity.
    let bars: Rc<VecModel<f32>> = Rc::new(VecModel::from(vec![0.0f32; 16]));
    let spectral: Rc<VecModel<f32>> = Rc::new(VecModel::from(vec![0.0f32; 512]));
    let energy: Rc<VecModel<f32>> = Rc::new(VecModel::from(vec![0.0f32; 5]));
    let waveform: Rc<VecModel<f32>> = Rc::new(VecModel::from(vec![0.0f32; 512]));

    let st = window.global::<VisualizerState>();
    st.set_bars(ModelRc::from(bars.clone()));
    st.set_spectral(ModelRc::from(spectral.clone()));
    st.set_energy(ModelRc::from(energy.clone()));
    st.set_waveform(ModelRc::from(waveform.clone()));

    // Producer thread: computes the five streams, latches each into its cell.
    // Keep its `Thread` handle so the set-enabled handler (registered below,
    // after the drain timer exists) can unpark it out of its disabled idle.
    let cells = Arc::new(VizCells::default());
    let sink = Arc::new(SlintVizSink {
        cells: cells.clone(),
    });
    let fft_thread = spawn_visualizer_thread(tap.clone(), sink).thread().clone();

    // ~30 fps drain on the UI thread: copy the latest frames into the models.
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    // Latest audio kept across ticks so the shaders animate smoothly (their own
    // `time` accumulator) and only *react* at 30 fps. `last_tr`/`last_beat` decay
    // each tick (a one-frame transient -> a multi-frame flash); `last_phase` is
    // the audio-reactive forward-motion clock. Bands/energy are passed RAW
    // (already smoothed upstream in qbz-audio); only `last_level_smooth` is a new
    // slow EMA — do not double-smooth.
    let mut last_tr = 0.0f32;
    let mut last_energy = [0.0f32; 5];
    let mut last_bars16 = [0.0f32; 16];
    let mut last_level_smooth = 0.0f32;
    let mut last_beat = 0.0f32;
    let mut last_phase = 0.0f32;
    // Spectral-ribbon (mode 4) reset tracking: track change / seek → clear.
    let mut last_track_id = String::new();
    let mut last_progress = 0.0f32;
    // Smoothed highest-active-band fraction → the spectral-ribbon ceiling line.
    let mut last_peak = 0.0f32;
    // Paused gate edge tracker (see the top of the closure).
    let mut drain_saw_playing = false;
    // Second handle to the producer thread for the resume unpark below (the
    // original moves into the set-enabled handler).
    let fft_thread_drain = fft_thread.clone();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(33),
        move || {
            // Paused gate: while NowPlayingState says not-playing, skip the
            // whole drain (cell takes, 28 set_row_data models, shader frame) —
            // the producer is parked via the tap's `paused` flag (playback.rs
            // mirrors every set_playing flip), so there is no fresh data; the
            // bars simply freeze at their last values. On the paused→playing
            // edge, unpark the producer so resume feels instant (≤33ms to
            // observe the flag here vs its 200ms park self-wake).
            let Some(win) = weak.upgrade() else {
                return;
            };
            let playing = win.global::<NowPlayingState>().get_playing();
            if playing && !drain_saw_playing {
                fft_thread_drain.unpark();
            }
            drain_saw_playing = playing;
            if !playing {
                return;
            }
            if let Some(b) = cells.bars.lock().unwrap().take() {
                for (i, v) in b.iter().enumerate() {
                    bars.set_row_data(i, *v);
                }
                last_bars16 = b;
            }
            if let Some(b) = cells.energy.lock().unwrap().take() {
                for (i, v) in b.iter().enumerate() {
                    energy.set_row_data(i, *v);
                }
                last_energy = b;
            }
            // Capture the latest spectral frame for the spectral-ribbon shader
            // (mode 4): a Some here = a new column to paint this tick.
            let mut new_spectral: Option<Vec<f32>> = None;
            if let Some(b) = cells.spectral.lock().unwrap().take() {
                for (i, v) in b.iter().enumerate() {
                    spectral.set_row_data(i, *v);
                }
                new_spectral = Some(b);
            }
            if let Some(b) = cells.waveform.lock().unwrap().take() {
                for (i, v) in b.iter().enumerate() {
                    waveform.set_row_data(i, *v);
                }
            }
            if let Some(x) = cells.transient.lock().unwrap().take() {
                if let Some(w) = weak.upgrade() {
                    w.global::<VisualizerState>().set_transient(x);
                }
                last_tr = x.max(last_tr);
                last_beat = x.max(last_beat);
            }

            // WGPU UNDERLAY SPIKE: render one GPU shader frame into the wgpu
            // texture and hand it to ImmersiveState. Only runs while a shader
            // scene is active (shader-mode > 0) AND the immersive view is OPEN
            // — the close handlers clear `open` but deliberately keep
            // shader-mode (reopening restores the scene), so without the open
            // check the fragment pass would keep running invisibly after close.
            // The device/queue were captured by the rendering notifier
            // (main.rs); render_frame is a no-op until that fires.
            last_tr *= 0.85;
            last_beat *= 0.88;
            if let Some(w) = weak.upgrade() {
                let imm = w.global::<ImmersiveState>();
                let m = imm.get_shader_mode();
                if m > 0 && imm.get_open() {
                    // Derive the enriched audio pack from the latched cells.
                    let mut bands8 = [0.0f32; 8];
                    for i in 0..8 {
                        bands8[i] = (last_bars16[2 * i] + last_bars16[2 * i + 1]) * 0.5;
                    }
                    let level = (last_energy[0]
                        + last_energy[1]
                        + last_energy[2]
                        + last_energy[3]
                        + last_energy[4])
                        * 0.2;
                    last_level_smooth = last_level_smooth * 0.96 + level * 0.04;
                    // Forward-motion clock: host-side (rate is audio-dependent),
                    // wrapped at an integer so fract()-based ring patterns stay
                    // continuous across the wrap.
                    last_phase += 0.012 + level * 0.02 + last_beat * 0.02;
                    if last_phase >= 4096.0 {
                        last_phase -= 4096.0;
                    }
                    // Real-time ceiling (mode 4): the highest band with signal,
                    // smoothed (EMA) so the line tracks the audio without jitter.
                    if m == 4 {
                        if let Some(bins) = new_spectral.as_ref() {
                            let n = bins.len();
                            if n > 1 {
                                let mut hi = 0usize;
                                for (i, &v) in bins.iter().enumerate() {
                                    if v > 0.05 {
                                        hi = i;
                                    }
                                }
                                let target = hi as f32 / (n - 1) as f32;
                                last_peak = last_peak * 0.85 + target * 0.15;
                            }
                        }
                    }
                    // Spectral feed: mode 4 (ribbon) AND mode 5 (line bed) both
                    // consume the 512-band frame. The ribbon also needs the
                    // playback fraction + a reset (track change / seek).
                    let sp = if m == 4 || m == 5 { new_spectral.take() } else { None };
                    let (progress, reset) = if m == 4 {
                        let np = w.global::<NowPlayingState>();
                        let tid = np.get_track_id().to_string();
                        let prog = np.get_progress();
                        let rst = tid != last_track_id
                            || prog + 0.01 < last_progress
                            || prog > last_progress + 0.15;
                        last_track_id = tid;
                        last_progress = prog;
                        (prog, rst)
                    } else {
                        (0.0, false)
                    };
                    let audio = crate::shader_underlay::FrameAudio {
                        level,
                        level_smooth: last_level_smooth,
                        beat: last_beat,
                        phase: last_phase,
                        transient: last_tr,
                        energy: last_energy,
                        bands: bands8,
                        spectral: sp,
                        progress,
                        reset,
                        spectral_peak: last_peak,
                    };
                    // Window physical size → the underlay clamps its offscreen
                    // target to it (capped at its 2560x1440 ceiling).
                    let win_size = w.window().size();
                    if let Some(img) = crate::shader_underlay::render_frame(
                        m,
                        &audio,
                        win_size.width,
                        win_size.height,
                    ) {
                        imm.set_shader_texture(img);
                    }
                }
            }
        },
    );
    // The drain only needs to run while the tap captures (it used to tick for
    // the whole app lifetime doing lock/None-takes). Register the callback via
    // start(), then park it stopped; the set-enabled handler below restarts /
    // stops it together with the tap. All on the UI thread.
    timer.stop();
    DRAIN_TIMER.with(|t| *t.borrow_mut() = Some(timer));

    // set-enabled → toggle capture on the tap, wake the parked FFT producer,
    // and start/stop the UI drain timer. Registered AFTER the timer is stored
    // so any invoke — including the initial seed in main.rs, which runs right
    // after install() — always finds it.
    {
        let tap = tap.clone();
        st.on_set_enabled(move |on| {
            tap.set_enabled(on);
            if on {
                // The producer parks (park_timeout) while disabled; unpark for
                // an instant wake instead of waiting out its idle poll.
                fft_thread.unpark();
            }
            DRAIN_TIMER.with(|t| {
                if let Some(timer) = t.borrow().as_ref() {
                    if on {
                        timer.restart();
                    } else {
                        timer.stop();
                    }
                }
            });
        });
    }
    log::info!("[viz] producer + 30fps drain installed (idle until the tap enables)");
}
