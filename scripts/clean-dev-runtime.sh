#!/usr/bin/env bash
set -euo pipefail

# Kill stale dev processes that commonly leave Vite/Svelte CSS pipeline in a bad state.
pkill -f '[n]ode .*vite dev' || true
pkill -f '[n]ode .*tauri dev' || true
pkill -f '[t]arget/debug/qbz-nix' || true

# Clean generated frontend caches.
rm -rf .svelte-kit .vite node_modules/.vite

echo "[qbz] Dev runtime cleaned (.svelte-kit/.vite + stale vite/tauri/qbz processes)."
