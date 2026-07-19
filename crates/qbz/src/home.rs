//! Discover / Home controller.
//!
//! Fetches the Qobuz discover index through `QbzCore`, maps it into plain
//! (Send) data on the worker thread, and — separately, on the Slint event
//! loop — converts that into Slint models pushed onto the `HomeState`
//! global. Domain types never reach the `.slint` files.

use std::cell::RefCell;
use std::sync::Arc;

use qbz_app::settings::discover_prefs::{DiscoverPrefs, DiscoverySectionId, DiscoveryTab};
use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{
    AlbumAward, DiscoverAlbum, DiscoverAudioInfo, DiscoverContainer, DiscoverPlaylist,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{
    AlbumCardItem, AppWindow, DiscoverSection, DiscoverState, HomeState, PlaylistTagItem,
    SearchPlaylistItem, SectionDescriptor, SlimItem,
};

/// Plain, `Send` home data produced on the worker thread.
pub struct HomeData {
    pub sections: Vec<SectionData>,
    /// Editorial-only section set for the Editor's Picks tab.
    pub editor_sections: Vec<SectionData>,
    pub popular: Vec<SlimData>,
    pub recent: Vec<SlimData>,
    pub recent_albums: Vec<CardData>,
    /// Qobuz playlists row — home tab.
    pub playlists: Vec<PlaylistCardData>,
    /// Qobuz playlists row — editorPicks tab (same data, separate cache slot).
    pub editor_playlists: Vec<PlaylistCardData>,
    /// Category tags for the Qobuz Playlists multi-select filter: (slug,
    /// localized name). Empty when the index carries no `playlists_tags`.
    pub playlist_tags: Vec<(String, String)>,
    /// "Library Albums" rail (#566) — the user's favorite albums from the
    /// SAME pipeline For You uses (`foryou::favorite_album_cards`), fetched
    /// concurrently with the discover index. Feeds
    /// `HomeState.favorite-albums`; the view arm self-hides while empty.
    pub favorite_albums: Vec<crate::foryou::AlbumCard>,
    /// "Release Watch" rail (#566) — same pipeline as For You
    /// (`foryou::fetch_release_watch`, blacklist-filtered), fetched
    /// concurrently. Feeds `HomeState.release-watch`; self-hides while empty.
    pub release_watch: Vec<crate::foryou::AlbumCard>,
    /// "Your Top Artists" rail (#566) — same pipeline as For You
    /// (`foryou::top_artist_cards`), fetched concurrently. Feeds
    /// `HomeState.top-artists`; self-hides while empty. (qobuzMixes, the
    /// fourth ported Tauri-Home section, is static navigation tiles — no
    /// data field needed.)
    pub top_artists: Vec<crate::foryou::ArtistSlim>,
}

thread_local! {
    /// The per-tab section sets, cached on the UI thread after a load
    /// so a tab switch can swap HomeState.sections without re-fetching.
    /// (home, editor, foryou)
    static TAB_SECTIONS: RefCell<TabSections> = RefCell::new(TabSections::default());
}

#[derive(Default)]
struct TabSections {
    home: Vec<SectionData>,
    editor: Vec<SectionData>,
    home_playlists: Vec<PlaylistCardData>,
    editor_playlists: Vec<PlaylistCardData>,
    /// Slugs of the currently-selected category tags (Qobuz Playlists filter).
    /// Empty = show all. Client-side; survives a tab switch.
    selected_tags: Vec<String>,
}

/// Keep only the playlists whose tag slugs intersect `selected` (union of the
/// selected tags). An empty selection passes everything through.
fn filter_playlists<'a>(
    playlists: &'a [PlaylistCardData],
    selected: &[String],
) -> Vec<&'a PlaylistCardData> {
    if selected.is_empty() {
        return playlists.iter().collect();
    }
    playlists
        .iter()
        .filter(|p| p.tags.iter().any(|slug| selected.iter().any(|s| s == slug)))
        .collect()
}

#[derive(Clone)]
pub struct SectionData {
    /// The configurator section id this album carousel maps to. Lets the
    /// prefs-driven render loop key a pref id to its cached section data
    /// (Slice 5). Album-carousel sections only.
    pub id: DiscoverySectionId,
    pub title: String,
    /// Discover endpoint path for the "View all" page ("" = no full-list page).
    pub endpoint: String,
    pub albums: Vec<CardData>,
}

#[derive(Clone, Default)]
pub struct CardData {
    pub id: String,
    pub title: String,
    pub artist: String,
    /// Artist id for the clickable artist name; empty = not clickable
    /// (e.g. artist-page release cards, whose subtitle slot is the year).
    pub artist_id: String,
    pub genre: String,
    pub year: String,
    /// "hires" | "cd" | "" — drives the icon-only quality badge.
    pub quality_tier: String,
    /// "Hi-Res: 24-bit / 96 kHz" — shown when hovering the quality badge.
    pub quality_label: String,
    pub ribbon: String,
    pub ribbon_kind: String,
    pub artwork_url: String,
    // --- List-row extras (AlbumListRow); empty/default for grid-only data.
    /// "Album" | "EP" | "Single" | "Live" | "Compilation".
    pub release_type: String,
    /// "qobuz" | "local" | "plex" | "" — the hideable SOURCE column.
    pub source: String,
    /// "24-bit / 96 kHz" — the bare detail line for QualityBadgeFull.
    pub quality_detail: String,
    /// Track count, as a display string ("" = unknown).
    pub track_count: String,
    /// Bare 4-digit year for the list-row YEAR column ("" = unknown).
    pub plain_year: String,
}

/// A single-cover playlist card for the Discover `qobuzPlaylists` row
/// (Home + Editor's Picks). Tauri's PlaylistCardLite renders name only — no
/// owner/subtitle/track-count — and a single cover, so we drop them too.
#[derive(Clone)]
pub struct PlaylistCardData {
    pub id: String,
    pub title: String,
    pub artwork_url: String, // rectangle || covers[0] || ""
    /// First tag's localized name — the UPPERCASE accent subtag on the card
    /// ("" = the playlist carries no tags).
    pub category: String,
    /// All tag slugs — the material for the client-side category filter (C).
    pub tags: Vec<String>,
}

