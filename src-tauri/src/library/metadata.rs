//! Metadata extraction for audio files

use lofty::{Accessor, AudioFile, ItemKey, MimeType, Probe, TaggedFileExt};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::library::{AudioFormat, AudioProperties, LibraryError, LocalTrack};

/// Metadata extractor using lofty
pub struct MetadataExtractor;

impl MetadataExtractor {
    /// Extract metadata from an audio file
    pub fn extract(file_path: &Path) -> Result<LocalTrack, LibraryError> {
        log::debug!("Extracting metadata from: {}", file_path.display());

        // Probe the file
        let tagged_file = Probe::open(file_path)
            .map_err(|e| LibraryError::Metadata(format!("Failed to open file: {}", e)))?
            .read()
            .map_err(|e| LibraryError::Metadata(format!("Failed to read file: {}", e)))?;

        // Get the primary tag (prefer ID3v2/Vorbis/APE)
        let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());

        // Get audio properties
        let properties = tagged_file.properties();
        let duration_secs = properties.duration().as_secs();
        let sample_rate = properties.sample_rate().unwrap_or(44100);
        let bit_depth = properties.bit_depth().map(|b| b as u32);
        let channels = properties.channels().unwrap_or(2) as u8;

        // Get file metadata
        let file_metadata = fs::metadata(file_path).map_err(LibraryError::Io)?;
        let file_size_bytes = file_metadata.len();
        let last_modified = file_metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Detect format
        let format = Self::detect_format(file_path);

        // Get filename for fallback title
        let filename = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();

        // Build track
        let track = if let Some(tag) = tag {
            LocalTrack {
                id: 0,
                file_path: file_path.to_string_lossy().to_string(),
                title: tag.title().map(|s| s.to_string()).unwrap_or(filename),
                artist: tag
                    .artist()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "Unknown Artist".to_string()),
                album: tag
                    .album()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "Unknown Album".to_string()),
                album_artist: tag.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string()),
                track_number: tag.track().map(|t| t as u32),
                disc_number: tag.disk().map(|d| d as u32),
                year: tag.year().map(|y| y as u32),
                genre: tag.genre().map(|s| s.to_string()),
                duration_secs,
                format,
                bit_depth,
                sample_rate,
                channels,
                file_size_bytes,
                cue_file_path: None,
                cue_start_secs: None,
                cue_end_secs: None,
                artwork_path: None,
                last_modified,
                indexed_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            }
        } else {
            // No tag found, use defaults
            LocalTrack {
                id: 0,
                file_path: file_path.to_string_lossy().to_string(),
                title: filename,
                artist: "Unknown Artist".to_string(),
                album: "Unknown Album".to_string(),
                album_artist: None,
                track_number: None,
                disc_number: None,
                year: None,
                genre: None,
                duration_secs,
                format,
                bit_depth,
                sample_rate,
                channels,
                file_size_bytes,
                cue_file_path: None,
                cue_start_secs: None,
                cue_end_secs: None,
                artwork_path: None,
                last_modified,
                indexed_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            }
        };

        Ok(track)
    }

    /// Extract audio properties without full metadata
    pub fn extract_properties(file_path: &Path) -> Result<AudioProperties, LibraryError> {
        let tagged_file = Probe::open(file_path)
            .map_err(|e| LibraryError::Metadata(format!("Failed to open file: {}", e)))?
            .read()
            .map_err(|e| LibraryError::Metadata(format!("Failed to read file: {}", e)))?;

        let properties = tagged_file.properties();

        Ok(AudioProperties {
            duration_secs: properties.duration().as_secs(),
            bit_depth: properties.bit_depth().map(|b| b as u32),
            sample_rate: properties.sample_rate().unwrap_or(44100),
            channels: properties.channels().unwrap_or(2) as u8,
        })
    }

    /// Determine AudioFormat from file extension
    pub fn detect_format(path: &Path) -> AudioFormat {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .as_deref()
        {
            Some("flac") => AudioFormat::Flac,
            Some("m4a") => AudioFormat::Alac,
            Some("wav") => AudioFormat::Wav,
            Some("aiff") | Some("aif") => AudioFormat::Aiff,
            Some("ape") => AudioFormat::Ape,
            _ => AudioFormat::Unknown,
        }
    }

    /// Extract and save artwork to cache directory
    /// Returns path to saved artwork or None
    pub fn extract_artwork(file_path: &Path, cache_dir: &Path) -> Option<String> {
        let tagged_file = Probe::open(file_path).ok()?.read().ok()?;
        let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag())?;

        let picture = tag.pictures().first()?;

        // Generate filename from hash of file path
        let hash = Self::simple_hash(&file_path.to_string_lossy());
        let ext = match picture.mime_type() {
            Some(MimeType::Png) => "png",
            Some(MimeType::Jpeg) => "jpg",
            Some(MimeType::Gif) => "gif",
            Some(MimeType::Bmp) => "bmp",
            _ => "jpg",
        };

        let artwork_filename = format!("{:x}.{}", hash, ext);
        let artwork_path = cache_dir.join(&artwork_filename);

        // Skip if already exists
        if artwork_path.exists() {
            return Some(artwork_path.to_string_lossy().to_string());
        }

        // Ensure cache dir exists
        fs::create_dir_all(cache_dir).ok()?;

        // Write artwork
        fs::write(&artwork_path, picture.data()).ok()?;

        Some(artwork_path.to_string_lossy().to_string())
    }

    /// Simple hash function for generating filenames
    fn simple_hash(s: &str) -> u64 {
        let mut hash: u64 = 5381;
        for byte in s.bytes() {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
        }
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_format() {
        assert_eq!(
            MetadataExtractor::detect_format(Path::new("test.flac")),
            AudioFormat::Flac
        );
        assert_eq!(
            MetadataExtractor::detect_format(Path::new("test.m4a")),
            AudioFormat::Alac
        );
        assert_eq!(
            MetadataExtractor::detect_format(Path::new("test.wav")),
            AudioFormat::Wav
        );
        assert_eq!(
            MetadataExtractor::detect_format(Path::new("test.mp3")),
            AudioFormat::Unknown
        );
    }
}
