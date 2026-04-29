//! Tauri command layer for Mixtapes & Collections CRUD.
//!
//! These 12 commands wrap `mixtape::repo` functions with the standard
//! LibraryState access pattern used by the rest of commands_v2.

use tauri::State;

use crate::core_bridge::CoreBridgeState;
use crate::library::LibraryState;
use crate::runtime::{CommandRequirement, RuntimeError, RuntimeManagerState};

// ──────────────────────────── String → enum helpers ────────────────────────────
// These mirror the private helpers in mixtape::repo. Duplicated here to avoid
// making the repo helpers pub; they're small enough that duplication is fine.

fn parse_kind(s: &str) -> qbz_models::mixtape::CollectionKind {
    use qbz_models::mixtape::CollectionKind;
    match s {
        "mixtape" => CollectionKind::Mixtape,
        "artist_collection" => CollectionKind::ArtistCollection,
        _ => CollectionKind::Collection,
    }
}

fn parse_source_type(s: &str) -> qbz_models::mixtape::CollectionSourceType {
    use qbz_models::mixtape::CollectionSourceType;
    match s {
        "artist_discography" => CollectionSourceType::ArtistDiscography,
        _ => CollectionSourceType::Manual,
    }
}

fn parse_play_mode(s: &str) -> qbz_models::mixtape::CollectionPlayMode {
    use qbz_models::mixtape::CollectionPlayMode;
    match s {
        "album_shuffle" => CollectionPlayMode::AlbumShuffle,
        _ => CollectionPlayMode::InOrder,
    }
}

fn parse_item_type(s: &str) -> qbz_models::mixtape::ItemType {
    use qbz_models::mixtape::ItemType;
    match s {
        "track" => ItemType::Track,
        "playlist" => ItemType::Playlist,
        _ => ItemType::Album,
    }
}

fn parse_source(s: &str) -> qbz_models::mixtape::AlbumSource {
    use qbz_models::mixtape::AlbumSource;
    match s {
        "local" => AlbumSource::Local,
        _ => AlbumSource::Qobuz,
    }
}

// ──────────────────────────── Helper macro ────────────────────────────

/// Acquire a shared `&LibraryDatabase` from LibraryState or return
/// `RuntimeError::UserSessionNotActivated`.
macro_rules! acquire_db {
    ($library:expr) => {{
        let guard__ = $library.db.lock().await;
        // SAFETY: guard__ must outlive the reference we hand back.
        // We move the guard into a let-binding that lives for the rest of
        // the enclosing block.
        guard__
    }};
}

// ──────────────────────────── Collection commands ────────────────────────────