/// A compact ranked item for the slim grid sections.
pub struct SlimData {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub rank: String,
    pub artwork_url: String,
}

/// Fetch the discover index (optionally genre-filtered) and map it
/// into the Home / Editor's Picks / For You section sets.
pub async fn load_home<A>(
    runtime: &Arc<AppRuntime<A>>,
    genre_ids: Option<Vec<u64>>,
) -> Result<HomeData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    // The personalized Home rails (#566: Library Albums / Release Watch /
    // Your Top Artists) resolve CONCURRENTLY with the index so they add no
    // latency to the home load. Fetched unconditionally (like the
    // Qobuzissimes cache-pool precedent below): the configurator re-render
    // is cache-only, so enabling a section must find its data populated.
    let (response, favorite_albums, release_watch, top_artists) = futures_util::join!(
        runtime.core().get_discover_index(genre_ids),
        crate::foryou::favorite_album_cards(runtime),
        crate::foryou::fetch_release_watch(runtime),
        crate::foryou::top_artist_cards(runtime),
    );
    let response = response.map_err(|e| e.to_string())?;
    let mut containers = response.containers;

    // T8: drop blacklisted DiscoverAlbums (ANY of artists[], featured-aware)
    // from the discover-index containers. Tauri filters exactly these six
    // containers (ideal_discography, new_releases, qobuzissims, most_streamed,
    // press_awards, album_of_the_week) and adjusts NO count — log-only parity
    // (the carousels are has_more/cache-driven, not total-driven).
    {
        let (bl, abl) = if crate::artist_blacklist::is_enabled() {
            (
                crate::artist_blacklist::ids_snapshot(),
                crate::artist_blacklist::album_ids_snapshot(),
            )
        } else {
            Default::default()
        };
        if !bl.is_empty() || !abl.is_empty() {
            let retain = |c: &mut Option<DiscoverContainer<DiscoverAlbum>>| {
                if let Some(container) = c.as_mut() {
                    container
                        .data
                        .items
                        .retain(|a| !qbz_core::core::discover_album_blacklisted(a, &bl, &abl));
                }
            };
            retain(&mut containers.new_releases);
            retain(&mut containers.qobuzissims);
            retain(&mut containers.press_awards);
            retain(&mut containers.most_streamed);
            retain(&mut containers.ideal_discography);
            retain(&mut containers.album_of_the_week);
        }
    }

    // Genre filtering is server-side: the selected genre ids (parent OR
    // sub-genre, raw) were passed to get_discover_index, which honors sub-genre
    // ids in `genre_ids`. No client-side narrowing — 1:1 with Tauri
    // discovery-v2, which rendered the faceted response as-is (narrowing here
    // wrongly dropped albums tagged only at top level).

    // Editorial-only set for the Editor's Picks tab — built first
    // (by cloning the containers) so the same data can also feed the
    // Home set and the most-streamed slim grid below. Order mirrors
    // Tauri's DEFAULT_PREFS.editorPicks.
    let mut editor_sections = Vec::new();
    push_section_ref(&mut editor_sections, DiscoverySectionId::NewReleases, &qbz_i18n::t("New Releases"), "/discover/newReleases", &containers.new_releases);
    push_section_ref(&mut editor_sections, DiscoverySectionId::Qobuzissimes, &qbz_i18n::t("Qobuzissimes"), "/discover/qobuzissims", &containers.qobuzissims);
    push_section_ref(&mut editor_sections, DiscoverySectionId::PressAwards, &qbz_i18n::t("Press Accolades"), "/discover/pressAward", &containers.press_awards);
    push_section_ref(&mut editor_sections, DiscoverySectionId::MostStreamed, &qbz_i18n::t("Most Streamed"), "/discover/mostStreamed", &containers.most_streamed);
    push_section_ref(
        &mut editor_sections,
        DiscoverySectionId::IdealDiscography,
        &qbz_i18n::t("Ideal Discography"),
        "/discover/idealDiscography",
        &containers.ideal_discography,
    );
    push_section_ref(
        &mut editor_sections,
        DiscoverySectionId::EditorPicks,
        &qbz_i18n::t("Albums of the Week"),
        "/discover/albumOfTheWeek",
        &containers.album_of_the_week,
    );

    let mut sections = Vec::new();
    push_section(&mut sections, DiscoverySectionId::NewReleases, &qbz_i18n::t("New Releases"), "/discover/newReleases", containers.new_releases);
    push_section(&mut sections, DiscoverySectionId::PressAwards, &qbz_i18n::t("Press Accolades"), "/discover/pressAward", containers.press_awards);
    push_section(
        &mut sections,
        DiscoverySectionId::IdealDiscography,
        &qbz_i18n::t("Ideal Discography"),
        "/discover/idealDiscography",
        containers.ideal_discography,
    );
    push_section(
        &mut sections,
        DiscoverySectionId::EditorPicks,
        &qbz_i18n::t("Albums of the Week"),
        "/discover/albumOfTheWeek",
        containers.album_of_the_week,
    );
    // Qobuzissimes is OFF on the Home tab by DEFAULT (the prefs control
    // visibility now), but the configurator OFFERS it on Home — so keep its data
    // in the Home cache pool so enabling it actually renders. Visibility is
    // governed by DEFAULT_PREFS (off), not by absence from the cache.
    push_section(
        &mut sections,
        DiscoverySectionId::Qobuzissimes,
        &qbz_i18n::t("Qobuzissimes"),
        "/discover/qobuzissims",
        containers.qobuzissims,
    );

    // Capped at 24 (two carousel pages of 12) — the slim carousel
    // does not show beyond that.
    let popular = containers
        .most_streamed
        .map(|container| container.data.items)
        .unwrap_or_default()
        .into_iter()
        .take(24)
        .enumerate()
        .map(|(index, album)| map_slim(index, album))
        .collect();

    // For You is loaded separately + lazily by crate::foryou into its
    // own dedicated view, so the home load no longer builds a For You
    // section set here.

    // Recently played comes from the local play-history store, not the
    // discover index. Empty until the playback session records plays.
    let recent = recent_track_slims();
    let recent_albums = recent_album_cards();

    // Qobuz Playlists row — both the Home and Editor's Picks tabs draw from
    // the SAME `containers.playlists` (one fetch). Capped at 40 (raised from
    // Tauri's 18) so the client-side category filter has material to work with
    // without holding 100 cards' covers in memory; the carousel still pages,
    // and the un-filtered view shows the same first cards. Each card carries
    // its tag slugs for the filter.
    let playlist_items: Vec<DiscoverPlaylist> =
        containers.playlists.map(|c| c.data.items).unwrap_or_default();
    let editor_playlists: Vec<PlaylistCardData> =
        playlist_items.iter().cloned().take(40).map(map_playlist).collect();
    let playlists: Vec<PlaylistCardData> =
        playlist_items.into_iter().take(40).map(map_playlist).collect();

    // Category tags for the multi-select filter (slug + localized name).
    let playlist_tags: Vec<(String, String)> = containers
        .playlists_tags
        .map(|c| {
            c.data
                .items
                .into_iter()
                .map(|tag| (tag.slug, tag.name))
                .collect()
        })
        .unwrap_or_default();

    Ok(HomeData {
        sections,
        editor_sections,
        popular,
        recent,
        recent_albums,
        playlists,
        editor_playlists,
        playlist_tags,
        favorite_albums,
        release_watch,
        top_artists,
    })
}

