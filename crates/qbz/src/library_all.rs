//! Library "All" — the mixed feed controller (webplayer /user-library/all).
//!
//! There is NO single Qobuz endpoint for the aggregated library; the webplayer
//! merges favorites + purchases + playlists client-side. We do the same: fan out
//! to the existing per-type loaders, normalize each into a `Feed` item, merge and
//! order by "date added" (approximated from each source's server order), then push
//! into `LibraryAllState`. Search / sort / source-switch filtering all run in Rust
//! (`derive`) — Slint renders the pre-computed `items-visible`.

use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::adapter::SlintAdapter;
use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::favorites::{self, FavData, FavTab};
use crate::{AppWindow, LibraryAllState, LibraryFeedItem};

type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Plain, `Send` feed item produced on the worker thread.
#[derive(Clone, Default)]
pub struct Feed {
    pub kind: String,   // track | album | artist | playlist | label
    pub group: String,  // favorites | following | purchases
    pub source: String, // qobuz | local | plex
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub artist: String,
    pub artist_id: String,
    pub album: String,
    pub album_id: String,
    pub image_url: String,
    pub quality_tier: String,
    pub quality_detail: String,
    pub is_favorite: bool,
    /// Genre name (albums + tracks carry one; artists/labels/playlists ""). Feeds
    /// the client-side genre filter — "" is excluded when a genre is selected.
    pub genre: String,
    /// Playlist ownership (only meaningful for kind == "playlist"): owned →
    /// favorite affordance; foreign Qobuz → follow + copy.
    pub playlist_owned: bool,
    pub playlist_following: bool,
    pub playlist_copied: bool,
    /// Recency proxy in [0.0, 1.0]; 0.0 = most-recently added. Each source list
    /// comes back date-desc, so `index / len` interleaves the sources by recency
    /// without needing exact per-item timestamps.
    pub added_rank: f32,
}

fn rank(i: usize, n: usize) -> f32 {
    if n <= 1 {
        0.0
    } else {
        i as f32 / n as f32
    }
}

