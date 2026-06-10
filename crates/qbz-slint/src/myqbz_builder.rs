//! Discography Builder controller (spec 13) — the Rust side of
//! `DiscographyBuilderView`. It fetches an artist's complete discography from
//! THREE sources (Qobuz artist page + local library + Plex cache), dedupes the
//! releases into GROUPS (one logical album = a chosen "primary" + "alternates"),
//! and persists the user-selected groups as a `kind='artist_collection'`
//! collection (`source_type='artist_discography'`, `source_ref=<artist_id>`).
//!
//! `artist_discography` is a MARKER, not a sync feature (spec 40 §8): the builder
//! resolves the artist's albums and bulk-adds them as ordinary album items via
//! `qbz_mixtape::repo` — there is NO sync command. On save it collapses Plex
//! candidates to `source='local'` (the LocalLibrary resolver re-detects Plex at
//! enqueue time) and navigates to the new collection's detail.
//!
//! Sourcing notes (degradation):
//! - Qobuz: `runtime.core().get_artist_page` — the full path, sets artist name +
//!   avatar.
//! - Local + Plex: ONE unified `get_albums_metadata_page(…, plex_cache_db_path)`
//!   query (the same one the Local Library Albums tab uses). Plex rows arrive in
//!   that set with `source == "plex"` when the Plex master toggle is ON; with it
//!   OFF the query runs local-only. Rows are filtered to the artist by a
//!   case-insensitive match on `artist` OR any comma-split entry in `all_artists`
//!   (mirrors the PSD `matchesArtist`). If the DB is unavailable the local/plex
//!   contribution degrades to empty (logged) — the Qobuz path still fully works.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::mixtape::{CollectionKind, CollectionSourceType};

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{
    AppWindow, DiscoBuilderState, DiscoCandidate, DiscoGroup, ToastKind,
};

// ──────────────────────────── candidate model ─────────────────────────────

/// One release candidate (a primary or an alternate). Plain `Send` data built
/// on the worker thread; the UI thread maps it into the Slint `DiscoCandidate`.
#[derive(Clone)]
pub struct Candidate {
    pub group_key: String,
    /// "qobuz" | "local" | "plex".
    pub source: String,
    pub source_item_id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub artwork_url: Option<String>,
    pub track_count: Option<i32>,
    pub max_bit_depth: Option<u32>,
    /// kHz.
    pub max_sample_rate: Option<f64>,
    pub format: String,
    pub is_compilation: bool,
    /// The AUTO-classified release type (before any user override).
    pub release_type: String,
    pub quality_score: i64,
}

impl Candidate {
    /// `${source}|${source_item_id}` — the dedupe + selection key.
    fn key(&self) -> String {
        format!("{}|{}", self.source, self.source_item_id)
    }
}

/// One dedupe group: a primary plus its alternates.
#[derive(Clone)]
pub struct Group {
    pub key: String,
    pub title: String,
    pub year: Option<i32>,
    pub primary: Candidate,
    pub alternates: Vec<Candidate>,
    pub is_compilation: bool,
}

/// The full builder session state, held process-global on the UI thread side
/// (the controller mutates it; `apply` renders it). Mirrors the Svelte `$state`.
struct Session {
    artist_id: String,
    artist_name: String,
    artist_avatar_url: String,
    groups: Vec<Group>,
    /// `Record<groupKey, Set<candidateKey>>`.
    checked: HashMap<String, Vec<String>>,
    order_by: String,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            artist_id: String::new(),
            artist_name: String::new(),
            artist_avatar_url: String::new(),
            groups: Vec::new(),
            checked: HashMap::new(),
            order_by: "release_date".to_string(),
        }
    }
}

static SESSION: LazyLock<Mutex<Session>> = LazyLock::new(|| Mutex::new(Session::default()));

fn session() -> std::sync::MutexGuard<'static, Session> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

