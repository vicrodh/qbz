//! Purchases orchestration service (Slice 3 of the Purchases port).
//!
//! Frontend-agnostic glue between the `qbz-qobuz` purchase HTTP methods
//! (Slice 2) and the `qbz-library` `downloaded_purchases` registry. This crate
//! is the only one that already depends on BOTH `qbz-qobuz` and `qbz-library`,
//! so the orchestration lives here (ADR-006); the `qbz-slint` controller calls
//! these fns directly, never wrapping a `src-tauri` command.
//!
//! Slice 3 scope: the pagination-glue helpers around the client's
//! `get_user_purchases_*` methods, and the pure `filter_purchase_response`
//! search filter (`v2_filter_purchase_response`, ported from
//! `src-tauri/src/commands_v2/legacy_compat.rs:2627`). Format synthesis
//! (Slice 4), download-flag annotation (Slice 4) and the single-track download
//! primitive (Slice 5) are added in later slices.

use qbz_models::{PurchaseAlbum, PurchaseResponse, PurchaseTrack, SearchResultsPage};
use qbz_qobuz::QobuzClient;
use qbz_qobuz::Result as QobuzResult;

/// Fetch ONE purchases page, typed by purchase kind (`"albums"` / `"tracks"`,
/// or `None` for both). Thin pass-through to the client's
/// `get_user_purchases_page_typed` so the controller has a single service entry
/// point and never reaches into `qbz-qobuz` directly. Mirrors command #1's
/// single-page branch (both `limit` + `offset` present).
pub async fn get_user_purchases_page(
    client: &QobuzClient,
    limit: u32,
    offset: u32,
    kind: Option<&str>,
) -> QobuzResult<PurchaseResponse> {
    client
        .get_user_purchases_page_typed(kind, limit, offset)
        .await
}

/// Fetch ALL purchases (both types) by paginating through the Qobuz API.
/// Pass-through to the client's `get_user_purchases_all` (command #1's
/// paginate-all branch, used by search). The two-call per-type totals quirk is
/// preserved inside the client; this glue does not collapse it.
pub async fn get_user_purchases_all(client: &QobuzClient) -> QobuzResult<PurchaseResponse> {
    client.get_user_purchases_all().await
}

/// Fetch ALL purchases for a SINGLE type by paginating (`"albums"` /
/// `"tracks"`). Pass-through to `get_user_purchases_all_typed` — the primary
/// per-tab list-load path (command #3). The OTHER type's `total` is forced to 0
/// in the returned envelope (the root of the totals gotcha); the controller
/// recovers both totals via the two separate `get_ids(1,0,type)` calls in
/// `load_purchases_metadata`.
pub async fn get_user_purchases_by_type(
    client: &QobuzClient,
    purchase_type: &str,
) -> QobuzResult<PurchaseResponse> {
    client.get_user_purchases_all_typed(purchase_type).await
}

/// Read the per-type purchase TOTAL via a single `getUserPurchasesIds`
/// page (`limit=1, offset=0, type`). The items are opaque; only `.total` for
/// the matching type is read. Returns `None` on any error (the controller falls
/// back to 0 / the response length — `loadPurchasesMetadata`'s `.catch(()=>null)`).
///
/// GOTCHA (per-type totals): this MUST be called once per type. A single
/// unfiltered `limit=1` ids call carries only the FIRST type's total, so the
/// controller fires two of these — `get_purchase_total(client, "albums")` and
/// `get_purchase_total(client, "tracks")` — never one combined call.
pub async fn get_purchase_total(client: &QobuzClient, purchase_type: &str) -> Option<u32> {
    match client
        .get_user_purchases_ids_page_typed(Some(purchase_type), 1, 0)
        .await
    {
        Ok(resp) => match purchase_type {
            "albums" => Some(resp.albums.total),
            "tracks" => Some(resp.tracks.total),
            _ => None,
        },
        Err(e) => {
            log::warn!("[Purchases] get_purchase_total({purchase_type}) failed: {e}");
            None
        }
    }
}

