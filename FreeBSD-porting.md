# QBZ FreeBSD Porting Notes

This document records every change made to port QBZ from Linux to FreeBSD, the
rationale behind each decision, the options considered, and what functionality
was intentionally left as a no-op on FreeBSD.

---

## Background

QBZ is a native Qobuz streaming client built with:

- **Rust** backend (Tauri 2, axum, rodio, symphonia)
- **SvelteKit** frontend (served via WebView2/WebKitGTK)
- **GTK 3 + WebKitGTK 4.1** as the display stack

The codebase already used `#[cfg(target_os = "linux")]` throughout, so the
porting work was mostly a matter of extending guards, adding FreeBSD-specific
implementations, and patching third-party crates that had no FreeBSD path.

---

## 1. Audio Backend — OSS Direct (`qbz-audio`)

### What changed

Two new source files were added to `crates/qbz-audio/src/`:

- `oss_backend.rs` — implements `AudioBackend` for FreeBSD. Enumerates
  `/dev/dspN` devices by reading `/dev/sndstat`, discovers supported sample
  rates via the `SNDCTL_DSP_GETFMTS` / `SNDCTL_DSP_SPEED` ioctls, and
  implements `try_create_direct_stream()`.
- `oss_direct.rs` — implements the `DirectAudioStream` trait. Opens
  `/dev/dspN` directly, configures format/channels/rate via OSS ioctls
  (`SNDCTL_DSP_SETFMT`, `SNDCTL_DSP_CHANNELS`, `SNDCTL_DSP_SPEED`), and
  writes PCM data via a writer-thread that drains a `SourceQueue`.

`crates/qbz-audio/src/lib.rs` gates the modules:

```rust
#[cfg(target_os = "linux")]
pub mod alsa_backend;
#[cfg(target_os = "linux")]
pub mod alsa_direct;
#[cfg(target_os = "linux")]
pub mod pipewire_backend;
#[cfg(target_os = "linux")]
pub mod pulse_backend;

#[cfg(target_os = "freebsd")]
pub mod oss_backend;
#[cfg(target_os = "freebsd")]
pub mod oss_direct;
```

`crates/qbz-audio/src/backend.rs` was updated so that
`BackendManager::available_backends()` returns only `[AudioBackendType::Oss]`
on FreeBSD, and `create_backend()` dispatches to `OssBackend::new()`.

`crates/qbz-audio/Cargo.toml` already had `alsa` gated under
`[target.'cfg(target_os = "linux")'.dependencies]`; a new
`[target.'cfg(target_os = "freebsd")'.dependencies]` block was added for
`libc = "0.2"` (OSS ioctls).

### Why not CPAL/PipeWire on FreeBSD?

PipeWire does not exist on FreeBSD. CPAL's OSS backend exists but cannot do
exclusive / bit-perfect playback (no hardware bypass). The whole point of QBZ's
audio stack is bit-perfect delivery to DACs, so the only correct approach is
direct `/dev/dspN` writes, exactly as `alsa_direct.rs` does on Linux via
`hw:N,0`.

### Player wiring (`crates/qbz-player`)

`crates/qbz-player/src/player/mod.rs` was updated to dispatch to
`OssBackend::try_create_direct_stream()` on FreeBSD, mirroring the existing
Linux ALSA Direct path:

```rust
#[cfg(target_os = "freebsd")]
if backend_type == AudioBackendType::Oss {
    if let Some(oss_backend) = backend.as_any()
        .downcast_ref::<qbz_audio::oss_backend::OssBackend>() {
        if let Some(result) = oss_backend.try_create_direct_stream(&config) {
            return Some(result.map(|(stream, mode)| StreamType::Direct(...)));
        }
    }
}
```

---

## 2. UI — OSS Backend Selection (`src/lib/components/`)

### What changed

