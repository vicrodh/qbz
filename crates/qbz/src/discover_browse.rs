//! Discover "View all" full-list controller.
//!
//! Opens a single album module (New Releases, Press Accolades, Ideal
//! Discography, Qobuzissimes, Albums of the Week, Most Streamed) as a
//! paginated full-grid (or list) page. The Carousel's "View all" link
//! fires `discover-view-all(endpoint, title)`; the shell records the
//! history entry, switches the view to ContentView::DiscoverBrowse, and
//! calls `navigate` here.
//!
//! Pagination is driven off the backend `has_more` flag (the discover
//! endpoints carry no `total`): each page advances `offset` by the
//! FETCHED item count and stops once `has_more` is false. Reuses the
//! Discover home mappers (`crate::home::map_album` / `card_to_item`) so the
//! cards carry the same genre + localized release date as the home carousels.
//!
//! Header tools (mirroring Tauri's DiscoverBrowseView): a client-side
//! search filter over the loaded albums (disables load-more while active),
//! the shared genre filter (re-fetches from offset 0 with the raw selected
//! genre ids — Qobuz facets sub-genre ids server-side, no client narrowing),
//! and a grid/list view toggle.

use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::adapter::SlintAdapter;
use crate::artwork::{self, ArtworkJob, ArtworkTarget, ImageCache};
use crate::home::CardData;
use crate::{AlbumCardItem, AppWindow, ContentView, DiscoverBrowseState, NavState};

/// Page size — two carousel pages' worth, fetched per request.
const PAGE_SIZE: u32 = 50;

/// Fetch one page starting at `offset`, dropping blacklisted albums. Returns
/// the surviving cards, the FETCHED item count (the server offset advances by
/// the fetched — not visible — count; the blacklist drop is log-only) and
/// `has_more`. Genre filtering is server-side: the raw selection (parent or
/// sub-genre id) is in `genre_ids` and Qobuz honors sub-genre ids, so there is
/// no client-side narrowing — 1:1 with Tauri discovery-v2.
async fn fetch_pages(
    runtime: &Arc<AppRuntime<SlintAdapter>>,
    endpoint: &str,
    genre_ids: Option<Vec<u64>>,
    offset: u32,
) -> Result<(Vec<CardData>, u32, bool), String> {
    let mut data = runtime
        .core()
        .get_discover_albums(endpoint, genre_ids, offset, PAGE_SIZE)
        .await
        .map_err(|e| e.to_string())?;
    let has_more = data.has_more;
    let fetched = data.items.len() as u32;
    // T8: drop blacklisted DiscoverAlbums (ANY of artists[], featured-aware via
    // discover_album_blacklisted). Tauri's discover surfaces log-only — no count
    // adjustment (the endpoints carry no `total`; pagination is has_more-driven).
    let (bl, abl) = if crate::artist_blacklist::is_enabled() {
        (
            crate::artist_blacklist::ids_snapshot(),
            crate::artist_blacklist::album_ids_snapshot(),
        )
    } else {
        Default::default()
    };
    if !bl.is_empty() || !abl.is_empty() {
        data.items
            .retain(|a| !qbz_core::core::discover_album_blacklisted(a, &bl, &abl));
    }
    let cards: Vec<CardData> = data.items.into_iter().map(crate::home::map_album).collect();
    Ok((cards, fetched, has_more))
}

/// Open the full-list page for `endpoint` and load its first page, then
/// fan out artwork. `genre_ids` is the shared genre-filter selection
/// (None = no filter). Mirrors `navigate_favorites` in main.rs.
pub fn navigate(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: ImageCache,
    endpoint: String,
    title: String,
    genre_ids: Option<Vec<u64>>,
) {
    let endpoint_for_fetch = endpoint.clone();
    let genre_for_fetch = genre_ids.clone();
    handle.spawn(async move {
        // Reset the page state and switch the view on the UI thread. The
        // search query is cleared on a fresh navigation; the view mode is
        // left as-is so it persists across pages.
        {
            let title = title.clone();
            let endpoint = endpoint.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                let state = w.global::<DiscoverBrowseState>();
                state.set_title(title.into());
                state.set_endpoint(endpoint.into());
                state.set_next_offset(0);
                state.set_search_query("".into());
                state.set_albums(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
                state.set_visible(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
                state.set_loading(true);
                state.set_loading_more(false);
                state.set_has_more(true);
                w.global::<NavState>().set_view(ContentView::DiscoverBrowse);
            });
        }

        match fetch_pages(&runtime, &endpoint_for_fetch, genre_for_fetch, 0).await {
            // CardData is plain/Send — map it to the (non-Send)
            // AlbumCardItem inside the event-loop closure below. The offset
            // advances by the FETCHED count (blacklist drop is log-only).
            Ok((cards, fetched, has_more)) => {
                let jobs = artwork_jobs(&cards, 0);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let items: Vec<AlbumCardItem> =
                        cards.into_iter().map(crate::home::card_to_item).collect();
                    let state = w.global::<DiscoverBrowseState>();
                    state.set_albums(ModelRc::new(VecModel::from(items)));
                    state.set_next_offset(fetched as i32);
                    state.set_has_more(has_more);
                    state.set_loading(false);
                    apply_filter(&w);
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
            }
            Err(e) => {
                log::error!("[qbz-slint] discover-browse load failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    let state = w.global::<DiscoverBrowseState>();
                    state.set_loading(false);
                    state.set_has_more(false);
                });
            }
        }
    });
}

