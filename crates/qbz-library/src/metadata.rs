//! Metadata extraction for audio files

use lofty::{Accessor, AudioFile, ItemKey, Probe, TaggedFileExt};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{AudioFormat, AudioProperties, LibraryError, LocalTrack};
use crate::thumbnails::{generate_thumbnail, generate_thumbnail_from_bytes};

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

        // Must contain at least one disc-related token
        let has_disc_token = tokens.iter().any(|token| {
            *token == "disc" || *token == "disk" || *token == "cd"
                || (token.starts_with("disc") && token[4..].chars().all(|c| c.is_ascii_digit()) && !token[4..].is_empty())
                || (token.starts_with("disk") && token[4..].chars().all(|c| c.is_ascii_digit()) && !token[4..].is_empty())
                || (token.starts_with("cd") && token[2..].chars().all(|c| c.is_ascii_digit()) && !token[2..].is_empty())
        });

        if !has_disc_token {
            return false;
        }

        // A genuine disc folder name consists ONLY of disc-related tokens,
        // digits, and common modifiers like "bonus". If other words remain
        // after filtering these out, the name is an album title that happens
        // to contain "Disc 1" etc., not a standalone disc folder.
        // Examples that ARE disc folders: "Disc 1", "CD2", "Bonus Disc", "disc01"
        // Examples that are NOT: "Relaxation Disc1", "Now 75 - CD1",
        //   "100 Popular Classics, Disc 1"
        let has_extra_words = tokens.iter().any(|token| {
            // Pure digits are fine
            if token.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
            // Disc keywords are fine
            if *token == "disc" || *token == "disk" || *token == "cd" {
                return false;
            }
            // Disc+number compounds are fine (disc1, cd02, etc.)
            for prefix in &["disc", "disk", "cd"] {
                if token.starts_with(prefix) {
                    let rest = &token[prefix.len()..];
                    if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                        return false;
                    }
                }
            }
            // Common disc folder modifiers are fine
            if matches!(*token, "bonus" | "extra" | "side" | "part") {
                return false;
            }
            // Anything else means this is not a pure disc folder
            true
        });

        !has_extra_words
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

    /// Try to extract a track number from the filename.
    /// Handles common patterns like:
    /// - "01 - Title.flac"
    /// - "01. Title.flac"
    /// - "01_Title.flac"
    /// - "Track 01.flac"
    /// - "1-01 Title.flac" (disc-track)
    pub fn infer_track_number_from_filename(file_path: &Path) -> Option<u32> {
        let stem = file_path.file_stem()?.to_str()?;
        let trimmed = stem.trim();

        // Pattern: starts with digits
        if let Some(cap) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()) {
            // Collect leading digits
            let digit_end = 1 + cap.chars().take_while(|c| c.is_ascii_digit()).count();
            let num_str = &trimmed[..digit_end];
            let rest = &trimmed[digit_end..];

            // Check for disc-track pattern FIRST: "D-TT" like "1-01 Title", "2-05 Song"
            // Only when leading number is 1-2 digits (disc number) followed by dash+digits
            if digit_end <= 2 && rest.starts_with('-') {
                let after_dash = &rest[1..];
                let track_digits: String = after_dash.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !track_digits.is_empty() {
                    if let Ok(n) = track_digits.parse::<u32>() {
                        if n > 0 && n < 10000 {
                            return Some(n);
                        }
                    }
                }
            }

            // Regular pattern: digits followed by separator
            // "01 - Title", "01. Title", "01_Title", "01-Title"
            let rest_trimmed = rest.trim_start();
            let has_separator = rest_trimmed.starts_with('-')
                || rest_trimmed.starts_with('.')
                || rest_trimmed.starts_with('_')
                || rest_trimmed.starts_with(' ')
                || rest_trimmed.is_empty();

            if has_separator {
                if let Ok(n) = num_str.parse::<u32>() {
                    if n > 0 && n < 10000 {
                        return Some(n);
                    }
                }
            }
        }

        // Pattern: "Track 01" or "Track01"
        let lower = trimmed.to_lowercase();
        if lower.starts_with("track") {
            let after = trimmed[5..].trim_start();
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                if let Ok(n) = digits.parse::<u32>() {
                    if n > 0 && n < 10000 {
                        return Some(n);
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

    /// Returns true if the folder name looks like an audio encoding/quality
    /// directory (e.g., "FLAC 24-bit - 96 kHz", "MP3 320 kbps").
    fn is_encoding_folder(name: &str) -> bool {
        let lower = name.to_lowercase();
        let first_word = lower
            .split(|c: char| c.is_whitespace() || c == '-' || c == '_')
            .find(|tok| !tok.is_empty());

        if let Some(word) = first_word {
            if matches!(
                word,
                "flac" | "mp3" | "aac" | "alac" | "wav" | "aiff" | "ogg"
                    | "dsd" | "opus" | "wma" | "ape" | "pcm"
            ) {
                return true;
            }
        }

        // Standalone bitrate patterns like "320kbps"
        if lower.contains("kbps") {
            return true;
        }

        false
    }

    fn album_root_dir(file_path: &Path) -> Option<PathBuf> {
        let mut dir = file_path.parent()?.to_path_buf();

        // Skip past disc and encoding subdirectories to find the actual album root.
        // Handles: album/track, album/disc1/track, album/FLAC 24-96/track,
        //          album/FLAC 24-96/disc1/track
        for _ in 0..2 {
            let name = dir.file_name().and_then(|s| s.to_str());
            match name {
                Some(n) if Self::is_disc_folder(n) || Self::is_encoding_folder(n) => {
                    dir = dir.parent()?.to_path_buf();
                }
                _ => break,
            }
        }

        Some(dir)
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
        let sample_rate = properties.sample_rate().unwrap_or(44100) as f64;
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
                track_number: tag.track().map(|t| t as u32)
                    .or_else(|| Self::infer_track_number_from_filename(file_path)),
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
                track_number: Self::infer_track_number_from_filename(file_path),
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
            sample_rate: properties.sample_rate().unwrap_or(44100) as f64,
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

    /// Extract and save artwork as thumbnail to cache directory
    pub fn extract_artwork(file_path: &Path, _cache_dir: &Path) -> Option<String> {
        let tagged_file = Probe::open(file_path).ok()?.read().ok()?;
        let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag())?;

        let picture = tag.pictures().first()?;

        let cache_key = file_path.to_string_lossy().to_string();

        match generate_thumbnail_from_bytes(picture.data(), &cache_key) {
            Ok(thumbnail_path) => Some(thumbnail_path.to_string_lossy().to_string()),
            Err(e) => {
                log::warn!("Failed to generate thumbnail for {:?}: {}", file_path, e);
                None
            }
        }
    }

    /// Generate thumbnail from an existing artwork file
    pub fn cache_artwork_file(artwork_path: &Path, _cache_dir: &Path) -> Option<String> {
        if !artwork_path.is_file() {
            return None;
        }

        match generate_thumbnail(artwork_path) {
            Ok(thumbnail_path) => Some(thumbnail_path.to_string_lossy().to_string()),
            Err(e) => {
                log::warn!("Failed to generate thumbnail for {:?}: {}", artwork_path, e);
                None
            }
        }
    }

    /// Look for folder artwork by file name heuristics
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

    #[test]
    fn test_is_encoding_folder() {
        // QBZ-generated quality folder names
        assert!(MetadataExtractor::is_encoding_folder("FLAC 16-bit - 44.1 kHz"));
        assert!(MetadataExtractor::is_encoding_folder("FLAC 24-bit - 96 kHz"));
        assert!(MetadataExtractor::is_encoding_folder("FLAC 24-bit - 192 kHz"));
        assert!(MetadataExtractor::is_encoding_folder("MP3 320 kbps"));

        // Common encoding folder names from other tools
        assert!(MetadataExtractor::is_encoding_folder("FLAC"));
        assert!(MetadataExtractor::is_encoding_folder("flac"));
        assert!(MetadataExtractor::is_encoding_folder("MP3"));
        assert!(MetadataExtractor::is_encoding_folder("WAV"));
        assert!(MetadataExtractor::is_encoding_folder("ALAC"));
        assert!(MetadataExtractor::is_encoding_folder("DSD"));
        assert!(MetadataExtractor::is_encoding_folder("320kbps"));

        // Not encoding folders
        assert!(!MetadataExtractor::is_encoding_folder("Abbey Road"));
        assert!(!MetadataExtractor::is_encoding_folder("Disc 1"));
        assert!(!MetadataExtractor::is_encoding_folder("The Beatles"));
        assert!(!MetadataExtractor::is_encoding_folder("2024"));
    }

    #[test]
    fn test_album_root_dir_plain() {
        // artist/album/track.flac -> album/
        let path = Path::new("/music/EELS/Beautiful Freak/01 - Novocaine.flac");
        let root = MetadataExtractor::album_root_dir(path).unwrap();
        assert_eq!(root, Path::new("/music/EELS/Beautiful Freak"));
    }

    #[test]
    fn test_album_root_dir_disc_folder() {
        // artist/album/disc1/track.flac -> album/
        let path = Path::new("/music/EELS/Beautiful Freak/Disc 1/01 - Novocaine.flac");
        let root = MetadataExtractor::album_root_dir(path).unwrap();
        assert_eq!(root, Path::new("/music/EELS/Beautiful Freak"));
    }

    #[test]
    fn test_album_root_dir_encoding_folder() {
        // artist/album/quality/track.flac -> album/
        let path = Path::new("/music/EELS/Beautiful Freak/FLAC 24-bit - 96 kHz/01 - Novocaine.flac");
        let root = MetadataExtractor::album_root_dir(path).unwrap();
        assert_eq!(root, Path::new("/music/EELS/Beautiful Freak"));
    }

    #[test]
    fn test_album_root_dir_encoding_and_disc() {
        // artist/album/quality/disc1/track.flac -> album/
        let path = Path::new("/music/EELS/Beautiful Freak/FLAC 24-bit - 96 kHz/Disc 1/01 - Novocaine.flac");
        let root = MetadataExtractor::album_root_dir(path).unwrap();
        assert_eq!(root, Path::new("/music/EELS/Beautiful Freak"));
    }

    #[test]
    fn test_infer_track_number_from_filename() {
        // Common patterns: "01 - Title"
        assert_eq!(
            MetadataExtractor::infer_track_number_from_filename(Path::new("/music/01 - Novocaine.flac")),
            Some(1)
        );
        assert_eq!(
            MetadataExtractor::infer_track_number_from_filename(Path::new("/music/12 - Beautiful Freak.flac")),
            Some(12)
        );
        // "01. Title"
        assert_eq!(
            MetadataExtractor::infer_track_number_from_filename(Path::new("/music/03. Song Name.flac")),
            Some(3)
        );
        // "01_Title"
        assert_eq!(
            MetadataExtractor::infer_track_number_from_filename(Path::new("/music/05_Track.flac")),
            Some(5)
        );
        // Just number
        assert_eq!(
            MetadataExtractor::infer_track_number_from_filename(Path::new("/music/07.flac")),
            Some(7)
        );
        // "Track 01"
        assert_eq!(
            MetadataExtractor::infer_track_number_from_filename(Path::new("/music/Track 09.flac")),
            Some(9)
        );
        // "1-01 Title" (disc-track)
        assert_eq!(
            MetadataExtractor::infer_track_number_from_filename(Path::new("/music/2-05 Song.flac")),
            Some(5)
        );
        // Not a track number (title starting with non-track digits)
        assert_eq!(
            MetadataExtractor::infer_track_number_from_filename(Path::new("/music/Novocaine.flac")),
            None
        );
        // Zero is not a valid track number
        assert_eq!(
            MetadataExtractor::infer_track_number_from_filename(Path::new("/music/00 - Intro.flac")),
            None
        );
    }

    #[test]
    fn test_is_disc_folder_true() {
        // Pure disc folders
        assert!(MetadataExtractor::is_disc_folder("Disc 1"));
        assert!(MetadataExtractor::is_disc_folder("disc 2"));
        assert!(MetadataExtractor::is_disc_folder("Disc1"));
        assert!(MetadataExtractor::is_disc_folder("disc01"));
        assert!(MetadataExtractor::is_disc_folder("CD 1"));
        assert!(MetadataExtractor::is_disc_folder("CD1"));
        assert!(MetadataExtractor::is_disc_folder("cd2"));
        assert!(MetadataExtractor::is_disc_folder("Disk 3"));
        assert!(MetadataExtractor::is_disc_folder("Bonus Disc"));
        assert!(MetadataExtractor::is_disc_folder("Bonus Disc 1"));
        assert!(MetadataExtractor::is_disc_folder("Extra CD 2"));
        assert!(MetadataExtractor::is_disc_folder("Side Disc 1"));
    }

    #[test]
    fn test_is_disc_folder_false_album_names() {
        // Album names containing disc/cd keywords — NOT disc folders (issue #147)
        assert!(!MetadataExtractor::is_disc_folder("100 Popular Classics, Disc 1"));
        assert!(!MetadataExtractor::is_disc_folder("100 Popular Classics_ Best Loved Works of the Great Composers, Disc 1"));
        assert!(!MetadataExtractor::is_disc_folder("Relaxation Disc1"));
        assert!(!MetadataExtractor::is_disc_folder("Now 75 - CD1"));
        assert!(!MetadataExtractor::is_disc_folder("Match of the Day - The Album CD1"));
        assert!(!MetadataExtractor::is_disc_folder("20 Blues Greats"));
        assert!(!MetadataExtractor::is_disc_folder("The Beatles"));
        assert!(!MetadataExtractor::is_disc_folder("Abbey Road"));
    }

    #[test]
    fn test_album_root_dir_album_with_disc_in_name() {
        // Issue #147: album names containing "Disc N" should NOT be treated as disc folders
        let path = Path::new("/music/Various Artists/100 Popular Classics, Disc 1/01 - Track.flac");
        let root = MetadataExtractor::album_root_dir(path).unwrap();
        assert_eq!(root, Path::new("/music/Various Artists/100 Popular Classics, Disc 1"));

        let path = Path::new("/music/Various Artists/Relaxation Disc1/01 - Track.flac");
        let root = MetadataExtractor::album_root_dir(path).unwrap();
        assert_eq!(root, Path::new("/music/Various Artists/Relaxation Disc1"));

        let path = Path::new("/music/Various Artists/Now 75 - CD1/01 - Track.flac");
        let root = MetadataExtractor::album_root_dir(path).unwrap();
        assert_eq!(root, Path::new("/music/Various Artists/Now 75 - CD1"));
    }
}
