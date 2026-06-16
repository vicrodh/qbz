//! Artist detail controller.
//!
//! Fetches an artist page through `QbzCore`, maps it to plain (Send)
//! data on the worker thread, and applies it to the `ArtistState`
//! global on the Slint event loop.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{
    PageArtistRelease, PageArtistResponse, PageArtistTrack, PageArtistTrackAlbum,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::album::TrackData;
use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::home::CardData;
use crate::{
    AlbumCardItem, TrackItem, AppWindow, ArtistState, DiscoverSection, DiscoveryArtist,
    JumpNavTab, LabelEntry, MbOriginData, MbRelationship, MbRelationshipsData,
    NetworkSidebarState, ShellState, SimilarEntry,
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
    /// Labels collected from the artist's own album releases (deduped
    /// by id, sorted by name) — sidebar Labels section.
    pub labels: Vec<LabelData>,
    /// Similar artists from /artist/page — sidebar Similar Artists.
    pub similar_artists: Vec<SimilarArtistData>,
}

#[derive(Clone)]
pub struct LabelData {
    pub id: String,
    pub name: String,
}

#[derive(Clone)]
pub struct SimilarArtistData {
    pub id: String,
    pub name: String,
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
                .map(|c| crate::strip_html::strip_html(&c))
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
    // Labels collected while iterating the artist's own album releases.
    // Tauri's extractLabelsFromPageReleases: only group.type == "album"
    // (not the bucket result after the download re-categorization),
    // only own releases, dedupe by label id.
    let mut labels_by_id: BTreeMap<u64, String> = BTreeMap::new();

    for group in page.releases.into_iter().flatten() {
        let group_bucket = map_release_type(&group.release_type);
        let is_album_group = group.release_type == "album";
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

            // Label collection — before consuming `release` into the card.
            if is_album_group {
                if let Some(label) = release.label.as_ref() {
                    labels_by_id
                        .entry(label.id)
                        .or_insert_with(|| label.name.clone());
                }
            }

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

    let mut labels: Vec<LabelData> = labels_by_id
        .into_iter()
        .map(|(id, name)| LabelData {
            id: id.to_string(),
            name,
        })
        .collect();
    labels.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let similar_artists: Vec<SimilarArtistData> = page
        .similar_artists
        .map(|s| {
            s.items
                .into_iter()
                .map(|item| SimilarArtistData {
                    id: item.id.to_string(),
                    name: item.name.display,
                })
                .collect()
        })
        .unwrap_or_default();

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
        labels,
        similar_artists,
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
        artist_id: String::new(),
        genre: album.genre.map(|g| g.name).unwrap_or_default(),
        year: String::new(),
        quality_tier: String::new(),
        quality_label: String::new(),
        ribbon: String::new(),
        ribbon_kind: String::new(),
        artwork_url,
        ..CardData::default()
    }
}

