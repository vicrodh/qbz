//! Track Info + Album Info (Credits/Review) modal controllers.
//!
//! 1:1 port of Tauri's `TrackInfoModal.svelte` + `AlbumCreditsModal.svelte`.
//! Both fetch fresh data through `QbzCore` (`get_track` / `get_album`), map it
//! to plain `Send` structs on the worker thread, then apply it to the
//! `TrackInfoState` / `AlbumInfoState` globals on the Slint event loop —
//! mirroring `crate::album::navigate_album`. Role parsing / grouping /
//! localization lives in `qbz_qobuz::performers` (frontend-agnostic, ADR-006).

use std::sync::Arc;

use chrono::NaiveDate;
use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{Album, Track};
use qbz_qobuz::performers::{format_role_label, group_credits_ordered, parse_performers};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::{
    AlbumCreditPerformer, AlbumCreditTrack, AlbumInfoState, AlbumState, AppWindow, InfoCreditPair,
    InfoCreditRow, TrackInfoState,
};

// ---------------------------------------------------------------------------
// Plain, `Send` data produced on the worker thread.
// ---------------------------------------------------------------------------

struct CreditRowData {
    /// Already-localized, UPPER-CASED role label (display).
    role: String,
    /// Original role string (for the musician-nav role hint, 1:1 with Tauri
    /// which passes the raw group role, not the display label).
    role_raw: String,
    names: Vec<String>,
}

pub struct TrackInfoData {
    title: String,
    album: String,
    artist: String,
    /// "" -> render the artist as plain text (no link).
    artist_id: String,
    duration: String,
    quality: String,
    isrc: String,
    label: String,
    label_id: String,
    copyright: String,
    credits: Vec<CreditRowData>,
}

struct PerformerData {
    name: String,
    /// ", Role1, Role2" suffix (empty when the performer has no roles).
    roles: String,
    /// First role (clean), or "Performer" — the musician-nav role hint,
    /// 1:1 with Tauri `handlePerformerClick` (roles[0] || 'Performer').
    primary_role: String,
}

struct AlbumTrackData {
    id: String,
    number: String,
    title: String,
    artist: String,
    has_credits: bool,
    performers: Vec<PerformerData>,
    copyright: String,
}

pub struct AlbumCreditsData {
    title: String,
    artist: String,
    label: String,
    label_id: String,
    release_date: String,
    meta_line: String,
    quality: String,
    review: String,
    has_review: bool,
    tracks: Vec<AlbumTrackData>,
}

// ---------------------------------------------------------------------------
// Formatting helpers (mirror the Tauri modal helpers).
// ---------------------------------------------------------------------------

/// "Title (Version)" when a non-empty version exists, else "Title".
fn format_title(title: &str, version: Option<&str>) -> String {
    let title = title.trim();
    match version.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => format!("{title} ({v})"),
        None => title.to_string(),
    }
}

