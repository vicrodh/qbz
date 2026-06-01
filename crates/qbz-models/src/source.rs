//! Source-aware playback types.
//!
//! Playable tracks reach the queue from multiple origins: Qobuz streaming,
//! the offline cache (downloaded Qobuz), local files, and Plex. These types
//! let every frontend reason about a track's origin and resolve its cover
//! art uniformly, instead of branching on stringly-typed `source` values at
//! each call site.
//!
//! This is the frontend-agnostic contract behind the source-aware playback
//! context: the now-playing bar, the queue, and the artwork pipeline consume
//! `PlaybackSource` + [`ArtworkRef`] and never special-case a source themselves.
//! The same contract drives the Qobuz Connect queue gate (only castable tracks
//! may be cast — see [`PlaybackSource::is_castable_to_qconnect`]).

use serde::{Deserialize, Serialize};

use crate::playback::QueueTrack;

/// Where a playable track comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackSource {
    /// The Qobuz streaming catalog.
    Qobuz,
    /// A Qobuz track downloaded into the offline cache (`qobuz_download`).
    OfflineCache,
    /// A local file indexed in the user's library.
    Local,
    /// A track served by a Plex Media Server.
    Plex,
}

impl PlaybackSource {
    /// Parse the stringly-typed `source` value carried by `QueueTrack` /
    /// `LocalTrack`. Unknown or absent values default to [`Qobuz`] — every
    /// pre-existing queue track was Qobuz, so this preserves history.
    ///
    /// [`Qobuz`]: PlaybackSource::Qobuz
    pub fn from_source_str(s: Option<&str>) -> Self {
        match s {
            Some("local") => Self::Local,
            Some("plex") => Self::Plex,
            Some("qobuz_download") => Self::OfflineCache,
            _ => Self::Qobuz,
        }
    }

    /// The canonical string written to `source` fields.
    pub fn as_source_str(self) -> &'static str {
        match self {
            Self::Qobuz => "qobuz",
            Self::OfflineCache => "qobuz_download",
            Self::Local => "local",
            Self::Plex => "plex",
        }
    }

    /// Whether this source streams live from the Qobuz catalog. NOTE: NOT the
    /// cast gate — offline-cache also carries a valid Qobuz id and IS castable.
    /// Use is_castable_to_qconnect for the Qobuz Connect gate.
    pub fn is_qobuz_streamable(self) -> bool {
        matches!(self, Self::Qobuz)
    }

    /// The admission-side cast predicate. Offline-cache maps to castable (the
    /// offline copy carries a valid Qobuz track id). This is the method the
    /// QConnect gate consults; is_qobuz_streamable stays "streams live from Qobuz".
    ///
    /// Shared QConnect-admission gate primitive: this is the single predicate
    /// both the Tauri and the upcoming Slint frontends call to gate casting.
    pub fn is_castable_to_qconnect(self) -> bool {
        matches!(self, Self::Qobuz | Self::OfflineCache)
    }
}

/// Admission-only origin tag. Unlike PlaybackSource, this has ExternalUnknown
/// so the Qobuz Connect gate can default unknown/absent to *blocked* not *Qobuz*.
///
/// Shared QConnect-admission gate primitive consumed by the Slint port (its
/// strict-parse companion for the cast gate); kept intentionally for that use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackOriginTag {
    Qobuz,
    OfflineCache,
    Local,
    Plex,
    ExternalUnknown,
}

impl PlaybackSource {
    /// Strict parse for the admission path: unknown/absent → ExternalUnknown (blocked).
    ///
    /// Shared QConnect-admission gate primitive consumed by the Slint port (it
    /// feeds the cast gate, where unknown origins must block, not default to Qobuz).
    pub fn from_source_str_strict(s: Option<&str>) -> TrackOriginTag {
        match s {
            Some("qobuz") => TrackOriginTag::Qobuz,
            Some("qobuz_download") => TrackOriginTag::OfflineCache,
            Some("local") => TrackOriginTag::Local,
            Some("plex") => TrackOriginTag::Plex,
            _ => TrackOriginTag::ExternalUnknown,
        }
    }
}

impl TrackOriginTag {
    pub fn is_castable_to_qconnect(self) -> bool {
        matches!(self, Self::Qobuz | Self::OfflineCache)
    }
}

/// A reference to a piece of cover art, resolvable regardless of origin.
///
/// The artwork loaders historically handled only remote HTTP URLs, which is
/// why local-file and Plex artwork failed to reach the UI. This enum is the
/// uniform contract: a frontend's artwork pipeline matches on it and fetches
/// the bytes the right way for each variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtworkRef {
    /// An HTTP(S) URL (Qobuz covers; Plex thumbs that already carry a token).
    Remote(String),
    /// A path to a cover image on the local filesystem.
    LocalFile(String),
    /// A Plex thumbnail fetched with an auth token appended. `path` is the
    /// server-relative thumb path (e.g. `/library/metadata/42/thumb/1`).
    PlexThumb {
        base_url: String,
        token: String,
        path: String,
    },
    /// Cover bytes already in memory (e.g. embedded tags).
    Embedded(Vec<u8>),
    /// No artwork available.
    None,
}

