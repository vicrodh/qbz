//! Album detail controller.
//!
//! Fetches a full album through `QbzCore`, maps it to plain (Send) data
//! on the worker thread, and applies it to the `AlbumState` global on the
//! Slint event loop.

use std::cell::RefCell;
use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{Album, Track};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{AlbumCardItem, AlbumState, AppWindow, ArtistCredit, DiscoverSection, TrackItem};

thread_local! {
    /// The current album's full, unfiltered track list — kept so the
    /// track search can filter against it without a re-fetch. UI thread
    /// only, hence `thread_local`.
    static FULL_TRACKS: RefCell<Vec<TrackItem>> = RefCell::new(Vec::new());
    /// The current album's RAW catalog tracks (qbz_models::Track), kept so
    /// the multi-select bulk actions (enqueue / cache) can resolve the
    /// selected rows to full Track objects without a re-fetch — mirrors the
    /// `play` Vec the favorites tab keeps. UI thread only.
    static PLAY_TRACKS: RefCell<Vec<Track>> = RefCell::new(Vec::new());
}

/// One credited album artist for the header credit line (E1). Plain/`Send`.
pub struct ArtistCreditData {
    pub id: String,
    pub name: String,
    /// Localized role suffix ("" for the main artist(s)).
    pub role: String,
}

/// Plain, `Send` album data produced on the worker thread.
pub struct AlbumData {
    pub id: String,
    pub title: String,
    /// Primary interpreter (back-compat: now-playing, fallbacks).
    pub artist: String,
    pub artist_id: String,
    /// Full credited-artist list with roles for the header credit line.
    pub artists: Vec<ArtistCreditData>,
    /// Pre-formatted "year • label • genre • N tracks • duration".
    pub info_line: String,
    /// Meta-line segment BEFORE the label (the year) — rendered with the
    /// label as a clickable link in the header.
    pub meta_pre: String,
    /// Meta-line segment AFTER the label (genre • N tracks • duration).
    pub meta_post: String,
    pub quality_tier: String,
    /// "24-bit / 96 kHz" — the quality-badge detail line.
    pub quality_detail: String,
    /// Editorial description / review (HTML stripped). May be empty.
    pub description: String,
    /// Short, truncated description for the header (full text in a modal).
    pub description_short: String,
    /// Half-length truncation used when the content area is space-constrained.
    pub description_shorter: String,
    pub artwork_url: String,
    /// Record label name, for the sidebar (empty when unknown).
    pub label: String,
    /// Record label id, so the sidebar label card can navigate to the label
    /// page ("" when unknown).
    pub label_id: String,
    /// Editorial awards for the sidebar, as `(id, name)` pairs. `id` may be
    /// "" — some /album/get entries omit it; the award controller then
    /// resolves it by name on click. Mirrors Tauri's `AlbumAward { id?, name }`.
    pub awards: Vec<(String, String)>,
    /// True when the album bundles a downloadable booklet/liner-notes PDF
    /// (Qobuz goodies) — gates the header booklet button.
    pub has_booklet: bool,
    /// URL of the booklet PDF goody (the controller downloads + rasterizes it
    /// on demand). Empty when the album bundles no booklet.
    pub booklet_url: String,
    pub tracks: Vec<TrackData>,
    /// Raw catalog tracks, kept for the multi-select bulk actions.
    pub raw_tracks: Vec<Track>,
}

pub struct TrackData {
    pub id: String,
    pub number: String,
    pub title: String,
    pub artist: String,
    /// Performer id for the clickable artist link ("" = plain text).
    pub artist_id: String,
    /// Album id for the clickable album link ("" = plain text). Album view
    /// leaves this empty (its rows belong to the album being viewed, so the
    /// apply layer stamps the viewed album's id); artist top-tracks set it
    /// per-track since they span different albums.
    pub album_id: String,
    pub duration: String,
    pub quality_tier: String,
    pub quality_detail: String,
    pub explicit: bool,
    /// Disc/media number (Qobuz `media_number`, defaulting to 1 when absent).
    /// Used after mapping to decide where the "Disc N" headers fall when the
    /// album spans more than one disc.
    pub disc: u32,
    /// Classical work TITLE (e.g. "Symphony No. 9"), or "" when the track
    /// carries no `work` metadata. Used after mapping to run-length stamp the
    /// per-work headers (PR #536). E3: the composer is split out below so the
    /// view can render its name as a clickable artist link.
    pub work: String,
    /// Work composer display name ("" when none); shown in the work header's
    /// parentheses as a link.
    pub work_composer_name: String,
    /// Work composer artist id ("" => the name renders as plain text).
    pub work_composer_id: String,
}

/// Fetch and map a full album by id.
pub async fn load_album<A>(runtime: &Arc<AppRuntime<A>>, album_id: &str) -> Result<AlbumData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let album = runtime
        .core()
        .get_album(album_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(map_album(album))
}

