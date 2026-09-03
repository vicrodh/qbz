# QBZ Website

Marketing site for QBZ (native hi-fi music player for Linux, macOS and Windows). Built with React + Vite and deployed as a static site to GitHub Pages.

## Features

- English and Spanish routes (`/` and `/es`)
- Dark and OLED themes via CSS variables
- Client-side GitHub Releases integration for latest downloads and changelog
- Dynamic install commands based on actual release filenames
- Dependency commands for Debian/Ubuntu and Fedora/RHEL
- Self-hosted fonts (Archivo + IBM Plex Mono, OFL); no third-party font requests
- Downloads laid out as a pyramid: Linux (primary) on top, macOS and Windows below
- Screenshot placeholders (`<Capture pending=… />`) for captures not yet taken
- No trackers beyond the existing GoatCounter pageview counter

## Download Section

The download section dynamically generates install commands based on actual release filenames from GitHub Releases:

- **Linux**: distro tabs (AUR, APT repo + .deb, .rpm, Flathub + .flatpak, Snap, AppImage, Nix flake, Gentoo overlay, tarball, qbzd tarball, source)
- **macOS**: Homebrew cask + signed DMGs (afonsojramos/qbz-macos), upstream ad-hoc DMGs behind a disclosure
- **Windows**: `.msi` assets from the release when present; otherwise the 2.1.0 notice. The as-is disclaimer is always shown.

Asset classification lives in `getType()` in `src/components/DownloadSection.tsx`:
`.sig`, `latest.json` and `*.app.tar.gz` are ignored; `qbzd-*.tar.gz` goes to the qbzd tab.

## Development

```bash
npm install
npm run dev
```

## Build

```bash
npm run build
```

The output is generated in `dist/` and is ready for GitHub Pages.

## Deployment

A GitHub Actions workflow builds the site on push to the `website` branch and publishes the output to the `gh-pages` branch.

## Third-party libraries

- React
- Vite
- TypeScript
- i18next
- react-i18next

## Notes

- The website pulls release assets from the GitHub Releases API for `vicrodh/qbz`.
- Production domain: https://qbz.lol
- QBZ is Linux-first. macOS is supported (community-signed builds); Windows ships as-is.
- Qobuz is a trademark of its respective owner. QBZ is not affiliated with Qobuz.
