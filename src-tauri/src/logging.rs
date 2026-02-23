//! Log capture system
//!
//! Provides a global ring buffer that captures backend logs for the "View Logs" developer feature.
//! Uses a TeeWriter to send env_logger output to both stderr AND the ring buffer.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{LazyLock, Mutex};

/// Maximum number of log lines to keep in the ring buffer
const MAX_LOG_LINES: usize = 5000;

/// Global ring buffer for captured log lines
static LOG_BUFFER: LazyLock<Mutex<VecDeque<String>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES)));

/// Push a log line to the ring buffer (used by startup messages before env_logger is initialized)
pub fn push_log(line: String) {
    if let Ok(mut buf) = LOG_BUFFER.lock() {
        if buf.len() >= MAX_LOG_LINES {
            buf.pop_front();
        }
        buf.push_back(line);
    }
}

/// Get all captured log lines
pub fn get_logs() -> Vec<String> {
    LOG_BUFFER
        .lock()
        .map(|buf| buf.iter().cloned().collect())
        .unwrap_or_default()
}

/// A writer that tees output to both stderr and the log ring buffer.
/// Used as the env_logger target so all log::info!/warn!/error! output is captured.
pub struct TeeWriter;

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Always write to stderr first
        std::io::stderr().write_all(buf)?;

        // Also capture to ring buffer (best-effort, line-based)
        if let Ok(s) = std::str::from_utf8(buf) {
            for line in s.lines() {
                if !line.is_empty() {
                    push_log(line.to_string());
                }
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }
}

/// Helper for startup messages (before env_logger): prints to stderr AND captures to buffer
pub fn log_startup(msg: &str) {
    eprintln!("{}", msg);
    push_log(msg.to_string());
}

// Tauri commands

#[tauri::command]
pub fn get_backend_logs() -> Vec<String> {
    get_logs()
}

#[tauri::command]
pub async fn upload_logs_to_paste(content: String) -> Result<String, String> {
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::text(content)
            .file_name("qbz-logs.txt")
            .mime_str("text/plain")
            .map_err(|e| format!("Failed to create multipart part: {}", e))?,
    );

    let client = reqwest::Client::builder()
        .user_agent("QBZ/1.0")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    let response = client
        .post("https://0x0.st")
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Failed to upload logs: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Upload failed with status: {}", response.status()));
    }

    let url = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    Ok(url.trim().to_string())
}
