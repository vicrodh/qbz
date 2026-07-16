//! Qobuz Playlists "View all" full-list controller.
//!
//! The playlist twin of `discover_browse`: opens the Qobuz Playlists rail
//! (Home / Editor's Picks) as a paginated full grid/list page backed by
//! `/discover/playlists`. Two filters are SERVER-side — the single-select
//! category tag bar (`/playlist/getTags`, localized names) and the shared
//! "discover" genre selection both re-fetch from offset 0 — while the
//! header search box filters the loaded set client-side and disables
//! load-more while active (same contract as the album browse page).
//!
//! Unlike the album endpoints there is no sub-genre narrowing here: the
//! `genre_ids` facet (`current_genre_filter()`, top-level ancestor ids) is
//! passed through as-is — Discover playlists carry no `genre.path` to
//! narrow against client-side.
//!
//! The selected tag is process state (`SELECTED_TAG`) rather than a
//! read-back of the Slint global, so the fetch tasks can read it off the
//! UI thread. It survives genre-filter re-navigations and resets only on
//! a fresh open from the rail's "View all" link (Tauri resets on mount).

use std::sync::{Arc, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_models::{DiscoverPlaylist, PlaylistTag};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::adapter::SlintAdapter;
use crate::artwork::{self, ArtworkJob, ArtworkTarget, ImageCache};
use crate::{
    AppWindow, ContentView, NavState, PlaylistBrowseState, PlaylistTagItem, SearchPlaylistItem,
};

/// Page size — mirrors discover_browse (and Tauri's limit=50).
const PAGE_SIZE: u32 = 50;

/// The active category tag slug ("" = All). See the module docs for why
/// this lives outside the Slint global.
static SELECTED_TAG: Mutex<String> = Mutex::new(String::new());

fn selected_tag() -> String {
    SELECTED_TAG
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

/// One loaded playlist: the shared single-cover card plus the list-row
/// subtitle (owner + track count) that the rail cards drop but the browse
/// list view renders.
struct BrowseCard {
    card: crate::home::PlaylistCardData,
    subtitle: String,
}

/// Map a Discover playlist reusing the Home rail mapper, capturing the
/// owner + localized track count for the list rows before the payload
/// moves into `map_playlist` (same "owner   •   N tracks" convention as
/// the search playlist rows).
fn map_browse(p: DiscoverPlaylist) -> BrowseCard {
    let mut subtitle = p.owner.name.clone();
    if p.tracks_count > 0 {
        let n = p.tracks_count;
        let tracks_label =
            qbz_i18n::tf("{} track", "{} tracks", n as i64, &[&n.to_string()]);
        if subtitle.is_empty() {
            subtitle = tracks_label;
        } else {
            subtitle = format!("{}   •   {}", subtitle, tracks_label);
        }
    }
    BrowseCard {
        card: crate::home::map_playlist(p),
        subtitle,
    }
}

/// Convert to the Slint item — the rail converter plus the subtitle (the
/// grid cards ignore it; the list rows render it).
fn to_item(bc: &BrowseCard) -> SearchPlaylistItem {
    let mut item = crate::home::playlist_to_item(&bc.card);
    item.subtitle = bc.subtitle.clone().into();
    item
}

/// Fetch one page of `/discover/playlists` faceted by the tag slug
/// ("" = All) + the shared genre selection. Returns the cards and the
/// backend `has_more` flag (the endpoint carries no `total`).
async fn fetch_page(
    runtime: &Arc<AppRuntime<SlintAdapter>>,
    tag: &str,
    genre_ids: Option<Vec<u64>>,
    offset: u32,
) -> Result<(Vec<BrowseCard>, bool), String> {
    let tag_opt = (!tag.is_empty()).then(|| tag.to_string());
    let data = runtime
        .core()
        .get_discover_playlists(tag_opt, genre_ids, Some(PAGE_SIZE), Some(offset))
        .await
        .map_err(|e| e.to_string())?;
    let has_more = data.has_more;
    Ok((data.items.into_iter().map(map_browse).collect(), has_more))
}

/// Open the Qobuz Playlists full-list page: reset the page state, switch
/// the view, fetch the tag bar + the first page concurrently, then fan
/// out artwork. `genre_ids` is the shared genre-filter selection (None =
/// no filter). `reset_tag` picks the tag semantics: true on a fresh open
/// from the rail's "View all" (back to All), false on genre-filter /
/// history re-navigations (the selected tab survives).
pub fn navigate(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: ImageCache,
    genre_ids: Option<Vec<u64>>,
    reset_tag: bool,
) {
    if reset_tag {
        if let Ok(mut s) = SELECTED_TAG.lock() {
            s.clear();
        }
    }
    let selected = selected_tag();
    handle.spawn(async move {
        // Reset the page state and switch the view on the UI thread. The
        // search query clears on a fresh navigation; the view mode and the
        // selected tag persist (the tag was cleared above when this is a
        // fresh open from the rail).
        {
            let selected = selected.clone();
            let _ = weak.upgrade_in_event_loop(move |w| {
                let state = w.global::<PlaylistBrowseState>();
                state.set_title(qbz_i18n::t("Qobuz Playlists").into());
                state.set_selected_tag(selected.into());
                state.set_next_offset(0);
                state.set_search_query("".into());
                state.set_playlists(ModelRc::new(VecModel::from(
                    Vec::<SearchPlaylistItem>::new(),
                )));
                state.set_visible(ModelRc::new(VecModel::from(
                    Vec::<SearchPlaylistItem>::new(),
                )));
                state.set_loading(true);
                state.set_loading_more(false);
                state.set_has_more(true);
                w.global::<NavState>().set_view(ContentView::PlaylistBrowse);
            });
        }

        let (tags_res, page_res) = futures_util::join!(
            runtime.core().get_playlist_tags(),
            fetch_page(&runtime, &selected, genre_ids, 0)
        );

        // A tag-bar failure is non-fatal: the page still lists playlists.
        let tags: Vec<PlaylistTag> = match tags_res {
            Ok(tags) => tags,
            Err(e) => {
                log::warn!("[qbz-slint] playlist-browse tags load failed: {e}");
                Vec::new()
            }
        };

        match page_res {
            Ok((cards, has_more)) => {
                let jobs = artwork_jobs(&cards, 0);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let items: Vec<SearchPlaylistItem> = cards.iter().map(to_item).collect();
                    let fetched = items.len() as i32;
                    let state = w.global::<PlaylistBrowseState>();
                    state.set_tags(ModelRc::new(VecModel::from(
                        tags.into_iter()
                            .map(|t| {
                                let is_selected = t.slug == selected;
                                PlaylistTagItem {
                                    slug: t.slug.into(),
                                    name: t.name.into(),
                                    selected: is_selected,
                                }
                            })
                            .collect::<Vec<_>>(),
                    )));
                    state.set_playlists(ModelRc::new(VecModel::from(items)));
                    state.set_next_offset(fetched);
                    state.set_has_more(has_more);
                    state.set_loading(false);
                    apply_filter(&w);
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
            }
            Err(e) => {
                log::error!("[qbz-slint] playlist-browse load failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    let state = w.global::<PlaylistBrowseState>();
                    state.set_loading(false);
                    state.set_has_more(false);
                });
            }
        }
    });
}

/// Fetch the next page (offset = PlaylistBrowseState.next-offset) and
/// append it. Wired to PlaylistBrowseActions::load-more; `genre_ids` is
/// the shared genre-filter selection. UI thread entry.
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
    let state = w.global::<PlaylistBrowseState>();
    if !state.get_has_more() || state.get_loading_more() || state.get_loading() {
        return;
    }
    // A non-empty search filters the loaded set client-side; pulling more
    // pages while filtering matches no UX (Tauri disables load-more too).
    if !state.get_search_query().is_empty() {
        return;
    }
    let offset = state.get_next_offset().max(0) as u32;
    let base_index = state.get_playlists().row_count();
    state.set_loading_more(true);
    let selected = selected_tag();

    handle.spawn(async move {
        match fetch_page(&runtime, &selected, genre_ids, offset).await {
            Ok((cards, has_more)) => {
                let jobs = artwork_jobs(&cards, base_index);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    append_playlists(&w, cards, has_more);
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
            }
            Err(e) => {
                log::error!("[qbz-slint] playlist-browse load-more failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    w.global::<PlaylistBrowseState>().set_loading_more(false);
                });
            }
        }
    });
}

