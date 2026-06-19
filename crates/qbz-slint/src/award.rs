//! Award pages controller — the AwardView landing (hero + award-winning
//! releases preview + "Other awards" carousel + follow heart) and the
//! AwardAlbumsView listing (paginated album grid + client-side search).
//!
//! Mirrors Tauri's AwardView.svelte / AwardAlbumsView.svelte. Two hard
//! rules carried from the Tauri review:
//!   * the album grid is ALWAYS sourced from `/award/getAlbums`, never
//!     `/award/page.releases` (that endpoint is user-scoped and is used
//!     only for the hero name/image/magazine);
//!   * the favorite split is plural-read / singular-write — reads use
//!     `get_favorites("awards", ..)`, writes use `add/remove_favorite("award", ..)`.
//!
//! The name->id resolver (`resolve_award_id_by_name`) replaces Tauri's
//! sessionStorage `awardCatalogStore`: a process-local normalized cache,
//! harvested from album awards as they load (`remember_awards`), with an
//! `/award/explore` crawl fallback.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::album_map::{map_album, to_item, AlbumCard};
use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{AlbumCardItem, AppWindow, AwardState, SlimItem};

/// Landing preview grid page size (Tauri AwardView PAGE_SIZE).
pub const PREVIEW_PAGE_SIZE: u32 = 20;
/// Full-listing page size (Tauri AwardAlbumsView PAGE_SIZE).
pub const LIST_PAGE_SIZE: u32 = 50;
/// "Other awards" carousel size (Tauri AwardView loadOtherAwards limit).
const OTHER_AWARDS_LIMIT: u32 = 30;
/// /award/explore crawl page size + cap (Tauri awardCatalogStore).
const EXPLORE_PAGE_SIZE: u32 = 100;
const EXPLORE_MAX_PAGES: u32 = 40;

// ======================================================================
//  Name -> id resolver (process-local, harvested + explore-crawl)
// ======================================================================

/// Normalized award-name -> stringified-id map. Seeded by `remember_awards`
/// (from album awards + explore crawl); read by `resolve_award_id_by_name`.
fn catalog() -> &'static Mutex<HashMap<String, String>> {
    static CATALOG: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CATALOG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lowercase + trim + collapse-whitespace. Both the harvested names and the
/// to-resolve names come from the same Qobuz field, so this consistent
/// normalization is sufficient for self-matching (Tauri additionally strips
/// diacritics, which is moot here since both sides share the source string).
fn normalize(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Remember `(id, name)` award pairs (the harvesting Tauri's
/// `rememberAwardsFromAlbums` does). No-op for empty id/name. Called on the
/// UI thread from album/award applies — cheap, lock-guarded.
pub fn remember_awards(pairs: &[(String, String)]) {
    if pairs.is_empty() {
        return;
    }
    if let Ok(mut map) = catalog().lock() {
        for (id, name) in pairs {
            if id.is_empty() || name.is_empty() {
                continue;
            }
            map.entry(normalize(name)).or_insert_with(|| id.clone());
        }
    }
}

fn lookup_cached(name: &str) -> Option<String> {
    catalog()
        .lock()
        .ok()
        .and_then(|m| m.get(&normalize(name)).cloned())
}

/// Resolve an award id from its name. Cache-first; on a miss, crawl
/// `/award/explore` (remembering every pair) until found or exhausted.
/// Returns `None` when unresolvable (the caller toasts awardUnavailable).
pub async fn resolve_award_id_by_name<A>(
    runtime: &Arc<AppRuntime<A>>,
    name: &str,
) -> Option<String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    if name.trim().is_empty() {
        return None;
    }
    if let Some(id) = lookup_cached(name) {
        return Some(id);
    }
    let target = normalize(name);
    let mut offset = 0u32;
    let mut seen: HashSet<String> = HashSet::new();
    for _ in 0..EXPLORE_MAX_PAGES {
        let resp = match runtime
            .core()
            .get_award_explore(EXPLORE_PAGE_SIZE, offset)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[qbz-slint] award explore crawl failed: {e}");
                break;
            }
        };
        let items = resp
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if items.is_empty() {
            break;
        }
        let mut new_ids = 0usize;
        let mut found: Option<String> = None;
        let pairs: Vec<(String, String)> = items
            .iter()
            .filter_map(|it| {
                let id = it.get("id").map(value_to_string)?;
                let nm = it.get("name").and_then(|v| v.as_str())?.to_string();
                if id.is_empty() || nm.is_empty() {
                    return None;
                }
                if seen.insert(id.clone()) {
                    new_ids += 1;
                }
                if normalize(&nm) == target {
                    found = Some(id.clone());
                }
                Some((id, nm))
            })
            .collect();
        remember_awards(&pairs);
        if let Some(id) = found {
            return Some(id);
        }
        let has_more = resp
            .get("has_more")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        offset += items.len() as u32;
        if !has_more || new_ids == 0 {
            break;
        }
    }
    log::warn!("[qbz-slint] award id unresolved for name: {name}");
    None
}

