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
    heading: 'QBZ',
    title: 'Bit-perfect playback, native control, no browser limits.',
    lead: 'Qobuz streams up to 192 kHz. QBZ is an unofficial native Linux client with a Rust audio engine that preserves sample rate and bit depth, supports DAC passthrough, and keeps playback transparent.',
    primaryCta: 'Download',
    secondaryCta: 'View on GitHub',
    stats: {
      audio: 'Bit-perfect pipeline',
      dac: 'DAC passthrough',
      native: 'Native Linux + Rust',
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
        title: 'Immersive mode',
        text: 'Full-screen coverflow, lyrics, and ambient backgrounds.',
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
      radio: {
        title: 'Radio',
        bullets: [
          'Deterministic, local radio playlists.',
          'Consistent listening experience.',
          'Transparent and explainable.',
        ],
      },
      offline: {
        title: 'Offline mode',
        bullets: [
          'Works offline when there is no internet, or by choice.',
          'Access your local library offline.',
          'Listen now, scroll and sync later.',
        ],
      },
      immersive: {
        title: 'Immersive Player',
        bullets: [
          'WebGL-powered ambient backgrounds.',
          'Lyrics, coverflow, and focused listening panels.',
          'Distraction-free visual experience.',
        ],
      },
      dacWizard: {
        title: 'DAC Setup Wizard',
        bullets: [
          'Guided configuration for bit-perfect PipeWire.',
          'Generates distro-specific commands automatically.',
          'Simplifies complex audio setup.',
        ],
      },

      discovery: {
        title: 'Smart Discovery',
        bullets: [
          'Vector-based playlist suggestions.',
          'Local similarity engine for deeper cuts.',
          'Find tracks ensuring consistent vibe.',
        ],
      },
      genres: {
        title: 'Advanced Filtering',
        bullets: [
          'Deep three-level genre hierarchy.',
          'Context-aware subgenre precision.',
          'Drill down beyond basic categories.',
        ],
      },
      metadata: {
        title: 'Metadata & credits',
        bullets: [
          'MusicBrainz integration for artist and album enrichment.',
          'Musician pages with roles, credits, and discography.',
          'Tag editor for local library with non-destructive sidecar storage.',
        ],
      },
      hideArtists: {
        title: 'Hide Artists',
        bullets: [
          'Block artists from your library and recommendations.',
          'Clean up discovery feeds automatically.',
          'Persistent across sessions.',
        ],
      },
      songRecommendations: {
        title: 'Song Recommendations',
        bullets: [
          'Algorithmic suggestions based on your local playback history.',
          'Powered by unique Qobuz and MusicBrainz metadata combination.',
          'Expand playlists with one click.',
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
    buildInstructions: {
      summary: 'Show build instructions',
      prereqTitle: 'Prerequisites',
      nodeNote: 'Node.js 20+ required. Use nvm, fnm, or your distro package manager.',
      cloneTitle: 'Clone and build',
      apiTitle: 'API keys (optional)',
      apiLead: 'API keys are embedded at compile-time. Copy the example file and add your keys:',
      apiBody: 'Edit .env with your API keys, then run npm run dev:tauri to load them automatically.',
      apiKeysTitle: 'Where to get API keys',
      apiOptional: 'All integrations are optional. The app works without them, but corresponding features will be disabled.',
    },
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
      'Inspired by the broader FOSS audio ecosystem and Linux audio community.',
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
    disclaimer: 'This application uses the Qobuz API but is not certified by, affiliated with, or endorsed by Qobuz.',
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
        items: ['LRCLIB', 'lyrics.ovh'],
      },
      integrations: {
        title: 'Integrations and APIs',
        items: ['Qobuz', 'Last.fm API', 'Discogs API', 'Spotify API', 'Tidal API', 'Song.link (Odesli)'],
      },
      inspiration: {
        title: 'Inspiration',
        items: ['Linux audio community', 'FOSS audio ecosystem'],
      },
      website: {
        title: 'Website stack',
        items: ['React', 'Vite', 'TypeScript', 'i18next', 'react-i18next'],
      },
    },
    acknowledgments: 'Thanks to the open-source projects and API providers that make QBZ possible.',
    qobuzDisclaimer: 'Qobuz is a trademark of its respective owner. QBZ is not affiliated with Qobuz.',
  },
  comingSoon: {
    title: 'Coming Soon / Experimental',
    lead: 'Features currently in active development or testing.',
    badge: 'Experimental',
    items: [
      {
        title: 'Remote Control API',
        text: 'Headless operation and external control support.',
      },
      {
        title: 'Advanced Audio Visualizer',
        text: 'Spectrum analyzer and waveform visualization.',
      },
    ],
  },
  about: {
    title: 'Why QBZ?',
    content: `QBZ is a personal project that first saw the light over {{years}} years ago. It started when I used qobuz-dl's code to create a local API backend for searching and playing music on my machine. Months—maybe a year—later, caught up in the hype of migrating everything to Rust and wanting to learn a new language for my tech stack, I rewrote that backend in Rust. I also built a fairly handcrafted web interface that at least let me pull my Qobuz playlists and use it as a distraction-free media player. I still hoped an official client would come along. Honestly, even though I'm a Linux enthusiast, I'm not a fan of terminal music players—I use the terminal so much that I sometimes close it without thinking, which kills my music when I close the wrong window.

Like many people in 2025, I integrated AI code agents into my workflow (the real one—the one that pays the bills). This made me think about unlocking this project from my personal stack. So I took ideas from the music players I normally use, features I thought people like me would enjoy, and—yes, if you're wondering, "Is this app vibe-coded?"—the answer is yes, without shame. But let me be clear: I'm a software engineer, so I've made sure to incorporate best practices, design patterns, and proper architecture. The planning alone, writing prompts, architecture plans, and orchestration took me a couple of weeks. This project is not a "I built a new ERP in 3 days without writing a single line of code" kind of thing. Every block of code has been reviewed as if I were reviewing an intern's work. I don't believe in zero-code, but I don't hate vibe-coding either. I believe in adapt or die, and that every tool is useful when used responsibly. If you're curious about the tools I used: Claude Code, GPT Codex, Copilot, and Figma AI have had to put up with me and my mood swings and decision changes—highly recommended.`,
    donationsTitle: 'Donations',
    donationsContent: `If you'd like to support QBZ, I truly appreciate it. That said, there are projects that have shaped my workflow and deserve recognition: KDE Plasma, Neovim, and of course Arch Linux (I use Arch BTW). Consider splitting your generosity—or donating to them in QBZ's name. Either way, your feedback and kind words already mean a lot. Fresh eyes are always the best QA for a solo developer. Of course, a coffee can't be refused.`,
    donationLinks: {
      kde: 'KDE Plasma',
      neovim: 'Neovim',
      arch: 'Arch Linux',
    },
  },

  qobuzLinux: {
    hero: {
      kicker: 'Native Linux Qobuz client',
      title: 'Qobuz for Linux — Native Hi-Fi Player (Not a Web Wrapper)',
      lead1: 'QBZ is a native Linux desktop client for Qobuz™, built for users who care about bit-perfect playback, direct DAC control, and real high-resolution audio.',
      lead2: 'Unlike browser-based players or web wrappers, QBZ does not rely on Chromium or WebAudio. It uses a native audio pipeline designed specifically for Linux.',
      ctaDownload: 'Download QBZ',
      ctaGithub: 'View on GitHub',
    },
    whyNative: {
      title: 'Why Qobuz needs a native Linux client',
      lead: 'Qobuz streams lossless audio up to 24-bit/192 kHz. But without a native Linux application, users are forced to rely on the web player or third-party wrappers—both of which compromise audio quality.',
      bullets: [
        'The official Qobuz web player uses browser audio stacks that resample to 48 kHz.',
        'Web wrappers (Electron-based) inherit the same WebAudio limitations.',
        'Linux audiophiles have no way to achieve bit-perfect playback through a browser.',
        'DAC passthrough and exclusive mode are impossible via WebAudio.',
      ],
    },
    different: {
      title: 'What makes QBZ different',
      lead: 'QBZ is not a web wrapper. It is a native Linux application built with Rust and Tauri, using a dedicated audio engine that bypasses browser limitations entirely.',
      features: [
        { title: 'Native audio pipeline', text: 'Built-in decoders for FLAC, ALAC, AAC, and MP3. No browser audio stack. No hidden resampling.' },
        { title: 'Direct DAC access', text: 'Supports ALSA exclusive mode (hw: devices) and PipeWire passthrough for bit-perfect output.' },
        { title: 'Per-track sample-rate switching', text: 'Automatically adjusts output sample rate to match source (44.1, 48, 88.2, 96, 176.4, 192 kHz).' },
        { title: 'No Chromium', text: 'QBZ uses Tauri (WebView-based UI) with a Rust backend. It does not bundle Chromium or Electron.' },
      ],
    },
    bitPerfect: {
      title: 'Bit-perfect playback on Linux',
      lead: 'QBZ supports two primary audio backend configurations for achieving bit-perfect playback.',
      alsa: {
        title: 'ALSA Direct (hw: devices)',
        text: 'For maximum control, QBZ can output directly to ALSA hardware devices, bypassing PulseAudio and PipeWire entirely. This enables exclusive mode, where QBZ takes full control of the DAC.',
        bullets: [
          'Exclusive access to the audio device (no mixing with system sounds).',
          'True bit-perfect output—no resampling, no format conversion.',
          'Per-track sample rate switching at the hardware level.',
        ],
      },
      pipewire: {
        title: 'PipeWire (advanced setups)',
        text: 'For users running PipeWire, QBZ can be configured for passthrough mode with proper WirePlumber rules, achieving near-bit-perfect output while maintaining system integration.',
        bullets: [
          'Compatible with modern Linux desktops (Fedora, Arch, etc.).',
          'Supports hardware volume control delegation to the DAC.',
          'QBZ includes a DAC Setup Wizard to generate the necessary configuration.',
        ],
      },
    },
    wrappers: {
      title: 'Why web wrappers fall short',
      lead: 'Web wrappers package the Qobuz web player inside a browser shell. They look like native apps, but they inherit all the audio limitations of browsers.',
      bullets: [
        'WebAudio API resamples all audio to 48 kHz, regardless of source quality.',
        'No access to ALSA or PipeWire—audio goes through the browser\'s audio stack.',
        'Cannot request exclusive mode or DAC passthrough.',
        'Hi-Res content (88.2, 96, 176.4, 192 kHz) is downsampled before playback.',
        'No per-track sample rate switching.',
      ],
      note: 'If you\'re using a web wrapper and expecting Hi-Res audio, you\'re likely hearing 48 kHz resampled output.',
    },
    comparison: {
      title: 'QBZ vs web-based Qobuz players',
      lead: 'A technical comparison of audio capabilities.',
      headers: ['Feature', 'QBZ', 'Web Player / Wrappers'],
      rows: [
        { feature: 'Native audio pipeline', qbz: true, web: false, webText: '✗' },
        { feature: 'Bit-perfect playback', qbz: true, web: false, webText: '✗' },
        { feature: 'ALSA exclusive mode', qbz: true, web: false, webText: '✗' },
        { feature: 'DAC passthrough', qbz: true, web: false, webText: '✗' },
        { feature: 'Per-track sample rate switching', qbz: true, web: false, webText: '✗' },
        { feature: 'Hi-Res output (88.2–192 kHz)', qbz: true, web: false, webText: 'Resampled to 48 kHz' },
        { feature: 'No Chromium/Electron', qbz: true, web: false, webText: '✗' },
      ],
    },
    features: {
      title: 'Features at a glance',
      items: [
        { title: 'Qobuz streaming', text: 'Full access to your Qobuz library, favorites, and playlists.' },
        { title: 'Local library', text: 'Index and play local FLAC/ALAC/MP3 files alongside Qobuz content.' },
        { title: 'Chromecast & DLNA', text: 'Cast to network devices with stable playback handling.' },
        { title: 'MPRIS integration', text: 'Media keys and desktop controls work out of the box.' },
        { title: 'Lyrics & metadata', text: 'MusicBrainz enrichment, credits, and synchronized lyrics.' },
        { title: 'Playlist import', text: 'Import playlists from Spotify, Apple Music, Tidal, and Deezer.' },
      ],
    },
    forWho: {
      title: 'Who QBZ is for',
      bullets: [
        'Linux users who want a native Qobuz desktop client.',
        'Audiophiles who care about sample rate, bit depth, and DAC control.',
        'Users frustrated by browser audio limitations.',
        'Anyone who wants streaming and local library in one application.',
      ],
      note: 'QBZ is not a replacement for Qobuz. It is a native interface for users who want more control over their audio playback on Linux.',
    },
    openSource: {
      title: 'Open source and transparent',
      bullets: [
        'MIT licensed—free to use, modify, and distribute.',
        'No telemetry, no analytics, no tracking.',
        'Source code available on GitHub.',
        'Developed in the open with public issue tracking.',
      ],
    },
    install: {
      title: 'Installation',
      lead: 'QBZ is available as AppImage, .deb, .rpm, Flatpak, and AUR packages.',
      cta: 'View all downloads',
    },
    legal: {
      title: 'Legal notice',
      text: 'Qobuz is a trademark of Xandrie SA. QBZ is an independent, unofficial project. It is not certified by, affiliated with, or endorsed by Qobuz. QBZ uses the Qobuz API in accordance with their terms of service. A valid Qobuz subscription is required to use QBZ.',
    },
  },
}
