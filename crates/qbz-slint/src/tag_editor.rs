//! Tag editor controller (Slint) — local album metadata.
//!
//! Opens from the local album detail's edit pencil, edits album + per-track
//! fields, and persists via sidecar (default) or opt-in direct file-write
//! (one-time confirm, blocked for CUE albums). The DB index is updated in the
//! same transaction. All DB / lofty work runs on `spawn_blocking`; the rfd
//! confirm runs async. Remote MusicBrainz/Discogs lookup is a follow-up.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak};

use qbz_library::{
    AlbumMetadataOverride, AlbumTagSidecar, AlbumTagWrite, AlbumTrackUpdate, LibraryError,
    TrackMetadataOverride, TrackTagWrite,
};

use crate::{AppWindow, TagEditorState, TagTrackEdit};

/// kv key for the direct-write one-time acknowledgement (replaces the Tauri
/// localStorage flag; cross-compat not required for an ack bit).
const ACK_KEY: &str = "localLibrary.tagEditor.directWriteAcknowledged";

/// Save generation — a newer save supersedes a slow one on apply.
static SAVE_GEN: AtomicU64 = AtomicU64::new(0);

/// Parse the year input (trim; empty => None = clear; 0..=3000 allowed).
fn parse_year(s: &str) -> Result<Option<u32>, ()> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    match t.parse::<i64>() {
        Ok(y) if (0..=3000).contains(&y) => Ok(Some(y as u32)),
        _ => Err(()),
    }
}

/// Lenient u32 parse for track/disc numbers (empty/invalid => None).
fn parse_num(s: &str) -> Option<u32> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<u32>().ok()
}

/// Open the editor for a local album. Pre-fetches the album's tracks off-thread
/// (LocalTrack carries file_path/cue_* the AlbumState rows lack), then seeds +
/// opens on the UI thread. `group_key` and `directory_path` are equal for
/// folder-grouped local albums (the common case).
pub fn open_tag_editor(
    weak: Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    group_key: String,
    directory_path: String,
) {
    let gk = group_key.clone();
    handle.spawn(async move {
        let tracks = tokio::task::spawn_blocking(move || {
            crate::local_library::fetch_album_tracks_blocking(&gk)
        })
        .await
        .unwrap_or_default();
        let _ = weak.upgrade_in_event_loop(move |w| {
            populate(&w, group_key, directory_path, tracks);
        });
    });
}

fn populate(
    w: &AppWindow,
    group_key: String,
    directory_path: String,
    tracks: Vec<qbz_library::LocalTrack>,
) {
    let album_title = tracks
        .first()
        .map(|t| {
            if !t.album_group_title.is_empty() {
                t.album_group_title.clone()
            } else {
                t.album.clone()
            }
        })
        .unwrap_or_default();
    let album_artist = qbz_library::compute_track_artist_match(&tracks).unwrap_or_default();
    let year = tracks
        .iter()
        .find_map(|t| t.year)
        .map(|y| y.to_string())
        .unwrap_or_default();
    let genre = tracks
        .iter()
        .find_map(|t| t.genre.clone().filter(|g| !g.trim().is_empty()))
        .unwrap_or_default();
    let catalog = tracks
        .iter()
        .find_map(|t| t.catalog_number.clone().filter(|c| !c.trim().is_empty()))
        .unwrap_or_default();
    let total_discs = tracks
        .iter()
        .filter_map(|t| t.disc_number)
        .max()
        .unwrap_or(1)
        .max(1) as i32;
    let can_direct = tracks
        .iter()
        .all(|t| t.cue_file_path.is_none() && t.cue_start_secs.is_none());

    let rows: Vec<TagTrackEdit> = tracks
        .iter()
        .map(|t| TagTrackEdit {
            id: t.id as i32,
            file_path: t.file_path.clone().into(),
            cue_file_path: t.cue_file_path.clone().unwrap_or_default().into(),
            cue_start_secs: t.cue_start_secs.unwrap_or(-1.0) as f32,
            has_cue: t.cue_file_path.is_some() || t.cue_start_secs.is_some(),
            title: t.title.clone().into(),
            disc_number: t.disc_number.map(|n| n.to_string()).unwrap_or_default().into(),
            track_number: t.track_number.map(|n| n.to_string()).unwrap_or_default().into(),
        })
        .collect();

    let s = w.global::<TagEditorState>();
    s.set_album_group_key(group_key.into());
    s.set_directory_path(directory_path.into());
    s.set_album_title(album_title.into());
    s.set_album_artist(album_artist.into());
    s.set_year_input(year.into());
    s.set_genre(genre.into());
    s.set_catalog_number(catalog.into());
    s.set_album_total_discs(total_discs);
    s.set_can_direct_write(can_direct);
    s.set_persistence_index(0);
    s.set_saving(false);
    s.set_write_progress_current(0);
    s.set_write_progress_total(0);
    s.set_tracks(ModelRc::new(VecModel::from(rows)));
    // Reset remote-lookup state.
    s.set_remote_provider_index(0);
    s.set_remote_searching(false);
    s.set_remote_loading(false);
    s.set_remote_results(ModelRc::new(VecModel::from(Vec::<crate::RemoteResultItem>::new())));
    s.set_selected_result_id("".into());
    s.set_show_remote_panel(false);
    s.set_has_searched(false);
    s.set_open(true);
}

