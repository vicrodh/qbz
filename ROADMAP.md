# QBZ Launch Roadmap

> Last updated: 2026-01-11

## Overview

6-point roadmap for QBZ v1.0 launch - a native Qobuz client for Linux.

---

## 1. Session Persistence

**Status:** Partially Working

**Description:** Remember playback state, queue, and position when restarting the app.

**Current State:**
- Basic implementation exists but inconsistent
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

**Status:** Completed

**Description:** Full playlist management with CRUD, statistics, and enhanced UI.

### Implemented Features:

**Import (100% complete):**
- [x] Import from Spotify, Apple Music, Tidal, Deezer
- [x] Track matching via ISRC + fuzzy matching algorithm
- [x] Progress log UI during import
- [x] Auto-create Qobuz playlist with matched tracks
- [x] Rate limiting and API chunk size handling

**Database & Backend (100% complete):**
- [x] `playlist_settings` table with custom_artwork_path, sort_by, sort_order, hidden, position
- [x] `playlist_stats` table with play_count, last_played_at
- [x] Backend commands: `playlist_get_settings`, `playlist_set_artwork`, `playlist_get_stats`, etc.
- [x] `get_playlist_suggestions` command for ML-based track suggestions

**UI Components (100% complete):**
- [x] PlaylistCollage component (2x2 grid layout)
- [x] Custom artwork selection per playlist
- [x] Sort menu in sidebar (dropdown with submenu)
- [x] PlaylistDetailView with collage header
- [x] Stats display (play count)
- [x] Filter dropdown (All/Visible/Hidden) in PlaylistManagerView
- [x] Drag-to-reorder playlists in PlaylistManagerView
- [x] Grid view toggle (list vs mosaic) in PlaylistManagerView
- [x] PlaylistEditModal (PlaylistModal handles edit name/description/visibility/delete)

**ML Suggestions (Removed):**
- Removed due to lack of similarity algorithm (suggested unrelated tracks)

### Recent Fixes (This Session):
- [x] Fixed custom artwork not loading (asset:// URL conversion)
- [x] Fixed settings state persisting between playlists
- [x] Fixed sort submenu closing too fast
- [x] Fixed z-index for all menus/tooltips (10000+)
- [x] Added hi-res logo variants (grayscale version in use)

---

## Additional Completed Features

**UI/UX:**
- [x] Enhanced notifications with album artwork
- [x] Quality info in notifications (Hi-Res / CD Quality badges)
- [x] 3-line notification format (Title, Artist • Album, Quality)
- [x] Window drag region fix for TitleBar
- [x] Hi-res logo variants (gold, gray, blue, mono SVGs)
- [x] Sidebar tooltip system with lazy loading
- [x] Fixed scrollbar spacing in main content

**Performance:**
- [x] Removed unnecessary PipeWire polling (was every 10s)
- [x] Artwork caching with MD5 hash filenames

---

## Technical Notes

- **Stack:** Tauri 2.0, Rust backend, Svelte 5 (runes) frontend
- **Audio:** rodio/cpal with PipeWire/ALSA integration
- **Commands:** `pactl list sinks` for PipeWire device info
- **Notifications:** `notify_rust` with `reqwest::blocking` for artwork download
- **Icons:** Hi-res logo at `/static/hi-res-gray.svg` (grayscale version)

---

## Priority Order for Remaining Work

1. **Session Persistence debugging** - Critical for UX
2. **Tray Icon** - Expected feature for music apps
3. **MiniPlayer** - Nice to have
4. **DLNA/AirPlay** - Lower priority
