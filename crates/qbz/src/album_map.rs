//! Shared album → card mapping for every Qobuz album surface (label
//! releases, favorites albums, and any toolbar-driven album list).
//!
//! Owns the V2-nested-first-with-flat-fallback decode so the list-row
//! extras (TYPE / QUALITY / TRACKS / YEAR columns of `AlbumListRow`)
//! populate consistently, the quality-tier classification, and the
//! local sort used by the grid/list toolbar views. Both `label.rs` and
//! `favorites.rs` map through here so there is one implementation to
//! maintain.

use qbz_models::Album;

use crate::AlbumCardItem;

/// Plain album card — every field an `AlbumCard`/`AlbumListRow` can show.
#[derive(Clone)]
pub struct AlbumCard {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    pub genre: String,
    pub year: String,
    pub quality_tier: String,
    pub quality_label: String,
    pub artwork_url: String,
    // List-row extras (AlbumListRow columns; ignored by the grid card).
    pub release_type: String,   // "Album" | "EP" | "Single" (TYPE column)
    // "local" | "qobuz_download" | "plex" | "" — SOURCE column + the
    // always-visible source badge on the Local Library grid card.
    pub source: String,
    pub quality_detail: String, // "24-bit / 96 kHz"
    pub track_count: String,    // "12"
    pub plain_year: String,     // "1973"
}

/// Append a release `version` to a title the way the Qobuz web player does:
/// `Octavarium (2009 Remaster)`, `A Dramatic Turn Of Events (Hi-Res)`. No-op
/// when the version is absent/empty. Used by every album-title surface
/// (discography, search, album header, suggestions) so re-editions of the same
/// album are distinguishable instead of rendering as identical duplicates.
pub fn format_album_title(title: &str, version: Option<&str>) -> String {
    match version.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => format!("{title} ({v})"),
        None => title.to_string(),
    }
}

/// Map a decoded Qobuz album into an `AlbumCard`.
///
/// Prefers the V2 nested shape (`audio_info` / `dates` / `track_count` /
/// `artists[]`) returned by `/label/getAlbums` and the discover feeds,
/// falling back to the flat fields used by the favorites (`legacy`)
/// payload. The fallbacks resolve to the flat values when the nested
/// fields are absent, so favorites albums map losslessly too.
pub fn map_album(album: Album) -> AlbumCard {
    let bit_depth = album
        .audio_info
        .as_ref()
        .and_then(|a| a.maximum_bit_depth)
        .or(album.maximum_bit_depth);
    let sample_rate = album
        .audio_info
        .as_ref()
        .and_then(|a| a.maximum_sampling_rate)
        .or(album.maximum_sampling_rate);
    let quality_tier = tier_hires(bit_depth, album.hires || album.hires_streamable).to_string();
    let quality_label = match (bit_depth, sample_rate) {
        (Some(bd), Some(sr)) => format!("{}-bit / {} kHz", bd, sr),
        _ => String::new(),
    };
    let quality_detail = quality_label.clone();

    // Release date — nested `dates` first (original > download > stream),
    // else the flat `release_date_original`.
    let date = album
        .dates
        .as_ref()
        .and_then(|d| {
            d.original
                .clone()
                .or_else(|| d.download.clone())
                .or_else(|| d.stream.clone())
        })
        .or_else(|| album.release_date_original.clone());
    let year = crate::dates::release_label(date.as_deref());
    let plain_year = date
        .as_deref()
        .and_then(|s| s.get(..4).map(|y| y.to_string()))
        .unwrap_or_default();

    let tc = album.track_count.or(album.tracks_count);
    let track_count = tc
        .filter(|n| *n > 0)
        .map(|n| n.to_string())
        .unwrap_or_default();
    let release_type = release_type_label(album.release_type.as_deref(), tc);
    // Borrow `album` (artist + versioned title) before any owned field moves.
    let (artist, artist_id) = album_artist(&album);
    let title = format_album_title(&album.title, album.version.as_deref());
    let genre = album.genre.map(|g| g.name).unwrap_or_default();
    AlbumCard {
        id: album.id,
        title,
        artist,
        artist_id,
        genre,
        year,
        quality_tier,
        quality_label,
        artwork_url: album.image.best().cloned().unwrap_or_default(),
        release_type,
        // Qobuz album surfaces (Discover / Favorites / Label) hide the SOURCE
        // column and the badge, so leave it empty (preserves prior behavior).
        source: String::new(),
        quality_detail,
        track_count,
        plain_year,
    }
}

/// Quality tier from a bit depth alone — `hires` above 16-bit, else `cd`.
pub fn tier(bit_depth: Option<u32>) -> &'static str {
    match bit_depth {
        Some(b) if b > 16 => "hires",
        Some(_) => "cd",
        None => "",
    }
}