/// List all mixtape collections, optionally filtered by kind.
///
/// `kind`: `"mixtape"` | `"collection"` | `"artist_collection"` | `null`
#[tauri::command]
pub async fn v2_list_mixtape_collections(
    kind: Option<String>,
    library: State<'_, LibraryState>,
) -> Result<Vec<qbz_models::mixtape::MixtapeCollection>, RuntimeError> {
    log::debug!("[V2] list_mixtape_collections kind={:?}", kind);
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        let k = kind.as_deref().map(parse_kind);
        crate::mixtape::repo::list_collections(conn, k)
            .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

/// Get a single mixtape collection (with its items) by ID.
#[tauri::command]
pub async fn v2_get_mixtape_collection(
    id: String,
    library: State<'_, LibraryState>,
) -> Result<Option<qbz_models::mixtape::MixtapeCollection>, RuntimeError> {
    log::debug!("[V2] get_mixtape_collection id={}", id);
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        crate::mixtape::repo::get_collection(conn, &id)
            .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

/// Create a new mixtape collection.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_create_mixtape_collection(
    kind: String,
    name: String,
    description: Option<String>,
    source_type: Option<String>,
    source_ref: Option<String>,
    library: State<'_, LibraryState>,
) -> Result<qbz_models::mixtape::MixtapeCollection, RuntimeError> {
    log::debug!("[V2] create_mixtape_collection kind={} name={}", kind, name);
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        let k = parse_kind(&kind);
        let st = source_type.as_deref().map(parse_source_type)
            .unwrap_or(qbz_models::mixtape::CollectionSourceType::Manual);
        crate::mixtape::repo::create_collection(
            conn,
            k,
            &name,
            description.as_deref(),
            st,
            source_ref.as_deref(),
        )
        .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

/// Rename a mixtape collection.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_rename_mixtape_collection(
    id: String,
    new_name: String,
    library: State<'_, LibraryState>,
) -> Result<(), RuntimeError> {
    log::debug!("[V2] rename_mixtape_collection id={}", id);
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        crate::mixtape::repo::rename_collection(conn, &id, &new_name)
            .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

/// Set the description of a mixtape collection (pass `null` to clear it).
#[tauri::command]
pub async fn v2_set_mixtape_description(
    id: String,
    description: Option<String>,
    library: State<'_, LibraryState>,
) -> Result<(), RuntimeError> {
    log::debug!("[V2] set_mixtape_description id={}", id);
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        crate::mixtape::repo::set_description(conn, &id, description.as_deref())
            .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

/// Set the play mode of a mixtape collection.
///
/// `mode`: `"in_order"` | `"album_shuffle"`
#[tauri::command]
pub async fn v2_set_mixtape_play_mode(
    id: String,
    mode: String,
    library: State<'_, LibraryState>,
) -> Result<(), RuntimeError> {
    log::debug!("[V2] set_mixtape_play_mode id={} mode={}", id, mode);
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        crate::mixtape::repo::set_play_mode(conn, &id, parse_play_mode(&mode))
            .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

/// Change the kind of a mixtape collection between `"mixtape"` and `"collection"`.
///
/// Converting to/from `"artist_collection"` is rejected by the repository.
#[tauri::command]
pub async fn v2_set_mixtape_kind(
    id: String,
    kind: String,
    library: State<'_, LibraryState>,
) -> Result<(), RuntimeError> {
    log::debug!("[V2] set_mixtape_kind id={} kind={}", id, kind);
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        crate::mixtape::repo::set_kind(conn, &id, parse_kind(&kind))
            .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

/// Set (or clear) the custom artwork path for a mixtape collection.
#[tauri::command]
pub async fn v2_set_mixtape_custom_artwork(
    id: String,
    path: Option<String>,
    library: State<'_, LibraryState>,
) -> Result<(), RuntimeError> {
    log::debug!("[V2] set_mixtape_custom_artwork id={}", id);
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        crate::mixtape::repo::set_custom_artwork(conn, &id, path.as_deref())
            .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

/// Upload a user-picked image to become a mixtape / collection's custom
/// cover. Mirrors v2_library_set_custom_album_cover: copies source → artwork
/// cache, resizes to 1000×1000, then stamps the resulting path into
/// mixtape_collections.custom_artwork_path. Returns the stored path so the
/// frontend can `convertFileSrc` it immediately.
#[tauri::command]
pub async fn v2_mixtape_upload_custom_cover(
    id: String,
    source_path: String,
    library: State<'_, LibraryState>,
) -> Result<String, RuntimeError> {
    log::debug!("[V2] mixtape_upload_custom_cover id={}", id);

    let source = std::path::Path::new(&source_path);
    if !source.exists() {
        return Err(RuntimeError::Internal(format!(
            "Source image does not exist: {}",
            source_path
        )));
    }
    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if !["png", "jpg", "jpeg", "webp"].contains(&extension.as_str()) {
        return Err(RuntimeError::Internal(format!(
            "Unsupported image format: {}. Use png, jpg, jpeg, or webp.",
            extension
        )));
    }

    // Clean up previous custom cover file if one existed.
    let previous_path: Option<String> = {
        let guard = acquire_db!(library);
        let db = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        db.with_connection(|conn| {
            crate::mixtape::repo::get_custom_artwork(conn, &id)
                .map_err(|e| RuntimeError::Internal(e.to_string()))
        })?
    };

    let artwork_dir = qbz_library::get_artwork_cache_dir();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let safe_id = id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>();
    let filename = format!("mixtape_custom_{}_{}.jpg", safe_id, timestamp);
    let dest_path = artwork_dir.join(&filename);

    let img = image::ImageReader::open(source)
        .map_err(|e| RuntimeError::Internal(format!("Failed to open image: {}", e)))?
        .decode()
        .map_err(|e| RuntimeError::Internal(format!("Failed to decode image: {}", e)))?;
    let resized = img.resize(1000, 1000, image::imageops::FilterType::Lanczos3);
    resized
        .save(&dest_path)
        .map_err(|e| RuntimeError::Internal(format!("Failed to save resized image: {}", e)))?;

    let dest_str = dest_path.to_string_lossy().into_owned();
    {
        let guard = acquire_db!(library);
        let db = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        db.with_connection(|conn| {
            crate::mixtape::repo::set_custom_artwork(conn, &id, Some(&dest_str))
                .map_err(|e| RuntimeError::Internal(e.to_string()))
        })?;
    }

    // Delete the previous file AFTER the new path is persisted, so a failure
    // above leaves the previous cover intact.
    if let Some(prev) = previous_path {
        if prev != dest_str {
            let _ = std::fs::remove_file(&prev);
        }
    }

    Ok(dest_str)
}

/// Clear a mixtape / collection's custom cover: delete the file on disk
/// and null out the DB column.
#[tauri::command]
pub async fn v2_mixtape_remove_custom_cover(
    id: String,
    library: State<'_, LibraryState>,
) -> Result<(), RuntimeError> {
    log::debug!("[V2] mixtape_remove_custom_cover id={}", id);

    let previous_path: Option<String> = {
        let guard = acquire_db!(library);
        let db = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        db.with_connection(|conn| {
            crate::mixtape::repo::get_custom_artwork(conn, &id)
                .map_err(|e| RuntimeError::Internal(e.to_string()))
        })?
    };

    {
        let guard = acquire_db!(library);
        let db = guard
            .as_ref()
            .ok_or(RuntimeError::UserSessionNotActivated)?;
        db.with_connection(|conn| {
            crate::mixtape::repo::set_custom_artwork(conn, &id, None)
                .map_err(|e| RuntimeError::Internal(e.to_string()))
        })?;
    }

    if let Some(prev) = previous_path {
        let _ = std::fs::remove_file(&prev);
    }

    Ok(())
}

/// Delete a mixtape collection and all its items (CASCADE).
#[tauri::command]
pub async fn v2_delete_mixtape_collection(
    id: String,
    library: State<'_, LibraryState>,
) -> Result<(), RuntimeError> {
    log::debug!("[V2] delete_mixtape_collection id={}", id);
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        crate::mixtape::repo::delete_collection(conn, &id)
            .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

// ──────────────────────────── Item commands ────────────────────────────

/// Add an item to a mixtape collection.
///
/// Returns `true` if the item was inserted, `false` if it was a duplicate
/// (same `source` + `sourceItemId` already exists in the collection).
#[tauri::command]
#[allow(non_snake_case, clippy::too_many_arguments)]
pub async fn v2_add_mixtape_item(
    collection_id: String,
    item_type: String,
    source: String,
    source_item_id: String,
    title: String,
    subtitle: Option<String>,
    artwork_url: Option<String>,
    year: Option<i32>,
    track_count: Option<i32>,
    allow_duplicate: Option<bool>,
    library: State<'_, LibraryState>,
) -> Result<bool, RuntimeError> {
    log::debug!(
        "[V2] add_mixtape_item collection_id={} source_item_id={} allow_duplicate={:?}",
        collection_id,
        source_item_id,
        allow_duplicate
    );
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        crate::mixtape::repo::add_item_with(
            conn,
            &collection_id,
            parse_item_type(&item_type),
            parse_source(&source),
            &source_item_id,
            &title,
            subtitle.as_deref(),
            artwork_url.as_deref(),
            year,
            track_count,
            allow_duplicate.unwrap_or(false),
        )
        .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

/// Check whether `(collection_id, source, source_item_id)` already has an
/// item inside the given collection. Lets the frontend ask "are any of
/// these items already in there?" before bulk-adding and show a
/// confirmation dialog.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_mixtape_item_exists(
    collection_id: String,
    source: String,
    source_item_id: String,
    library: State<'_, LibraryState>,
) -> Result<bool, RuntimeError> {
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        crate::mixtape::repo::item_exists(
            conn,
            &collection_id,
            parse_source(&source),
            &source_item_id,
        )
        .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

/// Remove the item at `position` from a mixtape collection.
///
/// Positions above the removed index are compacted automatically.
#[tauri::command]
pub async fn v2_remove_mixtape_item(
    collection_id: String,
    position: i32,
    library: State<'_, LibraryState>,
) -> Result<(), RuntimeError> {
    log::debug!(
        "[V2] remove_mixtape_item collection_id={} position={}",
        collection_id,
        position
    );
    let guard = acquire_db!(library);
    let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection(|conn| {
        crate::mixtape::repo::remove_item(conn, &collection_id, position)
            .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

/// Reorder the items in a mixtape collection.
///
/// `new_order` is a permutation of the current positions (0..N). For example,
/// `[2, 0, 1]` means: slot 0 ← old item 2, slot 1 ← old item 0, etc.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_reorder_mixtape_items(
    collection_id: String,
    new_order: Vec<i32>,
    library: State<'_, LibraryState>,
) -> Result<(), RuntimeError> {
    log::debug!(
        "[V2] reorder_mixtape_items collection_id={} len={}",
        collection_id,
        new_order.len()
    );
    let mut guard = library.db.lock().await;
    let db = guard.as_mut().ok_or(RuntimeError::UserSessionNotActivated)?;
    db.with_connection_mut(|conn| {
        crate::mixtape::repo::reorder_items(conn, &collection_id, &new_order)
            .map_err(|e| RuntimeError::Internal(e.to_string()))
    })
}

// ──────────────────────────── Enqueue command ────────────────────────────

/// Resolve and enqueue all tracks in a MixtapeCollection.
///
/// `mode`: `"replace"` | `"append"` | `"play_next"` (default: `"append"`)
///
/// On `"replace"` the queue is cleared and the first track starts playing
/// immediately. On `"append"` the tracks are appended to the end of the queue.
/// On `"play_next"` they are inserted after the current track (in order).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn v2_enqueue_collection(
    collection_id: String,
    mode: String,
    library: State<'_, LibraryState>,
    state: State<'_, crate::AppState>,
    bridge: State<'_, crate::core_bridge::CoreBridgeState>,
    runtime: State<'_, crate::runtime::RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    use crate::mixtape::enqueue::{ProdItemResolver, resolve_collection_tracks};
    use crate::runtime::CommandRequirement;

    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    log::info!(
        "[V2] enqueue_collection id={} mode={}",
        collection_id,
        mode
    );

    // 1. Load the collection from the repo.
    let collection = {
        let guard = acquire_db!(library);
        let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
        db.with_connection(|conn| {
            crate::mixtape::repo::get_collection(conn, &collection_id)
                .map_err(|e| RuntimeError::Internal(e.to_string()))
        })?
        .ok_or_else(|| RuntimeError::Internal("collection not found".into()))?
    };

    // 2. Build the resolver from the shared Qobuz client + library state.
    let client_guard = state.client.read().await;
    let client = client_guard.clone();
    drop(client_guard);

    let resolver = ProdItemResolver {
        client: &client,
        library: &library,
    };

    // 3. Resolve items → Vec<CoreQueueTrack>.
    let tracks = resolve_collection_tracks(
        collection.items.clone(),
        collection.play_mode,
        &resolver,
    )
    .await;

    if tracks.is_empty() {
        return Err(RuntimeError::Internal(
            "collection resolved to 0 playable tracks".into(),
        ));
    }

    log::info!(
        "[V2] enqueue_collection id={}: {} tracks resolved, applying mode={}",
        collection_id,
        tracks.len(),
        mode
    );

    // 4. Apply to the queue via CoreBridge.
    let bridge = bridge.get().await;
    match mode.as_str() {
        "replace" => {
            bridge.set_queue(tracks, Some(0)).await;
            bridge.play_index(0).await;
        }
        "play_next" => {
            // Insert in reverse so the first track ends up immediately after current.
            for track in tracks.into_iter().rev() {
                bridge.add_track_next(track).await;
            }
        }
        _ => {
            // Default: append to end.
            bridge.add_tracks(tracks).await;
        }
    }

    // 5. Stamp queue_source_collection_id on RuntimeManager.
    // Only set when the queue is replaced (replace mode). Append/play_next ops
    // preserve whatever context was already set.
    if mode.as_str() == "replace" {
        runtime
            .manager()
            .set_queue_source_collection(Some(collection_id.clone()))
            .await;
    }

    // 6. Bump play stats (best-effort).
    {
        let guard = acquire_db!(library);
        if let Some(db) = guard.as_ref() {
            let _ = db.with_connection(|conn| {
                crate::mixtape::repo::touch_play(conn, &collection_id)
            });
        }
    }

    Ok(())
}

/// Enqueue a single Collection/Mixtape item (by position) through the same
/// ProdItemResolver used by v2_enqueue_collection. Lets the per-row Play /
/// Play-next / Queue-later buttons in the detail view work for local and
/// plex items too — the frontend can't resolve those itself without duplicating
/// the local_tracks + plex_cache lookup logic. `mode` matches v2_enqueue_collection:
/// "replace" (stop + set queue + play from 0), "play_next", or default/append.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
#[allow(non_snake_case)]
pub async fn v2_enqueue_collection_item(
    collectionId: String,
    position: usize,
    mode: String,
    library: State<'_, LibraryState>,
    state: State<'_, crate::AppState>,
    bridge: State<'_, crate::core_bridge::CoreBridgeState>,
    runtime: State<'_, crate::runtime::RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    use crate::mixtape::enqueue::{ItemResolver, ProdItemResolver};
    use crate::runtime::CommandRequirement;

    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    log::info!(
        "[V2] enqueue_collection_item id={} position={} mode={}",
        collectionId,
        position,
        mode
    );

    let collection = {
        let guard = acquire_db!(library);
        let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
        db.with_connection(|conn| {
            crate::mixtape::repo::get_collection(conn, &collectionId)
                .map_err(|e| RuntimeError::Internal(e.to_string()))
        })?
        .ok_or_else(|| RuntimeError::Internal("collection not found".into()))?
    };

    let item = collection
        .items
        .iter()
        .find(|it| it.position as usize == position)
        .cloned()
        .ok_or_else(|| {
            RuntimeError::Internal(format!(
                "item at position {} not found in collection {}",
                position, collectionId
            ))
        })?;

    let client_guard = state.client.read().await;
    let client = client_guard.clone();
    drop(client_guard);

    let resolver = ProdItemResolver {
        client: &client,
        library: &library,
    };

    let mut tracks = resolver
        .resolve(&item)
        .await
        .map_err(|e| RuntimeError::Internal(format!("item resolve failed: {}", e)))?;

    // Same source_item_id_hint stamp the whole-collection path applies, so
    // item-boundary skip commands still recognize the item.
    let hint = item.source_item_id.clone();
    for track in &mut tracks {
        track.source_item_id_hint = Some(hint.clone());
    }

    if tracks.is_empty() {
        return Err(RuntimeError::Internal(
            "item resolved to 0 playable tracks".into(),
        ));
    }

    let bridge = bridge.get().await;
    match mode.as_str() {
        "replace" => {
            bridge.set_queue(tracks, Some(0)).await;
            bridge.play_index(0).await;
        }
        "play_next" => {
            for track in tracks.into_iter().rev() {
                bridge.add_track_next(track).await;
            }
        }
        _ => {
            bridge.add_tracks(tracks).await;
        }
    }

    Ok(())
}

// ──────────────────────────── Item-level skip commands ────────────────────────────

/// Skip to the first track of the next collection item.
///
/// Uses `next_item_index` to locate the item boundary after the current track.
/// If already at the last item, no action is taken (same no-op as reaching the
/// end of the queue via the standard next-track button).
#[tauri::command]
pub async fn v2_skip_to_next_item(
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    log::info!("[V2] skip_to_next_item");

    let bridge = bridge.get().await;

    // Snapshot queue + current index in one atomic call.
    let (queue, current_opt) = bridge.get_all_queue_tracks().await;
    let current = current_opt.unwrap_or(0);

    match crate::mixtape::enqueue::next_item_index(&queue, current) {
        Some(target) => {
            log::info!(
                "[V2] skip_to_next_item: current={} → target={}",
                current,
                target
            );
            bridge
                .play_index(target)
                .await
                .ok_or_else(|| RuntimeError::Internal("play_index returned None".into()))?;
        }
        None => {
            // End of queue — match the behavior of v2_next_track when there is
            // no next track: do nothing (the player stays on the last track).
            log::info!("[V2] skip_to_next_item: already at last item, no-op");
        }
    }

    Ok(())
}

/// Go back to the start of the current collection item, or — if playback is
/// near the beginning of the item — jump to the start of the previous item.
///
/// Uses the same 3-second threshold as `previous_item_index`:
/// - elapsed ≤ 3 s AND we are at item-start → jump to previous item
/// - elapsed > 3 s OR we are mid-item → restart current item
/// - If already at the very first item and eligible for "previous", stay put.
#[tauri::command]
pub async fn v2_skip_to_previous_item(
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    log::info!("[V2] skip_to_previous_item");

    let bridge = bridge.get().await;

    // Snapshot queue + current index.
    let (queue, current_opt) = bridge.get_all_queue_tracks().await;
    let current = current_opt.unwrap_or(0);

    // Current position from the player (seconds); convert to ms for the helper.
    let elapsed_ms = bridge.player().state.current_position() * 1_000;

    if let Some(target) =
        crate::mixtape::enqueue::previous_item_index(&queue, current, elapsed_ms)
    {
        log::info!(
            "[V2] skip_to_previous_item: current={} elapsed_ms={} → target={}",
            current,
            elapsed_ms,
            target
        );
        bridge
            .play_index(target)
            .await
            .ok_or_else(|| RuntimeError::Internal("play_index returned None".into()))?;
    } else {
        // Queue is empty or index is out of bounds — stay put.
        log::info!("[V2] skip_to_previous_item: queue empty or no target, no-op");
    }

    Ok(())
}

// ──────────────────────────── Track-mix shuffle ────────────────────────────

/// Returns the number of distinct songs in the collection after similarity-
/// based deduplication. Used by the DJ-mix modal to size the slider of pickable
/// queue sizes. Deterministic — same collection state yields the same count.
#[tauri::command]
pub async fn v2_collection_unique_track_count(
    collection_id: String,
    library: State<'_, LibraryState>,
    state: State<'_, crate::AppState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<usize, RuntimeError> {
    use crate::mixtape::enqueue::{ProdItemResolver, resolve_collection_tracks};
    use crate::mixtape::shuffle;

    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    log::info!("[V2] collection_unique_track_count id={}", collection_id);

    // 1. Load the collection.
    let collection = {
        let guard = acquire_db!(library);
        let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
        db.with_connection(|conn| {
            crate::mixtape::repo::get_collection(conn, &collection_id)
                .map_err(|e| RuntimeError::Internal(e.to_string()))
        })?
        .ok_or_else(|| RuntimeError::Internal("collection not found".into()))?
    };

    // 2. Resolve the natural in-order track list (we count, not play).
    let client_guard = state.client.read().await;
    let client = client_guard.clone();
    drop(client_guard);

    let resolver = ProdItemResolver {
        client: &client,
        library: &library,
    };

    let tracks = resolve_collection_tracks(
        collection.items.clone(),
        qbz_models::mixtape::CollectionPlayMode::InOrder,
        &resolver,
    )
    .await;

    let count = shuffle::unique_track_count(&tracks);
    log::info!(
        "[V2] collection_unique_track_count id={}: total={} unique={}",
        collection_id,
        tracks.len(),
        count
    );
    Ok(count)
}

/// Result of [`v2_collection_shuffle_tracks`]: how many tracks the user asked
/// for and how many actually ended up in the queue. The two can differ when
/// dedup or the per-album cap shrinks the available pool.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShuffleTracksResult {
    pub requested_count: usize,
    pub actual_count: usize,
}

/// Replace the queue with a randomly sampled mix of `sample_size` tracks drawn
/// from the collection's natural track list, after similarity-based dedup and
/// with a per-album cap. Starts playback from index 0.
///
/// Mirrors the side effects of `v2_enqueue_collection` in `replace` mode:
/// stamps `queue_source_collection` and bumps the play counter. Does NOT
/// modify the collection's persistent `play_mode` — this is a one-off action.
#[tauri::command]
pub async fn v2_collection_shuffle_tracks(
    collection_id: String,
    sample_size: usize,
    library: State<'_, LibraryState>,
    state: State<'_, crate::AppState>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<ShuffleTracksResult, RuntimeError> {
    use crate::mixtape::enqueue::{ProdItemResolver, resolve_collection_tracks};
    use crate::mixtape::shuffle;

    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    log::info!(
        "[V2] collection_shuffle_tracks id={} sample_size={}",
        collection_id,
        sample_size
    );

    if sample_size == 0 {
        return Err(RuntimeError::Internal(
            "sample_size must be greater than 0".into(),
        ));
    }

    // 1. Load the collection.
    let collection = {
        let guard = acquire_db!(library);
        let db = guard.as_ref().ok_or(RuntimeError::UserSessionNotActivated)?;
        db.with_connection(|conn| {
            crate::mixtape::repo::get_collection(conn, &collection_id)
                .map_err(|e| RuntimeError::Internal(e.to_string()))
        })?
        .ok_or_else(|| RuntimeError::Internal("collection not found".into()))?
    };

    // 2. Resolve the natural in-order track list.
    let client_guard = state.client.read().await;
    let client = client_guard.clone();
    drop(client_guard);

    let resolver = ProdItemResolver {
        client: &client,
        library: &library,
    };

    let tracks = resolve_collection_tracks(
        collection.items.clone(),
        qbz_models::mixtape::CollectionPlayMode::InOrder,
        &resolver,
    )
    .await;

    if tracks.is_empty() {
        return Err(RuntimeError::Internal(
            "collection resolved to 0 playable tracks".into(),
        ));
    }

    // 3. Dedup + sample. ThreadRng is not Send, so confine it to a sync
    //    scope that ends before any subsequent .await.
    let sampled = {
        let mut rng = rand::rng();
        let unique = shuffle::dedup_by_similarity(tracks, &mut rng);
        shuffle::hybrid_sample(unique, sample_size, &mut rng)
    };

    if sampled.is_empty() {
        return Err(RuntimeError::Internal(
            "sample produced 0 tracks".into(),
        ));
    }

    let actual = sampled.len();
    log::info!(
        "[V2] collection_shuffle_tracks id={}: requested={} actual={}",
        collection_id,
        sample_size,
        actual
    );

    // 4. Replace the queue and start playback from index 0.
    let bridge = bridge.get().await;
    bridge.set_queue(sampled, Some(0)).await;
    bridge.play_index(0).await;

    // 5. Stamp queue source so skip-next/prev know the boundary semantics.
    runtime
        .manager()
        .set_queue_source_collection(Some(collection_id.clone()))
        .await;

    // 6. Bump play stats (best-effort).
    {
        let guard = acquire_db!(library);
        if let Some(db) = guard.as_ref() {
            let _ = db.with_connection(|conn| {
                crate::mixtape::repo::touch_play(conn, &collection_id)
            });
        }
    }

    Ok(ShuffleTracksResult {
        requested_count: sample_size,
        actual_count: actual,
    })
}
