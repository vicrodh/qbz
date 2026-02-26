#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

NO_START=false
if [[ "${1:-}" == "--no-start" ]]; then
  NO_START=true
fi

echo "[qbz] Ghost doctor: checking for t-shadow and fragile i18n patterns..."
bash ./scripts/check-no-t-shadow.sh

echo "[qbz] Ghost doctor: cleaning runtime state..."
bash ./scripts/clean-dev-runtime.sh

if [[ "${NO_START}" == "true" ]]; then
  echo "[qbz] Ghost doctor complete (no-start mode)."
  exit 0
fi

echo "[qbz] Ghost doctor: starting tauri dev in foreground..."
exec npm run tauri dev
