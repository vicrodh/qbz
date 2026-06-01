use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackOrigin {
    QobuzOnline,
    QobuzOfflineCache,
    LocalLibrary,
    Plex,
    ExternalUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionDecision {
    pub accepted: bool,
    pub reason: &'static str,
}

impl AdmissionDecision {
    pub const fn allow(reason: &'static str) -> Self {
        Self {
            accepted: true,
            reason,
        }
    }

    pub const fn block(reason: &'static str) -> Self {
        Self {
            accepted: false,
            reason,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandoffIntent {
    ContinueLocally,
    SendToConnect,
}

pub fn evaluate_remote_queue_admission(origin: TrackOrigin) -> AdmissionDecision {
    match origin {
        TrackOrigin::QobuzOnline => AdmissionDecision::allow("qobuz_online_source"),
        TrackOrigin::QobuzOfflineCache => AdmissionDecision::allow("qobuz_offline_cache_source"),
        TrackOrigin::LocalLibrary => {
            AdmissionDecision::block("local_library_tracks_never_enter_remote_qconnect_queue")
        }
        TrackOrigin::Plex => {
            AdmissionDecision::block("plex_tracks_never_enter_remote_qconnect_queue")
        }
        TrackOrigin::ExternalUnknown => {
            AdmissionDecision::block("unknown_origin_blocked_for_remote_qconnect_queue")
        }
    }
}

pub fn resolve_handoff_intent(origin: TrackOrigin) -> HandoffIntent {
    match origin {
        TrackOrigin::QobuzOnline | TrackOrigin::QobuzOfflineCache => HandoffIntent::SendToConnect,
        TrackOrigin::LocalLibrary | TrackOrigin::Plex | TrackOrigin::ExternalUnknown => {
            HandoffIntent::ContinueLocally
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qbz_models::PlaybackSource;

    #[test]
    fn admission_matches_playback_source_predicate() {
        let pairs = [
            (TrackOrigin::QobuzOnline, PlaybackSource::Qobuz),
            (TrackOrigin::LocalLibrary, PlaybackSource::Local),
            (TrackOrigin::Plex, PlaybackSource::Plex),
        ];
        for (origin, source) in pairs {
            assert_eq!(
                evaluate_remote_queue_admission(origin).accepted,
                source.is_qobuz_streamable(),
                "admission/predicate disagree for {origin:?}",
            );
        }
    }
}