// ──────────────────────── release-type override store ──────────────────────
//
// The PSD writes overrides to a per-user localStorage sidecar shared with the
// Collection detail view. Here it's a process-global map persisted to a JSON
// sidecar under the user data dir so a chosen override survives the session and
// (when the Slint Collection-detail override UI lands) can be read back. Keyed
// `${source}|${source_item_id}` -> one of album|ep|single|live|compilation.

static OVERRIDES: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(load_overrides()));

fn overrides_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| {
        d.join("qbz")
            .join("discography-release-type-overrides.json")
    })
}

fn load_overrides() -> HashMap<String, String> {
    let Some(path) = overrides_path() else {
        return HashMap::new();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return HashMap::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_overrides(map: &HashMap<String, String>) {
    let Some(path) = overrides_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec(map) {
        let _ = std::fs::write(&path, bytes);
    }
}

fn override_for(source: &str, source_item_id: &str) -> Option<String> {
    let key = format!("{source}|{source_item_id}");
    OVERRIDES.lock().ok().and_then(|m| m.get(&key).cloned())
}

/// The EFFECTIVE release type after applying the user override (if any).
fn effective_type(c: &Candidate) -> String {
    override_for(&c.source, &c.source_item_id).unwrap_or_else(|| c.release_type.clone())
}

fn is_overridden(c: &Candidate) -> bool {
    override_for(&c.source, &c.source_item_id).is_some()
}

// ──────────────────────────── classification ──────────────────────────────

/// `normalizeTitle`: lowercase, strip parenthetical/bracket edition suffixes,
/// collapse whitespace, trim. Mirrors the PSD regex.
fn normalize_title(title: &str) -> String {
    let lower = title.to_lowercase();
    // Edition keywords that mark a re-issue suffix.
    const KEYWORDS: [&str; 14] = [
        "deluxe",
        "remaster",
        "expanded",
        "anniversary",
        "collector",
        "special",
        "bonus",
        "extended",
        "definitive",
        "20th",
        "25th",
        "30th",
        "40th",
        "50th",
    ];
    // Find the first '(' or '[' whose inner text starts with an edition
    // keyword (or an Nth marker) and strip from there to the end.
    let bytes = lower.as_bytes();
    let mut cut = lower.len();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch == '(' || ch == '[' {
            let inner = lower[i + 1..].trim_start();
            let starts_kw = KEYWORDS.iter().any(|k| inner.starts_with(k))
                || starts_with_nth(inner);
            if starts_kw {
                cut = i;
                break;
            }
        }
        i += 1;
    }
    let trimmed = &lower[..cut];
    // Collapse whitespace + trim.
    trimmed.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `s` begins with an `<digits>th` token (e.g. "10th anniversary").
fn starts_with_nth(s: &str) -> bool {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    !digits.is_empty() && s[digits.len()..].starts_with("th")
}

/// `isCompilation(title)` — best-of / greatest-hits markers.
fn title_is_compilation(title: &str) -> bool {
    let l = title.to_lowercase();
    const M: [&str; 6] = [
        "best of",
        "greatest hits",
        "anthology",
        "the very best",
        "essential",
        "collection",
    ];
    M.iter().any(|m| l.contains(m))
}

/// `classifyRelease` — precedence: compilation → live → ep → single → album →
/// track-count heuristic. `qobuz_release_type` / `qobuz_group_type` are the
/// per-item and the group's release_type from /artist/page (None for local/plex).
fn classify_release(
    title: &str,
    track_count: Option<i32>,
    qobuz_release_type: Option<&str>,
    qobuz_group_type: Option<&str>,
    title_is_comp: bool,
) -> String {
    let l = title.to_lowercase();
    let rt = qobuz_release_type.unwrap_or("");
    let gt = qobuz_group_type.unwrap_or("");
    let type_is = |needle: &str| rt.eq_ignore_ascii_case(needle) || gt.eq_ignore_ascii_case(needle);

    if rt.to_lowercase().contains("compilation")
        || gt.to_lowercase().contains("compilation")
        || title_is_comp
    {
        return "compilation".to_string();
    }
    let word = |w: &str| {
        // crude word-boundary contains
        l.split(|c: char| !c.is_alphanumeric()).any(|tok| tok == w)
    };
    if type_is("live") || word("live") || word("concert") || word("unplugged") {
        return "live".to_string();
    }
    if type_is("ep") || word("ep") {
        return "ep".to_string();
    }
    if type_is("single") {
        return "single".to_string();
    }
    if type_is("album") {
        return "album".to_string();
    }
    match track_count {
        Some(n) if n <= 3 => "single".to_string(),
        Some(n) if n <= 6 => "ep".to_string(),
        _ => "album".to_string(),
    }
}

/// `qualityScore`: bit*10_000_000 + rateHz + fmtBonus.
fn quality_score(
    max_bit_depth: Option<u32>,
    max_sample_rate_khz: Option<f64>,
    format: &str,
) -> i64 {
    let bit = max_bit_depth.unwrap_or(16) as i64;
    let rate_hz = ((max_sample_rate_khz.unwrap_or(44.1)) * 1000.0).round() as i64;
    let fmt = format.to_lowercase();
    let fmt_bonus = if fmt.contains("flac") || fmt.contains("alac") {
        1000
    } else if fmt.contains("mp3") || fmt.contains("aac") {
        0
    } else {
        500
    };
    bit * 10_000_000 + rate_hz + fmt_bonus
}

// ──────────────────────────── source fetchers ─────────────────────────────

/// Fetch the artist's Qobuz releases. Returns `(candidates, artist_name,
/// avatar_url)`. Mirrors `fetchQobuzAlbums` — name + avatar are a side effect.
pub async fn fetch_qobuz<A>(
    runtime: &Arc<AppRuntime<A>>,
    artist_id: &str,
) -> Result<(Vec<Candidate>, String, String), String>
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

    let artist_name = page.name.display.clone();
    let avatar_url = page
        .images
        .as_ref()
        .and_then(|imgs| imgs.portrait.as_ref())
        .map(|p| {
            format!(
                "https://static.qobuz.com/images/artists/covers/medium/{}.{}",
                p.hash, p.format
            )
        })
        .unwrap_or_default();

    let mut out: Vec<Candidate> = Vec::new();
    for group in page.releases.into_iter().flatten() {
        let group_type = group.release_type.clone();
        for r in group.items.into_iter() {
            let year = r
                .dates
                .as_ref()
                .and_then(|d| d.original.as_deref())
                .and_then(|s| s.get(..4))
                .and_then(|y| y.parse::<i32>().ok());
            let artwork = r
                .image
                .as_ref()
                .and_then(|img| img.large.clone().or_else(|| img.best().cloned()));
            let track_count = r.tracks_count.map(|n| n as i32);
            let bit = r.audio_info.as_ref().and_then(|a| a.maximum_bit_depth);
            let rate = r.audio_info.as_ref().and_then(|a| a.maximum_sampling_rate);
            let title = r.title.clone();
            let title_comp = title_is_compilation(&title);
            let release_type = classify_release(
                &title,
                track_count,
                r.release_type.as_deref(),
                Some(&group_type),
                title_comp,
            );
            let is_comp = release_type == "compilation";
            let group_key = format!(
                "{}|{}",
                normalize_title(&title),
                year.map(|y| y.to_string()).unwrap_or_default()
            );
            out.push(Candidate {
                group_key,
                source: "qobuz".to_string(),
                source_item_id: r.id.clone(),
                title,
                artist: artist_name.clone(),
                year,
                artwork_url: artwork,
                track_count,
                max_bit_depth: bit,
                max_sample_rate: rate,
                format: "FLAC".to_string(),
                is_compilation: is_comp,
                release_type,
                quality_score: quality_score(bit, rate, "FLAC"),
            });
        }
    }
    Ok((out, artist_name, avatar_url))
}

fn plex_cache_db_path() -> Option<std::path::PathBuf> {
    if !crate::plex_settings::get().enabled {
        return None;
    }
    dirs::data_dir().map(|d| d.join("qbz").join("plex_cache.db"))
}

/// Whether a local album's `artist` / `all_artists` matches `artist_name`
/// (case-insensitive exact match on artist OR any comma-split all_artists entry).
fn matches_artist(artist: &str, all_artists: &str, artist_name: &str) -> bool {
    let needle = artist_name.trim().to_lowercase();
    if needle.is_empty() {
        return false;
    }
    if artist.trim().to_lowercase() == needle {
        return true;
    }
    all_artists
        .split(',')
        .any(|a| a.trim().to_lowercase() == needle)
}

/// Fetch local + Plex albums by the artist via the unified metadata-grouped
/// page (Plex union included when enabled). Blocking (DB). Degrades to empty on
/// DB failure (logged by `with_db`). `artist_name` MUST be resolved first (the
/// Qobuz fetch sets it) — an empty name drops every match (mirrors the PSD).
pub fn fetch_local_and_plex(artist_name: &str) -> Vec<Candidate> {
    if artist_name.trim().is_empty() {
        return Vec::new();
    }
    let plex_path = plex_cache_db_path();
    // Map INSIDE the `with_db` closure so `db.resolve_album_cover_fallback` is
    // reachable (mirrors the Albums grid at local_library.rs:500-514): the cover
    // PATH rides on the candidate's `artwork_url`, so the saved collection item
    // carries it and the detail rows render the real cover instead of the disc
    // placeholder. Plex rows arrive with a non-empty `/library/...` thumb path;
    // local rows carry `a.artwork_path`, with the same cover.jpg/folder.jpg
    // on-disk fallback the grid uses so a DB row missing artwork_path still
    // resolves a cover.
    // Same flags as the LocalLibrary Albums tab (include offline copies;
    // network content connectivity-keyed — see local_library.rs's
    // NETWORK-FOLDER VISIBILITY note) so the builder sees the identical
    // candidate set.
    let exclude_network = crate::local_library::exclude_network_folders_now();
    crate::library_db::with_db(move |db| {
        let page = db.get_albums_metadata_page(
            0,
            1_000_000,
            None,
            "artist",
            "asc",
            true,
            exclude_network,
            plex_path.as_deref(),
        )?;
        let out: Vec<Candidate> = page
            .albums
            .into_iter()
            .filter(|a| matches_artist(&a.artist, &a.all_artists, artist_name))
            .map(|a| {
                let source = if a.source == "plex" { "plex" } else { "local" }.to_string();
                let year = a.year.map(|y| y as i32);
                let track_count = if a.track_count > 0 {
                    Some(a.track_count as i32)
                } else {
                    None
                };
                let bit = a.bit_depth;
                // LocalAlbum.sample_rate is Hz; the candidate carries kHz.
                let rate_khz = if a.sample_rate >= 1000.0 {
                    Some(a.sample_rate / 1000.0)
                } else if a.sample_rate > 0.0 {
                    Some(a.sample_rate)
                } else {
                    None
                };
                // Cover path: the row's own artwork_path, else the on-disk
                // cover.jpg/folder.jpg fallback (local only — Plex rows already
                // carry a non-empty thumb path so the fallback no-ops).
                let artwork_url = a
                    .artwork_path
                    .clone()
                    .filter(|p| !p.is_empty())
                    .or_else(|| db.resolve_album_cover_fallback(&a.id));
                let format = a.format.to_string();
                let title = a.title.clone();
                let title_comp = title_is_compilation(&title);
                let release_type = classify_release(&title, track_count, None, None, title_comp);
                let is_comp = release_type == "compilation";
                let group_key = format!(
                    "{}|{}",
                    normalize_title(&title),
                    year.map(|y| y.to_string()).unwrap_or_default()
                );
                Candidate {
                    group_key,
                    source,
                    source_item_id: a.id,
                    title,
                    artist: a.artist,
                    year,
                    artwork_url,
                    track_count,
                    max_bit_depth: bit,
                    max_sample_rate: rate_khz,
                    format,
                    is_compilation: is_comp,
                    release_type,
                    quality_score: quality_score(bit, rate_khz, &a.format.to_string()),
                }
            })
            .collect();
        Ok(out)
    })
    .unwrap_or_default()
}

// ──────────────────────────── buildGroups ─────────────────────────────────

/// Dedupe incoming candidates by `${source}|${source_item_id}`, bucket by
/// `group_key`, sort each bucket by quality (Qobuz wins ties for the primary
/// slot), and produce groups.
pub fn build_groups(candidates: Vec<Candidate>) -> Vec<Group> {
    // 1. Dedupe by candidate key, preserving first-seen order.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deduped: Vec<Candidate> = Vec::new();
    for c in candidates {
        if seen.insert(c.key()) {
            deduped.push(c);
        }
    }

    // 2. Bucket by group_key, preserving first-seen bucket order (Map order).
    let mut order: Vec<String> = Vec::new();
    let mut buckets: HashMap<String, Vec<Candidate>> = HashMap::new();
    for c in deduped {
        if !buckets.contains_key(&c.group_key) {
            order.push(c.group_key.clone());
        }
        buckets.entry(c.group_key.clone()).or_default().push(c);
    }

    // 3+4. Within each bucket sort by quality_score desc, Qobuz-preferred on
    // ties; primary = sorted[0], alternates = rest.
    let mut groups: Vec<Group> = Vec::new();
    for key in order {
        let mut bucket = buckets.remove(&key).unwrap_or_default();
        bucket.sort_by(|a, b| {
            b.quality_score.cmp(&a.quality_score).then_with(|| {
                // equal quality → qobuz first.
                let aq = a.source == "qobuz";
                let bq = b.source == "qobuz";
                bq.cmp(&aq)
            })
        });
        let is_compilation = bucket.iter().all(|c| c.is_compilation);
        let primary = bucket.remove(0);
        let title = primary.title.clone();
        let year = primary.year;
        groups.push(Group {
            key,
            title,
            year,
            primary,
            alternates: bucket,
            is_compilation,
        });
    }
    groups
}

// ──────────────────────────── sort (orderedGroups) ─────────────────────────

/// Return the groups in the active sort order (clones into a fresh Vec).
fn ordered_groups(groups: &[Group], order_by: &str) -> Vec<Group> {
    let mut g = groups.to_vec();
    match order_by {
        "title" => g.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        "manual" => {} // insertion order — unchanged.
        _ => {
            // release_date: year asc, nulls last, tiebreak by title.
            g.sort_by(|a, b| match (a.year, b.year) {
                (None, None) => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(ya), Some(yb)) => ya.cmp(&yb),
            });
        }
    }
    g
}

