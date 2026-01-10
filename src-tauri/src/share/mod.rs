//! Share module for generating universal song links
//!
//! This module provides functionality to generate shareable links
//! that work across multiple music streaming platforms using Odesli/song.link.

pub mod errors;
pub mod models;
pub mod songlink;

pub use errors::ShareError;
pub use models::{ContentType, SongLinkResponse};
pub use songlink::SongLinkClient;
