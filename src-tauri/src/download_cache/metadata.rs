//! Metadata fetching and FLAC tagging for downloaded tracks
//!
//! Handles complete metadata retrieval from Qobuz API and writing to FLAC tags.

use serde::{Deserialize, Serialize};
use lofty::{
    Accessor, AudioFile, ItemKey, Picture, PictureType, Tag, TagExt, TaggedFileExt,
};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteTrackMetadata {
    pub track_id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub isrc: Option<String>,
    pub label: Option<String>,
    pub copyright: Option<String>,
    pub composer: Option<String>,
    pub duration_secs: u64,
    pub artwork_url: Option<String>,
}

/// Fetch complete metadata for a track from Qobuz API
pub async fn fetch_complete_metadata(
    track_id: u64,
    qobuz_client: &crate::api::QobuzClient,
) -> Result<CompleteTrackMetadata, String> {
    log::info!("Fetching complete metadata for track {}", track_id);

    let track = qobuz_client
        .get_track(track_id)
        .await
        .map_err(|e| format!("Failed to fetch track: {}", e))?;

    let album = if let Some(album_obj) = &track.album {
        qobuz_client
            .get_album(&album_obj.id)
            .await
            .ok()
    } else {
        None
    };

    let album_artist = album
        .as_ref()
        .map(|a| a.artist.name.clone())
        .or_else(|| track.performer.as_ref().map(|p| p.name.clone()));

    let genre = album
        .as_ref()
        .and_then(|a| a.genre.as_ref())
        .map(|g| g.name.clone());

    let label = album
        .as_ref()
        .and_then(|a| a.label.as_ref())
        .map(|l| l.name.clone());

    let year = album
        .as_ref()
        .and_then(|a| a.release_date_original.as_ref())
        .and_then(|date_str| {
            // Parse YYYY-MM-DD or YYYY format
            date_str.split('-').next()
                .and_then(|year_str| year_str.parse::<u32>().ok())
        });

    let artwork_url = album
        .as_ref()
        .and_then(|a| a.image.large.clone())
        .or_else(|| {
            track.album.as_ref()
                .and_then(|a| a.image.large.clone())
        });

    Ok(CompleteTrackMetadata {
        track_id,
        title: track.title,
        artist: track.performer.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
        album: track.album.as_ref().map(|a| a.title.clone()).unwrap_or_default(),
        album_artist,
        track_number: Some(track.track_number),
        disc_number: track.media_number,
        year,
        genre,
        isrc: track.isrc.clone(),
        label,
        copyright: None, // Qobuz API doesn't provide copyright in album model
        composer: None,  // Qobuz API doesn't provide composer in track model
        duration_secs: track.duration as u64,
        artwork_url,
    })
}

/// Write metadata tags to a FLAC file
pub fn write_flac_tags(
    file_path: &str,
    metadata: &CompleteTrackMetadata,
) -> Result<(), String> {
    log::info!("Writing FLAC tags to: {}", file_path);

    let path = Path::new(file_path);
    let mut tagged_file = lofty::read_from_path(path)
        .map_err(|e| format!("Failed to read FLAC file: {}", e))?;

    let tag = match tagged_file.primary_tag_mut() {
        Some(primary_tag) => primary_tag,
        None => {
            let tag_type = tagged_file.primary_tag_type();
            tagged_file.insert_tag(Tag::new(tag_type));
            tagged_file.primary_tag_mut().unwrap()
        }
    };

    // Clear existing tags
    tag.clear();

    // Write standard Vorbis comments
    tag.set_title(metadata.title.clone());
    tag.set_artist(metadata.artist.clone());
    tag.set_album(metadata.album.clone());

    if let Some(album_artist) = &metadata.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, album_artist.clone());
    }

    if let Some(track_number) = metadata.track_number {
        tag.set_track(track_number);
    }

    if let Some(disc_number) = metadata.disc_number {
        tag.set_disk(disc_number);
    }

    if let Some(year) = metadata.year {
        tag.set_year(year);
    }

    if let Some(genre) = &metadata.genre {
        tag.set_genre(genre.clone());
    }

    if let Some(isrc) = &metadata.isrc {
        tag.insert_text(ItemKey::Unknown("ISRC".to_string()), isrc.clone());
    }

    if let Some(label) = &metadata.label {
        tag.insert_text(ItemKey::Label, label.clone());
    }

    if let Some(copyright) = &metadata.copyright {
        tag.insert_text(ItemKey::CopyrightMessage, copyright.clone());
    }

    if let Some(composer) = &metadata.composer {
        tag.insert_text(ItemKey::Composer, composer.clone());
    }

    // Save tags
    tagged_file
        .save_to_path(path)
        .map_err(|e| format!("Failed to save tags: {}", e))?;

    Ok(())
}

