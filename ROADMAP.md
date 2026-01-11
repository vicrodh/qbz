# QBZ Launch Roadmap

> Last updated: 2026-01-11

## Overview

6-point roadmap for QBZ v1.0 launch - a native Qobuz client for Linux.

---

## 1. Session Persistence

**Status:** Partially Working

**Description:** Remember playback state, queue, and position when restarting the app.

**Current State:**
- Basic implementation exists but inconsistent ("funciona a veces si a veces no")
- Sometimes restores session, sometimes doesn't

**TODO:**
- [ ] Debug why persistence is inconsistent
- [ ] Ensure queue is always saved on app close
- [ ] Restore playback position accurately
- [ ] Handle edge cases (empty queue, corrupted state file)

---

## 2. Device Select / Passthrough / Exclusivity Wiring

**Status:** Completed

**Description:** Full audio device management with exclusive mode and DAC passthrough.

**Implemented Features:**
- [x] Device selection from PipeWire sinks
- [x] Pretty device names from PipeWire descriptions
- [x] Exclusive mode toggle
- [x] DAC passthrough for external devices
- [x] Proper device release when disabling exclusive mode
- [x] Audio device reinitialization (`ReinitDevice` command)
- [x] AudioOutputBadges showing DAC/EXC status
- [x] Volume display in device tooltip

---

## 3. Tray Icon

**Status:** Not Started

**Description:** System tray icon with playback controls and quick actions.

**TODO:**
- [ ] Add system tray icon
- [ ] Play/Pause from tray
- [ ] Next/Previous track
- [ ] Show current track info
- [ ] Quick access to settings
- [ ] Minimize to tray option

---

## 4. MiniPlayer

**Status:** Not Started

**Description:** Compact floating player window for minimal screen usage.

**TODO:**
- [ ] Design compact player UI
- [ ] Implement floating window mode
- [ ] Essential controls (play/pause, next/prev, seek)
- [ ] Album art display
- [ ] Always-on-top option
- [ ] Toggle between full and mini mode

---

## 5. DLNA and AirCast Integration

**Status:** Partial (ChromeCast only)

**Description:** Stream to network devices via DLNA, AirPlay, and ChromeCast.

**Current State:**
- ChromeCast streaming works
- DLNA not implemented
- AirPlay (AirCast) not implemented

**TODO:**
- [ ] DLNA device discovery
- [ ] DLNA streaming implementation
- [ ] AirPlay/AirCast support
- [ ] Device selector in UI
- [ ] Handle network device disconnection gracefully

---

## 6. Playlist Management

**Status:** Partial (Import implemented)

**Description:** Create, edit, and manage playlists within QBZ.

**Implemented Features:**
- [x] Import playlists from Spotify, Apple Music, Tidal, Deezer
- [x] Track matching via ISRC + fuzzy matching algorithm
- [x] Progress log UI during import
- [x] Auto-create Qobuz playlist with matched tracks

**TODO:**
- [ ] Create new playlists (basic UI exists)
- [ ] Add/remove tracks from playlists
- [ ] Reorder tracks in playlist
- [ ] Delete playlists
- [ ] Sync with Qobuz account playlists
- [ ] Export playlists

---

## Additional Completed Features (This Session)

- [x] Enhanced notifications with album artwork
- [x] Quality info in notifications (Hi-Res / CD Quality badges)
- [x] 3-line notification format (Title, Artist • Album, Quality)
- [x] Window drag region fix for TitleBar
- [x] Removed unnecessary PipeWire polling (was every 10s)
- [x] Artwork caching with MD5 hash filenames
- [x] Playlist import from Spotify/Apple/Tidal/Deezer (Codex integration)

---

## Technical Notes

- **Stack:** Tauri 2.0, Rust backend, Svelte 5 frontend
- **Audio:** rodio/cpal with PipeWire/ALSA integration
- **Commands:** `pactl list sinks` for PipeWire device info
- **Notifications:** `notify_rust` with `reqwest::blocking` for artwork download