// ======================================================================
//  Landing page (AwardView)
// ======================================================================

#[derive(Clone)]
pub struct OtherAward {
    pub id: String,
    pub name: String,
    pub magazine: String,
    pub image_url: String,
}

pub struct AwardPagePayload {
    pub id: String,
    pub name: String,
    pub image_url: String,
    pub magazine_name: String,
    pub is_following: bool,
    pub albums: Vec<AlbumCard>,
    pub total: usize,
    pub has_more: bool,
    /// True when the /award/getAlbums grid load failed (drives the
    /// error+retry branch, distinct from a genuinely empty award).
    pub albums_failed: bool,
    pub other_awards: Vec<OtherAward>,
}

/// Fetch the award landing: hero (best-effort `/award/page`), the first
/// `/award/getAlbums` preview page, and the `/award/explore` other-awards
/// rail. The hero never blocks the grid (mirrors Tauri's independent loads).
pub async fn load_award_page<A>(
    runtime: &Arc<AppRuntime<A>>,
    award_id: &str,
    fallback_name: &str,
) -> AwardPagePayload
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    // Hero — best-effort; on failure fall back to the passed name + no image.
    let (mut name, mut image_url, mut magazine_name) =
        (fallback_name.to_string(), String::new(), String::new());
    match runtime.core().get_award_page(award_id).await {
        Ok(page) => {
            if let Some(n) = page.name.filter(|s| !s.is_empty()) {
                name = n;
            }
            image_url = page.image.unwrap_or_default();
            if let Some(mag) = page.magazine {
                if image_url.is_empty() {
                    image_url = mag.image.clone().unwrap_or_default();
                }
                magazine_name = mag.name.unwrap_or_default();
            }
        }
        Err(e) => log::warn!("[qbz-slint] award page hero failed: {e}"),
    }

    // Album grid — ALWAYS from /award/getAlbums (never page.releases).
    let (albums, total, has_more, albums_failed) =
        match runtime.core().get_award_albums(award_id, PREVIEW_PAGE_SIZE, 0).await {
            Ok(page) => {
                let loaded = page.items.len();
                let total = page.total as usize;
                let albums: Vec<AlbumCard> = page.items.into_iter().map(map_album).collect();
                // D2: trust the real has_more (total is the client's hint =
                // loaded + has_more), so an exactly-full page still offers
                // "See all" only when more genuinely remain.
                (albums, total.max(loaded), total > loaded, false)
            }
            Err(e) => {
                log::error!("[qbz-slint] award albums (preview) failed: {e}");
                (Vec::new(), 0, false, true)
            }
        };

    let other_awards = load_other_awards(runtime, award_id).await;
    let is_following = crate::fav_cache::is_award_favorite(award_id);

    AwardPagePayload {
        id: award_id.to_string(),
        name,
        image_url,
        magazine_name,
        is_following,
        albums,
        total,
        has_more,
        albums_failed,
        other_awards,
    }
}

