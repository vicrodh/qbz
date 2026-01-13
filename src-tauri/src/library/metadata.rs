//! Metadata extraction for audio files

use lofty::{Accessor, AudioFile, ItemKey, MimeType, Probe, TaggedFileExt};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::library::{AudioFormat, AudioProperties, LibraryError, LocalTrack};

/// Metadata extractor using lofty
pub struct MetadataExtractor;

impl MetadataExtractor {
    fn normalize_field(value: Option<&str>) -> Option<String> {
        value
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    }

    fn strip_year_suffix(name: &str) -> String {
        let trimmed = name.trim();
        for (open, close) in [("(", ")"), ("[", "]")] {
            if trimmed.ends_with(close) {
                if let Some(start) = trimmed.rfind(open) {
                    let inside = &trimmed[start + 1..trimmed.len() - 1];
                    if inside.len() == 4 && inside.chars().all(|c| c.is_ascii_digit()) {
                        return trimmed[..start].trim().to_string();
                    }
                }
            }
        }
        trimmed.to_string()
    }

    fn is_disc_folder(name: &str) -> bool {
        let lower = name.to_lowercase();
        let tokens: Vec<&str> = lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();

        for (i, token) in tokens.iter().enumerate() {
            let has_digits = token.chars().any(|c| c.is_ascii_digit());
            if (*token == "disc" || *token == "disk" || *token == "cd") && (has_digits || tokens.get(i + 1).map_or(false, |t| t.chars().all(|c| c.is_ascii_digit()))) {
                return true;
            }
            if token.starts_with("disc") {
                let rest = &token[4..];
                if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                    return true;
                }
            }
            if token.starts_with("disk") {
                let rest = &token[4..];
                if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                    return true;
                }
            }
            if token.starts_with("cd") {
                let rest = &token[2..];
                if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                    return true;
                }
            }
            if *token == "bonus" && tokens.get(i + 1).map_or(false, |t| *t == "disc" || *t == "disk" || *t == "cd") {
                return true;
            }
        }

        false
    }

    fn infer_artist_album(file_path: &Path) -> (Option<String>, Option<String>) {
        let parent_dir = file_path.parent();
        let parent_name = parent_dir.and_then(|p| p.file_name()).and_then(|s| s.to_str());

        let (album_dir, album_name) = if let Some(name) = parent_name {
            if Self::is_disc_folder(name) {
                let album_dir = parent_dir.and_then(|p| p.parent());
                let album_name = album_dir
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .map(Self::strip_year_suffix);
                (album_dir, album_name)
            } else {
                (parent_dir, parent_name.map(Self::strip_year_suffix))
            }
        } else {
            (None, None)
        };

        let artist_name = album_dir
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .map(Self::strip_year_suffix);

        if artist_name.is_none() {
            if let Some(album_dir_name) = album_name.as_deref() {
                if let Some((artist, album)) = album_dir_name.split_once(" - ") {
                    return (
                        Some(Self::strip_year_suffix(artist)),
                        Some(Self::strip_year_suffix(album)),
                    );
                }
            }
        }

        (artist_name, album_name)
    }

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

        let (fallback_artist, fallback_album) = Self::infer_artist_album(file_path);

        // Build track
        let track = if let Some(tag) = tag {
            LocalTrack {
                id: 0,
                file_path: file_path.to_string_lossy().to_string(),
                title: tag
                    .title()
                    .map(|s| s.to_string())
                    .unwrap_or(filename),
                artist: Self::normalize_field(tag.artist())
                    .or_else(|| fallback_artist.clone())
                    .unwrap_or_else(|| "Unknown Artist".to_string()),
                album: Self::normalize_field(tag.album())
                    .or_else(|| fallback_album.clone())
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
                artist: fallback_artist
                    .unwrap_or_else(|| "Unknown Artist".to_string()),
                album: fallback_album
                    .unwrap_or_else(|| "Unknown Album".to_string()),
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

    /// Look for folder artwork (cover.jpg, folder.jpg, etc.)
    /// Returns the path if found
    pub fn find_folder_artwork(audio_file_path: &Path) -> Option<String> {
        let parent_dir = audio_file_path.parent()?;

        // Common artwork filenames in order of preference
        const ARTWORK_NAMES: &[&str] = &[
            "cover.jpg", "cover.jpeg", "cover.png",
            "folder.jpg", "folder.jpeg", "folder.png",
            "front.jpg", "front.jpeg", "front.png",
            "album.jpg", "album.jpeg", "album.png",
            "Cover.jpg", "Cover.jpeg", "Cover.png",
            "Folder.jpg", "Folder.jpeg", "Folder.png",
        ];

        let mut dirs_to_check: Vec<PathBuf> = Vec::new();
        dirs_to_check.push(parent_dir.to_path_buf());

        if let Some(parent_name) = parent_dir.file_name().and_then(|s| s.to_str()) {
            if Self::is_disc_folder(parent_name) {
                if let Some(album_dir) = parent_dir.parent() {
                    dirs_to_check.push(album_dir.to_path_buf());
                }
            }
        }

        for dir in dirs_to_check {
            if let Some(path) = Self::find_artwork_in_dir(&dir, ARTWORK_NAMES) {
                return Some(path);
            }
        }

        None
    }

    fn find_artwork_in_dir(dir: &Path, names: &[&str]) -> Option<String> {
        for name in names {
            let artwork_path = dir.join(name);
            if artwork_path.exists() {
                return Some(artwork_path.to_string_lossy().to_string());
            }
        }

        let mut candidates: Vec<PathBuf> = Vec::new();
        for entry in fs::read_dir(dir).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if ext == "jpg" || ext == "jpeg" || ext == "png" {
                candidates.push(path);
            }
        }

        if candidates.len() == 1 {
            return Some(candidates[0].to_string_lossy().to_string());
        }

        None
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
