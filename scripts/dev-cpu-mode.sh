#!/usr/bin/env bash
# Force QBZ to run under software/CPU rendering across every layer that
# can sneak the GPU back in. Useful for testing the degraded paint path
# (modal mount cost, Discovery V2 perf, etc.) without touching the
# persisted graphics_settings DB. Sister script of dev-ultra-restore.sh.
#
# Layers forced off / to SW:
#   QBZ_HARDWARE_ACCEL=0              — QBZ's own nuclear opt-out
#                                       (overrides graphics_settings DB)
#   WEBKIT_DISABLE_COMPOSITING_MODE=1 — WebKit redraws via SW compositor
#   WEBKIT_DISABLE_DMABUF_RENDERER=1  — no DMA-BUF GPU texture sharing
#   GSK_RENDERER=cairo                — GTK4 picks the cairo SW renderer
#                                       (Vulkan / GL paths disabled)
#   LIBGL_ALWAYS_SOFTWARE=1           — Mesa GL falls back to llvmpipe
#
# Usage:
#   bash ./scripts/dev-cpu-mode.sh           # kill stale + start tauri dev
#   bash ./scripts/dev-cpu-mode.sh --no-kill # skip process cleanup
#   bash ./scripts/dev-cpu-mode.sh --print   # print the env vars and exit
#                                              (for sourcing into another shell)

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

KILL_STALE=true
PRINT_ONLY=false

for arg in "$@"; do
  case "$arg" in
    --no-kill)
      KILL_STALE=false
      ;;
    --print)
      PRINT_ONLY=true
      ;;
    *)
      echo "[qbz] Unknown option: ${arg}"
      echo "Usage: bash ./scripts/dev-cpu-mode.sh [--no-kill] [--print]"
      exit 1
      ;;
  esac
done

export QBZ_HARDWARE_ACCEL=0
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export GSK_RENDERER=cairo
export LIBGL_ALWAYS_SOFTWARE=1

echo "[qbz] dev-cpu-mode: software-rendering env applied"
echo "[qbz]   QBZ_HARDWARE_ACCEL              = ${QBZ_HARDWARE_ACCEL}"
echo "[qbz]   WEBKIT_DISABLE_COMPOSITING_MODE = ${WEBKIT_DISABLE_COMPOSITING_MODE}"
echo "[qbz]   WEBKIT_DISABLE_DMABUF_RENDERER  = ${WEBKIT_DISABLE_DMABUF_RENDERER}"
echo "[qbz]   GSK_RENDERER                    = ${GSK_RENDERER}"
echo "[qbz]   LIBGL_ALWAYS_SOFTWARE           = ${LIBGL_ALWAYS_SOFTWARE}"

if [[ "${PRINT_ONLY}" == "true" ]]; then
  exit 0
fi

if [[ "${KILL_STALE}" == "true" ]]; then
  echo "[qbz] dev-cpu-mode: stopping stale dev processes..."
  pkill -f '[n]ode .*vite dev' || true
  pkill -f '[n]ode .*tauri dev' || true
  pkill -f '[c]argo run' || true
  pkill -f '[t]arget/debug/qbz' || true
  pkill -f '[t]arget/debug/qbz-nix' || true

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
fi

echo "[qbz] dev-cpu-mode: starting tauri dev in foreground..."
exec npm run tauri dev
