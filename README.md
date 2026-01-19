<p align="center">
  <img src="static/logo.png" alt="QBZ logo" width="180" />
</p>

<p align="center">
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/badge/github-vicrodh%2Fqbz-0b0b0b?style=flat&logo=github" alt="GitHub repo" /></a>
  <a href="https://github.com/vicrodh/qbz/releases"><img src="https://img.shields.io/github/v/release/vicrodh/qbz?style=flat" alt="Release" /></a>
  <a href="https://aur.archlinux.org/packages/qbz-bin"><img src="https://img.shields.io/aur/version/qbz-bin?style=flat&logo=archlinux" alt="AUR" /></a>
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/github/license/vicrodh/qbz?style=flat" alt="License" /></a>
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/badge/platform-Linux-0b0b0b?style=flat&logo=linux" alt="Platform" /></a>
</p>

# QBZ

QBZ is a free and open source (FOSS) Qobuz client for Linux with native, high-fidelity playback. It is a real desktop application, not a web wrapper, so it can use DAC passthrough, switch sample rates per track, and deliver bit-perfect audio.

## Why QBZ

Browsers cap audio output around 48 kHz, while Qobuz streams up to 192 kHz. QBZ uses a native playback pipeline with direct device control, exclusive mode, and no forced resampling so your system and DAC receive the original resolution, with caching and system integrations that wrappers cannot provide.

## Installation

### Arch Linux (AUR)

```bash
# Using yay
yay -S qbz-bin

# Using paru
paru -S qbz-bin
```

### Flatpak

```bash
# Download from releases
flatpak install ./QBZ.flatpak
```

#### Important for Audiophiles

Due to Flatpak sandbox restrictions, **PipeWire backend cannot guarantee bit-perfect playback**. The sandbox prevents QBZ from controlling the PipeWire daemon's sample rate configuration.

**For bit-perfect audio in Flatpak:**
- Use **ALSA Direct backend** in Settings → Audio → Audio Backend
- Select your DAC from the device list
- Enable DAC Passthrough

**For full PipeWire bit-perfect support:**
- Install via native packages (.deb, .rpm) or build from source

The app will display a warning in Settings when this limitation affects your configuration.

#### NAS/Network Storage Access

If your music library is on a NAS or network mount, grant filesystem access:

```bash
# CIFS/Samba mount
flatpak override --user --filesystem=/mnt/nas com.blitzfc.qbz

# SSHFS mount
flatpak override --user --filesystem=/home/$USER/music-nas com.blitzfc.qbz

# Custom mount point
flatpak override --user --filesystem=/path/to/music com.blitzfc.qbz
```

This permission persists across reboots and updates.

### AppImage

