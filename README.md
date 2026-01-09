<p align="center">
  <img src="static/logo.png" alt="QBZ logo" width="180" />
</p>

<p align="center">
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/badge/github-vicrodh%2Fqbz-0b0b0b?style=flat&logo=github" alt="GitHub repo" /></a>
  <a href="https://github.com/vicrodh/qbz/releases"><img src="https://img.shields.io/github/v/release/vicrodh/qbz?style=flat" alt="Release" /></a>
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/badge/version-0.1.0-0b0b0b?style=flat" alt="Version" /></a>
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/github/license/vicrodh/qbz?style=flat" alt="License" /></a>
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/github/last-commit/vicrodh/qbz?style=flat" alt="Last commit" /></a>
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/badge/platform-Linux-0b0b0b?style=flat&logo=linux" alt="Platform" /></a>
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/badge/lang-Rust-0b0b0b?style=flat&logo=rust" alt="Language: Rust" /></a>
  <a href="https://github.com/vicrodh/qbz"><img src="https://img.shields.io/badge/ui-Svelte-0b0b0b?style=flat&logo=svelte" alt="UI: Svelte" /></a>
</p>

# QBZ

QBZ is a free and open source (FOSS) Qobuz client for Linux with native, high-fidelity playback. It is a real desktop application, not a web wrapper, so it can use DAC passthrough, preserve hi-res sample rates end-to-end, and deliver bit-perfect audio.

## Why QBZ

Browsers cap audio output around 48 kHz, while Qobuz streams up to 192 kHz. QBZ uses a native playback pipeline with direct device control so your system and DAC receive the original resolution, with caching and system integrations that wrappers cannot provide.

## Screenshots

Coming soon.

## Features

### Streaming and Playback
- Qobuz authentication and full catalog search (albums, tracks, artists, playlists).
- Native decoding for FLAC and MP3 with real-time playback state updates.
- Quality selection with automatic fallback across Qobuz tiers.
- Audio device enumeration and per-device output selection.
- DAC passthrough mode for bit-perfect playback.
- Gapless-ready playback pipeline with precise position tracking.

### Queue and Library
- Queue management with shuffle, repeat, and history navigation.
- In-memory audio cache with LRU eviction and next-track prefetching.
- Favorites and playlists from your Qobuz account.
- Local library backend: directory scanning, metadata extraction, CUE sheet parsing, and SQLite indexing.

### Integrations
- MPRIS media controls and media key support on Linux.
- Desktop notifications for track changes.
- Last.fm scrobbling and now-playing updates.
- Shareable Qobuz URLs and universal SongLink links (Odesli).

### Interface
- Now playing, queue panel, and full-screen playback views.
- Focus mode for distraction-free listening.
- English and Spanish localization.

## Open Source

QBZ is MIT-licensed and fully open source. No telemetry, no lock-in, and no hidden services. Just a clean, transparent player built for Linux audio fans.

## Inspiration

QBZ draws inspiration from projects like qobuz-dl, and from the broader Linux audio community that values open tools and high-fidelity playback.

## Tech Stack

- **Desktop:** Rust + Tauri 2
- **Frontend:** SvelteKit + TypeScript + Vite
- **Audio:** rodio + symphonia
- **Networking:** reqwest
- **Local library:** walkdir + lofty + rusqlite
- **Integrations:** souvlaki (MPRIS), notify-rust, SongLink (Odesli)
- **UX:** svelte-i18n, lucide-svelte

## Development

### Prerequisites

- Rust (latest stable)
- Node.js 18+
- Linux with audio support (PipeWire, ALSA, or PulseAudio)

### Setup

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Project Structure

```
qbz-nix/
├── src/                  # Frontend (SvelteKit)
├── src-tauri/
│   └── src/
│       ├── api/          # Qobuz API client
│       ├── player/       # Audio playback engine
│       ├── queue/        # Queue management
│       ├── cache/        # Audio cache and prefetch
│       ├── library/      # Local library backend
│       ├── lastfm/       # Last.fm integration
│       ├── share/        # SongLink / share utilities
│       ├── media_controls/ # MPRIS integration
│       └── commands/     # Tauri IPC commands
└── static/               # Static assets and logo
```

## License

MIT
