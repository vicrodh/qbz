import { defineConfig } from 'vite'
import { resolve } from 'path'

export default defineConfig({
  base: '/',
  publicDir: 'public',
  build: {
    outDir: 'dist',
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, 'index.html'),
        licenses: resolve(import.meta.dirname, 'licenses.html'),
        changelog: resolve(import.meta.dirname, 'changelog.html'),
        es: resolve(import.meta.dirname, 'es/index.html'),
        esLicenses: resolve(import.meta.dirname, 'es/licenses.html'),
        esChangelog: resolve(import.meta.dirname, 'es/changelog.html')
      }
    }
  }
})