/// Localize an ISO `YYYY-MM-DD` release date to a short readable form
/// ("Feb 19, 2026"), via the active locale. Empty when absent or unparseable
/// (the header simply omits the date segment, as before). Mirrors the
/// `info_modals` date formatter but with the short month (`%b`).
fn format_release_date(iso: Option<&str>) -> String {
    let Some(raw) = iso.map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let head = raw.get(0..10).unwrap_or(raw);
    chrono::NaiveDate::parse_from_str(head, "%Y-%m-%d")
        .map(|d| {
            d.format_localized("%b %-d, %Y", crate::dates::current_locale())
                .to_string()
        })
        .unwrap_or_default()
}

/// Localized role suffix for a credit (E1). "" for the main artist (no suffix);
/// otherwise the first non-`main-artist` role, localized (e.g. "compositor").
fn credit_role(roles: Option<&Vec<String>>) -> String {
    let Some(roles) = roles else {
        return String::new();
    };
    roles
        .iter()
        .find(|r| r.as_str() != "main-artist")
        .map(|r| qbz_i18n::t(&qbz_qobuz::performers::format_role_label(r)))
        .unwrap_or_default()
}

/// Build the header credit line (E1): every credited artist with its role,
/// falling back to the single primary interpreter when the album carries no
/// `artists[]` array (some V2/discover shapes). A single album-level composer
/// is appended last.
///
/// This mirrors the official web player's `releaseArtistsMapper` exactly:
/// `mergeRoles([...album.artists, fallbackArtist, composerMapper(album.composer)])`.
/// The composer leg comes from the album-level `composer` field (a single
/// Artist), NOT from the per-track `composer` — deriving from the tracklist
/// over-credits every songwriter on non-classical albums (the player shows no
/// composer for e.g. Anthrax). The player also drops the composer when its
/// name is the localized "Various Composers" placeholder, detected by the
/// case-insensitive "VARIOUS" substring (bundle module 80145 / `hasAlbumComposer`).
fn build_credits(album: &Album) -> Vec<ArtistCreditData> {
    let mut credits: Vec<ArtistCreditData> = match album.artists.as_ref().filter(|v| !v.is_empty())
    {
        Some(list) => list
            .iter()
            .map(|a| ArtistCreditData {
                id: a.id.to_string(),
                name: a.name.clone(),
                role: credit_role(a.roles.as_ref()),
            })
            .collect(),
        None => vec![ArtistCreditData {
            id: album.artist.id.to_string(),
            name: album.artist.name.clone(),
            role: String::new(),
        }],
    };

    // Append the album-level composer (mergeRoles-style: dedup by id, skip the
    // "Various Composers" placeholder).
    if let Some(comp) = album.composer.as_ref() {
        let id = comp.id.to_string();
        let already_credited = credits.iter().any(|c| c.id == id);
        let is_various = comp.name.to_uppercase().contains("VARIOUS");
        if !comp.name.is_empty() && !is_various && !already_credited {
            credits.push(ArtistCreditData {
                id,
                name: comp.name.clone(),
                role: qbz_i18n::t("Composer"),
            });
        }
    }
    credits
}

