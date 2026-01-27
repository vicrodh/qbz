import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import Sitemap from 'vite-plugin-sitemap'
import { resolve } from 'node:path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    react(),
    Sitemap({
      hostname: 'https://qbz.lol',
      changefreq: 'weekly',
      priority: 0.8,
      lastmod: new Date(),
      readable: true,
      generateRobotsTxt: false,
    }),
  ],
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        changelog: resolve(__dirname, 'changelog/index.html'),
        licenses: resolve(__dirname, 'licenses/index.html'),
        es: resolve(__dirname, 'es/index.html'),
        esChangelog: resolve(__dirname, 'es/changelog/index.html'),
        esLicenses: resolve(__dirname, 'es/licenses/index.html'),
      },
      output: {
        // Hashed filenames for long-term caching
        entryFileNames: 'assets/[name]-[hash].js',
        chunkFileNames: 'assets/[name]-[hash].js',
        assetFileNames: 'assets/[name]-[hash][extname]',
      },
    },
    // Enable source maps for debugging
    sourcemap: false,
    // Optimize chunk size
    chunkSizeWarningLimit: 500,
  },
})
