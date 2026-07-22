//! Artist detail controller.
//!
//! Fetches an artist page through `QbzCore`, maps it to plain (Send)
//! data on the worker thread, and applies it to the `ArtistState`
//! global on the Slint event loop.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{
    ArtistStoryItem, PageArtistRelease, PageArtistResponse, PageArtistTrack,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::album::TrackData;
use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::home::CardData;
use crate::{
    AlbumCardItem, TrackItem, AppWindow, ArtistReleaseSection, ArtistState, DiscoveryArtist,
    JumpNavTab, LabelEntry, MbOriginData, MbRelationship, MbRelationshipsData,
    NetworkSidebarState, SearchPlaylistItem, SettingsState, ShellState, SimilarEntry, StoryItem,
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
    /// "Novedad más reciente" — the single highlighted latest release
    /// (`last_release` in /artist/page). None when the API omits it.
    pub last_release: Option<CardData>,
    /// "Appears On" (`tracks_appears_on`) — tracks where the artist guests,
    /// rendered as a flat track section (NOT albums).
    pub appears_on: Vec<TrackData>,
    /// Releases grouped into titled sections (Albums, EPs & Singles, ...).
    pub release_sections: Vec<ReleaseSection>,
    /// Labels collected from the artist's own album releases (deduped
    /// by id, sorted by name) — sidebar Labels section.
    pub labels: Vec<LabelData>,
    /// Similar artists from /artist/page — sidebar Similar Artists.
    pub similar_artists: Vec<SimilarArtistData>,
    /// Curated playlists featuring this artist (the /artist/page `playlists`
    /// section) — main-column Playlists carousel, above the "Other" block.
    pub playlists: Vec<PlaylistSlim>,
}

/// One curated playlist card for the artist Playlists carousel.
#[derive(Clone)]
pub struct PlaylistSlim {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub image_url: String,
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
    /// Raw server `release_type` key (album/epSingle/live/…). Stable id
    /// for jump-tabs, sort persistence (Phase 2) and "see discography".
    pub release_type: String,
    pub title: String,
    /// Server `has_more` for this bucket — gates the per-section load-more.
    pub has_more: bool,
    pub cards: Vec<CardData>,
}

/// Official on-screen order of release buckets, with their display titles
/// (webplayer-faithful). `release_type` keys come straight from the server.
// Display titles are `mark`ed so the extractor registers the English literals;
// they are translated once with `t(...)` at the consumer sites (the section
// header, the jump-tab label, and `release_type_title`).
const RELEASE_SECTION_ORDER: &[(&str, &str)] = &[
    ("album", qbz_i18n::mark("Albums")),
    ("epSingle", qbz_i18n::mark("EPs & Singles")),
    ("ep", qbz_i18n::mark("EPs & Singles")),
    ("single", qbz_i18n::mark("EPs & Singles")),
    ("live", qbz_i18n::mark("Live")),
    ("compilation", qbz_i18n::mark("Compilations")),
    ("download", qbz_i18n::mark("Purchase Only")),
    ("composer", qbz_i18n::mark("Composer")),
    ("other", qbz_i18n::mark("Other")),
    ("awardedRelease", qbz_i18n::mark("Critics' Picks")),
    ("next", qbz_i18n::mark("Upcoming")),
];

/// Display title for a release_type (the dedicated discography page header).
pub fn release_type_title(release_type: &str) -> String {
    RELEASE_SECTION_ORDER
        .iter()
        .find(|(rt, _)| *rt == release_type)
        .map(|(_, title)| qbz_i18n::t(title))
        .unwrap_or_else(|| title_case(release_type))
}

/// Title-case a raw release_type key for unknown buckets (fallback only).
fn title_case(key: &str) -> String {
    let mut chars = key.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
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

/// Fetch one more page of an artist's releases for a given bucket via
/// `get_releases_grid` (the reused, already-wired endpoint). Returns the
/// mapped cards + the server `has_more` flag.
pub async fn load_release_page<A>(
    runtime: &Arc<AppRuntime<A>>,
    artist_id: &str,
    release_type: &str,
    offset: u32,
) -> Result<(Vec<CardData>, bool), String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let id: u64 = artist_id
        .parse()
        .map_err(|_| format!("invalid artist id: {artist_id}"))?;
    let resp = runtime
        .core()
        .get_releases_grid(id, release_type, RELEASE_PAGE_SIZE, offset, Some("release_date"))
        .await
        .map_err(|e| e.to_string())?;
    let has_more = resp.has_more;
    let cards = resp
        .items
        .into_iter()
        .map(map_release)
        .filter(|c| !crate::artist_blacklist::card_blacklisted(&c.id, &c.artist_id))
        .collect();
    Ok((cards, has_more))
}

/// One Magazine/Stories teaser for the sidebar.
pub struct StoryData {
    pub title: String,
    pub author: String,
    pub excerpt: String,
    pub url: String,
    pub image_url: String,
}

