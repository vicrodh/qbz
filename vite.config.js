// @ts-nocheck - Node.js globals used in vite config
import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sveltekit()],
  define: {
    'import.meta.env.VITE_BUILD_DATE': JSON.stringify(
      new Date().toISOString().split('T')[0]
    ),
    // Immersive rendering feature flag (WebGL2-based effects)
    // Set QBZ_IMMERSIVE=false to compile out immersive rendering
    'import.meta.env.VITE_IMMERSIVE_ENABLED': JSON.stringify(
      process.env.QBZ_IMMERSIVE !== 'false'
    ),
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
