# Codex Task: Chromecast Integration

## Context

QBZ is a native Qobuz streaming client for Linux built with Tauri 2.0 + Rust backend + SvelteKit frontend. We need to add Chromecast support using the `rust-cast` crate.

**Reference document**: See `docs/CASTING_RESEARCH.md` for full analysis.

## Objective

Implement Chromecast audio casting so users can stream music to Google Cast devices.

---

## Architecture Overview

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  QBZ Frontend   │────▶│   Tauri Commands │────▶│  Cast Module    │
│  (SvelteKit)    │     │   (lib.rs)       │     │  (rust-cast)    │
└─────────────────┘     └──────────────────┘     └────────┬────────┘
                                                          │
                        ┌─────────────────────────────────┼─────────┐
                        │                                 ▼         │
                        │  ┌──────────────┐    ┌─────────────────┐  │
                        │  │ mDNS Device  │    │  Local HTTP     │  │
                        │  │ Discovery    │    │  Audio Server   │  │
                        │  └──────────────┘    └────────┬────────┘  │
                        │                               │           │
                        └───────────────────────────────┼───────────┘
                                                        ▼
                                               ┌─────────────────┐
                                               │  Chromecast     │
                                               │  Device         │
                                               └─────────────────┘
```

---

## Module Structure

Create files under `src-tauri/src/cast/`:

```
src-tauri/src/cast/
├── mod.rs           # Module exports + CastState
├── discovery.rs     # Device discovery via mDNS
├── device.rs        # CastDevice wrapper + connection
├── media_server.rs  # Local HTTP server for streaming
├── commands.rs      # Tauri commands
└── errors.rs        # Error types
```

---

## Task 1: Dependencies & Module Setup

### Cargo.toml additions:
```toml
# Chromecast
rust-cast = "0.21"
mdns-sd = "0.11"  # For device discovery
tiny_http = "0.12"  # Simple HTTP server for streaming
```

### mod.rs structure:
```rust
pub mod commands;
pub mod device;
pub mod discovery;
pub mod errors;
pub mod media_server;

pub use commands::CastState;
pub use device::CastDevice;
pub use discovery::DeviceDiscovery;
pub use errors::CastError;
pub use media_server::MediaServer;
```

---

## Task 2: Device Discovery (`discovery.rs`)

Use `mdns-sd` crate to discover Chromecast devices on the local network.

### Data Model:
```rust
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredDevice {
    pub id: String,           // Unique device ID
    pub name: String,         // Friendly name (e.g., "Living Room TV")
    pub model: String,        // Device model
    pub ip: String,           // IP address
    pub port: u16,            // Cast port (usually 8009)
}
```

### Functions needed:
- `start_discovery()` - Start mDNS discovery in background
- `stop_discovery()` - Stop discovery
- `get_discovered_devices()` - Return list of found devices

### mDNS service type:
- `_googlecast._tcp.local.`

---

## Task 3: Cast Device Connection (`device.rs`)

Wrapper around `rust-cast::CastDevice` for managing connections.

### Functions needed:
```rust
impl CastDeviceConnection {
    /// Connect to a Chromecast by IP and port
    pub fn connect(ip: &str, port: u16) -> Result<Self, CastError>;

    /// Disconnect from device
    pub fn disconnect(&mut self) -> Result<(), CastError>;

    /// Get device status
    pub fn get_status(&self) -> Result<DeviceStatus, CastError>;

    /// Launch media receiver app
    pub fn launch_media_app(&mut self) -> Result<String, CastError>;

    /// Load media URL for playback
    pub fn load_media(&mut self, url: &str, content_type: &str, metadata: MediaMetadata) -> Result<(), CastError>;

    /// Play/pause/stop controls
    pub fn play(&mut self) -> Result<(), CastError>;
    pub fn pause(&mut self) -> Result<(), CastError>;
    pub fn stop(&mut self) -> Result<(), CastError>;

    /// Volume control (0.0 - 1.0)
    pub fn set_volume(&mut self, volume: f32) -> Result<(), CastError>;

    /// Seek to position
    pub fn seek(&mut self, position_secs: f64) -> Result<(), CastError>;
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork_url: Option<String>,
    pub duration_secs: Option<u64>,
}
```

---

## Task 4: Local Media Server (`media_server.rs`)

HTTP server to stream audio to Chromecast. The Cast device will fetch audio from this server.

### Requirements:
- Serve audio files via HTTP
- Support range requests (for seeking)
- Run on dynamic port, return URL to cast device
- Support both local files and in-memory audio data

### Interface:
```rust
pub struct MediaServer {
    port: u16,
    // ...
}

impl MediaServer {
    /// Start server on available port
    pub fn start() -> Result<Self, CastError>;

    /// Stop server
    pub fn stop(&mut self);

    /// Get base URL (e.g., "http://192.168.1.100:8080")
    pub fn base_url(&self) -> String;

