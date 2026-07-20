//! ArtistsByLocationView controller — runs the scene discovery for a
//! source artist's location and pushes the validated artist grid into
//! `LocationViewState`. Mirrors Tauri's ArtistsByLocationView.svelte
//! (minus the in-progress event stream, which the Slint port replaces
//! with a simple loading flag).

use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::artist::LocationParams;
use crate::artwork::{ArtworkJob, ArtworkTarget};
use crate::{AppWindow, LocationViewState, SlimItem};

/// Validation page size — how many MB candidates to validate against
/// Qobuz per call. Matches the Tauri view's LIMIT.
pub const PAGE_SIZE: usize = 30;

pub struct LocationData {
    pub scene_label: String,
    pub genre_summary: String,
    pub artists: Vec<ArtistCard>,
    pub total: usize,
}

#[derive(Clone)]
pub struct ArtistCard {
    pub qobuz_id: String,
    pub name: String,
    pub genres_line: String,
    pub image_url: String,
}

fn map_candidate(c: qbz_integrations::musicbrainz::LocationCandidate) -> ArtistCard {
    ArtistCard {
        qobuz_id: c.qobuz_id.map(|id| id.to_string()).unwrap_or_default(),
        name: c.qobuz_name.unwrap_or(c.mb_name),
        genres_line: c.genres.join(" · "),
        image_url: c.qobuz_image.unwrap_or_default(),
    }
}

/// Run the first page of scene discovery for `params`.
pub async fn load_scene<A>(
    runtime: &Arc<AppRuntime<A>>,
    params: &LocationParams,
    offset: usize,
) -> Result<LocationData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let area_id = (!params.area_id.is_empty()).then_some(params.area_id.as_str());
    let country = (!params.country.is_empty()).then_some(params.country.as_str());
    let response = runtime
        .core()
        .discover_artists_by_location(
            &params.mbid,
            area_id,
            &params.area_name,
            country,
            params.genres.clone(),
            params.tags.clone(),
            PAGE_SIZE,
            offset,
        )
        .await
        .map_err(|e| e.to_string())?;

    let _ = response.next_offset; // offset is recomputed from row_count

    // T8 + D-FIX-c: drop blacklisted scene candidates by resolved Qobuz id.
    //
    // D-FIX-c note: the Slint scene controller does NOT cache results (no
    // 30-day TTL cache exists in `qbz-core::discover_artists_by_location` nor
    // here — the Tauri bug was in the Tauri-only `integrations.rs` command
    // cache, which has no analogue in this path). We still re-apply the
    // blacklist HERE, on every result on the way out, so that even if any
    // upstream cache were ever introduced, a newly-blocked artist disappears
    // immediately. `is_blacklisted` auto-gates on the enabled flag; a
    // missing/None Qobuz id is kept (fail-open). `total` is decremented by
    // the number removed so the count stays honest.
    let mut artists = response.artists;
    let before = artists.len();
    artists.retain(|c| match c.qobuz_id {
        Some(id) if id >= 0 => !crate::artist_blacklist::is_blacklisted(id as u64),
        _ => true,
    });
    let removed = before - artists.len();
    let total = response.total_candidates.saturating_sub(removed);

    Ok(LocationData {
        scene_label: response.scene_label,
        genre_summary: response.genre_summary,
        artists: artists.into_iter().map(map_candidate).collect(),
        total,
    })
}

fn to_item(card: ArtistCard) -> SlimItem {
    // SlimItem mapping for the app-wide ArtistGridCard: the genres ride the
    // subtitle (second row under the name), follow/pin seed from the
    // disk-backed caches so the chips are right from first paint.
    SlimItem {
        following: card
            .qobuz_id
            .parse::<u64>()
            .map(crate::fav_cache::is_artist_favorite)
            .unwrap_or(false),
        is_pinned: crate::pinned::is_pinned("artist", &card.qobuz_id),
        id: card.qobuz_id.into(),
        title: card.name.into(),
        subtitle: card.genres_line.into(),
        artwork_url: card.image_url.into(),
        ..Default::default()
    }
}

pub fn apply_scene(window: &AppWindow, data: LocationData) {
    let items: Vec<SlimItem> = data.artists.into_iter().map(to_item).collect();
    let state = window.global::<LocationViewState>();
    state.set_scene_label(data.scene_label.into());
    state.set_genre_summary(data.genre_summary.into());
    state.set_artists(ModelRc::new(VecModel::from(items)));
    state.set_total(data.total as i32);
    state.set_loading(false);
}

pub fn append_scene(window: &AppWindow, artists: Vec<ArtistCard>, total: usize) {
    let state = window.global::<LocationViewState>();
    let model = state.get_artists();
    let mut combined: Vec<SlimItem> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .collect();
    combined.extend(artists.into_iter().map(to_item));
    state.set_artists(ModelRc::new(VecModel::from(combined)));
    state.set_total(total as i32);
    state.set_load_more_loading(false);
}

pub fn reset_scene(window: &AppWindow) {
    let state = window.global::<LocationViewState>();
    state.set_scene_label("".into());
    state.set_genre_summary("".into());
    state.set_artists(ModelRc::new(VecModel::from(Vec::<SlimItem>::new())));
    state.set_total(0);
    state.set_loading(true);
    state.set_load_more_loading(false);
}

/// Artwork jobs for the scene artist grid (the candidates' Qobuz
/// thumbnails).
pub fn artwork_jobs(data: &LocationData) -> Vec<ArtworkJob> {
    data.artists
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.image_url.is_empty())
        .map(|(i, a)| ArtworkJob {
            url: a.image_url.clone(),
            target: ArtworkTarget::LocationArtist { index: i },
        })
        .collect()
}
