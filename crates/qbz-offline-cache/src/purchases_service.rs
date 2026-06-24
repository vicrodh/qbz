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
//! `src-tauri/src/commands_v2/legacy_compat.rs:2627`).
//!
//! Slice 4 scope (pure, no I/O): `synth_formats` (the §4.9 client-side
//! format-synthesis table, ported from `v2_purchases_get_formats`
//! `legacy_compat.rs:2953`) and `apply_download_flags` (the §3.4 download-flag
//! annotation, ported from `v2_apply_purchase_download_flags`
//! `legacy_compat.rs:2594`).
//!
//! Slice 5 scope: the single-track download primitive
//! `download_purchase_track` (the canonical getFileUrl → CDN → `.part`→rename →
//! registry pipeline, ported from `v2_download_purchase_track_impl`
//! `legacy_compat.rs:2651` PLUS the registry write that `v2_purchases_download_track`
//! `legacy_compat.rs:3013` performs after it), and the pure path/extension
//! helpers `target_path` (§7.3 `v2_purchase_target_path`) and
//! `purchase_extension` (§7.1.5 `v2_purchase_extension`). The album loop, cancel,
//! and per-track progress live in the `qbz-slint` controller (Slice 7), not here
//! — this crate only exposes the single-track primitive.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use qbz_library::LibraryDatabase;
use qbz_models::{
    Album, PurchaseAlbum, PurchaseFormatOption, PurchaseResponse, PurchaseTrack, SearchResultsPage,
};
use qbz_qobuz::QobuzClient;
use qbz_qobuz::Result as QobuzResult;

use crate::metadata::sanitize_filename;

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

/// Synthesize the downloadable format options for a purchased album,
/// client-side from `/album/get` (command #6 `v2_purchases_get_formats`,
/// `legacy_compat.rs:2953-3001`). There is NO Qobuz formats endpoint — the
/// options are derived purely from `album.hires` + `album.maximum_sampling_rate`.
///
/// Order is load-bearing (it IS the dropdown order; the frontend default-selects
/// `formats[0]`, so the highest available quality is the default):
///   * id **27** `[FLAC][24-bit,192kHz]` — only if `hires && max_sr > 96.0`.
///   * id **7**  `[FLAC][24-bit,96kHz]`  — only if `hires`.
///   * id **6**  `[FLAC][16-bit,44.1kHz]` — always.
///   * id **5**  `[MP3][320kbps]`         — always.
///
/// The ids feed `getFileUrl`'s `format_id`; the `label` (with `/`→`-`) becomes
/// the `qualityDir` subfolder, so both ids AND label strings are reproduced
/// EXACTLY (port idéntico — these are not cosmetic).
pub fn synth_formats(album: &Album) -> Vec<PurchaseFormatOption> {
    let mut formats = Vec::new();

    if album.hires && album.maximum_sampling_rate.unwrap_or(0.0) > 96.0 {
        formats.push(PurchaseFormatOption {
            id: 27,
            label: "[FLAC][24-bit,192kHz]".to_string(),
            bit_depth: Some(24),
            sampling_rate: Some(192.0),
        });
    }

    if album.hires {
        formats.push(PurchaseFormatOption {
            id: 7,
            label: "[FLAC][24-bit,96kHz]".to_string(),
            bit_depth: Some(24),
            sampling_rate: Some(96.0),
        });
    }

    formats.push(PurchaseFormatOption {
        id: 6,
        label: "[FLAC][16-bit,44.1kHz]".to_string(),
        bit_depth: Some(16),
        sampling_rate: Some(44.1),
    });

    formats.push(PurchaseFormatOption {
        id: 5,
        label: "[MP3][320kbps]".to_string(),
        bit_depth: None,
        sampling_rate: None,
    });

    formats
}