`DeviceDropdown.svelte` — added `'oss'` to the `backend` prop type and an OSS
grouping: "System Default" under "Defaults", hardware devices under "DSP
Devices (Bit-perfect)".

`SettingsView.svelte` — added `'Oss'` to `BackendInfo.backend_type`, wired
`loadBackendDevices` for OSS, added hardware-volume and exclusive-mode rules,
and added a `{:else if selectedBackend === 'OSS Direct'}` device dropdown block.

---

## 3. Machine ID (`src-tauri/src/credentials/mod.rs`)

### What changed

The credential encryption key is derived partly from a machine-unique ID.
On Linux this is read from `/etc/machine-id`. On FreeBSD that file does not
exist; the equivalent is the UUID generated at install time in the kernel:

```rust
#[cfg(target_os = "linux")]
if let Ok(id) = fs::read_to_string("/etc/machine-id") { ... }

#[cfg(target_os = "freebsd")]
if let Ok(output) = std::process::Command::new("sysctl")
    .args(["-n", "kern.hostuuid"]).output() { ... }
```

### Why `sysctl kern.hostuuid`?

It is the standard FreeBSD machine identity (written to NVRAM/disk at first
boot, stable across reboots, unique per machine). Using the `libc::sysctl`
API directly was considered but the command approach is simpler and the
overhead is negligible (runs once at startup).

---

## 4. Network Mount Detection (`src-tauri/src/network/mod.rs`)

### What changed

`parse_mount_info()` now dispatches at compile time:

```rust
fn parse_mount_info() -> Result<Vec<MountInfo>, String> {
    #[cfg(target_os = "linux")]   return parse_mount_info_linux();
    #[cfg(target_os = "freebsd")] return parse_mount_info_freebsd();
    #[cfg(not(any(...)))]         Ok(Vec::new())
}
```

The Linux implementation reads `/proc/self/mountinfo`. The FreeBSD
implementation calls `libc::getmntinfo(&mut mntbuf, libc::MNT_NOWAIT)` and
maps each `statfs` entry to a `MountInfo` struct.

FreeBSD filesystem type names were added to `classify_fs_type`:
- Virtual: `devfs`, `procfs`, `fdescfs`, `linprocfs`, `linsysfs`, `nullfs`
- Local: `ufs`, `zfs`, `msdosfs`, `cd9660`, `hammer`, `hammer2`
- Network: `nfs`, `smbfs`, `fusefs.smbnetfs`

---

## 5. Graphics Auto-Config (`src-tauri/src/lib.rs`)

### What changed

The `autoconfig_graphics` module (which reads `/sys/class/dmi/id/`,
`/proc/modules`, `/sys/module/nvidia`, etc.) is entirely Linux-specific.

```rust
#[cfg(target_os = "linux")]
pub mod autoconfig_graphics;
```

The call site in `lib.rs`:

```rust
#[cfg(target_os = "linux")]
match flatpak::migrate_app_id_data() { ... }
```

The CLI flags `--autoconfig-graphics`, `--reset-graphics`, `--reset-dmabuf`
in `main.rs` are all wrapped in `#[cfg(target_os = "linux")]`.

### What is lost

The graphics auto-configuration tool is not available on FreeBSD. This is
intentional: the tool detects NVIDIA/AMD/Intel GPUs and Wayland/X11 to
recommend WebKit rendering flags. FreeBSD uses X11 with GTK and does not have
the same GPU driver module structure.

---

## 6. Media Controls — MPRIS stub (`src-tauri/src/media_controls/mod.rs`)

### What changed

The `MediaControlsManager` has two compile-time variants:

- **Linux**: Full souvlaki/MPRIS implementation — registers a D-Bus
  `org.mpris.MediaPlayer2` service, handles play/pause/next/prev/seek events,
  updates Now Playing metadata.
- **FreeBSD (and all non-Linux)**: All methods are no-ops. The struct exists
  and compiles, but nothing is registered on D-Bus.