/// The "Other awards" rail — `/award/explore` minus the current award.
async fn load_other_awards<A>(
    runtime: &Arc<AppRuntime<A>>,
    current_id: &str,
) -> Vec<OtherAward>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let resp = match runtime
        .core()
        .get_award_explore(OTHER_AWARDS_LIMIT, 0)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[qbz-slint] award explore (other-awards) failed: {e}");
            return Vec::new();
        }
    };
    let items = resp
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    // Harvest the pairs while we're here (feeds the resolver).
    let pairs: Vec<(String, String)> = items
        .iter()
        .filter_map(|it| {
            let id = it.get("id").map(value_to_string)?;
            let nm = it.get("name").and_then(|v| v.as_str())?.to_string();
            (!id.is_empty() && !nm.is_empty()).then_some((id, nm))
        })
        .collect();
    remember_awards(&pairs);

    items
        .iter()
        .filter_map(|it| {
            let id = it.get("id").map(value_to_string).filter(|s| !s.is_empty())?;
            if id == current_id {
                return None;
            }
            let name = it.get("name").and_then(|v| v.as_str())?.to_string();
            if name.is_empty() {
                return None;
            }
            let magazine = it
                .get("magazine")
                .and_then(|m| m.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let image_url = it.get("image").map(parse_image_value).unwrap_or_default();
            Some(OtherAward {
                id,
                name,
                magazine,
                image_url,
            })
        })
        .collect()
}

fn other_to_item(o: &OtherAward) -> SlimItem {
    SlimItem {
        id: o.id.clone().into(),
        title: o.name.clone().into(),
        subtitle: o.magazine.clone().into(),
        rank: "".into(),
        artwork_url: o.image_url.clone().into(),
        artwork: slint::Image::default(),
        following: false,
    }
}

/// Apply the landing payload to `AwardState`. Runs on the Slint event loop.
pub fn apply_award_page(window: &AppWindow, payload: AwardPagePayload) {
    let albums: Vec<AlbumCardItem> = payload.albums.into_iter().map(to_item).collect();
    let other: Vec<SlimItem> = payload.other_awards.iter().map(other_to_item).collect();

    let state = window.global::<AwardState>();
    state.set_id(payload.id.into());
    state.set_name(payload.name.into());
    state.set_image_url(payload.image_url.into());
    state.set_magazine_name(payload.magazine_name.into());
    state.set_is_following(payload.is_following);
    state.set_following_toggling(false);
    state.set_albums(ModelRc::new(VecModel::from(albums)));
    state.set_total(payload.total as i32);
    state.set_has_more(payload.has_more);
    state.set_other_awards(ModelRc::new(VecModel::from(other)));
    state.set_load_error(payload.albums_failed);
    state.set_loading(false);
    state.set_page_loaded(true);
}

/// Artwork jobs for the landing (preview grid covers + other-awards images).
pub fn page_artwork_jobs(payload: &AwardPagePayload) -> Vec<ArtworkJob> {
    let mut jobs = Vec::new();
    for (i, a) in payload.albums.iter().enumerate() {
        if !a.artwork_url.is_empty() {
            jobs.push(ArtworkJob {
                url: a.artwork_url.clone(),
                target: ArtworkTarget::AwardAlbum { index: i },
            });
        }
    }
    for (i, o) in payload.other_awards.iter().enumerate() {
        if !o.image_url.is_empty() {
            jobs.push(ArtworkJob {
                url: o.image_url.clone(),
                target: ArtworkTarget::AwardOther { index: i },
            });
        }
    }
    jobs
}

/// Apply the decoded award hero image. Runs on the Slint event loop.
pub fn apply_image(window: &AppWindow, pixels: &[u8], width: u32, height: u32) {
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    let dst = buffer.make_mut_bytes();
    if dst.len() != pixels.len() {
        return;
    }
    dst.copy_from_slice(pixels);
    window
        .global::<AwardState>()
        .set_image(slint::Image::from_rgba8(buffer));
}

