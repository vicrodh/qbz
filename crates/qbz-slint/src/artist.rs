//! Artist detail controller.
//!
//! Fetches an artist page through `QbzCore`, maps it to plain (Send)
//! data on the worker thread, and applies it to the `ArtistState`
//! global on the Slint event loop.

use std::collections::HashSet;
use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{
    PageArtistRelease, PageArtistResponse, PageArtistTrack, PageArtistTrackAlbum,
};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::album::TrackData;
use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::home::CardData;
use crate::{
    AlbumCardItem, AlbumTrackItem, AppWindow, ArtistState, DiscoverSection, MbOriginData,
    NetworkSidebarState,
};

/// Plain, `Send` artist data produced on the worker thread.
pub struct ArtistData {
    pub name: String,
    pub bio: String,
    /// Word-boundary truncated bio used at rest; the Read-more toggle
    /// swaps to `bio`. Equal to `bio` when the text fits in the cap.
    pub bio_short: String,
    pub bio_truncated: bool,
    /// Editorial source for the biography ("TiVo" etc). Empty when absent.
    pub bio_source: String,
    pub artwork_url: String,
    pub top_tracks: Vec<TrackData>,
    /// Releases grouped into titled sections (Albums, EPs & Singles, ...).
    pub release_sections: Vec<ReleaseSection>,
}

/// One titled group of artist releases.
pub struct ReleaseSection {
    pub title: String,
    pub cards: Vec<CardData>,
}

/// Fetch and map an artist page by id.
pub async fn load_artist<A>(
    runtime: &Arc<AppRuntime<A>>,
    artist_id: &str,
) -> Result<ArtistData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let id: u64 = artist_id
        .parse()
        .map_err(|_| format!("invalid artist id: {artist_id}"))?;
    let page = runtime
        .core()
        .get_artist_page(id, None)
        .await
        .map_err(|e| e.to_string())?;
    Ok(map_artist(page))
}

