#!/usr/bin/env bash
# QBZ Slint — build with cargo, then run the BINARY DIRECTLY.
#
# Why this exists (vs slint-dev.sh which does `cargo run`): `cargo run` launches
# the app as a cargo-managed run target — the process inherits CARGO_* env and a
# cargo launch context, which KDE plasma-systemmonitor surfaces by labelling the
# RUNNING APP as "cargo" instead of "qbz-slint" (the kernel comm is still
# qbz-slint; it's only the monitor's display name). Running the prebuilt binary
# directly (no cargo wrapper) makes process monitors show it as `qbz-slint`,
# cleanly separate from the cargo/rustc BUILD processes.
#
# Same RELEASE build + flags as slint-dev.sh (the produced binary is identical);
# only the launch differs. Use slint-dev.sh for quick build+run iteration; use
# this when you want the running process to read as `qbz-slint` in the monitor.
#
# Usage: ./scripts/slint-run.sh [extra app args]
#        THREADS=16 ./scripts/slint-run.sh   # override frontend threads
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/.."
# Keep in sync with slint-dev.sh / .cargo/config.toml (RUSTFLAGS overrides, not
# merges, the config rustflags — so re-list the AES-NI/SSSE3 features).
export RUSTFLAGS="-C target-feature=+aes,+ssse3 -C link-arg=-fuse-ld=mold -Z threads=${THREADS:-16}"
cargo +nightly build --release --manifest-path crates/Cargo.toml -p qbz-slint
# exec the binary directly — no `cargo run`, so no CARGO_* env / cargo context,
# so the monitor shows `qbz-slint`.
exec crates/target/release/qbz-slint "$@"
