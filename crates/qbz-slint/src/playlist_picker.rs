//! "Add to playlist" picker controller. Loads the user's playlists
//! into PlaylistPickerState for the global picker modal; the pick
//! handler in main.rs adds the pending track to the chosen playlist.

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AppWindow, PlaylistPickItem, PlaylistPickerState};

pub struct PickPlaylist {
    pub id: String,
    pub name: String,
    pub tracks: u32,
    /// LOCAL playlist (library.db, id `local:<uuid>`) — adds write the
    /// local repo (works offline) instead of the Qobuz endpoint.
    pub is_local: bool,
}

/// Open the picker for `track_id` and mark it loading. UI thread.
pub fn open(window: &AppWindow, track_id: &str) {
    let state = window.global::<PlaylistPickerState>();
    state.set_track_id(track_id.into());
    state.set_track_ids(ModelRc::new(VecModel::from(Vec::<slint::SharedString>::new())));
    state.set_playlists(ModelRc::new(VecModel::from(Vec::<PlaylistPickItem>::new())));
    state.set_local_mode(false);
    state.set_loading(true);
    state.set_open(true);
}

/// Open the picker for a batch of track ids (bulk add). `local` routes the ids
/// as LocalLibrary row ids (i64) to `add_local_track_to_playlist` instead of
/// the Qobuz endpoint. UI thread.
pub fn open_multi(window: &AppWindow, ids: &[String], local: bool) {
    let state = window.global::<PlaylistPickerState>();
    state.set_track_id("".into());
    let model: Vec<slint::SharedString> = ids.iter().map(|s| s.clone().into()).collect();
    state.set_track_ids(ModelRc::new(VecModel::from(model)));
    state.set_playlists(ModelRc::new(VecModel::from(Vec::<PlaylistPickItem>::new())));
    state.set_local_mode(local);
    state.set_loading(true);
    state.set_open(true);
}

/// Fetch the user's playlists (worker thread): the LOCAL playlists
/// (library.db — always available) followed by the Qobuz set. While
/// OFFLINE the Qobuz fetch is skipped entirely (D3/D11: Qobuz playlists
/// can't be written to offline, so they are hidden from the picker).
pub async fn load<A>(runtime: &AppRuntime<A>) -> Vec<PickPlaylist>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    let mut out: Vec<PickPlaylist> = tokio::task::spawn_blocking(|| {
        crate::local_playlist::list_blocking()
            .into_iter()
            .map(|p| PickPlaylist {
                id: p.id,
                name: p.name,
                tracks: p.track_count,
                is_local: true,
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();

    if crate::offline_mode::engine().is_offline() {
        return out;
    }
    match runtime.core().get_user_playlists().await {
        Ok(playlists) => {
            out.extend(playlists.into_iter().map(|p| PickPlaylist {
                id: p.id.to_string(),
                name: p.name,
                tracks: p.tracks_count,
                is_local: false,
            }));
        }
        Err(e) => {
            log::warn!("[qbz-slint] playlist picker load failed: {e}");
        }
    }
    out
}

pub fn apply(window: &AppWindow, playlists: Vec<PickPlaylist>) {
    let items: Vec<PlaylistPickItem> = playlists
        .into_iter()
        .map(|p| PlaylistPickItem {
            id: p.id.into(),
            name: p.name.into(),
            tracks_line: if p.tracks > 0 {
                format!("{} tracks", p.tracks).into()
            } else {
                "".into()
            },
            is_local: p.is_local,
        })
        .collect();
    let state = window.global::<PlaylistPickerState>();
    state.set_playlists(ModelRc::new(VecModel::from(items)));
    state.set_loading(false);
}
