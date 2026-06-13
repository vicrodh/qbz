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
use crate::{AppWindow, VisualizerState};

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
    /// firing). Lives on the UI thread, like the models it writes.
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

    // set-enabled → toggle capture on the tap directly.
    {
        let tap = tap.clone();
        st.on_set_enabled(move |on| tap.set_enabled(on));
    }

    // Producer thread: computes the five streams, latches each into its cell.
    let cells = Arc::new(VizCells::default());
    let sink = Arc::new(SlintVizSink {
        cells: cells.clone(),
    });
    let _ = spawn_visualizer_thread(tap, sink);

    // ~30 fps drain on the UI thread: copy the latest frames into the models.
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(33),
        move || {
            if let Some(b) = cells.bars.lock().unwrap().take() {
                for (i, v) in b.iter().enumerate() {
                    bars.set_row_data(i, *v);
                }
            }
            if let Some(b) = cells.energy.lock().unwrap().take() {
                for (i, v) in b.iter().enumerate() {
                    energy.set_row_data(i, *v);
                }
            }
            if let Some(b) = cells.spectral.lock().unwrap().take() {
                for (i, v) in b.iter().enumerate() {
                    spectral.set_row_data(i, *v);
                }
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
            }
        },
    );
    DRAIN_TIMER.with(|t| *t.borrow_mut() = Some(timer));
    log::info!("[viz] producer + 30fps drain installed (tap disabled until immersive opens)");
}
