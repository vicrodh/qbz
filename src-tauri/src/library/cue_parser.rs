//! CUE sheet parser for single-file albums

use std::fs;
use std::path::Path;

use crate::library::{AudioFormat, AudioProperties, LibraryError, LocalTrack};

/// Parsed CUE sheet
#[derive(Debug, Clone)]
pub struct CueSheet {
    /// Path to the .cue file
    pub file_path: String,
    /// Referenced audio file (resolved to absolute path)
    pub audio_file: String,
    /// Album title
    pub title: Option<String>,
    /// Album performer/artist
    pub performer: Option<String>,
    /// Tracks in the CUE sheet
    pub tracks: Vec<CueTrack>,
}

/// A track within a CUE sheet
#[derive(Debug, Clone)]
pub struct CueTrack {
    /// Track number
    pub number: u32,
    /// Track title
    pub title: String,
    /// Track performer (if different from album)
    pub performer: Option<String>,
    /// Start time in seconds
    pub start_secs: f64,
}

/// CUE time format (MM:SS:FF where FF is frames, 75 frames per second)
#[derive(Debug, Clone, Copy)]
pub struct CueTime {
    pub minutes: u32,
    pub seconds: u32,
    pub frames: u32,
}

impl CueTime {
    /// Convert to seconds (frames are 1/75 second)
    pub fn to_seconds(&self) -> f64 {
        self.minutes as f64 * 60.0 + self.seconds as f64 + self.frames as f64 / 75.0
    }

    /// Parse "MM:SS:FF" format
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 3 {
            return None;
        }
        Some(CueTime {
            minutes: parts[0].parse().ok()?,
            seconds: parts[1].parse().ok()?,
            frames: parts[2].parse().ok()?,
        })
    }
}

/// CUE sheet parser
pub struct CueParser;

impl CueParser {
    /// Parse a CUE file
    pub fn parse(cue_path: &Path) -> Result<CueSheet, LibraryError> {
        log::debug!("Parsing CUE file: {}", cue_path.display());

        // Try UTF-8 first, then fall back to Latin-1
        let content = fs::read_to_string(cue_path).or_else(|_| {
            let bytes = fs::read(cue_path)?;
            Ok::<String, std::io::Error>(bytes.iter().map(|&b| b as char).collect())
        })?;

        Self::parse_content(&content, cue_path)
    }

    /// Parse CUE content
    fn parse_content(content: &str, cue_path: &Path) -> Result<CueSheet, LibraryError> {
        let mut sheet = CueSheet {
            file_path: cue_path.to_string_lossy().to_string(),
            audio_file: String::new(),
            title: None,
            performer: None,
            tracks: Vec::new(),
        };

        let mut current_track: Option<CueTrack> = None;
        let mut in_track = false;

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with("REM") {
                continue;
            }

            // Parse FILE "name" TYPE
            if line.to_uppercase().starts_with("FILE ") {
                if let Some(filename) = Self::extract_quoted(line) {
                    // Resolve path relative to CUE file
                    if let Some(parent) = cue_path.parent() {
                        let audio_path = parent.join(&filename);
                        sheet.audio_file = audio_path.to_string_lossy().to_string();
                    } else {
                        sheet.audio_file = filename;
                    }
                }
            }
            // Parse album-level TITLE (before any TRACK)
            else if line.to_uppercase().starts_with("TITLE ") && !in_track {
                sheet.title = Self::extract_quoted(line);
            }
            // Parse album-level PERFORMER (before any TRACK)
            else if line.to_uppercase().starts_with("PERFORMER ") && !in_track {
                sheet.performer = Self::extract_quoted(line);
            }
            // Parse TRACK NN AUDIO
            else if line.to_uppercase().starts_with("TRACK ") {
                // Save previous track
                if let Some(track) = current_track.take() {
                    sheet.tracks.push(track);
                }

                // Start new track
                in_track = true;
                if let Some(num) = Self::extract_track_number(line) {
                    current_track = Some(CueTrack {
                        number: num,
                        title: format!("Track {}", num),
                        performer: None,
                        start_secs: 0.0,
                    });
                }
            }
            // Parse track TITLE
            else if line.to_uppercase().starts_with("TITLE ") && in_track {
                if let Some(ref mut track) = current_track {
                    if let Some(title) = Self::extract_quoted(line) {
                        track.title = title;
                    }
                }
            }
            // Parse track PERFORMER
            else if line.to_uppercase().starts_with("PERFORMER ") && in_track {
                if let Some(ref mut track) = current_track {
                    track.performer = Self::extract_quoted(line);
                }
            }
            // Parse INDEX 01 MM:SS:FF (track start time)
            else if line.to_uppercase().starts_with("INDEX 01 ") {
                if let Some(ref mut track) = current_track {
                    let time_str = line.get(9..).map(|s| s.trim()).unwrap_or("");
                    if let Some(time) = CueTime::parse(time_str) {
                        track.start_secs = time.to_seconds();
                    }
                }
            }
        }

