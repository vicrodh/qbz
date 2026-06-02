#!/usr/bin/env bash
# QBZ Slint — standard dev build/run.
#
# Builds + runs qbz-slint in RELEASE mode (release runtime performance is
# the whole reason we moved from Tauri to Slint — we always test at release
# perf so a regression never hides behind "it's just dev mode"), but uses
# the nightly rustc PARALLEL FRONTEND (-Z threads) so the serial compile of
# the giant Slint-generated module is split across cores. The produced
# binary is the SAME optimized release binary as `cargo build --release`;
# only compile time improves. We deliberately do NOT use cranelift — it
# would lower runtime performance.
#
# RUSTFLAGS must re-list the AES-NI/SSSE3 features from .cargo/config.toml,
# because setting RUSTFLAGS *overrides* (does not merge) the config's
# [target.'cfg(target_arch = "x86_64")'] rustflags. Keep these in sync with
# .cargo/config.toml (the AES-NI path keeps offline CMAF decrypt fast).
#
# Usage: ./scripts/slint-dev.sh [extra cargo args]
#        THREADS=16 ./scripts/slint-dev.sh   # override frontend threads
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/.."
# `mold` (faster linker) + more frontend threads only cut COMPILE time — they
# don't change the optimised binary, so release perf measurements stay valid.
export RUSTFLAGS="-C target-feature=+aes,+ssse3 -C link-arg=-fuse-ld=mold -Z threads=${THREADS:-16}"
exec cargo +nightly run --release --manifest-path crates/Cargo.toml -p qbz-slint "$@"
