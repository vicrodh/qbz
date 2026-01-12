# QBZ Launch Roadmap

> Last updated: 2026-01-12

## Overview

6-point roadmap for QBZ v1.0 launch - a native Qobuz client for Linux.

---

## 1. Session Persistence

**Status:** Completed

**Description:** Remember playback state, queue, and position when restarting the app.

**Implemented Features:**
- [x] SQLite-based session storage (~/.local/share/qbz-nix/session.db)
- [x] Position saved every 5 seconds during playback (debounced)
- [x] Full session save every 30 seconds during playback
- [x] Immediate save on pause
- [x] Volume changes persisted immediately
- [x] Shuffle/repeat mode changes persisted immediately
- [x] Queue and current track restored on login
- [x] Playback position restored accurately

**Fixed Issues (2026-01-12):**
- Fixed race condition in beforeunload (async save might not complete)
- Enabled debouncedSavePosition() which was imported but never called
- Added periodic full session save during playback
- Position now saved frequently enough that app crash loses max 5-30 seconds

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

**Status:** Completed

**Description:** System tray icon with playback controls and quick actions.

**Implemented Features:**
- [x] System tray icon with app icon
- [x] Play/Pause from tray menu
- [x] Next/Previous track from tray menu
- [x] Show/Hide window toggle
- [x] Left-click toggles window visibility
- [x] Double-click shows and focuses window
- [x] Right-click shows context menu
- [x] Quit option in tray menu

**Not Implemented (optional):**
- [ ] Dynamic tooltip with current track info
- [ ] Quick access to settings
- [ ] Minimize to tray on window close (currently just hides)

---

## 4. MiniPlayer

**Status:** Completed

**Description:** Compact floating player window for minimal screen usage.

**Implemented Features:**
- [x] Compact 340x90 floating window
- [x] Draggable (no decorations)
- [x] Always-on-top toggle (pin button)
- [x] Play/Pause, Next/Previous controls
- [x] Album art, track title, artist display
- [x] Progress bar
- [x] State sync from main window via events
- [x] Toggle via NowPlayingBar button (PictureInPicture icon)

**Technical:**
- Route: `/miniplayer`
- Service: `src/lib/services/miniplayerService.ts`
- Uses Tauri WebviewWindow for secondary window
- Events: `miniplayer:track`, `miniplayer:playback`

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
