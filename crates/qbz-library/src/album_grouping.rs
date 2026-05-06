//! SQL fragments for metadata-based album grouping in the Local Library.
//!
//! The metadata-grouped Albums view groups tracks by `album` +
//! `COALESCE(album_artist, artist)` when the album tag is usable, falls
//! back to the existing folder-based `album_group_key` when it's not,
//! and dumps anything else into a single `__unknown_album__` bucket.
//!
//! Both `get_albums_metadata_grouped` and `get_album_tracks_metadata`
//! must produce the same group_key for the same row, so the expression
//! is centralized here.

/// SQL expression that produces the metadata group key for a row of
/// `local_tracks`. Insert wherever you would otherwise use a column.
pub fn metadata_group_key_sql_expression() -> &'static str {
    r#"CASE
        WHEN album IS NOT NULL
          AND TRIM(album) != ''
          AND album != 'Unknown Album'
        THEN album || '|' || COALESCE(album_artist, artist, 'Unknown Artist')

        WHEN album_group_key IS NOT NULL
          AND album_group_key != ''
        THEN album_group_key

        ELSE '__unknown_album__'
    END"#
}

/// Sentinel group_key value used for the orphan bucket. Frontend can
/// special-case this if it needs a localized label.
pub const UNKNOWN_ALBUM_GROUP_KEY: &str = "__unknown_album__";