/// Fan out to every source, normalize + merge into one date-ordered feed.
/// Qobuz-only for now (favorites + following + purchases); local/Plex arrive
/// with the Phase 2 local-favorites layer behind the `show-local` switch.
pub async fn load_library_all(runtime: &Runtime) -> Result<Vec<Feed>, String> {
    let mut feed: Vec<Feed> = Vec::new();

    // --- Favorites: tracks + albums (group "favorites") -------------------
    if let Ok(FavData::Tracks { items, .. }) =
        favorites::load_favorites(runtime, FavTab::Tracks).await
    {
        let n = items.len();
        for (i, t) in items.into_iter().enumerate() {
            feed.push(Feed {
                kind: "track".into(),
                group: "favorites".into(),
                source: "qobuz".into(),
                subtitle: t.artist.clone(),
                artist: t.artist,
                artist_id: t.artist_id,
                album: t.album,
                album_id: t.album_id,
                image_url: t.artwork_url,
                quality_tier: t.quality_tier,
                quality_detail: t.quality_detail,
                is_favorite: true,
                genre: t.genre,
                added_rank: rank(i, n),
                id: t.id,
                title: t.title,
                ..Default::default()
            });
        }
    }
    if let Ok(FavData::Albums { items, .. }) =
        favorites::load_favorites(runtime, FavTab::Albums).await
    {
        let n = items.len();
        for (i, a) in items.into_iter().enumerate() {
            feed.push(Feed {
                kind: "album".into(),
                group: "favorites".into(),
                source: "qobuz".into(),
                subtitle: a.artist.clone(),
                artist: a.artist,
                artist_id: a.artist_id,
                album: String::new(),
                album_id: String::new(),
                image_url: a.artwork_url,
                quality_tier: a.quality_tier,
                quality_detail: a.quality_detail,
                is_favorite: true,
                genre: a.genre,
                added_rank: rank(i, n),
                id: a.id,
                title: a.title,
                ..Default::default()
            });
        }
    }

    // --- Following: artists + labels (group "following") ------------------
    if let Ok(FavData::Artists { items, .. }) =
        favorites::load_favorites(runtime, FavTab::Artists).await
    {
        let n = items.len();
        for (i, ar) in items.into_iter().enumerate() {
            feed.push(Feed {
                kind: "artist".into(),
                group: "following".into(),
                source: "qobuz".into(),
                subtitle: String::new(),
                artist: String::new(),
                artist_id: ar.id.clone(),
                album: String::new(),
                album_id: String::new(),
                image_url: ar.image_url,
                quality_tier: String::new(),
                quality_detail: String::new(),
                is_favorite: true,
                added_rank: rank(i, n),
                id: ar.id,
                title: ar.name,
                ..Default::default()
            });
        }
    }
    if let Ok(FavData::Labels { items, .. }) =
        favorites::load_favorites(runtime, FavTab::Labels).await
    {
        let n = items.len();
        for (i, l) in items.into_iter().enumerate() {
            feed.push(Feed {
                kind: "label".into(),
                group: "following".into(),
                source: "qobuz".into(),
                subtitle: l.albums_line,
                artist: String::new(),
                artist_id: String::new(),
                album: String::new(),
                album_id: String::new(),
                image_url: l.image_url,
                quality_tier: String::new(),
                quality_detail: String::new(),
                is_favorite: true,
                added_rank: rank(i, n),
                id: l.id,
                title: l.name,
                ..Default::default()
            });
        }
    }

    // --- Playlists: owned/hearted = favorites, followed = following -------
    if let Ok(FavData::Playlists {
        favorites: fav_pl,
        following: fol_pl,
    }) = favorites::load_favorites(runtime, FavTab::Playlists).await
    {
        let n = fav_pl.len();
        for (i, p) in fav_pl.into_iter().enumerate() {
            let image_url = p.cover_urls.iter().next().cloned().unwrap_or_default();
            feed.push(Feed {
                kind: "playlist".into(),
                group: "favorites".into(),
                source: "qobuz".into(),
                subtitle: p.subtitle,
                artist: String::new(),
                artist_id: String::new(),
                album: String::new(),
                album_id: String::new(),
                image_url,
                quality_tier: String::new(),
                quality_detail: String::new(),
                is_favorite: true,
                playlist_owned: p.is_owned,
                playlist_following: p.is_following,
                playlist_copied: p.is_copied,
                added_rank: rank(i, n),
                id: p.id,
                title: p.title,
                ..Default::default()
            });
        }
        let n = fol_pl.len();
        for (i, p) in fol_pl.into_iter().enumerate() {
            let image_url = p.cover_urls.iter().next().cloned().unwrap_or_default();
            feed.push(Feed {
                kind: "playlist".into(),
                group: "following".into(),
                source: "qobuz".into(),
                subtitle: p.subtitle,
                artist: String::new(),
                artist_id: String::new(),
                album: String::new(),
                album_id: String::new(),
                image_url,
                quality_tier: String::new(),
                quality_detail: String::new(),
                is_favorite: false,
                playlist_owned: p.is_owned,
                playlist_following: p.is_following,
                playlist_copied: p.is_copied,
                added_rank: rank(i, n),
                id: p.id,
                title: p.title,
                ..Default::default()
            });
        }
    }

    // --- Purchases: albums + tracks (group "purchases") -------------------
    // `search_purchases("")` returns the full owned set (both types).
    if let Ok((albums, tracks)) = crate::purchases::search_purchases(runtime, "").await {
        let n = albums.len();
        for (i, a) in albums.into_iter().enumerate() {
            let image_url = a.image.best().cloned().unwrap_or_default();
            let tier = if a.hires { "hires" } else { "cd" };
            let genre = a.genre.as_ref().map(|g| g.name.clone()).unwrap_or_default();
            feed.push(Feed {
                kind: "album".into(),
                group: "purchases".into(),
                source: "qobuz".into(),
                subtitle: a.artist.name.clone(),
                artist: a.artist.name,
                artist_id: a.artist.id.to_string(),
                album: String::new(),
                album_id: String::new(),
                image_url,
                quality_tier: tier.into(),
                quality_detail: String::new(),
                is_favorite: false,
                genre,
                added_rank: rank(i, n),
                id: a.id,
                title: a.title,
                ..Default::default()
            });
        }
        let n = tracks.len();
        for (i, t) in tracks.into_iter().enumerate() {
            let (artist, image_url, album, album_id) = {
                let artist = t.performer.name.clone();
                let (img, alb, aid) = t
                    .album
                    .as_ref()
                    .map(|a| {
                        (
                            a.image.best().cloned().unwrap_or_default(),
                            a.title.clone(),
                            a.id.clone(),
                        )
                    })
                    .unwrap_or_default();
                (artist, img, alb, aid)
            };
            let tier = if t.hires { "hires" } else { "cd" };
            feed.push(Feed {
                kind: "track".into(),
                group: "purchases".into(),
                source: "qobuz".into(),
                subtitle: artist.clone(),
                artist,
                artist_id: t.performer.id.to_string(),
                album,
                album_id,
                image_url,
                quality_tier: tier.into(),
                quality_detail: String::new(),
                is_favorite: false,
                added_rank: rank(i, n),
                id: t.id.to_string(),
                title: t.title,
                ..Default::default()
            });
        }
    }

    // --- Local + Plex favorites (source "local"/"plex"; gated by show-local
    // in derive). group "local" — bypasses the Qobuz source switches. ---
    {
        let locals = crate::local_favorites::list();
        let n = locals.len();
        for (i, lf) in locals.into_iter().enumerate() {
            feed.push(Feed {
                kind: lf.kind,
                group: "local".into(),
                source: lf.source,
                subtitle: lf.subtitle,
                artist: lf.artist.clone(),
                artist_id: String::new(),
                album: String::new(),
                album_id: String::new(),
                image_url: lf.artwork_url,
                quality_tier: String::new(),
                quality_detail: String::new(),
                is_favorite: true,
                added_rank: rank(i, n),
                id: lf.id,
                title: lf.title,
                ..Default::default()
            });
        }
    }

    // Merge by recency proxy (stable so equal ranks keep source order).
    feed.sort_by(|a, b| {
        a.added_rank
            .partial_cmp(&b.added_rank)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(feed)
}

fn to_item(f: &Feed) -> LibraryFeedItem {
    LibraryFeedItem {
        kind: f.kind.clone().into(),
        group: f.group.clone().into(),
        source: f.source.clone().into(),
        id: f.id.clone().into(),
        title: f.title.clone().into(),
        subtitle: f.subtitle.clone().into(),
        artist: f.artist.clone().into(),
        artist_id: f.artist_id.clone().into(),
        album: f.album.clone().into(),
        album_id: f.album_id.clone().into(),
        image_url: f.image_url.clone().into(),
        image: slint::Image::default(),
        quality_tier: f.quality_tier.clone().into(),
        quality_detail: f.quality_detail.clone().into(),
        is_favorite: f.is_favorite,
        removing: false,
        sort_title: f.title.to_lowercase().into(),
        sort_artist: f.artist.to_lowercase().into(),
        genre: f.genre.to_lowercase().into(),
        playlist_owned: f.playlist_owned,
        playlist_following: f.playlist_following,
        playlist_copied: f.playlist_copied,
    }
}

/// Push the full merged feed into `LibraryAllState` and derive the first view.
pub fn apply_library_all(window: &AppWindow, feed: Vec<Feed>) {
    let items: Vec<LibraryFeedItem> = feed.iter().map(to_item).collect();
    let total = items.len() as i32;
    let st = window.global::<LibraryAllState>();
    st.set_items(ModelRc::new(VecModel::from(items)));
    st.set_total(total);
    st.set_loading(false);
    st.set_load_error("".into());
    derive(window);
}

/// PlaylistView-style sort toggle: re-selecting the active field flips its
/// direction; a new field resets to that field's natural default ("date"
/// newest-first, "title"/"artist" A→Z). Then re-derive.
pub fn set_sort(window: &AppWindow, field: &str) {
    let st = window.global::<LibraryAllState>();
    let cur_field = st.get_sort_by().to_string();
    let new_asc = if cur_field == field {
        !st.get_sort_asc()
    } else {
        // "date" starts descending (newest first); the others start ascending.
        field != "date"
    };
    st.set_sort_by(field.into());
    st.set_sort_asc(new_asc);
    derive(window);
}

/// Apply search + source-switch + genre + sort over the full model into
/// `items-visible`. Runs on the Slint event loop; Slint never sorts/filters.
pub fn derive(window: &AppWindow) {
    let st = window.global::<LibraryAllState>();
    let needle = st.get_search().to_lowercase();
    let show_purchases = st.get_show_purchases();
    let show_favorites = st.get_show_favorites();
    let show_following = st.get_show_following();
    let show_local = st.get_show_local();
    let sort_by = st.get_sort_by();
    let sort_asc = st.get_sort_asc();
    // Shared genre filter (its own "library-all" context). Empty = no filter;
    // otherwise an item shows only when its (lowercased) genre matches one of
    // the selected genre names — kinds with no genre (artist/label/playlist)
    // are excluded, so the feed narrows to the chosen genre's albums + tracks.
    let genre_names: Vec<String> = crate::genre_filter::selected_names("library-all")
        .into_iter()
        .map(|g| g.to_lowercase())
        .collect();

    let full = st.get_items();
    let mut out: Vec<LibraryFeedItem> = Vec::new();
    for i in 0..full.row_count() {
        let Some(item) = full.row_data(i) else {
            continue;
        };
        let src = item.source.as_str();
        let is_local = src == "local" || src == "plex";
        if is_local {
            // Local files + Plex are gated ONLY by the show-local switch; they
            // bypass the Qobuz purchases/favorites/following switches.
            if !show_local {
                continue;
            }
        } else {
            // Qobuz source switches: an item shows when its group's switch is on.
            // If ALL three are off, treat as "no filter" (show everything) to
            // avoid an empty grid from an accidental all-off state.
            let any_group = show_purchases || show_favorites || show_following;
            let group = item.group.as_str();
            let group_ok = !any_group
                || (group == "purchases" && show_purchases)
                || (group == "favorites" && show_favorites)
                || (group == "following" && show_following);
            if !group_ok {
                continue;
            }
        }
        if !needle.is_empty() {
            let hit = item.sort_title.as_str().contains(&needle)
                || item.sort_artist.as_str().contains(&needle);
            if !hit {
                continue;
            }
        }
        if !genre_names.is_empty() {
            let g = item.genre.as_str();
            if g.is_empty() || !genre_names.iter().any(|n| g.contains(n.as_str())) {
                continue;
            }
        }
        out.push(item);
    }

    // Canonical ascending order per field, then reverse for the other
    // direction. "date" has no key on the item (the model is stored
    // newest-first from load), so it uses the inherent order: asc(false) =
    // newest-first (default), asc(true) = oldest-first (reversed).
    match sort_by.as_str() {
        "title" => {
            out.sort_by(|a, b| a.sort_title.as_str().cmp(b.sort_title.as_str()));
            if !sort_asc {
                out.reverse();
            }
        }
        "artist" => {
            out.sort_by(|a, b| a.sort_artist.as_str().cmp(b.sort_artist.as_str()));
            if !sort_asc {
                out.reverse();
            }
        }
        // "date": model order is newest-first; reverse only for oldest-first.
        _ => {
            if sort_asc {
                out.reverse();
            }
        }
    }

    st.set_items_visible(ModelRc::new(VecModel::from(out)));
}

/// Build cover-download jobs for the CURRENT visible feed. Call after apply and
/// after every derive (the ImageCache dedups already-decoded covers, so
/// re-dispatching on filter/sort is cheap). Indices target `items-visible`.
pub fn artwork_jobs(window: &AppWindow) -> Vec<ArtworkJob> {
    let visible = window.global::<LibraryAllState>().get_items_visible();
    let mut jobs = Vec::new();
    for i in 0..visible.row_count() {
        if let Some(item) = visible.row_data(i) {
            let url = item.image_url.to_string();
            if !url.is_empty() {
                jobs.push(ArtworkJob {
                    target: ArtworkTarget::LibraryAllCover { index: i },
                    url,
                });
            }
        }
    }
    jobs
}