    /// Register audio data to serve (returns path like "/audio/123")
    pub fn register_audio(&mut self, id: u64, data: Vec<u8>, content_type: &str) -> String;

    /// Register local file to serve
    pub fn register_file(&mut self, id: u64, file_path: &str) -> Result<String, CastError>;

    /// Get full URL for registered audio
    pub fn get_audio_url(&self, id: u64) -> Option<String>;
}
```

### Content types:
- FLAC: `audio/flac`
- WAV: `audio/wav`
- ALAC/M4A: `audio/mp4`
- AIFF: `audio/aiff`

---

## Task 5: Tauri Commands (`commands.rs`)

### CastState:
```rust
pub struct CastState {
    pub discovery: Arc<Mutex<DeviceDiscovery>>,
    pub connection: Arc<Mutex<Option<CastDeviceConnection>>>,
    pub media_server: Arc<Mutex<MediaServer>>,
}
```

### Commands to implement:
```rust
// Discovery
#[tauri::command]
pub async fn cast_start_discovery(state: State<'_, CastState>) -> Result<(), String>;

#[tauri::command]
pub async fn cast_stop_discovery(state: State<'_, CastState>) -> Result<(), String>;

#[tauri::command]
pub async fn cast_get_devices(state: State<'_, CastState>) -> Result<Vec<DiscoveredDevice>, String>;

// Connection
#[tauri::command]
pub async fn cast_connect(device_id: String, state: State<'_, CastState>) -> Result<(), String>;

#[tauri::command]
pub async fn cast_disconnect(state: State<'_, CastState>) -> Result<(), String>;

#[tauri::command]
pub async fn cast_get_status(state: State<'_, CastState>) -> Result<CastStatus, String>;

// Playback
#[tauri::command]
pub async fn cast_play_track(
    track_id: u64,
    metadata: MediaMetadata,
    state: State<'_, CastState>,
    app_state: State<'_, AppState>,  // For accessing audio cache
) -> Result<(), String>;

#[tauri::command]
pub async fn cast_play_local_track(
    track_id: i64,
    state: State<'_, CastState>,
    library_state: State<'_, LibraryState>,
) -> Result<(), String>;

#[tauri::command]
pub async fn cast_play(state: State<'_, CastState>) -> Result<(), String>;

#[tauri::command]
pub async fn cast_pause(state: State<'_, CastState>) -> Result<(), String>;

#[tauri::command]
pub async fn cast_stop(state: State<'_, CastState>) -> Result<(), String>;

#[tauri::command]
pub async fn cast_seek(position_secs: f64, state: State<'_, CastState>) -> Result<(), String>;

#[tauri::command]
pub async fn cast_set_volume(volume: f32, state: State<'_, CastState>) -> Result<(), String>;
```

---

## Task 6: Integration with lib.rs

Add to `src-tauri/src/lib.rs`:
1. `pub mod cast;`
2. Initialize `CastState` in `run()`
3. Add `.manage(cast_state)`
4. Register all cast commands in `invoke_handler`

---

## Important Notes

### DO NOT modify these files (they're complex and tightly integrated):
- `src-tauri/src/player/mod.rs`
- `src-tauri/src/queue/mod.rs`
- `src-tauri/src/commands/playback.rs`
- Any frontend files (`src/`)

### Testing approach:
1. First test discovery: ensure devices are found
2. Test connection to device
3. Test media server with a simple audio file
4. Test full flow: play track to Chromecast

### Error handling:
- Use `CastError` enum with variants for Discovery, Connection, Media, Protocol errors
- All commands return `Result<T, String>` for Tauri compatibility

### Network considerations:
- Media server must bind to 0.0.0.0 (not localhost) so Chromecast can reach it
- Need to determine local IP address for the Cast device to connect back

---

## Expected Output

After implementation, these commands should work:
```typescript
// Frontend can call:
await invoke('cast_start_discovery');
const devices = await invoke('cast_get_devices');
await invoke('cast_connect', { deviceId: devices[0].id });
await invoke('cast_play_track', { trackId: 123, metadata: {...} });
await invoke('cast_pause');
await invoke('cast_set_volume', { volume: 0.5 });
await invoke('cast_disconnect');
```

---

## Changelog Format

When done, provide a changelog like:
```
- src-tauri/Cargo.toml — added rust-cast, mdns-sd, tiny_http dependencies
- src-tauri/src/cast/errors.rs — CastError enum
- src-tauri/src/cast/discovery.rs — mDNS device discovery
- src-tauri/src/cast/device.rs — CastDeviceConnection wrapper
- src-tauri/src/cast/media_server.rs — HTTP server for audio streaming
- src-tauri/src/cast/commands.rs — Tauri commands + CastState
- src-tauri/src/cast/mod.rs — module exports
- src-tauri/src/lib.rs — wiring (mod cast, manage, commands)
```