/// Annotate a `PurchaseResponse` in-place with frontend-computed download flags
/// (§3.4 `v2_apply_purchase_download_flags`, `legacy_compat.rs:2594-2625`, used
/// by commands #1 / #4). Pure — no I/O. The frontend OVERRIDES any backend
/// `downloaded` value here.
///
/// Per track:
///   * `downloaded = downloaded_ids.contains(track.id)`;
///   * `downloaded_format_ids = format_map.get(track.id).cloned().unwrap_or_default()`.
///
/// Per album (the all-mode / by-type path where albums and tracks are sibling
/// pages): collect the ids of `response.tracks.items` whose
/// `track.album.id == album.id`; then
/// `album.downloaded = !ids.is_empty() && all ids ∈ downloaded_ids`.
/// An album with NO matching tracks in this response → `downloaded = false`
/// (the empty-set rule; partial-page albums may flip to not-downloaded — this
/// is the documented page-mode gotcha and is replicated verbatim).
///
/// `downloaded_ids`/`format_map` are keyed by `i64` (registry track ids); track
/// ids are `u64` and compared via `track.id as i64`, exactly as the source.
pub fn apply_download_flags(
    response: &mut PurchaseResponse,
    downloaded_ids: &HashSet<i64>,
    format_map: &HashMap<i64, Vec<u32>>,
) {
    for track in &mut response.tracks.items {
        let tid = track.id as i64;
        track.downloaded = downloaded_ids.contains(&tid);
        track.downloaded_format_ids = format_map.get(&tid).cloned().unwrap_or_default();
    }

    for album in &mut response.albums.items {
        let album_track_ids: Vec<i64> = response
            .tracks
            .items
            .iter()
            .filter(|track| {
                track
                    .album
                    .as_ref()
                    .map(|album_ref| album_ref.id == album.id)
                    .unwrap_or(false)
            })
            .map(|track| track.id as i64)
            .collect();

        album.downloaded = !album_track_ids.is_empty()
            && album_track_ids
                .iter()
                .all(|track_id| downloaded_ids.contains(track_id));
    }
}

/// Pick the on-disk file extension for a purchased track from the RESPONSE
/// stream's `format_id` / `mime_type` (§7.1.5, ported byte-for-byte from
/// `v2_purchase_extension` `legacy_compat.rs:2553-2559`):
///   * `"mp3"` if the served `format_id == 5` OR the served `mime_type`
///     contains `"mpeg"`;
///   * `"flac"` otherwise.
///
/// IMPORTANT (Addendum B.2): the extension keys off the RESPONSE's served
/// format, NOT the requested one. If Qobuz downgrades (e.g. you asked for 27 but
/// it serves an MP3), the file gets the served extension while the registry
/// records the REQUESTED `format_id` (see `download_purchase_track`). Do NOT
/// reconcile the two — the Tauri app does not.
pub fn purchase_extension(format_id: u32, mime_type: &str) -> &'static str {
    if format_id == 5 || mime_type.contains("mpeg") {
        "mp3"
    } else {
        "flac"
    }
}

/// Build the deterministic on-disk target path for a purchased track
/// (§7.3, ported byte-for-byte from `v2_purchase_target_path`
/// `legacy_compat.rs:2561-2592`):
///   `{destination}/{artist_dir}/{album_dir}/{file_name}`
/// where
///   * `artist_dir = sanitize_filename(artist_name)`;
///   * `album_dir = sanitize_filename(album_title)` and, if `quality_dir` is
///     non-empty, `format!("{album_clean} {sanitize(quality_dir)}")` (a single
///     space joins them — this is the `"Album [FLAC][24-bit,96kHz]"` folder);
///   * `file_name = "{NN} - {title_clean}.{ext}"` (`NN` = zero-padded `{:02}`)
///     when `track_number > 0`, else `"{title_clean}.{ext}"`.
///
/// All three segments are run through the SHARED `sanitize_filename` (§7.4) so
/// the path matches what the library scan + Add-to-Library expect (the `[`/`]`
/// in the quality label become `-`, brackets collapse). The caller passes the
/// already-`'/'→'-'`-transformed `quality_dir` (§7.5 `qualityDir` derivation);
/// the re-sanitize here is idempotent for `/` and additionally strips brackets.
pub fn target_path(
    destination: &str,
    artist_name: &str,
    album_title: &str,
    quality_dir: &str,
    track_number: u32,
    track_title: &str,
    ext: &str,
) -> PathBuf {
    let artist_dir = sanitize_filename(artist_name);
    let album_clean = sanitize_filename(album_title);
    let title_clean = sanitize_filename(track_title);

    let file_name = if track_number > 0 {
        format!("{:02} - {}.{}", track_number, title_clean, ext)
    } else {
        format!("{}.{}", title_clean, ext)
    };

    // Embed quality in album folder name: "Album [FLAC][24-bit,96kHz]".
    let album_dir = if !quality_dir.is_empty() {
        let quality_clean = sanitize_filename(quality_dir);
        format!("{} {}", album_clean, quality_clean)
    } else {
        album_clean
    };

    PathBuf::from(destination)
        .join(artist_dir)
        .join(album_dir)
        .join(file_name)
}

