//! `qbz-log` — frontend-agnostic logging core for the qbz desktop client.
//!
//! It owns a composite [`log::Log`] implementation ([`tee::TeeLogger`]) that wraps
//! `env_logger`'s built `Logger` and fans every record out to three sinks:
//!   1. **stderr** (redacted text, same line format as the file sink),
//!   2. a bounded **in-memory ring** ([`ring`], cap [`ring::RING_CAP`]), and
//!   3. an **on-disk file** (`~/.local/share/qbz/logs/qbz.log`, prev-rotated at startup).
//!
//! Secret **redaction** ([`redact`]) is applied once at the single write choke point,
//! so every downstream consumer (stderr, ring, file, clipboard, paste upload) gets clean text.
//!
//! This crate is network-free and UI-free: no `reqwest`, no `tokio`, no `slint`.

pub mod bundle;
pub mod install;
pub mod line;
pub mod redact;
pub mod ring;
pub mod tee;

pub use bundle::{format_diagnostics_bundle, DiagFields};
pub use install::{install, set_level};
pub use line::LogLine;
pub use redact::{redact, register_secret};