        // Don't forget the last track
        if let Some(track) = current_track {
            sheet.tracks.push(track);
        }

        // Validate we have an audio file and at least one track
        if sheet.audio_file.is_empty() {
            return Err(LibraryError::CueParse(
                "No FILE directive found in CUE sheet".to_string(),
            ));
        }

        if sheet.tracks.is_empty() {
            return Err(LibraryError::CueParse(
                "No tracks found in CUE sheet".to_string(),
            ));
        }

        log::info!(
            "Parsed CUE: {} tracks, audio file: {}",
            sheet.tracks.len(),
            sheet.audio_file
        );

        Ok(sheet)
    }

    /// Extract quoted string: COMMAND "value" -> value
    fn extract_quoted(line: &str) -> Option<String> {
        let start = line.find('"')?;
        let end = line.rfind('"')?;
        if end <= start {
            return None;
        }
        Some(line[start + 1..end].to_string())
    }

    /// Extract track number: "TRACK 01 AUDIO" -> 1
    fn extract_track_number(line: &str) -> Option<u32> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            return None;
        }
        parts[1].parse().ok()
    }
}

/// Convert a CUE sheet into LocalTrack entries
pub fn cue_to_tracks(
    cue: &CueSheet,
    audio_duration_secs: u64,
    format: AudioFormat,
    properties: &AudioProperties,
) -> Vec<LocalTrack> {
    let mut tracks = Vec::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    for (i, cue_track) in cue.tracks.iter().enumerate() {
        // Calculate end time (next track's start or audio end)
        let end_secs = if i + 1 < cue.tracks.len() {
            cue.tracks[i + 1].start_secs
        } else {
            audio_duration_secs as f64
        };

        let duration = (end_secs - cue_track.start_secs).max(0.0) as u64;

        tracks.push(LocalTrack {
            id: 0,
            file_path: cue.audio_file.clone(),
            title: cue_track.title.clone(),
            artist: cue_track
                .performer
                .clone()
                .or_else(|| cue.performer.clone())
                .unwrap_or_else(|| "Unknown Artist".to_string()),
            album: cue
                .title
                .clone()
                .unwrap_or_else(|| "Unknown Album".to_string()),
            album_artist: cue.performer.clone(),
            track_number: Some(cue_track.number),
            disc_number: None,
            year: None,
            genre: None,
            duration_secs: duration,
            format: format.clone(),
            bit_depth: properties.bit_depth,
            sample_rate: properties.sample_rate,
            channels: properties.channels,
            file_size_bytes: 0,
            cue_file_path: Some(cue.file_path.clone()),
            cue_start_secs: Some(cue_track.start_secs),
            cue_end_secs: Some(end_secs),
            artwork_path: None,
            last_modified: 0,
            indexed_at: now,
        });
    }

    tracks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cue_time_parse() {
        let time = CueTime::parse("03:45:22").unwrap();
        assert_eq!(time.minutes, 3);
        assert_eq!(time.seconds, 45);
        assert_eq!(time.frames, 22);

        let secs = time.to_seconds();
        assert!((secs - 225.293).abs() < 0.01);
    }

    #[test]
    fn test_extract_quoted() {
        assert_eq!(
            CueParser::extract_quoted("TITLE \"My Song\""),
            Some("My Song".to_string())
        );
        assert_eq!(
            CueParser::extract_quoted("FILE \"album.flac\" WAVE"),
            Some("album.flac".to_string())
        );
    }

    #[test]
    fn test_extract_track_number() {
        assert_eq!(CueParser::extract_track_number("TRACK 01 AUDIO"), Some(1));
        assert_eq!(CueParser::extract_track_number("TRACK 12 AUDIO"), Some(12));
    }
}
