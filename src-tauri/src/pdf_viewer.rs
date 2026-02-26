//! PDF Viewer module using MuPDF for high-quality rendering.
//!
//! Used by the booklet viewer to render Qobuz album booklet PDFs.
//! MuPDF renders pages server-side to PNG images at configurable DPI,
//! producing much sharper results than browser-based pdfjs-dist canvas rendering.

use base64::Engine;
use mupdf::{Colorspace, Document, Matrix};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Mutex;

/// Page size in points (1/72 inch)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

/// Info returned when opening a booklet
#[derive(Debug, Serialize, Deserialize)]
pub struct BookletInfo {
    pub num_pages: u32,
    pub page_sizes: Vec<PageSize>,
}

/// Rendered page result
#[derive(Debug, Serialize, Deserialize)]
pub struct RenderedPage {
    /// Base64-encoded PNG image data
    pub data: String,
    /// Width of the rendered image in pixels
    pub width: u32,
    /// Height of the rendered image in pixels
    pub height: u32,
}

/// State for the currently open booklet PDF
pub struct BookletState {
    /// Path to the temp file holding the downloaded PDF
    current_path: Mutex<Option<PathBuf>>,
}

impl BookletState {
    pub fn new() -> Self {
        Self {
            current_path: Mutex::new(None),
        }
    }
}

fn booklet_temp_dir() -> PathBuf {
    std::env::temp_dir().join("qbz-booklets")
}

/// Download a PDF from a URL, save to temp, and return page info.
#[tauri::command]
pub async fn v2_booklet_open(
    url: String,
    state: tauri::State<'_, BookletState>,
) -> Result<BookletInfo, String> {
    // Clean up any previous booklet
    {
        let mut path_lock = state.current_path.lock().map_err(|e| e.to_string())?;
        if let Some(old_path) = path_lock.take() {
            let _ = std::fs::remove_file(&old_path);
        }
    }

    // Download PDF bytes
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch PDF: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}: {}", response.status(), url));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // Save to temp file
    let temp_dir = booklet_temp_dir();
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Failed to create temp dir: {}", e))?;

    let file_name = format!("{}.pdf", uuid::Uuid::new_v4());
    let temp_path = temp_dir.join(&file_name);

    std::fs::write(&temp_path, &bytes)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    // Open with MuPDF and get page info (blocking operation)
    let path_str = temp_path.to_string_lossy().to_string();
    let info = tokio::task::spawn_blocking(move || -> Result<BookletInfo, String> {
        let document = Document::open(&path_str)
            .map_err(|e| format!("Failed to open PDF: {:?}", e))?;

        let num_pages = document
            .page_count()
            .map_err(|e| format!("Failed to get page count: {:?}", e))? as u32;

        let mut page_sizes = Vec::with_capacity(num_pages as usize);
        for i in 0..num_pages {
            match document.load_page(i as i32) {
                Ok(page) => {
                    let bounds = page
                        .bounds()
                        .map_err(|e| format!("Failed to get page bounds: {:?}", e))?;
                    page_sizes.push(PageSize {
                        width: bounds.width(),
                        height: bounds.height(),
                    });
                }
                Err(e) => {
                    log::warn!("Failed to load page {}: {:?}", i, e);
                    page_sizes.push(PageSize {
                        width: 612.0,
                        height: 792.0,
                    });
                }
            }
        }

        Ok(BookletInfo {
            num_pages,
            page_sizes,
        })
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))??;

    // Store the temp path
    {
        let mut path_lock = state.current_path.lock().map_err(|e| e.to_string())?;
        *path_lock = Some(temp_path);
    }

    Ok(info)
}

/// Render a single page at the specified DPI.
#[tauri::command]
pub async fn v2_booklet_render_page(
    page: u32,
    dpi: u32,
    rotation: Option<u32>,
    state: tauri::State<'_, BookletState>,
) -> Result<RenderedPage, String> {
    let path = {
        let path_lock = state.current_path.lock().map_err(|e| e.to_string())?;
        path_lock
            .as_ref()
            .ok_or_else(|| "No booklet is open".to_string())?
            .to_string_lossy()
            .to_string()
    };

    let rotation = rotation.unwrap_or(0);
    let dpi = dpi.max(36).min(600); // Clamp DPI to sane range

    tokio::task::spawn_blocking(move || -> Result<RenderedPage, String> {
        let document = Document::open(&path)
            .map_err(|e| format!("Failed to open PDF: {:?}", e))?;

        let page_index = (page - 1) as i32;
        let pdf_page = document
            .load_page(page_index)
            .map_err(|e| format!("Failed to load page {}: {:?}", page, e))?;

        // Calculate scale factor (PDF base is 72 DPI)
        let scale = dpi as f32 / 72.0;

        // Build transformation matrix: scale, then rotate if needed
        let matrix = if rotation == 0 {
            Matrix::new_scale(scale, scale)
        } else {
            // Get page bounds to translate rotation center
            let bounds = pdf_page
                .bounds()
                .map_err(|e| format!("Failed to get bounds: {:?}", e))?;
            let w = bounds.width() * scale;
            let h = bounds.height() * scale;

            // Compose: scale -> rotate -> translate (to keep in positive coords)
            let mut m = Matrix::new_scale(scale, scale);
            m.concat(Matrix::new_rotate(rotation as f32));
            match rotation % 360 {
                90 => { m.concat(Matrix::new_translate(h, 0.0)); }
                180 => { m.concat(Matrix::new_translate(w, h)); }
                270 => { m.concat(Matrix::new_translate(0.0, w)); }
                _ => {}
            };
            m
        };

        // Render page to pixmap (RGB, no alpha — ensures white background)
        let pixmap = pdf_page
            .to_pixmap(&matrix, &Colorspace::device_rgb(), false, true)
            .map_err(|e| format!("Failed to render page: {:?}", e))?;

        let actual_width = pixmap.width() as u32;
        let actual_height = pixmap.height() as u32;

        // Encode to PNG
        let mut png_data = Vec::new();
        let mut cursor = Cursor::new(&mut png_data);
        pixmap
            .write_to(&mut cursor, mupdf::ImageFormat::PNG)
            .map_err(|e| format!("Failed to encode PNG: {:?}", e))?;

        // Base64 encode
        let base64_data = base64::engine::general_purpose::STANDARD.encode(&png_data);

        Ok(RenderedPage {
            data: base64_data,
            width: actual_width,
            height: actual_height,
        })
    })
    .await
    .map_err(|e| format!("Render task failed: {}", e))?
}

/// Copy the current booklet PDF to a user-chosen destination.
#[tauri::command]
pub async fn v2_booklet_save(
    dest: String,
    state: tauri::State<'_, BookletState>,
) -> Result<(), String> {
    let src = {
        let path_lock = state.current_path.lock().map_err(|e| e.to_string())?;
        path_lock
            .as_ref()
            .ok_or_else(|| "No booklet is open".to_string())?
            .clone()
    };

    std::fs::copy(&src, &dest)
        .map_err(|e| format!("Failed to save booklet: {}", e))?;

    Ok(())
}

/// Clean up the current booklet temp file.
#[tauri::command]
pub async fn v2_booklet_close(
    state: tauri::State<'_, BookletState>,
) -> Result<(), String> {
    let mut path_lock = state.current_path.lock().map_err(|e| e.to_string())?;
    if let Some(old_path) = path_lock.take() {
        let _ = std::fs::remove_file(&old_path);
    }
    Ok(())
}