fn map_album(album: Album) -> AlbumData {
    let artist = album.artist.name.clone();
    let artist_id = album.artist.id.to_string();
    let artists = build_credits(&album);

    // Full readable release date ("Feb 19, 2026"); was year-only before.
    // Prefer the flat ISO field, fall back to the nested V2 `dates.original`.
    let date_display = format_release_date(
        album
            .release_date_original
            .as_deref()
            .or_else(|| album.dates.as_ref().and_then(|d| d.original.as_deref())),
    );

    // Meta line, split around the label so the header can render the label
    // as a clickable link (Tauri's `date • [label] • genre • N tracks •
    // duration`). `meta_pre` = the segment before the label (the year);
    // `meta_post` = everything after (genre / tracks / duration). The full
    // `info_line` (label inlined as plain text) stays the fallback for when
    // there is no label id to navigate to.
    let label_name = album
        .label
        .as_ref()
        .filter(|l| !l.name.is_empty())
        .map(|l| l.name.clone());
    let genre_str = album
        .genre
        .as_ref()
        .filter(|g| !g.name.is_empty())
        .map(|g| g.name.clone());
    let tracks_str = album.tracks_count.map(|count| {
        qbz_i18n::tf("{} track", "{} tracks", count as i64, &[&count.to_string()])
    });
    let duration_str = album.duration.map(format_duration);

    let mut pre_parts: Vec<String> = Vec::new();
    if !date_display.is_empty() {
        pre_parts.push(date_display.clone());
    }
    let mut post_parts: Vec<String> = Vec::new();
    if let Some(g) = &genre_str {
        post_parts.push(g.clone());
    }
    if let Some(tc) = &tracks_str {
        post_parts.push(tc.clone());
    }
    if let Some(d) = &duration_str {
        post_parts.push(d.clone());
    }

    let meta_pre = pre_parts.join("   •   ");
    let meta_post = post_parts.join("   •   ");

    let mut all_parts = pre_parts.clone();
    if let Some(l) = &label_name {
        all_parts.push(l.clone());
    }
    all_parts.extend(post_parts.clone());
    let info_line = all_parts.join("   •   ");

    let quality_tier = tier(album.maximum_bit_depth).to_string();
    let quality_detail = crate::quality::detail(album.maximum_bit_depth, album.maximum_sampling_rate);
    let description = album
        .description
        .as_deref()
        .map(crate::strip_html::strip_html)
        .unwrap_or_default();
    // The header description fills the full width to the right of the
    // artwork, so a longer truncation keeps it from looking like a thin
    // strip; the Read more modal still holds the complete text.
    let description_short = truncate_words(&description, 360);
    // Half the cutoff for the space-constrained layout (tracks get priority).
    let description_shorter = truncate_words(&description, 180);
    let artwork_url = album.image.best().cloned().unwrap_or_default();
    let label = album
        .label
        .as_ref()
        .map(|l| l.name.clone())
        .unwrap_or_default();
    let label_id = album
        .label
        .as_ref()
        .map(|l| l.id.to_string())
        .unwrap_or_default();
    let awards = album
        .awards
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|a| (a.id.clone().unwrap_or_default(), a.name.clone()))
        .filter(|(_, n)| !n.is_empty())
        .collect();
    // Pick the booklet goody: prefer the PDF format id (21), else the first
    // goody whose url/original_url ends in ".pdf". `original_url` (full-size)
    // wins over the thumbnail `url`. `has_booklet` gates the header button on
    // a usable URL — not merely the presence of a goody.
    let booklet_url = album
        .goodies
        .as_deref()
        .and_then(|goodies| {
            goodies
                .iter()
                .find(|g| g.file_format_id == Some(21))
                .or_else(|| {
                    goodies.iter().find(|g| {
                        let ends_pdf = |s: &str| s.to_lowercase().ends_with(".pdf");
                        ends_pdf(&g.original_url) || ends_pdf(&g.url)
                    })
                })
        })
        .map(|g| {
            if !g.original_url.is_empty() {
                g.original_url.clone()
            } else {
                g.url.clone()
            }
        })
        .unwrap_or_default();
    let has_booklet = !booklet_url.is_empty();
    let raw_tracks: Vec<Track> = album
        .tracks
        .map(|container| container.items)
        .unwrap_or_default();
    let tracks = raw_tracks.iter().cloned().map(map_track).collect();
    let title = crate::album_map::format_album_title(&album.title, album.version.as_deref());

    AlbumData {
        id: album.id,
        title,
        artist,
        artist_id,
        artists,
        info_line,
        meta_pre,
        meta_post,
        quality_tier,
        quality_detail,
        description,
        description_short,
        description_shorter,
        artwork_url,
        label,
        label_id,
        awards,
        has_booklet,
        booklet_url,
        tracks,
        raw_tracks,
    }
}

/// Last.fm path segment: percent-encode, then render spaces as `+` (Last.fm's
/// `/music/{artist}/{album}` paths use `+` for spaces, like Tauri's link
/// builder). `urlencoding::encode` already emits `%20` for spaces, so swap
/// them — the remaining percent-escapes (e.g. `/`, `?`) stay path-safe.
fn lastfm_segment(text: &str) -> String {
    urlencoding::encode(text).replace("%20", "+")
}

/// Truncate text to at most `max` characters, cutting back to the last
/// word boundary and appending an ellipsis. Returns the text unchanged
/// when it is already short enough.
fn truncate_words(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max).collect();
    let cut = truncated.rfind(' ').unwrap_or(truncated.len());
    format!("{}…", truncated[..cut].trim_end())
}

/// "24-bit / 96 kHz" — the quality-badge detail string.
/// Crude HTML strip for Qobuz album descriptions. Break and paragraph
// The previous local strip_html lived here; moved to
// `crate::strip_html` so artist and album views share the same
// paragraph + entity handling and pick up the same future
// improvements.