pub fn close_tag_editor(weak: Weak<AppWindow>) {
    let _ = weak.upgrade_in_event_loop(|w| {
        w.global::<TagEditorState>().set_open(false);
    });
}

/// Persist the edits. Validates, gates the directory + CUE for direct mode,
/// confirms direct-write once, then writes (sidecar or files) + updates the DB
/// index on a blocking thread, and refreshes the open album.
pub fn save_tags(
    weak: Weak<AppWindow>,
    handle: tokio::runtime::Handle,
    image_cache: crate::artwork::ImageCache,
) {
    let Some(w) = weak.upgrade() else {
        return;
    };
    let s = w.global::<TagEditorState>();

    let group_key = s.get_album_group_key().to_string();
    let album_title = s.get_album_title().trim().to_string();
    let album_artist = s.get_album_artist().to_string();
    let year_input = s.get_year_input().to_string();
    let genre = s.get_genre().to_string();
    let catalog = s.get_catalog_number().to_string();
    let directory_path = s.get_directory_path().to_string();
    let direct = s.get_persistence_index() == 1;

    let model = s.get_tracks();
    let rows: Vec<TagTrackEdit> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .collect();

    // Validate (UI thread; toast + abort, leave the modal open).
    if album_title.is_empty() {
        crate::toast::error_weak(&weak, "Album title is required");
        return;
    }
    if rows.iter().any(|r| r.title.trim().is_empty()) {
        crate::toast::error_weak(&weak, "Every track needs a title");
        return;
    }
    let year = match parse_year(&year_input) {
        Ok(y) => y,
        Err(()) => {
            crate::toast::error_weak(&weak, "Year must be a number");
            return;
        }
    };
    let album_dir = group_key.trim().to_string();
    if !Path::new(&album_dir).is_dir() {
        crate::toast::error_weak(&weak, "Album folder not found on disk");
        return;
    }
    if direct && rows.iter().any(|r| r.has_cue) {
        crate::toast::error_weak(
            &weak,
            "Writing tags to files isn't supported for CUE albums; use sidecar mode",
        );
        return;
    }

    // Build the Send-safe payloads up front.
    let artist_opt = {
        let a = album_artist.trim();
        if a.is_empty() {
            None
        } else {
            Some(a.to_string())
        }
    };
    let genre_opt = {
        let g = genre.trim();
        if g.is_empty() {
            None
        } else {
            Some(g.to_string())
        }
    };
    let catalog_opt = {
        let c = catalog.trim();
        if c.is_empty() {
            None
        } else {
            Some(c.to_string())
        }
    };

    let track_updates: Vec<AlbumTrackUpdate> = rows
        .iter()
        .map(|r| AlbumTrackUpdate {
            id: r.id as i64,
            title: r.title.trim().to_string(),
            disc_number: parse_num(&r.disc_number),
            track_number: parse_num(&r.track_number),
        })
        .collect();
    let tw_tracks: Vec<TrackTagWrite> = rows
        .iter()
        .map(|r| TrackTagWrite {
            file_path: r.file_path.to_string(),
            title: r.title.trim().to_string(),
            track_number: parse_num(&r.track_number),
            disc_number: parse_num(&r.disc_number),
        })
        .collect();
    let track_overs: Vec<TrackMetadataOverride> = rows
        .iter()
        .map(|r| TrackMetadataOverride {
            file_path: r.file_path.to_string(),
            cue_start_secs: if r.cue_start_secs >= 0.0 {
                Some(r.cue_start_secs as f64)
            } else {
                None
            },
            title: Some(r.title.trim().to_string()),
            disc_number: parse_num(&r.disc_number),
            track_number: parse_num(&r.track_number),
        })
        .collect();
    let album_over = AlbumMetadataOverride {
        album_title: Some(album_title.clone()),
        album_artist: artist_opt.clone(),
        year,
        genre: genre_opt.clone(),
        catalog_number: catalog_opt.clone(),
    };
    let tw_album = AlbumTagWrite {
        album_title: album_title.clone(),
        album_artist: album_artist.clone(),
        year,
        genre: genre_opt.clone(),
        catalog_number: catalog_opt.clone(),
    };

    let handle2 = handle.clone();
    handle.spawn(async move {
        // Direct-write one-time confirm.
        if direct {
            let acked = tokio::task::spawn_blocking(|| {
                crate::library_db::with_db(|db| db.get_kv(ACK_KEY))
            })
            .await
            .ok()
            .flatten()
            .flatten()
            .as_deref()
                == Some("1");
            if !acked {
                let ok = rfd::AsyncMessageDialog::new()
                    .set_title("Write tags to audio files?")
                    .set_description(
                        "This modifies your audio files on disk and cannot be undone.",
                    )
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    .await
                    == rfd::MessageDialogResult::Yes;
                if !ok {
                    return;
                }
                let _ = tokio::task::spawn_blocking(|| {
                    crate::library_db::with_db(|db| db.set_kv(ACK_KEY, "1"))
                })
                .await;
            }
        }

        let gen = SAVE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = weak.upgrade_in_event_loop(|w| {
            w.global::<TagEditorState>().set_saving(true);
        });

        let weak_p = weak.clone();
        let album_dir_c = album_dir.clone();
        let group_key_c = group_key.clone();
        let album_title_c = album_title.clone();
        let album_artist_c = album_artist.clone();
        let genre_d = genre_opt.clone();
        let catalog_d = catalog_opt.clone();
        let result: Result<(), LibraryError> = tokio::task::spawn_blocking(move || {
            let dir = Path::new(&album_dir_c);
            if direct {
                qbz_library::write_album_tags_to_files(&tw_album, &tw_tracks, |cur, tot| {
                    let _ = weak_p.upgrade_in_event_loop(move |w| {
                        let s = w.global::<TagEditorState>();
                        s.set_write_progress_current(cur as i32);
                        s.set_write_progress_total(tot as i32);
                    });
                })?;
                let _ = qbz_library::delete_album_sidecar(dir);
            } else {
                let sidecar = AlbumTagSidecar::new(album_over, track_overs);
                qbz_library::write_album_sidecar(dir, &sidecar)?;
            }
            // DB index update (transactional -> &mut db).
            crate::library_db::with_db_mut(|db| {
                let existing = db.get_album_tracks(&group_key_c)?;
                let m = qbz_library::compute_track_artist_match(&existing);
                db.update_album_group_metadata(
                    &group_key_c,
                    &album_title_c,
                    &album_artist_c,
                    year,
                    genre_d.as_deref(),
                    catalog_d.as_deref(),
                    m.as_deref(),
                    &track_updates,
                )
            })
            .ok_or_else(|| LibraryError::Database("library index update failed".to_string()))?;
            Ok(())
        })
        .await
        .unwrap_or_else(|e| Err(LibraryError::Other(format!("save task panicked: {e}"))));

        let ok = result.is_ok();
        let err_msg = result.err().map(|e| e.to_string()).unwrap_or_default();
        let _ = weak.upgrade_in_event_loop(move |w| {
            if SAVE_GEN.load(Ordering::SeqCst) != gen {
                return;
            }
            let s = w.global::<TagEditorState>();
            s.set_saving(false);
            s.set_write_progress_current(0);
            s.set_write_progress_total(0);
            if ok {
                s.set_open(false);
            }
        });

        if ok {
            crate::toast::success_weak(&weak, "Album metadata saved");
            // Refresh the open album detail + reset browse models (D7).
            refresh_after_save(weak.clone(), handle2.clone(), image_cache.clone());
        } else {
            crate::toast::error_weak(&weak, format!("Couldn't save metadata: {err_msg}"));
        }
        let _ = directory_path; // reserved (explicit directory plumbing, if added later)
    });
}

