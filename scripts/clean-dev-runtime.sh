#!/usr/bin/env bash
set -euo pipefail

# Kill stale dev processes that commonly leave Vite/Svelte CSS pipeline in a bad state.
pkill -f '[n]ode .*vite dev' || true
pkill -f '[n]ode .*tauri dev' || true
pkill -f '[c]argo run' || true
pkill -f '[t]arget/debug/qbz' || true
pkill -f '[t]arget/debug/qbz-nix' || true

# Force-free dev ports in case a process survives pattern-based kill.
for port in 1420 1421; do
  pids="$(lsof -ti :"${port}" 2>/dev/null || true)"
  if [[ -n "${pids}" ]]; then
    kill ${pids} 2>/dev/null || true
    sleep 0.2
    # Escalate only if still alive after TERM.
    still_alive="$(lsof -ti :"${port}" 2>/dev/null || true)"
    if [[ -n "${still_alive}" ]]; then
      kill -9 ${still_alive} 2>/dev/null || true
    fi
  fi
done

# Clean generated frontend caches.
rm -rf .svelte-kit .vite node_modules/.vite build dist .cache

echo "[qbz] Dev runtime cleaned (processes + ports 1420/1421 + cache directories)."

# Run guardrail check so runtime clean does not hide code-level ghost triggers.
bash ./scripts/check-no-t-shadow.sh