/// Filter a `PurchaseResponse` in-memory by a search query. Pure — no I/O.
///
/// Ported byte-for-byte from `v2_filter_purchase_response`
/// (`src-tauri/src/commands_v2/legacy_compat.rs:2627`):
///   * the query is lowercased once;
///   * an album is RETAINED when its lowercased `title` OR `artist.name`
///     contains the query (case-insensitive substring);
///   * a track is RETAINED when its lowercased `title` OR `performer.name` OR
///     (if present) `album.title` contains the query;
///   * each surviving page's `total` is reset to its filtered `items.len()` and
///     `offset` is reset to 0 (`limit` is left untouched, matching the source).
///
/// No fuzzy matching, no ranking. An empty/whitespace query is handled by the
/// caller (it skips the filter entirely), so this fn always applies the
/// substring test as written.
pub fn filter_purchase_response(response: PurchaseResponse, query: &str) -> PurchaseResponse {
    let needle = query.to_lowercase();

    let albums: Vec<PurchaseAlbum> = response
        .albums
        .items
        .into_iter()
        .filter(|album| {
            album.title.to_lowercase().contains(&needle)
                || album.artist.name.to_lowercase().contains(&needle)
        })
        .collect();

    let tracks: Vec<PurchaseTrack> = response
        .tracks
        .items
        .into_iter()
        .filter(|track| {
            track.title.to_lowercase().contains(&needle)
                || track.performer.name.to_lowercase().contains(&needle)
                || track
                    .album
                    .as_ref()
                    .map(|a| a.title.to_lowercase().contains(&needle))
                    .unwrap_or(false)
        })
        .collect();

    PurchaseResponse {
        albums: SearchResultsPage {
            total: albums.len() as u32,
            offset: 0,
            limit: response.albums.limit,
            items: albums,
        },
        tracks: SearchResultsPage {
            total: tracks.len() as u32,
            offset: 0,
            limit: response.tracks.limit,
            items: tracks,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qbz_models::{Artist, AlbumSummary, PurchaseAlbum, PurchaseTrack, SearchResultsPage};

    fn album(title: &str, artist: &str) -> PurchaseAlbum {
        PurchaseAlbum {
            title: title.to_string(),
            artist: Artist {
                name: artist.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn track(title: &str, performer: &str, album_title: Option<&str>) -> PurchaseTrack {
        PurchaseTrack {
            title: title.to_string(),
            performer: Artist {
                name: performer.to_string(),
                ..Default::default()
            },
            album: album_title.map(|t| AlbumSummary {
                id: String::new(),
                title: t.to_string(),
                image: Default::default(),
                label: None,
                genre: None,
            }),
            ..Default::default()
        }
    }

    fn response(albums: Vec<PurchaseAlbum>, tracks: Vec<PurchaseTrack>) -> PurchaseResponse {
        PurchaseResponse {
            albums: SearchResultsPage {
                total: albums.len() as u32,
                offset: 7,
                limit: 500,
                items: albums,
            },
            tracks: SearchResultsPage {
                total: tracks.len() as u32,
                offset: 9,
                limit: 500,
                items: tracks,
            },
        }
    }

    #[test]
    fn filter_matches_album_title_case_insensitive() {
        let resp = response(
            vec![album("Kind of Blue", "Miles Davis"), album("Thriller", "Michael Jackson")],
            vec![],
        );
        let out = filter_purchase_response(resp, "BLUE");
        assert_eq!(out.albums.items.len(), 1);
        assert_eq!(out.albums.items[0].title, "Kind of Blue");
        // total reset to filtered length, offset reset to 0, limit preserved.
        assert_eq!(out.albums.total, 1);
        assert_eq!(out.albums.offset, 0);
        assert_eq!(out.albums.limit, 500);
    }

    #[test]
    fn filter_matches_album_artist_name() {
        let resp = response(
            vec![album("Thriller", "Michael Jackson"), album("Blue", "Davis")],
            vec![],
        );
        let out = filter_purchase_response(resp, "jackson");
        assert_eq!(out.albums.items.len(), 1);
        assert_eq!(out.albums.items[0].title, "Thriller");
    }

    #[test]
    fn filter_matches_track_title_performer_and_album() {
        let resp = response(
            vec![],
            vec![
                track("So What", "Miles Davis", Some("Kind of Blue")),
                track("Beat It", "Michael Jackson", Some("Thriller")),
                track("Random", "Nobody", None),
            ],
        );
        // performer match
        let by_performer = filter_purchase_response(resp.clone(), "jackson");
        assert_eq!(by_performer.tracks.items.len(), 1);
        assert_eq!(by_performer.tracks.items[0].title, "Beat It");

        // album-title match
        let by_album = filter_purchase_response(resp.clone(), "kind of blue");
        assert_eq!(by_album.tracks.items.len(), 1);
        assert_eq!(by_album.tracks.items[0].title, "So What");

        // title match
        let by_title = filter_purchase_response(resp, "random");
        assert_eq!(by_title.tracks.items.len(), 1);
        assert_eq!(by_title.tracks.items[0].title, "Random");
    }

    #[test]
    fn filter_track_with_no_album_does_not_panic() {
        let resp = response(vec![], vec![track("Solo", "Artist", None)]);
        let out = filter_purchase_response(resp, "solo");
        assert_eq!(out.tracks.items.len(), 1);
        // total/offset reset, limit preserved on the tracks page too.
        assert_eq!(out.tracks.total, 1);
        assert_eq!(out.tracks.offset, 0);
        assert_eq!(out.tracks.limit, 500);
    }

    #[test]
    fn filter_no_match_yields_empty_pages() {
        let resp = response(
            vec![album("Thriller", "Michael Jackson")],
            vec![track("Beat It", "Michael Jackson", Some("Thriller"))],
        );
        let out = filter_purchase_response(resp, "zzz-no-match");
        assert!(out.albums.items.is_empty());
        assert!(out.tracks.items.is_empty());
        assert_eq!(out.albums.total, 0);
        assert_eq!(out.tracks.total, 0);
    }
}
