# QBZ 1.1.15

> **Largest feature release since 1.1.8.** Ten new user-facing features, a redesigned Label view, and several targeted fixes.

---

## New Features

### Purchases
Browse and manage your Qobuz purchases directly in the app. The Purchases view shows all purchased tracks and albums, tracks what has been downloaded locally, and lets you re-download anything from your purchase history. Download records are persisted in a local SQLite registry so they survive app restarts.

### Your Mixes
A new **Your Mixes** section on the Home view surfaces four Qobuz-curated personal radio feeds:
- **Daily Q** — fresh recommendations refreshed every day
- **Weekly Q** — a curated weekly playlist
- **Fav Q** — generated from your favorite artists
- **Top Q** — based on your most-played tracks

Each mix has its own full-screen view with artwork, queue controls, and the shuffle/search/filter controls you already know.

### Multi-select tracks
Every track list in the app now supports multi-select. Tap the checkbox icon in the section header to enter selection mode, check any number of tracks, then act on the whole batch at once:
- **Add to Queue** (at position or next)
- **Add to Playlist**
- **Add to / Remove from Favorites**
- **Remove from Playlist** (in playlist views)

Multi-select is available in: Album Detail, Artist Detail, Playlist Detail, Favorites Tracks, Label Popular Tracks, Daily Q, Weekly Q, Fav Q, and Top Q.

### Custom artist images
Right-click any artist photo anywhere in the app to replace it with an image of your choice. QBZ resizes and thumbnails the image automatically and persists the mapping per artist ID. Custom images show up consistently across the Home view, Search results, Favorites, Artist Detail, and Label pages without needing to re-assign them.

### Image lightbox
Click any album artwork or artist photo to open it full-screen. Press Escape or click outside to dismiss.

### Qobuz Radio
Start a Qobuz radio station directly from an artist or track. A submenu lists the available radio feeds for that artist or track and lets you pick one. The radio queue replaces the current queue and starts playing immediately.

### Session restore
QBZ now remembers where you were when you closed it and returns to the same view on launch. You can configure the startup page in **Settings → General** — options include Home, Library, Purchases, or the last view visited.

### Shareable and deep links
Paste a [Songlink/Odesli](https://odesli.co) URL or any streaming platform link into QBZ and it will resolve it to the Qobuz equivalent and navigate there directly. You can also share tracks outward: the track context menu now includes a **Share via Songlink** option that copies a cross-platform link to the clipboard.

### Font selector
Pick the UI font family in **Settings → Appearance**. The selection applies immediately across the whole interface.

### System theme (Auto-theme)
A new **System** theme option in Appearance generates a color scheme to match your desktop environment. On KDE and GNOME it reads the active accent color; on other DEs it extracts a palette from the current album artwork. The generated swatches are editable with a color picker if you want to fine-tune.

---

## Improvements

- **Label view redesigned** — completely rebuilt as a multi-section page: Popular Tracks (with playback controls), Releases carousel, Playlists, and Artists. The previous single-list layout is replaced.
- **Cache limits raised** — L1 cache increased to 400 MB, L2 to 800 MB, reducing re-fetches on large libraries.
- **UI scale options** — 85% and 95% added to the existing scale selector in Appearance settings.
- **PipeWire force bit-perfect** — New audio option to bypass the PipeWire volume mixer entirely and deliver the raw PCM stream directly to the hardware sink.
- **Custom artist images sync** — Images assigned in Artist Detail now appear immediately in Home, Search results, Favorites, and Label views without restarting.

---

## Bug Fixes

- **Multi-select bar overlapping tracks** — The bulk-action bar no longer covers the last visible track. It is positioned as a sticky footer below all track rows, and every view that contains tracks already has 100 px of bottom padding so no track is unreachable.
- **Scroll position bleeding between items** — Navigating back to an album/artist/playlist correctly restores *that item's* saved scroll position. Previously, all albums shared one saved position, so opening any album would jump to wherever you last were in a different album.
- **Scroll position TTL** — Saved positions now expire after 1 hour. Opening an album you visited days ago starts fresh at the top instead of teleporting to a stale offset.
- **PostCSS CSS-extraction errors (ADR-001)** — Several views introduced during this cycle used `t` as an arrow-function parameter (e.g. `.filter(t => ...)`), shadowing the svelte-i18n store and breaking Vite's CSS extraction pipeline. Fixed across AlbumDetailView, WeeklySuggestView, and related helpers.
