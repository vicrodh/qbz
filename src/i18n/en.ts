export const en = {
  nav: {
    home: 'Home',
    changelog: 'Changelog',
    licenses: 'Licenses',
    github: 'GitHub',
    download: 'Download',
    themeDark: 'Dark',
    themeOled: 'OLED',
    menu: 'Menu',
    close: 'Close',
  },
  hero: {
    kicker: 'Native Linux Qobuz client',
    title: 'Bit-perfect playback, native control, no browser limits.',
    lead: 'Qobuz streams up to 192 kHz. QBZ is a native Linux client with a Rust audio engine that preserves sample rate and bit depth, supports DAC passthrough, and keeps playback transparent.',
    primaryCta: 'Download',
    secondaryCta: 'View on GitHub',
    stats: {
      audio: 'Bit-perfect pipeline',
      dac: 'DAC passthrough',
      casting: 'Chromecast + DLNA',
    },
  },
  why: {
    title: 'Why QBZ exists',
    lead: 'Qobuz does not ship a native Linux app. The web player relies on browser audio stacks that resample, lock output rates, and limit device control. QBZ replaces the web player on Linux with a native pipeline and direct output handling.',
    bullets: [
      'Browsers cap output rates and force resampling.',
      'Limited control over output devices and clocks.',
      'Inconsistent behavior across desktop environments.',
    ],
    note: 'QBZ does not replace Qobuz. It replaces reliance on the web player on Linux.',
  },
  goals: {
    title: 'Design goals',
    lead: 'QBZ focuses on predictable, transparent playback for long listening sessions.',
    items: [
      {
        title: 'Native-first audio pipeline',
        text: 'No browser stack, no hidden resampling, and explicit format handling.',
      },
      {
        title: 'Explicit device control',
        text: 'Choose output devices and modes without guessing what the system is doing.',
      },
      {
        title: 'Predictable behavior',
        text: 'Playback logic is visible, debuggable, and designed to avoid surprises.',
      },
      {
        title: 'Open source by default',
        text: 'MIT licensed, no telemetry, and development in the open.',
      },
    ],
  },
  screenshots: {
    title: 'Interface snapshots',
    lead: 'Native views optimized for long listening sessions.',
    items: [
      {
        title: 'Home and queue control',
        text: 'Fast navigation with direct queue and playback context.',
      },
      {
        title: 'Focus playback mode',
        text: 'Full-screen listening with lyrics and device awareness.',
      },
      {
        title: 'Local library management',
        text: 'Indexed local collections with artwork, CUE, and metadata support.',
      },
    ],
  },
  capabilities: {
    title: 'Key capabilities',
    lead: 'Focused features that do what the web player cannot.',
    items: {
      audio: {
        title: 'Native audio playback',
        bullets: [
          'Native decoding for FLAC, ALAC, AAC, and MP3.',
          'Preserve sample rate and bit depth end-to-end.',
          'DAC passthrough and exclusive mode support.',
        ],
      },
      library: {
        title: 'Local music library',
        bullets: [
          'Folder scanning with metadata extraction.',
          'Cover art discovery and caching.',
          'CUE sheet support and SQLite indexing.',
        ],
      },
      playlists: {
        title: 'Playlist interoperability',
        bullets: [
          'Import from Spotify, Apple Music, Tidal, and Deezer.',
          'Local track matching with quality preference.',
          'No third-party conversion services required.',
        ],
      },
      desktop: {
        title: 'Linux desktop integration',
        bullets: [
          'MPRIS media controls and media keys.',
          'Desktop notifications and keyboard shortcuts.',
          'PipeWire device enumeration and selection.',
        ],
      },
      casting: {
        title: 'Network playback',
        bullets: [
          'Chromecast and DLNA/UPnP support.',
          'Unified device picker with handoff.',
          'Stable playback keepalive handling.',
        ],
      },
    },
  },
  downloads: {
    title: 'Downloads',
    lead: 'Latest builds are pulled from GitHub Releases. Choose the format that fits your distro.',
    recommendedLabel: 'Recommended for your system',
    allLabel: 'All available downloads',
    loading: 'Loading release data…',
    error: 'Unable to load release data. Use the GitHub Releases page instead.',
    versionLabel: 'Release',
    viewAll: 'View all releases',
    fileCount: '{{count}} files',
    instructionsTitle: 'Install commands',
    instructions: {
      aur: 'yay -S qbz-bin',
      appimage: 'chmod +x QBZ.AppImage && ./QBZ.AppImage',
      deb: 'sudo dpkg -i qbz_*.deb',
      rpm: 'sudo rpm -i qbz-*.rpm',
      flatpak: 'flatpak install --user ./qbz.flatpak',
      tarball: 'tar -xzf qbz.tar.gz && ./qbz',
    },
    buildTitle: 'Build from source (advanced)',
    buildBody: 'QBZ targets Linux. macOS builds may work, but features like PipeWire, casting, and device control can be incomplete or unstable.',
    buildDisclaimer: 'If you build your own binaries, you are responsible for API keys and platform-specific dependencies.',
  },
  audience: {
    title: 'Who this is for',
    lead: 'QBZ is designed for listeners who want a native, transparent playback path on Linux.',
    items: [
      'Linux users who want a real Qobuz desktop client.',
      'Audiophiles who care about sample rate, bit depth, and DAC control.',
      'Listeners who prefer native tools over browser wrappers.',
      'Users who want local library and streaming in one place.',
    ],
    notFor: 'QBZ is not a general-purpose streaming service replacement.',
  },
  openSource: {
    title: 'Open source and transparent',
    lead: 'QBZ is free and open source, with no telemetry or tracking.',
    items: [
      'MIT licensed and developed in the open.',
      'No analytics, ads, or background tracking.',
      'Optional integrations only when you enable them.',
      'Inspired by the FOSS Qobuz ecosystem, including qobuz-dl.',
    ],
  },
  linuxFirst: {
    title: 'Linux first',
    lead: 'QBZ is developed and tested for Linux. macOS builds are experimental and may lack features or stability.',
  },
  apis: {
    title: 'Optional API keys',
    lead: 'API keys are only required if you build QBZ yourself. Releases include the defaults needed for standard features.',
    summary: 'Show optional integrations',
    items: [
      'Last.fm scrobbling and now-playing updates.',
      'Discogs artwork lookup for local libraries.',
      'Spotify and Tidal playlist import.',
      'Song.link sharing.',
    ],
  },
  footer: {
    disclaimer: 'QBZ is an independent project and is not affiliated with Qobuz.',
    rights: 'Released under the MIT License.',
  },
  changelog: {
    title: 'Changelog',
    lead: 'Release notes are pulled directly from GitHub Releases.',
    latestLabel: 'Latest release',
    loading: 'Loading release notes…',
    empty: 'No releases found yet.',
    viewOnGitHub: 'View full release notes on GitHub',
  },
  licenses: {
    title: 'Licenses and attributions',
    lead: 'QBZ is MIT licensed and built on top of open-source libraries and APIs.',
    qbzLicense: 'QBZ License',
    qbzLicenseBody: 'QBZ is released under the MIT License.',
    viewLicense: 'View license on GitHub',
    categories: {
      core: {
        title: 'Core technologies',
        items: ['Rust', 'Tauri', 'Svelte', 'Vite', 'SQLite'],
      },
      audio: {
        title: 'Audio and media libraries',
        items: ['Rodio', 'Symphonia', 'Lofty'],
      },
      casting: {
        title: 'Casting and networking',
        items: ['rust_cast', 'DLNA/UPnP AVTransport'],
      },
      lyrics: {
        title: 'Lyrics providers',
        items: ['LRCLIB'],
      },
      integrations: {
        title: 'Integrations and APIs',
        items: ['Qobuz', 'Last.fm API', 'Discogs API', 'Spotify API', 'Tidal API', 'Song.link (Odesli)'],
      },
      website: {
        title: 'Website stack',
        items: ['React', 'Vite', 'TypeScript', 'i18next', 'react-i18next'],
      },
    },
    acknowledgments: 'Thanks to the open-source projects and API providers that make QBZ possible.',
    qobuzDisclaimer: 'Qobuz is a trademark of its respective owner. QBZ is not affiliated with Qobuz.',
  },
}