/// Track length as "M:SS" (zero-padded seconds), like Tauri `formatDuration`.
fn track_duration(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Album length as "1h 21m" / "45m" (no seconds), like `formatAlbumDuration`.
fn album_duration(secs: u32) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// Sample-rate value without a trailing ".0" (96.0 -> "96", 44.1 -> "44.1"),
/// matching JS number interpolation in the Tauri modals. NOT normalized
/// (Hz vs kHz) — the modals print the raw maximum_sampling_rate as Tauri does.
fn fmt_rate(rate: f64) -> String {
    if rate.fract().abs() < f64::EPSILON {
        format!("{}", rate as i64)
    } else {
        format!("{rate}")
    }
}

/// Track Info quality — 1:1 with Tauri `formatQuality`: "24-bit / 96kHz" (NO
/// space before kHz), each field only when present; when both are absent fall
/// back to the Hi-Res / Lossless label (by `hires_streamable`).
fn track_quality(bit: Option<u32>, rate: Option<f64>, hires_streamable: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(b) = bit {
        parts.push(format!("{b}-bit"));
    }
    if let Some(r) = rate {
        parts.push(format!("{}kHz", fmt_rate(r)));
    }
    if parts.is_empty() {
        return if hires_streamable { "Hi-Res" } else { "Lossless" }.to_string();
    }
    parts.join(" / ")
}

/// Album Info quality — 1:1 with Tauri `formatQuality(bitDepth, samplingRate)`:
/// "24-Bit / 96 kHz" (capital Bit, space before kHz); the three present/absent
/// branches; empty string when both are absent (no fabricated defaults).
fn album_quality(bit: Option<u32>, rate: Option<f64>) -> String {
    match (bit, rate) {
        (Some(b), Some(r)) => format!("{b}-Bit / {} kHz", fmt_rate(r)),
        (Some(b), None) => format!("{b}-Bit"),
        (None, Some(r)) => format!("{} kHz", fmt_rate(r)),
        (None, None) => String::new(),
    }
}

/// Localized "September 2, 2021" (full month). Empty when no/invalid date.
fn full_release_date(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return String::new();
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    let head = raw.get(0..10).unwrap_or(raw);
    if let Ok(parsed) = NaiveDate::parse_from_str(head, "%Y-%m-%d") {
        return parsed
            .format_localized("%B %-d, %Y", crate::dates::current_locale())
            .to_string();
    }
    String::new()
}

/// ", Role1, Role2" suffix for an Album-Credits performer row (raw roles,
/// 1:1 with Tauri which does NOT localize these), or "" when role-less.
fn roles_suffix(roles: &[String]) -> String {
    if roles.is_empty() {
        String::new()
    } else {
        format!(", {}", roles.join(", "))
    }
}

// ---------------------------------------------------------------------------
// Mapping (worker thread).
// ---------------------------------------------------------------------------

fn map_track_info(track: Track) -> TrackInfoData {
    let title = format_title(&track.title, track.version.as_deref());

    let (artist, artist_id) = match track.performer.as_ref() {
        Some(a) if a.id != 0 => (a.name.clone(), a.id.to_string()),
        Some(a) => (a.name.clone(), String::new()),
        None => (String::new(), String::new()),
    };

    let album_title = track
        .album
        .as_ref()
        .map(|a| a.title.clone())
        .unwrap_or_default();

    let (label, label_id) = match track.album.as_ref().and_then(|a| a.label.as_ref()) {
        Some(l) => (l.name.clone(), l.id.to_string()),
        None => (String::new(), String::new()),
    };

    let credits = group_credits_ordered(&parse_performers(
        track.performers.as_deref().unwrap_or_default(),
    ))
    .into_iter()
    .map(|(role, names)| CreditRowData {
        role: format_role_label(&role).to_uppercase(),
        role_raw: role,
        names,
    })
    .collect();

    TrackInfoData {
        title,
        album: album_title,
        artist,
        artist_id,
        duration: track_duration(track.duration),
        quality: track_quality(
            track.maximum_bit_depth,
            track.maximum_sampling_rate,
            track.hires_streamable,
        ),
        isrc: track.isrc.unwrap_or_default(),
        label,
        label_id,
        copyright: track.copyright.unwrap_or_default(),
        credits,
    }
}

fn map_album_credits(album: Album) -> AlbumCreditsData {
    let album_artist = album.artist.name.clone();

    let (label, label_id) = match album.label.as_ref() {
        Some(l) => (l.name.clone(), l.id.to_string()),
        None => (String::new(), String::new()),
    };

    let raw_tracks = album
        .tracks
        .as_ref()
        .map(|c| c.items.clone())
        .unwrap_or_default();

    let track_count = album
        .tracks_count
        .or(album.track_count)
        .unwrap_or(raw_tracks.len() as u32);

    // "Hard Rock · 10 tracks · 1h 21m" — gated on a genre, 1:1 with Tauri.
    let meta_line = match album.genre.as_ref().filter(|g| !g.name.is_empty()) {
        Some(g) => {
            // Tauri always appends formatAlbumDuration(album.duration || 0),
            // so an absent duration shows "· 0m" rather than dropping it.
            let parts = vec![
                g.name.clone(),
                format!("{track_count} tracks"),
                album_duration(album.duration.unwrap_or(0)),
            ];
            parts.join(" · ")
        }
        None => String::new(),
    };

    let tracks = raw_tracks
        .into_iter()
        .enumerate()
        .map(|(index, t)| {
            let performers: Vec<PerformerData> = parse_performers(
                t.performers.as_deref().unwrap_or_default(),
            )
            .into_iter()
            .map(|p| PerformerData {
                roles: roles_suffix(&p.roles),
                primary_role: p
                    .roles
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "Performer".to_string()),
                name: p.name,
            })
            .collect();
            let copyright = t.copyright.unwrap_or_default();
            let number = if t.track_number > 0 {
                t.track_number.to_string()
            } else {
                (index + 1).to_string()
            };
            let artist = t
                .performer
                .as_ref()
                .map(|a| a.name.clone())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| {
                    if album_artist.is_empty() {
                        "Unknown Artist".to_string()
                    } else {
                        album_artist.clone()
                    }
                });
            AlbumTrackData {
                id: t.id.to_string(),
                number,
                title: format_title(&t.title, t.version.as_deref()),
                artist,
                has_credits: !performers.is_empty() || !copyright.is_empty(),
                performers,
                copyright,
            }
        })
        .collect();

    let review = album
        .description
        .as_deref()
        .map(crate::strip_html::strip_html)
        .unwrap_or_default();
    let has_review = !review.trim().is_empty();

    AlbumCreditsData {
        title: album.title,
        artist: album_artist,
        label,
        label_id,
        release_date: full_release_date(album.release_date_original.as_deref()),
        meta_line,
        quality: album_quality(album.maximum_bit_depth, album.maximum_sampling_rate),
        review,
        has_review,
        tracks,
    }
}

// ---------------------------------------------------------------------------
// Apply (Slint event loop).
// ---------------------------------------------------------------------------

