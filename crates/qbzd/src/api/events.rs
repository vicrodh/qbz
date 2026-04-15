//! SSE (Server-Sent Events) endpoint for real-time event streaming.
//!
//! Clients connect to `/api/events` and receive a continuous stream of
//! DaemonEvents formatted as SSE. Slow clients that fall behind the
//! broadcast buffer are disconnected (SSE auto-reconnect handles this).

use std::convert::Infallible;
use std::sync::Arc;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::adapter::{DaemonEvent, PlaybackSnapshot, RuntimeEvent};
use crate::daemon::DaemonCore;

/// SSE endpoint handler. Each connected client gets its own broadcast receiver.
pub async fn sse_handler(
    daemon: Arc<DaemonCore>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = daemon.event_bus.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => Some(Ok(format_sse_event(&event))),
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
            log::warn!("[SSE] Client lagged, dropped {} events", n);
            Some(Ok(Event::default().comment(format!("lagged:{}", n))))
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Convert a DaemonEvent to an SSE Event with typed event name.
fn format_sse_event(event: &DaemonEvent) -> Event {
    match event {
        DaemonEvent::Core(core_event) => {
            let (event_type, data) = format_core_event(core_event);
            Event::default().event(event_type).data(data)
        }
        DaemonEvent::Playback(snapshot) => Event::default()
            .event("playback")
            .data(serde_json::to_string(snapshot).unwrap_or_default()),
        DaemonEvent::Runtime(runtime_event) => Event::default()
            .event("runtime")
            .data(serde_json::to_string(runtime_event).unwrap_or_default()),
    }
}

/// Map CoreEvent variants to SSE event type + JSON data.
fn format_core_event(event: &qbz_models::CoreEvent) -> (&'static str, String) {
    use qbz_models::CoreEvent;
    match event {
        CoreEvent::TrackStarted { track, .. } => (
            "track-started",
            serde_json::to_string(track).unwrap_or_default(),
        ),
        CoreEvent::TrackEnded { track_id } => (
            "track-ended",
            serde_json::json!({"track_id": track_id}).to_string(),
        ),
        CoreEvent::QueueUpdated { state } => (
            "queue",
            serde_json::to_string(state).unwrap_or_default(),
        ),
        CoreEvent::FavoritesUpdated { favorite_type } => (
            "favorites-updated",
            serde_json::json!({"type": favorite_type}).to_string(),
        ),
        CoreEvent::PlaylistCreated { playlist } => (
            "playlist-created",
            serde_json::to_string(playlist).unwrap_or_default(),
        ),
        CoreEvent::PlaylistUpdated { playlist_id } => (
            "playlist-updated",
            serde_json::json!({"playlist_id": playlist_id}).to_string(),
        ),
        CoreEvent::Error {
            code,
            message,
            recoverable,
        } => (
            "error",
            serde_json::json!({"code": code, "message": message, "recoverable": recoverable})
                .to_string(),
        ),
        CoreEvent::LoggedIn { session } => (
            "logged-in",
            serde_json::json!({"user_id": session.user_id, "display_name": session.display_name})
                .to_string(),
        ),
        CoreEvent::LoggedOut => ("logged-out", "{}".to_string()),
        // Catch-all for any future CoreEvent variants
        _ => ("core-event", format!("{:?}", event)),
    }
}
