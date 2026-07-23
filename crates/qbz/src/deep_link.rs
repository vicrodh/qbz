//! XDG / launcher deep links — `xdg-open https://open.qobuz.com/album/<id>`
//! with the desktop files' `Exec=qbz %u` (Tauri parity: the old app scanned
//! argv in the single-instance plugin callback and on cold start; the Slint
//! rebuild kept the raise-half but dropped the URL-half, so a deep link
//! raised/focused the window without ever navigating).
//!
//! Two entry paths, one pending slot:
//!
//! - **Cold start:** `capture_argv()` at the top of `main()` stashes the
//!   first Qobuz-link argv entry BEFORE the single-instance guard runs (a
//!   second launch must read its own argv before deciding to raise+exit).
//!   Drained at the END of `enter_shell` — after the startup-page/view
//!   restore, so the restore can't re-root over the deep link. At that point
//!   the session is active and the AppWindow exists, so NO sleep hack (the
//!   Tauri 1500 ms delay is not ported). Sitting at the login screen (no
//!   session) the URL simply stays pending until the next successful
//!   `enter_shell`. Offline entry (`enter_shell_offline`) never binds the
//!   shell context, so the URL rides until an online shell — navigation
//!   needs the API (same limitation as the Tauri era).
//! - **Warm start:** the second launch forwards the URL over the
//!   single-instance D-Bus interface (`OpenUrl`, see `single_instance.rs`),
//!   which presents the running instance and drains through the same path.
//!
//! The dispatch itself is the EXISTING Ctrl+L machinery:
//! `link_resolver::resolve` → `apply_resolved_link` → `navigate_*`.

use std::sync::{Arc, Mutex};

use qbz_app::shell::AppRuntime;

use crate::adapter::SlintAdapter;
use crate::artwork;
use crate::AppWindow;

/// The first Qobuz link seen, waiting for a shell to navigate it. A warm
/// `OpenUrl` overwrites: the newest user intent wins.
static PENDING: Mutex<Option<String>> = Mutex::new(None);

/// Everything `dispatch` needs, bound once per shell entry (`enter_shell`)
/// and cleared on logout so "context set" means "a session is active and
/// navigation can succeed". The warm D-Bus path has no other way to reach
/// these — they only exist as locals in `main()`.
#[derive(Clone)]
struct ShellCtx {
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
}

static SHELL_CTX: Mutex<Option<ShellCtx>> = Mutex::new(None);

/// Whether a string looks like a Qobuz link (custom scheme or web URL) —
/// 1:1 the legacy Tauri `is_qobuz_link` prefixes
/// (`qbz-worktrees/legacy-tauri` `src-tauri/src/lib.rs:594`).
pub fn is_qobuz_link(arg: &str) -> bool {
    arg.starts_with("qobuzapp://")
        || arg.starts_with("https://play.qobuz.com/")
        || arg.starts_with("http://play.qobuz.com/")
        || arg.starts_with("https://open.qobuz.com/")
        || arg.starts_with("http://open.qobuz.com/")
}

/// The first Qobuz link in an argument list, if any (pure — unit-tested).
fn select_link(args: &[String]) -> Option<String> {
    args.iter().find(|a| is_qobuz_link(a)).cloned()
}

/// Scan the process argv for a Qobuz link and stash it pending. Call at the
/// top of `main()`, BEFORE `single_instance::acquire_or_raise`: when another
/// instance owns the bus name the guard forwards the stashed URL to it, and
/// when we are the primary it rides until the `enter_shell` drain.
pub fn capture_argv() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(url) = select_link(&args) {
        log::info!(
            "[qbz-slint] deep link: captured from argv: {}",
            url.split('?').next().unwrap_or(&url)
        );
        stash(url);
    }
}

/// Stash a URL pending (cold argv capture and the warm D-Bus `OpenUrl`).
pub fn stash(url: String) {
    if let Ok(mut guard) = PENDING.lock() {
        *guard = Some(url);
    }
}

/// Take the pending URL, leaving the slot empty.
pub fn take_pending() -> Option<String> {
    PENDING.lock().ok().and_then(|mut guard| guard.take())
}

/// Bind the shell context at `enter_shell` — the gate that lets
/// `drain_pending` dispatch (session active, AppWindow alive).
pub fn bind_shell_ctx(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: artwork::ImageCache,
) {
    if let Ok(mut guard) = SHELL_CTX.lock() {
        *guard = Some(ShellCtx {
            runtime,
            weak,
            handle,
            image_cache,
        });
    }
}

/// Clear the shell context on logout: back at the login screen a pending
/// URL must wait for the next `enter_shell`, not fire into a dead session.
pub fn clear_shell_ctx() {
    if let Ok(mut guard) = SHELL_CTX.lock() {
        *guard = None;
    }
}

