//! Data models for local library

use serde::{Deserialize, Serialize};

/// Supported audio formats
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AudioFormat {
    Flac,
    Alac,
    Wav,
    Aiff,
    Ape,
    Unknown,
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self::Unknown
    }
}

impl std::fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioFormat::Flac => write!(f, "FLAC"),
            AudioFormat::Alac => write!(f, "ALAC"),
            AudioFormat::Wav => write!(f, "WAV"),
            AudioFormat::Aiff => write!(f, "AIFF"),
            AudioFormat::Ape => write!(f, "APE"),
            AudioFormat::Unknown => write!(f, "Unknown"),
        }
    }
}

/// A track from the local library
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTrack {
    pub id: i64,
    pub file_path: String,

    // Metadata
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,

    // Audio properties
    pub duration_secs: u64,
    pub format: AudioFormat,
    pub bit_depth: Option<u32>,
    pub sample_rate: u32,
    pub channels: u8,
    pub file_size_bytes: u64,

    // CUE support
    pub cue_file_path: Option<String>,
    pub cue_start_secs: Option<f64>,
    pub cue_end_secs: Option<f64>,

    // Artwork
    pub artwork_path: Option<String>,

    // Indexing
    pub last_modified: i64,
    pub indexed_at: i64,
}

impl Default for LocalTrack {
    fn default() -> Self {
        Self {
            id: 0,
            file_path: String::new(),
            title: String::new(),
            artist: "Unknown Artist".to_string(),
            album: "Unknown Album".to_string(),
            album_artist: None,
            track_number: None,
            disc_number: None,
            year: None,
            genre: None,
            duration_secs: 0,
            format: AudioFormat::Unknown,
            bit_depth: None,
            sample_rate: 44100,
            channels: 2,
            file_size_bytes: 0,
            cue_file_path: None,
            cue_start_secs: None,
            cue_end_secs: None,
            artwork_path: None,
            last_modified: 0,
            indexed_at: 0,
        }
    }
}

/// An album aggregated from local tracks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAlbum {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<u32>,
    pub artwork_path: Option<String>,
    pub track_count: u32,
    pub total_duration_secs: u64,
    pub format: AudioFormat,
    pub directory_path: String,
}

/// An artist aggregated from local tracks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalArtist {
    pub name: String,
    pub album_count: u32,
    pub track_count: u32,
}

/// Scan progress for UI updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub status: ScanStatus,
    pub total_files: u32,
    pub processed_files: u32,
    pub current_file: Option<String>,
    pub errors: Vec<ScanError>,
}

impl Default for ScanProgress {
    fn default() -> Self {
        Self {
            status: ScanStatus::Idle,
            total_files: 0,
            processed_files: 0,
            current_file: None,
            errors: Vec::new(),
        }
    }
}

/// Scan status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScanStatus {
    Idle,
    Scanning,
    Complete,
    Error,
}

/// A scan error for a specific file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanError {
    pub file_path: String,
    pub error: String,
}

/// Audio properties extracted from a file
#[derive(Debug, Clone, Default)]
pub struct AudioProperties {
    pub duration_secs: u64,
    pub bit_depth: Option<u32>,
    pub sample_rate: u32,
    pub channels: u8,
}
