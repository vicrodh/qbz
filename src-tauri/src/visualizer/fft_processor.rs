//! Tauri adapter for the frontend-agnostic visualizer producer.
//!
//! The DSP now lives in `qbz_audio::visualizer::processor`. This file only maps
//! each computed [`VizFrame`] back to the historical `viz:*` Tauri events, with a
//! byte-identical little-endian `f32` payload, so the Svelte frontend listeners
//! are unchanged.

use qbz_audio::visualizer::{VizFrame, VizSink};
use tauri::{AppHandle, Emitter};

/// Re-emits visualization frames as the legacy binary `viz:*` Tauri events.
pub struct TauriVizSink {
    app: AppHandle,
}

impl TauriVizSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl VizSink for TauriVizSink {
    fn submit(&self, frame: VizFrame) {
        match frame {
            VizFrame::Viz16(bars) => {
                let bytes: Vec<u8> = bars.iter().flat_map(|f| f.to_le_bytes()).collect();
                let _ = self.app.emit("viz:data", bytes);
            }
            VizFrame::Wave256x2(wave) => {
                let bytes: Vec<u8> = wave.iter().flat_map(|f| f.to_le_bytes()).collect();
                let _ = self.app.emit("viz:waveform", bytes);
            }
            VizFrame::Spectral512(bands) => {
                let bytes: Vec<u8> = bands.iter().flat_map(|f| f.to_le_bytes()).collect();
                let _ = self.app.emit("viz:spectral", bytes);
            }
            VizFrame::Energy5(energy) => {
                let bytes: Vec<u8> = energy.iter().flat_map(|f| f.to_le_bytes()).collect();
                let _ = self.app.emit("viz:energy", bytes);
            }
            VizFrame::Transient1(intensity) => {
                let _ = self.app.emit("viz:transient", intensity.to_le_bytes().to_vec());
            }
        }
    }
}
