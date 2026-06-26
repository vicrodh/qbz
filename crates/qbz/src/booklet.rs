//! Album booklet download.
//!
//! Booklets (digital liner-notes PDFs) are an occasional Qobuz "goody" — not
//! every album has one. We no longer render them in-app: the in-app MuPDF reader
//! was heavy (~20 MB of static MuPDF linked into the binary, plus a slow C build)
//! and clumsy to read. Instead the album-header booklet button downloads the PDF
//! to a user-chosen location for their own viewer. The fetch uses the app's HTTP
//! client, so it works whether the goody URL is public or session-scoped.
//!
//! The `AlbumBookletModal.slint` reader UI + the `BookletState`/`BookletActions`
//! globals are now unused (left in place; remove in a UI cleanup pass that
//! recompiles qbz-ui).

use std::cell::RefCell;
use std::time::Duration;

use slint::ComponentHandle;

use crate::AppWindow;

/// PDF download timeout (matches the former in-app reader's client).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

thread_local! {
    /// Booklet goody URL of the currently-open album ("" = no booklet). Stashed
    /// by `album::apply_album`, cleared on album reset; only ever touched on the
    /// Slint event loop.
    static BOOKLET_URL: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Stash the booklet goody URL for the currently-open album.
pub fn set_current_url(url: &str) {
    BOOKLET_URL.with(|cell| *cell.borrow_mut() = url.to_string());
}

/// Clear the stashed booklet URL (album reset).
pub fn clear_current_url() {
    BOOKLET_URL.with(|cell| cell.borrow_mut().clear());
}

/// Download the current album's booklet PDF to a user-chosen location. No-op
/// when no booklet URL is stashed. Fetches with the app's HTTP client, then
/// opens a native save dialog seeded with the album title.
pub fn download_booklet(weak: slint::Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let url = BOOKLET_URL.with(|cell| cell.borrow().clone());
    if url.is_empty() {
        return;
    }
    let default_name = weak
        .upgrade()
        .map(|w| {
            let title = w.global::<crate::AlbumState>().get_title().to_string();
            if title.is_empty() {
                "booklet.pdf".to_string()
            } else {
                format!("{title}.pdf")
            }
        })
        .unwrap_or_else(|| "booklet.pdf".to_string());

    handle.spawn(async move {
        let client = match reqwest::Client::builder().timeout(DOWNLOAD_TIMEOUT).build() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("[qbz-slint] booklet HTTP client error: {e}");
                return;
            }
        };
        let resp = match client.get(&url).send().await.and_then(|r| r.error_for_status()) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[qbz-slint] booklet fetch failed: {e}");
                return;
            }
        };
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                log::warn!("[qbz-slint] booklet read failed: {e}");
                return;
            }
        };

        // Native "save as" dialog seeded with the album title; cancel = no-op.
        let Some(dest) = rfd::AsyncFileDialog::new()
            .set_file_name(&default_name)
            .add_filter("PDF", &["pdf"])
            .save_file()
            .await
        else {
            return;
        };
        if let Err(e) = tokio::fs::write(dest.path(), &bytes).await {
            log::warn!("[qbz-slint] booklet save failed: {e}");
        }
    });
}