/// The recently-played TRACK window mapped to slim-card data, newest first,
/// capped at 24 (two carousel pages of 12 — the slim carousel does not show
/// beyond that). Local file read — cheap. Shared by [`load_home`] and the
/// targeted recent-rails refresh (`main::refresh_recent_rails`).
pub fn recent_track_slims() -> Vec<SlimData> {
    crate::recently::load()
        .into_iter()
        .take(24)
        .map(|track| SlimData {
            id: track.id,
            title: track.title,
            subtitle: track.subtitle,
            rank: String::new(),
            artwork_url: track.artwork_url,
        })
        .collect()
}

/// The recently-played album history mapped to card data, newest first.
/// Shared by the Home "Recently Played Albums" rail (via [`load_home`]) and
/// the full "View all" page (`main::navigate_recent_albums`), so both apply
/// the same blacklist filter and date localization. Local file read — cheap.
pub fn recent_album_cards() -> Vec<CardData> {
    crate::recently::load_albums()
        .into_iter()
        // Drop blocked albums (own id) at the SOURCE so the model + artwork jobs
        // stay index-aligned. Recently Played is Qobuz album ids.
        .filter(|album| !crate::artist_blacklist::is_album_blacklisted(&album.id))
        .map(|album| CardData {
            id: album.id,
            title: album.title,
            artist: album.artist,
            artist_id: String::new(),
            genre: album.genre,
            // Localize the stored ISO release date to "MMM D, YYYY" the
            // same way the discover cards do (empty stays empty).
            year: if album.release_date.is_empty() {
                String::new()
            } else {
                crate::dates::release_label(Some(&album.release_date))
            },
            quality_tier: album.quality_tier,
            quality_label: album.quality_label,
            ribbon: String::new(),
            ribbon_kind: String::new(),
            artwork_url: album.artwork_url,
            // Carry the origin so the card resolves source-aware artwork
            // (PlexThumb / local file) and the play/open route correctly.
            source: album.source,
            // Recently-played cards render in the grid only — list-row
            // extras stay default.
            ..CardData::default()
        })
        .collect()
}

fn push_section(
    out: &mut Vec<SectionData>,
    id: DiscoverySectionId,
    title: &str,
    endpoint: &str,
    container: Option<DiscoverContainer<DiscoverAlbum>>,
) {
    let Some(container) = container else {
        return;
    };
    if container.data.items.is_empty() {
        return;
    }
    out.push(SectionData {
        id,
        title: title.to_string(),
        endpoint: endpoint.to_string(),
        albums: container.data.items.into_iter().map(map_album).collect(),
    });
}

/// Like `push_section` but borrows the container (clones the items)
/// so the same data can feed more than one tab's section set.
fn push_section_ref(
    out: &mut Vec<SectionData>,
    id: DiscoverySectionId,
    title: &str,
    endpoint: &str,
    container: &Option<DiscoverContainer<DiscoverAlbum>>,
) {
    let Some(container) = container else {
        return;
    };
    if container.data.items.is_empty() {
        return;
    }
    out.push(SectionData {
        id,
        title: title.to_string(),
        endpoint: endpoint.to_string(),
        albums: container.data.items.iter().cloned().map(map_album).collect(),
    });
}

pub(crate) fn map_album(album: DiscoverAlbum) -> CardData {
    let artist = album
        .artists
        .first()
        .map(|a| a.name.clone())
        .unwrap_or_default();
    let artist_id = album
        .artists
        .first()
        .map(|a| a.id.to_string())
        .unwrap_or_default();
    let genre = album.genre.map(|g| g.name).unwrap_or_default();
    let year = crate::dates::release_label(
        album
            .dates
            .as_ref()
            .and_then(|d| d.original.as_ref().or(d.download.as_ref()).or(d.stream.as_ref()))
            .map(|s| s.as_str()),
    );
    let (ribbon, ribbon_kind) = pick_ribbon(album.awards.as_deref());
    let quality_tier = quality_tier(album.audio_info.as_ref()).to_string();
    let quality_label = quality_label(album.audio_info.as_ref());
    let quality_detail = quality_detail(album.audio_info.as_ref());
    let artwork_url = album
        .image
        .large
        .or(album.image.thumbnail)
        .or(album.image.small)
        .unwrap_or_default();
    // Bare 4-digit year for the list-row YEAR column (the grid uses the
    // localized `year`); plus a track-count display string and a release
    // type heuristic for the list-row TYPE column.
    let plain_year = album
        .dates
        .as_ref()
        .and_then(|d| d.original.as_ref().or(d.download.as_ref()).or(d.stream.as_ref()))
        .and_then(|s| s.get(0..4))
        .unwrap_or_default()
        .to_string();
    let track_count = album.track_count.map(|n| n.to_string()).unwrap_or_default();
    let release_type = qbz_i18n::t(classify_release_type(album.track_count));
    CardData {
        id: album.id,
        title: album.title,
        artist,
        artist_id,
        genre,
        year,
        quality_tier,
        quality_label,
        ribbon,
        ribbon_kind,
        artwork_url,
        release_type,
        // Discover is always the Qobuz catalog.
        source: "qobuz".to_string(),
        quality_detail,
        track_count,
        plain_year,
    }
}