fn map_artist(page: PageArtistResponse) -> ArtistData {
    let main_artist_id = page.id;
    let name = page.name.display;

    // Biography: content (HTML-stripped) + source name (when present). The
    // /artist/page biography.source is a raw JSON value because Qobuz
    // sometimes returns a string and sometimes an object; we only care
    // about the string form.
    let (bio, bio_source) = match page.biography {
        Some(biography) => {
            let content = biography
                .content
                .map(|c| strip_html(&c))
                .unwrap_or_default();
            let source = biography
                .source
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default();
            (content, source)
        }
        None => (String::new(), String::new()),
    };
    let bio_short = truncate_words(&bio, 360);
    let bio_truncated = bio_short != bio;

    let artwork_url = page
        .images
        .and_then(|images| images.portrait)
        .map(|portrait| {
            format!(
                "https://static.qobuz.com/images/artists/covers/large/{}.{}",
                portrait.hash, portrait.format
            )
        })
        .unwrap_or_default();

    let top_tracks = page
        .top_tracks
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(index, track)| map_track(index, track))
        .collect();

    // Releases: bucket into 4 categories per Tauri's convertPageArtist:
    //   album              → Discography
    //   ep | single | epSingle → EPs & Singles
    //   live               → Live Albums
    //   compilation | boxset | download | other | _ → Others
    // The "download" group items are re-categorized by their individual
    // release_type because download is a distribution channel, not a
    // content type. Foreign-artist releases are filtered out (only the
    // main artist's own pieces stay) and IDs are deduped across groups
    // so a release listed in multiple groups appears once.
    let mut albums: Vec<CardData> = Vec::new();
    let mut eps_singles: Vec<CardData> = Vec::new();
    let mut live_albums: Vec<CardData> = Vec::new();
    let mut others: Vec<CardData> = Vec::new();
    let mut seen_release_ids: HashSet<String> = HashSet::new();

    for group in page.releases.into_iter().flatten() {
        let group_bucket = map_release_type(&group.release_type);
        for release in group.items.into_iter() {
            // Foreign-artist filter — the page sometimes surfaces "appears
            // on" entries inside release groups; those don't belong here.
            if let Some(artist_ref) = release.artist.as_ref() {
                if artist_ref.id != main_artist_id {
                    continue;
                }
            }
            if seen_release_ids.contains(&release.id) {
                continue;
            }
            seen_release_ids.insert(release.id.clone());

            let item_bucket = if group.release_type == "download" {
                release
                    .release_type
                    .as_deref()
                    .map(map_release_type)
                    .unwrap_or(group_bucket)
            } else {
                group_bucket
            };

            let card = map_release(release);
            match item_bucket {
                ReleaseBucket::Albums => albums.push(card),
                ReleaseBucket::Eps => eps_singles.push(card),
                ReleaseBucket::Live => live_albums.push(card),
                ReleaseBucket::Others => others.push(card),
            }
        }
    }

    // Compilations come from tracks_appears_on — albums by other artists
    // that include this artist. Dedupe by album id.
    let mut compilations: Vec<CardData> = Vec::new();
    let mut seen_compilation_albums: HashSet<String> = HashSet::new();
    for track in page.tracks_appears_on.into_iter().flatten() {
        let Some(album) = track.album else { continue };
        if seen_compilation_albums.contains(&album.id) {
            continue;
        }
        seen_compilation_albums.insert(album.id.clone());
        compilations.push(map_compilation_album(album));
    }

    let mut release_sections: Vec<ReleaseSection> = Vec::new();
    if !albums.is_empty() {
        release_sections.push(ReleaseSection {
            title: "Discography".to_string(),
            cards: albums,
        });
    }
    if !eps_singles.is_empty() {
        release_sections.push(ReleaseSection {
            title: "EPs & Singles".to_string(),
            cards: eps_singles,
        });
    }
    if !live_albums.is_empty() {
        release_sections.push(ReleaseSection {
            title: "Live Albums".to_string(),
            cards: live_albums,
        });
    }
    if !compilations.is_empty() {
        release_sections.push(ReleaseSection {
            title: "Compilations".to_string(),
            cards: compilations,
        });
    }
    if !others.is_empty() {
        release_sections.push(ReleaseSection {
            title: "Others".to_string(),
            cards: others,
        });
    }

    ArtistData {
        name,
        bio,
        bio_short,
        bio_truncated,
        bio_source,
        artwork_url,
        top_tracks,
        release_sections,
    }
}

/// UI bucket a Qobuz release_type maps to. Mirrors Tauri's adapter so the
/// section headings stay 1:1 with the WebKit build.
#[derive(Debug, Clone, Copy)]
enum ReleaseBucket {
    Albums,
    Eps,
    Live,
    Others,
}

/// Map a Qobuz release_type string to its UI bucket. Anything not in the
/// curated three categories (album / ep* / live) lands in Others so the
/// page never grows a long tail of one-off section titles.
fn map_release_type(release_type: &str) -> ReleaseBucket {
    match release_type {
        "album" => ReleaseBucket::Albums,
        "ep" | "single" | "epSingle" => ReleaseBucket::Eps,
        "live" => ReleaseBucket::Live,
        // compilation, boxset, download, other, and anything new the API
        // adds in the future.
        _ => ReleaseBucket::Others,
    }
}

/// Truncate text at the last word boundary within `max` characters,
/// appending an ellipsis. Returns the text unchanged when it already
/// fits.
fn truncate_words(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max).collect();
    let cut = truncated.rfind(' ').unwrap_or(truncated.len());
    format!("{}…", truncated[..cut].trim_end())
}