```rust
#[cfg(target_os = "linux")]
use souvlaki::{MediaControls, MediaMetadata, ...};
```

`souvlaki` was moved from general `[dependencies]` to
`[target.'cfg(target_os = "linux")'.dependencies]` to avoid compiling it
(and its D-Bus platform glue) on FreeBSD.

### What is lost

- **MPRIS**: Desktop environments on FreeBSD (e.g. KDE Plasma, XFCE) expose
  MPRIS-compatible media control widgets. These will not integrate with QBZ.
- **Media keys**: Hardware media keys (via MPRIS) will not work.

### Why not implement it?

souvlaki's `PlatformConfig` on Linux takes a `dbus_name: &str` field that does
not exist in its stub for other platforms. FreeBSD does have D-Bus and zbus
works on it (proven by single-instance). A full MPRIS implementation for
FreeBSD is possible and would be a good future contribution to souvlaki
upstream.

---

## 7. Desktop Notifications — ashpd stub (`src-tauri/src/commands_v2.rs`)

### What changed

`ashpd` (Freedesktop portal notifications) is only available on Linux:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
ashpd = { version = "0.13", ... }
```

All `ashpd` import and usage sites are wrapped in `#[cfg(target_os = "linux")]`.
The `v2_show_track_notification` command has a Linux branch (full portal
notification with artwork) and a non-Linux branch that logs a debug message.

### What is lost

Track-change desktop notifications (the OS-level pop-up with album art that
appears when a new track starts). On FreeBSD, notifications are silently
dropped.

### Why not implement it?

The `notify-rust` crate or direct D-Bus notifications could be used as a
FreeBSD alternative. Not implemented in this port but straightforward to add.

---

## 8. Single-Instance Enforcement (`vendor/tauri-plugin-single-instance/`)

### What changed

`tauri-plugin-single-instance` had only Linux and macOS platform
implementations. A FreeBSD implementation was added by copying the Linux
D-Bus implementation verbatim:

- `src/platform_impl/freebsd.rs` — identical to `linux.rs` (zbus D-Bus)
- `src/lib.rs` — added `#[cfg(target_os = "freebsd")] #[path = "platform_impl/freebsd.rs"] mod platform_impl;`
- `Cargo.toml` — added `[target.'cfg(target_os = "freebsd")'.dependencies.zbus] version = "5.9"`

### Why D-Bus on FreeBSD?

D-Bus is available on FreeBSD (`pkg install dbus`) and is commonly used by
GTK desktop environments. The Linux implementation already uses zbus (pure-Rust
D-Bus), so reusing it for FreeBSD requires no new dependencies.

**Prerequisite**: `dbus` must be running (`service dbus start`, or
`dbus_enable="YES"` in `/etc/rc.conf`).

---

## 9. Vendored Crate Patches

Three crates were vendored into `src-tauri/vendor/` and patched because they
lacked FreeBSD platform implementations.

### 9.1 `muda` (GTK menu bar)

`muda` is Tauri's cross-platform menu crate. Its GTK implementation was gated
exclusively on Linux:

```rust
// Before
#[cfg(all(target_os = "linux", feature = "gtk"))]

// After (all occurrences across all .rs files)
#[cfg(all(any(target_os = "linux", target_os = "freebsd"), feature = "gtk"))]
```

`Cargo.toml` GTK dependency was similarly extended from
`cfg(target_os = "linux")` to `cfg(any(target_os = "linux", target_os = "freebsd"))`.

Fixed with `sed` across all source files:

```sh
sed -i '' 's/cfg(all(target_os = "linux", feature = "gtk"))/cfg(all(any(target_os = "linux", target_os = "freebsd"), feature = "gtk"))/g' src/**/*.rs
```

### 9.2 `tray-icon` (system tray)

`tray-icon` dispatches to platform-specific implementations. FreeBSD had no
entry:

```rust
// Before
#[cfg(target_os = "linux")]
#[path = "gtk/mod.rs"]
mod platform;

// After
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[path = "gtk/mod.rs"]
mod platform;
```

Additional changes:
- `src/error.rs`: `PngEncodingError` variant extended to include `freebsd`
- `src/lib.rs`: `set_temp_dir_path()` guard extended to include `freebsd`
- `Cargo.toml`: `dirs`, `libappindicator`, `png` deps extended from
  `cfg(target_os = "linux")` to `cfg(any(target_os = "linux", target_os = "freebsd"))`

### 9.3 `tauri-plugin-single-instance`

Described in §8 above.

### Patch registration in `Cargo.toml`

```toml
[patch.crates-io]
tauri-plugin-single-instance = { path = "vendor/tauri-plugin-single-instance" }
muda                          = { path = "vendor/muda" }
tray-icon                     = { path = "vendor/tray-icon" }
```

---

## 10. ALSA-specific function stubs (`src-tauri/src/`)

Several ALSA-only functions from `qbz-audio` are re-exported via
`pub use qbz_audio::*` and called at the Tauri command layer without platform
guards. Three issues were fixed:

### `normalize_device_id_to_stable`

On Linux, this converts ephemeral `hw:3,0` ALSA device IDs to stable
`front:CARD=name,DEV=0` form so saved settings survive reboots and USB
reconnections. On FreeBSD, OSS device paths (`/dev/dsp0`) are already stable.
A pass-through stub was added to `src/audio/mod.rs`:

```rust
#[cfg(not(target_os = "linux"))]
pub fn normalize_device_id_to_stable(device_id: &str) -> String {
    device_id.to_string()
}
```

### HiFi Wizard rate detection

The wizard detects DAC-supported sample rates via PipeWire sink → ALSA card
mapping (`/proc/asound/cardN/stream0`). Wrapped in `#[cfg(target_os = "linux")]`
with a `log::debug!` no-op on FreeBSD. On FreeBSD the OSS backend queries
supported rates directly via ioctls at stream-open time.

### Smart quality downgrade

Before downloading UltraHiRes, the Linux path checks if the ALSA device
supports the track's sample rate and downgrades to HiRes if not. This check
uses `qbz_audio::device_supports_sample_rate` which reads `/proc/asound`. The
entire block is gated `#[cfg(target_os = "linux")]` on FreeBSD (no downgrade
— OSS kernel SRC handles rate mismatches transparently if needed).

---

## 11. HTTP Client — TLS backend (`src-tauri/src/commands_v2.rs`)

### What changed

The audio download client was originally built with `.use_native_tls()`. On
FreeBSD, `native-tls` uses OpenSSL, which had an issue negotiating HTTP/2 with
the Qobuz CDN — connections dropped after 1 byte with
`end of file before message length reached`.