/// Clear the landing state before loading a new award. Unlike Tauri's
/// effect reset, this also clears `other-awards` (D2) so an award->award
/// jump never flashes the previous award's carousel.
pub fn reset_award_page(window: &AppWindow) {
    let state = window.global::<AwardState>();
    state.set_name("".into());
    state.set_image_url("".into());
    state.set_image(slint::Image::default());
    state.set_magazine_name("".into());
    state.set_is_following(false);
    state.set_following_toggling(false);
    state.set_albums(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
    state.set_visible(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
    state.set_other_awards(ModelRc::new(VecModel::from(Vec::<SlimItem>::new())));
    state.set_total(0);
    state.set_has_more(false);
    state.set_load_error(false);
    state.set_loading(true);
    state.set_load_more_loading(false);
    state.set_page_loaded(false);
    state.set_search_query("".into());
    state.set_shown(0);
}

// ======================================================================
//  Albums listing (AwardAlbumsView)
// ======================================================================

pub struct AwardAlbumsPayload {
    pub id: String,
    pub name: String,
    pub albums: Vec<AlbumCard>,
    pub total: usize,
    pub has_more: bool,
}

/// Fetch the first full-listing page (`/award/getAlbums`, limit 50). The
/// listing has no hero — the kicker uses the passed fallback name (1:1 with
/// Tauri's AwardAlbumsView, which never calls /award/page).
pub async fn load_award_albums<A>(
    runtime: &Arc<AppRuntime<A>>,
    award_id: &str,
    fallback_name: &str,
) -> Result<AwardAlbumsPayload, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let page = runtime
        .core()
        .get_award_albums(award_id, LIST_PAGE_SIZE, 0)
        .await
        .map_err(|e| e.to_string())?;
    let loaded = page.items.len();
    let total = page.total as usize;
    let albums = page.items.into_iter().map(map_album).collect();
    Ok(AwardAlbumsPayload {
        id: award_id.to_string(),
        name: fallback_name.to_string(),
        albums,
        total: total.max(loaded),
        has_more: total > loaded,
    })
}

/// Fetch one more listing page for load-more.
pub async fn load_more_award_albums<A>(
    runtime: &Arc<AppRuntime<A>>,
    award_id: &str,
    offset: u32,
) -> Result<(Vec<AlbumCard>, usize, bool), String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let page = runtime
        .core()
        .get_award_albums(award_id, LIST_PAGE_SIZE, offset)
        .await
        .map_err(|e| e.to_string())?;
    let loaded = offset as usize + page.items.len();
    let total = page.total as usize;
    let albums = page.items.into_iter().map(map_album).collect();
    Ok((albums, total.max(loaded), total > loaded))
}

/// Apply the first listing page to `AwardState`.
pub fn apply_award_albums(window: &AppWindow, payload: AwardAlbumsPayload) {
    let items: Vec<AlbumCardItem> = payload.albums.into_iter().map(to_item).collect();
    let state = window.global::<AwardState>();
    state.set_id(payload.id.into());
    state.set_name(payload.name.into());
    state.set_albums(ModelRc::new(VecModel::from(items)));
    state.set_total(payload.total as i32);
    state.set_has_more(payload.has_more);
    state.set_load_error(false);
    state.set_loading(false);
    derive_award_albums(window);
}

/// Append the next listing page (load-more).
pub fn append_award_albums(
    window: &AppWindow,
    albums: Vec<AlbumCard>,
    total: usize,
    has_more: bool,
) {
    let state = window.global::<AwardState>();
    let model = state.get_albums();
    let mut combined: Vec<AlbumCardItem> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .collect();
    combined.extend(albums.into_iter().map(to_item));
    state.set_albums(ModelRc::new(VecModel::from(combined)));
    state.set_total(total as i32);
    state.set_has_more(has_more);
    state.set_load_more_loading(false);
    derive_award_albums(window);
}