/// Map a Discover playlist into a single-cover card. Preferred cover is the
/// landscape `rectangle`, falling back to the first square `cover`. Owner,
/// duration and tracks_count are intentionally dropped (1:1 with Tauri's
/// PlaylistCardLite, which shows the name only).
pub(crate) fn map_playlist(p: DiscoverPlaylist) -> PlaylistCardData {
    let artwork_url = p
        .image
        .rectangle
        .or_else(|| p.image.covers.and_then(|c| c.into_iter().next()))
        .unwrap_or_default();
    // First tag → the UPPERCASE accent subtag; all tag slugs → the filter
    // material. DiscoverPlaylist.tags is Option<Vec<PlaylistTag{id,slug,name}>>.
    // Uppercased here (Slint has no text-transform; same convention as the
    // MIXTAPE/COLLECTION eyebrow tags). The name is already localized by the
    // API response.
    let category = p
        .tags
        .as_ref()
        .and_then(|t| t.first())
        .map(|t| t.name.to_uppercase())
        .unwrap_or_default();
    let tags = p
        .tags
        .as_ref()
        .map(|t| t.iter().map(|tag| tag.slug.clone()).collect())
        .unwrap_or_default();
    PlaylistCardData {
        id: p.id.to_string(),
        title: p.name,
        artwork_url,
        category,
        tags,
    }
}

/// Classify a Discover album's release type for the list-row TYPE column.
/// The Discover index carries no explicit release_type, so this mirrors
/// DiscographyBuilderView's track-count fallback heuristic (<=3 = Single,
/// <=6 = EP, otherwise Album).
fn classify_release_type(track_count: Option<u32>) -> &'static str {
    match track_count {
        Some(n) if n <= 3 => "Single",
        Some(n) if n <= 6 => "EP",
        _ => "Album",
    }
}

/// Bare exact-quality detail for QualityBadgeFull's detail line, e.g.
/// "24-bit / 96 kHz" (no "Hi-Res:" prefix — the badge supplies the tier
/// label itself). Empty when the entry carries no audio info.
fn quality_detail(audio: Option<&DiscoverAudioInfo>) -> String {
    let Some(audio) = audio else {
        return String::new();
    };
    let hi_res = matches!(audio.maximum_bit_depth, Some(depth) if depth >= 24);
    let depth = audio
        .maximum_bit_depth
        .unwrap_or(if hi_res { 24 } else { 16 });
    let rate = audio
        .maximum_sampling_rate
        .unwrap_or(if hi_res { 96.0 } else { 44.1 });
    format!("{depth}-bit / {} kHz", format_rate(rate))
}

fn map_slim(index: usize, album: DiscoverAlbum) -> SlimData {
    let subtitle = album
        .artists
        .first()
        .map(|a| a.name.clone())
        .unwrap_or_default();
    let artwork_url = album
        .image
        .thumbnail
        .or(album.image.small)
        .or(album.image.large)
        .unwrap_or_default();
    SlimData {
        id: album.id,
        title: album.title,
        subtitle,
        rank: (index + 1).to_string(),
        artwork_url,
    }
}

/// Pick the single award ribbon, mirroring `pickAlbumRibbon` in data.ts:
/// award id 151 = Album of the Week, 88 = Qobuzissime, otherwise the last
/// award becomes a generic "press" ribbon.
fn pick_ribbon(awards: Option<&[AlbumAward]>) -> (String, String) {
    let Some(awards) = awards else {
        return (String::new(), String::new());
    };
    if awards.is_empty() {
        return (String::new(), String::new());
    }
    if let Some(a) = awards.iter().find(|a| a.id.as_deref() == Some("151")) {
        return (a.name.clone(), "albumOfTheWeek".to_string());
    }
    if let Some(a) = awards.iter().find(|a| a.id.as_deref() == Some("88")) {
        return (a.name.clone(), "qobuzissime".to_string());
    }
    let last = awards.last().expect("non-empty checked above");
    (last.name.clone(), "press".to_string())
}

/// Classify the quality tier for the icon-only badge: 24-bit and up is
/// Hi-Res, anything else with audio info is CD-quality.
fn quality_tier(audio: Option<&DiscoverAudioInfo>) -> &'static str {
    let Some(audio) = audio else {
        return "";
    };
    match audio.maximum_bit_depth {
        Some(depth) if depth >= 24 => "hires",
        _ => "cd",
    }
}

/// Exact-quality label for the badge hover tooltip, mirroring the Tauri
/// `QualityBadge` (`{tier}: {depth}-bit / {rate} kHz`). Empty when the
/// discover entry carries no audio info, matching `quality_tier`.
fn quality_label(audio: Option<&DiscoverAudioInfo>) -> String {
    let Some(audio) = audio else {
        return String::new();
    };
    let hi_res = matches!(audio.maximum_bit_depth, Some(depth) if depth >= 24);
    let tier = if hi_res { "Hi-Res" } else { "CD" };
    let depth = audio
        .maximum_bit_depth
        .unwrap_or(if hi_res { 24 } else { 16 });
    let rate = audio
        .maximum_sampling_rate
        .unwrap_or(if hi_res { 96.0 } else { 44.1 });
    format!("{tier}: {depth}-bit / {} kHz", format_rate(rate))
}

/// Format a kHz sample rate without a trailing `.0` (96.0 -> "96",
/// 44.1 -> "44.1").
fn format_rate(rate: f64) -> String {
    if (rate.fract()).abs() < f64::EPSILON {
        format!("{}", rate as i64)
    } else {
        format!("{rate}")
    }
}

