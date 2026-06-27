//! Frontend-agnostic artist-vector recommendation engine (ADR-006).
//!
//! Cleanroom port of Tauri's `src-tauri/src/artist_vectors/` into a shared
//! crate so the Slint frontend (and any headless caller) can produce a
//! playlist's "Suggested Songs" without a `tauri::State` dependency.
//!
//! Modules are ported 1:1 from the Tauri source. The dead cosine-similarity /
//! `find_nearest` ranking path is dropped (production ranks by summed
//! relationship weight via the vector store) per the epic's decision D3.

mod sparse_vector;

pub use sparse_vector::SparseVector;