/// Dispatch the pending URL through the existing Ctrl+L resolve flow, but
/// only when a shell is up. No-op otherwise — the URL stays pending for the
/// next successful `enter_shell`. Safe to call from any thread (the zbus
/// executor included): the resolve spawns on the stored tokio handle and
/// the navigation hops to the Slint event loop.
pub fn drain_pending() {
    let ctx = SHELL_CTX.lock().ok().and_then(|guard| guard.clone());
    let Some(ctx) = ctx else { return };
    let Some(url) = take_pending() else { return };
    dispatch(url, ctx);
}

/// Mirror of the Ctrl+L `LinkResolverActions::on_submit` flow in `main.rs`,
/// minus the modal state: resolve off the UI thread, then apply the
/// navigation on the event loop. Failures surface as a toast (there is no
/// modal to hold an error here).
fn dispatch(url: String, ctx: ShellCtx) {
    let ShellCtx {
        runtime,
        weak,
        handle,
        image_cache,
    } = ctx;
    log::info!(
        "[qbz-slint] deep link: resolving {}",
        url.split('?').next().unwrap_or(&url)
    );
    handle.clone().spawn(async move {
        let result = crate::link_resolver::resolve(runtime.clone(), url).await;
        let _ = weak.upgrade_in_event_loop(move |w| match result {
            Ok(qbz_music_link::MusicLinkResult::Resolved { link, .. }) => {
                crate::apply_resolved_link(link, &runtime, &w.as_weak(), &handle, &image_cache);
            }
            Ok(qbz_music_link::MusicLinkResult::PlaylistDetected { provider }) => {
                // Unreachable for the native Qobuz shapes the argv/D-Bus
                // matcher accepts (cross-platform provider playlists only).
                log::info!("[qbz-slint] deep link: {provider} playlist — nothing to navigate to");
            }
            Ok(qbz_music_link::MusicLinkResult::NotOnQobuz { .. }) => {
                crate::toast::error(
                    &w,
                    qbz_i18n::t("This content is not available on Qobuz"),
                );
            }
            Err(e) => {
                log::warn!("[qbz-slint] deep link: resolve failed: {e}");
                crate::toast::error(&w, qbz_i18n::t("Could not resolve that link"));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_open_qobuz_album_https_and_http() {
        assert!(is_qobuz_link("https://open.qobuz.com/album/kq3s910v1qufc"));
        assert!(is_qobuz_link("http://open.qobuz.com/album/kq3s910v1qufc"));
    }

    #[test]
    fn matches_play_qobuz_https_and_http() {
        assert!(is_qobuz_link("https://play.qobuz.com/album/kq3s910v1qufc"));
        assert!(is_qobuz_link("http://play.qobuz.com/track/123456"));
    }

    #[test]
    fn matches_qobuzapp_scheme() {
        assert!(is_qobuz_link("qobuzapp://album/kq3s910v1qufc"));
        assert!(is_qobuz_link("qobuzapp://artist/123"));
    }

    #[test]
    fn ignores_non_link_args() {
        assert!(!is_qobuz_link("--verbose"));
        assert!(!is_qobuz_link("/home/user/music.flac"));
        assert!(!is_qobuz_link("https://example.com/album/kq3s910v1qufc"));
        assert!(!is_qobuz_link("https://open.qobuz.com.evil.test/album/x"));
        // The dead `qbz://` scheme resolves nowhere and stays unmatched.
        assert!(!is_qobuz_link("qbz://album/kq3s910v1qufc"));
        assert!(!is_qobuz_link(""));
    }

    #[test]
    fn select_link_takes_first_match() {
        let args = vec![
            "--flag".to_string(),
            "https://open.qobuz.com/album/first".to_string(),
            "qobuzapp://album/second".to_string(),
        ];
        assert_eq!(
            select_link(&args),
            Some("https://open.qobuz.com/album/first".to_string())
        );
    }

    #[test]
    fn select_link_returns_none_without_match() {
        let args = vec!["--flag".to_string(), "cover.jpg".to_string()];
        assert_eq!(select_link(&args), None);
    }

    /// Single stateful test: PENDING is process-global and cargo runs tests
    /// on threads, so the round-trip + overwrite checks stay sequential here.
    #[test]
    fn pending_drains_once_and_newest_wins() {
        stash("qobuzapp://album/old".to_string());
        stash("qobuzapp://album/new".to_string());
        assert_eq!(take_pending(), Some("qobuzapp://album/new".to_string()));
        assert_eq!(take_pending(), None);
    }
}
