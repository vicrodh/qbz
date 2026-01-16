# Wayland Diagnostic Information

For users testing the Wayland compatibility fix (issue #6), please run these commands and share the output.

## Quick Diagnostic Script

Save this as `wayland-diag.sh` and run with `bash wayland-diag.sh`:

```bash
#!/bin/bash
echo "=== QBZ Wayland Diagnostic Information ==="
echo ""
echo "Date: $(date)"
echo "User: $USER"
echo ""

echo "--- System Information ---"
uname -a
echo ""

echo "--- Distribution ---"
cat /etc/os-release | grep -E "^(NAME|VERSION)="
echo ""

echo "--- Display Server ---"
echo "WAYLAND_DISPLAY=$WAYLAND_DISPLAY"
echo "XDG_SESSION_TYPE=$XDG_SESSION_TYPE"
echo "DISPLAY=$DISPLAY"
echo ""

echo "--- Desktop Environment ---"
echo "XDG_CURRENT_DESKTOP=$XDG_CURRENT_DESKTOP"
echo "XDG_SESSION_DESKTOP=$XDG_SESSION_DESKTOP"
loginctl show-session $(loginctl | grep $(whoami) | awk '{print $1}' | head -1) -p Type
echo ""

echo "--- Wayland Compositor ---"
ps aux | grep -E "wayland|sway|mutter|kwin" | grep -v grep | head -5
echo ""

echo "--- GPU Information ---"
lspci | grep -E "VGA|3D|Display"
echo ""

echo "--- GPU Driver ---"
glxinfo | grep -E "OpenGL (vendor|renderer|version)" 2>/dev/null || echo "glxinfo not available (install mesa-utils)"
echo ""

echo "--- Mesa Version ---"
glxinfo | grep "OpenGL version" 2>/dev/null || echo "glxinfo not available"
echo ""

echo "--- WebKitGTK Version ---"
pacman -Q webkit2gtk 2>/dev/null || \
dpkg -l | grep webkit2gtk 2>/dev/null || \
rpm -qa | grep webkit2gtk 2>/dev/null || \
echo "Package manager not detected or webkit2gtk not found"
echo ""

echo "--- GTK Version ---"
pacman -Q gtk3 gtk4 2>/dev/null || \
dpkg -l | grep "^ii.*gtk-[34]" 2>/dev/null || \
rpm -qa | grep "^gtk[34]" 2>/dev/null || \
echo "Package manager not detected"
echo ""

echo "--- GDK Backend Support ---"
echo "Checking for Wayland libraries..."
ldconfig -p | grep -E "wayland|gdk" | head -5
echo ""

echo "--- Relevant Environment Variables ---"
env | grep -E "WAYLAND|GDK|WEBKIT|GTK|DISPLAY|XDG" | sort
echo ""

echo "--- QBZ Test Run (with env vars visible) ---"
echo "Running: env | grep -E 'GDK|WEBKIT|WAYLAND' && qbz"
echo "Please paste the output from running QBZ here:"
echo ""
```

## Individual Commands

If the script doesn't work, run these commands individually:

### 1. Display Server Type
```bash
echo "Display Server: $XDG_SESSION_TYPE"
echo "Wayland Display: $WAYLAND_DISPLAY"
```

### 2. GPU and Driver
```bash
lspci | grep -E "VGA|3D"
glxinfo | grep "OpenGL renderer"
```

### 3. WebKitGTK Version
```bash
# Arch/Manjaro
pacman -Q webkit2gtk

# Debian/Ubuntu
dpkg -l | grep webkit2gtk

# Fedora/RHEL
rpm -qa | grep webkit2gtk
```

### 4. GTK Version
```bash
# Arch/Manjaro
pacman -Q gtk3 gtk4

# Debian/Ubuntu
dpkg -l | grep gtk-3

# Fedora/RHEL
rpm -qa | grep gtk3
```

### 5. Wayland Compositor
```bash
# Check what compositor is running
ps aux | grep -E "wayland|sway|mutter|kwin" | grep -v grep
```

### 6. Full Error Log
```bash
# Run QBZ with full logging
qbz 2>&1 | tee qbz-error.log
```

### 7. Environment Variables Check
```bash
# See what environment QBZ will inherit
env | grep -E "WAYLAND|GDK|WEBKIT|GTK|DISPLAY" | sort
```

## What to Include in Your Report

When reporting on issue #6, please include:

1. **Output from the diagnostic script above** (or individual commands)
2. **Full error log** from running QBZ
3. **Whether the fix worked** (app started successfully)
4. **Any workarounds** you found (e.g., specific env vars)

## Common Information Patterns

This helps identify if the issue is related to:
- **GPU vendor/driver**: AMD, Intel, NVIDIA proprietary, NVIDIA nouveau
- **Compositor**: GNOME (mutter), KDE (kwin_wayland), Sway, Hyprland
- **WebKitGTK version**: Older versions may have more Wayland issues
- **GTK version**: GTK3 vs GTK4 compatibility
- **Mesa version**: Older Mesa may have GBM buffer issues

## Privacy Note

The diagnostic script does not collect:
- Personal files or data
- Passwords or credentials
- Network activity
- Location information

It only collects system configuration relevant to display server and GPU setup.