/// Build artwork download jobs for every release card so the cover grid
/// fills in once the images decode.
pub fn artwork_jobs(data: &ArtistData) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    for (section_idx, section) in data.release_sections.iter().enumerate() {
        for (album_idx, card) in section.cards.iter().enumerate() {
            if !card.artwork_url.is_empty() {
                jobs.push(ArtworkJob {
                    target: ArtworkTarget::ArtistRelease {
                        section_idx,
                        album_idx,
                    },
                    url: card.artwork_url.clone(),
                });
            }
        }
    }
    jobs
}

/// Build a CardData for the Compilations section out of a track's album
/// (tracks_appears_on entries — albums by other artists that feature the
/// main artist). The PageArtistTrackAlbum payload is lighter than a full
/// release (no year/dates), so the year stays empty.
fn map_compilation_album(album: PageArtistTrackAlbum) -> CardData {
    let artwork_url = album
        .image
        .and_then(|img| img.best().cloned())
        .unwrap_or_default();
    CardData {
        id: album.id,
        title: album.title,
        artist: String::new(),
        genre: album.genre.map(|g| g.name).unwrap_or_default(),
        year: String::new(),
        quality_tier: String::new(),
        quality_label: String::new(),
        ribbon: String::new(),
        ribbon_kind: String::new(),
        artwork_url,
    }
}

fn map_track(index: usize, track: PageArtistTrack) -> TrackData {
    let mut title = track.title;
    if let Some(version) = track.version.as_ref().filter(|v| !v.is_empty()) {
        title = format!("{title} ({version})");
    }
    TrackData {
        id: track.id.to_string(),
        number: (index + 1).to_string(),
        title,
        artist: track.artist.map(|a| a.name.display).unwrap_or_default(),
        duration: mmss(track.duration.unwrap_or(0)),
        quality_tier: tier(track.audio_info.and_then(|a| a.maximum_bit_depth)).to_string(),
        explicit: track.parental_warning.unwrap_or(false),
    }
}

fn map_release(release: PageArtistRelease) -> CardData {
    let artist = release
        .artist
        .map(|a| a.name.display)
        .or_else(|| release.artists.and_then(|list| list.into_iter().next().map(|a| a.name)))
        .unwrap_or_default();
    let artwork_url = release
        .image
        .and_then(|img| img.best().cloned())
        .unwrap_or_default();
    CardData {
        id: release.id,
        title: release.title,
        artist,
        genre: release.genre.map(|g| g.name).unwrap_or_default(),
        year: String::new(),
        quality_tier: String::new(),
        quality_label: String::new(),
        ribbon: String::new(),
        ribbon_kind: String::new(),
        artwork_url,
    }
}

fn tier(bit_depth: Option<u32>) -> &'static str {
    match bit_depth {
        Some(depth) if depth >= 24 => "hires",
        Some(_) => "cd",
        None => "",
    }
}

fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Crude HTML strip for Qobuz biographies (tags + a few entities). The
/// entity set is intentionally small — only the ones the Qobuz API
/// regularly emits in biography bodies, with the © family explicitly
/// covered because TiVo-sourced bios often close with a `&copy; TiVo`
/// credit line.
fn strip_html(input: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&nbsp;", " ")
        .replace("&copy;", "©")
        .replace("&#169;", "©")
        .replace("&#xa9;", "©")
        .replace("&#xA9;", "©")
        .replace("&reg;", "®")
        .replace("&trade;", "™")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
        .trim()
        .to_string()
}

fn card_to_item(card: CardData) -> AlbumCardItem {
    AlbumCardItem {
        id: card.id.into(),
        title: card.title.into(),
        artist: card.artist.into(),
        genre: card.genre.into(),
        year: card.year.into(),
        quality_tier: card.quality_tier.into(),
        quality_label: card.quality_label.into(),
        ribbon: card.ribbon.into(),
        ribbon_kind: card.ribbon_kind.into(),
        artwork_url: card.artwork_url.into(),
        artwork: slint::Image::default(),
    }
}

