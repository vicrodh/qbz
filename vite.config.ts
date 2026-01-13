import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
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
    },
  },
})
