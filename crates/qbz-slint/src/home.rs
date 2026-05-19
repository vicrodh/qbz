//! Discover / Home controller.
//!
//! Fetches the Qobuz discover index through `QbzCore`, maps it into plain
//! (Send) data on the worker thread, and — separately, on the Slint event
//! loop — converts that into Slint models pushed onto the `HomeState`
//! global. Domain types never reach the `.slint` files.

use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{AlbumAward, DiscoverAlbum, DiscoverAudioInfo, DiscoverContainer};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AlbumCardItem, AppWindow, DiscoverSection, HomeState, SlimItem};

/// Plain, `Send` home data produced on the worker thread.
pub struct HomeData {
    pub sections: Vec<SectionData>,
    pub popular: Vec<SlimData>,
    pub recent: Vec<SlimData>,
    pub recent_albums: Vec<CardData>,
}

pub struct SectionData {
    pub title: String,
    pub albums: Vec<CardData>,
}

pub struct CardData {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub genre: String,
    pub year: String,
    /// "hires" | "cd" | "" — drives the icon-only quality badge.
    pub quality_tier: String,
    /// "Hi-Res: 24-bit / 96 kHz" — shown when hovering the quality badge.
    pub quality_label: String,
    pub ribbon: String,
    pub ribbon_kind: String,
    pub artwork_url: String,
}

/// A compact ranked item for the slim grid sections.
pub struct SlimData {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub rank: String,
    pub artwork_url: String,
}

/// Fetch the discover index and map it into Home sections.
pub async fn load_home<A>(runtime: &Arc<AppRuntime<A>>) -> Result<HomeData, String>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let response = runtime
        .core()
        .get_discover_index(None)
        .await
        .map_err(|e| e.to_string())?;
    let containers = response.containers;

    let mut sections = Vec::new();
    push_section(&mut sections, "New Releases", containers.new_releases);
    push_section(&mut sections, "Press Accolades", containers.press_awards);
    push_section(
        &mut sections,
        "Ideal Discography",
        containers.ideal_discography,
    );
    push_section(&mut sections, "Qobuzissimes", containers.qobuzissims);
    push_section(
        &mut sections,
        "Albums of the Week",
        containers.album_of_the_week,
    );

    let popular = containers
        .most_streamed
        .map(|container| container.data.items)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(index, album)| map_slim(index, album))
        .collect();

    // Recently played comes from the local play-history store, not the
    // discover index. Empty until the playback session records plays.
    let recent = crate::recently::load()
        .into_iter()
        .map(|track| SlimData {
            id: track.id,
            title: track.title,
            subtitle: track.subtitle,
            rank: String::new(),
            artwork_url: track.artwork_url,
        })
        .collect();
    let recent_albums = crate::recently::load_albums()
        .into_iter()
        .map(|album| CardData {
            id: album.id,
            title: album.title,
            artist: album.artist,
            genre: String::new(),
            year: String::new(),
            quality_tier: String::new(),
            quality_label: String::new(),
            ribbon: String::new(),
            ribbon_kind: String::new(),
            artwork_url: album.artwork_url,
        })
        .collect();

    Ok(HomeData {
        sections,
        popular,
        recent,
        recent_albums,
    })
}

fn push_section(
    out: &mut Vec<SectionData>,
    title: &str,
    container: Option<DiscoverContainer<DiscoverAlbum>>,
) {
    let Some(container) = container else {
        return;
    };
    if container.data.items.is_empty() {
        return;
    }
    out.push(SectionData {
        title: title.to_string(),
        albums: container.data.items.into_iter().map(map_album).collect(),
    });
}

fn map_album(album: DiscoverAlbum) -> CardData {
    let artist = album
        .artists
        .first()
        .map(|a| a.name.clone())
        .unwrap_or_default();
    let genre = album.genre.map(|g| g.name).unwrap_or_default();
    let year = album
        .dates
        .as_ref()
        .and_then(|d| d.original.as_ref().or(d.download.as_ref()).or(d.stream.as_ref()))
        .and_then(|date| date.get(0..4))
        .unwrap_or("")
        .to_string();
    let (ribbon, ribbon_kind) = pick_ribbon(album.awards.as_deref());
    let quality_tier = quality_tier(album.audio_info.as_ref()).to_string();
    let quality_label = quality_label(album.audio_info.as_ref());
    let artwork_url = album
        .image
        .large
        .or(album.image.thumbnail)
        .or(album.image.small)
        .unwrap_or_default();
    CardData {
        id: album.id,
        title: album.title,
        artist,
        genre,
        year,
        quality_tier,
        quality_label,
        ribbon,
        ribbon_kind,
        artwork_url,
    }
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

/// Convert one `CardData` into the Slint `AlbumCardItem`.
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

/// Convert worker-thread home data into Slint models and push them onto
/// the `HomeState` global. Must run on the Slint event loop.
pub fn apply_home(window: &AppWindow, data: HomeData) {
    let sections: Vec<DiscoverSection> = data
        .sections
        .into_iter()
        .map(|section| DiscoverSection {
            title: section.title.into(),
            albums: ModelRc::new(VecModel::from(
                section.albums.into_iter().map(card_to_item).collect::<Vec<_>>(),
            )),
        })
        .collect();

    let to_slim_items = |items: Vec<SlimData>| -> Vec<SlimItem> {
        items
            .into_iter()
            .map(|slim| SlimItem {
                id: slim.id.into(),
                title: slim.title.into(),
                subtitle: slim.subtitle.into(),
                rank: slim.rank.into(),
                artwork_url: slim.artwork_url.into(),
                artwork: slint::Image::default(),
                following: false,
            })
            .collect()
    };
    let popular = to_slim_items(data.popular);
    let recent = to_slim_items(data.recent);
    let recent_albums: Vec<AlbumCardItem> =
        data.recent_albums.into_iter().map(card_to_item).collect();

    let state = window.global::<HomeState>();
    state.set_sections(ModelRc::new(VecModel::from(sections)));
    state.set_popular(ModelRc::new(VecModel::from(popular)));
    state.set_recent(ModelRc::new(VecModel::from(recent)));
    state.set_recent_albums(ModelRc::new(VecModel::from(recent_albums)));
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