/// Fetch the next page (offset = DiscoverBrowseState.next-offset) and
/// append it to the grid. Wired to DiscoverBrowseActions::load-more.
/// `genre_ids` is the shared genre-filter selection (None = no filter).
pub fn load_more(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: ImageCache,
    genre_ids: Option<Vec<u64>>,
) {
    let Some(w) = weak.upgrade() else {
        return;
    };
    let state = w.global::<DiscoverBrowseState>();
    if !state.get_has_more() || state.get_loading_more() {
        return;
    }
    // A non-empty search filters the loaded set client-side; pulling more
    // pages while filtering matches no UX (Tauri disables load-more too).
    if !state.get_search_query().is_empty() {
        return;
    }
    let endpoint = state.get_endpoint().to_string();
    let offset = state.get_next_offset().max(0) as u32;
    // New cards land after the currently-loaded albums. With narrowing
    // active the server offset outruns the model length, so the artwork
    // base index must come from the model, not the offset.
    let base_index = state.get_albums().row_count();
    state.set_loading_more(true);

    handle.spawn(async move {
        match fetch_pages(&runtime, &endpoint, genre_ids, offset).await {
            Ok((cards, fetched, has_more)) => {
                let jobs = artwork_jobs(&cards, base_index);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    append_albums(&w, cards, fetched, has_more);
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
            }
            Err(e) => {
                log::error!("[qbz-slint] discover-browse load-more failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<DiscoverBrowseState>().set_loading_more(false);
                });
            }
        }
    });
}

/// Append a freshly-fetched page onto the existing grid, advancing the
/// offset by the FETCHED item count (`cards` may be a narrowed subset)
/// and updating has-more. UI thread only.
fn append_albums(window: &AppWindow, cards: Vec<CardData>, fetched: u32, has_more: bool) {
    let state = window.global::<DiscoverBrowseState>();
    let model = state.get_albums();
    let mut combined: Vec<AlbumCardItem> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .collect();
    combined.extend(cards.into_iter().map(crate::home::card_to_item));
    state.set_albums(ModelRc::new(VecModel::from(combined)));
    state.set_next_offset(state.get_next_offset() + fetched as i32);
    state.set_has_more(has_more);
    state.set_loading_more(false);
    apply_filter(window);
}

/// Rebuild `visible` from `albums` honoring the current search query
/// (case-insensitive substring over title + artist). UI thread only.
/// Wired to DiscoverBrowseActions::search-changed and called after every
/// model mutation so the rendered list stays consistent.
pub fn apply_filter(window: &AppWindow) {
    let state = window.global::<DiscoverBrowseState>();
    let query = state.get_search_query().trim().to_lowercase();
    let albums = state.get_albums();
    if query.is_empty() {
        // No filter — share the SAME model so artwork-pipeline updates
        // (which mutate `albums[index]`) propagate to the rendered list
        // without rebuilding it. This is the common case.
        state.set_visible(albums);
        return;
    }
    let visible: Vec<AlbumCardItem> = (0..albums.row_count())
        .filter_map(|i| albums.row_data(i))
        .filter(|a| {
            a.title.to_lowercase().contains(&query)
                || a.artist.to_lowercase().contains(&query)
        })
        .collect();
    state.set_visible(ModelRc::new(VecModel::from(visible)));
}

/// Artwork jobs for a page of cards, targeting their absolute indices
/// (`base_index` is the offset of the first card in the model).
fn artwork_jobs(cards: &[CardData], base_index: usize) -> Vec<ArtworkJob> {
    cards
        .iter()
        .enumerate()
        .filter(|(_, card)| !card.artwork_url.is_empty())
        .map(|(i, card)| ArtworkJob {
            url: card.artwork_url.clone(),
            target: ArtworkTarget::DiscoverBrowseAlbum {
                index: base_index + i,
            },
        })
        .collect()
}