Download the latest release from the [Releases](https://github.com/vicrodh/qbz/releases) page.

```bash
chmod +x QBZ.AppImage
./QBZ.AppImage
```

### DEB (Debian/Ubuntu/Mint/Pop!_OS/Zorin)

Download the `.deb` from [Releases](https://github.com/vicrodh/qbz/releases).

```bash
sudo apt install ./qbz_*.deb
```

### RPM (Fedora/openSUSE/RHEL-based)

Download the `.rpm` from [Releases](https://github.com/vicrodh/qbz/releases).

```bash
sudo dnf install ./qbz-*.rpm
```

> **Note:** Pre-built binaries include all API integrations (Last.fm, Discogs, Spotify, Apple Music, Tidal, Deezer) ready to use. If you build from source, you'll need to provide your own API keys.

## Screenshots

Coming soon.

## Features

### Streaming and Playback
- Qobuz authentication and full catalog search (albums, tracks, artists, playlists).
- Native decoding for FLAC, MP3, AAC, and ALAC with real-time playback state updates.
- Quality selection with automatic fallback across Qobuz tiers.
- Audio device enumeration and per-device output selection.
- Exclusive mode and DAC passthrough for bit-perfect playback.
- Preserve original sample rates end-to-end where supported.
- Gapless-ready playback pipeline with precise position tracking.

### Queue and Library
- Queue management with shuffle, repeat, and history navigation.
- In-memory audio cache with LRU eviction and next-track prefetching.
- Favorites and playlists from your Qobuz account.
- Local library backend: directory scanning, metadata extraction, CUE sheet parsing, and SQLite indexing.
- Grid and list views with search, A-Z index, and grouping by artist or album.
- Multi-disc album grouping with disc headers in album views.
- Local artwork detection (folder and embedded) with Discogs fallback.

### Playlist Import
- Import public playlists from Spotify, Apple Music, Tidal, and Deezer into your Qobuz library.
- Automatic track matching with fuzzy search.
- Batch import with progress tracking.

### Network Casting
- Chromecast device discovery and streaming.
- DLNA/UPnP device discovery and streaming (AVTransport SOAP).
- Unified cast picker with protocol selection.
- Seamless playback handoff to network devices.

### Integrations
- MPRIS media controls and media key support on Linux.
- Desktop notifications for track changes.
- Last.fm scrobbling and now-playing updates.
- Discogs artwork fetching for local library.
- Shareable Qobuz URLs and universal SongLink links (Odesli).

### Interface
- Now playing, queue panel, and full-screen playback views.
- Focus mode for distraction-free listening.
- Mini player mode.
- Keyboard shortcuts for common actions.
- English and Spanish localization.

### Settings
- Audio device selection and quality preferences.
- API keys configuration for self-hosted builds.
- Theme and appearance options.

### Performance
- Low CPU usage in idle and playback with adaptive audio thread scheduling.

## Open Source

QBZ is MIT-licensed and fully open source. No telemetry, no lock-in, and no hidden services. Just a clean, transparent player built for Linux audio fans.

## Inspiration

QBZ draws inspiration from projects like qobuz-dl, and from the broader Linux audio community that values open tools and high-fidelity playback.

## Tech Stack

- **Desktop:** Rust + Tauri 2
- **Frontend:** SvelteKit 5 + TypeScript + Vite
- **Audio:** rodio + symphonia
- **Networking:** reqwest
- **Local library:** walkdir + lofty + rusqlite
- **Integrations:** souvlaki (MPRIS), notify-rust, SongLink (Odesli)
- **UX:** svelte-i18n, lucide-svelte

## Building from Source

### Prerequisites

- Rust (latest stable)
- Node.js 20+
- Linux with audio support (PipeWire, ALSA, or PulseAudio)
- System dependencies: `webkit2gtk-4.1`, `gtk3`, `alsa-lib`

### Setup

```bash
# Clone the repository
git clone https://github.com/vicrodh/qbz.git
cd qbz

# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

### Environment Variables

API keys are only required when you build QBZ yourself. The app runs without them, but the corresponding features will be disabled.

<details>
<summary>API keys and integrations (optional)</summary>

When building from source, you need to provide your own API keys. Copy the example environment file:

```bash
cp .env.example .env
```

Edit `.env` with your API keys, then use the development script that properly loads them:

```bash
# Use this command for development with API keys
npm run dev:tauri
```

**Important**: API keys are embedded at compile-time using Rust's `option_env!()` macro. The `.env` file must be loaded into your shell environment before compilation. The `npm run dev:tauri` script does this automatically. If you prefer manual control:

```bash
# Load .env into current shell (once per terminal session)
set -a
source .env
set +a

# Now run dev
npm run tauri dev
```

#### Last.fm Integration

1. Go to [Last.fm API Account](https://www.last.fm/api/account/create)
2. Create a new application
3. Add to `.env`:

```env
LAST_FM_API_KEY=your_api_key
LAST_FM_API_SHARED_SECRET=your_shared_secret
```

#### Discogs Integration (Local Library Artwork)

1. Go to [Discogs Developer Settings](https://www.discogs.com/settings/developers)
2. Create a new application
3. Add to `.env`:

```env
DISCOGS_API_CLIENT_KEY=your_consumer_key
DISCOGS_API_CLIENT_SECRET=your_consumer_secret
```

#### Spotify Integration (Playlist Import)

1. Go to [Spotify Developer Dashboard](https://developer.spotify.com/dashboard)
2. Create a new application
3. Add to `.env`:

```env
SPOTIFY_API_CLIENT_ID=your_client_id
SPOTIFY_API_CLIENT_SECRET=your_client_secret
```

#### Tidal Integration (Playlist Import)

1. Go to [Tidal Developer Portal](https://developer.tidal.com/)
2. Create a new application
3. Add to `.env`:

```env
TIDAL_API_CLIENT_ID=your_client_id
TIDAL_API_CLIENT_SECRET=your_client_secret
```

> **Note:** All integrations are optional. The application will work without them, but the corresponding features will be disabled.
</details>

### Data Migration

If you've used QBZ before version 1.1.6, your data was stored under `qbz-nix` directories. The application now uses unified `qbz` paths. To migrate your existing data:

```bash
# Migrate cache (downloads, artwork, etc.)
mv ~/.cache/qbz-nix ~/.cache/qbz

# Migrate config (credentials)
mv ~/.config/qbz-nix ~/.config/qbz

# Migrate data (library database, settings)
mv ~/.local/share/qbz-nix ~/.local/share/qbz
```

If you run both development and production builds, they now share the same data directories.

## Project Structure

```
qbz/
├── src/                  # Frontend (SvelteKit)
├── src-tauri/
│   └── src/
│       ├── api/          # Qobuz API client
│       ├── player/       # Audio playback engine
│       ├── queue/        # Queue management
│       ├── cache/        # Audio cache and prefetch
│       ├── cast/         # Chromecast & DLNA casting
│       ├── library/      # Local library backend
│       ├── lastfm/       # Last.fm integration
│       ├── discogs/      # Discogs integration
│       ├── playlist_import/ # Spotify/Tidal import
│       ├── share/        # SongLink / share utilities
│       ├── media_controls/ # MPRIS integration
│       └── commands/     # Tauri IPC commands
├── packaging/
│   ├── aur/              # Arch Linux PKGBUILD
│   └── flatpak/          # Flatpak manifest
└── static/               # Static assets and logo
```

## Known Issues

### Audio Playback

**Seekbar Performance with Hi-Res Audio**
- When using ALSA Direct or PipeWire DAC Passthrough with high sample rates (>96kHz), seeking can take 10-20 seconds
- This is due to the decoder needing to decode all samples from the start to the seek position
- Workaround: Use prev/next track buttons for instant navigation
- Future: Byte-level seeking will be implemented in a future release to fix this

**First Sample Rate Change**
- The first large sample rate change (e.g., 88.2kHz → 44.1kHz) may have a brief delay as the hardware stabilizes
- Subsequent changes of the same type are smooth
- This is normal hardware behavior and not a bug

### Audio Backends

**ALSA Direct Mode (hw: devices)**
- Provides bit-perfect playback by bypassing all software mixing
- Exclusive access: Other applications cannot play audio simultaneously
- Hardware volume control is experimental and may not work with all DACs
- If hardware mixer fails, use your DAC/amplifier's physical volume control

**PipeWire DAC Passthrough**
- Requires PipeWire configuration for automatic sample rate switching
- See [AUDIO_BACKENDS.md](qbz-nix-docs/AUDIO_BACKENDS.md) for setup instructions
- **Flatpak users:** PipeWire bit-perfect is not available due to sandbox restrictions - use ALSA Direct backend instead

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## macOS (Experimental)

QBZ is built for Linux first, but the codebase can compile on macOS in many cases. Expect missing integrations and bugs, especially around audio devices, exclusive mode, MPRIS, and platform-specific packaging.

If you want to try it:

```bash
npm install
npm run tauri dev
```

Please treat macOS support as best-effort and report issues with clear logs.

## License

MIT