fn map_track(track: Track) -> TrackData {
    // Classical work metadata, read before `title`/`performer` are moved out of
    // `track`. Qobuz serves `work` on the track (null for non-classical) and a
    // `composer` artist; the official player renders the work title with the
    // composer parenthesized AND the composer name is a link to the artist page
    // (PR #536 + E3). `work` holds the TITLE only (for run-length grouping); the
    // composer name + id are carried separately so the view can make the name a
    // clickable link. All "" when there is no work.
    let work = track
        .work
        .as_ref()
        .filter(|w| !w.is_empty())
        .cloned()
        .unwrap_or_default();
    let (work_composer_name, work_composer_id) = if work.is_empty() {
        (String::new(), String::new())
    } else {
        track
            .composer
            .as_ref()
            .filter(|c| !c.name.is_empty())
            .map(|c| (c.name.clone(), c.id.to_string()))
            .unwrap_or_default()
    };
    let mut title = track.title;
    if let Some(version) = track.version.as_ref().filter(|v| !v.is_empty()) {
        title = format!("{title} ({version})");
    }
    let (artist, artist_id) = track
        .performer
        .map(|p| (p.name, p.id.to_string()))
        .unwrap_or_default();
    TrackData {
        id: track.id.to_string(),
        number: track.track_number.to_string(),
        title,
        artist,
        artist_id,
        // The album view stamps the viewed album's id at the apply layer.
        album_id: String::new(),
        duration: mmss(track.duration),
        quality_tier: tier(track.maximum_bit_depth).to_string(),
        quality_detail: crate::quality::detail(
            track.maximum_bit_depth,
            track.maximum_sampling_rate,
        ),
        explicit: track.parental_warning,
        // Tauri: `disc = track.media_number ?? 1`.
        disc: track.media_number.unwrap_or(1),
        work,
        work_composer_name,
        work_composer_id,
    }
}

/// 24-bit and up is Hi-Res, anything else with depth info is CD-quality.
fn tier(bit_depth: Option<u32>) -> &'static str {
    match bit_depth {
        Some(depth) if depth >= 24 => "hires",
        Some(_) => "cd",
        None => "",
    }
}

