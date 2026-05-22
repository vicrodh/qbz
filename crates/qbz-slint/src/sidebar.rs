//! Sidebar playlists controller. Loads the user's own playlists into
//! SidebarState for the left-nav list (clicking a row opens the
//! playlist detail view). Reloaded on shell entry and after a playlist
//! is created or deleted.

use qbz_app::shell::AppRuntime;
use qbz_core::FrontendAdapter;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AppWindow, SidebarPlaylistItem, SidebarState};

pub struct SidebarPlaylist {
    pub id: String,
    pub name: String,
}

pub async fn load<A>(runtime: &AppRuntime<A>) -> Vec<SidebarPlaylist>
where
    A: FrontendAdapter + Send + Sync + 'static,
{
    match runtime.core().get_user_playlists().await {
        Ok(playlists) => playlists
            .into_iter()
            .map(|p| SidebarPlaylist {
                id: p.id.to_string(),
                name: p.name,
            })
            .collect(),
        Err(e) => {
            log::warn!("[qbz-slint] sidebar playlists load failed: {e}");
            Vec::new()
        }
    }
}

pub fn set_loading(window: &AppWindow, loading: bool) {
    window.global::<SidebarState>().set_loading(loading);
}

pub fn apply(window: &AppWindow, playlists: Vec<SidebarPlaylist>) {
    let items: Vec<SidebarPlaylistItem> = playlists
        .into_iter()
        .map(|p| SidebarPlaylistItem {
            id: p.id.into(),
            name: p.name.into(),
        })
        .collect();
    let state = window.global::<SidebarState>();
    state.set_playlists(ModelRc::new(VecModel::from(items)));
    state.set_loading(false);
}

/// Highlight the open playlist in the sidebar (or clear with "").
pub fn set_active(window: &AppWindow, id: &str) {
    window.global::<SidebarState>().set_active_id(id.into());
}