/// Convert one `SlimData` into the Slint `SlimItem` (shared by `apply_home`
/// and `apply_recent_rails`).
pub(crate) fn slim_to_item(slim: SlimData) -> SlimItem {
    SlimItem {
        id: slim.id.into(),
        title: slim.title.into(),
        subtitle: slim.subtitle.into(),
        rank: slim.rank.into(),
        artwork_url: slim.artwork_url.into(),
        artwork: slint::Image::default(),
        following: false,
        // Slim rails (popular / recently played) render pin-less slim rows,
        // not the grid card — nothing here is pinnable.
        is_pinned: false,
    }
}

/// Push ONLY the two recently-played rails onto `HomeState` — the targeted
/// auto/manual refresh path. Everything else on Home is left untouched: no
/// discover-index fetch, no descriptor rebuild, no section-cache write. Must
/// run on the Slint event loop (`card_to_item` seeds is-favorite from the
/// login cache, same as `apply_home`).
pub fn apply_recent_rails(window: &AppWindow, recent: Vec<SlimData>, albums: Vec<CardData>) {
    let recent: Vec<SlimItem> = recent.into_iter().map(slim_to_item).collect();
    let albums: Vec<AlbumCardItem> = albums.into_iter().map(card_to_item).collect();
    let state = window.global::<HomeState>();
    state.set_recent(ModelRc::new(VecModel::from(recent)));
    state.set_recent_albums(ModelRc::new(VecModel::from(albums)));
}

/// Convert one `CardData` into the Slint `AlbumCardItem`.
pub(crate) fn card_to_item(card: CardData) -> AlbumCardItem {
    AlbumCardItem {
        // Favorite heart state from the login-seeded cache (kept live by
        // main::set_album_row_favorite when a favorite toggles anywhere).
        is_favorite: crate::fav_cache::is_album_favorite(&card.id),
        // Pin badge state from the per-user pinned store (kept live by
        // main::set_album_row_pinned when a pin toggles anywhere).
        is_pinned: crate::pinned::is_pinned("album", &card.id),
        id: card.id.into(),
        title: card.title.into(),
        artist: card.artist.into(),
        artist_id: card.artist_id.into(),
        genre: card.genre.into(),
        year: card.year.into(),
        quality_tier: card.quality_tier.into(),
        quality_label: card.quality_label.into(),
        ribbon: card.ribbon.into(),
        ribbon_kind: card.ribbon_kind.into(),
        artwork_url: card.artwork_url.into(),
        artwork: slint::Image::default(),
        release_type: card.release_type.into(),
        source: card.source.into(),
        quality_detail: card.quality_detail.into(),
        track_count: card.track_count.into(),
        plain_year: card.plain_year.into(),
        removing: false,
        selected: false,
    }
}

/// Convert one `PlaylistCardData` into the Slint `SearchPlaylistItem`,
/// single-cover shape (slot 0 only). Mirrors label.rs's playlist converter:
/// no subtitle (1:1 with Tauri's PlaylistCardLite), cover-count 0 when there
/// is no artwork so the card draws its placeholder.
pub(crate) fn playlist_to_item(p: &PlaylistCardData) -> SearchPlaylistItem {
    SearchPlaylistItem {
        id: p.id.clone().into(),
        title: p.title.clone().into(),
        subtitle: "".into(),
        // Pin badge state from the per-user pinned store (kept live by
        // main::set_playlist_row_pinned when a pin toggles anywhere).
        is_pinned: crate::pinned::is_pinned("playlist", &p.id),
        cover_count: if p.artwork_url.is_empty() { 0 } else { 1 },
        url1: p.artwork_url.clone().into(),
        url2: "".into(),
        url3: "".into(),
        url4: "".into(),
        cover1: slint::Image::default(),
        cover2: slint::Image::default(),
        cover3: slint::Image::default(),
        cover4: slint::Image::default(),
        category: p.category.clone().into(),
        // Neutral dark letterbox until the cover decodes and the artwork
        // pipeline writes the real dominant colour (mirrors immersive::
        // dominant_cover_color's own fallback).
        dominant_color: slint::Color::from_rgb_u8(30, 30, 34),
        // Discover playlists are editorial (foreign Qobuz) → follow + copy.
        is_owned: false,
        is_following: false,
        is_copied: false,
    }
}

/// Build the Slint section model for one tab's section set.
fn build_sections(sections: &[SectionData]) -> Vec<DiscoverSection> {
    sections
        .iter()
        .map(|section| DiscoverSection {
            title: section.title.clone().into(),
            endpoint: section.endpoint.clone().into(),
            albums: ModelRc::new(VecModel::from(
                section.albums.iter().cloned().map(card_to_item).collect::<Vec<_>>(),
            )),
        })
        .collect()
}

