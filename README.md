<p align="center">
  <img src="static/logo.png" alt="QBZ logo" width="180" />
</p>

<p align="center">
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/badge/github-vicrodh%2Fqbz-0b0b0b?style=flat-square&logo=github" alt="GitHub repo" /></a>
  <a href="https://github.com/vicrodh/qbz/releases"><img src="https://img.shields.io/github/v/release/vicrodh/qbz?style=flat-square" alt="Release" /></a>
  <a href="https://aur.archlinux.org/packages/qbz-bin"><img src="https://img.shields.io/aur/version/qbz-bin?style=flat-square&logo=archlinux" alt="AUR" /></a>
  <a href="https://snapcraft.io/qbz-player"><img src="https://img.shields.io/badge/snap-qbz--player-0b0b0b?style=flat-square&logo=snapcraft" alt="Snap" /></a>
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/github/license/vicrodh/qbz?style=flat-square" alt="License" /></a>
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/badge/platform-Linux-0b0b0b?style=flat-square&logo=linux" alt="Platform" /></a>
</p>

<p align="center">
  <a href="https://techforpalestine.org/learn-more"><img src="https://raw.githubusercontent.com/Safouene1/support-palestine-banner/master/StandWithPalestine.svg" alt="StandWithPalestine" /></a>
</p>

# QBZ

QBZ is a free and open source (FOSS) high-fidelity streaming client for Linux with native playback. It is a real desktop application, not a web wrapper, so it can use DAC passthrough, switch sample rates per track, and deliver bit-perfect audio.

## Legal / Branding

- This application uses the Qobuz API but is not certified by Qobuz.
- Qobuz is a trademark of Qobuz. QBZ is not affiliated with, endorsed by, or certified by Qobuz.
- The offline library is a temporary playback cache for listening without an internet connection while you have a valid subscription. You do not receive a license to keep or redistribute the content. If your subscription becomes invalid, QBZ will remove all cached content after 3 days.
- Credentials may be stored in your system keyring if you have a keyring configured.
- Qobuz Terms of Service: https://www.qobuz.com/us-en/legal/terms

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

### Snap

```bash
sudo snap install qbz-player
```

Post-install plug connections for audio backends:

```bash
sudo snap connect qbz-player:alsa
sudo snap connect qbz-player:pulseaudio
sudo snap connect qbz-player:pipewire
```

Optional: external drives and network mounts

```bash
sudo snap connect qbz-player:removable-media
```

### Flatpak

```bash
# Download from releases
flatpak install ./QBZ.flatpak
```

#### Important for Audiophiles

Due to Flatpak sandbox restrictions, **PipeWire backend cannot guarantee bit-perfect playback**. The sandbox prevents QBZ from controlling the PipeWire daemon's sample rate configuration.

**For bit-perfect audio in Flatpak:**
- Use **ALSA Direct backend** in Settings > Audio > Audio Backend
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

> **Requires glibc 2.38+** (Ubuntu 24.04+, Debian 13+, Mint 22+). Older releases like Ubuntu 22.04 or Pop!_OS 22.04 ship glibc 2.35 and won't work -- use Flatpak, Snap, or AppImage instead.

### RPM (Fedora/openSUSE/RHEL-based)