impl ArtworkRef {
    /// True when there is effectively nothing to load (explicit `None` or an
    /// empty Remote/LocalFile string).
    pub fn is_empty(&self) -> bool {
        match self {
            ArtworkRef::None => true,
            ArtworkRef::Remote(s) | ArtworkRef::LocalFile(s) => s.is_empty(),
            ArtworkRef::Embedded(b) => b.is_empty(),
            ArtworkRef::PlexThumb { path, .. } => path.is_empty(),
        }
    }
}

impl QueueTrack {
    /// The track's playback source, parsed from its `source` field.
    pub fn source_kind(&self) -> PlaybackSource {
        PlaybackSource::from_source_str(self.source.as_deref())
    }

    /// A uniform reference to this track's cover art.
    ///
    /// The heuristic is source-agnostic (it does not trust `source` to be
    /// set): an `http(s)://` value is [`ArtworkRef::Remote`]; a `file://`
    /// value or a bare filesystem path is [`ArtworkRef::LocalFile`] (local
    /// library + offline-cache covers live on disk). Plex thumbnails that
    /// need a token are produced by the Plex queue builder via
    /// [`ArtworkRef::PlexThumb`] directly, not here.
    pub fn artwork_ref(&self) -> ArtworkRef {
        let raw = self.artwork_url.as_deref().unwrap_or("");
        if raw.is_empty() {
            return ArtworkRef::None;
        }
        if raw.starts_with("http://") || raw.starts_with("https://") {
            ArtworkRef::Remote(raw.to_string())
        } else if let Some(path) = raw.strip_prefix("file://") {
            ArtworkRef::LocalFile(path.to_string())
        } else {
            ArtworkRef::LocalFile(raw.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_str_roundtrip_and_default() {
        assert_eq!(PlaybackSource::from_source_str(Some("local")), PlaybackSource::Local);
        assert_eq!(PlaybackSource::from_source_str(Some("plex")), PlaybackSource::Plex);
        assert_eq!(
            PlaybackSource::from_source_str(Some("qobuz_download")),
            PlaybackSource::OfflineCache
        );
        assert_eq!(PlaybackSource::from_source_str(Some("qobuz")), PlaybackSource::Qobuz);
        // Unknown / absent -> Qobuz (historical default).
        assert_eq!(PlaybackSource::from_source_str(None), PlaybackSource::Qobuz);
        assert_eq!(PlaybackSource::from_source_str(Some("???")), PlaybackSource::Qobuz);
        for s in [
            PlaybackSource::Qobuz,
            PlaybackSource::OfflineCache,
            PlaybackSource::Local,
            PlaybackSource::Plex,
        ] {
            assert_eq!(PlaybackSource::from_source_str(Some(s.as_source_str())), s);
        }
    }

    #[test]
    fn offline_cache_is_castable() {
        assert!(PlaybackSource::OfflineCache.is_castable_to_qconnect());
        assert!(TrackOriginTag::OfflineCache.is_castable_to_qconnect());
    }

    #[test]
    fn strict_parse_blocks_unknown_and_absent() {
        use TrackOriginTag::*;
        assert_eq!(PlaybackSource::from_source_str_strict(Some("qobuz")), Qobuz);
        assert_eq!(PlaybackSource::from_source_str_strict(Some("local")), Local);
        assert_eq!(PlaybackSource::from_source_str_strict(Some("plex")), Plex);
        assert_eq!(PlaybackSource::from_source_str_strict(Some("qobuz_download")), OfflineCache);
        assert_eq!(PlaybackSource::from_source_str_strict(None), ExternalUnknown);
        assert_eq!(PlaybackSource::from_source_str_strict(Some("???")), ExternalUnknown);
        // Lenient parser still defaults to Qobuz (playback compatibility).
        assert_eq!(PlaybackSource::from_source_str(None), PlaybackSource::Qobuz);
    }

    #[test]
    fn only_qobuz_is_castable() {
        assert!(PlaybackSource::Qobuz.is_qobuz_streamable());
        assert!(!PlaybackSource::OfflineCache.is_qobuz_streamable());
        assert!(!PlaybackSource::Local.is_qobuz_streamable());
        assert!(!PlaybackSource::Plex.is_qobuz_streamable());
    }

    fn track_with(source: Option<&str>, artwork: Option<&str>) -> QueueTrack {
        QueueTrack {
            id: 1,
            title: "t".into(),
            version: None,
            artist: "a".into(),
            album: "al".into(),
            duration_secs: 0,
            artwork_url: artwork.map(|s| s.to_string()),
            hires: false,
            bit_depth: None,
            sample_rate: None,
            is_local: false,
            album_id: None,
            artist_id: None,
            streamable: true,
            source: source.map(|s| s.to_string()),
            parental_warning: false,
            source_item_id_hint: None,
        }
    }

    #[test]
    fn artwork_ref_classifies_by_value() {
        assert_eq!(track_with(None, None).artwork_ref(), ArtworkRef::None);
        assert_eq!(
            track_with(Some("qobuz"), Some("https://x/cover.jpg")).artwork_ref(),
            ArtworkRef::Remote("https://x/cover.jpg".into())
        );
        assert_eq!(
            track_with(Some("local"), Some("/home/u/cover.jpg")).artwork_ref(),
            ArtworkRef::LocalFile("/home/u/cover.jpg".into())
        );
        assert_eq!(
            track_with(Some("local"), Some("file:///home/u/cover.jpg")).artwork_ref(),
            ArtworkRef::LocalFile("/home/u/cover.jpg".into())
        );
    }
}
