# QBZ Website

Marketing site for QBZ (native Qobuz client for Linux). Built with React + Vite and deployed as a static site to GitHub Pages.

## Features

- English and Spanish routes (`/` and `/es`)
- Dark and OLED themes via CSS variables
- Client-side GitHub Releases integration for latest downloads and changelog
- No analytics or tracking

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
