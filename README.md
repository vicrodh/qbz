<p align="center">
  <img src="static/logo.png" alt="QBZ-NIX logo" width="180" />
</p>

# QBZ-NIX

Free and open source (FOSS) Qobuz client for Linux with native, high-fidelity playback. QBZ-NIX goes beyond wrapper apps by using a purpose-built Rust playback engine that removes browser sample-rate limits, adds DAC passthrough, and delivers true hi-res audio.

## Why QBZ-NIX

Browsers cap audio output around 48 kHz, while Qobuz streams up to 192 kHz. QBZ-NIX uses a native playback pipeline so your system and DAC can receive the original resolution. This is a native client, not a wrapper, which enables features like DAC passthrough, real device control, caching, and media integration.

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

QBZ-NIX is MIT-licensed and fully open source. No telemetry, no lock-in, and no hidden services. Just a clean, transparent player built for Linux audio fans.

## Inspiration

QBZ-NIX draws inspiration from projects like qobuz-dl, and from the broader Linux audio community that values open tools and high-fidelity playback.

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
