//! First-class LOCAL playlists (offline-mode port, decisions D7/D8).
//!
//! Unlike the `playlist_*` sidecar tables (which enhance a *Qobuz* playlist
//! keyed by its server id), these are standalone entities living entirely in
//! the per-user `library.db`. Ids are TEXT `local:<uuid>` (Mixtape precedent)
//! so they are unrepresentable in any Qobuz-bound call that takes a `u64`
//! playlist id — the type guard demanded by D7.
//!
//! All functions take `&Connection` (the qbz-mixtape repo idiom): no Tauri
//! state, no async runtime — testable with in-memory SQLite. The Slint
//! command layer reaches them through `LibraryDatabase::with_connection`.

use rusqlite::{params, Connection, OptionalExtension, Result};
use uuid::Uuid;

/// Id prefix that marks a local playlist. A `local:<uuid>` id can never
/// parse as the `u64` every Qobuz playlist endpoint takes.
pub const LOCAL_PLAYLIST_PREFIX: &str = "local:";

/// True when `id` names a local playlist (`local:<uuid>` namespace).
pub fn is_local_playlist_id(id: &str) -> bool {
    id.starts_with(LOCAL_PLAYLIST_PREFIX)
}

/// Track source inside a local playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalPlaylistTrackSource {
    Qobuz,
    Local,
    Plex,
}

impl LocalPlaylistTrackSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Qobuz => "qobuz",
            Self::Local => "local",
            Self::Plex => "plex",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "local" => Self::Local,
            "plex" => Self::Plex,
            _ => Self::Qobuz,
        }
    }
}

/// One playlist row (header metadata + per-source counts).
#[derive(Debug, Clone)]
pub struct LocalPlaylist {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// D8: never offered for upload, never reaches any Qobuz call or
    /// QConnect queue push.
    pub offline_only: bool,
    pub custom_artwork_path: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub track_count: u32,
    pub qobuz_count: u32,
    pub local_count: u32,
    pub plex_count: u32,
}

/// One membership row, ordered by `position`.
#[derive(Debug, Clone)]
pub struct LocalPlaylistTrack {
    pub playlist_id: String,
    pub position: i32,
    pub source: LocalPlaylistTrackSource,
    pub qobuz_track_id: Option<u64>,
    pub local_path: Option<String>,
    pub plex_key: Option<String>,
    pub added_at: i64,
}

/// Input for `add_tracks` — exactly one of the three refs per source.
#[derive(Debug, Clone)]
pub enum LocalPlaylistTrackInput {
    Qobuz(u64),
    Local(String),
    Plex(String),
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Create the local-playlist tables. Idempotent (`IF NOT EXISTS`), run by
/// `LibraryDatabase::open` next to the rest of the schema.
pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS local_playlists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            offline_only INTEGER NOT NULL DEFAULT 0,
            custom_artwork_path TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS local_playlist_tracks (
            playlist_id TEXT NOT NULL REFERENCES local_playlists(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            source TEXT NOT NULL,           -- 'qobuz' | 'local' | 'plex'
            qobuz_track_id INTEGER,
            local_path TEXT,
            plex_key TEXT,
            added_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_local_playlist_tracks_playlist
            ON local_playlist_tracks(playlist_id, position);
        "#,
    )
}

// ──────────────────────────── Playlist CRUD ────────────────────────────

/// Create a playlist; returns its `local:<uuid>` id.
pub fn create(
    conn: &Connection,
    name: &str,
    description: Option<&str>,
    offline_only: bool,
) -> Result<String> {
    let id = format!("{LOCAL_PLAYLIST_PREFIX}{}", Uuid::new_v4());
    let ts = now_ms();
    conn.execute(
        "INSERT INTO local_playlists (id, name, description, offline_only, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![id, name, description, offline_only as i64, ts],
    )?;
    Ok(id)
}

pub fn rename(conn: &Connection, id: &str, new_name: &str) -> Result<()> {
    conn.execute(
        "UPDATE local_playlists SET name = ?1, updated_at = ?2 WHERE id = ?3",
        params![new_name, now_ms(), id],
    )?;
    Ok(())
}

pub fn set_description(conn: &Connection, id: &str, description: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE local_playlists SET description = ?1, updated_at = ?2 WHERE id = ?3",
        params![description, now_ms(), id],
    )?;
    Ok(())
}