/// Append a freshly-fetched page onto the loaded set, advancing the
/// offset and updating has-more. UI thread only.
fn append_playlists(window: &AppWindow, cards: Vec<BrowseCard>, has_more: bool) {
    let state = window.global::<PlaylistBrowseState>();
    let model = state.get_playlists();
    let mut combined: Vec<SearchPlaylistItem> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .collect();
    combined.extend(cards.iter().map(to_item));
    state.set_playlists(ModelRc::new(VecModel::from(combined)));
    state.set_next_offset(state.get_next_offset() + cards.len() as i32);
    state.set_has_more(has_more);
    state.set_loading_more(false);
    apply_filter(window);
}

/// Rebuild `visible` from `playlists` honoring the current search query
/// (case-insensitive substring over title + subtitle). UI thread only.
/// Wired to PlaylistBrowseActions::search-changed and called after every
/// model mutation so the rendered list stays consistent.
pub fn apply_filter(window: &AppWindow) {
    let state = window.global::<PlaylistBrowseState>();
    let query = state.get_search_query().trim().to_lowercase();
    let playlists = state.get_playlists();
    if query.is_empty() {
        // No filter — share the SAME model so artwork-pipeline updates
        // (which mutate `playlists[idx]`) propagate to the rendered list
        // without rebuilding it. This is the common case.
        state.set_visible(playlists);
        return;
    }
    let visible: Vec<SearchPlaylistItem> = (0..playlists.row_count())
        .filter_map(|i| playlists.row_data(i))
        .filter(|p| {
            p.title.to_lowercase().contains(&query)
                || p.subtitle.to_lowercase().contains(&query)
        })
        .collect();
    state.set_visible(ModelRc::new(VecModel::from(visible)));
}

