//! Filesystem scanner for audio files

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::LibraryError;

/// Supported audio file extensions
const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &["flac", "m4a", "wav", "aiff", "aif", "ape", "mp3"];

/// CUE file extension
const CUE_EXTENSION: &str = "cue";

/// Result of scanning a directory
#[derive(Debug, Default)]
pub struct ScanResult {
    /// Audio files found
    pub audio_files: Vec<PathBuf>,
    /// CUE files found
    pub cue_files: Vec<PathBuf>,
}

/// Library scanner for discovering audio files
pub struct LibraryScanner;

impl LibraryScanner {
    /// Create a new scanner
    pub fn new() -> Self {
        Self
    }

    /// Scan a directory recursively for audio and CUE files
    pub fn scan_directory(&self, path: &Path) -> Result<ScanResult, LibraryError> {
        if !path.exists() {
            return Err(LibraryError::InvalidPath(format!(
                "Path does not exist: {}",
                path.display()
            )));
        }

        if !path.is_dir() {
            return Err(LibraryError::InvalidPath(format!(
                "Path is not a directory: {}",
                path.display()
            )));
        }

        let mut result = ScanResult::default();

        for entry in WalkDir::new(path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();

                if Self::is_supported_audio_extension(&ext_lower) {
                    result.audio_files.push(path.to_path_buf());
                } else if ext_lower == CUE_EXTENSION {
                    result.cue_files.push(path.to_path_buf());
                }
            }
        }

        log::info!(
            "Scanned {}: found {} audio files, {} CUE files",
            path.display(),
            result.audio_files.len(),
            result.cue_files.len()
        );

        Ok(result)
    }

    /// Check if an extension is a supported audio format
    fn is_supported_audio_extension(ext: &str) -> bool {
        SUPPORTED_AUDIO_EXTENSIONS.contains(&ext)
    }

    /// Get all supported extensions (for UI display)
    pub fn supported_extensions() -> &'static [&'static str] {
        SUPPORTED_AUDIO_EXTENSIONS
    }
}

impl Default for LibraryScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_extensions() {
        assert!(LibraryScanner::is_supported_audio_extension("flac"));
        assert!(LibraryScanner::is_supported_audio_extension("wav"));
        assert!(LibraryScanner::is_supported_audio_extension("m4a"));
        assert!(LibraryScanner::is_supported_audio_extension("mp3"));
        assert!(!LibraryScanner::is_supported_audio_extension("txt"));
    }
}