// ──────────────────────────── selection helpers ────────────────────────────

fn candidate_key(c: &Candidate) -> String {
    c.key()
}

fn is_checked(s: &Session, group_key: &str, cand_key: &str) -> bool {
    s.checked
        .get(group_key)
        .map(|set| set.iter().any(|k| k == cand_key))
        .unwrap_or(false)
}

/// `allPrimariesChecked` — every non-compilation group has its primary checked
/// (compilations are treated as always-satisfied).
fn all_primaries_checked(s: &Session) -> bool {
    !s.groups.is_empty()
        && s.groups.iter().all(|g| {
            g.is_compilation || is_checked(s, &g.key, &candidate_key(&g.primary))
        })
}

fn some_primaries_checked(s: &Session) -> bool {
    !all_primaries_checked(s)
        && s.groups
            .iter()
            .any(|g| !g.is_compilation && is_checked(s, &g.key, &candidate_key(&g.primary)))
}

/// Total checked candidate count across all groups (primaries + alternates).
fn selected_count(s: &Session) -> usize {
    s.checked.values().map(|set| set.len()).sum()
}

// ──────────────────────────── apply / render ──────────────────────────────

fn map_candidate(s: &Session, c: &Candidate) -> DiscoCandidate {
    let eff = effective_type(c);
    let (tier, detail, _) = crate::quality::badge(&c.format, c.max_bit_depth, c.max_sample_rate);
    DiscoCandidate {
        key: candidate_key(c).into(),
        source: c.source.clone().into(),
        source_item_id: c.source_item_id.clone().into(),
        title: c.title.clone().into(),
        artist: c.artist.clone().into(),
        year: c
            .year
            .map(|y| y.to_string())
            .unwrap_or_else(|| "—".to_string())
            .into(),
        tracks: c
            .track_count
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".to_string())
            .into(),
        artwork_url: c.artwork_url.clone().unwrap_or_default().into(),
        release_type: eff.into(),
        is_overridden: is_overridden(c),
        quality_tier: tier.into(),
        quality_detail: detail.into(),
        checked: is_checked(s, &c.group_key, &candidate_key(c)),
    }
}

