# QBZ Website

Marketing website for [QBZ](https://github.com/vicrodh/qbz) - Native Hi-Fi Qobuz Client for Linux.

## Development

```bash
npm install
npm run dev
```

## Build

```bash
npm run build
npm run preview
```

## Languages

- English: `/`
- Spanish: `/es/`

## Deployment

This branch deploys automatically to GitHub Pages via GitHub Actions on push.

The built assets are served from the `gh-pages` branch (or via the workflow artifact).

## DNS Configuration (Cloudflare)

1. Add a CNAME record pointing your domain to `vicrodh.github.io`
2. Enable "Proxied" for SSL/TLS
3. Create a `public/CNAME` file with your custom domain