pub fn set_offline_only(conn: &Connection, id: &str, offline_only: bool) -> Result<()> {
    conn.execute(
        "UPDATE local_playlists SET offline_only = ?1, updated_at = ?2 WHERE id = ?3",
        params![offline_only as i64, now_ms(), id],
    )?;
    Ok(())
}

pub fn set_custom_artwork(conn: &Connection, id: &str, path: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE local_playlists SET custom_artwork_path = ?1, updated_at = ?2 WHERE id = ?3",
        params![path, now_ms(), id],
    )?;
    Ok(())
}

/// Delete the playlist. Membership rows are removed explicitly as well as
/// by the FK cascade — `PRAGMA foreign_keys` is connection-scoped and the
/// app's connections don't enable it, so don't rely on the cascade alone.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM local_playlist_tracks WHERE playlist_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM local_playlists WHERE id = ?1", params![id])?;
    Ok(())
}

fn row_to_playlist(r: &rusqlite::Row) -> Result<LocalPlaylist> {
    Ok(LocalPlaylist {
        id: r.get("id")?,
        name: r.get("name")?,
        description: r.get("description")?,
        offline_only: r.get::<_, i64>("offline_only")? != 0,
        custom_artwork_path: r.get("custom_artwork_path")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        track_count: 0,
        qobuz_count: 0,
        local_count: 0,
        plex_count: 0,
    })
}

/// Fill the per-source counts on a loaded playlist header.
fn hydrate_counts(conn: &Connection, p: &mut LocalPlaylist) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT source, COUNT(*) FROM local_playlist_tracks
         WHERE playlist_id = ?1 GROUP BY source",
    )?;
    let rows = stmt.query_map(params![p.id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?))
    })?;
    for row in rows {
        let (source, count) = row?;
        match LocalPlaylistTrackSource::parse(&source) {
            LocalPlaylistTrackSource::Qobuz => p.qobuz_count = count,
            LocalPlaylistTrackSource::Local => p.local_count = count,
            LocalPlaylistTrackSource::Plex => p.plex_count = count,
        }
        p.track_count += count;
    }
    Ok(())
}

/// All local playlists (counts hydrated), newest first.
pub fn list(conn: &Connection) -> Result<Vec<LocalPlaylist>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, description, offline_only, custom_artwork_path,
                created_at, updated_at
           FROM local_playlists
          ORDER BY created_at DESC",
    )?;
    let mut out: Vec<LocalPlaylist> = Vec::new();
    for r in stmt.query_map([], row_to_playlist)? {
        out.push(r?);
    }
    for p in out.iter_mut() {
        hydrate_counts(conn, p)?;
    }
    Ok(out)
}

/// One playlist header (counts hydrated), or None.
pub fn get(conn: &Connection, id: &str) -> Result<Option<LocalPlaylist>> {
    let maybe = conn
        .query_row(
            "SELECT id, name, description, offline_only, custom_artwork_path,
                    created_at, updated_at
               FROM local_playlists WHERE id = ?1",
            params![id],
            row_to_playlist,
        )
        .optional()?;
    match maybe {
        Some(mut p) => {
            hydrate_counts(conn, &mut p)?;
            Ok(Some(p))
        }
        None => Ok(None),
    }
}

// ──────────────────────────── Track CRUD ────────────────────────────

/// Membership rows in position order.
pub fn get_tracks(conn: &Connection, playlist_id: &str) -> Result<Vec<LocalPlaylistTrack>> {
    let mut stmt = conn.prepare(
        "SELECT playlist_id, position, source, qobuz_track_id, local_path, plex_key, added_at
           FROM local_playlist_tracks
          WHERE playlist_id = ?1
          ORDER BY position ASC",
    )?;
    let mut out = Vec::new();
    for r in stmt.query_map(params![playlist_id], |r| {
        Ok(LocalPlaylistTrack {
            playlist_id: r.get("playlist_id")?,
            position: r.get("position")?,
            source: LocalPlaylistTrackSource::parse(&r.get::<_, String>("source")?),
            qobuz_track_id: r.get::<_, Option<i64>>("qobuz_track_id")?.map(|v| v as u64),
            local_path: r.get("local_path")?,
            plex_key: r.get("plex_key")?,
            added_at: r.get("added_at")?,
        })
    })? {
        out.push(r?);
    }
    Ok(out)
}