// ============================== Remote lookup =============================

/// Remote search/apply generation — a newer request supersedes a slow one.
static REMOTE_GEN: AtomicU64 = AtomicU64::new(0);

fn map_search(r: &qbz_integrations::RemoteAlbumSearchResult) -> crate::RemoteResultItem {
    let provider = if matches!(r.provider, qbz_integrations::RemoteProvider::Discogs) {
        "discogs"
    } else {
        "musicbrainz"
    };
    crate::RemoteResultItem {
        provider: provider.into(),
        provider_id: r.provider_id.clone().into(),
        title: r.title.clone().into(),
        artist: r.artist.clone().into(),
        year: r.year.unwrap_or(0) as i32,
        has_year: r.year.is_some(),
        track_count: r.track_count.unwrap_or(0) as i32,
        has_track_count: r.track_count.is_some(),
        country: r.country.clone().unwrap_or_default().into(),
        format: r.format.clone().unwrap_or_default().into(),
        label: r.label.clone().unwrap_or_default().into(),
        catalog_number: r.catalog_number.clone().unwrap_or_default().into(),
    }
}

/// Search the selected provider (MusicBrainz/Discogs) for the current album
/// title + artist. Generation-guarded so a slow reply can't clobber a newer one.
pub fn search_remote(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let Some(w) = weak.upgrade() else {
        return;
    };
    let s = w.global::<TagEditorState>();
    let title = s.get_album_title().trim().to_string();
    let artist = s.get_album_artist().trim().to_string();
    let provider = s.get_remote_provider_index();
    if title.is_empty() && artist.is_empty() {
        crate::toast::error_weak(&weak, "Enter a title or artist to search");
        return;
    }
    let gen = REMOTE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    s.set_remote_searching(true);

    handle.spawn(async move {
        let results: Result<Vec<crate::RemoteResultItem>, String> = if provider == 1 {
            let dc = qbz_integrations::DiscogsClient::new();
            dc.search_releases(&artist, &title, None, 12).await.map(|v| {
                v.iter()
                    .map(|r| map_search(&qbz_integrations::discogs_extended_to_search_result(r)))
                    .collect()
            })
        } else {
            let mb = qbz_integrations::MusicBrainzClient::new();
            mb.search_releases_extended(&title, &artist, None, 12)
                .await
                .map(|resp| {
                    resp.releases
                        .iter()
                        .map(|r| map_search(&qbz_integrations::musicbrainz_release_to_search_result(r)))
                        .collect()
                })
                .map_err(|e| e.to_string())
        };
        let _ = weak.upgrade_in_event_loop(move |w| {
            if REMOTE_GEN.load(Ordering::SeqCst) != gen {
                return;
            }
            let s = w.global::<TagEditorState>();
            s.set_remote_searching(false);
            s.set_has_searched(true);
            match results {
                Ok(items) => {
                    let empty = items.is_empty();
                    s.set_remote_results(ModelRc::new(VecModel::from(items)));
                    s.set_show_remote_panel(!empty);
                }
                Err(e) => {
                    s.set_show_remote_panel(false);
                    crate::toast::error(&w, format!("Search failed: {e}"));
                }
            }
        });
    });
}

