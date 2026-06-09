//! Page navigation history — a browser-like back/forward stack.
//!
//! The shell records a [`NavEntry`] on every fresh navigation; the
//! `[<] [>]` button pair (and the mouse back/forward buttons) walk the
//! stack. UI thread only, hence `thread_local`.
//!
//! Scroll-position restoration: each entry remembers the viewport-y of the
//! scroll container that was showing it. The mounted view continuously
//! reports its live scroll via [`set_live_scroll`] (a NavState callback), so
//! every navigation — fresh [`record`] or [`go_back`]/[`go_forward`] — can
//! stamp the outgoing entry without touching the ~30 `record` call sites.
//! `go_back`/`go_forward` hand the restored scroll back to the shell, which
//! arms `NavState.restore-scope` + `scroll-restore`; the destination's scroll
//! container picks it up once its content has laid out.

use std::cell::{Cell, RefCell};

/// One navigable destination.
#[derive(Clone, Debug, PartialEq)]
pub enum NavEntry {
    Home,
    /// A Discover tab page ("home" | "editorPicks" | "forYou"). Each
    /// tab is its own history entry so back/forward moves between the
    /// three Discover pages.
    Discover {
        tab: String,
    },
    /// A Library > Favorites tab page ("tracks" | "albums" | "artists" |
    /// "playlists" | "labels"). Each tab is its own history entry so
    /// back/forward moves between the favorites pages, mirroring Discover.
    Favorites {
        tab: String,
    },
    /// A Local Library browse tab page ("tracks" | "folders" | "albums" |
    /// "artists"). Each tab is its own history entry so back/forward moves
    /// between the Local Library tabs, mirroring Favorites / Discover.
    LocalLibrary {
        tab: String,
    },
    /// A Discover "View all" full-list page — one album module
    /// (new releases, qobuzissimes, ...) opened from a Carousel's
    /// "View all" link. Carries the /discover/<x> endpoint + the
    /// section title used as the page heading.
    DiscoverBrowse {
        endpoint: String,
        title: String,
    },
    /// A Qobuz mix detail page ("daily" | "weekly" | "fav" | "top").
    Mix {
        kind: String,
    },
    /// A playlist detail page; the string is the playlist id.
    Playlist(String),
    /// The Playlist Manager — the full playlist + folder organization
    /// surface (Tauri's PlaylistManagerView). Toolbar state
    /// (filter/sort/view/folder-mode) is session-scoped in the
    /// controller, so the entry carries no payload.
    PlaylistManager,
    /// The Offline Cache Manager — the manage-downloads surface (Tauri's
    /// OfflineCacheManagerView). Session-scoped; no payload.
    OfflineManager,
    /// The My QBZ > Mixtapes index grid (read-only in this slice). Toolbar
    /// state (sort/view/search) is session-scoped in the controller, so the
    /// entry carries no payload.
    Mixtapes,
    /// The My QBZ > Collections index grid (read-only in this slice). Same
    /// session-scoped toolbar; no payload.
    Collections,
    Album(String),
    /// A Local Library album detail page (dedicated view, separate from the
    /// Qobuz Album view); the string is the metadata group key.
    LocalAlbum(String),
    Artist(String),
    Settings,
    /// A search results page; the string is the query.
    Search(String),
    /// MusicianPageView — opened by the artist network sidebar when
    /// the resolved musician does not have a confident Qobuz artist
    /// match. Carries the musician name + the role for the
    /// appearances query.
    Musician {
        name: String,
        role: String,
    },
    /// LabelView landing — the rich label page (header + popular tracks +
    /// releases / critics / playlists / artists / more-labels carousels).
    /// Reached by clicking a label anywhere. Carries the id + name fallback.
    Label {
        id: u64,
        name: String,
    },
    /// LabelReleasesView — the "See all releases" sub-view reached from the
    /// landing's Releases carousel. Carries the label id + name fallback.
    LabelReleases {
        id: u64,
        name: String,
    },
    /// ArtistsByLocationView — opened by the Origin section's
    /// location link. Carries the full scene-discovery payload.
    Location {
        mbid: String,
        area_id: String,
        area_name: String,
        country: String,
        genres: Vec<String>,
        tags: Vec<String>,
    },
}

