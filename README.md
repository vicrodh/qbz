# QBZ Website

Marketing site for QBZ (native Qobuz client for Linux). Built with React + Vite and deployed as a static site to GitHub Pages.

## Features

- English and Spanish routes (`/` and `/es`)
- Dark and OLED themes via CSS variables
- Client-side GitHub Releases integration for latest downloads and changelog
- Dynamic install commands based on actual release filenames
- Dependency commands for Debian/Ubuntu and Fedora/RHEL
- Infinite carousel for key capabilities section
- No analytics or tracking

## Download Section

The download section dynamically generates install commands based on actual release filenames from GitHub Releases:

- **AUR (Arch)**: `yay -S qbz-bin`
- **AppImage**: Dynamic filename from release
- **Flatpak**: Dynamic filename from release
- **Debian/Ubuntu**: Shows dependency install command + dpkg install
- **Fedora/RHEL**: Shows dependency install command + rpm install
- **Tarball**: Dynamic filename from release

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
- QBZ is Linux-first. macOS builds are experimental and may be incomplete.
- Qobuz is a trademark of its respective owner. QBZ is not affiliated with Qobuz.