/// Append tracks at the end (positions continue after the current max).
/// Exact duplicates (same source + same ref already in the playlist) are
/// skipped silently (the sidecar tables' UNIQUE idempotence, kept here by
/// an explicit existence check since position is part of every row).
/// Returns the number of rows actually inserted.
pub fn add_tracks(
    conn: &Connection,
    playlist_id: &str,
    entries: &[LocalPlaylistTrackInput],
) -> Result<usize> {
    let mut next_pos: i32 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1
           FROM local_playlist_tracks WHERE playlist_id = ?1",
        params![playlist_id],
        |r| r.get(0),
    )?;
    let ts = now_ms();
    let mut inserted = 0usize;
    for entry in entries {
        let (source, qobuz_id, local_path, plex_key): (
            &str,
            Option<i64>,
            Option<&str>,
            Option<&str>,
        ) = match entry {
            LocalPlaylistTrackInput::Qobuz(id) => ("qobuz", Some(*id as i64), None, None),
            LocalPlaylistTrackInput::Local(path) => ("local", None, Some(path.as_str()), None),
            LocalPlaylistTrackInput::Plex(key) => ("plex", None, None, Some(key.as_str())),
        };
        let exists: bool = conn
            .prepare(
                "SELECT 1 FROM local_playlist_tracks
                  WHERE playlist_id = ?1 AND source = ?2
                    AND COALESCE(qobuz_track_id, -1) = COALESCE(?3, -1)
                    AND COALESCE(local_path, '') = COALESCE(?4, '')
                    AND COALESCE(plex_key, '') = COALESCE(?5, '')
                  LIMIT 1",
            )?
            .exists(params![playlist_id, source, qobuz_id, local_path, plex_key])?;
        if exists {
            continue;
        }
        conn.execute(
            "INSERT INTO local_playlist_tracks
                (playlist_id, position, source, qobuz_track_id, local_path, plex_key, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![playlist_id, next_pos, source, qobuz_id, local_path, plex_key, ts],
        )?;
        next_pos += 1;
        inserted += 1;
    }
    if inserted > 0 {
        conn.execute(
            "UPDATE local_playlists SET updated_at = ?1 WHERE id = ?2",
            params![ts, playlist_id],
        )?;
    }
    Ok(inserted)
}

/// Remove the row at `position` and compact the positions above it.
pub fn remove_track(conn: &Connection, playlist_id: &str, position: i32) -> Result<()> {
    conn.execute(
        "DELETE FROM local_playlist_tracks WHERE playlist_id = ?1 AND position = ?2",
        params![playlist_id, position],
    )?;
    conn.execute(
        "UPDATE local_playlist_tracks SET position = position - 1
          WHERE playlist_id = ?1 AND position > ?2",
        params![playlist_id, position],
    )?;
    conn.execute(
        "UPDATE local_playlists SET updated_at = ?1 WHERE id = ?2",
        params![now_ms(), playlist_id],
    )?;
    Ok(())
}