/// Mark a result card selected.
pub fn select_result(weak: Weak<AppWindow>, provider_id: String) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        w.global::<TagEditorState>().set_selected_result_id(provider_id.into());
    });
}

/// Fetch the selected result's full metadata and apply it (album fields +
/// positional per-track titles). Generation-guarded.
pub fn apply_remote(weak: Weak<AppWindow>, handle: tokio::runtime::Handle) {
    let Some(w) = weak.upgrade() else {
        return;
    };
    let s = w.global::<TagEditorState>();
    let id = s.get_selected_result_id().to_string();
    if id.is_empty() {
        return;
    }
    let provider = s.get_remote_provider_index();
    let gen = REMOTE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    s.set_remote_loading(true);

    handle.spawn(async move {
        let meta: Result<qbz_integrations::RemoteAlbumMetadata, String> = if provider == 1 {
            match id.parse::<u64>() {
                Ok(rid) => {
                    let dc = qbz_integrations::DiscogsClient::new();
                    dc.get_release_metadata(rid)
                        .await
                        .map(|m| qbz_integrations::discogs_full_to_metadata(&m))
                }
                Err(_) => Err("Invalid Discogs release id".to_string()),
            }
        } else {
            let mb = qbz_integrations::MusicBrainzClient::new();
            mb.get_release_with_tracks(&id)
                .await
                .map(|r| qbz_integrations::musicbrainz_full_to_metadata(&r))
                .map_err(|e| e.to_string())
        };
        let _ = weak.upgrade_in_event_loop(move |w| {
            if REMOTE_GEN.load(Ordering::SeqCst) != gen {
                return;
            }
            let s = w.global::<TagEditorState>();
            s.set_remote_loading(false);
            match meta {
                Ok(m) => {
                    s.set_album_title(m.title.clone().into());
                    s.set_album_artist(m.artist.clone().into());
                    if let Some(y) = m.year {
                        s.set_year_input(y.to_string().into());
                    }
                    if let Some(g) = m.genres.first() {
                        s.set_genre(g.clone().into());
                    }
                    if let Some(c) = m.catalog_number.as_ref() {
                        s.set_catalog_number(c.clone().into());
                    }
                    if m.disc_count > 0 {
                        s.set_album_total_discs(m.disc_count as i32);
                    }
                    // Positional per-track title merge.
                    let model = s.get_tracks();
                    let local_n = model.row_count();
                    let n = local_n.min(m.tracks.len());
                    for i in 0..n {
                        if let Some(mut row) = model.row_data(i) {
                            row.title = m.tracks[i].title.clone().into();
                            model.set_row_data(i, row);
                        }
                    }
                    s.set_show_remote_panel(false);
                    let remote_n = m.tracks.len();
                    if remote_n > 0 && remote_n != local_n {
                        crate::toast::warning(
                            &w,
                            "Track count differs from the result; titles applied by position",
                        );
                    }
                }
                Err(e) => {
                    let lower = e.to_lowercase();
                    if lower.contains("429") || lower.contains("rate") {
                        crate::toast::error(&w, "Rate limited, try again shortly");
                    } else {
                        crate::toast::error(&w, "Failed to fetch metadata");
                    }
                }
            }
        });
    });
}