/// Render the full state global from the session. UI thread only.
pub fn apply(window: &AppWindow) {
    let s = session();
    let state = window.global::<DiscoBuilderState>();

    let groups = ordered_groups(&s.groups, &s.order_by);
    let rows: Vec<DiscoGroup> = groups
        .iter()
        .map(|g| {
            let alternates: Vec<DiscoCandidate> =
                g.alternates.iter().map(|c| map_candidate(&s, c)).collect();
            DiscoGroup {
                key: g.key.clone().into(),
                title: g.title.clone().into(),
                year: g
                    .year
                    .map(|y| y.to_string())
                    .unwrap_or_else(|| "—".to_string())
                    .into(),
                primary: map_candidate(&s, &g.primary),
                alternates: ModelRc::new(VecModel::from(alternates)),
                is_compilation: g.is_compilation,
            }
        })
        .collect();

    state.set_groups(ModelRc::new(VecModel::from(rows)));
    state.set_all_checked(all_primaries_checked(&s));
    state.set_some_checked(some_primaries_checked(&s));
    state.set_selected_count(selected_count(&s) as i32);
    state.set_group_count(s.groups.len() as i32);
    state.set_artist_name(s.artist_name.clone().into());
    state.set_artist_id(s.artist_id.clone().into());
    state.set_artist_avatar_url(s.artist_avatar_url.clone().into());
}

