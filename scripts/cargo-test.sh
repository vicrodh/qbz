#!/usr/bin/env bash
# Run the shipping-stack unit/doc tests (crates workspace).
#
# Same command CI uses (.github/workflows/test-crates.yml). Excludes the Slint
# UI compile graph (qbz-ui + dependents) so a laptop / Actions runner never hits
# the 20–30 GB UI memory wall. qbz-ui has no #[test]s; the binary crate's tests
# need a full Slint link and stay out of the default suite.
#
# Usage:
#   ./scripts/cargo-test.sh
#   ./scripts/cargo-test.sh -- --lib          # skip doctests
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