/// `m:ss` track duration.
fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// `Xh Ym` / `Ym` album duration.
fn format_duration(secs: u32) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// Apply album data to the `AlbumState` global. Runs on the Slint event loop.
pub fn apply_album(window: &AppWindow, data: AlbumData) {
    // These rows belong to the album currently being viewed, so the
    // album link target is this album's own id (the album column is not
    // shown here, but album-id keeps the row model complete).
    let album_id: slint::SharedString = data.id.clone().into();
    // Album's primary artist id — the fallback blacklist key for rows whose
    // own performer id is missing (Task 6: `track.artist_id ?? album.artist_id`).
    let album_artist_id = data.artist_id.clone();
    // Multi-disc grouping (Tauri PurchaseAlbumDetailView: groupByDisc +
    // isMultiDisc). The album is "multi-disc" when its tracks span more than
    // one distinct disc number; only then do we emit "Disc N" headers. The
    // header is stamped on the first track of each disc run, and tracks stay
    // in their delivered order (Qobuz returns them disc-then-track ordered).
    let is_multi_disc = {
        let mut seen: Option<u32> = None;
        let mut multi = false;
        for track in &data.tracks {
            match seen {
                Some(d) if d != track.disc => {
                    multi = true;
                    break;
                }
                _ => seen = Some(track.disc),
            }
        }
        multi
    };
    let mut prev_disc: Option<u32> = None;
    // Run-length work grouping (PR #536): the header is stamped on the first
    // row of each consecutive same-work run, mirroring the disc grouping above.
    // Albums with no work metadata leave every header "" → flat list, unchanged.
    let mut prev_work: Option<String> = None;
    let tracks: Vec<TrackItem> = data
        .tracks
        .into_iter()
        .map(|track| {
            // Stamp the disc number on the first row of each disc run, but
            // only for multi-disc albums (single-disc renders flat → 0). The
            // delegate renders `@tr("Disc") <n>` above the row when this is
            // > 0, matching the Tauri `{$t('album.disc')} {discNum}` markup.
            let disc_header_number = if is_multi_disc && prev_disc != Some(track.disc) {
                track.disc as i32
            } else {
                0
            };
            prev_disc = Some(track.disc);
            // Work header on the first row of each consecutive same-work run;
            // an empty work resets the run so a later same-named work re-heads.
            let work_header = if !track.work.is_empty()
                && prev_work.as_deref() != Some(track.work.as_str())
            {
                track.work.clone()
            } else {
                String::new()
            };
            // Composer (name + id) accompanies the header only on its leading row.
            let (work_composer_name, work_composer_id) = if work_header.is_empty() {
                (String::new(), String::new())
            } else {
                (
                    track.work_composer_name.clone(),
                    track.work_composer_id.clone(),
                )
            };
            prev_work = if track.work.is_empty() {
                None
            } else {
                Some(track.work.clone())
            };
            // Blacklist key: the row's own performer id, falling back to the
            // album's primary artist when the track carries none (Task 6).
            // NOTE: the album-track row model (`TrackData`) does NOT carry a
            // composer id — only performer/album-primary — so the composer leg
            // of the D-FEAT predicate is not available here. The album queue
            // builder filters off the raw `Track` (which DOES carry composer)
            // via `track_is_blacklisted_full`, so play-all still honors
            // composer; only this row greyout is performer/album-primary-only.
            let row_artist_id = if track.artist_id.is_empty() {
                album_artist_id.as_str()
            } else {
                track.artist_id.as_str()
            };
            let is_blacklisted = crate::artist_blacklist::stamp_row(
                "qobuz",
                &[row_artist_id],
                Some(album_id.as_str()),
            );
            TrackItem {
            id: track.id.clone().into(),
            number: track.number.into(),
            title: track.title.into(),
            artist: track.artist.into(),
            album: "".into(),
            duration: track.duration.into(),
            quality_tier: track.quality_tier.into(),
            quality_detail: track.quality_detail.into(),
            explicit: track.explicit,
            selected: false,
            artwork_url: "".into(),
            artwork: slint::Image::default(),
            is_favorite: crate::fav_cache::is_favorite(&track.id),
            artist_id: track.artist_id.into(),
            album_id: album_id.clone(),
            is_blacklisted,
            removing: false,
            cache_status: if crate::offline_cache::is_cached(&track.id) { 3 } else { 0 },
            cache_progress: 0.0,
            // Qobuz album-detail rows; local albums override via map_local_track.
            source: "qobuz".into(),
            unlocking: false,
            disc_header_number,
            work_header: work_header.into(),
            work_composer_name: work_composer_name.into(),
            work_composer_id: work_composer_id.into(),
            }
        })
        .collect();

    // Seed the award name->id resolver from this album's awards (the same
    // harvesting Tauri's awardCatalogStore.rememberAwardsFromAlbums does), so
    // a sidebar laurel whose id Qobuz omitted can still resolve on click.
    crate::award::remember_awards(&data.awards);
    let awards: Vec<crate::AwardEntry> = data
        .awards
        .into_iter()
        .map(|(id, name)| crate::AwardEntry {
            id: id.into(),
            name: name.into(),
        })
        .collect();

    let has_custom_cover = crate::custom_artwork::album_cover(&data.id).is_some();
    let artwork_url = data.artwork_url.clone();

    let state = window.global::<AlbumState>();
    state.set_id(data.id.into());
    state.set_title(data.title.into());
    state.set_artwork_url(artwork_url.into());
    state.set_has_custom_cover(has_custom_cover);
    state.set_artist(data.artist.into());
    state.set_artist_id(data.artist_id.into());
    let credits: Vec<ArtistCredit> = data
        .artists
        .into_iter()
        .map(|c| ArtistCredit {
            id: c.id.into(),
            name: c.name.into(),
            role: c.role.into(),
        })
        .collect();
    state.set_artists(ModelRc::new(VecModel::from(credits)));
    state.set_info_line(data.info_line.into());
    state.set_meta_pre(data.meta_pre.into());
    state.set_meta_post(data.meta_post.into());
    state.set_quality_tier(data.quality_tier.into());
    state.set_quality_detail(data.quality_detail.into());
    state.set_description(data.description.into());
    state.set_description_short(data.description_short.into());
    state.set_description_shorter(data.description_shorter.into());
    state.set_label(data.label.into());
    state.set_label_id(data.label_id.into());
    state.set_awards(ModelRc::new(VecModel::from(awards)));
    state.set_has_booklet(data.has_booklet);
    // Stash the booklet goody URL for the reader controller; cleared on reset.
    crate::booklet::set_current_url(&data.booklet_url);

    // External-music-database links (Last.fm / Discogs / MusicBrainz), built
    // from artist + title. apply_album is the Qobuz path (local albums load
    // through LocalAlbumState), so is_local is always false here — gate on
    // having both an artist and a title. Mirrors Tauri's AlbumExternalLinks.
    let ext_artist = state.get_artist().to_string();
    let ext_title = state.get_title().to_string();
    let show_external = !ext_artist.is_empty() && !ext_title.is_empty();
    if show_external {
        let lastfm = format!(
            "https://www.last.fm/music/{}/{}",
            lastfm_segment(&ext_artist),
            lastfm_segment(&ext_title),
        );
        // `{artist}+{album}` query (spaces as `+`, each part percent-encoded).
        let query = format!(
            "{}+{}",
            urlencoding::encode(&ext_artist),
            urlencoding::encode(&ext_title)
        );
        let discogs = format!("https://www.discogs.com/search/?q={query}&type=release");
        let musicbrainz =
            format!("https://musicbrainz.org/search?query={query}&type=release&method=indexed");
        state.set_lastfm_url(lastfm.into());
        state.set_discogs_url(discogs.into());
        state.set_musicbrainz_url(musicbrainz.into());
    } else {
        state.set_lastfm_url("".into());
        state.set_discogs_url("".into());
        state.set_musicbrainz_url("".into());
    }
    state.set_show_external_links(show_external);
    // Fully cached = every track already has a ready (3) offline copy. Kept
    // live afterwards by set_row_cache_status as downloads complete.
    let album_fully_cached =
        !tracks.is_empty() && tracks.iter().all(|t| t.cache_status == 3);
    state.set_album_fully_cached(album_fully_cached);
    // Seed the header heart from the favorite-album cache (kept in sync with
    // the server at login + on every toggle).
    state.set_is_favorite(crate::fav_cache::is_album_favorite(album_id.as_str()));
    state.set_is_album_blocked(crate::artist_blacklist::is_album_blacklisted(album_id.as_str()));
    // Seed the pin state from the pinned store (Home "Pinned" section).
    state.set_pinned(crate::pinned::is_pinned("album", album_id.as_str()));
    state.set_favorite_loading(false);

    // Keep the unfiltered list for the track search + the raw tracks for the
    // multi-select bulk actions, then show them all.
    FULL_TRACKS.with(|cell| *cell.borrow_mut() = tracks.clone());
    PLAY_TRACKS.with(|cell| *cell.borrow_mut() = data.raw_tracks);
    // A freshly loaded album starts out of select mode with nothing selected.
    state.set_multi_select(false);
    state.set_selected_count(0);
    state.set_tracks(ModelRc::new(VecModel::from(tracks)));
}

