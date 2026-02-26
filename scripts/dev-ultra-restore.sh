#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

FULL=false
NO_START=false
RUN_CHECK=false

for arg in "$@"; do
  case "$arg" in
    --full|-f)
      FULL=true
      ;;
    --no-start)
      NO_START=true
      ;;
    --check)
      RUN_CHECK=true
      ;;
    *)
      echo "[qbz] Unknown option: ${arg}"
      echo "Usage: bash ./scripts/dev-ultra-restore.sh [--full|-f] [--no-start] [--check]"
      exit 1
      ;;
  esac
done

echo "[qbz] Ultra restore: stopping dev processes..."
pkill -f '[n]ode .*vite dev' || true
pkill -f '[n]ode .*tauri dev' || true
pkill -f '[c]argo run' || true
pkill -f '[t]arget/debug/qbz' || true
pkill -f '[t]arget/debug/qbz-nix' || true

echo "[qbz] Ultra restore: freeing dev ports..."
for port in 1420 1421; do
  pids="$(lsof -ti :"${port}" -sTCP:LISTEN 2>/dev/null || true)"
  if [[ -n "${pids}" ]]; then
    kill ${pids} 2>/dev/null || true
    sleep 0.5
    still_alive="$(lsof -ti :"${port}" -sTCP:LISTEN 2>/dev/null || true)"
    if [[ -n "${still_alive}" ]]; then
      kill -9 ${still_alive} 2>/dev/null || true
    fi
  fi
done

echo "[qbz] Ultra restore: clearing frontend caches..."
rm -rf \
  .svelte-kit \
  .vite \
  build \
  dist \
  .cache \
  node_modules/.vite \
  node_modules/.vite_temp \
  node_modules/.svelte \
  node_modules/.cache \
  node_modules/.postcss \
  tsconfig.tsbuildinfo \
  .eslintcache

echo "[qbz] Ultra restore: clearing temp vite directories..."
rm -rf /tmp/vite-* 2>/dev/null || true

if [[ "${FULL}" == "true" ]]; then
  echo "[qbz] Ultra restore: full mode enabled (Rust/Tauri clean)..."
  rm -rf src-tauri/target/debug/.fingerprint src-tauri/target/debug/incremental
  if command -v cargo >/dev/null 2>&1; then
    cargo clean --manifest-path src-tauri/Cargo.toml || true
  fi
fi

echo "[qbz] Ultra restore: running ghost guard checks..."
bash ./scripts/check-no-t-shadow.sh

if [[ "${RUN_CHECK}" == "true" ]]; then
  echo "[qbz] Ultra restore: running svelte-check..."
  npm run -s check
fi

if [[ "${NO_START}" == "true" ]]; then
  echo "[qbz] Ultra restore complete (no-start mode)."
  exit 0
fi

echo "[qbz] Ultra restore: starting tauri dev in foreground..."
exec npm run tauri dev
