# QBZ 1.1.15-rc-1 Beta Test Checklist

Thanks for helping validate this release candidate.

## Important context

- This RC includes significant backend changes.
- Please test your normal daily flows end-to-end, not only new features.
- Any regression report is valuable, even if it looks minor.

## Region note for Purchases

Qobuz purchases are not available in all countries (for example Mexico). If Purchases is unavailable in your region/account, report that as "not testable" and continue with the rest of the checklist.

## Purchases validation (if available on your account)

1. Open `Purchases` from sidebar.
2. Confirm albums and tracks load (no empty-error state).
3. Search by artist/album/track and confirm results update correctly.
4. Open album detail and confirm tracklist renders.
5. Verify format selector options are visible.
6. Download one track to a chosen folder.
7. Download full album to a chosen folder.
8. Restart app and verify downloaded state persists.
9. Confirm unavailable purchases show clear unavailable UI.

## Core regression checklist (all testers)

1. Login, restart app, and confirm session persistence.
2. Home loads correctly and navigation remains responsive.
3. Search (all tabs you normally use) returns expected results.
4. Playback basics: play/pause, next/prev, seekbar, queue advance.
5. Queue operations: add, reorder, shuffle, clear queue, history behavior.
6. Favorites: tracks/albums/artists/playlists (load and toggle).
7. Playlists: open, edit, reorder tracks, play all.
8. Local Library: browse albums/artists/tracks and play local content.
9. Settings panels: open each section and confirm no command-not-found errors.
10. Immersive mode: open, switch tabs, return to main player.

## Report template

Please include:

- OS + desktop environment
- QBZ version: `1.1.15-rc-1`
- Repro steps
- Expected vs actual behavior
- Console/terminal logs if available
- Screenshot/video if useful