/// Apply artist data to the `ArtistState` global. Runs on the Slint event loop.
pub fn apply_artist(window: &AppWindow, data: ArtistData) {
    let top_tracks: Vec<AlbumTrackItem> = data
        .top_tracks
        .into_iter()
        .map(|track| AlbumTrackItem {
            id: track.id.into(),
            number: track.number.into(),
            title: track.title.into(),
            artist: track.artist.into(),
            duration: track.duration.into(),
            quality_tier: track.quality_tier.into(),
            explicit: track.explicit,
            selected: false,
        })
        .collect();
    let release_sections: Vec<DiscoverSection> = data
        .release_sections
        .into_iter()
        .map(|section| DiscoverSection {
            title: section.title.into(),
            albums: ModelRc::new(VecModel::from(
                section.cards.into_iter().map(card_to_item).collect::<Vec<_>>(),
            )),
        })
        .collect();

    let state = window.global::<ArtistState>();
    state.set_name(data.name.into());
    state.set_bio(data.bio.into());
    state.set_bio_short(data.bio_short.into());
    state.set_bio_truncated(data.bio_truncated);
    state.set_bio_source(data.bio_source.into());
    state.set_top_tracks(ModelRc::new(VecModel::from(top_tracks)));
    state.set_release_sections(ModelRc::new(VecModel::from(release_sections)));
}

/// Clear artist state before loading a new artist.
pub fn reset_artist(window: &AppWindow) {
    let state = window.global::<ArtistState>();
    state.set_top_tracks(ModelRc::new(VecModel::from(Vec::<AlbumTrackItem>::new())));
    state.set_release_sections(ModelRc::new(VecModel::from(Vec::<DiscoverSection>::new())));
    state.set_artwork(slint::Image::default());
    state.set_name("".into());
    state.set_bio("".into());
    state.set_bio_source("".into());
    state.set_loading(true);
}

/// Apply decoded portrait artwork. Runs on the Slint event loop.
pub fn apply_artwork(window: &AppWindow, pixels: &[u8], width: u32, height: u32) {
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    let dst = buffer.make_mut_bytes();
    if dst.len() != pixels.len() {
        return;
    }
    dst.copy_from_slice(pixels);
    let (r, g, b) = crate::artwork::header_tint(pixels);
    let state = window.global::<ArtistState>();
    state.set_artwork(slint::Image::from_rgba8(buffer));
    state.set_header_color(slint::Color::from_rgb_u8(r, g, b));
}

// ----- MusicBrainz network sidebar ---------------------------------------

/// Plain, `Send` payload mapping `qbz_integrations::ArtistMetadata` into
/// the shape the Origin section of the sidebar renders.
pub struct MbMetadata {
    pub mbid: String,
    pub origin: MbOrigin,
}

#[derive(Default)]
pub struct MbOrigin {
    pub is_person: bool,
    pub begin_date: String,
    pub end_date: String,
    pub location_display: String,
    pub location_clickable: bool,
}

/// Reset the network sidebar's MB-driven state. Called when the artist
/// changes so a stale Origin/Relationships/Discovery never bleeds
/// across artists.
pub fn reset_network_sidebar(window: &AppWindow) {
    let state = window.global::<NetworkSidebarState>();
    state.set_mb_available(true);
    state.set_mb_mbid("".into());
    state.set_origin_loading(false);
    state.set_origin(MbOriginData::default());
    state.set_relationships_loading(false);
    state.set_discovery_loading(false);
}