fn map_story(item: ArtistStoryItem) -> StoryData {
    let author = item
        .authors
        .and_then(|list| list.into_iter().next())
        .map(|a| a.name)
        .unwrap_or_default();
    // `image` is a ready-to-use arc-cdn URL; fall back to the first `images[]`.
    let image_url = item
        .image
        .or_else(|| {
            item.images
                .and_then(|list| list.into_iter().next())
                .map(|img| img.url)
        })
        .unwrap_or_default();
    StoryData {
        url: format!("https://play.qobuz.com/magazine/story/{}", item.id),
        // Magazine content comes from a CMS: titles carry entities
        // (&amp; …), excerpts may additionally carry markup.
        title: crate::strip_html::decode_html_entities(&item.title),
        author,
        excerpt: item
            .description_short
            .as_deref()
            .map(crate::strip_html::strip_html)
            .unwrap_or_default(),
        image_url,
    }
}

/// Fetch the artist's Magazine stories (limit 2, like the official client).
/// Returns an empty list on any failure (the section just stays hidden).
pub async fn load_stories<A>(runtime: &Arc<AppRuntime<A>>, artist_id: &str) -> Vec<StoryData>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let Ok(id) = artist_id.parse::<u64>() else {
        return Vec::new();
    };
    match runtime.core().get_artist_story(id, 0, 2).await {
        Ok(resp) => resp.items.into_iter().map(map_story).collect(),
        Err(e) => {
            log::warn!("[qbz-slint] artist story load failed: {e}");
            Vec::new()
        }
    }
}

/// Apply fetched stories to the sidebar Magazine tab. Returns artwork jobs
/// for the thumbnails (caller spawns them). UI thread.
pub fn apply_stories(window: &AppWindow, stories: Vec<StoryData>) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    let items: Vec<StoryItem> = stories
        .into_iter()
        .enumerate()
        .map(|(index, s)| {
            if !s.image_url.is_empty() {
                jobs.push(ArtworkJob {
                    target: ArtworkTarget::ArtistStory { index },
                    url: s.image_url.clone(),
                });
            }
            StoryItem {
                title: s.title.into(),
                author: s.author.into(),
                excerpt: s.excerpt.into(),
                url: s.url.into(),
                image_url: s.image_url.into(),
                image: slint::Image::default(),
            }
        })
        .collect();
    let st = window.global::<ArtistState>();
    st.set_stories(ModelRc::new(VecModel::from(items)));
    st.set_stories_loading(false);
    jobs
}