The fix: use rustls (already the app's global TLS provider via `aws-lc-rs`):

```rust
// Before
let client = reqwest::Client::builder()
    .use_native_tls()
    .build()?;

// After
let client = reqwest::Client::builder()
    .use_rustls_tls()
    .build()?;
```

This also allows HTTP/2 to be negotiated via ALPN where the CDN supports it,
which is more correct. The rest of the app already used rustls everywhere;
this was an inconsistency in the original code.

---

## 12. Build toolchain — `tauri-cli` unavailable on FreeBSD

### Problem

`@tauri-apps/cli` (the npm package) ships pre-compiled native binaries. There
is no `@tauri-apps/cli-freebsd-x64` package, so `npm run tauri dev` fails.

`cargo install tauri-cli` also fails because `cargo-mobile2` (a Tauri CLI
dependency for iOS/Android support) explicitly does not support FreeBSD.

### Workaround

Run the frontend and backend separately:

```sh
# Terminal 1: Vite dev server (frontend)
npm run dev

# Terminal 2: Rust binary (connects to devUrl: http://localhost:1420)
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:$PKG_CONFIG_PATH \
  cargo run --manifest-path src-tauri/Cargo.toml
```

For a production binary:

```sh
npm run build   # SvelteKit → ../build/
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:$PKG_CONFIG_PATH \
  cargo build --manifest-path src-tauri/Cargo.toml --release
# Binary: src-tauri/target/release/qbz
```

### PKG_CONFIG_PATH

WebKitGTK 4.1 installs its `.pc` files to `/usr/local/lib/pkgconfig` which is
not in the default `PKG_CONFIG_PATH` on FreeBSD. Add to `~/.profile`:

```sh
export PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:$PKG_CONFIG_PATH
```

---

## 13. Shell Scripts — bash shebang

Several build scripts used `#!/bin/bash`. On FreeBSD, bash is at
`/usr/local/bin/bash` and `/bin/bash` does not exist. Fixed to use the
portable form:

```sh
#!/usr/bin/env bash
```

Affected: `scripts/dev-with-env.sh`, `scripts/monitor-resources.sh`,
`scripts/build-and-run.sh`.

---

## 14. Required FreeBSD Packages

```sh
pkg install \
  webkit2-gtk3          # WebKitGTK 4.1 (WebView)
  gtk3                  # GTK 3
  dbus                  # D-Bus (single-instance, tray)
  libayatana-appindicator  # System tray icon (StatusNotifierItem)
  harfbuzz              # Text shaping (mupdf sys-lib)
  freetype2             # Font rendering (mupdf sys-lib)
  llvm                  # libclang for mupdf bindgen
```

Enable D-Bus at boot:
```sh
echo 'dbus_enable="YES"' >> /etc/rc.conf
service dbus start
```

---

## Summary: What Works on FreeBSD

| Feature | Status |
|---|---|
| Login, browse, search | ✅ Full |
| Audio playback (OSS Direct) | ✅ Full bit-perfect |
| Gapless playback | ✅ Full |
| Queue, repeat, shuffle | ✅ Full |
| Offline cache / downloads | ✅ Full |
| Settings UI (OSS device selection) | ✅ Full |
| Qobuz Connect | ✅ Full |
| System tray | ✅ Full (GTK/StatusNotifierItem) |
| Single-instance enforcement | ✅ Via D-Bus |
| HiFi Wizard | ⚠️ No DAC rate detection (OSS ioctls handle it at stream time) |
| MPRIS / media keys | ❌ No-op stub |
| Desktop notifications | ❌ No-op stub |
| `--autoconfig-graphics` CLI | ❌ Linux-only |
| `--reset-graphics` / `--reset-dmabuf` | ❌ Linux-only |

---

## Did We Also Port Tauri?

Partially, yes. Tauri's core (`tauri`, `wry`, `tao`) already had FreeBSD in
their `cfg` guards — the Tauri team had done that groundwork. What was missing
was the **ecosystem layer** around Tauri:

| Crate | Status before | What we did |
|---|---|---|
| `tauri` | ✅ FreeBSD supported | Nothing |
| `wry` (WebView) | ✅ FreeBSD supported | Nothing |
| `tao` (windowing) | ✅ FreeBSD supported | Nothing |
| `muda` (menus) | ❌ Linux-only GTK guard | Extended all guards to include FreeBSD |
| `tray-icon` (tray) | ❌ No FreeBSD platform path | Added FreeBSD → GTK dispatch; extended error/dep guards |
| `tauri-plugin-single-instance` | ❌ No FreeBSD impl | Added FreeBSD D-Bus impl (copy of Linux) |

The three patches to `muda`, `tray-icon`, and `tauri-plugin-single-instance`
are self-contained and correct. They could each be submitted as upstream pull
requests — the changes are minimal (a few `cfg` guard extensions) and follow
exactly the same pattern already used for Linux. If accepted upstream, future
Tauri apps would get FreeBSD support for menus, tray icons, and
single-instance enforcement for free.