/// Download and embed artwork in FLAC file
pub async fn embed_artwork(
    file_path: &str,
    artwork_url: &str,
) -> Result<(), String> {
    log::info!("Embedding artwork from: {}", artwork_url);

    // Download artwork
    let response = reqwest::get(artwork_url)
        .await
        .map_err(|e| format!("Failed to download artwork: {}", e))?;

    let artwork_bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read artwork bytes: {}", e))?;

    // Determine MIME type from URL
    let mime_type = if artwork_url.contains(".jpg") || artwork_url.contains(".jpeg") {
        lofty::MimeType::Jpeg
    } else if artwork_url.contains(".png") {
        lofty::MimeType::Png
    } else {
        lofty::MimeType::Jpeg // Default to JPEG
    };

    // Create picture
    let picture = Picture::new_unchecked(
        PictureType::CoverFront,
        Some(mime_type),
        None,
        artwork_bytes.to_vec(),
    );

    // Read file
    let path = Path::new(file_path);
    let mut tagged_file = lofty::read_from_path(path)
        .map_err(|e| format!("Failed to read FLAC file: {}", e))?;

    // Add picture to primary tag
    if let Some(tag) = tagged_file.primary_tag_mut() {
        tag.push_picture(picture);
    } else {
        let tag_type = tagged_file.primary_tag_type();
        let mut tag = Tag::new(tag_type);
        tag.push_picture(picture);
        tagged_file.insert_tag(tag);
    }

    // Save
    tagged_file
        .save_to_path(path)
        .map_err(|e| format!("Failed to save artwork: {}", e))?;

    Ok(())
}

/// Sanitize filename to be ASCII-safe and filesystem-compatible
pub fn sanitize_filename(name: &str) -> String {
    // Remove or replace invalid characters
    let invalid_chars = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    
    let mut sanitized = name
        .chars()
        .map(|c| {
            if invalid_chars.contains(&c) {
                '-'
            } else if c.is_ascii() || c.is_alphanumeric() {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();

    // Replace multiple consecutive dashes with single dash
    while sanitized.contains("--") {
        sanitized = sanitized.replace("--", "-");
    }

    // Trim dashes and whitespace from ends
    sanitized = sanitized.trim_matches('-').trim().to_string();

    // Limit length to 200 chars (leaving room for extension and path)
    if sanitized.len() > 200 {
        sanitized.truncate(200);
        sanitized = sanitized.trim_matches('-').trim().to_string();
    }

    // If empty after sanitization, use fallback
    if sanitized.is_empty() {
        sanitized = "track".to_string();
    }

    sanitized
}

/// Organize downloaded file into proper folder structure
pub fn organize_download(
    track_id: u64,
    temp_path: &str,
    root_dir: &str,
    metadata: &CompleteTrackMetadata,
) -> Result<String, String> {
    log::info!("Organizing download for track {}", track_id);

    let temp = Path::new(temp_path);
    let root = Path::new(root_dir);

    // Build target path: <root>/<artist>/<album>/[Disc N/]NN - Title.flac
    let artist_dir = sanitize_filename(&metadata.album_artist.as_ref().unwrap_or(&metadata.artist));
    let album_dir = sanitize_filename(&metadata.album);

    let mut target_dir = root.join(&artist_dir).join(&album_dir);

    // Add disc subfolder if multi-disc
    if let Some(disc) = metadata.disc_number {
        if disc > 1 {
            target_dir = target_dir.join(format!("Disc {}", disc));
        }
    }

    // Create directory structure
    std::fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Failed to create directories: {}", e))?;

    // Build filename: NN - Title.flac
    let track_num = metadata.track_number.unwrap_or(0);
    let title_clean = sanitize_filename(&metadata.title);
    let filename = if track_num > 0 {
        format!("{:02} - {}.flac", track_num, title_clean)
    } else {
        format!("{}.flac", title_clean)
    };

    let target_path = target_dir.join(&filename);

    // Handle filename conflicts
    let final_path = if target_path.exists() {
        let mut counter = 2;
        loop {
            let alt_filename = if track_num > 0 {
                format!("{:02} - {} ({}).flac", track_num, title_clean, counter)
            } else {
                format!("{} ({}).flac", title_clean, counter)
            };
            let alt_path = target_dir.join(&alt_filename);
            if !alt_path.exists() {
                break alt_path;
            }
            counter += 1;
            if counter > 100 {
                return Err("Too many filename conflicts".to_string());
            }
        }
    } else {
        target_path
    };

    // Move file
    std::fs::rename(temp, &final_path)
        .map_err(|e| format!("Failed to move file: {}", e))?;

    Ok(final_path.to_string_lossy().to_string())
}