/// Reset the builder state before a fresh load.
pub fn reset(window: &AppWindow, artist_id: &str) {
    {
        let mut s = session();
        *s = Session {
            artist_id: artist_id.to_string(),
            order_by: "release_date".to_string(),
            ..Session::default()
        };
    }
    let state = window.global::<DiscoBuilderState>();
    state.set_loading(true);
    state.set_load_error("".into());
    state.set_creating(false);
    state.set_open_type_menu_key("".into());
    state.set_collection_name("".into());
    // Empty name -> trimmed-empty (Create disabled until a name is typed or the
    // default is installed).
    state.set_name_trimmed_empty(true);
    state.set_order_by("release_date".into());
    state.set_artist_id(artist_id.into());
    state.set_artist_name("".into());
    state.set_artist_avatar_url("".into());
    state.set_artist_avatar(slint::Image::default());
    state.set_groups(ModelRc::new(VecModel::from(Vec::<DiscoGroup>::new())));
    state.set_selected_count(0);
    state.set_group_count(0);
}

/// Install the fetched + grouped data into the session and seed the default
/// selection (each non-compilation primary pre-checked). UI thread.
pub fn install(
    window: &AppWindow,
    artist_name: String,
    artist_avatar_url: String,
    groups: Vec<Group>,
) {
    {
        let mut s = session();
        s.artist_name = artist_name.clone();
        s.artist_avatar_url = artist_avatar_url.clone();
        s.groups = groups;
        // Default selection: non-compilation primaries pre-checked.
        s.checked.clear();
        let group_snapshot: Vec<(String, bool, String)> = s
            .groups
            .iter()
            .map(|g| (g.key.clone(), g.is_compilation, candidate_key(&g.primary)))
            .collect();
        for (key, is_comp, primary_key) in group_snapshot {
            let mut set = Vec::new();
            if !is_comp {
                set.push(primary_key);
            }
            s.checked.insert(key, set);
        }
    }
    let state = window.global::<DiscoBuilderState>();
    // Default collection name if still empty.
    if state.get_collection_name().is_empty() {
        let base = if artist_name.is_empty() {
            "Artist".to_string()
        } else {
            artist_name.clone()
        };
        state.set_collection_name(format!("{base} — Complete Discography").into());
    }
    // Keep the trimmed-empty gate in sync with whatever the name now holds
    // (the default name is non-empty; a pre-existing whitespace name stays
    // disabled).
    state.set_name_trimmed_empty(state.get_collection_name().trim().is_empty());
    state.set_loading(false);
    apply(window);
}

