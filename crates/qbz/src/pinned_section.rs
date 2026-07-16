//! Pinned-section data pipeline (the Home / For You "Pinned" mixed carousel).
//!
//! Maps the per-user pinned store rows (`crate::pinned` — display snapshots
//! taken at pin time) into the mixed Slint `PinnedItem` model, pushes it onto
//! the shared `PinnedState` global, then fans out the cover downloads. ONE
//! rebuild path on purpose: the session seed AND every pin/unpin mutation
//! re-run [`rebuild_pinned`] — the store family has no change-notify, and
//! index-keyed `PinnedCard` artwork jobs must always be dispatched from the
//! row set the model was JUST rebuilt from (the album-blacklist
//! index-misalignment lesson: model first, then a fresh full job batch,
//! never jobs derived from a stale set).

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AppWindow, PinnedState};

/// Map one stored pinned row into the mixed Slint model row. `kind` selects
/// which sub-struct is populated; the other two stay at their defaults (the
/// carousel dispatches on `kind`, so they are never read). The snapshot is
/// deliberately thin — the cards render fine without the fields it does not
/// carry (genre / year / quality / ribbon stay empty).
fn to_slint_item(r: &crate::pinned::PinnedItem) -> crate::PinnedItem {
    let mut item = crate::PinnedItem {
        kind: r.kind.clone().into(),
        ..Default::default()
    };
    match r.kind.as_str() {
        "album" => {
            item.album = crate::AlbumCardItem {
                id: r.id.clone().into(),
                title: r.title.clone().into(),
                // Snapshot subtitle = the artist name at pin time.
                artist: r.subtitle.clone().into(),
                artwork_url: r.artwork_url.clone().into(),
                // Live heart state from the login-seeded cache, like every
                // other album card (home::card_to_item precedent).
                is_favorite: crate::fav_cache::is_album_favorite(&r.id),
                // Rows in the pinned list are pinned by definition — without
                // this the pinned carousel's own cards draw the unpinned glyph.
                is_pinned: true,
                ..Default::default()
            };
        }
        "artist" => {
            item.artist = crate::SlimItem {
                id: r.id.clone().into(),
                title: r.title.clone().into(),
                artwork_url: r.artwork_url.clone().into(),
                // Live follow state (= Qobuz artist favorite): the pinned
                // ArtistGridCard shows the Follow button (its follow-mode
                // defaults to "toggle"), so a hardcoded `false` would
                // mislabel already-followed artists.
                following: r
                    .id
                    .parse::<u64>()
                    .map(crate::fav_cache::is_artist_favorite)
                    .unwrap_or(false),
                // Rows in the pinned list are pinned by definition.
                is_pinned: true,
                ..Default::default()
            };
        }
        "playlist" => {
            item.playlist = crate::SearchPlaylistItem {
                id: r.id.clone().into(),
                title: r.title.clone().into(),
                subtitle: r.subtitle.clone().into(),
                // Single-cover shape, like home::playlist_to_item: slot 0
                // only; count 0 draws the card's placeholder.
                cover_count: if r.artwork_url.is_empty() { 0 } else { 1 },
                url1: r.artwork_url.clone().into(),
                // Neutral dark letterbox until the cover decodes and the
                // artwork pipeline writes the real dominant colour.
                dominant_color: slint::Color::from_rgb_u8(30, 30, 34),
                // Rows in the pinned list are pinned by definition.
                is_pinned: true,
                ..Default::default()
            };
        }
        // Unreachable while the store's CHECK constraint pins the kind set;
        // a foreign row degrades to an empty slot (no carousel arm matches).
        _ => {}
    }
    item
}

/// Map the stored pinned rows (newest pin first) into the Slint model rows.
pub fn pinned_items_model(rows: &[crate::pinned::PinnedItem]) -> Vec<crate::PinnedItem> {
    rows.iter().map(to_slint_item).collect()
}

/// Rebuild `PinnedState.items` from the pinned store and fan out the cover
/// downloads. Called at shell entry (online AND offline — the store is
/// local-only) and after every pin/unpin. The model is set FIRST, then the
/// job batch built from the very rows the model now holds is dispatched;
/// covers ride the shared image cache, so disk-cached art resolves offline.
pub fn rebuild_pinned(window: &AppWindow) {
    let rows = crate::pinned::list();
    let items = pinned_items_model(&rows);
    let jobs = crate::artwork::pinned_artwork_jobs(&items);
    window
        .global::<PinnedState>()
        .set_items(ModelRc::new(VecModel::from(items)));
    if jobs.is_empty() {
        return;
    }
    let Some(cache) = crate::artwork::shared_cache() else {
        return;
    };
    // The pinned list mixes sources: Qobuz covers (http URLs), Local Library
    // covers (filesystem paths) and Plex thumbs (`/library/…`). spawn_search_loads
    // routes each job by its url shape (the same mixed-source dispatcher the
    // search cortinilla uses), so local/plex pinned albums render their real
    // covers instead of a placeholder. Click routing to the right page is
    // already source-aware via is_local_album_key on the stored id.
    let plex = crate::plex_settings::get();
    crate::artwork::spawn_search_loads(jobs, plex.base_url, plex.token, window.as_weak(), cache);
}
