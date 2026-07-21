# 2.0.2 — Rebuild 破 (You Can (Not) Advance)

This one got away from me. 2.0.2 started as a maintenance release and turned into one of the biggest updates QBZ has had — when I opened the changelog it was much longer than I expected, and honestly that's the good kind of surprise.

The headline is that QBZ now has a life outside its own window: a full headless daemon you can run on a Raspberry Pi or a server and drive entirely from the terminal, plus a touch-first Kiosk mode for the other end of the spectrum. On the desktop there's a new dynamic album-art background, casting and quality that finally tell you the truth about what you're hearing, and a long run of audio, rendering and library work. The full 2.0/2.0.1 story is kept below.

---

## The headless daemon (qbzd)

The big one. `qbzd` is a single slint-free binary — the daemon, its own CLI, and an interactive setup TUI, all in one — meant for a box that's always on and wired to your DAC.

  - **A player with no window** — plays music, shows up in the official Qobuz apps as a castable Connect device, and configures itself over SSH; no desktop, no webview.
  - **Bit-perfect, same as the desktop** — it runs the exact protected audio core (ALSA direct, no forced resampling), so 192 kHz/24-bit stays 192 kHz/24-bit.
  - **A full remote control from the terminal** — search, play anything (a Qobuz id, a share URL, even a Deezer link), browse albums and artists, discover, radio, recommendations, favorites, playlist create/edit, queue editing, lyrics, cover art, shuffle and repeat — all as `qbzd <verb>`, local or across the LAN.
  - **Setup TUI** — a raspi-config-style configurator for account, audio/DAC, playback, Qobuz Connect and network, with the HiFi Wizard ported in, all over SSH.
  - **Scrobbling** — connect Last.fm and ListenBrainz from the terminal or the setup TUI; it scrobbles what it plays and queues listens offline so none are lost.
  - **Desktop media controls** — publishes MPRIS, so a KDE/GNOME media widget, a plasmoid, or your keyboard's media keys drive the daemon with no extra client.
  - **Live events** — `qbzd watch` streams track changes, queue edits and volume as newline-delimited JSON, ready to pipe into a status bar or a panel.
  - **Service files for any init** — `qbzd service` generates a ready-to-install systemd, OpenRC or runit unit and resolves the audio environment for you.