/// One slot in the history stack: where we went, and how far it was
/// scrolled when we last left it.
#[derive(Clone, Debug)]
struct Entry {
    nav: NavEntry,
    /// Saved Flickable `viewport-y` (logical px; 0 at top, negative when
    /// scrolled down — Slint's convention).
    scroll: f32,
}

struct History {
    entries: Vec<Entry>,
    /// Index of the entry currently shown.
    cursor: usize,
}

thread_local! {
    static HISTORY: RefCell<History> = RefCell::new(History {
        entries: vec![Entry { nav: NavEntry::Home, scroll: 0.0 }],
        cursor: 0,
    });
    /// Live `viewport-y` of the scroll container currently on screen, kept
    /// fresh by the mounted view via [`set_live_scroll`]. Read when leaving
    /// a page so its entry can be stamped without per-call-site plumbing.
    static LIVE_SCROLL: Cell<f32> = const { Cell::new(0.0) };
}

/// Record the on-screen scroll container's current `viewport-y`. Wired to
/// `NavState.report-scroll`, fired from the view's `changed viewport-y`.
pub fn set_live_scroll(y: f32) {
    LIVE_SCROLL.with(|s| s.set(y));
}

fn live_scroll() -> f32 {
    LIVE_SCROLL.with(|s| s.get())
}

/// Push a Search history entry, OR replace the cursor entry in place
/// when it is already a Search. Used by the live-search debounce so
/// quick keystrokes do not push one entry per character, while still
/// keeping the page reachable via back/forward at the final query.
pub fn push_or_replace_search(query: String) {
    HISTORY.with(|h| {
        let h = &mut *h.borrow_mut();
        match h.entries.get(h.cursor).map(|e| &e.nav) {
            Some(NavEntry::Search(_)) => {
                // Replace in place: same Search page, keep its scroll.
                let scroll = h.entries[h.cursor].scroll;
                h.entries.truncate(h.cursor + 1);
                h.entries[h.cursor] = Entry {
                    nav: NavEntry::Search(query),
                    scroll,
                };
            }
            _ => {
                if let Some(cur) = h.entries.get_mut(h.cursor) {
                    cur.scroll = live_scroll();
                }
                h.entries.truncate(h.cursor + 1);
                h.entries.push(Entry {
                    nav: NavEntry::Search(query),
                    scroll: 0.0,
                });
                h.cursor = h.entries.len() - 1;
            }
        }
    });
}

/// Record a fresh forward navigation, dropping any forward history. A
/// no-op when the destination already is the current entry, so repeated
/// clicks on the same page do not pile up.
pub fn record(entry: NavEntry) {
    let pushed = HISTORY.with(|h| {
        let h = &mut *h.borrow_mut();
        if h.entries.get(h.cursor).map(|e| &e.nav) == Some(&entry) {
            return false;
        }
        // Stamp the page we are leaving with its live scroll position.
        if let Some(cur) = h.entries.get_mut(h.cursor) {
            cur.scroll = live_scroll();
        }
        h.entries.truncate(h.cursor + 1);
        h.entries.push(Entry {
            nav: entry,
            scroll: 0.0,
        });
        h.cursor = h.entries.len() - 1;
        true
    });
    // A fresh page starts at the top; the new view will report its own
    // scroll as the user moves it.
    if pushed {
        set_live_scroll(0.0);
    }
}

/// Step back; returns the entry that is now current plus its saved scroll
/// position, or `None` at the start of the stack.
pub fn go_back() -> Option<(NavEntry, f32)> {
    let res = HISTORY.with(|h| {
        let h = &mut *h.borrow_mut();
        if h.cursor == 0 {
            return None;
        }
        // Stamp the page we are leaving before stepping away.
        if let Some(cur) = h.entries.get_mut(h.cursor) {
            cur.scroll = live_scroll();
        }
        h.cursor -= 1;
        h.entries.get(h.cursor).map(|e| (e.nav.clone(), e.scroll))
    });
    if let Some((_, scroll)) = &res {
        set_live_scroll(*scroll);
    }
    res
}