fn apply_track_info(window: &AppWindow, data: TrackInfoData) {
    let st = window.global::<TrackInfoState>();
    // Build the ordered cells, then pair them into 2-column rows so the modal
    // needs no dynamic grid placement.
    let cells: Vec<InfoCreditRow> = data
        .credits
        .into_iter()
        .map(|c| {
            let names: Vec<SharedString> = c.names.into_iter().map(SharedString::from).collect();
            InfoCreditRow {
                role: c.role.into(),
                role_raw: c.role_raw.into(),
                names: ModelRc::new(VecModel::from(names)),
            }
        })
        .collect();
    let mut pairs: Vec<InfoCreditPair> = Vec::new();
    let mut iter = cells.into_iter();
    while let Some(left) = iter.next() {
        match iter.next() {
            Some(right) => pairs.push(InfoCreditPair {
                left,
                right,
                has_right: true,
            }),
            None => pairs.push(InfoCreditPair {
                left,
                right: InfoCreditRow {
                    role: SharedString::new(),
                    role_raw: SharedString::new(),
                    names: ModelRc::new(VecModel::from(Vec::<SharedString>::new())),
                },
                has_right: false,
            }),
        }
    }
    st.set_title(data.title.into());
    st.set_album(data.album.into());
    st.set_artist(data.artist.into());
    st.set_artist_id(data.artist_id.into());
    st.set_duration(data.duration.into());
    st.set_quality(data.quality.into());
    st.set_isrc(data.isrc.into());
    st.set_label(data.label.into());
    st.set_label_id(data.label_id.into());
    st.set_copyright(data.copyright.into());
    st.set_credits(ModelRc::new(VecModel::from(pairs)));
}

fn apply_album_credits(window: &AppWindow, data: AlbumCreditsData) {
    let st = window.global::<AlbumInfoState>();
    let tracks: Vec<AlbumCreditTrack> = data
        .tracks
        .into_iter()
        .map(|t| {
            let perfs: Vec<AlbumCreditPerformer> = t
                .performers
                .into_iter()
                .map(|p| AlbumCreditPerformer {
                    name: p.name.into(),
                    roles: p.roles.into(),
                    primary_role: p.primary_role.into(),
                })
                .collect();
            AlbumCreditTrack {
                id: t.id.into(),
                number: t.number.into(),
                title: t.title.into(),
                artist: t.artist.into(),
                has_credits: t.has_credits,
                performers: ModelRc::new(VecModel::from(perfs)),
                copyright: t.copyright.into(),
            }
        })
        .collect();
    // The modal opens from the album header, so the cover already lives in
    // AlbumState — reuse it instead of re-fetching the artwork.
    st.set_artwork(window.global::<AlbumState>().get_artwork());
    st.set_title(data.title.into());
    st.set_artist(data.artist.into());
    st.set_label(data.label.into());
    st.set_label_id(data.label_id.into());
    st.set_release_date(data.release_date.into());
    st.set_meta_line(data.meta_line.into());
    st.set_quality(data.quality.into());
    st.set_review(data.review.into());
    st.set_has_review(data.has_review);
    st.set_active_tab("credits".into());
    st.set_tracks(ModelRc::new(VecModel::from(tracks)));
}

// ---------------------------------------------------------------------------
// Public spawn entry points (called from the media-action handler).
// ---------------------------------------------------------------------------

/// Fetch a track and open the Track Info modal.
pub fn open_track_info<A>(
    runtime: Arc<AppRuntime<A>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    track_id: u64,
) where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let _ = weak.upgrade_in_event_loop(|w| {
        let st = w.global::<TrackInfoState>();
        st.set_error("".into());
        st.set_loading(true);
        st.set_open(true);
    });
    handle.spawn(async move {
        match runtime.core().get_track(track_id).await {
            Ok(track) => {
                let data = map_track_info(track);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    apply_track_info(&w, data);
                    w.global::<TrackInfoState>().set_loading(false);
                });
            }
            Err(e) => {
                log::error!("[qbz-slint] track-info load failed: {e}");
                let msg = e.to_string();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let st = w.global::<TrackInfoState>();
                    st.set_error(msg.into());
                    st.set_loading(false);
                });
            }
        }
    });
}

/// Fetch an album and open the Album Info (Credits/Review) modal.
pub fn open_album_credits<A>(
    runtime: Arc<AppRuntime<A>>,
    weak: slint::Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    album_id: String,
) where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let _ = weak.upgrade_in_event_loop(|w| {
        let st = w.global::<AlbumInfoState>();
        st.set_error("".into());
        st.set_active_tab("credits".into());
        st.set_loading(true);
        st.set_open(true);
    });
    handle.spawn(async move {
        match runtime.core().get_album(&album_id).await {
            Ok(album) => {
                let data = map_album_credits(album);
                let _ = weak.upgrade_in_event_loop(move |w| {
                    apply_album_credits(&w, data);
                    w.global::<AlbumInfoState>().set_loading(false);
                });
            }
            Err(e) => {
                log::error!("[qbz-slint] album-info load failed: {e}");
                let msg = e.to_string();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    let st = w.global::<AlbumInfoState>();
                    st.set_error(msg.into());
                    st.set_loading(false);
                });
            }
        }
    });
}