/// Mark the load failed with a raw error message (shown verbatim).
pub fn fail(window: &AppWindow, message: String) {
    let state = window.global::<DiscoBuilderState>();
    state.set_loading(false);
    state.set_load_error(message.into());
}

// ──────────────────────────── interactions ────────────────────────────────

/// Toggle one candidate's checked state. UI thread; re-renders.
pub fn toggle_checked(window: &AppWindow, group_key: &str, cand_key: &str) {
    {
        let mut s = session();
        let set = s.checked.entry(group_key.to_string()).or_default();
        if let Some(pos) = set.iter().position(|k| k == cand_key) {
            set.remove(pos);
        } else {
            set.push(cand_key.to_string());
        }
    }
    apply(window);
}

/// Header tri-state toggle — only ever touches non-compilation PRIMARIES;
/// alternates + compilations are left exactly as the user had them.
pub fn toggle_all(window: &AppWindow) {
    {
        let mut s = session();
        let target = !all_primaries_checked(&s);
        let prims: Vec<(String, String)> = s
            .groups
            .iter()
            .filter(|g| !g.is_compilation)
            .map(|g| (g.key.clone(), candidate_key(&g.primary)))
            .collect();
        for (gkey, pkey) in prims {
            let set = s.checked.entry(gkey).or_default();
            let has = set.iter().any(|k| k == &pkey);
            if target && !has {
                set.push(pkey);
            } else if !target && has {
                set.retain(|k| k != &pkey);
            }
        }
    }
    apply(window);
}

