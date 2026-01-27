//! Local HTTP server for Chromecast media streaming

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::cast::CastError;

#[derive(Clone)]
struct MediaEntry {
    content_type: String,
    size: u64,
    source: MediaSource,
}

#[derive(Clone)]
enum MediaSource {
    Data(Vec<u8>),
    File(PathBuf),
}

/// Simple HTTP server for audio streaming
pub struct MediaServer {
    port: u16,
    base_url: String,
    entries: Arc<Mutex<HashMap<u64, MediaEntry>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MediaServer {
    /// Start server on available port
    /// Uses port 9876 by default for DLNA compatibility, falls back to random port if busy
    pub fn start() -> Result<Self, CastError> {
        // Try fixed port first for easier firewall configuration
        let server = Server::http("0.0.0.0:9876")
            .or_else(|_| {
                log::warn!("MediaServer: Port 9876 busy, using random port");
                Server::http("0.0.0.0:0")
            })
            .map_err(|e| CastError::Server(format!("Failed to start HTTP server: {}", e)))?;

        let port = server
            .server_addr()
            .to_ip()
            .map(|addr| addr.port())
            .ok_or_else(|| CastError::Server("Failed to determine HTTP port".to_string()))?;

        let base_ip = local_ip().unwrap_or_else(|| IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        let base_url = format_base_url(base_ip, port);
        
        log::info!("MediaServer: Started on {}:{} (base_url: {})", base_ip, port, base_url);

        let entries = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let entries_clone = entries.clone();
        let shutdown_clone = shutdown.clone();
        let port_for_log = port;

        let handle = thread::spawn(move || {
            log::info!("MediaServer: Thread started, listening on port {}", port_for_log);
            while !shutdown_clone.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(250)) {
                    Ok(Some(request)) => {
                        let response = handle_request(request.method(), request.url(), &request, &entries_clone);
                        let _ = request.respond(response);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        log::error!("MediaServer: Thread error: {:?}, exiting", e);
                        break;
                    }
                }
            }
            log::info!("MediaServer: Thread exiting");
        });

        Ok(Self {
            port,
            base_url,
            entries,
            shutdown,
            handle: Some(handle),
        })
    }

    /// Stop server
    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    /// Get base URL (e.g., "http://192.168.1.100:8080")
    pub fn base_url(&self) -> String {
        self.base_url.clone()
    }

    /// Register audio data to serve (returns path like "/audio/123")
    pub fn register_audio(&mut self, id: u64, data: Vec<u8>, content_type: &str) -> String {
        let entry = MediaEntry {
            content_type: content_type.to_string(),
            size: data.len() as u64,
            source: MediaSource::Data(data),
        };

        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(id, entry);
        }

        format!("/audio/{}", id)
    }

    /// Register local file to serve
    pub fn register_file(&mut self, id: u64, file_path: &str) -> Result<String, CastError> {
        let path = Path::new(file_path);
        if !path.exists() {
            return Err(CastError::InvalidRequest(format!(
                "File not found: {}",
                file_path
            )));
        }

        let size = path
            .metadata()
            .map_err(CastError::Io)?
            .len();
        let content_type = content_type_for_path(path);

        let entry = MediaEntry {
            content_type,
            size,
            source: MediaSource::File(path.to_path_buf()),
        };

        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(id, entry);
        }

        Ok(format!("/audio/{}", id))
    }

    /// Get full URL for registered audio
    pub fn get_audio_url(&self, id: u64) -> Option<String> {
        let entries = self.entries.lock().ok()?;
        if entries.contains_key(&id) {
            return Some(format!("{}/audio/{}", self.base_url, id));
        }
        None
    }

    /// Get full URL for registered audio using the local IP that can reach target_ip.
    pub fn get_audio_url_for_target(&self, id: u64, target_ip: &str) -> Option<String> {
        let entries = self.entries.lock().ok()?;
        if !entries.contains_key(&id) {
            return None;
        }

        let base_url = local_ip_for_target(target_ip)
            .map(|ip| format_base_url(ip, self.port))
            .unwrap_or_else(|| self.base_url.clone());

        Some(format!("{}/audio/{}", base_url, id))
    }

    /// Get server port
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for MediaServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_request(
    method: &Method,
    url: &str,
    request: &tiny_http::Request,
    entries: &Arc<Mutex<HashMap<u64, MediaEntry>>>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    // Log all incoming requests for debugging
    log::info!("MediaServer: {} request from {:?} for {}", method, request.remote_addr(), url);
    
    if method != &Method::Get {
        log::warn!("MediaServer: Rejected non-GET request: {}", method);
        return Response::from_data(Vec::new()).with_status_code(StatusCode(405));
    }

    let id = match parse_audio_id(url) {
        Some(id) => id,
        None => {
            log::warn!("MediaServer: 404 - Could not parse audio ID from URL: {}", url);
            return Response::from_data(Vec::new()).with_status_code(StatusCode(404));
        }
    };

    let entry = match entries.lock().ok().and_then(|map| map.get(&id).cloned()) {
        Some(entry) => {
            log::info!("MediaServer: Found entry for ID {}, content-type: {}, size: {} bytes", id, entry.content_type, entry.size);
            entry
        }
        None => {
            log::warn!("MediaServer: 404 - No entry found for ID: {}", id);
            return Response::from_data(Vec::new()).with_status_code(StatusCode(404));
        }
    };

    let range_header = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .map(|h| h.value.as_str());

    let range = range_header.and_then(|header| parse_range(header, entry.size));

    let (data, status_code, content_range) = match read_range(&entry, range) {
        Ok(result) => result,
        Err(_) => return Response::from_data(Vec::new()).with_status_code(StatusCode(500)),
    };

    let mut response = Response::from_data(data)
        .with_status_code(status_code)
        .with_header(header("Content-Type", &entry.content_type))
        .with_header(header("Accept-Ranges", "bytes"));

    if let Some(content_range) = content_range {
        response = response.with_header(header("Content-Range", &content_range));
    }

    response
}

