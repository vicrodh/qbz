---
name: Bug report
about: Report something that is not working as expected
title: "[Bug] "
labels: bug
assignees: ""
---

## Description

A clear and concise description of the problem.

## Steps to reproduce

1.
2.
3.

## Expected behavior

What you expected to happen.

## Actual behavior

What actually happened.

## Environment

- QBZ version:
- Installation method: <!-- AppImage / .deb / .rpm / AUR / Tarball / Flatpak / Built from source -->
- Distribution:
- Desktop environment / WM:
- Display server: <!-- Wayland / X11 -->
- GPU: <!-- e.g. NVIDIA RTX 3070, AMD RX 6600 XT, Intel Iris Xe -->
- Audio backend (PipeWire / ALSA Direct / PulseAudio):
- Audio device / DAC (if relevant):

## Logs

Logs help us diagnose problems much faster. To share them:

1. Open **Settings > Developer Mode > View Logs**
2. Click **Upload** on both the **Terminal** and **Console** tabs
3. Paste both URLs here:

- Terminal log URL:
- Console log URL:

<details>
<summary><strong>If QBZ crashes on startup</strong> (click to expand)</summary>

Run QBZ from a terminal to capture the output. Use the command that matches your installation:

```bash
# AppImage
./QBZ*.AppImage 2>&1 | tee /tmp/qbz-crash.log

# AUR / Tarball
qbz 2>&1 | tee /tmp/qbz-crash.log

# .deb / .rpm
qbz 2>&1 | tee /tmp/qbz-crash.log

# Flatpak
flatpak run com.blitzfc.qbz 2>&1 | tee /tmp/qbz-crash.log
```

Then upload the log and paste the URL here:

```bash
curl -F'file=@/tmp/qbz-crash.log' https://0x0.st
```

</details>

## Screenshots

If applicable, add screenshots to help illustrate the issue.

## Additional context

Anything else that may help reproduce or understand the issue.
