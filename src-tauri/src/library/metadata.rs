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

    fn strip_disc_suffix(title: &str) -> String {
        let trimmed = title.trim();

        for (open, close) in [("(", ")"), ("[", "]")] {
            if trimmed.ends_with(close) {
                if let Some(start) = trimmed.rfind(open) {
                    let inside = trimmed[start + 1..trimmed.len() - 1].trim();
                    if Self::is_disc_designator(inside) {
                        return trimmed[..start].trim().to_string();
                    }
                }
            }
        }

        let tokens: Vec<&str> = trimmed
            .split_whitespace()
            .filter(|token| *token != "-" && *token != "–" && *token != "—")
            .collect();

        if tokens.len() >= 2 {
            let last = tokens[tokens.len() - 1];
            let prev = tokens[tokens.len() - 2];
            if Self::is_disc_marker(prev) && last.chars().all(|c| c.is_ascii_digit()) {
                return tokens[..tokens.len() - 2].join(" ").trim().to_string();
            }
        }

        if let Some(last) = tokens.last() {
            if Self::is_disc_designator(last) {
                if tokens.len() > 1 {
                    return tokens[..tokens.len() - 1].join(" ").trim().to_string();
                }
            }
        }

        trimmed.to_string()
    }

    fn is_disc_marker(value: &str) -> bool {
        matches!(value.to_lowercase().as_str(), "disc" | "disk" | "cd")
    }

    fn is_disc_designator(value: &str) -> bool {
        let cleaned: String = value
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();

        if cleaned.starts_with("disc") {
            let rest = &cleaned[4..];
            return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
        }
        if cleaned.starts_with("disk") {
            let rest = &cleaned[4..];
            return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
        }
        if cleaned.starts_with("cd") {
            let rest = &cleaned[2..];
            return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
        }

        false
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

    fn disc_number_from_name(name: &str) -> Option<u32> {
        let lower = name.to_lowercase();
        let tokens: Vec<&str> = lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();

        for (i, token) in tokens.iter().enumerate() {
            if (*token == "disc" || *token == "disk" || *token == "cd")
                && tokens
                    .get(i + 1)
                    .map_or(false, |t| t.chars().all(|c| c.is_ascii_digit()))
            {
                if let Some(next) = tokens.get(i + 1) {
                    if let Ok(value) = next.parse::<u32>() {
                        if value > 0 {
                            return Some(value);
                        }
                    }
                }
            }

            for prefix in ["disc", "disk", "cd"] {
                if token.starts_with(prefix) {
                    let rest = &token[prefix.len()..];
                    if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                        if let Ok(value) = rest.parse::<u32>() {
                            if value > 0 {
                                return Some(value);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    pub fn infer_disc_number(file_path: &Path) -> Option<u32> {
        let parent_dir = file_path.parent()?;
        let parent_name = parent_dir.file_name()?.to_str()?;
        if !Self::is_disc_folder(parent_name) {
            return None;
        }
        Self::disc_number_from_name(parent_name)
    }

    fn album_root_dir(file_path: &Path) -> Option<PathBuf> {
        let parent_dir = file_path.parent()?;
        let parent_name = parent_dir.file_name().and_then(|s| s.to_str());

        if let Some(name) = parent_name {
            if Self::is_disc_folder(name) {
                return parent_dir.parent().map(|p| p.to_path_buf());
            }
        }

        Some(parent_dir.to_path_buf())
    }

    fn infer_artist_album(file_path: &Path) -> (Option<String>, Option<String>) {
        let album_dir = Self::album_root_dir(file_path);
        let album_name = album_dir
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .map(Self::strip_year_suffix);

        let artist_name = album_dir
            .as_ref()
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

    pub fn album_group_info(file_path: &Path, tag_album: Option<&str>) -> (String, String) {
        let album_dir = Self::album_root_dir(file_path);
        let group_key = album_dir
            .as_ref()
            .map(|dir| dir.to_string_lossy().to_string())
            .unwrap_or_else(|| file_path.to_string_lossy().to_string());

        let mut group_title = tag_album
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .filter(|value| !value.eq_ignore_ascii_case("Unknown Album"))
            .map(|value| value.to_string())
            .or_else(|| {
                album_dir
                    .as_ref()
                    .and_then(|dir| dir.file_name())
                    .and_then(|s| s.to_str())
                    .map(Self::strip_year_suffix)
            })
            .unwrap_or_else(|| "Unknown Album".to_string());

        group_title = Self::strip_disc_suffix(&group_title);

        (group_key, group_title)
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
        let sample_rate = properties.sample_rate().unwrap_or(44100) as f64;  // Convert to f64 for decimal precision
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

        let inferred_disc = Self::infer_disc_number(file_path);

        // Build track
        let track = if let Some(tag) = tag {
            let album_title = Self::normalize_field(tag.album().as_deref())
                .or_else(|| fallback_album.clone())
                .unwrap_or_else(|| "Unknown Album".to_string());
            let (album_group_key, album_group_title) =
                Self::album_group_info(file_path, Some(album_title.as_str()));

            LocalTrack {
                id: 0,
                file_path: file_path.to_string_lossy().to_string(),
                title: tag
                    .title()
                    .map(|s| s.to_string())
                    .unwrap_or(filename),
                artist: Self::normalize_field(tag.artist().as_deref())
                    .or_else(|| fallback_artist.clone())
                    .unwrap_or_else(|| "Unknown Artist".to_string()),
                album: album_title,
                album_artist: tag.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string()),
                album_group_key,
                album_group_title,
                track_number: tag.track().map(|t| t as u32),
                disc_number: tag
                    .disk()
                    .and_then(|d| if d > 0 { Some(d as u32) } else { None })
                    .or(inferred_disc),
                year: tag.year().map(|y| y as u32),
                genre: tag.genre().map(|s| s.to_string()),
                catalog_number: tag.get_string(&ItemKey::CatalogNumber).map(|s| s.to_string()),
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
                source: None,
                qobuz_track_id: None,
            }
        } else {
            // No tag found, use defaults
            let album_title = fallback_album
                .clone()
                .unwrap_or_else(|| "Unknown Album".to_string());
            let (album_group_key, album_group_title) =
                Self::album_group_info(file_path, Some(album_title.as_str()));

            LocalTrack {
                id: 0,
                file_path: file_path.to_string_lossy().to_string(),
                title: filename,
                artist: fallback_artist
                    .unwrap_or_else(|| "Unknown Artist".to_string()),
                album: album_title,
                album_artist: None,
                album_group_key,
                album_group_title,
                track_number: None,
                disc_number: inferred_disc,
                year: None,
                genre: None,
                catalog_number: None,
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
                source: None,
                qobuz_track_id: None,
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
            sample_rate: properties.sample_rate().unwrap_or(44100) as f64,  // Convert to f64
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
            Some("mp3") => AudioFormat::Mp3,
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

    /// Copy an existing artwork file into the cache directory
    /// Returns path to cached artwork or None
    pub fn cache_artwork_file(artwork_path: &Path, cache_dir: &Path) -> Option<String> {
        if !artwork_path.is_file() {
            return None;
        }

        let ext = artwork_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "jpg".to_string());

        let normalized_ext = match ext.as_str() {
            "jpeg" => "jpg",
            "png" | "jpg" | "gif" | "bmp" | "webp" => ext.as_str(),
            _ => "jpg",
        };

        let hash = Self::simple_hash(&artwork_path.to_string_lossy());
        let cached_name = format!("local_{:x}.{}", hash, normalized_ext);
        let cached_path = cache_dir.join(cached_name);

        if cached_path.exists() {
            return Some(cached_path.to_string_lossy().to_string());
        }

        fs::create_dir_all(cache_dir).ok()?;
        fs::copy(artwork_path, &cached_path).ok()?;

        Some(cached_path.to_string_lossy().to_string())
    }

    /// Simple hash function for generating filenames
    fn simple_hash(s: &str) -> u64 {
        let mut hash: u64 = 5381;
        for byte in s.bytes() {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
        }
        hash
    }

    /// Look for folder artwork by file name heuristics.
    /// Returns the path if found.
    pub fn find_folder_artwork(
        audio_file_path: &Path,
        album_title: Option<&str>,
    ) -> Option<String> {
        let parent_dir = audio_file_path.parent()?;
        let album_dir =
            Self::album_root_dir(audio_file_path).unwrap_or_else(|| parent_dir.to_path_buf());

        let mut dirs_to_check: Vec<PathBuf> = Vec::new();
        if album_dir != parent_dir {
            dirs_to_check.push(album_dir.clone());
        }
        dirs_to_check.push(parent_dir.to_path_buf());

        let album_key = album_title
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .filter(|value| !value.eq_ignore_ascii_case("Unknown Album"))
            .map(Self::strip_disc_suffix)
            .and_then(|value| Self::normalize_artwork_key(&value));
        let folder_key = album_dir
            .file_name()
            .and_then(|s| s.to_str())
            .map(Self::strip_disc_suffix)
            .and_then(|value| Self::normalize_artwork_key(&value));

        let mut best: Option<(PathBuf, i32)> = None;
        let mut best_score = 0;
        let mut candidate_count = 0;

        for (index, dir) in dirs_to_check.iter().enumerate() {
            let dir_bonus = if index == 0 { 5 } else { 0 };
            let entries = match fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !Self::is_supported_artwork_ext(&ext) {
                    continue;
                }

                candidate_count += 1;
                let file_stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .trim();
                let file_key = match Self::normalize_artwork_key(file_stem) {
                    Some(key) => key,
                    None => {
                        let fallback = file_stem.to_lowercase();
                        if fallback.trim().is_empty() {
                            continue;
                        }
                        fallback
                    }
                };

                let mut score = Self::artwork_score(
                    &file_key,
                    album_key.as_deref(),
                    folder_key.as_deref(),
                );
                if score == 0 {
                    score = 5;
                }
                score += dir_bonus;

                if score > best_score {
                    best_score = score;
                    best = Some((path, score));
                }
            }
        }

        if let Some((path, score)) = best {
            if score >= 10 || candidate_count == 1 {
                return Some(path.to_string_lossy().to_string());
            }
        }

        None
    }

    fn normalize_artwork_key(value: &str) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        let normalized: String = trimmed
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    }

    fn is_supported_artwork_ext(ext: &str) -> bool {
        matches!(ext, "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp")
    }

    fn artwork_score(
        file_key: &str,
        album_key: Option<&str>,
        folder_key: Option<&str>,
    ) -> i32 {
        const EXACT: &[&str] = &["cover", "folder", "front", "album", "artwork", "art"];
        let mut score = 0;

        if EXACT.iter().any(|name| *name == file_key) {
            score = score.max(100);
        }
        if let Some(key) = album_key {
            if file_key == key {
                score = score.max(95);
            } else if file_key.contains(key) {
                score = score.max(70);
            }
        }
        if let Some(key) = folder_key {
            if file_key == key {
                score = score.max(90);
            } else if file_key.contains(key) {
                score = score.max(65);
            }
        }
        if EXACT.iter().any(|name| file_key.contains(name)) {
            score = score.max(80);
        }

        score
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
            AudioFormat::Mp3
        );
    }
}
