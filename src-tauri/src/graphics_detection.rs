//! Tauri adapter for host GPU detection.
//!
//! The portable detection and preference parsing live in `qbz-app`; Tauri keeps
//! the existing import surface and applies the resulting env actions in `main.rs`.

pub use qbz_app::graphics_detection::*;
