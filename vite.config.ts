import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    react(),
    // Sitemap is now a static file in public/sitemap.xml
  ],
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        changelog: resolve(__dirname, 'changelog/index.html'),
        licenses: resolve(__dirname, 'licenses/index.html'),
        qobuzLinux: resolve(__dirname, 'qobuz-linux/index.html'),
        es: resolve(__dirname, 'es/index.html'),
        esChangelog: resolve(__dirname, 'es/changelog/index.html'),
        esLicenses: resolve(__dirname, 'es/licenses/index.html'),
        esQobuzLinux: resolve(__dirname, 'es/qobuz-linux/index.html'),
      },
      output: {
        // Hashed filenames for long-term caching
        entryFileNames: 'assets/[name]-[hash].js',
        chunkFileNames: 'assets/[name]-[hash].js',
        assetFileNames: 'assets/[name]-[hash][extname]',
        // Manual chunk splitting for better caching
        manualChunks: {
          // React core - rarely changes, cache separately
          'vendor-react': ['react', 'react-dom'],
          // i18n - shared across pages
          'vendor-i18n': ['react-i18next', 'i18next'],
        },
      },
    },
    // Enable source maps for debugging
    sourcemap: false,
    // Optimize chunk size
    chunkSizeWarningLimit: 500,
    // Enable CSS code splitting
    cssCodeSplit: true,
  },
})