// ==================== Polish carousels (more-from-artist / suggestions) =====

/// "From the same artist" carousel data — other releases by this album's
/// primary artist, with the current album removed. `Send` (plain cards).
pub struct MoreFromArtist {
    pub cards: Vec<crate::home::CardData>,
    /// Whether the carousel should show (non-empty + not Various Artists).
    pub show: bool,
}

/// Maximum cards in the "From the same artist" carousel.
const MORE_FROM_ARTIST_MAX: usize = 16;

/// Fetch + map other releases by `artist_id`, excluding `current_album_id`.
/// Skipped (returns hidden) when the artist id is missing/unparseable or the
/// artist is "Various Artists". Best-effort: a fetch error yields a hidden
/// carousel, never an error to the caller.
pub async fn load_more_from_artist<A>(
    runtime: &Arc<AppRuntime<A>>,
    artist_id: &str,
    artist_name: &str,
    current_album_id: &str,
) -> MoreFromArtist
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let hidden = MoreFromArtist {
        cards: Vec::new(),
        show: false,
    };
    if artist_name.eq_ignore_ascii_case("Various Artists") {
        return hidden;
    }
    let Ok(id) = artist_id.parse::<u64>() else {
        return hidden;
    };
    let resp = match runtime
        .core()
        .get_releases_grid(id, "album", 20, 0, Some("release_date"))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[qbz-slint] more-from-artist load failed: {e}");
            return hidden;
        }
    };
    let cards: Vec<crate::home::CardData> = resp
        .items
        .into_iter()
        .map(crate::artist::map_release)
        .filter(|c| c.id != current_album_id)
        .filter(|c| !crate::artist_blacklist::card_blacklisted(&c.id, &c.artist_id))
        .take(MORE_FROM_ARTIST_MAX)
        .collect();
    let show = !cards.is_empty();
    MoreFromArtist { cards, show }
}

/// Apply the "From the same artist" carousel. Runs on the Slint event loop.
/// Returns the artwork jobs for its cards.
pub fn apply_more_from_artist(window: &AppWindow, data: MoreFromArtist) -> Vec<ArtworkJob> {
    let items: Vec<AlbumCardItem> = data
        .cards
        .iter()
        .cloned()
        .map(crate::home::card_to_item)
        .collect();
    let jobs = data
        .cards
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.artwork_url.is_empty())
        .map(|(i, c)| ArtworkJob {
            url: c.artwork_url.clone(),
            target: ArtworkTarget::AlbumMoreFromArtist { index: i },
        })
        .collect();
    let section = DiscoverSection {
        title: qbz_i18n::t("From the same artist").into(),
        endpoint: "".into(),
        albums: ModelRc::new(VecModel::from(items)),
    };
    let state = window.global::<AlbumState>();
    state.set_more_from_artist(section);
    state.set_show_more_from_artist(data.show);
    jobs
}

/// "Listening suggestions" carousel data — albums similar to the open album
/// (`/album/suggest`). `Send` (plain cards).
pub struct Suggestions {
    pub cards: Vec<crate::album_map::AlbumCard>,
    pub show: bool,
}