/// Step forward; returns the entry that is now current plus its saved scroll
/// position, or `None` at the end of the stack.
pub fn go_forward() -> Option<(NavEntry, f32)> {
    let res = HISTORY.with(|h| {
        let h = &mut *h.borrow_mut();
        if h.cursor + 1 >= h.entries.len() {
            return None;
        }
        if let Some(cur) = h.entries.get_mut(h.cursor) {
            cur.scroll = live_scroll();
        }
        h.cursor += 1;
        h.entries.get(h.cursor).map(|e| (e.nav.clone(), e.scroll))
    });
    if let Some((_, scroll)) = &res {
        set_live_scroll(*scroll);
    }
    res
}

/// Whether a back step is available.
pub fn can_back() -> bool {
    HISTORY.with(|h| h.borrow().cursor > 0)
}

/// Whether a forward step is available.
pub fn can_forward() -> bool {
    HISTORY.with(|h| {
        let h = h.borrow();
        h.cursor + 1 < h.entries.len()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        HISTORY.with(|h| {
            *h.borrow_mut() = History {
                entries: vec![Entry {
                    nav: NavEntry::Home,
                    scroll: 0.0,
                }],
                cursor: 0,
            };
        });
        set_live_scroll(0.0);
    }

    /// Drop the scroll component for the assertions that only care about the
    /// destination page.
    fn nav_of(res: Option<(NavEntry, f32)>) -> Option<NavEntry> {
        res.map(|(e, _)| e)
    }

    #[test]
    fn record_then_back_and_forward() {
        reset();
        assert!(!can_back());
        record(NavEntry::Album("1".into()));
        record(NavEntry::Artist("2".into()));
        assert!(can_back());
        assert!(!can_forward());
        assert_eq!(nav_of(go_back()), Some(NavEntry::Album("1".into())));
        assert_eq!(nav_of(go_back()), Some(NavEntry::Home));
        assert_eq!(nav_of(go_back()), None);
        assert_eq!(nav_of(go_forward()), Some(NavEntry::Album("1".into())));
    }

    #[test]
    fn record_truncates_forward_history() {
        reset();
        record(NavEntry::Album("1".into()));
        record(NavEntry::Album("2".into()));
        go_back();
        record(NavEntry::Artist("3".into()));
        assert!(!can_forward());
        assert_eq!(nav_of(go_back()), Some(NavEntry::Album("1".into())));
    }

    #[test]
    fn search_entry_round_trips_history() {
        reset();
        record(NavEntry::Search("metallica".into()));
        record(NavEntry::Album("5".into()));
        assert_eq!(
            nav_of(go_back()),
            Some(NavEntry::Search("metallica".into()))
        );
        assert_eq!(nav_of(go_back()), Some(NavEntry::Home));
    }

    #[test]
    fn record_dedupes_current_entry() {
        reset();
        record(NavEntry::Album("1".into()));
        record(NavEntry::Album("1".into()));
        assert_eq!(nav_of(go_back()), Some(NavEntry::Home));
    }

    #[test]
    fn scroll_is_stamped_on_leave_and_restored_on_return() {
        reset();
        // On Home, scroll down a bit, then navigate away.
        set_live_scroll(-420.0);
        record(NavEntry::Album("1".into()));
        // Fresh page starts at the top.
        assert_eq!(live_scroll(), 0.0);
        // Scroll the album page, then go back to Home.
        set_live_scroll(-90.0);
        let (entry, scroll) = go_back().expect("back to Home");
        assert_eq!(entry, NavEntry::Home);
        assert_eq!(scroll, -420.0);
        // Going forward returns to the album at its saved scroll.
        let (entry, scroll) = go_forward().expect("forward to album");
        assert_eq!(entry, NavEntry::Album("1".into()));
        assert_eq!(scroll, -90.0);
    }
}