/// Change the sort order.
pub fn set_order(window: &AppWindow, order_by: &str) {
    {
        let mut s = session();
        s.order_by = order_by.to_string();
    }
    window.global::<DiscoBuilderState>().set_order_by(order_by.into());
    apply(window);
}

/// Sync the collection-name into the state (the input is bound but we mirror
/// it so the save flow reads a single source). Also recomputes the
/// trimmed-empty flag that gates the Create button (Slint can't trim, so a
/// whitespace-only name must be detected here — spec §10.19).
pub fn name_changed(window: &AppWindow, name: &str) {
    let state = window.global::<DiscoBuilderState>();
    state.set_collection_name(name.into());
    state.set_name_trimmed_empty(name.trim().is_empty());
}

/// Apply a release-type override (persisted) + re-render.
pub fn set_type_override(window: &AppWindow, source: &str, source_item_id: &str, choice: &str) {
    {
        if let Ok(mut m) = OVERRIDES.lock() {
            m.insert(format!("{source}|{source_item_id}"), choice.to_string());
            save_overrides(&m);
        }
    }
    apply(window);
}

/// Clear a release-type override + re-render.
pub fn reset_type_override(window: &AppWindow, source: &str, source_item_id: &str) {
    {
        if let Ok(mut m) = OVERRIDES.lock() {
            m.remove(&format!("{source}|{source_item_id}"));
            save_overrides(&m);
        }
    }
    apply(window);
}