/// Quality tier from a resolved bit depth, with a `hires` boolean fallback
/// for payloads that omit the bit depth but still flag the release hi-res.
pub fn tier_hires(bit_depth: Option<u32>, hires: bool) -> &'static str {
    match bit_depth {
        Some(b) if b > 16 => "hires",
        Some(_) => "cd",
        None if hires => "hires",
        None => "",
    }
}

/// Display label for the TYPE column — the explicit `release_type` when the
/// payload provides a known one, else a track-count heuristic.
fn release_type_label(release_type: Option<&str>, track_count: Option<u32>) -> String {
    match release_type {
        Some("album") | Some("download") => qbz_i18n::t("Album"),
        Some("ep") | Some("epSingle") => qbz_i18n::t("EP"),
        Some("single") => qbz_i18n::t("Single"),
        Some("live") => qbz_i18n::t("Live"),
        Some("compilation") => qbz_i18n::t("Compilation"),
        _ => qbz_i18n::t(classify_release_type(track_count)),
    }
}

/// Album artist name + id. Many /label/getAlbums items leave the `artist`
/// object empty and only populate the `artists` credit array, which left
/// group-by-artist showing "Unknown Artist". Fall back to the main-artist
/// credit (else the first) — mirrors artist.rs map_release.
fn album_artist(album: &Album) -> (String, String) {
    if !album.artist.name.is_empty() {
        return (album.artist.name.clone(), album.artist.id.to_string());
    }
    if let Some(list) = album.artists.as_ref() {
        let pick = list
            .iter()
            .find(|a| {
                a.roles
                    .as_ref()
                    .map(|r| r.iter().any(|role| role == "main-artist"))
                    .unwrap_or(false)
            })
            .or_else(|| list.first());
        if let Some(a) = pick {
            return (a.name.clone(), a.id.to_string());
        }
    }
    (String::new(), String::new())
}

/// Classify the list-row TYPE column from the album's track count, for
/// payloads (favorites, /label/getAlbums) that carry no explicit
/// release_type. Mirrors home.rs's Discover heuristic
/// (<=3 = Single, <=6 = EP, else Album).
pub fn classify_release_type(track_count: Option<u32>) -> &'static str {
    // Marked at the definition so the extractor sees the English literals; the
    // call sites (`album_map`/`home`) translate the marked value with `t(...)`.
    match track_count {
        Some(n) if n <= 3 => qbz_i18n::mark("Single"),
        Some(n) if n <= 6 => qbz_i18n::mark("EP"),
        _ => qbz_i18n::mark("Album"),
    }
}

/// Build a Slint `AlbumCardItem` from an `AlbumCard`. SOURCE is left empty
/// (single-source Qobuz context — hide the column with `show-source: false`).
pub fn to_item(card: AlbumCard) -> AlbumCardItem {
    AlbumCardItem {
        // Favorite heart state from the login-seeded cache, so every card
        // surface fed by this funnel (album suggestions, label, awards, …)
        // renders the filled heart in sync with the album-detail header.
        // Local Library ids never match a Qobuz favorite id (and LL hides
        // the heart), so the lookup is harmless there.
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
        ribbon: "".into(),
        ribbon_kind: "".into(),
        artwork_url: card.artwork_url.into(),
        artwork: slint::Image::default(),
        // List-row extras — feed the AlbumListRow columns (TYPE / QUALITY /
        // TRACKS / YEAR) for the list view toggle.
        release_type: card.release_type.into(),
        source: card.source.into(),
        quality_detail: card.quality_detail.into(),
        track_count: card.track_count.into(),
        plain_year: card.plain_year.into(),
        ..Default::default()
    }
}

/// Local sort over already-mapped card items, by the toolbar's sort key.
/// `newest`/`oldest` sort on `plain_year`; `title-*`/`artist-*` are
/// case-insensitive; any other key (e.g. `default`) leaves order intact.
pub fn sort_album_items(items: &mut [AlbumCardItem], sort: &str) {
    match sort {
        "oldest" | "year-asc" => {
            items.sort_by(|a, b| a.plain_year.as_str().cmp(b.plain_year.as_str()))
        }
        "newest" | "year-desc" => {
            items.sort_by(|a, b| b.plain_year.as_str().cmp(a.plain_year.as_str()))
        }
        "title-asc" => items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        "title-desc" => items.sort_by(|a, b| b.title.to_lowercase().cmp(&a.title.to_lowercase())),
        "artist-asc" => {
            items.sort_by(|a, b| a.artist.to_lowercase().cmp(&b.artist.to_lowercase()))
        }
        "artist-desc" => {
            items.sort_by(|a, b| b.artist.to_lowercase().cmp(&a.artist.to_lowercase()))
        }
        _ => {}
    }
}