/// I/O tail of the single-track download: given the already-fetched audio
/// `data` and the resolved track/stream metadata, derive the extension from the
/// RESPONSE format, build the target path, `create_dir_all`, write the `.part`
/// file, `fs::rename` to final, then write the registry row with the REQUESTED
/// `format_id`. Returns the final on-disk path string.
///
/// Split out from `download_purchase_track` purely so the filesystem +
/// registry ordering (Addendum B.1/B.2/B.3) is unit-testable without a live
/// HTTP client. The behavior is the EXACT concatenation of
/// `v2_download_purchase_track_impl`'s write tail (`legacy_compat.rs:2681-2701`)
/// and `v2_purchases_download_track`'s registry write (`:3019`).
///
/// Ordering & failure semantics (Addendum B.1 — replicated verbatim):
///   1. write `target.with_extension("{ext}.part")` → `2. fs::rename` to final
///      → `3. mark_purchase_downloaded`.
///   If the file write/rename SUCCEEDS but the registry write FAILS, this
///   returns `Err` with the file LEFT ON DISK (orphaned, no registry row). Do
///   NOT roll back the file or treat the registry failure as success.
///
/// No collision preflight (Addendum B.3): `.part`→`fs::rename` overwrites any
/// pre-existing final file or stale `.part` silently.
/// Filesystem-only tail: derive the extension from the RESPONSE format, build
/// the target path, `create_dir_all`, write the `.part`, `fs::rename` to final,
/// and return the final on-disk path string. Does **NOT** touch the registry.
///
/// This is the shared write core. The album loop uses it directly (so a registry
/// failure does NOT mark the track `Failed`; the album loop instead does a
/// SEPARATE best-effort registry write that swallows the error — Svelte
/// `markTrackDownloaded(...).catch(()=>{})`, §B.1 album-path semantics). The
/// single-track bundled path (`write_and_register_track`) wraps this and then
/// propagates a registry error so the track shows `Failed` (§B.1 single-track
/// semantics).
///
/// Addendum B.2: extension/MIME from the RESPONSE; Addendum B.3: silent overwrite.
#[allow(clippy::too_many_arguments)]
fn write_track_file(
    data: &[u8],
    artist_name: &str,
    album_title: &str,
    quality_dir: &str,
    track_number: u32,
    track_title: &str,
    response_format_id: u32,
    response_mime_type: &str,
    destination: &str,
) -> Result<String, String> {
    // Addendum B.2: extension derives from the RESPONSE's served format.
    let extension = purchase_extension(response_format_id, response_mime_type);
    let target = target_path(
        destination,
        artist_name,
        album_title,
        quality_dir,
        track_number,
        track_title,
        extension,
    );

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create destination folder: {}", e))?;
    }

    // Addendum B.3: write `.part`, then rename — no overwrite preflight.
    let temp_path = target.with_extension(format!("{}.part", extension));
    std::fs::write(&temp_path, data).map_err(|e| format!("Failed to write temporary file: {}", e))?;
    std::fs::rename(&temp_path, &target).map_err(|e| format!("Failed to finalize file: {}", e))?;

    Ok(target.to_string_lossy().to_string())
}

#[allow(clippy::too_many_arguments)]
fn write_and_register_track(
    db: &LibraryDatabase,
    track_id: u64,
    requested_format_id: u32,
    data: &[u8],
    artist_name: &str,
    album_title: &str,
    quality_dir: &str,
    track_number: u32,
    track_title: &str,
    response_format_id: u32,
    response_mime_type: &str,
    destination: &str,
) -> Result<String, String> {
    let file_path = write_track_file(
        data,
        artist_name,
        album_title,
        quality_dir,
        track_number,
        track_title,
        response_format_id,
        response_mime_type,
        destination,
    )?;

    // Addendum B.1/B.2: registry write AFTER the file is on disk, with the
    // REQUESTED format_id (album_id None for single-track downloads). A registry
    // failure here returns Err while the file stays on disk (orphaned).
    db.mark_purchase_downloaded(
        track_id as i64,
        None,
        &file_path,
        requested_format_id as i64,
    )
    .map_err(|e| e.to_string())?;

    Ok(file_path)
}

