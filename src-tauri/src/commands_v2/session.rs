use tauri::State;

use crate::runtime::RuntimeError;

/// Save session position (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_save_session_position(
    positionSecs: u64,
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .save_position(positionSecs)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Save session volume (V2)
#[tauri::command]
pub async fn v2_save_session_volume(
    volume: f32,
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .save_volume(volume)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Save session playback mode (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_save_session_playback_mode(
    shuffle: bool,
    repeatMode: String,
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .save_playback_mode(shuffle, &repeatMode)
        .map_err(|e| RuntimeError::Internal(e))
}

/// Save session state - full state (V2)
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_save_session_state(
    queueTracks: Vec<crate::session_store::PersistedQueueTrack>,
    currentIndex: Option<usize>,
    currentPositionSecs: u64,
    volume: f32,
    shuffleEnabled: bool,
    repeatMode: String,
    wasPlaying: bool,
    lastView: Option<String>,
    viewContextId: Option<String>,
    viewContextType: Option<String>,
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let session = crate::session_store::PersistedSession {
        queue_tracks: queueTracks,
        current_index: currentIndex,
        current_position_secs: currentPositionSecs,
        volume,
        shuffle_enabled: shuffleEnabled,
        repeat_mode: repeatMode,
        was_playing: wasPlaying,
        saved_at: 0, // Will be set by save_session
        last_view: lastView.unwrap_or_else(|| "home".to_string()),
        view_context_id: viewContextId,
        view_context_type: viewContextType,
    };

    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .save_session(&session)
        .map_err(|e| RuntimeError::Internal(e))?;
    log::debug!(
        "[V2] save_session_state: index={:?} pos={}",
        currentIndex,
        currentPositionSecs
    );
    Ok(())
}

/// Load session state (V2)
#[tauri::command]
pub async fn v2_load_session_state(
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<crate::session_store::PersistedSession, RuntimeError> {
    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store.load_session().map_err(|e| RuntimeError::Internal(e))
}

/// Clear session (V2)
#[tauri::command]
pub async fn v2_clear_session(
    session_state: State<'_, crate::session_store::SessionStoreState>,
) -> Result<(), RuntimeError> {
    let guard = session_state
        .store
        .lock()
        .map_err(|e| RuntimeError::Internal(format!("Lock error: {}", e)))?;
    let store = guard
        .as_ref()
        .ok_or_else(|| RuntimeError::Internal("No active session".to_string()))?;
    store
        .clear_session()
        .map_err(|e| RuntimeError::Internal(e))?;
    log::info!("[V2] clear_session");
    Ok(())
}