// ──────────────────────────── Tests ────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn create_assigns_namespaced_id_and_roundtrips() {
        let conn = fresh_db();
        let id = create(&conn, "Road Trip", Some("desc"), false).unwrap();
        assert!(is_local_playlist_id(&id), "id must carry the local: prefix");
        let p = get(&conn, &id).unwrap().unwrap();
        assert_eq!(p.name, "Road Trip");
        assert_eq!(p.description.as_deref(), Some("desc"));
        assert!(!p.offline_only);
        assert_eq!(p.track_count, 0);
    }

    #[test]
    fn offline_only_flag_persists_and_flips() {
        let conn = fresh_db();
        let id = create(&conn, "Vault", None, true).unwrap();
        assert!(get(&conn, &id).unwrap().unwrap().offline_only);
        set_offline_only(&conn, &id, false).unwrap();
        assert!(!get(&conn, &id).unwrap().unwrap().offline_only);
    }

    #[test]
    fn rename_and_description_update() {
        let conn = fresh_db();
        let id = create(&conn, "Old", None, false).unwrap();
        rename(&conn, &id, "New").unwrap();
        set_description(&conn, &id, Some("hello")).unwrap();
        let p = get(&conn, &id).unwrap().unwrap();
        assert_eq!(p.name, "New");
        assert_eq!(p.description.as_deref(), Some("hello"));
    }

    #[test]
    fn add_tracks_appends_positions_across_sources() {
        let conn = fresh_db();
        let id = create(&conn, "Mixed", None, false).unwrap();
        let n = add_tracks(
            &conn,
            &id,
            &[
                LocalPlaylistTrackInput::Qobuz(111),
                LocalPlaylistTrackInput::Local("/music/a.flac".into()),
                LocalPlaylistTrackInput::Plex("plex-key-9".into()),
            ],
        )
        .unwrap();
        assert_eq!(n, 3);
        // Second batch continues the position sequence.
        let n2 = add_tracks(&conn, &id, &[LocalPlaylistTrackInput::Qobuz(222)]).unwrap();
        assert_eq!(n2, 1);

        let rows = get_tracks(&conn, &id).unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows.iter().map(|r| r.position).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        assert_eq!(rows[0].qobuz_track_id, Some(111));
        assert_eq!(rows[1].local_path.as_deref(), Some("/music/a.flac"));
        assert_eq!(rows[2].plex_key.as_deref(), Some("plex-key-9"));
        assert_eq!(rows[3].qobuz_track_id, Some(222));

        let p = get(&conn, &id).unwrap().unwrap();
        assert_eq!(p.track_count, 4);
        assert_eq!(p.qobuz_count, 2);
        assert_eq!(p.local_count, 1);
        assert_eq!(p.plex_count, 1);
    }

    #[test]
    fn add_tracks_skips_exact_duplicates() {
        let conn = fresh_db();
        let id = create(&conn, "Dedupe", None, false).unwrap();
        add_tracks(&conn, &id, &[LocalPlaylistTrackInput::Qobuz(7)]).unwrap();
        let n = add_tracks(
            &conn,
            &id,
            &[
                LocalPlaylistTrackInput::Qobuz(7),
                LocalPlaylistTrackInput::Local("/x.flac".into()),
            ],
        )
        .unwrap();
        assert_eq!(n, 1, "duplicate qobuz id skipped, new local row inserted");
        assert_eq!(get_tracks(&conn, &id).unwrap().len(), 2);
    }

    #[test]
    fn remove_track_compacts_positions() {
        let conn = fresh_db();
        let id = create(&conn, "Compact", None, false).unwrap();
        add_tracks(
            &conn,
            &id,
            &[
                LocalPlaylistTrackInput::Qobuz(1),
                LocalPlaylistTrackInput::Qobuz(2),
                LocalPlaylistTrackInput::Qobuz(3),
            ],
        )
        .unwrap();
        remove_track(&conn, &id, 1).unwrap();
        let rows = get_tracks(&conn, &id).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].qobuz_track_id, Some(1));
        assert_eq!(rows[0].position, 0);
        assert_eq!(rows[1].qobuz_track_id, Some(3));
        assert_eq!(rows[1].position, 1);
    }

    #[test]
    fn delete_cascades_membership_rows() {
        let conn = fresh_db();
        let id = create(&conn, "Doomed", None, true).unwrap();
        add_tracks(&conn, &id, &[LocalPlaylistTrackInput::Qobuz(42)]).unwrap();
        delete(&conn, &id).unwrap();
        assert!(get(&conn, &id).unwrap().is_none());
        assert!(get_tracks(&conn, &id).unwrap().is_empty());
        // The membership table holds no orphans.
        let orphans: i64 = conn
            .query_row("SELECT COUNT(*) FROM local_playlist_tracks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(orphans, 0);
    }

    #[test]
    fn list_returns_all_with_counts() {
        let conn = fresh_db();
        let a = create(&conn, "A", None, false).unwrap();
        let b = create(&conn, "B", None, true).unwrap();
        add_tracks(&conn, &a, &[LocalPlaylistTrackInput::Qobuz(5)]).unwrap();
        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 2);
        let pa = all.iter().find(|p| p.id == a).unwrap();
        let pb = all.iter().find(|p| p.id == b).unwrap();
        assert_eq!(pa.track_count, 1);
        assert!(!pa.offline_only);
        assert_eq!(pb.track_count, 0);
        assert!(pb.offline_only);
    }
}
