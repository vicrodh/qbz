//! Discover / Home controller.
//!
//! Fetches the Qobuz discover index through `QbzCore`, maps it into plain
//! (Send) section data on the worker thread, and — separately, on the
//! Slint event loop — converts that into Slint models pushed onto the
//! `HomeState` global. Domain types never reach the `.slint` files.

use std::sync::Arc;

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use qbz_models::{AlbumAward, DiscoverAlbum, DiscoverAudioInfo, DiscoverContainer};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AlbumCardItem, AppWindow, DiscoverSection, HomeState};

/// Plain, `Send` section data produced on the worker thread.
pub struct SectionData {
    pub title: String,
    pub albums: Vec<CardData>,
}

pub struct CardData {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub quality: String,
    pub ribbon: String,
    pub ribbon_kind: String,
    pub artwork_url: String,
}

/// Time-of-day greeting, matching the QBZ home greeting strings.
pub fn greeting(display_name: &str) -> String {
    use chrono::Timelike;
    let hour = chrono::Local::now().hour();
    let part = if (5..12).contains(&hour) {
        "Good morning"
    } else if (12..17).contains(&hour) {
        "Good afternoon"
    } else if (17..21).contains(&hour) {
        "Good evening"
    } else {
        "Good night"
    };
    let first = display_name.split_whitespace().next().unwrap_or("");
    if first.is_empty() {
        part.to_string()
    } else {
        format!("{part}, {first}")
    }
}

/// Fetch the discover index and map it into Home sections.
pub async fn load_home<A>(runtime: &Arc<AppRuntime<A>>) -> Result<Vec<SectionData>, String>
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
    Ok(sections)
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
    let (ribbon, ribbon_kind) = pick_ribbon(album.awards.as_deref());
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
        quality: quality_string(album.audio_info.as_ref()),
        ribbon,
        ribbon_kind,
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

/// Format the quality badge as `{depth}-bit / {rate} kHz`.
fn quality_string(audio: Option<&DiscoverAudioInfo>) -> String {
    let Some(audio) = audio else {
        return String::new();
    };
    match (audio.maximum_bit_depth, audio.maximum_sampling_rate) {
        (Some(depth), Some(rate)) => format!("{depth}-bit / {} kHz", format_rate(rate)),
        _ => String::new(),
    }
}

fn format_rate(rate: f64) -> String {
    if rate.fract().abs() < 0.05 {
        format!("{}", rate.round() as i64)
    } else {
        format!("{rate:.1}")
    }
}

/// Convert worker-thread section data into Slint models and push them onto
/// the `HomeState` global. Must run on the Slint event loop.
pub fn apply_sections(window: &AppWindow, data: Vec<SectionData>) {
    let sections: Vec<DiscoverSection> = data
        .into_iter()
        .map(|section| {
            let albums: Vec<AlbumCardItem> = section
                .albums
                .into_iter()
                .map(|card| AlbumCardItem {
                    id: card.id.into(),
                    title: card.title.into(),
                    artist: card.artist.into(),
                    quality: card.quality.into(),
                    ribbon: card.ribbon.into(),
                    ribbon_kind: card.ribbon_kind.into(),
                    artwork_url: card.artwork_url.into(),
                })
                .collect();
            DiscoverSection {
                title: section.title.into(),
                albums: ModelRc::new(VecModel::from(albums)),
            }
        })
        .collect();
    window
        .global::<HomeState>()
        .set_sections(ModelRc::new(VecModel::from(sections)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_uses_first_name_only() {
        let g = greeting("Victor Rodriguez Hernandez");
        assert!(g.ends_with(", Victor"), "got: {g}");
    }

    #[test]
    fn greeting_without_name_omits_comma() {
        let g = greeting("");
        assert!(!g.contains(','), "got: {g}");
    }

    #[test]
    fn quality_formats_bit_depth_and_rate() {
        let audio = DiscoverAudioInfo {
            maximum_bit_depth: Some(24),
            maximum_sampling_rate: Some(96.0),
            maximum_channel_count: Some(2),
        };
        assert_eq!(quality_string(Some(&audio)), "24-bit / 96 kHz");
    }

    #[test]
    fn quality_keeps_fractional_rate() {
        let audio = DiscoverAudioInfo {
            maximum_bit_depth: Some(16),
            maximum_sampling_rate: Some(44.1),
            maximum_channel_count: Some(2),
        };
        assert_eq!(quality_string(Some(&audio)), "16-bit / 44.1 kHz");
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