Full setup and usage instructions live in the daemon manual: [Headless Daemon wiki](https://github.com/vicrodh/qbz/wiki/Headless-Daemon).

---

## Library, home & discovery

  - **Library "All"** — a mixed feed with track cards, ownership-aware playlist cards, artist playlists, local favorites, and genre/sort controls.
  - **Pinned on Home** — a per-user Pinned section as a mixed carousel with pin affordances; recently-played rails auto-refresh.
  - **ArtistGridCard everywhere** — a consistent artist card across the mixed carousels and the Scene view, genres as the subtitle.
  - **Qobuz Playlists "View all"** — a full page with shared header tools; the advanced sub-genre filter sends raw ids to the server for Tauri parity.
  - **Playlist reorder** — drag-and-drop for custom-order tracks using the shared drag gesture, plus an optimistic sidebar rename.

---

## Kiosk mode

At the opposite end of "no window at all," a touch-first face for touchscreens and small panels.

  - **Opt-in profile** — set `QBZ_PROFILE=kiosk` for a big-target, touch-friendly interface built for a Raspberry Pi screen or a small display.
  - **Its own shell** — a NavRail, touch scrolling, an on-screen keyboard, and lightweight Search / Library / Discover / Album / Artist views that only build what's on screen.
  - **A centerpiece Now Playing** — a dominant cover, a cover↔lyrics toggle with synced follow, and queue/history tabs.
  - **Switch on the fly** — a live Kiosk↔Desktop toggle in the Now Playing layout menu; boots windowed by default, fullscreen opt-in via `QBZ_KIOSK_FULLSCREEN`.

---

## A dynamic background

  - **App-wide album-art background** — turn on a backdrop that blooms behind the whole shell, with ambient shaders and translucent bars, panels and controls.
  - **Shows through the content** — the backdrop reads through the content area, with glass carousels and a livelier ambient scene in immersive mode.
  - **Per-GPU selector** — choose the adapter that drives it.

---

## Casting & quality, told truthfully (#638)

  - **An honest quality badge** — Now Playing reports the quality actually delivered and names the reason when it's lower than requested (device cap, cast tier, or the source itself).
  - **Per-renderer quality caps** — cap the tier for a specific Chromecast/DLNA device and QBZ honors it, cast requests included.
  - **Measured, not assumed** — cast surfaces report the real delivered quality, and probed FLAC is never mislabeled as MP3.
  - **Streaming quality honored when casting** — casts follow your quality preference, and the audio cache clears when you change it.
  - **Safer media paths** — CSPRNG media-path tokens and redacted URIs in the logs.

---

## Hardening, fixes & polish

The rest is the kind of work you feel more than see — the full list is in the changelog.

  - **Audio & playback** — ALSA-exclusive and DAC fixes (#641, #508), fail-closed on inexact exclusive rates, sturdier player / DSD / CMAF paths, and a fully fluid (de-quantized) volume slider.
  - **Rendering & performance** — femtovg partial rendering so only what changed repaints (#617), one wgpu device reused across Wayland surface recreations (#558), and smarter weak-GPU tiering.
  - **New Catppuccin themes** — Latte, Frappé and Macchiato, thanks to [@TerminalTilt](https://github.com/TerminalTilt); plus a searchable theme dropdown and light-theme legibility fixes.
  - **Interface** — a unified track context menu, cursor-anchored card menus, and assorted layout fixes.
  - **Tighter security** — secret files stored `0600`, secrets redacted from logs, and Last.fm token prefixes no longer logged.
  - **Under the hood** — a headless playback driver, a settings bundle engine, a Qobuz 403 circuit breaker, and routine dependency bumps.

Full changelog: https://github.com/vicrodh/qbz/compare/v2.0.1...v2.0.2

---

# 2.0.1 — Rebuild 破 (You Can (Not) Advance)

The first maintenance release on top of the 2.0 rebuild: lighter scrolling, self-recovery from bad states, more faithful text across languages, and a few features that missed the 2.0 cutoff. The full 2.0 story is kept below.

---

## New and fixed in 2.0.1

  - **Queue row menu** — "Remove all after" and "Add to playlist" now work.
  - **Viewport windowing** — Home, Discover, Favorites, Search and carousels only build what's on screen.
  - **Idle CPU** — equalizer bars and animations run off a coarse clock; near-zero when paused.
  - **Renderer resilience** — autonomous degradation ladder with a UI-loop watchdog and auto-revert.
  - **Startup recovery** — crash-chain watchdog resets a bad view and skips queue restore after repeated early deaths.
  - **Single instance on Linux** — relaunching focuses the running window (#544, #559).
  - **Hybrid GPUs** — high-performance adapter by default (#542); wgpu surface timeout treated as a skipped frame.
  - **CJK on macOS** — Japanese and Korean glyphs render at every weight (#543); first line never culled under elide.
  - **HTML entities** — decoded in titles and descriptions, including malformed bare forms.
  - **Lenient parsing** — one bad item no longer blanks a whole list.
  - **Dutch** — the interface now ships in 8 languages.
  - **Auto theme** — derive the palette from your desktop accent, wallpaper or an image.
  - **Custom theme editor** — an inline editor with a color picker.
  - **Auto-skip** — unavailable tracks are skipped instead of parking at 0:00.
  - **Stream errors** — surfaced as toasts, sandbox-aware for ALSA under Flatpak and Snap.
  - **DLNA casting** — strict-renderer discovery, DIDL and self-heal; no more full-track buffer hoarding.
  - **Clean quit** — the forced PipeWire clock is released on exit (#521).
  - **Home** — all 13 configurable sections render, with a Recently Played "View all" page.
  - **Album favorites** — a real toggle, kept in sync everywhere.
  - **Guest offline mode** — use your local library without signing in.
  - **Sorting** — the "Date added" playlist sort is back; the Library Tracks tab gained a sort control.
  - **Log sharing** — the viewer opens expanded with Copy-all, Upload and an always-visible Share logs entry.
  - **Qobuz Connect** — device-name setting restored; startup behavior setting (remember / on / off).
  - **macOS title bar** — restart-to-apply toggle.
  - **Dead-key input** — compose commits are delivered to text fields.
  - **Downgrade note** — recently-played history now stores tracks and albums; downgrading to 2.0.0 clears it on first save.

Full changelog: https://github.com/vicrodh/qbz/compare/v2.0.0...v2.0.1

---

# 2.0.0 — Rebuild 破 (You Can (Not) Advance)

It's been a while since the last release — in software terms that's not really that long, it's actually pretty normal, but it did break the weekly-release rhythm we had going.

It's been something like a month and a half of hard work — more than the initial launch, honestly — but I think it was worth it. I've dared to mark this as a major version bump: this is qbz 2.0.

QBZ was originally supposed to be a small project. I leaned hard into vibe coding, for better, but I also made some decisions based more on hype than on the future, and that caught up with me fast. I built it on Tauri, which I still think is an amazing tool — it brings the ease of web development to desktop apps, not as a WebApp like Electron, but it still depends on web engines underneath. Tauri is perfectly configurable to run well on any machine, but at some point I had a graphics settings section bigger than the audio settings section — absurd for an audio player.

That felt unacceptable, so I started a rebuild I'd basically had in mind since week 1 of QBZ's life. Since the 1.2.x releases I'd been quietly making backend changes to enable this migration. Well, here it is.

---

## The rewrite

  - **Frontend completely rebuilt** — considerably smaller CPU and memory footprint; memory now goes to playback, not to drawing the app.
  - **Wayland/X11/Metal, all the same** (Direct3D tested... spoiler alert?) — no more chasing the right configuration, the app handles it for you.
  - **macOS is no longer second-class** — thanks to [@afonsojramos](https://github.com/afonsojramos) and [@Vudgekek](https://github.com/Vudgekek), the macOS port is now solid and out of its experimental phase; QBZ is a proper Linux and Mac platform.
  - **Now also in Russian and Japanese** — the UI ships in 7 languages; thanks to [@mxnix](https://github.com/mxnix) for the Russian translation.
  - **Friendlier, more minimalist layouts** — focus on content and music instead of piling options on screen (try small mode with the sidebar fully closed, it's my favorite).

---

## Audio, playback & Qobuz Connect

  - **Playback engine hardening** — more than one problem fixed here; the engine is considerably more solid now (Pulse out of the box, Jack available).
  - **HiFi Wizard revamped** — one of QBZ's crown jewels got hardware auto-detection and simpler tests, without being intrusive; configuration is still yours to make, just easier to get there.
  - **Qobuz Connect, more stable** — still work to do for full 1:1 parity with the official clients, but noticeably closer.

---

## Search, lyrics & blocking

  - **New search overlay** — makes results more accessible, backed by a small cache layer that learns your preferences the more you use it, so it stops surfacing results you never touch.
  - **Faster lyrics** — now pulled straight from Qobuz since they offer them; the fallback to our previous lyrics engine is still there for when Qobuz doesn't have them.
  - **Granular blacklist** — block artists or albums individually, not just genres — goodbye to that reggaeton you don't like, that covers album, the AI slop, or the same-name artist sneaking into your results. Fully reversible and manageable.

---

## Offline & local music

  - **Login-free local music** — use QBZ for your local library without ever logging into Qobuz.
  - **Fully offline playlists** — create and manage playlists with zero Qobuz connection; offline mode overall is just more practical now.
  - **DSD file support** — because what's a HiFi player without DSD?

---

## Discovery & recommendations

  - **New recommendations tab** — Last.fm and ListenBrainz/MusicBrainz were already wired in for scrobbling but underused; connect them and get recommendations based on what you listen to, similarities, and local-listen vectorization. Logic, not AI. If you haven't connected these yet, do it.
  - **Better in-playlist recommendations** — improved vectors, disambiguation, and more.

---

## Immersive mode

  - **Trimmed, but for the better** — a couple of views were dropped, others added, and the ones that remain are no longer a resource drain; more and better full-screen views are coming.
  - **Search overlay now works inside immersive mode** — switch albums without leaving the view. The goal is something like the legendary Winamp and Windows Media Player visualizers of the 2000s — the ceiling will be an Xbox 360-style visualizer, haha.

---

There's a lot more under the hood that you probably won't notice visually, but you will in day-to-day use. I hope you enjoy this new version as much as I enjoyed working on it.

Full changelog: https://github.com/vicrodh/qbz/compare/v1.2.15...v2.0.0