// ──────────────────────────── save flow ───────────────────────────────────

/// Build the ordered list of `(candidate, checked)` for the save flow, in the
/// CURRENT sort order, each group emitting `[primary, ...alternates]`.
pub struct SavePayload {
    pub name: String,
    pub artist_id: String,
    /// Checked candidates, in save order.
    pub items: Vec<Candidate>,
}

/// Snapshot the current selection into a `SavePayload` (UI thread).
pub fn save_payload(window: &AppWindow) -> Option<SavePayload> {
    let s = session();
    let name_raw = window.global::<DiscoBuilderState>().get_collection_name().to_string();
    let name = {
        let t = name_raw.trim();
        if t.is_empty() {
            "Artist Collection".to_string()
        } else {
            t.to_string()
        }
    };
    let groups = ordered_groups(&s.groups, &s.order_by);
    let mut items: Vec<Candidate> = Vec::new();
    for g in &groups {
        let mut ordered = vec![g.primary.clone()];
        ordered.extend(g.alternates.iter().cloned());
        for c in ordered {
            if is_checked(&s, &c.group_key, &candidate_key(&c)) {
                items.push(c);
            }
        }
    }
    if items.is_empty() {
        return None;
    }
    Some(SavePayload {
        name,
        artist_id: s.artist_id.clone(),
        items,
    })
}

/// Create the artist_collection + bulk-add the checked candidates. Blocking
/// (DB). Returns the new collection id on success. Plex candidates are stored
/// as `source='local'` (resolver re-detects at enqueue). Mirrors `handleCreate`.
pub fn create_collection(payload: &SavePayload) -> Option<String> {
    crate::library_db::with_db(|db| {
        Ok(db.with_connection(|conn| {
            let col = qbz_mixtape::repo::create_collection(
                conn,
                CollectionKind::ArtistCollection,
                &payload.name,
                None,
                CollectionSourceType::ArtistDiscography,
                Some(&payload.artist_id),
            )?;
            for c in &payload.items {
                // Source collapse on save: qobuz -> Qobuz; local OR plex ->
                // Local (the LocalLibrary resolver re-detects Plex at enqueue).
                let src = if c.source == "qobuz" {
                    qbz_models::mixtape::AlbumSource::Qobuz
                } else {
                    qbz_models::mixtape::AlbumSource::Local
                };
                if let Err(e) = qbz_mixtape::repo::add_item(
                    conn,
                    &col.id,
                    qbz_models::mixtape::ItemType::Album,
                    src,
                    &c.source_item_id,
                    &c.title,
                    Some(c.artist.as_str()),
                    c.artwork_url.as_deref(),
                    c.year,
                    c.track_count,
                ) {
                    log::warn!("[qbz-slint] disco builder add_item failed: {e}");
                }
            }
            Ok::<String, rusqlite::Error>(col.id)
        })
        .map_err(|e| {
            qbz_library::LibraryError::Database(format!("disco builder create failed: {e}"))
        })?)
    })
}

/// Set the creating flag (UI thread).
pub fn set_creating(window: &AppWindow, creating: bool) {
    window.global::<DiscoBuilderState>().set_creating(creating);
}

/// Success toast (spec note: the PSD keys are missing; use a real string).
pub fn toast_created(window: &AppWindow) {
    crate::toast::show(window, "Artist Collection created", ToastKind::Success);
}

/// Failure toast.
pub fn toast_failed(window: &AppWindow) {
    crate::toast::show(window, "Failed to create collection", ToastKind::Error);
}

/// Apply the decoded artist avatar to the state. UI thread.
pub fn apply_avatar(window: &AppWindow, pixels: &[u8], width: u32, height: u32) {
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    let dst = buffer.make_mut_bytes();
    if dst.len() != pixels.len() {
        return;
    }
    dst.copy_from_slice(pixels);
    window
        .global::<DiscoBuilderState>()
        .set_artist_avatar(slint::Image::from_rgba8(buffer));
}

