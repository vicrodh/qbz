//! Mixtapes & Collections backend — schema, repository, shuffle/DJ-mix, and
//! enqueue/resolution logic for QBZ.
//!
//! Extracted verbatim from `src-tauri/src/mixtape/*` and re-typed against the
//! real shared crates (`qbz-models`, `qbz-qobuz`, `qbz-library`, `qbz-plex`),
//! so it runs headless with no Tauri state and no `#[tauri::command]` wrappers
//! (ADR-005: no legacy wrappers; ADR-006: frontend-agnostic core).
//!
//! - [`schema`]  — SQLite migrations (`run_mixtape_migrations`).
//! - [`repo`]    — CRUD over `&Connection` / `&mut Connection`.
//! - [`shuffle`] — pure DJ-mix sampler (`rand`, `strsim`).
//! - [`enqueue`] — `ItemResolver` trait, `resolve_collection_tracks`,
//!   `shuffle_items`, `next_item_index` / `previous_item_index`, plus the
//!   free resolver fns + `ProdItemResolver` typed against the shared crates.

pub mod enqueue;
pub mod repo;
pub mod schema;
pub mod shuffle;

// Convenience re-exports for callers (the Slint app in a later slice).
pub use enqueue::{
    next_item_index, previous_item_index, resolve_collection_tracks, ItemResolver,
    ProdItemResolver,
};
pub use schema::run_mixtape_migrations;
