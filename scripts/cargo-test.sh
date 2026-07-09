#!/usr/bin/env bash
# Run the shipping-stack unit/doc tests (crates workspace).
#
# Excludes the Slint UI compile graph (qbz-ui + dependents). Building those for
# tests re-triggers the UI "memory wall" and is unnecessary: qbz-ui has no
# #[test]s, and the binary crate's tests need a full Slint link.
#
# Usage:
#   ./scripts/cargo-test.sh              # default: workspace minus UI graph
#   ./scripts/cargo-test.sh -- --lib     # extra args after -- go to cargo test
#   CARGO_BUILD_JOBS=1 ./scripts/cargo-test.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

exec cargo test \
  --manifest-path crates/Cargo.toml \
  --workspace \
  --exclude qbz \
  --exclude qbz-ui \
  --exclude qbz-dac-wizard \
  --exclude qbz-slint-common \
  --no-fail-fast \
  "$@"