Download the `.rpm` from [Releases](https://github.com/vicrodh/qbz/releases).

```bash
sudo dnf install ./qbz-*.rpm
```

> **Requires glibc 2.38+** (Fedora 39+, openSUSE Tumbleweed, RHEL 10+). For older releases, use Flatpak, Snap, or AppImage instead.

> **Note:** Pre-built binaries include all API integrations (Last.fm, Discogs, Spotify, Apple Music, Tidal, Deezer) ready to use. If you build from source, you'll need to provide your own API keys.

## Features

### Audio and Playback
- **Bit-perfect playback** with DAC passthrough and per-track sample rate switching (44.1 kHz to 192 kHz).
- **Four audio backends:** PipeWire, ALSA, ALSA Direct (hw: bypass), and PulseAudio.
- **DAC Setup Wizard** for guided bit-perfect configuration.
- Native decoding for FLAC, MP3, AAC, ALAC, WavPack, Ogg Vorbis, and Opus via Symphonia.
- Quality selection with automatic fallback across Qobuz tiers.
- Per-device output selection with exclusive mode.
- Gapless playback with precise position tracking.
- **Loudness normalization** (EBU R128) with ReplayGain support.
- Two-level audio cache: L1 in-memory (400 MB) with L2 disk spillover (800 MB) and next-track prefetching.

### Queue and Library
- Queue management with shuffle, repeat (track/queue/off), and history navigation.
- Favorites and playlists from your Qobuz account.
- **Local library:** directory scanning, metadata extraction with lofty, CUE sheet parsing, and SQLite indexing.
- Grid and list views with search, A-Z index, and grouping by artist or album.
- Multi-disc album grouping with disc headers.
- Local artwork detection (folder and embedded) with Discogs fallback.
- **Tag editor:** edit metadata (title, artist, album, genre, year) with sidecar storage that preserves original files.
- **Virtualized lists** for smooth scrolling across large libraries.

### Playlist Import
- Import public playlists from Spotify, Apple Music, Tidal, and Deezer into your Qobuz library.
- Automatic track matching with fuzzy search.
- Batch import with progress tracking.

### Network Casting
- **Chromecast** device discovery and streaming.
- **DLNA/UPnP** device discovery and streaming (AVTransport SOAP).
- Unified cast picker with protocol selection.
- Seamless playback handoff to network devices.

### Integrations
- **MPRIS** media controls and media key support.
- **Last.fm** scrobbling and now-playing updates.
- **ListenBrainz** scrobbling with offline queue and listen history sync.
- **MusicBrainz** artist enrichment, musician credits, recording relationships, and album personnel.
- **Discogs** artwork fetching for local library.
- Desktop notifications for track changes.
- Shareable Qobuz URLs and universal SongLink links (Odesli).

### Immersive Player
- Full-screen expanded player with tabbed panel system.
- **14 visualization panels:** spectrum analyzer, oscilloscope, spectrogram, energy bands, Lissajous curves, transient pulse, vinyl animation, coverflow, and more.
- **Synchronized lyrics** with line-by-line display and mini view.
- Queue, track info, playback history, and suggestions panels.

### Interface
- 26 built-in themes (Dark, OLED, Nord, Dracula, Tokyo Night, Catppuccin Mocha, Breeze, Adwaita, and more).
- **Auto-theme:** generate themes dynamically from your DE color scheme, wallpaper, or a custom image (experimental).
- **Native KDE title bar** on Plasma Wayland via KWin window rules.
- Focus mode for distraction-free listening.
- Mini player mode.
- **Musician pages:** explore performers, composers, and producers with their discography and roles.
- **Label pages:** browse record label catalogs.
- **Album credits:** detailed personnel and recording credits from MusicBrainz.
- **Image lightbox** with full-screen artwork viewing.
- **Custom artist images:** right-click any artist to set a custom image.
- Configurable keyboard shortcuts.
- UI zoom from 80% to 200%.
- 5 font choices including system font.
- **4 languages:** English, Spanish, German, and French.

### Recommendations and Discovery
- **Home page** with personalized sections: recent albums, continue listening, top artists, favorites, and weekly suggestions.
- Genre filtering with 3-level hierarchy.
- **Artist similarity engine** using vector embeddings for radio and suggestions.
- Playlist suggestions based on artist similarity.
- Dynamic suggestions with seed-based recommendations.

### Offline Support
- Offline mode detection with automatic reconnection.
- Offline playback cache for previously streamed content.
- ListenBrainz and Last.fm offline queuing with automatic sync.

### Remote Control
- Built-in HTTP API for LAN remote control.
- Self-signed TLS with auto-generated certificates.

### Settings
- Audio device selection and quality preferences.
- API keys configuration for self-hosted builds.
- Theme and appearance options with live preview.
- System tray with minimize-to-tray and configurable behavior.
- **Update notifications** with What's New changelogs.
- Graphics composition settings for GPU/Wayland troubleshooting.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| **Desktop shell** | Rust + Tauri 2.0 |
| **Frontend** | SvelteKit + Svelte 5 (runes) + TypeScript + Vite |
| **Audio decoding** | Symphonia (all codecs) via vendored rodio |
| **Audio backends** | PipeWire, ALSA (via alsa-rs), ALSA Direct (hw:), PulseAudio |
| **Networking** | reqwest (rustls-tls), axum (local API server) |
| **Database** | rusqlite (bundled SQLite, WAL mode) |
| **Local library** | walkdir + lofty (metadata) + image (thumbnails) |
| **Desktop integration** | souvlaki (MPRIS), notify-rust, keyring |
| **Casting** | rust_cast (Chromecast), rupnp (DLNA/UPnP), mdns-sd (AirPlay) |
| **Internationalization** | svelte-i18n (4 locales) |
| **Icons** | lucide-svelte |

### Multi-Crate Architecture

The Rust backend is organized into independent crates for modularity:

```
crates/
  qbz-models/        Shared domain types (Track, Album, Artist, etc.)
  qbz-audio/         Audio backends, loudness analysis, device management
  qbz-player/        Playback engine, streaming, queue control
  qbz-qobuz/         Qobuz API client and authentication
  qbz-core/          Orchestrator coordinating player, audio, and API
  qbz-library/       Local library scanning, metadata, thumbnails
  qbz-integrations/  Last.fm, ListenBrainz, MusicBrainz, Discogs
  qbz-cache/         L1 memory + L2 disk audio caching
  qbz-cast/          Chromecast, DLNA/UPnP, AirPlay casting
```

## Project Structure

```
qbz/
├── src/                       Frontend (SvelteKit + Svelte 5)
│   ├── lib/
│   │   ├── components/        120+ Svelte components
│   │   │   ├── views/         20+ view components (Home, Search, Settings, etc.)
│   │   │   └── immersive/     Immersive player panels and visualizers
│   │   ├── stores/            40+ state stores (Svelte 5 runes)
│   │   ├── services/          Backend integration services
│   │   ├── i18n/locales/      en, es, de, fr translations
│   │   ├── app/               Bootstrap and initialization
│   │   └── utils/             Helpers (zoom, keyboard, sanitize, etc.)
│   └── routes/                SvelteKit routes (main + miniplayer)
├── src-tauri/
│   ├── src/
│   │   ├── auto_theme/        Dynamic theme generation from DE/wallpaper
│   │   ├── audio/             Audio backend re-exports
│   │   ├── config/            13 settings modules
│   │   ├── commands_v2.rs     V2 command handlers (active API surface)
│   │   ├── core_bridge.rs     Bridge to multi-crate architecture
│   │   └── ...                30+ feature modules
│   ├── vendor/
│   │   ├── rodio/             Vendored audio playback (f32 pipeline)
│   │   └── cpal/              Vendored audio device library
│   └── Cargo.toml
├── crates/                    9 independent Rust crates (see above)
├── packaging/
│   ├── aur/                   Arch Linux PKGBUILD
│   └── flatpak/               Flatpak manifest and metainfo
├── scripts/                   Build, dev, and CI helper scripts
├── .github/workflows/         CI/CD (Linux x86_64, aarch64, Flatpak, AUR)
└── snapcraft.yaml             Snap package definition
```

## Building from Source

### Prerequisites

- Rust (latest stable)
- Node.js 20+
- Linux with audio support (PipeWire, ALSA, or PulseAudio)

### System Dependencies

**Debian/Ubuntu:**
```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libasound2-dev \
  libappindicator3-dev librsvg2-dev libssl-dev pkg-config
```

**Fedora:**
```bash
sudo dnf install webkit2gtk4.1-devel gtk3-devel alsa-lib-devel \
  libappindicator-gtk3-devel librsvg2-devel openssl-devel pkg-config
```

**Arch Linux:**
```bash
sudo pacman -S webkit2gtk-4.1 gtk3 alsa-lib libappindicator-gtk3 \
  librsvg openssl pkg-config
```

### Build

```bash
# Clone the repository
git clone https://github.com/vicrodh/qbz.git
cd qbz

# Install frontend dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production (generates DEB, RPM, and AppImage)
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
# Migrate cache (offline library, artwork, etc.)
mv ~/.cache/qbz-nix ~/.cache/qbz

# Migrate config (credentials)
mv ~/.config/qbz-nix ~/.config/qbz

# Migrate data (library database, settings)
mv ~/.local/share/qbz-nix ~/.local/share/qbz
```

If you run both development and production builds, they now share the same data directories.

## Known Issues

### Audio Playback

**Seekbar Performance with Hi-Res Audio**
- When using ALSA Direct or PipeWire DAC Passthrough with high sample rates (>96kHz), seeking can take 10-20 seconds
- This is due to the decoder needing to decode all samples from the start to the seek position
- Workaround: Use prev/next track buttons for instant navigation
- Future: Byte-level seeking will be implemented in a future release to fix this

**First Sample Rate Change**
- The first large sample rate change (e.g., 88.2kHz to 44.1kHz) may have a brief delay as the hardware stabilizes
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
- Use the in-app Settings wizard for guided setup
- **Flatpak users:** PipeWire bit-perfect can work if PipeWire is configured correctly. If it does not, use ALSA Direct.

### Graphics and Rendering

QBZ uses WebKit for rendering. Some hardware/driver combinations may cause crashes or poor performance. Settings are available in **Settings > Appearance > Composition**.

#### Quick Recovery

If QBZ crashes on startup after changing graphics settings:

```bash
# Nuclear option: reset ALL graphics settings to defaults
qbz --reset-graphics

# Alternative: disable GPU rendering for this session only
QBZ_HARDWARE_ACCEL=0 qbz
```

The `--reset-graphics` flag resets force_x11, gdk_scale, gdk_dpi_scale, and force_dmabuf to their defaults, then exits. Start QBZ normally afterwards.

#### Environment Variables

Set these before launching QBZ to override settings:

| Variable | Effect | When to use |
|----------|--------|-------------|
| `QBZ_HARDWARE_ACCEL=0` | Disable all GPU rendering | Crashes on startup, severe UI glitches |
| `QBZ_HARDWARE_ACCEL=1` | Force full GPU (bypass safety) | Only if you know your GPU works perfectly |
| `QBZ_FORCE_X11=1` | Use XWayland instead of Wayland | NVIDIA crashes on Wayland, protocol errors |
| `QBZ_SOFTWARE_RENDER=1` | Force Mesa llvmpipe | VMs, headless servers, broken GPU drivers |
| `QBZ_DISABLE_DMABUF=1` | Disable DMA-BUF renderer | Intel Arc EGL crashes, NVIDIA Error 71 |
| `QBZ_FORCE_DMABUF=1` | Force DMA-BUF renderer | Testing only, may crash |

#### Common Scenarios

**NVIDIA on Wayland (crashes, protocol errors, black screen)**
```bash
QBZ_FORCE_X11=1 qbz
```
Or enable "Force X11 backend" in Settings > Appearance > Composition.

**Intel Arc GPU (EGL crashes, "Could not create default EGL display")**
```bash
QBZ_DISABLE_DMABUF=1 qbz
```

**Virtual machines or containers**
```bash
QBZ_SOFTWARE_RENDER=1 qbz
```

**Severe UI lag or freezing**
```bash
QBZ_HARDWARE_ACCEL=0 qbz
```

**XWayland scaling issues (blurry UI)**

After enabling Force X11, configure scaling in Settings > Appearance > Composition:
- **GDK_SCALE**: Integer scaling (1, 2)
- **GDK_DPI_SCALE**: Fractional scaling (0.5, 1, 1.5, 2)

#### What the defaults do

- **X11 sessions**: Full GPU acceleration, nothing disabled
- **Wayland sessions**: Compositing mode disabled (prevents protocol errors), DMA-BUF disabled for all GPUs (prevents EGL crashes)
- **NVIDIA on X11**: Only DMA-BUF disabled

All settings require a restart to take effect.

## Open Source

QBZ is MIT-licensed and fully open source. No telemetry, no lock-in, and no hidden services. Just a clean, transparent player built for Linux audio fans.

## Contributing

Contributions are welcome. Please read `CONTRIBUTING.md` before submitting issues or pull requests.

## License

MIT