/// Fetch + map listening suggestions for `album_id`. Best-effort: an error or
/// an empty result yields a hidden carousel.
pub async fn load_suggestions<A>(runtime: &Arc<AppRuntime<A>>, album_id: &str) -> Suggestions
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let resp = match runtime.core().get_album_suggest(album_id).await {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[qbz-slint] album suggestions load failed: {e}");
            return Suggestions {
                cards: Vec::new(),
                show: false,
            };
        }
    };
    let cards: Vec<crate::album_map::AlbumCard> = resp
        .albums
        .map(|page| page.items)
        .unwrap_or_default()
        .into_iter()
        .map(crate::album_map::map_album)
        .filter(|c| c.id != album_id)
        .filter(|c| !crate::artist_blacklist::card_blacklisted(&c.id, &c.artist_id))
        .collect();
    let show = !cards.is_empty();
    Suggestions { cards, show }
}

/// Apply the "Listening suggestions" carousel. Runs on the Slint event loop.
/// Returns the artwork jobs for its cards.
pub fn apply_suggestions(window: &AppWindow, data: Suggestions) -> Vec<ArtworkJob> {
    let items: Vec<AlbumCardItem> = data
        .cards
        .iter()
        .cloned()
        .map(crate::album_map::to_item)
        .collect();
    let jobs = data
        .cards
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.artwork_url.is_empty())
        .map(|(i, c)| ArtworkJob {
            url: c.artwork_url.clone(),
            target: ArtworkTarget::AlbumSuggestion { index: i },
        })
        .collect();
    let section = DiscoverSection {
        title: qbz_i18n::t("Listening suggestions").into(),
        endpoint: "".into(),
        albums: ModelRc::new(VecModel::from(items)),
    };
    let state = window.global::<AlbumState>();
    state.set_suggestions_section(section);
    state.set_show_suggestions(data.show);
    jobs
}

/// Apply the Last.fm "similar albums" carousel (sits under the Qobuz
/// suggestions, same heading). Runs on the Slint event loop. Returns its
/// artwork jobs. `recos` is already deduped against the Qobuz row by the caller.
pub fn apply_lastfm_suggestions(
    window: &AppWindow,
    recos: Vec<qbz_external_reco::AlbumReco>,
) -> Vec<ArtworkJob> {
    let items: Vec<AlbumCardItem> = recos
        .iter()
        .map(crate::external_reco::album_card)
        .collect();
    let jobs = recos
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.artwork_url.is_empty())
        .map(|(i, a)| ArtworkJob {
            url: a.artwork_url.clone(),
            target: ArtworkTarget::AlbumLastfmSuggestion { index: i },
        })
        .collect();
    let show = !items.is_empty();
    let section = DiscoverSection {
        title: "".into(),
        endpoint: "".into(),
        albums: ModelRc::new(VecModel::from(items)),
    };
    let state = window.global::<AlbumState>();
    state.set_lastfm_suggestions_section(section);
    state.set_show_lastfm_suggestions(show);
    jobs
}

/// Filter the visible track list by `query` (case-insensitive match on
/// title or artist), against the unfiltered list kept in `FULL_TRACKS`.
/// Runs on the Slint event loop.
pub fn filter_tracks(window: &AppWindow, query: &str) {
    let needle = query.trim().to_lowercase();
    let filtered: Vec<TrackItem> = FULL_TRACKS.with(|cell| {
        cell.borrow()
            .iter()
            .filter(|track| {
                needle.is_empty()
                    || track.title.as_str().to_lowercase().contains(&needle)
                    || track.artist.as_str().to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    });
    window
        .global::<AlbumState>()
        .set_tracks(ModelRc::new(VecModel::from(filtered)));
}

/// Clear album state and show an empty track list (used when opening a new
/// album so the previous one does not flash).
pub fn reset_album(window: &AppWindow) {
    FULL_TRACKS.with(|cell| cell.borrow_mut().clear());
    PLAY_TRACKS.with(|cell| cell.borrow_mut().clear());
    let state = window.global::<AlbumState>();
    state.set_multi_select(false);
    state.set_selected_count(0);
    state.set_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_artwork(slint::Image::default());
    state.set_header_atmosphere(slint::Image::default());
    // Clear the booklet gate so the previous album's value doesn't linger.
    state.set_has_booklet(false);
    crate::booklet::clear_current_url();
    // Clear the polish carousels + external links so the previous album's
    // values don't flash before the new loads land.
    state.set_more_from_artist(DiscoverSection::default());
    state.set_show_more_from_artist(false);
    state.set_suggestions_section(DiscoverSection::default());
    state.set_show_suggestions(false);
    state.set_lastfm_suggestions_section(DiscoverSection::default());
    state.set_show_lastfm_suggestions(false);
    state.set_show_external_links(false);
    state.set_lastfm_url("".into());
    state.set_discogs_url("".into());
    state.set_musicbrainz_url("".into());
    state.set_album_fully_cached(false);
    state.set_is_favorite(false);
    state.set_pinned(false);
    state.set_favorite_loading(false);
    // Default to a Qobuz album; the local-album loader opts in.
    state.set_is_local(false);
    state.set_loading(true);
}

// ==================== Multi-select (track selection) ====================

/// Toggle multi-select mode on the album track list. Leaving the mode
/// clears any current selection.
pub fn set_multi_select(window: &AppWindow, on: bool) {
    let state = window.global::<AlbumState>();
    state.set_multi_select(on);
    // Reset the Shift-range anchor whenever the mode changes (fresh session on
    // enter, no stale anchor on leave).
    crate::selection::clear_anchor();
    if !on {
        clear_selection(window);
    }
}

/// Recompute the "N selected" count from the track rows.
pub fn recount_selected(window: &AppWindow) {
    let state = window.global::<AlbumState>();
    let model = state.get_tracks();
    let count = (0..model.row_count())
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count();
    state.set_selected_count(count as i32);
}

/// Select every row, or clear if all are already selected (the toggle the
/// "Select all" bulk button drives — same semantics as the favorites bar).
pub fn select_all(window: &AppWindow) {
    let model = window.global::<AlbumState>().get_tracks();
    let total = model.row_count();
    let selected = (0..total)
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count();
    let target = selected != total;
    for i in 0..total {
        if let Some(mut item) = model.row_data(i) {
            if item.selected != target {
                item.selected = target;
                model.set_row_data(i, item);
            }
        }
    }
    recount_selected(window);
}

/// Clear the selection (uncheck all), keeping multi-select mode on.
pub fn clear_selection(window: &AppWindow) {
    let model = window.global::<AlbumState>().get_tracks();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.selected {
                item.selected = false;
                model.set_row_data(i, item);
            }
        }
    }
    window.global::<AlbumState>().set_selected_count(0);
}