fn read_range(
    entry: &MediaEntry,
    range: Option<(u64, u64)>,
) -> Result<(Vec<u8>, StatusCode, Option<String>), std::io::Error> {
    let (start, end, status_code, content_range) = if let Some((start, end)) = range {
        let content_range = format!("bytes {}-{}/{}", start, end, entry.size);
        (start, end, StatusCode(206), Some(content_range))
    } else {
        (0, entry.size.saturating_sub(1), StatusCode(200), None)
    };

    let mut buffer = Vec::new();

    match &entry.source {
        MediaSource::Data(data) => {
            let end = end.min(entry.size.saturating_sub(1)) as usize;
            let start = start.min(end as u64) as usize;
            buffer.extend_from_slice(&data[start..=end]);
        }
        MediaSource::File(path) => {
            let mut file = File::open(path)?;
            file.seek(SeekFrom::Start(start))?;
            let length = (end - start + 1) as usize;
            buffer.resize(length, 0);
            file.read_exact(&mut buffer)?;
        }
    }

    Ok((buffer, status_code, content_range))
}

fn parse_audio_id(url: &str) -> Option<u64> {
    let path = url.split('?').next().unwrap_or(url);
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    if parts.len() != 2 || parts[0] != "audio" {
        return None;
    }
    parts[1].parse().ok()
}

fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    if !header.starts_with("bytes=") {
        return None;
    }

    let range = header.trim_start_matches("bytes=");
    let mut parts = range.split('-');
    let start_str = parts.next().unwrap_or("");
    let end_str = parts.next().unwrap_or("");

    if start_str.is_empty() {
        let suffix = end_str.parse::<u64>().ok()?;
        if suffix == 0 {
            return None;
        }
        let start = total.saturating_sub(suffix);
        let end = total.saturating_sub(1);
        return Some((start, end));
    }

    let start = start_str.parse::<u64>().ok()?;
    if start >= total {
        return None;
    }

    let end = if end_str.is_empty() {
        total.saturating_sub(1)
    } else {
        end_str.parse::<u64>().ok()?.min(total.saturating_sub(1))
    };

    Some((start, end))
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).unwrap()
}

fn local_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|addr| addr.ip())
}

fn local_ip_for_target(target_ip: &str) -> Option<IpAddr> {
    let ip: IpAddr = target_ip.parse().ok()?;
    let bind_addr = if ip.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let socket = UdpSocket::bind(bind_addr).ok()?;
    socket.connect(SocketAddr::new(ip, 80)).ok()?;
    socket.local_addr().ok().map(|addr| addr.ip())
}

fn format_base_url(ip: IpAddr, port: u16) -> String {
    match ip {
        IpAddr::V4(addr) => format!("http://{}:{}", addr, port),
        IpAddr::V6(addr) => format!("http://[{}]:{}", addr, port),
    }
}

fn content_type_for_path(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase()) {
        Some(ext) if ext == "flac" => "audio/flac".to_string(),
        Some(ext) if ext == "wav" => "audio/wav".to_string(),
        Some(ext) if ext == "m4a" => "audio/mp4".to_string(),
        Some(ext) if ext == "aiff" || ext == "aif" => "audio/aiff".to_string(),
        Some(ext) if ext == "ape" => "audio/ape".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}