/// Re-derive the listing's rendered `visible` model from the loaded catalog
/// + the search query (client-side title/artist filter). The no-search fast
/// path shares the `albums` model so artwork keeps updating in place.
pub fn derive_award_albums(window: &AppWindow) {
    let state = window.global::<AwardState>();
    let albums = state.get_albums();
    let query_owned = state.get_search_query().to_lowercase();
    let query = query_owned.trim();

    if query.is_empty() {
        state.set_visible(albums.clone());
        state.set_shown(albums.row_count() as i32);
        return;
    }
    let filtered: Vec<AlbumCardItem> = (0..albums.row_count())
        .filter_map(|i| albums.row_data(i))
        .filter(|a| {
            a.title.to_lowercase().contains(query) || a.artist.to_lowercase().contains(query)
        })
        .collect();
    let shown = filtered.len();
    state.set_visible(ModelRc::new(VecModel::from(filtered)));
    state.set_shown(shown as i32);
}

/// Artwork jobs for the listing grid (AwardState.albums).
pub fn albums_artwork_jobs(albums: &[AlbumCard], offset: usize) -> Vec<ArtworkJob> {
    albums
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.artwork_url.is_empty())
        .map(|(i, a)| ArtworkJob {
            url: a.artwork_url.clone(),
            target: ArtworkTarget::AwardAlbum { index: offset + i },
        })
        .collect()
}

/// Clear the listing state before loading a new award's albums.
pub fn reset_award_albums(window: &AppWindow) {
    let state = window.global::<AwardState>();
    state.set_name("".into());
    state.set_albums(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
    state.set_visible(ModelRc::new(VecModel::from(Vec::<AlbumCardItem>::new())));
    state.set_total(0);
    state.set_has_more(false);
    state.set_load_error(false);
    state.set_loading(true);
    state.set_load_more_loading(false);
    state.set_search_query("".into());
    state.set_shown(0);
}

// ======================================================================
//  Follow heart helpers
// ======================================================================

/// The user's followed-award ids from the network (`/favorite/getUserFavorites`
/// type=awards — the PLURAL read). Best-effort: an error yields an empty set.
/// Seeds `fav_cache` at login so the AwardView heart is correct on first open.
pub async fn favorite_award_ids<A>(runtime: &Arc<AppRuntime<A>>) -> HashSet<String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    match runtime.core().get_favorites("awards", 500, 0).await {
        Ok(v) => v
            .get("awards")
            .and_then(|a| a.get("items"))
            .and_then(|i| i.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|it| it.get("id").map(value_to_string))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => HashSet::new(),
    }
}

/// Current follow state for `award_id` — the header when it's the open
/// award, else the cache.
pub fn following_state(window: &AppWindow, award_id: &str) -> bool {
    let state = window.global::<AwardState>();
    if state.get_id().as_str() == award_id {
        return state.get_is_following();
    }
    crate::fav_cache::is_award_favorite(award_id)
}

/// Optimistically reflect a follow toggle on the header (when it's the open
/// award) and set the toggling flag.
pub fn mark_following(window: &AppWindow, award_id: &str, following: bool, toggling: bool) {
    let state = window.global::<AwardState>();
    if state.get_id().as_str() == award_id {
        state.set_is_following(following);
        state.set_following_toggling(toggling);
    }
}

/// Name of an "Other awards" card by id (nav-history fallback for a click).
pub fn other_award_name(window: &AppWindow, award_id: &str) -> String {
    let model = window.global::<AwardState>().get_other_awards();
    for i in 0..model.row_count() {
        if let Some(item) = model.row_data(i) {
            if item.id.as_str() == award_id {
                return item.title.to_string();
            }
        }
    }
    String::new()
}

// ======================================================================
//  Value helpers
// ======================================================================

/// String for an id-ish Value (string verbatim, number stringified).
fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Best URL out of an award/magazine `image` Value (string or object).
fn parse_image_value(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    for key in ["large", "extralarge", "mega", "medium", "thumbnail", "small"] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            return s.to_string();
        }
    }
    String::new()
}
