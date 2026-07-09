# 2.0.1 — __CODENAME__ (__SUBTITLE__)

<!-- OWNER: replace __CODENAME__ and __SUBTITLE__ before tagging (G3). The
     codename is yours to name; 2.0.0 was "Rebuild 破". This same codename must
     also be set in the About modal literal (crates/qbz-ui/ui/.../AboutModal.slint).
     This is the only piece of this release that was left blank on purpose. -->

The first maintenance release on top of the 2.0 native rebuild. It is mostly stability, performance and polish — the app scrolls lighter, recovers from bad states on its own, and reads and renders text more faithfully across languages.

It also lands a handful of things that missed the 2.0 cutoff: an automatic theme that follows your wallpaper or accent color, an inline custom-theme editor, a guest offline mode, and a Dutch translation.

Everything here is additive and safe to update into from 2.0.0. Bit-perfect playback is unchanged.

---

## Performance

  - **Viewport windowing everywhere** — Home, Discover, Favorites, Search, Labels and the shared carousels only build and decode the rows on screen, so long grids scroll without the memory and CPU climb.
  - **Steadier idle** — the equalizer bars and animated surfaces run off a coarse clock, keeping paused and idle CPU near zero.

---

## Rendering and resilience

  - **Autonomous renderer degradation** — if the GPU path stalls, the app steps down through the rendering ladder on its own and a UI-loop watchdog keeps the window responsive.
  - **Cleaner startup recovery** — a crash-chain watchdog resets a bad last-view and bypasses queue restore after repeated early deaths instead of looping.
  - **Single instance on Linux** — launching QBZ again focuses the running window rather than starting a second copy.
  - **Better hybrid-GPU defaults** — desktops with a discrete GPU pick the high-performance adapter by default.

---

## Text and languages

  - **Faithful CJK rendering** — Japanese and Korean glyphs render at every weight on macOS, and the first line of elided text is never culled.
  - **Cleaner API prose** — HTML entities (including malformed bare forms) are decoded in titles and descriptions.
  - **Lenient parsing** — one bad item in a list no longer blanks the whole list.
  - **Dutch** — the interface now ships in 8 languages.

---

## Themes

  - **Auto theme** — derive the whole palette from your desktop accent, your wallpaper, or an image you pick.
  - **Custom theme editor** — an inline editor with a color picker to build and save your own palette.

---

## Playback and audio

  - **Skips dead tracks** — unavailable tracks are skipped automatically instead of parking playback at 0:00.
  - **Errors you can see** — stream errors surface as toasts, sandbox-aware for ALSA under Flatpak and Snap.
  - **Casting fixes** — DLNA discovery, media serving and self-heal now work with strict renderers, without hoarding full-track buffers.
  - **Clean quit** — the forced PipeWire clock is released when the app exits.

---

## Interface

  - **Queue row actions** — "Remove all after" and "Add to playlist" in the per-track queue menu now work.
  - **Home, complete** — all 13 configurable sections render, with a Recently Played "View all" page and its own album history.
  - **Album favorites** — the album favorite control is a real toggle and stays in sync everywhere.
  - **Guest offline mode** — use your local library without signing in.
  - **Sort where you expect it** — the "Date added" playlist sort is back and the Library Tracks tab gained a sort control.
  - **Log sharing** — the log viewer opens expanded with Copy-all and Upload, plus an always-visible Share logs entry.

---

## Qobuz Connect

  - **Device name** — the QConnect device-name setting from the Tauri build is back.
  - **Startup behavior** — choose whether Connect remembers its last state, starts on, or stays off.

---

## Notes

  - **Downgrade caveat** — the recently-played history now stores tracks and albums; downgrading to 2.0.0 empties that history on its first save.

**Full changelog:** https://github.com/vicrodh/qbz/compare/v2.0.0...v2.0.1
