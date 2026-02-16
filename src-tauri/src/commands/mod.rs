//! Legacy commands module.
//!
//! NOTE:
//! The active frontend/runtime contract is V2 (`runtime_*` + `v2_*`) only.
//! This module keeps only the minimal internal legacy pieces still referenced
//! by non-frontend subsystems while hard-delete migration is in progress.

pub mod playback;
pub mod search;
pub mod user_session;
pub use playback::*;
pub use user_session::*;