/// The catalog ids of the currently selected rows (for add-to-playlist /
/// add-to-favorites — Qobuz catalog ids only).
pub fn selected_ids(window: &AppWindow) -> Vec<String> {
    let model = window.global::<AlbumState>().get_tracks();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|t| t.selected)
        .map(|t| t.id.to_string())
        .filter(|s| s.parse::<u64>().is_ok())
        .collect()
}

/// The full catalog Track objects for the selected rows (for enqueue /
/// cache), resolved from the stashed raw album tracks by id.
pub fn selected_play_tracks(window: &AppWindow) -> Vec<Track> {
    let ids: std::collections::HashSet<String> = selected_ids(window).into_iter().collect();
    PLAY_TRACKS.with(|cell| {
        cell.borrow()
            .iter()
            .filter(|t| ids.contains(&t.id.to_string()))
            .cloned()
            .collect()
    })
}

/// The full catalog Track objects for one disc of the open album (for the
/// per-disc "Disc N" header menu), resolved from the stashed raw album tracks.
/// `disc` matches `media_number` (defaulting to 1, exactly as `map_track`
/// stamps `TrackData.disc`), so it lines up with the rendered "Disc N" header.
/// Preserves the delivered (disc-then-track) order.
pub fn disc_play_tracks(disc: i32) -> Vec<Track> {
    PLAY_TRACKS.with(|cell| {
        cell.borrow()
            .iter()
            .filter(|t| t.media_number.unwrap_or(1) as i32 == disc)
            .cloned()
            .collect()
    })
}

/// Apply decoded header artwork pixels. Runs on the Slint event loop.
pub fn apply_artwork(window: &AppWindow, pixels: &[u8], width: u32, height: u32) {
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    let dst = buffer.make_mut_bytes();
    if dst.len() != pixels.len() {
        return;
    }
    dst.copy_from_slice(pixels);
    let (r, g, b) = crate::artwork::header_tint(pixels);
    let state = window.global::<AlbumState>();
    state.set_artwork(slint::Image::from_rgba8(buffer));
    state.set_header_color(slint::Color::from_rgb_u8(r, g, b));
    if let Some(atmosphere) = crate::immersive::generate_atmosphere_image(pixels, width, height) {
        state.set_header_atmosphere(atmosphere);
    } else {
        state.set_header_atmosphere(slint::Image::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mmss_pads_seconds() {
        assert_eq!(mmss(5), "0:05");
        assert_eq!(mmss(65), "1:05");
        assert_eq!(mmss(225), "3:45");
    }

    #[test]
    fn duration_drops_zero_hours() {
        assert_eq!(format_duration(2700), "45m");
        assert_eq!(format_duration(3720), "1h 2m");
    }

    #[test]
    fn tier_classifies_bit_depth() {
        assert_eq!(tier(Some(24)), "hires");
        assert_eq!(tier(Some(16)), "cd");
        assert_eq!(tier(None), "");
    }
}