/// Artwork jobs for the Qobuz Playlists row (single cover per card, so they
/// target `HomeState.playlists[idx]` directly). Skips cards with no artwork.
pub fn playlist_artwork_jobs(playlists: &[PlaylistCardData]) -> Vec<ArtworkJob> {
    playlists
        .iter()
        .enumerate()
        .filter_map(|(idx, p)| {
            (!p.artwork_url.is_empty()).then(|| ArtworkJob {
                target: ArtworkTarget::HomePlaylistCover { idx },
                url: p.artwork_url.clone(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Slice 5 — prefs-driven descriptor lists for Home / Editor's Picks.
// ---------------------------------------------------------------------------

/// Build one Slint `DiscoverSection` from cached album data (mirrors
/// `build_sections` for a single entry).
fn descriptor_section(data: &SectionData) -> DiscoverSection {
    DiscoverSection {
        title: data.title.clone().into(),
        endpoint: data.endpoint.clone().into(),
        albums: ModelRc::new(VecModel::from(
            data.albums.iter().cloned().map(card_to_item).collect::<Vec<_>>(),
        )),
    }
}

/// SINGLE SOURCE OF TRUTH for what the Home / Editor's Picks repeater can
/// actually render (#566): the section ids `HomeView.slint`'s delegate
/// if-chain has arms for. `descriptors_for` drops any enabled id NOT in this
/// set, so a stale persisted pref (e.g. qobuzMixes / releaseWatch / topArtists,
/// removed from the Home defaults 2026-07) can never emit an armless
/// descriptor again — an enabled section that renders nothing. Belt and
/// suspenders with `reconcile_list` (qbz-app), which already scrubs ids
/// absent from the tab defaults at load time. Extend this list IN THE SAME
/// CHANGE that adds a new arm to HomeView.slint.
const HOME_RENDERABLE: &[DiscoverySectionId] = &[
    DiscoverySectionId::NewReleases,
    DiscoverySectionId::PressAwards,
    DiscoverySectionId::IdealDiscography,
    DiscoverySectionId::EditorPicks,
    DiscoverySectionId::Qobuzissimes,
    DiscoverySectionId::MostStreamed,
    DiscoverySectionId::QobuzPlaylists,
    DiscoverySectionId::RecentlyPlayedAlbums,
    DiscoverySectionId::ContinueListening,
    DiscoverySectionId::FavoriteAlbums,
    DiscoverySectionId::QobuzMixes,
    DiscoverySectionId::ReleaseWatch,
    DiscoverySectionId::TopArtists,
    DiscoverySectionId::Pinned,
];

/// Build one tab's ordered ENABLED descriptor list from `prefs` + the cached
/// section data. Album-carousel ids embed their `DiscoverSection` (Home/Editor
/// share the Carousel component but have no per-id HomeState field). The
/// fixed-data ids (qobuzPlaylists / continueListening / mostStreamed-slim) and
/// the always-present-with-placeholder ids (recentlyPlayedAlbums on Home) bind
/// HomeState fields in the view; they carry an empty `section` — as do the
/// #566 ported rails: favoriteAlbums / releaseWatch / topArtists bind their
/// HomeState fields and self-hide while empty; qobuzMixes is static
/// navigation tiles, always rendered when enabled (Tauri parity).
///
/// **Empty-section policy (b):** an album-carousel id with no cached data is
/// DROPPED (no backing `SectionData` → nothing to render, and these have no
/// placeholder). recentlyPlayedAlbums / continueListening / qobuzPlaylists /
/// mostStreamed are KEPT (the view arm self-hides or shows a placeholder on
/// empty data), preserving the 1:1 Home placeholders. This keeps every mounted
/// album-carousel delegate non-empty (the documented anti-spacing-doubling form).
fn descriptors_for(prefs: &DiscoverPrefs, tab: DiscoveryTab, cached: &[SectionData]) -> Vec<SectionDescriptor> {
    use DiscoverySectionId::*;
    let editor = tab == DiscoveryTab::EditorPicks;
    let mut out = Vec::new();
    for id in prefs.enabled_ordered(tab) {
        // #566 structural guard: skip ids the HomeView repeater has no arm
        // for (stale persisted prefs) instead of emitting an invisible row.
        if !HOME_RENDERABLE.contains(&id) {
            continue;
        }
        // mostStreamed renders as an album carousel on Editor's Picks, a slim
        // grid on Home — encode that in `kind` so the delegate dispatches it
        // without reading active-tab.
        let kind = if id == MostStreamed {
            if editor { "albumCarousel" } else { "slimGrid" }
        } else {
            crate::discover_prefs::render_kind(id)
        };
        // Album-carousel ids that pull from the cached SectionData.
        let is_album_cache = matches!(
            id,
            NewReleases | PressAwards | IdealDiscography | EditorPicks | Qobuzissimes
        ) || (id == MostStreamed && editor);
        let section = if is_album_cache {
            match cached.iter().find(|s| s.id == id) {
                Some(data) => descriptor_section(data),
                // Empty-section policy (b): no data for this album id → drop it.
                None => continue,
            }
        } else {
            DiscoverSection::default()
        };
        out.push(SectionDescriptor {
            id: SharedString::from(id.as_str()),
            kind: SharedString::from(kind),
            section,
        });
    }
    out
}

/// Build the Home + Editor's Picks descriptor lists from the cached section
/// data (called by the configurator controller after a mutation and at seed).
pub fn tab_descriptors(prefs: &DiscoverPrefs) -> (Vec<SectionDescriptor>, Vec<SectionDescriptor>) {
    TAB_SECTIONS.with(|cell| {
        let cache = cell.borrow();
        let home = descriptors_for(prefs, DiscoveryTab::Home, &cache.home);
        let editor = descriptors_for(prefs, DiscoveryTab::EditorPicks, &cache.editor);
        (home, editor)
    })
}

/// Artwork jobs for a tab's descriptor list — they target the embedded album
/// sections in `DiscoverState.home-sections` / `editor-sections` (NOT
/// HomeState.sections), so covers paint on the prefs-driven Home/Editor loop.
/// Built from the cached `SectionData` (CardData urls) keyed by the descriptor
/// id, so `section_idx` aligns with the descriptor's position in the pushed
/// list — no need to read back the Slint model.
fn discover_section_artwork_jobs(
    descriptors: &[SectionDescriptor],
    cached: &[SectionData],
    editor: bool,
) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    for (section_idx, desc) in descriptors.iter().enumerate() {
        // Only album-carousel descriptors map to a cached SectionData; the
        // fixed-data ids have no entry and contribute no jobs here.
        let Some(data) = cached.iter().find(|s| s.id.as_str() == desc.id.as_str()) else {
            continue;
        };
        for (album_idx, card) in data.albums.iter().enumerate() {
            if card.artwork_url.is_empty() {
                continue;
            }
            jobs.push(ArtworkJob {
                target: ArtworkTarget::DiscoverSectionAlbum {
                    editor,
                    section_idx,
                    album_idx,
                },
                url: card.artwork_url.clone(),
            });
        }
    }
    jobs
}

/// Re-render the active Home / Editor's Picks tab from the cached section data
/// (no network) after a configurator mutation: push the recomputed descriptor
/// lists + the active tab's Qobuz Playlists row, and return the descriptor
/// artwork jobs to re-fire for the active tab. For You is not handled here (its
/// data lives in ForYouState; the descriptor list alone drives it).
pub fn rerender_active_tab(window: &AppWindow, prefs: &DiscoverPrefs) -> Vec<ArtworkJob> {
    let active = window.global::<DiscoverState>().get_active_tab().to_string();
    if active == "forYou" {
        return Vec::new();
    }
    let editor = active == "editorPicks";
    let (home, editor_list) = tab_descriptors(prefs);
    let active_list = if editor { editor_list.clone() } else { home.clone() };

    let dstate = window.global::<DiscoverState>();
    dstate.set_home_sections(ModelRc::new(VecModel::from(home)));
    dstate.set_editor_sections(ModelRc::new(VecModel::from(editor_list)));

    // Re-push the active tab's Qobuz Playlists row (category-filtered) + build
    // the album-section artwork jobs from the same cached data (one borrow of
    // the cache). The playlist artwork jobs are built from the SAME filtered
    // slice, so their `idx` aligns with the pushed (filtered) row.
    let hstate = window.global::<HomeState>();
    let (pls, jobs) = TAB_SECTIONS.with(|cell| {
        let cache = cell.borrow();
        let (album_cache, pls) = if editor {
            (&cache.editor, &cache.editor_playlists)
        } else {
            (&cache.home, &cache.home_playlists)
        };
        let filtered: Vec<PlaylistCardData> = filter_playlists(pls, &cache.selected_tags)
            .into_iter()
            .cloned()
            .collect();
        let mut jobs = discover_section_artwork_jobs(&active_list, album_cache, editor);
        jobs.extend(playlist_artwork_jobs(&filtered));
        (
            filtered.iter().map(playlist_to_item).collect::<Vec<_>>(),
            jobs,
        )
    });
    hstate.set_playlists(ModelRc::new(VecModel::from(pls)));

    jobs
}

/// Re-push the active tab's Qobuz Playlists row filtered by `selected_tags`,
/// and return the artwork jobs for the (filtered) row. Shared by the toggle /
/// clear callbacks: the selection is already updated in the cache. For You has
/// no playlists row, so it returns no jobs.
fn rerender_playlists_filtered(window: &AppWindow) -> Vec<ArtworkJob> {
    let active = window.global::<DiscoverState>().get_active_tab().to_string();
    if active == "forYou" {
        return Vec::new();
    }
    let editor = active == "editorPicks";
    let hstate = window.global::<HomeState>();
    let (pls, jobs) = TAB_SECTIONS.with(|cell| {
        let cache = cell.borrow();
        let source = if editor {
            &cache.editor_playlists
        } else {
            &cache.home_playlists
        };
        let filtered: Vec<PlaylistCardData> = filter_playlists(source, &cache.selected_tags)
            .into_iter()
            .cloned()
            .collect();
        let jobs = playlist_artwork_jobs(&filtered);
        (
            filtered.iter().map(playlist_to_item).collect::<Vec<_>>(),
            jobs,
        )
    });
    hstate.set_playlists(ModelRc::new(VecModel::from(pls)));
    jobs
}

/// Toggle one category tag (by slug) in the Qobuz Playlists filter, re-filter
/// the cached row, and return the artwork jobs for the new (filtered) row. Also
/// updates the `playlist-tags[i].selected` flags + `playlist-tag-count` so the
/// dropdown reflects the selection.
pub fn toggle_playlist_tag(window: &AppWindow, slug: &str) -> Vec<ArtworkJob> {
    let count = TAB_SECTIONS.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.selected_tags.iter().position(|s| s == slug) {
            cache.selected_tags.remove(pos);
        } else {
            cache.selected_tags.push(slug.to_string());
        }
        cache.selected_tags.len() as i32
    });
    sync_tag_selection(window, count);
    rerender_playlists_filtered(window)
}

/// Clear every selected category tag (show all playlists). Returns the artwork
/// jobs for the now-unfiltered row.
pub fn clear_playlist_tags(window: &AppWindow) -> Vec<ArtworkJob> {
    TAB_SECTIONS.with(|cell| cell.borrow_mut().selected_tags.clear());
    sync_tag_selection(window, 0);
    rerender_playlists_filtered(window)
}

/// Mirror the cached selection onto `HomeState.playlist-tags[i].selected` and
/// publish the selected count. Reads the selection from the cache so the two
/// never drift.
fn sync_tag_selection(window: &AppWindow, count: i32) {
    use slint::Model;
    // Snapshot the selection so the cache borrow is released before any Slint
    // model mutation (which can synchronously re-enter Rust closures).
    let selected: Vec<String> =
        TAB_SECTIONS.with(|cell| cell.borrow().selected_tags.clone());
    let model = window.global::<HomeState>().get_playlist_tags();
    for i in 0..model.row_count() {
        if let Some(mut item) = model.row_data(i) {
            let is_sel = selected.iter().any(|s| s.as_str() == item.slug.as_str());
            if item.selected != is_sel {
                item.selected = is_sel;
                model.set_row_data(i, item);
            }
        }
    }
    window.global::<HomeState>().set_playlist_tag_count(count);
}

/// Switch the visible Discover tab ("home" | "editorPicks" | "forYou"). Writes
/// the active tab into BOTH HomeState (Slice-3 pill bindings) and DiscoverState
/// (the prefs-driven render loop + the configurator target — single source of
/// truth), then re-renders the active tab from the cached section data via the
/// descriptor lists. No re-fetch. For You renders from its own ForYouView /
/// ForYouState; the Home/Editor descriptor lists are pushed empty for it.
pub fn select_tab(window: &AppWindow, tab: &str) -> Vec<ArtworkJob> {
    window.global::<HomeState>().set_active_tab(tab.into());
    window.global::<DiscoverState>().set_active_tab(tab.into());

    if tab == "forYou" || tab == "recommendations" {
        // For You + Recommendations both render from their own dedicated state /
        // view; push the For You descriptor list + drive Home/Editor empty, and
        // clear the legacy HomeState models so nothing lingers underneath.
        let prefs = crate::discover_prefs::prefs_snapshot();
        crate::discover_prefs::push_descriptors(window, &prefs);
        let hstate = window.global::<HomeState>();
        hstate.set_playlists(ModelRc::new(VecModel::from(Vec::<SearchPlaylistItem>::new())));
        return Vec::new();
    }

    let prefs = crate::discover_prefs::prefs_snapshot();
    rerender_active_tab(window, &prefs)
}

/// Convert worker-thread home data into Slint models and push them onto
/// the `HomeState` global. Must run on the Slint event loop.
pub fn apply_home(window: &AppWindow, data: HomeData) {
    let sections: Vec<DiscoverSection> = build_sections(&data.sections);

    // Cache the Home + Editor's Picks section sets for instant tab
    // switching (For You has its own dedicated state/view). A fresh index
    // load resets the category-tag selection (the tag set may have changed).
    TAB_SECTIONS.with(|cell| {
        *cell.borrow_mut() = TabSections {
            home: data.sections.clone(),
            editor: data.editor_sections.clone(),
            home_playlists: data.playlists.clone(),
            editor_playlists: data.editor_playlists.clone(),
            selected_tags: Vec::new(),
        };
    });

    let to_slim_items =
        |items: Vec<SlimData>| -> Vec<SlimItem> { items.into_iter().map(slim_to_item).collect() };
    let popular = to_slim_items(data.popular);
    let recent = to_slim_items(data.recent);
    let recent_albums: Vec<AlbumCardItem> =
        data.recent_albums.into_iter().map(card_to_item).collect();

    // Push the HOME tab's Qobuz Playlists row (apply_home runs for the
    // default Home tab; a tab switch swaps it via select_tab). The selection
    // was just reset, so the unfiltered full set is shown.
    let home_playlists: Vec<SearchPlaylistItem> =
        data.playlists.iter().map(playlist_to_item).collect();
    // Category tags for the multi-select filter — all start unselected.
    let tag_items: Vec<PlaylistTagItem> = data
        .playlist_tags
        .iter()
        .map(|(slug, name)| PlaylistTagItem {
            slug: slug.clone().into(),
            name: name.clone().into(),
            selected: false,
        })
        .collect();

    let state = window.global::<HomeState>();
    state.set_sections(ModelRc::new(VecModel::from(sections)));
    state.set_popular(ModelRc::new(VecModel::from(popular)));
    state.set_recent(ModelRc::new(VecModel::from(recent)));
    state.set_recent_albums(ModelRc::new(VecModel::from(recent_albums)));
    // The #566 ported rails — same section builders + title msgids as their
    // For You twins (foryou::apply_favorite_albums / apply_release_watch /
    // apply_top_artists), separate lifecycles. Top Artists' title lives in
    // the HomeView arm (@tr, like ForYouView's) — its model is a bare list.
    // Cache the base (Recently-added) order so the header sort dropdown can
    // reorder without a re-fetch, and reset the selection to the default on
    // every fresh load (the load-time artwork dispatch is in base order).
    *crate::LIB_ALBUMS_BASE.lock().unwrap() = data.favorite_albums.clone();
    state.set_library_albums_sort(0);
    state.set_favorite_albums(crate::foryou::section(
        &qbz_i18n::t("Library Albums"),
        &data.favorite_albums,
    ));
    state.set_release_watch(crate::foryou::section(
        &qbz_i18n::t("Release Watch"),
        &data.release_watch,
    ));
    state.set_top_artists(ModelRc::new(VecModel::from(crate::foryou::artist_items(
        &data.top_artists,
    ))));
    state.set_playlists(ModelRc::new(VecModel::from(home_playlists)));
    state.set_playlist_tags(ModelRc::new(VecModel::from(tag_items)));
    state.set_playlist_tag_count(0);

    // Push the prefs-driven descriptor lists now that the section cache is
    // populated, so the Home/Editor render loop reflects the fresh data (the
    // following select_tab re-pushes for the active tab; this keeps the lists
    // correct even if select_tab is not called).
    let prefs = crate::discover_prefs::prefs_snapshot();
    crate::discover_prefs::push_descriptors(window, &prefs);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(bit_depth: Option<u32>) -> DiscoverAudioInfo {
        DiscoverAudioInfo {
            maximum_bit_depth: bit_depth,
            maximum_sampling_rate: Some(96.0),
            maximum_channel_count: Some(2),
        }
    }

    #[test]
    fn quality_tier_hires_for_24_bit() {
        assert_eq!(quality_tier(Some(&audio(Some(24)))), "hires");
    }

    #[test]
    fn classify_release_type_track_count_heuristic() {
        assert_eq!(classify_release_type(Some(1)), "Single");
        assert_eq!(classify_release_type(Some(3)), "Single");
        assert_eq!(classify_release_type(Some(4)), "EP");
        assert_eq!(classify_release_type(Some(6)), "EP");
        assert_eq!(classify_release_type(Some(12)), "Album");
        assert_eq!(classify_release_type(None), "Album");
    }

    #[test]
    fn quality_detail_bare_without_tier_prefix() {
        // The list-row QualityBadgeFull supplies the tier label itself,
        // so the detail line is just "<depth>-bit / <rate> kHz".
        assert_eq!(quality_detail(Some(&audio(Some(24)))), "24-bit / 96 kHz");
        assert_eq!(quality_detail(None), "");
    }

    #[test]
    fn quality_tier_cd_for_16_bit() {
        assert_eq!(quality_tier(Some(&audio(Some(16)))), "cd");
    }

    #[test]
    fn quality_tier_empty_without_audio_info() {
        assert_eq!(quality_tier(None), "");
    }

    #[test]
    fn ribbon_prioritizes_album_of_the_week() {
        let awards = vec![
            AlbumAward {
                id: Some("88".into()),
                name: "Qobuzissime".into(),
                awarded_at: None,
            },
            AlbumAward {
                id: Some("151".into()),
                name: "Album of the Week".into(),
                awarded_at: None,
            },
        ];
        let (label, kind) = pick_ribbon(Some(&awards));
        assert_eq!(kind, "albumOfTheWeek");
        assert_eq!(label, "Album of the Week");
    }

    #[test]
    fn ribbon_falls_back_to_press() {
        let awards = vec![AlbumAward {
            id: Some("7".into()),
            name: "Gramophone Editor's Choice".into(),
            awarded_at: None,
        }];
        let (label, kind) = pick_ribbon(Some(&awards));
        assert_eq!(kind, "press");
        assert_eq!(label, "Gramophone Editor's Choice");
    }

    #[test]
    fn ribbon_empty_when_no_awards() {
        assert_eq!(pick_ribbon(None), (String::new(), String::new()));
    }
}