/// Resolve the artist name to an MBID, then fetch artist metadata. Returns
/// `Ok(None)` when MB is disabled or no confident match is found — the
/// sidebar treats both the same (Origin section hides).
pub async fn load_mb_metadata<A>(
    runtime: &Arc<AppRuntime<A>>,
    artist_name: &str,
) -> Result<Option<MbMetadata>, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    if !runtime.core().musicbrainz_is_enabled().await {
        return Ok(None);
    }

    let resolved = runtime
        .core()
        .musicbrainz_resolve_artist(artist_name)
        .await
        .map_err(|e| e.to_string())?;
    let Some(resolved) = resolved else {
        return Ok(None);
    };
    // resolve_artist may return a low-confidence match; surface only the
    // mbid when the client gives us one (the qbz-integrations layer
    // already filters out the "no match at all" case before returning).
    let mbid = resolved.mbid;
    if mbid.is_empty() {
        return Ok(None);
    }

    let meta = runtime
        .core()
        .musicbrainz_get_artist_metadata(&mbid)
        .await
        .map_err(|e| e.to_string())?;

    Ok(Some(MbMetadata {
        mbid: mbid.clone(),
        origin: map_origin(&meta),
    }))
}

fn map_origin(meta: &qbz_integrations::musicbrainz::ArtistMetadata) -> MbOrigin {
    use qbz_integrations::musicbrainz::{ArtistType, LocationPrecision};

    let is_person = matches!(meta.artist_type, ArtistType::Person);

    let begin_date = meta
        .life_span
        .as_ref()
        .and_then(|ls| ls.begin.as_deref().map(format_mb_date_short))
        .unwrap_or_default();
    let end_date = meta
        .life_span
        .as_ref()
        .and_then(|ls| ls.end.as_deref().map(format_mb_date_short))
        .unwrap_or_default();

    let (location_display, location_clickable) = match &meta.location {
        Some(loc) => {
            // Tauri's gate: clickable when precision isn't "country" OR
            // a city is present somehow. Country-only locations stay as
            // plain text — there's nothing to drill into.
            let clickable = !matches!(loc.precision, LocationPrecision::Country)
                || loc.city.is_some();
            (loc.display_name.clone(), clickable)
        }
        None => (String::new(), false),
    };

    MbOrigin {
        is_person,
        begin_date,
        end_date,
        location_display,
        location_clickable,
    }
}

/// Apply the MB metadata to NetworkSidebarState. Runs on the Slint
/// event loop.
pub fn apply_mb_metadata(window: &AppWindow, meta: MbMetadata) {
    let state = window.global::<NetworkSidebarState>();
    state.set_mb_mbid(meta.mbid.into());
    state.set_origin(MbOriginData {
        is_person: meta.origin.is_person,
        begin_date: meta.origin.begin_date.into(),
        end_date: meta.origin.end_date.into(),
        location_display: meta.origin.location_display.into(),
        location_clickable: meta.origin.location_clickable,
    });
    state.set_origin_loading(false);
}

/// Mark the sidebar as MB-unavailable (disabled in settings, or no
/// confident match for this artist). The MB-driven sections hide.
pub fn apply_mb_unavailable(window: &AppWindow) {
    let state = window.global::<NetworkSidebarState>();
    state.set_mb_available(false);
    state.set_origin_loading(false);
    state.set_relationships_loading(false);
    state.set_discovery_loading(false);
}

/// Format a MusicBrainz partial date into a short human string —
/// "1990", "May 1990", or "May 14, 1990" — matching Tauri's
/// formatMbDate_v2 output when the locale is en-US.
fn format_mb_date_short(date: &str) -> String {
    let parts: Vec<&str> = date.split('-').collect();
    let month = |m: &str| -> Option<&'static str> {
        Some(match m {
            "01" => "January",
            "02" => "February",
            "03" => "March",
            "04" => "April",
            "05" => "May",
            "06" => "June",
            "07" => "July",
            "08" => "August",
            "09" => "September",
            "10" => "October",
            "11" => "November",
            "12" => "December",
            _ => return None,
        })
    };
    match parts.as_slice() {
        [y] => (*y).to_string(),
        [y, m] => match month(m) {
            Some(name) => format!("{} {}", name, y),
            None => date.to_string(),
        },
        [y, m, d] => match month(m) {
            Some(name) => {
                let day = d.trim_start_matches('0');
                format!("{} {}, {}", name, day, y)
            }
            None => date.to_string(),
        },
        _ => date.to_string(),
    }
}