/// Open the selected result's provider page in the system browser.
pub fn open_in_browser(weak: Weak<AppWindow>) {
    let Some(w) = weak.upgrade() else {
        return;
    };
    let s = w.global::<TagEditorState>();
    let id = s.get_selected_result_id().to_string();
    if id.is_empty() {
        return;
    }
    let url = if s.get_remote_provider_index() == 1 {
        format!("https://www.discogs.com/release/{id}")
    } else {
        format!("https://musicbrainz.org/release/{id}")
    };
    let _ = open::that(url);
}

/// Re-open the local album view (re-splits versions with the new tags) and
/// reset the LocalLibrary browse models so the tabs re-fetch. Avoids a full
/// library reload (the 16K-track freeze).
fn refresh_after_save(weak: Weak<AppWindow>, handle: tokio::runtime::Handle, image_cache: crate::artwork::ImageCache) {
    let _ = weak.upgrade_in_event_loop(move |w| {
        // Refresh the open local album detail (if any) by its metadata key.
        let id = w.global::<crate::LocalAlbumState>().get_id().to_string();
        if !id.is_empty() {
            crate::local_library::open_local_album(w.as_weak(), handle.clone(), image_cache.clone(), id);
        }
        // Reset browse models so Albums/Folders/Tracks/Artists re-fetch.
        let s = w.global::<crate::LocalLibraryState>();
        let empty_albums = ModelRc::new(VecModel::from(Vec::<crate::AlbumCardItem>::new()));
        let empty_tracks = ModelRc::new(VecModel::from(Vec::<crate::TrackItem>::new()));
        s.set_albums(empty_albums.clone());
        s.set_folders(empty_albums);
        s.set_tracks(empty_tracks);
        s.set_artists(ModelRc::new(VecModel::from(Vec::<crate::LocalArtistItem>::new())));
    });
}