/// The single canonical single-track download primitive (Slice 5).
///
/// Ported from `v2_download_purchase_track_impl` (`legacy_compat.rs:2651-2702`)
/// combined with the registry write of `v2_purchases_download_track`
/// (`:3013-3022`). Sequence:
///   1. `client.get_track(track_id)` → metadata. Error → `"Failed to fetch track
///      {id}: {e}"`.
///   2. `client.get_track_file_url_by_format(track_id, format_id)` → SIGNED
///      `StreamUrl` (intent=stream). Error → `"Failed to get download URL for
///      track {id}: {e}"`. (In the reference the client lock is dropped here; in
///      this crate the caller holds the `QobuzClient` by `&`, so there is no lock
///      to drop — the read guard is released by the controller before the
///      multi-minute CDN fetch. No behavioral divergence in the bytes path.)
///   3. `QobuzClient::download_audio(&stream.url)` → `Vec<u8>` (HTTP/1.1-only, no
///      total timeout — see `qbz-qobuz`).
///   4. Resolve names: artist = `track.performer.name` else `"Unknown Artist"`;
///      album = `track.album.title` else `"Singles"`.
///   5. Extension from RESPONSE `stream.format_id`/`mime_type` (B.2); path via
///      `target_path`; `.part`→rename; registry write with REQUESTED `format_id`.
///
/// `quality_dir` is the UI-selected format label with `'/'→'-'` already applied
/// (§7.5); it becomes the album-folder quality suffix AND the registry's quality
/// dimension is the REQUESTED format (B.2). Returns the final on-disk path (the
/// controller uses the FIRST track's returned path to rewrite the album-download
/// destination to the album folder — Slice 7).
///
/// Addendum B.5: only `stream.url` / `stream.format_id` / `stream.mime_type` are
/// consumed; `stream.restrictions` is IGNORED (no restriction-based blocking).
pub async fn download_purchase_track(
    client: &QobuzClient,
    db: &LibraryDatabase,
    track_id: u64,
    format_id: u32,
    destination: &str,
    quality_dir: &str,
) -> Result<String, String> {
    let track = client
        .get_track(track_id)
        .await
        .map_err(|e| format!("Failed to fetch track {}: {}", track_id, e))?;
    let stream = client
        .get_track_file_url_by_format(track_id, format_id)
        .await
        .map_err(|e| format!("Failed to get download URL for track {}: {}", track_id, e))?;

    let data = QobuzClient::download_audio(&stream.url).await?;

    let artist_name = track
        .performer
        .as_ref()
        .map(|artist| artist.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album_title = track
        .album
        .as_ref()
        .map(|album| album.title.clone())
        .unwrap_or_else(|| "Singles".to_string());

    write_and_register_track(
        db,
        track_id,
        format_id,
        &data,
        &artist_name,
        &album_title,
        quality_dir,
        track.track_number,
        &track.title,
        stream.format_id,
        &stream.mime_type,
        destination,
    )
}

/// Download-ONLY variant of the single-track primitive: identical CDN fetch +
/// `.part`→rename pipeline as `download_purchase_track`, but it does **NOT**
/// write the `downloaded_purchases` registry. Returns the final on-disk path.
///
/// This exists for the ALBUM loop (`qbz-slint::execute_album_download`), which
/// matches Svelte `executeAlbumDownload` (`purchaseDownloadStore.ts:130-147`):
/// the album loop marks the track `'complete'` on download success FIRST, then
/// performs the registry write as a SEPARATE best-effort step that SWALLOWS
/// failure (`await markTrackDownloaded(...).catch(() => {})`). So a registry-write
/// failure during an album download leaves the track `'complete'` (file on disk,
/// just unregistered) — UNLIKE the single-track path, where a registry error
/// propagates and the track shows `'failed'` (§B.1).
///
/// The caller is responsible for the best-effort registry write afterwards
/// (`db.mark_purchase_downloaded(track_id, Some(album_id), &file_path,
/// format_id)`), ignoring its error. Addendum B.2 (extension from RESPONSE,
/// registry/qualityDir from REQUESTED), B.3 (silent overwrite), and B.5
/// (restrictions ignored) all hold identically to `download_purchase_track`.
pub async fn download_purchase_track_file_only(
    client: &QobuzClient,
    track_id: u64,
    format_id: u32,
    destination: &str,
    quality_dir: &str,
) -> Result<String, String> {
    let track = client
        .get_track(track_id)
        .await
        .map_err(|e| format!("Failed to fetch track {}: {}", track_id, e))?;
    let stream = client
        .get_track_file_url_by_format(track_id, format_id)
        .await
        .map_err(|e| format!("Failed to get download URL for track {}: {}", track_id, e))?;

    let data = QobuzClient::download_audio(&stream.url).await?;

    let artist_name = track
        .performer
        .as_ref()
        .map(|artist| artist.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album_title = track
        .album
        .as_ref()
        .map(|album| album.title.clone())
        .unwrap_or_else(|| "Singles".to_string());

    write_track_file(
        &data,
        &artist_name,
        &album_title,
        quality_dir,
        track.track_number,
        &track.title,
        stream.format_id,
        &stream.mime_type,
        destination,
    )
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

    // ── Slice 4: synth_formats ───────────────────────────────────────────

    // `Album` has no `Default`; build the minimal shape from JSON (relying on
    // the serde defaults / Option fields) so we never reach into qbz-models.
    fn album_with(hires: bool, max_sr: Option<f64>) -> Album {
        let json = match max_sr {
            Some(sr) => format!(r#"{{"hires":{hires},"maximum_sampling_rate":{sr}}}"#),
            None => format!(r#"{{"hires":{hires}}}"#),
        };
        serde_json::from_str(&json).expect("minimal Album JSON deserializes")
    }

    #[test]
    fn synth_formats_24_192_yields_four_options_in_order() {
        // hires + max_sr > 96 → all four, highest first, index 0 = the 192k default.
        let fmts = synth_formats(&album_with(true, Some(192.0)));
        assert_eq!(fmts.len(), 4);
        let ids: Vec<u32> = fmts.iter().map(|f| f.id).collect();
        assert_eq!(ids, vec![27, 7, 6, 5]);
        // Exact labels are load-bearing (feed qualityDir + the dropdown).
        assert_eq!(fmts[0].label, "[FLAC][24-bit,192kHz]");
        assert_eq!(fmts[1].label, "[FLAC][24-bit,96kHz]");
        assert_eq!(fmts[2].label, "[FLAC][16-bit,44.1kHz]");
        assert_eq!(fmts[3].label, "[MP3][320kbps]");
        // bit_depth / sampling_rate carried verbatim.
        assert_eq!((fmts[0].bit_depth, fmts[0].sampling_rate), (Some(24), Some(192.0)));
        assert_eq!((fmts[1].bit_depth, fmts[1].sampling_rate), (Some(24), Some(96.0)));
        assert_eq!((fmts[2].bit_depth, fmts[2].sampling_rate), (Some(16), Some(44.1)));
        assert_eq!((fmts[3].bit_depth, fmts[3].sampling_rate), (None, None));
        // default-select is index 0 (highest available).
        assert_eq!(fmts[0].id, 27);
    }

    #[test]
    fn synth_formats_24_96_drops_192_option() {
        // hires but max_sr exactly 96 (not > 96) → no id 27.
        let fmts = synth_formats(&album_with(true, Some(96.0)));
        let ids: Vec<u32> = fmts.iter().map(|f| f.id).collect();
        assert_eq!(ids, vec![7, 6, 5]);
        assert_eq!(fmts[0].id, 7);
    }

    #[test]
    fn synth_formats_hires_with_no_sampling_rate_drops_192() {
        // max_sr None → unwrap_or(0.0) → not > 96 → no id 27, but hires keeps id 7.
        let fmts = synth_formats(&album_with(true, None));
        let ids: Vec<u32> = fmts.iter().map(|f| f.id).collect();
        assert_eq!(ids, vec![7, 6, 5]);
    }

    #[test]
    fn synth_formats_non_hires_yields_only_cd_and_mp3() {
        // Not hires → only the always-present 6 + 5; max_sr is irrelevant.
        let fmts = synth_formats(&album_with(false, Some(192.0)));
        let ids: Vec<u32> = fmts.iter().map(|f| f.id).collect();
        assert_eq!(ids, vec![6, 5]);
        assert_eq!(fmts[0].id, 6);
    }

    // ── Slice 4: apply_download_flags ────────────────────────────────────

    fn album_id(id: &str) -> PurchaseAlbum {
        PurchaseAlbum {
            id: id.to_string(),
            ..Default::default()
        }
    }

    fn track_for_album(id: u64, album_id: &str) -> PurchaseTrack {
        PurchaseTrack {
            id,
            album: Some(AlbumSummary {
                id: album_id.to_string(),
                title: String::new(),
                image: Default::default(),
                label: None,
                genre: None,
            }),
            ..Default::default()
        }
    }

    fn dl_ids(ids: &[i64]) -> HashSet<i64> {
        ids.iter().copied().collect()
    }

    #[test]
    fn apply_flags_marks_track_downloaded_and_records_format_ids() {
        let mut resp = response(vec![], vec![track_for_album(10, "a1"), track_for_album(20, "a1")]);
        let downloaded = dl_ids(&[10]);
        let mut format_map: HashMap<i64, Vec<u32>> = HashMap::new();
        format_map.insert(10, vec![27, 6]);

        apply_download_flags(&mut resp, &downloaded, &format_map);

        assert!(resp.tracks.items[0].downloaded);
        assert_eq!(resp.tracks.items[0].downloaded_format_ids, vec![27, 6]);
        // track not in dlIds → not downloaded, empty format ids.
        assert!(!resp.tracks.items[1].downloaded);
        assert!(resp.tracks.items[1].downloaded_format_ids.is_empty());
    }

    #[test]
    fn apply_flags_album_downloaded_when_all_nested_track_ids_present() {
        // Every track whose album.id == "a1" is in dlIds → album downloaded.
        let mut resp = response(
            vec![album_id("a1")],
            vec![track_for_album(10, "a1"), track_for_album(20, "a1")],
        );
        apply_download_flags(&mut resp, &dl_ids(&[10, 20]), &HashMap::new());
        assert!(resp.albums.items[0].downloaded);
    }

    #[test]
    fn apply_flags_album_not_downloaded_when_partially_owned() {
        // One of the two album tracks missing from dlIds → album NOT downloaded.
        let mut resp = response(
            vec![album_id("a1")],
            vec![track_for_album(10, "a1"), track_for_album(20, "a1")],
        );
        apply_download_flags(&mut resp, &dl_ids(&[10]), &HashMap::new());
        assert!(!resp.albums.items[0].downloaded);
    }

    #[test]
    fn apply_flags_album_with_no_matching_tracks_is_not_downloaded() {
        // No tracks reference this album (empty set rule) → false, never panic.
        let mut resp = response(vec![album_id("a1")], vec![track_for_album(10, "other")]);
        apply_download_flags(&mut resp, &dl_ids(&[10]), &HashMap::new());
        assert!(!resp.albums.items[0].downloaded);
    }

    #[test]
    fn apply_flags_frontend_overrides_stale_backend_downloaded() {
        // Backend wrongly set downloaded=true; frontend recomputes to false.
        let mut track = track_for_album(10, "a1");
        track.downloaded = true;
        track.downloaded_format_ids = vec![99];
        let mut backend_true_album = album_id("a1");
        backend_true_album.downloaded = true;

        let mut resp = response(vec![backend_true_album], vec![track]);
        // dlIds empty → both must be overridden to false / cleared.
        apply_download_flags(&mut resp, &dl_ids(&[]), &HashMap::new());
        assert!(!resp.tracks.items[0].downloaded);
        assert!(resp.tracks.items[0].downloaded_format_ids.is_empty());
        assert!(!resp.albums.items[0].downloaded);
    }

    // ── Slice 5: purchase_extension ──────────────────────────────────────

    #[test]
    fn purchase_extension_mp3_when_format_id_5() {
        // RESPONSE format_id 5 → mp3 regardless of mime.
        assert_eq!(purchase_extension(5, "audio/flac"), "mp3");
    }

    #[test]
    fn purchase_extension_mp3_when_mime_contains_mpeg() {
        // mime contains "mpeg" → mp3 even if the served id is a FLAC id.
        assert_eq!(purchase_extension(27, "audio/mpeg"), "mp3");
    }

    #[test]
    fn purchase_extension_flac_otherwise() {
        assert_eq!(purchase_extension(27, "audio/flac"), "flac");
        assert_eq!(purchase_extension(7, ""), "flac");
        assert_eq!(purchase_extension(6, "application/octet-stream"), "flac");
    }

    // ── Slice 5: target_path ─────────────────────────────────────────────

    #[test]
    fn target_path_full_template_with_quality_and_track_number() {
        // {dest}/{artist}/{album [quality]}/{NN - title.ext}; quality joined by a
        // single space; sanitize strips the `[`/`]` brackets to `-` and collapses.
        let p = target_path(
            "/music",
            "Miles Davis",
            "Kind of Blue",
            "[FLAC][24-bit,96kHz]",
            3,
            "So What",
            "flac",
        );
        // sanitize: "[FLAC][24-bit,96kHz]" → brackets→`-`, collapsed/trimmed.
        let expected = PathBuf::from("/music")
            .join("Miles Davis")
            .join(format!("Kind of Blue {}", sanitize_filename("[FLAC][24-bit,96kHz]")))
            .join("03 - So What.flac");
        assert_eq!(p, expected);
        // zero-padding is two digits.
        assert!(p.to_string_lossy().contains("/03 - So What.flac"));
    }

    #[test]
    fn target_path_no_quality_dir_uses_bare_album_folder() {
        let p = target_path("/d", "Artist", "Album", "", 1, "Title", "flac");
        assert_eq!(p, PathBuf::from("/d").join("Artist").join("Album").join("01 - Title.flac"));
    }

    #[test]
    fn target_path_zero_track_number_drops_number_prefix() {
        let p = target_path("/d", "Artist", "Album", "", 0, "Title", "mp3");
        assert_eq!(p, PathBuf::from("/d").join("Artist").join("Album").join("Title.mp3"));
    }

    #[test]
    fn target_path_unknown_artist_and_singles_fallbacks_sanitize() {
        // The "Unknown Artist"/"Singles" fallbacks are applied by the caller;
        // here verify they round-trip through sanitize unchanged (ASCII alnum +
        // spaces survive).
        let p = target_path("/d", "Unknown Artist", "Singles", "", 0, "Loose Track", "flac");
        assert_eq!(
            p,
            PathBuf::from("/d").join("Unknown Artist").join("Singles").join("Loose Track.flac")
        );
    }

    // ── Slice 5: write_and_register_track (filesystem + registry I/O) ─────

    fn open_temp_db(dir: &std::path::Path) -> LibraryDatabase {
        LibraryDatabase::open(&dir.join("library.db")).expect("open temp library db")
    }

    #[test]
    fn write_and_register_writes_part_then_renames_and_records_requested_format() {
        let tmp = tempfile::tempdir().unwrap();
        let db = open_temp_db(tmp.path());
        let dest = tmp.path().join("downloads");
        let data = b"FLACfakebytes".to_vec();

        // Requested format 27 (192k FLAC); RESPONSE downgraded to id 6 FLAC.
        let path = write_and_register_track(
            &db,
            /*track_id*/ 4242,
            /*requested_format_id*/ 27,
            &data,
            "Miles Davis",
            "Kind of Blue",
            /*quality_dir*/ "[FLAC][24-bit,192kHz]",
            /*track_number*/ 5,
            "So What",
            /*response_format_id*/ 6,
            /*response_mime_type*/ "audio/flac",
            dest.to_str().unwrap(),
        )
        .expect("write+register succeeds");

        // Final file exists with the RESPONSE-derived extension (flac), the `.part`
        // is gone, and the path embeds the REQUESTED-quality folder.
        let final_path = PathBuf::from(&path);
        assert!(final_path.exists(), "final file must exist");
        assert!(final_path.to_string_lossy().ends_with("05 - So What.flac"));
        assert!(!final_path.with_extension("flac.part").exists(), "`.part` removed after rename");
        assert_eq!(std::fs::read(&final_path).unwrap(), data);

        // Registry recorded the REQUESTED format (27), NOT the served 6 (B.2).
        let formats = db.get_downloaded_purchase_formats().unwrap();
        assert!(formats.contains(&(4242, 27)), "registry keys off REQUESTED format: {formats:?}");
        assert!(!formats.iter().any(|&(tid, fid)| tid == 4242 && fid == 6));
    }

    #[test]
    fn write_and_register_response_mp3_uses_mp3_extension() {
        // B.2: extension follows the RESPONSE (served id 5 → mp3) even though the
        // requested format was a FLAC id; registry still records the requested id.
        let tmp = tempfile::tempdir().unwrap();
        let db = open_temp_db(tmp.path());
        let dest = tmp.path().join("dl");

        let path = write_and_register_track(
            &db,
            1,
            /*requested*/ 7,
            b"x",
            "A",
            "B",
            "",
            1,
            "T",
            /*response*/ 5,
            "audio/mpeg",
            dest.to_str().unwrap(),
        )
        .unwrap();
        assert!(path.ends_with("01 - T.mp3"), "served mp3 → .mp3 extension: {path}");
        let formats = db.get_downloaded_purchase_formats().unwrap();
        assert!(formats.contains(&(1, 7)), "requested format 7 recorded: {formats:?}");
    }

    #[test]
    fn write_and_register_silently_overwrites_existing_final_file() {
        // B.3: no collision preflight — a pre-existing final file is replaced
        // without prompt or `(1)` disambiguation.
        let tmp = tempfile::tempdir().unwrap();
        let db = open_temp_db(tmp.path());
        let dest = tmp.path().join("dl");

        let first = write_and_register_track(
            &db, 9, 6, b"old", "A", "Alb", "", 2, "Song", 6, "audio/flac", dest.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(std::fs::read(&first).unwrap(), b"old");

        // Second write to the SAME deterministic path with new bytes overwrites.
        let second = write_and_register_track(
            &db, 9, 6, b"new", "A", "Alb", "", 2, "Song", 6, "audio/flac", dest.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(first, second, "same deterministic target path");
        assert_eq!(std::fs::read(&second).unwrap(), b"new", "silent overwrite");
    }

    #[test]
    fn write_and_register_registry_failure_leaves_file_orphaned() {
        // B.1: write file → rename → registry; if the registry write FAILS after a
        // successful file write, return Err with the file LEFT ON DISK (orphaned).
        //
        // Inject a real registry failure WITHOUT touching `qbz-library`'s API: open
        // a normal DB, then DROP the `downloaded_purchases` table via a SECOND
        // connection to the same file. The held `LibraryDatabase` connection then
        // sees "no such table" on its INSERT → registry write fails, while the
        // filesystem write (to a separate destination dir) still succeeds.
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("library.db");
        let db = LibraryDatabase::open(&db_path).unwrap();

        // Drop the registry table out from under the open connection.
        {
            let saboteur = rusqlite::Connection::open(&db_path).unwrap();
            saboteur
                .execute_batch("PRAGMA journal_mode=WAL; DROP TABLE downloaded_purchases;")
                .unwrap();
            // Checkpoint so the held connection observes the drop.
            let _ = saboteur.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }

        let dest = tmp.path().join("downloads");
        let res = write_and_register_track(
            &db, 77, 6, b"bytes", "A", "Alb", "", 1, "Song", 6, "audio/flac", dest.to_str().unwrap(),
        );

        // Registry INSERT failed (no such table) → Err.
        assert!(res.is_err(), "registry write failure must surface as Err: {res:?}");
        // ...but the file write happened BEFORE the registry write → orphaned file.
        let orphan = dest.join("A").join("Alb").join("01 - Song.flac");
        assert!(orphan.exists(), "file left on disk after registry failure (orphaned, B.1)");
        assert!(!orphan.with_extension("flac.part").exists(), "`.part` already renamed away");
    }
}
