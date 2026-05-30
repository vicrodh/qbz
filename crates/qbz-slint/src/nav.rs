//! Page navigation history — a browser-like back/forward stack.
//!
//! The shell records a [`NavEntry`] on every fresh navigation; the
//! `[<] [>]` button pair (and the mouse back/forward buttons) walk the
//! stack. UI thread only, hence `thread_local`.
//!
//! Scroll-position restoration per entry is a planned follow-up; for now
//! the stack tracks which page was visited, not where it was scrolled.

use std::cell::RefCell;

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

struct History {
    entries: Vec<NavEntry>,
    /// Index of the entry currently shown.
    cursor: usize,
}

thread_local! {
    static HISTORY: RefCell<History> = RefCell::new(History {
        entries: vec![NavEntry::Home],
        cursor: 0,
    });
}

/// Push a Search history entry, OR replace the cursor entry in place
/// when it is already a Search. Used by the live-search debounce so
/// quick keystrokes do not push one entry per character, while still
/// keeping the page reachable via back/forward at the final query.
pub fn push_or_replace_search(query: String) {
    HISTORY.with(|h| {
        let h = &mut *h.borrow_mut();
        match h.entries.get(h.cursor) {
            Some(NavEntry::Search(_)) => {
                h.entries.truncate(h.cursor + 1);
                h.entries[h.cursor] = NavEntry::Search(query);
            }
            _ => {
                h.entries.truncate(h.cursor + 1);
                h.entries.push(NavEntry::Search(query));
                h.cursor = h.entries.len() - 1;
            }
        }
    });
}

/// Record a fresh forward navigation, dropping any forward history. A
/// no-op when the destination already is the current entry, so repeated
/// clicks on the same page do not pile up.
pub fn record(entry: NavEntry) {
    HISTORY.with(|h| {
        let h = &mut *h.borrow_mut();
        if h.entries.get(h.cursor) == Some(&entry) {
            return;
        }
        h.entries.truncate(h.cursor + 1);
        h.entries.push(entry);
        h.cursor = h.entries.len() - 1;
    });
}

/// Step back; returns the entry that is now current, or `None` at the
/// start of the stack.
pub fn go_back() -> Option<NavEntry> {
    HISTORY.with(|h| {
        let h = &mut *h.borrow_mut();
        if h.cursor == 0 {
            return None;
        }
        h.cursor -= 1;
        h.entries.get(h.cursor).cloned()
    })
}

/// Step forward; returns the entry that is now current, or `None` at the
/// end of the stack.
pub fn go_forward() -> Option<NavEntry> {
    HISTORY.with(|h| {
        let h = &mut *h.borrow_mut();
        if h.cursor + 1 >= h.entries.len() {
            return None;
        }
        h.cursor += 1;
        h.entries.get(h.cursor).cloned()
    })
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
                entries: vec![NavEntry::Home],
                cursor: 0,
            };
        });
    }

    #[test]
    fn record_then_back_and_forward() {
        reset();
        assert!(!can_back());
        record(NavEntry::Album("1".into()));
        record(NavEntry::Artist("2".into()));
        assert!(can_back());
        assert!(!can_forward());
        assert_eq!(go_back(), Some(NavEntry::Album("1".into())));
        assert_eq!(go_back(), Some(NavEntry::Home));
        assert_eq!(go_back(), None);
        assert_eq!(go_forward(), Some(NavEntry::Album("1".into())));
    }

    #[test]
    fn record_truncates_forward_history() {
        reset();
        record(NavEntry::Album("1".into()));
        record(NavEntry::Album("2".into()));
        go_back();
        record(NavEntry::Artist("3".into()));
        assert!(!can_forward());
        assert_eq!(go_back(), Some(NavEntry::Album("1".into())));
    }

    #[test]
    fn search_entry_round_trips_history() {
        reset();
        record(NavEntry::Search("metallica".into()));
        record(NavEntry::Album("5".into()));
        assert_eq!(go_back(), Some(NavEntry::Search("metallica".into())));
        assert_eq!(go_back(), Some(NavEntry::Home));
    }

    #[test]
    fn record_dedupes_current_entry() {
        reset();
        record(NavEntry::Album("1".into()));
        record(NavEntry::Album("1".into()));
        assert_eq!(go_back(), Some(NavEntry::Home));
    }
}
