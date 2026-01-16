// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Use xdg-desktop-portal for file dialogs on Linux
    // This makes GTK apps use native file pickers (e.g., KDE's on Plasma)
    #[cfg(target_os = "linux")]
    std::env::set_var("GTK_USE_PORTAL", "1");

    // Prefer a writable TMPDIR to avoid GTK pixbuf cache crashes on some systems.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("TMPDIR").is_none() {
            if let Some(cache_dir) = dirs::cache_dir() {
                let tmp_dir = cache_dir.join("qbz/tmp");
                if std::fs::create_dir_all(&tmp_dir).is_ok() {
                    std::env::set_var("TMPDIR", tmp_dir);
                }
            }
        }
    }

    // Wayland and WebKit compatibility fixes for Linux
    // Addresses: https://github.com/vicrodh/qbz/issues/6
    #[cfg(target_os = "linux")]
    {
        let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();

        // Only apply Wayland-specific fixes if running under Wayland
        // and the user hasn't explicitly set GDK_BACKEND
        if is_wayland && std::env::var_os("GDK_BACKEND").is_none() {
            // Force Wayland backend to avoid fallback issues
            std::env::set_var("GDK_BACKEND", "wayland");

            // Disable WebKit's compositing mode which can cause protocol errors
            // with transparent windows on Wayland
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");

            // Prefer client-side decorations (we use custom titlebar anyway)
            std::env::set_var("GTK_CSD", "1");
        }

        // Hardware acceleration fixes for WebKit on both X11 and Wayland
        // Helps with GBM buffer errors on some GPU drivers
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    qbz_nix_lib::run()
}