fn map_track(index: usize, track: PageArtistTrack) -> TrackData {
    let mut title = track.title;
    if let Some(version) = track.version.as_ref().filter(|v| !v.is_empty()) {
        title = format!("{title} ({version})");
    }
    let (artist, artist_id) = track
        .artist
        .map(|a| (a.name.display, a.id.to_string()))
        .unwrap_or_default();
    let album_id = track.album.map(|a| a.id).unwrap_or_default();
    let bit_depth = track.audio_info.as_ref().and_then(|a| a.maximum_bit_depth);
    let sample_rate = track.audio_info.as_ref().and_then(|a| a.maximum_sampling_rate);
    TrackData {
        id: track.id.to_string(),
        number: (index + 1).to_string(),
        title,
        artist,
        artist_id,
        album_id,
        duration: mmss(track.duration.unwrap_or(0)),
        quality_tier: tier(bit_depth).to_string(),
        quality_detail: crate::quality::detail(bit_depth, sample_rate),
        explicit: track.parental_warning.unwrap_or(false),
        // Artist top-tracks are a flat cross-album list and never render
        // "Disc N" headers, so the disc value is unused here — default to 1.
        disc: 1,
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
    let year = release
        .dates
        .as_ref()
        .and_then(|d| d.original.as_deref())
        .and_then(|s| s.get(..4).map(|y| y.to_string()))
        .unwrap_or_default();
    let bit_depth = release.audio_info.as_ref().and_then(|a| a.maximum_bit_depth);
    let sample_rate = release
        .audio_info
        .as_ref()
        .and_then(|a| a.maximum_sampling_rate);
    let quality_tier = tier(bit_depth).to_string();
    let quality_label = match (bit_depth, sample_rate) {
        (Some(bd), Some(sr)) => format!("{}-bit / {} kHz", bd, sr),
        _ => String::new(),
    };
    CardData {
        id: release.id,
        title: release.title,
        artist,
        artist_id: String::new(),
        genre: release.genre.map(|g| g.name).unwrap_or_default(),
        year,
        quality_tier,
        quality_label,
        ribbon: String::new(),
        ribbon_kind: String::new(),
        artwork_url,
        ..CardData::default()
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
// strip_html now lives in `crate::strip_html` so both album and
// artist views use the same paragraph-preserving conversion. The
// previous artist-local helper produced one long paragraph for
// multi-paragraph biographies — gone with this refactor.

pub(crate) fn card_to_item(card: CardData) -> AlbumCardItem {
    // On the artist page the card subtitle slot should show the
    // release year — the artist is redundant since we're already on
    // their page. The AlbumCard reads `artist` for its subtitle line,
    // so re-route year through that field instead of changing the
    // shared card primitive.
    AlbumCardItem {
        id: card.id.into(),
        title: card.title.into(),
        artist: card.year.clone().into(),
        artist_id: "".into(),
        genre: card.genre.into(),
        year: card.year.into(),
        quality_tier: card.quality_tier.into(),
        quality_label: card.quality_label.into(),
        ribbon: card.ribbon.into(),
        ribbon_kind: card.ribbon_kind.into(),
        artwork_url: card.artwork_url.into(),
        artwork: slint::Image::default(),
        ..Default::default()
    }
}

// Full (unfiltered) rendered models cached on the UI thread so the
// in-page search can rebuild the visible models cheaply on every
// keystroke. Mirrors album::FULL_TRACKS.
thread_local! {
    static FULL_TOP_TRACKS: std::cell::RefCell<Vec<TrackItem>> =
        std::cell::RefCell::new(Vec::new());
    static FULL_RELEASE_SECTIONS: std::cell::RefCell<Vec<DiscoverSection>> =
        std::cell::RefCell::new(Vec::new());
}

/// Apply artist data to the `ArtistState` global. Runs on the Slint event loop.
pub fn apply_artist(window: &AppWindow, data: ArtistData) {
    // Capture counts before we move the data so the JUMP TO tab
    // anchor-y estimates can use them.
    let top_tracks_count = data.top_tracks.len();
    let section_counts: Vec<(String, usize)> = data
        .release_sections
        .iter()
        .map(|s| (s.title.clone(), s.cards.len()))
        .collect();

    let top_tracks: Vec<TrackItem> = data
        .top_tracks
        .into_iter()
        .map(|track| TrackItem {
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
            album_id: track.album_id.into(),
            removing: false,
            cache_status: if crate::offline_cache::is_cached(&track.id) { 3 } else { 0 },
            cache_progress: 0.0,
            source: "qobuz".into(),
            unlocking: false,
            // Disc grouping is album-detail only; flat lists carry none.
            disc_header_number: 0,
        })
        .collect();
    let release_sections: Vec<DiscoverSection> = data
        .release_sections
        .into_iter()
        .map(|section| DiscoverSection {
            title: section.title.into(),
            // Artist release sections have no Discover full-list page.
            endpoint: "".into(),
            albums: ModelRc::new(VecModel::from(
                section.cards.into_iter().map(card_to_item).collect::<Vec<_>>(),
            )),
        })
        .collect();

    let jump_tabs = build_jump_tabs(top_tracks_count, &section_counts);

    let labels: Vec<LabelEntry> = data
        .labels
        .into_iter()
        .map(|label| LabelEntry {
            id: label.id.into(),
            name: label.name.into(),
        })
        .collect();
    let similar_artists: Vec<SimilarEntry> = data
        .similar_artists
        .into_iter()
        .map(|sa| SimilarEntry {
            id: sa.id.into(),
            name: sa.name.into(),
        })
        .collect();

    // Cache the full models on the UI thread so the in-page search
    // can rebuild filtered views without re-fetching the artist.
    FULL_TOP_TRACKS.with(|cell| {
        *cell.borrow_mut() = top_tracks.clone();
    });
    FULL_RELEASE_SECTIONS.with(|cell| {
        *cell.borrow_mut() = release_sections.clone();
    });

    let has_custom_image = crate::custom_artwork::artist_image(&data.name).is_some();
    let artwork_url = data.artwork_url.clone();

    let state = window.global::<ArtistState>();
    state.set_name(data.name.into());
    state.set_artwork_url(artwork_url.into());
    state.set_has_custom_image(has_custom_image);
    state.set_bio(data.bio.into());
    state.set_bio_short(data.bio_short.into());
    state.set_bio_truncated(data.bio_truncated);
    state.set_bio_source(data.bio_source.into());
    state.set_top_tracks(ModelRc::new(VecModel::from(top_tracks)));
    state.set_release_sections(ModelRc::new(VecModel::from(release_sections)));
    state.set_labels(ModelRc::new(VecModel::from(labels)));
    state.set_similar_artists(ModelRc::new(VecModel::from(similar_artists)));
    state.set_jump_tabs(ModelRc::new(VecModel::from(jump_tabs)));
}

/// Build the JUMP TO tab list for this artist. Tabs are emitted only
/// for sections that actually have content (no empty Compilations
/// row when the artist has none); each tab carries a page-local
/// `anchor-y` estimate so a click can scroll the page-flickable
/// straight to that section. Heights are layout-derived
/// approximations — variable bio length and grid wrapping make a
/// truly precise number hard without measuring each frame, but the
/// estimates land the user inside the right section.
fn build_jump_tabs(
    top_tracks_count: usize,
    sections: &[(String, usize)],
) -> Vec<JumpNavTab> {
    // Layout constants — keep in sync with ArtistPageView.slint.
    const BODY_ROW_TOP_GUESS: f32 = 320.0;
    const SECTION_SPACER: f32 = 32.0;
    const RELEASE_HEADER: f32 = 28.0;
    const RELEASE_ROW: f32 = 290.0;
    const RELEASE_ROW_GAP: f32 = 24.0;
    const RELEASE_COLS: f32 = 5.0;
    const POPULAR_HEADER: f32 = 36.0;
    const POPULAR_HEADER_GAP: f32 = 10.0;
    const POPULAR_ROW: f32 = 52.0;
    const POPULAR_TAIL: f32 = 32.0;

    let mut tabs: Vec<JumpNavTab> = Vec::new();
    tabs.push(JumpNavTab {
        id: "about".into(),
        label: "About".into(),
        anchor_y: 0.0,
    });

    let mut cursor = BODY_ROW_TOP_GUESS;
    if top_tracks_count > 0 {
        tabs.push(JumpNavTab {
            id: "popular-tracks".into(),
            label: "Popular Tracks".into(),
            anchor_y: cursor,
        });
        let visible_rows = top_tracks_count.min(5) as f32;
        cursor +=
            POPULAR_HEADER + POPULAR_HEADER_GAP + visible_rows * POPULAR_ROW + POPULAR_TAIL;
    }

    for (title, count) in sections {
        let id = match title.as_str() {
            "Discography" => "discography",
            "EPs & Singles" => "eps-singles",
            "Live Albums" => "live-albums",
            "Compilations" => "compilations",
            "Others" => "others",
            _ => continue,
        };
        tabs.push(JumpNavTab {
            id: id.into(),
            label: title.clone().into(),
            anchor_y: cursor,
        });
        let rows = (*count as f32 / RELEASE_COLS).ceil().max(1.0);
        cursor += SECTION_SPACER
            + RELEASE_HEADER
            + rows * RELEASE_ROW
            + (rows - 1.0).max(0.0) * RELEASE_ROW_GAP;
    }

    tabs
}

/// Filter the visible Popular Tracks (title or artist substring) and
/// release-section albums (title substring) against `query`. An empty
/// query restores the full unfiltered view. Runs on the Slint event
/// loop; called by ArtistActions::on_search from main.rs.
pub fn filter_artist(window: &AppWindow, query: &str) {
    let needle = query.trim().to_lowercase();
    let filtered_tracks: Vec<TrackItem> = FULL_TOP_TRACKS.with(|cell| {
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
    let filtered_sections: Vec<DiscoverSection> = FULL_RELEASE_SECTIONS.with(|cell| {
        cell.borrow()
            .iter()
            .filter_map(|section| {
                let kept: Vec<AlbumCardItem> = section
                    .albums
                    .iter()
                    .filter(|album| {
                        needle.is_empty()
                            || album.title.as_str().to_lowercase().contains(&needle)
                    })
                    .collect();
                if kept.is_empty() {
                    return None;
                }
                Some(DiscoverSection {
                    title: section.title.clone(),
                    endpoint: section.endpoint.clone(),
                    albums: ModelRc::new(VecModel::from(kept)),
                })
            })
            .collect()
    });

    let state = window.global::<ArtistState>();
    state.set_top_tracks(ModelRc::new(VecModel::from(filtered_tracks)));
    state.set_release_sections(ModelRc::new(VecModel::from(filtered_sections)));
}

/// Clear artist state before loading a new artist.
pub fn reset_artist(window: &AppWindow) {
    let state = window.global::<ArtistState>();
    state.set_top_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_release_sections(ModelRc::new(VecModel::from(Vec::<DiscoverSection>::new())));
    state.set_labels(ModelRc::new(VecModel::from(Vec::<LabelEntry>::new())));
    state.set_similar_artists(ModelRc::new(VecModel::from(Vec::<SimilarEntry>::new())));
    state.set_jump_tabs(ModelRc::new(VecModel::from(Vec::<JumpNavTab>::new())));
    state.set_artwork(slint::Image::default());
    state.set_name("".into());
    state.set_bio("".into());
    state.set_bio_source("".into());
    state.set_top_tracks_multi_select(false);
    state.set_top_tracks_selected_count(0);
    state.set_loading(true);
}

// ============== Popular Tracks multi-select (mirrors AlbumState) ==========

/// Toggle Popular Tracks multi-select; leaving the mode clears the selection.
pub fn set_multi_select(window: &AppWindow, on: bool) {
    let state = window.global::<ArtistState>();
    state.set_top_tracks_multi_select(on);
    if !on {
        clear_selection(window);
    }
}

/// Recompute the "N selected" count from the Popular Tracks rows.
pub fn recount_selected(window: &AppWindow) {
    let state = window.global::<ArtistState>();
    let model = state.get_top_tracks();
    let count = (0..model.row_count())
        .filter(|&i| model.row_data(i).map(|t| t.selected).unwrap_or(false))
        .count();
    state.set_top_tracks_selected_count(count as i32);
}

/// Select every row, or clear if all are already selected.
pub fn select_all(window: &AppWindow) {
    let model = window.global::<ArtistState>().get_top_tracks();
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
    let model = window.global::<ArtistState>().get_top_tracks();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            if item.selected {
                item.selected = false;
                model.set_row_data(i, item);
            }
        }
    }
    window.global::<ArtistState>().set_top_tracks_selected_count(0);
}

/// Catalog ids of the selected Popular Tracks rows (Qobuz ids only).
pub fn selected_ids(window: &AppWindow) -> Vec<String> {
    let model = window.global::<ArtistState>().get_top_tracks();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|t| t.selected)
        .map(|t| t.id.to_string())
        .filter(|s| s.parse::<u64>().is_ok())
        .collect()
}

/// Catalog ids of ALL Popular Tracks rows (for the section "more" menu's
/// all-tracks actions).
pub fn all_top_track_ids(window: &AppWindow) -> Vec<String> {
    let model = window.global::<ArtistState>().get_top_tracks();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .map(|t| t.id.to_string())
        .filter(|s| s.parse::<u64>().is_ok())
        .collect()
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

/// Reset the network sidebar's MB-driven state and (re)apply the open
/// state on artist change so a stale Origin / Relationships / Discovery
/// never bleeds across artists. The sidebar opens fresh on every artist
/// visit (per user policy — close is per-session, never persisted)
/// EXCEPT when the content area is space-constrained (a Queue / Lyrics
/// right panel is open on a non-wide window). In that case it stays
/// collapsed so the Popular Tracks list keeps priority — mirroring the
/// `!net-cramped` rule the ArtistPageView Slint handlers use. Reading
/// the constraint here (instead of unconditionally force-opening) is
/// what fixes the artist->artist navigation case: when a panel was
/// already open, navigating to a new artist no longer re-opens the
/// sidebar over the Slint handler.
pub fn reset_network_sidebar(window: &AppWindow) {
    // Drop the previous artist's cached location params so a stale
    // scene-view click can't fire for the wrong artist.
    if let Ok(mut guard) = LOCATION_PARAMS.lock() {
        *guard = None;
    }
    // Open only when there's room — mirror ShellState.content-constrained
    // (the same signal AlbumView + the ArtistPageView `net-cramped`
    // handlers use). Constrained => keep collapsed.
    let constrained = window.global::<ShellState>().get_content_constrained();
    let state = window.global::<NetworkSidebarState>();
    state.set_open(!constrained);
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

    // Cache the location params so the Origin location click can
    // open ArtistsByLocationView without re-resolving. Stored in a
    // cross-thread Mutex because the click handler runs on the UI
    // thread while this loads on a worker.
    store_location_params(&mbid, &meta);

    Ok(Some(MbMetadata {
        mbid: mbid.clone(),
        origin: map_origin(&meta),
    }))
}

/// Location parameters for the "artists from the same place" scene
/// view, captured from the Origin metadata. None until an artist's
/// metadata with a location resolves.
#[derive(Clone, Default)]
pub struct LocationParams {
    pub mbid: String,
    pub area_id: String,
    pub area_name: String,
    pub country: String,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
}

static LOCATION_PARAMS: std::sync::Mutex<Option<LocationParams>> =
    std::sync::Mutex::new(None);

fn store_location_params(mbid: &str, meta: &qbz_integrations::musicbrainz::ArtistMetadata) {
    let params = meta.location.as_ref().map(|loc| LocationParams {
        mbid: mbid.to_string(),
        area_id: loc.area_id.clone().unwrap_or_default(),
        area_name: loc
            .city
            .clone()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| loc.display_name.clone()),
        country: loc.country.clone().unwrap_or_default(),
        genres: meta.affinity_seeds.genres.clone(),
        tags: meta.affinity_seeds.tags.clone(),
    });
    if let Ok(mut guard) = LOCATION_PARAMS.lock() {
        *guard = params;
    }
}

/// The location params for the currently loaded artist, if it has a
/// resolved MB location. Read by the Origin location-click handler.
pub fn location_params() -> Option<LocationParams> {
    LOCATION_PARAMS.lock().ok().and_then(|g| g.clone())
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

// ----- MB relationships -------------------------------------------------

/// Plain, `Send` mapped relationships ready to push into Slint. Members
/// here are the still-active ones (ended members already moved to
/// past_members on the qbz-core side, and Tauri's sidebar renders only
/// members — see groupedMembers in ArtistDetailView).
pub struct MbRelationshipsRowData {
    pub members: Vec<MbRelationshipRow>,
    pub groups: Vec<MbRelationshipRow>,
    pub collaborators: Vec<MbRelationshipRow>,
    pub has_data: bool,
}

pub struct MbRelationshipRow {
    pub mbid: String,
    pub name: String,
    /// Primary role for the musician-click callback. Defaults to "Band
    /// Member" / "Band" / "Collaborator" by section when MB has no
    /// attributes for the relation.
    pub role: String,
    /// Tooltip — roles joined with ", " plus the period in parens when
    /// present. Falls back to the period string or the name.
    pub tooltip: String,
}

/// Fetch MB relationships for `mbid` and map into the Slint-friendly
/// row shape. Groups members by mbid combining their roles, mirroring
/// Tauri's `groupMembersByMbid` plus the per-section role defaults.
pub async fn load_mb_relationships<A>(
    runtime: &Arc<AppRuntime<A>>,
    mbid: &str,
) -> Result<MbRelationshipsRowData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let relations = runtime
        .core()
        .musicbrainz_get_artist_relationships(mbid)
        .await
        .map_err(|e| e.to_string())?;
    Ok(map_relationships(relations))
}

fn map_relationships(
    rels: qbz_integrations::musicbrainz::ArtistRelationships,
) -> MbRelationshipsRowData {
    let members = group_relations(rels.members, "Band Member");
    let groups = group_relations(rels.groups, "Band");
    let collaborators = group_relations(rels.collaborators, "Collaborator");
    let has_data =
        !members.is_empty() || !groups.is_empty() || !collaborators.is_empty();
    MbRelationshipsRowData {
        members,
        groups,
        collaborators,
        has_data,
    }
}

fn group_relations(
    rels: Vec<qbz_integrations::musicbrainz::RelatedArtist>,
    default_role: &str,
) -> Vec<MbRelationshipRow> {
    use std::collections::HashMap;
    struct Pending {
        name: String,
        roles: Vec<String>,
        begin: Option<String>,
        end: Option<String>,
    }
    let mut by_mbid: HashMap<String, Pending> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for r in rels {
        let begin = r.period.as_ref().and_then(|p| p.begin.clone());
        let end = r.period.as_ref().and_then(|p| p.end.clone());
        match by_mbid.get_mut(&r.mbid) {
            Some(existing) => {
                if let Some(role) = r.role.clone() {
                    if !existing.roles.iter().any(|rr| rr == &role) {
                        existing.roles.push(role);
                    }
                }
            }
            None => {
                order.push(r.mbid.clone());
                let mut roles = Vec::new();
                if let Some(role) = r.role.clone() {
                    roles.push(role);
                }
                by_mbid.insert(
                    r.mbid.clone(),
                    Pending {
                        name: r.name,
                        roles,
                        begin,
                        end,
                    },
                );
            }
        }
    }
    order
        .into_iter()
        .filter_map(|mbid| by_mbid.remove(&mbid).map(|p| (mbid, p)))
        .map(|(mbid, p)| {
            let period = format_period(p.begin.as_deref(), p.end.as_deref());
            let tooltip = if !p.roles.is_empty() {
                let roles_joined = p.roles.join(", ");
                if period.is_empty() {
                    roles_joined
                } else {
                    format!("{} ({})", roles_joined, period)
                }
            } else if !period.is_empty() {
                period.clone()
            } else {
                p.name.clone()
            };
            let role = p
                .roles
                .first()
                .cloned()
                .unwrap_or_else(|| default_role.to_string());
            MbRelationshipRow {
                mbid,
                name: p.name,
                role,
                tooltip,
            }
        })
        .collect()
}

fn format_period(begin: Option<&str>, end: Option<&str>) -> String {
    if begin.is_some() || end.is_some() {
        let b = begin.unwrap_or("?");
        let e = end.unwrap_or("present");
        format!("{} - {}", b, e)
    } else {
        String::new()
    }
}

// ----- MB discovery -----------------------------------------------------

/// Plain, `Send` payload for the Discovery section. `primary_tag` is
/// kept alongside so the dismiss callback can look up the right key in
/// the dismiss store.
pub struct MbDiscoveryData {
    pub primary_tag: String,
    pub artists: Vec<MbDiscoveryRow>,
}

#[derive(Clone)]
pub struct MbDiscoveryRow {
    pub mbid: String,
    pub name: String,
    pub qobuz_id: String,
}

/// Load discovery candidates for `seed_mbid` (the artist's MB id) using
/// `similar_names` to suppress already-shown rows and the local
/// discovery_dismiss store to suppress thumbs-downed rows.
pub async fn load_mb_discovery<A>(
    runtime: &Arc<AppRuntime<A>>,
    seed_mbid: &str,
    seed_name: &str,
    similar_names: Vec<String>,
) -> Result<MbDiscoveryData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    // Tauri's listen threshold: artists with strictly more than 2
    // plays count as "already known" and are excluded from
    // suggestions.
    let known_threshold: u32 = 2;
    let response = runtime
        .core()
        .musicbrainz_discover_artists(
            seed_mbid,
            seed_name,
            &similar_names,
            &|tag| crate::discovery_dismiss::dismissed_for_tag(tag),
            &|| crate::play_history::known_artists(known_threshold),
        )
        .await
        .map_err(|e| e.to_string())?;

    let artists = response
        .artists
        .into_iter()
        .map(|a| MbDiscoveryRow {
            mbid: a.mbid,
            name: a.name,
            qobuz_id: a.qobuz_id.map(|id| id.to_string()).unwrap_or_default(),
        })
        .collect();

    Ok(MbDiscoveryData {
        primary_tag: response.primary_tag,
        artists,
    })
}

/// Apply discovery candidates to NetworkSidebarState. Runs on the
/// Slint event loop. `primary_tag` is stored on the sidebar state for
/// the dismiss callback to read.
pub fn apply_mb_discovery(window: &AppWindow, data: MbDiscoveryData) {
    let state = window.global::<NetworkSidebarState>();
    state.set_discovery_tag(data.primary_tag.into());
    let rows: Vec<DiscoveryArtist> = data
        .artists
        .into_iter()
        .map(|r| DiscoveryArtist {
            mbid: r.mbid.into(),
            name: r.name.into(),
            qobuz_id: r.qobuz_id.into(),
        })
        .collect();
    state.set_discovery_artists(ModelRc::new(VecModel::from(rows)));
    state.set_discovery_loading(false);
}

/// Remove a dismissed row from the visible Discovery list. The dismiss
/// store persistence is handled by the caller before this is invoked.
pub fn remove_discovery_artist(window: &AppWindow, mbid: &str) {
    let state = window.global::<NetworkSidebarState>();
    let model = state.get_discovery_artists();
    let kept: Vec<DiscoveryArtist> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|row| row.mbid.as_str() != mbid)
        .collect();
    state.set_discovery_artists(ModelRc::new(VecModel::from(kept)));
}

/// Apply MB relationships to NetworkSidebarState. Runs on the Slint
/// event loop.
pub fn apply_mb_relationships(window: &AppWindow, data: MbRelationshipsRowData) {
    let to_slint = |rows: Vec<MbRelationshipRow>| -> ModelRc<MbRelationship> {
        ModelRc::new(VecModel::from(
            rows.into_iter()
                .map(|r| MbRelationship {
                    mbid: r.mbid.into(),
                    name: r.name.into(),
                    role: r.role.into(),
                    tooltip: r.tooltip.into(),
                })
                .collect::<Vec<_>>(),
        ))
    };
    let state = window.global::<NetworkSidebarState>();
    state.set_relationships(MbRelationshipsData {
        members: to_slint(data.members),
        groups: to_slint(data.groups),
        collaborators: to_slint(data.collaborators),
        has_data: data.has_data,
    });
    state.set_relationships_loading(false);
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