/// Select a category tag (slug; "" = All): update the radio flags, then
/// re-fetch page 0 server-side with the tag + the shared genre selection
/// (same as `navigate` minus the view switch and the tag re-fetch). The
/// search query is kept — it is a client-side filter over whatever set is
/// loaded (Tauri keeps it across tag switches too). UI thread entry.
pub fn select_tag(
    runtime: Arc<AppRuntime<SlintAdapter>>,
    weak: slint::Weak<AppWindow>,
    handle: &tokio::runtime::Handle,
    image_cache: ImageCache,
    slug: String,
    genre_ids: Option<Vec<u64>>,
) {
    {
        let Ok(mut s) = SELECTED_TAG.lock() else {
            return;
        };
        if *s == slug {
            // Re-clicking the active tag (or All) is a no-op.
            return;
        }
        *s = slug.clone();
    }
    let Some(w) = weak.upgrade() else {
        return;
    };
    let state = w.global::<PlaylistBrowseState>();
    state.set_selected_tag(slug.clone().into());
    let tags = state.get_tags();
    for i in 0..tags.row_count() {
        if let Some(mut t) = tags.row_data(i) {
            let sel = t.slug.as_str() == slug;
            if t.selected != sel {
                t.selected = sel;
                tags.set_row_data(i, t);
            }
        }
    }
    // Reset the pagination and reload page 0.
    state.set_next_offset(0);
    state.set_playlists(ModelRc::new(VecModel::from(
        Vec::<SearchPlaylistItem>::new(),
    )));
    state.set_visible(ModelRc::new(VecModel::from(
        Vec::<SearchPlaylistItem>::new(),
    )));
    state.set_loading(true);
    state.set_loading_more(false);
    state.set_has_more(true);

    handle.spawn(async move {
        match fetch_page(&runtime, &slug, genre_ids, 0).await {
            Ok((cards, has_more)) => {
                let jobs = artwork_jobs(&cards, 0);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let items: Vec<SearchPlaylistItem> = cards.iter().map(to_item).collect();
                    let fetched = items.len() as i32;
                    let state = w.global::<PlaylistBrowseState>();
                    state.set_playlists(ModelRc::new(VecModel::from(items)));
                    state.set_next_offset(fetched);
                    state.set_has_more(has_more);
                    state.set_loading(false);
                    apply_filter(&w);
                });
                artwork::spawn_loads(jobs, weak.clone(), image_cache.clone());
            }
            Err(e) => {
                log::error!("[qbz-slint] playlist-browse tag load failed: {e}");
                let _ = weak.upgrade_in_event_loop(|w| {
                    let state = w.global::<PlaylistBrowseState>();
                    state.set_loading(false);
                    state.set_has_more(false);
                });
            }
        }
    });
}

/// Artwork jobs for a page of cards, targeting their absolute indices in
/// `PlaylistBrowseState.playlists` (`base_index` is the model length
/// before the page was appended).
fn artwork_jobs(cards: &[BrowseCard], base_index: usize) -> Vec<ArtworkJob> {
    cards
        .iter()
        .enumerate()
        .filter(|(_, bc)| !bc.card.artwork_url.is_empty())
        .map(|(i, bc)| ArtworkJob {
            url: bc.card.artwork_url.clone(),
            target: ArtworkTarget::PlaylistBrowseCover {
                idx: base_index + i,
            },
        })
        .collect()
}
