//! Direct embedded-tag writer (frontend-agnostic port of the Tauri
//! `v2_library_write_album_metadata_to_files` lofty loop). The Slint and Tauri
//! frontends both call this so the lofty logic lives in one place. Progress is
//! reported through an `on_progress` closure (no Tauri event bus); the caller
//! orchestrates the DB update + sidecar removal.

use std::path::Path;

use crate::{LibraryError, LocalTrack};

/// Album-level fields written into every file's embedded tags. A `None`
/// (or blank) field REMOVES that tag (direct write is destructive, unlike the
/// override-only sidecar).
pub struct AlbumTagWrite {
    pub album_title: String,
    pub album_artist: String, // "" => remove the AlbumArtist tag
    pub year: Option<u32>,    // None => remove the date
    pub genre: Option<String>,
    pub catalog_number: Option<String>,
}

/// One file's per-track fields.
pub struct TrackTagWrite {
    pub file_path: String,
    pub title: String,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
}

/// Write embedded tags to each file. Dedups by `file_path` keeping the FIRST
/// occurrence (order preserved). `on_progress(current, total)` is called
/// BEFORE each file write (1-based; total = deduped count). Partial-failure
/// unsafe by design: returns `Err` on the first failing file with prior files
/// already modified. Does NOT touch the DB or the sidecar.
pub fn write_album_tags_to_files(
    album: &AlbumTagWrite,
    tracks: &[TrackTagWrite],
    mut on_progress: impl FnMut(usize, usize),
) -> Result<(), LibraryError> {
    use lofty::config::WriteOptions;
    use lofty::prelude::*;
    use lofty::tag::{ItemKey, Tag};

    // Dedup by file_path, first wins, original order preserved.
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<&TrackTagWrite> = tracks
        .iter()
        .filter(|t| seen.insert(t.file_path.clone()))
        .collect();
    let total = unique.len();

    for (i, track) in unique.iter().enumerate() {
        on_progress(i + 1, total);

        let path = Path::new(&track.file_path);
        if !path.is_file() {
            return Err(LibraryError::Metadata(
                "One or more audio files were not found on disk.".to_string(),
            ));
        }

        let mut tagged_file = lofty::read_from_path(path)
            .map_err(|_| LibraryError::Metadata("Failed to read audio file tags.".to_string()))?;

        let primary_type = tagged_file.primary_tag_type();
        if tagged_file.primary_tag_mut().is_none() && tagged_file.first_tag_mut().is_none() {
            tagged_file.insert_tag(Tag::new(primary_type));
        }

        {
            let tag = if let Some(tag) = tagged_file.primary_tag_mut() {
                tag
            } else if let Some(tag) = tagged_file.first_tag_mut() {
                tag
            } else {
                return Err(LibraryError::Metadata(
                    "Failed to access audio file tags.".to_string(),
                ));
            };

            tag.set_title(track.title.trim().to_string());
            tag.set_album(album.album_title.trim().to_string());
            tag.set_artist(album.album_artist.trim().to_string());

            if let Some(no) = track.track_number {
                tag.set_track(no);
            }
            if let Some(disc) = track.disc_number {
                tag.set_disk(disc);
            }

            // Album artist (not part of the Accessor trait).
            if album.album_artist.trim().is_empty() {
                tag.remove_key(ItemKey::AlbumArtist);
            } else {
                tag.insert_text(ItemKey::AlbumArtist, album.album_artist.trim().to_string());
            }

            // Year.
            if let Some(year) = album.year {
                tag.set_date(lofty::tag::items::Timestamp {
                    year: year as u16,
                    ..Default::default()
                });
            } else {
                tag.remove_date();
            }

            // Genre.
            if let Some(g) = album
                .genre
                .as_ref()
                .map(|g| g.trim())
                .filter(|g| !g.is_empty())
            {
                tag.set_genre(g.to_string());
            } else {
                tag.remove_genre();
            }

            // Catalog number.
            if let Some(c) = album
                .catalog_number
                .as_ref()
                .map(|c| c.trim())
                .filter(|c| !c.is_empty())
            {
                tag.insert_text(ItemKey::CatalogNumber, c.to_string());
            } else {
                tag.remove_key(ItemKey::CatalogNumber);
            }
        }

        tagged_file
            .save_to_path(path, WriteOptions::default())
            .map_err(|_| {
                LibraryError::Metadata(
                    "Failed to write tags to audio files. Check that the album folder is mounted \
                     read-write and you have permissions."
                        .to_string(),
                )
            })?;
    }

    Ok(())
}

/// Returns `Some(v)` iff every non-blank track shares one
/// `album_artist ?? artist`, else `None`. Empty / all-blank => `None`.
/// Port of the Tauri `library_compute_track_artist_match`.
pub fn compute_track_artist_match(tracks: &[LocalTrack]) -> Option<String> {
    let mut artists: std::collections::HashSet<String> = std::collections::HashSet::new();
    for track in tracks {
        let value = track
            .album_artist
            .as_deref()
            .unwrap_or(track.artist.as_str())
            .trim();
        if value.is_empty() {
            continue;
        }
        artists.insert(value.to_string());
        if artists.len() > 1 {
            return None;
        }
    }
    artists.into_iter().next()
}
