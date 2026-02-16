#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_FILE="$ROOT_DIR/src-tauri/src/commands_v2.rs"

if [[ ! -f "$TARGET_FILE" ]]; then
  echo "ERROR: File not found: $TARGET_FILE" >&2
  exit 2
fi

PATTERN='crate::commands::|crate::library::commands::|crate::cast::[^[:space:]]*::commands::|crate::offline_cache::commands::'

if rg -n "$PATTERN" "$TARGET_FILE"; then
  echo
  echo "FAIL: Forbidden legacy delegation found in commands_v2.rs"
  exit 1
fi

echo "OK: commands_v2.rs backend purity check passed"