fn map_artist(page: PageArtistResponse) -> ArtistData {
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
            // Entity-decode (no tags expected in a source name, but the
            // same CMS that emits `&copy` in bodies feeds this field).
            let source = biography
                .source
                .and_then(|v| v.as_str().map(crate::strip_html::decode_html_entities))
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

    // Releases: server-driven bucketing (the official webplayer is the
    // source of truth). Each `releases[]` group is keyed by its own
    // `type` (release_type) — we render EVERY non-empty bucket, in the
    // official order, trusting the server's key. We never re-derive
    // buckets by heuristic and never collapse them into a curated few.
    // Foreign-artist releases (guest spots that surface inside a group)
    // are filtered out, and ids are deduped across groups so a release
    // listed in more than one group appears once. The `awardedRelease`
    // bucket can appear twice in the array (a server quirk) — keying by
    // release_type naturally folds the two into one section.
    let mut bucket_cards: HashMap<String, Vec<CardData>> = HashMap::new();
    let mut bucket_has_more: HashMap<String, bool> = HashMap::new();
    let mut seen_release_ids: HashSet<String> = HashSet::new();
    // Labels collected while iterating the artist's own album releases.
    // Only group.type == "album", only own releases, dedupe by label id.
    let mut labels_by_id: BTreeMap<u64, String> = BTreeMap::new();

    for group in page.releases.into_iter().flatten() {
        let release_type = group.release_type.clone();
        let is_album_group = release_type == "album";
        *bucket_has_more.entry(release_type.clone()).or_insert(false) |= group.has_more;
        for release in group.items.into_iter() {
            // NO foreign-artist filter: the official webplayer renders every
            // item the server placed in the bucket — including releases
            // credited to the artist's band or where they only guest (e.g.
            // Vicky Psarakis' albums are credited to "Sicksense"). The old
            // `artist.id == page.id` filter dropped exactly those, hiding a
            // real Albums section. Trust the server's bucketing (D3).
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

            bucket_cards
                .entry(release_type.clone())
                .or_default()
                .push(map_release(release));
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

    // Curated playlists featuring this artist (the /artist/page `playlists`
    // section) — rendered as a main-column carousel above the "Other" block.
    let playlists: Vec<PlaylistSlim> = page
        .playlists
        .map(|p| {
            p.items
                .into_iter()
                .map(|pl| {
                    let owner = pl
                        .owner
                        .and_then(|o| o.name)
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "Qobuz".to_string());
                    let track_count = pl.tracks_count.unwrap_or(0);
                    let image_url = pl
                        .images
                        .and_then(|imgs| imgs.rectangle)
                        .and_then(|rects| rects.into_iter().find(|s| !s.is_empty()))
                        .unwrap_or_default();
                    PlaylistSlim {
                        id: pl.id.to_string(),
                        title: pl.title.unwrap_or_default(),
                        subtitle: format!("{owner} · {track_count}"),
                        image_url,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // "Novedad más reciente" — the single latest-release highlight.
    // Drop a blocked "Latest release" at the SOURCE so has_last_release is
    // false (section hidden) and no stale cover job is queued.
    let last_release = page
        .last_release
        .map(map_release)
        .filter(|c| !crate::artist_blacklist::card_blacklisted(&c.id, &c.artist_id));

    // "Appears On" — tracks where the artist guests (tracks_appears_on).
    // These are TRACKS, not albums; rendered as a flat track section.
    let appears_on: Vec<TrackData> = page
        .tracks_appears_on
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, track)| map_track(index, track))
        .collect();

    // Emit one section per non-empty bucket in the official on-screen
    // order. Buckets the server adds in the future that aren't in this
    // list are appended at the end (still rendered, just untitled-mapped).
    let mut release_sections: Vec<ReleaseSection> = Vec::new();
    for &(rt, title) in RELEASE_SECTION_ORDER {
        // "download" ("Purchase Only") is intentionally hidden — drain it so
        // it can't resurface in the leftovers pass, but emit no section.
        if rt == "download" {
            bucket_cards.remove(rt);
            continue;
        }
        // `.remove` drains the bucket so the leftovers pass below can't
        // re-emit an already-rendered type.
        if let Some(cards) = bucket_cards.remove(rt) {
            if cards.is_empty() {
                continue;
            }
            release_sections.push(ReleaseSection {
                release_type: rt.to_string(),
                title: title.to_string(),
                has_more: bucket_has_more.get(rt).copied().unwrap_or(false),
                cards,
            });
        }
    }
    // Any unknown bucket types the order list doesn't cover — append them
    // last, titled from their raw key (rare; keeps D3 faithful).
    let mut leftovers: Vec<(String, Vec<CardData>)> = bucket_cards
        .into_iter()
        .filter(|(_, cards)| !cards.is_empty())
        .collect();
    leftovers.sort_by(|a, b| a.0.cmp(&b.0));
    for (rt, cards) in leftovers {
        let has_more = bucket_has_more.get(&rt).copied().unwrap_or(false);
        release_sections.push(ReleaseSection {
            title: title_case(&rt),
            release_type: rt,
            has_more,
            cards,
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
        last_release,
        appears_on,
        release_sections,
        labels,
        similar_artists,
        playlists,
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
    if let Some(card) = data.last_release.as_ref() {
        // The model drops a blocked last-release (apply_artist), so skip its job
        // too — otherwise the cover would land on the empty slot.
        if !card.artwork_url.is_empty()
            && !crate::artist_blacklist::card_blacklisted(&card.id, &card.artist_id)
        {
            jobs.push(ArtworkJob {
                target: ArtworkTarget::ArtistLastRelease,
                url: card.artwork_url.clone(),
            });
        }
    }
    for (section_idx, section) in data.release_sections.iter().enumerate() {
        // album_idx must be the POST-FILTER index so it matches the filtered
        // model apply_artist builds; otherwise a blocked card shifts every
        // subsequent cover onto the wrong album (and clicks open the wrong one).
        let mut album_idx = 0;
        for card in section.cards.iter() {
            if crate::artist_blacklist::card_blacklisted(&card.id, &card.artist_id) {
                continue;
            }
            if !card.artwork_url.is_empty() {
                jobs.push(ArtworkJob {
                    target: ArtworkTarget::ArtistRelease {
                        section_idx,
                        album_idx,
                    },
                    url: card.artwork_url.clone(),
                });
            }
            album_idx += 1;
        }
    }
    // "Popular Tracks" rows carry the album-cover URL but Slint can't fetch
    // network images — decode each into the row's `artwork` (#631). The
    // top-tracks model is built 1:1 from `data.top_tracks` (no blacklist
    // filter, unlike releases), so the enumerate index matches the row.
    for (i, track) in data.top_tracks.iter().enumerate() {
        if !track.artwork_url.is_empty() {
            jobs.push(ArtworkJob {
                target: ArtworkTarget::ArtistTopTrack { index: i },
                url: track.artwork_url.clone(),
            });
        }
    }
    // Curated playlist covers (single rectangle cover per card).
    for (i, playlist) in data.playlists.iter().enumerate() {
        if !playlist.image_url.is_empty() {
            jobs.push(ArtworkJob {
                target: ArtworkTarget::ArtistPlaylistCover { index: i },
                url: playlist.image_url.clone(),
            });
        }
    }
    jobs
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
    // Pull the album id, title AND cover from the same nested album object —
    // the /artist/page response already carries all three, so the row can show
    // the album name + thumbnail without an extra request. `smallest()` is the
    // list-row thumbnail variant (best() would download the mega/large cover).
    let (album_id, album, artwork_url) = track
        .album
        .map(|a| {
            let url = a.image.and_then(|img| img.smallest().cloned()).unwrap_or_default();
            (a.id, a.title, url)
        })
        .unwrap_or_default();
    let bit_depth = track.audio_info.as_ref().and_then(|a| a.maximum_bit_depth);
    let sample_rate = track.audio_info.as_ref().and_then(|a| a.maximum_sampling_rate);
    TrackData {
        id: track.id.to_string(),
        number: (index + 1).to_string(),
        title,
        artist,
        artist_id,
        album_id,
        album,
        artwork_url,
        duration: mmss(track.duration.unwrap_or(0)),
        quality_tier: tier(bit_depth).to_string(),
        quality_detail: crate::quality::detail(bit_depth, sample_rate),
        explicit: track.parental_warning.unwrap_or(false),
        // Artist top-tracks are a flat cross-album list and never render
        // "Disc N" headers, so the disc value is unused here — default to 1.
        disc: 1,
        // Work-section headers are album-view only; the flat artist list never
        // renders them, so leave them empty.
        work: String::new(),
        work_composer_name: String::new(),
        work_composer_id: String::new(),
    }
}

pub(crate) fn map_release(release: PageArtistRelease) -> CardData {
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
    // Per-release press award → gold ribbon (the AlbumCard "press" ribbon
    // is already styled). First award name wins.
    let (ribbon, ribbon_kind) = release
        .awards
        .as_ref()
        .and_then(|list| list.first())
        .map(|award| (award.name.clone(), "press".to_string()))
        .unwrap_or_default();
    CardData {
        id: release.id,
        title: crate::album_map::format_album_title(&release.title, release.version.as_deref()),
        artist,
        artist_id: String::new(),
        genre: release.genre.map(|g| g.name).unwrap_or_default(),
        year,
        quality_tier,
        quality_label,
        ribbon,
        ribbon_kind,
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

// strip_html now lives in `crate::strip_html` (qbz-text-utils) so both
// album and artist views use the same paragraph-preserving conversion,
// with full entity decoding (named + numeric + the malformed
// no-semicolon `&copy` TiVo emits in biography credit lines).

pub(crate) fn card_to_item(card: CardData) -> AlbumCardItem {
    // On the artist page the card subtitle slot should show the
    // release year — the artist is redundant since we're already on
    // their page. The AlbumCard reads `artist` for its subtitle line,
    // so re-route year through that field instead of changing the
    // shared card primitive.
    AlbumCardItem {
        plays: 0,
        // Favorite heart state from the login-seeded cache (kept live by
        // main::set_album_row_favorite when a favorite toggles anywhere).
        is_favorite: crate::fav_cache::is_album_favorite(&card.id),
        // Pin badge state from the per-user pinned store (kept live by
        // main::set_album_row_pinned when a pin toggles anywhere).
        is_pinned: crate::pinned::is_pinned("album", &card.id),
        id: card.id.into(),
        title: card.title.into(),
        artist: card.year.clone().into(),
        artist_id: "".into(),
        genre: card.genre.into(),
        plain_year: card.year.clone().into(),
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
    static FULL_APPEARS_ON: std::cell::RefCell<Vec<TrackItem>> =
        std::cell::RefCell::new(Vec::new());
    static FULL_RELEASE_SECTIONS: std::cell::RefCell<Vec<ArtistReleaseSection>> =
        std::cell::RefCell::new(Vec::new());
    // Per-release-type pages already loaded into the index (1 = the initial
    // /artist/page bucket). The index caps at MAX_INDEX_PAGES; beyond that
    // the dedicated discography page takes over.
    static LOADED_PAGES: std::cell::RefCell<HashMap<String, u32>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Index-page load-more cap (the user asked EPs & Singles / Live to page
/// up to 4). Page 1 is the embedded bucket; 3 more loads reach the cap.
pub const MAX_INDEX_PAGES: u32 = 4;
/// Items fetched per `get_releases_grid` load-more page.
pub const RELEASE_PAGE_SIZE: u32 = 20;

/// Build a Slint `TrackItem` from a controller `TrackData`, stamping
/// favorite/cache status. Shared by Popular Tracks and Appears On (both
/// flat cross-album lists — no disc headers, no per-row blacklist greyout).
fn track_data_to_item(track: TrackData) -> TrackItem {
    TrackItem {
        is_blacklisted: false,
        id: track.id.clone().into(),
        number: track.number.into(),
        title: track.title.into(),
        artist: track.artist.into(),
        album: track.album.clone().into(),
        duration: track.duration.into(),
        quality_tier: track.quality_tier.into(),
        quality_detail: track.quality_detail.into(),
        explicit: track.explicit,
        selected: false,
        artwork_url: track.artwork_url.clone().into(),
        artwork: slint::Image::default(),
        is_favorite: crate::fav_cache::is_favorite(&track.id),
        artist_id: track.artist_id.into(),
        album_id: track.album_id.into(),
        removing: false,
        cache_status: if crate::offline_cache::is_cached(&track.id) { 3 } else { 0 },
        cache_progress: 0.0,
        source: "qobuz".into(),
        unlocking: false,
        disc_header_number: 0,
        work_header: "".into(),
        work_composer_name: "".into(),
        work_composer_id: "".into(),
    }
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

    let appears_on_count = data.appears_on.len();
    let has_last_release = data.last_release.is_some();

    let top_tracks: Vec<TrackItem> = data
        .top_tracks
        .into_iter()
        .map(track_data_to_item)
        .collect();
    let appears_on: Vec<TrackItem> = data
        .appears_on
        .into_iter()
        .map(track_data_to_item)
        .collect();
    let last_release_item = data
        .last_release
        .filter(|c| !crate::artist_blacklist::card_blacklisted(&c.id, &c.artist_id))
        .map(card_to_item)
        .unwrap_or_default();
    let release_sections: Vec<ArtistReleaseSection> = data
        .release_sections
        .into_iter()
        .map(|section| {
            // Apply the persisted per-bucket sort up front so the first paint
            // already honors the user's choice.
            let sort = crate::artist_prefs::get_sort(&section.release_type);
            // Drop blocked albums (own id). The artist axis is moot here (you're
            // on the artist's own page) and CardData.artist_id is blank anyway.
            let mut albums: Vec<AlbumCardItem> = section
                .cards
                .into_iter()
                .filter(|c| !crate::artist_blacklist::card_blacklisted(&c.id, &c.artist_id))
                .map(card_to_item)
                .collect();
            crate::album_map::sort_album_items(&mut albums, &sort);
            ArtistReleaseSection {
                release_type: section.release_type.into(),
                // `section.title` is the English bucket title (kept English in
                // `map_artist` so jump-tab routing matches); translate for display.
                title: qbz_i18n::t(&section.title).into(),
                albums: ModelRc::new(VecModel::from(albums)),
                has_more: section.has_more,
                sort_by: sort.into(),
            }
        })
        .collect();

    // Reset the per-bucket page counters to 1 (the embedded bucket).
    LOADED_PAGES.with(|cell| {
        let mut m = cell.borrow_mut();
        m.clear();
        for s in &release_sections {
            m.insert(s.release_type.to_string(), 1);
        }
    });

    let jump_tabs = build_jump_tabs(
        top_tracks_count,
        has_last_release,
        &section_counts,
        appears_on_count,
    );

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
    let playlists: Vec<SearchPlaylistItem> =
        data.playlists.iter().map(playlist_to_item).collect();

    // Cache the full models on the UI thread so the in-page search
    // can rebuild filtered views without re-fetching the artist.
    FULL_TOP_TRACKS.with(|cell| {
        *cell.borrow_mut() = top_tracks.clone();
    });
    FULL_APPEARS_ON.with(|cell| {
        *cell.borrow_mut() = appears_on.clone();
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
    state.set_has_last_release(has_last_release);
    state.set_last_release(last_release_item);
    state.set_appears_on(ModelRc::new(VecModel::from(appears_on)));
    state.set_release_sections(ModelRc::new(VecModel::from(release_sections)));
    state.set_labels(ModelRc::new(VecModel::from(labels)));
    state.set_similar_artists(ModelRc::new(VecModel::from(similar_artists)));
    state.set_playlists(ModelRc::new(VecModel::from(playlists)));
    state.set_jump_tabs(ModelRc::new(VecModel::from(jump_tabs)));
}

/// Map an artist-page curated playlist to the shared collage card model
/// (single cover, slot 0 — filled by ArtworkTarget::ArtistPlaylistCover).
/// Mirrors `label::playlist_to_item`.
fn playlist_to_item(p: &PlaylistSlim) -> SearchPlaylistItem {
    SearchPlaylistItem {
        id: p.id.clone().into(),
        title: p.title.clone().into(),
        subtitle: p.subtitle.clone().into(),
        is_pinned: crate::pinned::is_pinned("playlist", &p.id),
        cover_count: if p.image_url.is_empty() { 0 } else { 1 },
        url1: p.image_url.clone().into(),
        url2: "".into(),
        url3: "".into(),
        url4: "".into(),
        cover1: slint::Image::default(),
        cover2: slint::Image::default(),
        cover3: slint::Image::default(),
        cover4: slint::Image::default(),
        category: "".into(),
        dominant_color: slint::Color::from_argb_u8(0, 0, 0, 0),
        // Artist-page playlists are foreign Qobuz playlists → follow + copy.
        is_owned: false,
        is_following: false,
        is_copied: false,
    }
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
    has_last_release: bool,
    sections: &[(String, usize)],
    appears_on_count: usize,
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
    // "Novedad más reciente" highlight block (header + one card row).
    const LAST_RELEASE_BLOCK: f32 = 172.0;

    let mut tabs: Vec<JumpNavTab> = Vec::new();
    tabs.push(JumpNavTab {
        id: "about".into(),
        label: qbz_i18n::t("About").into(),
        anchor_y: 0.0,
    });

    let mut cursor = BODY_ROW_TOP_GUESS;
    if top_tracks_count > 0 {
        tabs.push(JumpNavTab {
            id: "popular-tracks".into(),
            label: qbz_i18n::t("Popular Tracks").into(),
            anchor_y: cursor,
        });
        let visible_rows = top_tracks_count.min(5) as f32;
        cursor +=
            POPULAR_HEADER + POPULAR_HEADER_GAP + visible_rows * POPULAR_ROW + POPULAR_TAIL;
    }

    // The latest-release highlight has no jump tab (it's a highlight, not a
    // browsable section) but it shifts every section below it.
    if has_last_release {
        cursor += LAST_RELEASE_BLOCK;
    }

    for (title, count) in sections {
        // Route by display title → stable jump-tab id. Unknown titles still
        // render as sections; they just don't get a jump tab.
        let id = match title.as_str() {
            "Albums" => "albums",
            "EPs & Singles" => "eps-singles",
            "Live" => "live",
            "Compilations" => "compilations",
            "Purchase Only" => "purchase-only",
            "Composer" => "composer",
            // "Other" is rendered LAST + collapsed (below Appears On), so it
            // gets no jump tab and does not occupy main-flow height here.
            "Critics' Picks" => "critics-picks",
            "Upcoming" => "upcoming",
            _ => continue,
        };
        tabs.push(JumpNavTab {
            id: id.into(),
            // `title` is the English bucket title used for id routing above;
            // translate only the displayed label.
            label: qbz_i18n::t(title).into(),
            anchor_y: cursor,
        });
        let rows = (*count as f32 / RELEASE_COLS).ceil().max(1.0);
        cursor += SECTION_SPACER
            + RELEASE_HEADER
            + rows * RELEASE_ROW
            + (rows - 1.0).max(0.0) * RELEASE_ROW_GAP;
    }

    if appears_on_count > 0 {
        tabs.push(JumpNavTab {
            id: "appears-on".into(),
            label: qbz_i18n::t("Appears On").into(),
            anchor_y: cursor,
        });
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
    let filtered_sections: Vec<ArtistReleaseSection> = FULL_RELEASE_SECTIONS.with(|cell| {
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
                Some(ArtistReleaseSection {
                    release_type: section.release_type.clone(),
                    title: section.title.clone(),
                    albums: ModelRc::new(VecModel::from(kept)),
                    // No load-more while a search filter is active (it would
                    // append unfiltered items); restore on empty query.
                    has_more: if needle.is_empty() { section.has_more } else { false },
                    sort_by: section.sort_by.clone(),
                })
            })
            .collect()
    });

    let filtered_appears: Vec<TrackItem> = FULL_APPEARS_ON.with(|cell| {
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

    let state = window.global::<ArtistState>();
    state.set_top_tracks(ModelRc::new(VecModel::from(filtered_tracks)));
    state.set_appears_on(ModelRc::new(VecModel::from(filtered_appears)));
    state.set_release_sections(ModelRc::new(VecModel::from(filtered_sections)));
}

/// Flip the favorite heart on every discography card matching `album_id`:
/// the visible release sections, the last-release highlight card, and the
/// FULL section cache the in-page search rebuilds from (without the cache
/// pass, clearing a search filter would restore a stale heart). Called by
/// `main::set_album_row_favorite` whenever an album favorite toggles.
pub fn set_release_card_favorite(window: &AppWindow, album_id: &str, favorite: bool) {
    let flip = |model: &ModelRc<AlbumCardItem>| {
        for i in 0..model.row_count() {
            if let Some(mut item) = model.row_data(i) {
                if item.id == album_id && item.is_favorite != favorite {
                    item.is_favorite = favorite;
                    model.set_row_data(i, item);
                }
            }
        }
    };
    let state = window.global::<ArtistState>();
    let sections = state.get_release_sections();
    for s in 0..sections.row_count() {
        if let Some(section) = sections.row_data(s) {
            flip(&section.albums);
        }
    }
    // "In library" grid (the catalog/library header toggle).
    flip(&state.get_library_albums());
    let mut last = state.get_last_release();
    if last.id == album_id && last.is_favorite != favorite {
        last.is_favorite = favorite;
        state.set_last_release(last);
    }
    // The FULL cache shares the visible sections' ModelRc while no filter is
    // active (the `!= favorite` guard makes the second pass a no-op then);
    // under a filter it is a separate copy that must be flipped too.
    FULL_RELEASE_SECTIONS.with(|cell| {
        for section in cell.borrow().iter() {
            flip(&section.albums);
        }
    });
}

/// Pin twin of [`set_release_card_favorite`]: flip the `is-pinned` badge on
/// every artist-page release card matching `album_id` (sections +
/// last-release + the in-page-search FULL cache).
pub fn set_release_card_pinned(window: &AppWindow, album_id: &str, pinned: bool) {
    let flip = |model: &ModelRc<AlbumCardItem>| {
        for i in 0..model.row_count() {
            if let Some(mut item) = model.row_data(i) {
                if item.id == album_id && item.is_pinned != pinned {
                    item.is_pinned = pinned;
                    model.set_row_data(i, item);
                }
            }
        }
    };
    let state = window.global::<ArtistState>();
    let sections = state.get_release_sections();
    for s in 0..sections.row_count() {
        if let Some(section) = sections.row_data(s) {
            flip(&section.albums);
        }
    }
    // "In library" grid (the catalog/library header toggle).
    flip(&state.get_library_albums());
    let mut last = state.get_last_release();
    if last.id == album_id && last.is_pinned != pinned {
        last.is_pinned = pinned;
        state.set_last_release(last);
    }
    // The FULL cache shares the visible sections' ModelRc while no filter is
    // active (the `!= pinned` guard makes the second pass a no-op then);
    // under a filter it is a separate copy that must be flipped too.
    FULL_RELEASE_SECTIONS.with(|cell| {
        for section in cell.borrow().iter() {
            flip(&section.albums);
        }
    });
}

/// Clear artist state before loading a new artist.
pub fn reset_artist(window: &AppWindow) {
    let state = window.global::<ArtistState>();
    state.set_top_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_appears_on(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_has_last_release(false);
    state.set_last_release(AlbumCardItem::default());
    state.set_stories(ModelRc::new(VecModel::from(Vec::<StoryItem>::new())));
    state.set_stories_loading(true);
    state.set_release_sections(ModelRc::new(VecModel::from(Vec::<ArtistReleaseSection>::new())));
    LOADED_PAGES.with(|cell| cell.borrow_mut().clear());
    state.set_labels(ModelRc::new(VecModel::from(Vec::<LabelEntry>::new())));
    state.set_similar_artists(ModelRc::new(VecModel::from(Vec::<SimilarEntry>::new())));
    state.set_jump_tabs(ModelRc::new(VecModel::from(Vec::<JumpNavTab>::new())));
    state.set_artwork(slint::Image::default());
    state.set_header_atmosphere(slint::Image::default());
    state.set_name("".into());
    state.set_bio("".into());
    state.set_bio_source("".into());
    state.set_top_tracks_multi_select(false);
    state.set_top_tracks_selected_count(0);
    state.set_is_blacklisted(false);
    state.set_playlists(ModelRc::new(VecModel::from(Vec::<SearchPlaylistItem>::new())));
    // Catalog/library toggle — reset so an artist WITHOUT library items (apply
    // only seeds these when the index has the artist) never shows the previous
    // artist's count/subset.
    state.set_artist_tab("catalog".into());
    state.set_library_count(0);
    state.set_library_tracks(ModelRc::new(VecModel::from(Vec::<TrackItem>::new())));
    state.set_library_albums(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
    state.set_loading(true);
}

/// Re-sort one release bucket in place (operates on the LIVE model so loaded
/// artwork is preserved) and persist the choice. `sort` = default | newest |
/// oldest | title-asc | title-desc.
pub fn resort_section(window: &AppWindow, release_type: &str, sort: &str) {
    crate::artist_prefs::set_sort(release_type, sort);
    let model = window.global::<ArtistState>().get_release_sections();
    for i in 0..model.row_count() {
        let Some(row) = model.row_data(i) else { continue };
        if row.release_type.as_str() != release_type {
            continue;
        }
        let mut albums: Vec<AlbumCardItem> = row.albums.iter().collect();
        crate::album_map::sort_album_items(&mut albums, sort);
        let new_row = ArtistReleaseSection {
            albums: ModelRc::new(VecModel::from(albums)),
            sort_by: sort.into(),
            ..row
        };
        model.set_row_data(i, new_row);
        break;
    }
    // Keep the FULL cache (in-page search source) in the same order.
    FULL_RELEASE_SECTIONS.with(|cell| {
        for s in cell.borrow_mut().iter_mut() {
            if s.release_type.as_str() == release_type {
                let mut albums: Vec<AlbumCardItem> = s.albums.iter().collect();
                crate::album_map::sort_album_items(&mut albums, sort);
                s.albums = ModelRc::new(VecModel::from(albums));
                s.sort_by = sort.into();
                break;
            }
        }
    });
}

/// Current loaded item count for a bucket — the offset for the next page.
pub fn section_loaded_count(window: &AppWindow, release_type: &str) -> usize {
    let model = window.global::<ArtistState>().get_release_sections();
    for i in 0..model.row_count() {
        if let Some(row) = model.row_data(i) {
            if row.release_type.as_str() == release_type {
                return row.albums.row_count();
            }
        }
    }
    0
}

/// Whether a bucket may still load another page on the index (cap = 4).
pub fn section_can_load_more(release_type: &str) -> bool {
    LOADED_PAGES.with(|c| c.borrow().get(release_type).copied().unwrap_or(1)) < MAX_INDEX_PAGES
}

/// Append a freshly-fetched page to a bucket (dedupe by id, re-sort, update
/// has_more honoring the 4-page cap). Returns artwork jobs for the NEW cards
/// at their post-sort positions. Runs on the Slint event loop.
pub fn append_release_page(
    window: &AppWindow,
    release_type: &str,
    new_cards: Vec<CardData>,
    server_has_more: bool,
) -> Vec<ArtworkJob> {
    let pages = LOADED_PAGES.with(|cell| {
        let mut m = cell.borrow_mut();
        let e = m.entry(release_type.to_string()).or_insert(1);
        *e += 1;
        *e
    });
    let mut jobs = Vec::new();
    let model = window.global::<ArtistState>().get_release_sections();
    for i in 0..model.row_count() {
        let Some(row) = model.row_data(i) else { continue };
        if row.release_type.as_str() != release_type {
            continue;
        }
        let sort = row.sort_by.to_string();
        let mut items: Vec<AlbumCardItem> = row.albums.iter().collect();
        let mut seen: HashSet<String> = items.iter().map(|a| a.id.to_string()).collect();
        let mut appended_ids: Vec<String> = Vec::new();
        for card in new_cards {
            let item = card_to_item(card);
            let id = item.id.to_string();
            if seen.contains(&id) {
                continue;
            }
            seen.insert(id.clone());
            appended_ids.push(id);
            items.push(item);
        }
        crate::album_map::sort_album_items(&mut items, &sort);
        let has_more = server_has_more && pages < MAX_INDEX_PAGES && !appended_ids.is_empty();
        for (idx, item) in items.iter().enumerate() {
            if appended_ids.iter().any(|id| id == item.id.as_str())
                && !item.artwork_url.as_str().is_empty()
            {
                jobs.push(ArtworkJob {
                    target: ArtworkTarget::ArtistRelease {
                        section_idx: i,
                        album_idx: idx,
                    },
                    url: item.artwork_url.to_string(),
                });
            }
        }
        let new_row = ArtistReleaseSection {
            albums: ModelRc::new(VecModel::from(items.clone())),
            has_more,
            ..row
        };
        model.set_row_data(i, new_row);
        FULL_RELEASE_SECTIONS.with(|cell| {
            for s in cell.borrow_mut().iter_mut() {
                if s.release_type.as_str() == release_type {
                    s.albums = ModelRc::new(VecModel::from(items.clone()));
                    s.has_more = has_more;
                    break;
                }
            }
        });
        break;
    }
    jobs
}

// ============== Popular Tracks multi-select (mirrors AlbumState) ==========

/// Toggle Popular Tracks multi-select; leaving the mode clears the selection.
pub fn set_multi_select(window: &AppWindow, on: bool) {
    let state = window.global::<ArtistState>();
    state.set_top_tracks_multi_select(on);
    crate::selection::clear_anchor();
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
    if let Some(atmosphere) = crate::immersive::generate_atmosphere_image(pixels, width, height) {
        state.set_header_atmosphere(atmosphere);
    } else {
        state.set_header_atmosphere(slint::Image::default());
    }
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
    // MusicBrainz opt-out: when MB is off the Network tab (relationships /
    // discovery / origin) has nothing to show, so mark it unavailable and open
    // on the MB-independent Magazine/Stories tab instead of an empty Network
    // tab. The internal core guards (load_mb_metadata, musicbrainz_*) stay as
    // belt-and-suspenders.
    let mb_on = window.global::<SettingsState>().get_musicbrainz_enabled();
    let state = window.global::<NetworkSidebarState>();
    state.set_open(!constrained);
    // (Re)open a new artist on the Network tab when MB is on, else Magazine.
    let default_tab = if mb_on { "network" } else { "magazine" };
    state.set_active_tab(default_tab.into());
    state.set_mb_available(mb_on);
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
            &|| {
                // play_history supplies the (id, name) known set; reco augments
                // the id set with its richer signal (artists played >threshold
                // OR favorited). Names stay from play_history -- reco_events has
                // no artist names (schema frozen to match Tauri).
                let (mut ids, names) = crate::play_history::known_artists(known_threshold);
                if let Some(reco_ids) = crate::reco::known_artist_ids(known_threshold) {
                    ids.extend(reco_ids);
                }
                (ids, names)
            },
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
        // T8: smart-discovery rail (v2_get_discovery_artists equivalent) —
        // skip candidates whose resolved Qobuz id is blacklisted.
        // is_blacklisted_id_str auto-gates on the enabled flag and treats a
        // missing/non-numeric id as not-blacklisted (kept).
        .filter(|r| !crate::artist_blacklist::is_blacklisted_id_str(&r.qobuz_id))
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
    // Month names are `mark`ed so the extractor registers the English literals;
    // they are translated once with `t(name)` at the format arms below.
    let month = |m: &str| -> Option<&'static str> {
        Some(match m {
            "01" => qbz_i18n::mark("January"),
            "02" => qbz_i18n::mark("February"),
            "03" => qbz_i18n::mark("March"),
            "04" => qbz_i18n::mark("April"),
            "05" => qbz_i18n::mark("May"),
            "06" => qbz_i18n::mark("June"),
            "07" => qbz_i18n::mark("July"),
            "08" => qbz_i18n::mark("August"),
            "09" => qbz_i18n::mark("September"),
            "10" => qbz_i18n::mark("October"),
            "11" => qbz_i18n::mark("November"),
            "12" => qbz_i18n::mark("December"),
            _ => return None,
        })
    };
    match parts.as_slice() {
        [y] => (*y).to_string(),
        [y, m] => match month(m) {
            // "{month} {year}" — translate the month name and the layout.
            Some(name) => {
                let name_tr = qbz_i18n::t(name);
                qbz_i18n::t_args("{} {}", &[name_tr.as_str(), *y])
            }
            None => date.to_string(),
        },
        [y, m, d] => match month(m) {
            Some(name) => {
                let day = d.trim_start_matches('0');
                let name_tr = qbz_i18n::t(name);
                // "{month} {day}, {year}" — translate name + layout.
                qbz_i18n::t_args("{} {}, {}", &[name_tr.as_str(), day, *y])
            }
            None => date.to_string(),
        },
        _ => date.to_string(),
    }
}
