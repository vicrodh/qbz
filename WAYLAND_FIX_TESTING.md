# Wayland Compatibility Fix - Testing Instructions

This branch contains fixes for issue #6: Wayland protocol errors and GBM buffer issues.

## What Was Fixed

The application was crashing on Wayland with:
```
Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.
```

And when forced to X11 (`GDK_BACKEND=x11 qbz`), the GUI was invisible with:
```
Failed to create GBM buffer of size 1280x800: Invalid argument
```

## Changes Made

Added environment variables in `src-tauri/src/main.rs` to improve Wayland/WebKit compatibility:

1. **Wayland Detection**: Automatically detects if running under Wayland via `WAYLAND_DISPLAY`
2. **Backend Forcing**: Sets `GDK_BACKEND=wayland` to prevent fallback issues
3. **Compositing Fix**: Sets `WEBKIT_DISABLE_COMPOSITING_MODE=1` to fix transparent window protocol errors
4. **DMA-BUF Renderer**: Disables `WEBKIT_DISABLE_DMABUF_RENDERER=1` to fix GPU driver compatibility issues
5. **Client-Side Decorations**: Sets `GTK_CSD=1` (we use custom titlebar anyway)

## Testing Requirements

### For Wayland Users (Primary Issue)

**Environment:**
- Wayland compositor (GNOME, KDE Plasma, Sway, etc.)
- Any GPU (issue reported on non-Intel systems)

**Test Steps:**
1. Build from this branch:
   ```bash
   cd /path/to/qbz-nix-worktrees/bugfix-waylandissues
   npm install
   npm run tauri build
   ```

2. Run the built binary:
   ```bash
   ./src-tauri/target/release/qbz
   ```

3. Expected behavior:
   - Application should start without protocol errors
   - GUI should be visible and responsive
   - No crash on launch

4. Check logs for any remaining errors:
   ```bash
   ./src-tauri/target/release/qbz 2>&1 | tee qbz-test.log
   ```

### For X11 Users (Regression Testing)

**Environment:**
- X11 session
- Any GPU

**Test Steps:**
1. Run the same binary from Wayland testing
2. Expected behavior:
   - Application should work normally
   - No regression from previous behavior

### Advanced Testing (Optional)

**Test with explicit backend forcing:**

```bash
# Force Wayland (should work now)
WAYLAND_DISPLAY=wayland-0 ./src-tauri/target/release/qbz

# Force X11 (should still work)
GDK_BACKEND=x11 ./src-tauri/target/release/qbz
```

**Test with custom env vars (override fix):**

```bash
# User can still override if needed
WEBKIT_DISABLE_COMPOSITING_MODE=0 ./src-tauri/target/release/qbz
```

## Known Limitations

1. These fixes apply automatically on Linux only
2. If a user has already set `GDK_BACKEND`, the app respects that choice
3. DMA-BUF renderer is disabled by default (slight performance trade-off for compatibility)

## Reporting Results

When reporting test results on issue #6, please include:

1. Distribution and version (e.g., "Arch Linux, kernel 6.18.5")
2. Desktop environment (e.g., "KDE Plasma 6.x on Wayland")
3. GPU and driver (e.g., "AMD RX 6800 with Mesa 24.x")
4. Whether the app started successfully
5. Any errors in the log output
6. Screenshots if GUI has issues

## Building AppImage/DEB/RPM for Testing

For users who prefer not to build from source:

```bash
# Build all Linux packages
npm run tauri build

# Packages will be in:
# - src-tauri/target/release/bundle/deb/
# - src-tauri/target/release/bundle/rpm/
# - src-tauri/target/release/bundle/appimage/
```

## Rollback Plan

If this fix causes regressions, users can:

1. Set `WEBKIT_DISABLE_DMABUF_RENDERER=0` to re-enable hardware acceleration
2. Use the previous version from the main branch
3. Report the regression on the issue tracker
