//! Thumbnail generation for album artwork

use image::imageops::FilterType;
use image::ImageReader;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::LibraryError;

/// Default thumbnail size (width and height)
/// 500px is a good balance for UI display while keeping file size reasonable
const THUMBNAIL_SIZE: u32 = 500;

/// Get the thumbnails directory path
pub fn get_thumbnails_dir() -> Result<PathBuf, LibraryError> {
    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| LibraryError::Other("Could not find data directory".into()))?;
    let thumbnails_dir = data_dir.join("qbz").join("thumbnails");

    // Create directory if it doesn't exist
    if !thumbnails_dir.exists() {
        fs::create_dir_all(&thumbnails_dir)
            .map_err(|e| LibraryError::Other(format!("Failed to create thumbnails directory: {}", e)))?;
    }

    Ok(thumbnails_dir)
}

/// Generate a unique filename for a thumbnail based on the source path
fn get_thumbnail_filename(source_path: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    source_path.to_string_lossy().hash(&mut hasher);
    format!("{:016x}.jpg", hasher.finish())
}

/// Get the thumbnail path for a source image
pub fn get_thumbnail_path(source_path: &Path) -> Result<PathBuf, LibraryError> {
    let thumbnails_dir = get_thumbnails_dir()?;
    let filename = get_thumbnail_filename(source_path);
    Ok(thumbnails_dir.join(filename))
}

/// Check if a thumbnail exists for the given source path
pub fn thumbnail_exists(source_path: &Path) -> Result<bool, LibraryError> {
    let thumbnail_path = get_thumbnail_path(source_path)?;
    Ok(thumbnail_path.exists())
}

/// Generate a thumbnail for the given source image
pub fn generate_thumbnail(source_path: &Path) -> Result<PathBuf, LibraryError> {
    let thumbnail_path = get_thumbnail_path(source_path)?;

    // If thumbnail already exists, return it
    if thumbnail_path.exists() {
        return Ok(thumbnail_path);
    }

    // Read source image
    let img = ImageReader::open(source_path)
        .map_err(|e| LibraryError::Other(format!("Failed to open image: {}", e)))?
        .decode()
        .map_err(|e| LibraryError::Other(format!("Failed to decode image: {}", e)))?;

    // Resize to thumbnail size (maintaining aspect ratio, fit within square)
    let thumbnail = img.resize(THUMBNAIL_SIZE, THUMBNAIL_SIZE, FilterType::Lanczos3);

    // Save as JPEG with quality 85
    thumbnail
        .save(&thumbnail_path)
        .map_err(|e| LibraryError::Other(format!("Failed to save thumbnail: {}", e)))?;

    Ok(thumbnail_path)
}

/// Generate a thumbnail from image bytes (for embedded artwork)
pub fn generate_thumbnail_from_bytes(
    bytes: &[u8],
    cache_key: &str,
) -> Result<PathBuf, LibraryError> {
    let thumbnails_dir = get_thumbnails_dir()?;

    // Generate filename from cache key
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    cache_key.hash(&mut hasher);
    let filename = format!("{:016x}.jpg", hasher.finish());
    let thumbnail_path = thumbnails_dir.join(&filename);

    // If thumbnail already exists, return it
    if thumbnail_path.exists() {
        return Ok(thumbnail_path);
    }

    // Decode image from bytes
    let cursor = Cursor::new(bytes);
    let img = ImageReader::new(cursor)
        .with_guessed_format()
        .map_err(|e| LibraryError::Other(format!("Failed to guess image format: {}", e)))?
        .decode()
        .map_err(|e| LibraryError::Other(format!("Failed to decode image: {}", e)))?;

    // Resize to thumbnail size
    let thumbnail = img.resize(THUMBNAIL_SIZE, THUMBNAIL_SIZE, FilterType::Lanczos3);

    // Save as JPEG
    thumbnail
        .save(&thumbnail_path)
        .map_err(|e| LibraryError::Other(format!("Failed to save thumbnail: {}", e)))?;

    Ok(thumbnail_path)
}

/// Get or generate a thumbnail for an artwork path
/// Returns the path to the thumbnail file
pub fn get_or_generate_thumbnail(artwork_path: &Path) -> Result<PathBuf, LibraryError> {
    let thumbnail_path = get_thumbnail_path(artwork_path)?;

    if thumbnail_path.exists() {
        return Ok(thumbnail_path);
    }

    generate_thumbnail(artwork_path)
}

/// Clear all thumbnails (useful for cache cleanup)
pub fn clear_thumbnails() -> Result<(), LibraryError> {
    let thumbnails_dir = get_thumbnails_dir()?;

    if thumbnails_dir.exists() {
        fs::remove_dir_all(&thumbnails_dir)
            .map_err(|e| LibraryError::Other(format!("Failed to clear thumbnails: {}", e)))?;
        fs::create_dir_all(&thumbnails_dir)
            .map_err(|e| LibraryError::Other(format!("Failed to recreate thumbnails directory: {}", e)))?;
    }

    Ok(())
}

/// Get the total size of the thumbnails cache in bytes
pub fn get_cache_size() -> Result<u64, LibraryError> {
    let thumbnails_dir = get_thumbnails_dir()?;

    if !thumbnails_dir.exists() {
        return Ok(0);
    }

    let mut total_size = 0u64;

    for entry in fs::read_dir(&thumbnails_dir)
        .map_err(|e| LibraryError::Other(format!("Failed to read thumbnails directory: {}", e)))?
    {
        if let Ok(entry) = entry {
            if let Ok(metadata) = entry.metadata() {
                total_size += metadata.len();
            }
        }
    }

    Ok(total_size)
}
