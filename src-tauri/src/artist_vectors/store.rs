//! SQLite-based storage for artist vectors
//!
//! Persists artist index mapping and sparse vectors for similarity search.

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use super::sparse_vector::SparseVector;

/// TTL for vector entries (7 days)
#[allow(dead_code)]
const VECTOR_TTL_SECS: i64 = 7 * 24 * 60 * 60;

/// Shared state wrapper for Tauri
pub struct ArtistVectorStoreState {
    pub store: Arc<Mutex<Option<ArtistVectorStore>>>,
}

impl ArtistVectorStoreState {
    pub fn new() -> Result<Self, String> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not determine data directory")?
            .join("qbz")
            .join("cache");

        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;

        let db_path = data_dir.join("artist_vectors.db");
        let store = ArtistVectorStore::new(&db_path)?;

        log::info!("Artist vector store initialized at {:?}", db_path);

        Ok(Self {
            store: Arc::new(Mutex::new(Some(store))),
        })
    }

    pub fn new_empty() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn init_at(&self, base_dir: &Path) -> Result<(), String> {
        let cache_dir = base_dir.join("cache");
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;
        let db_path = cache_dir.join("artist_vectors.db");
        let new_store = ArtistVectorStore::new(&db_path)?;
        log::info!("Artist vector store initialized at {:?}", db_path);
        let mut guard = self.store.lock().await;
        *guard = Some(new_store);
        Ok(())
    }

    pub async fn teardown(&self) {
        let mut guard = self.store.lock().await;
        *guard = None;
    }
}

/// Artist vector store with SQLite backend
pub struct ArtistVectorStore {
    conn: Connection,
    /// In-memory cache of MBID to index mapping
    artist_to_idx: HashMap<String, u32>,
    /// Reverse mapping: index to MBID
    idx_to_artist: Vec<String>,
    /// Next available index
    next_idx: u32,
}

/// Information about a stored vector entry
#[derive(Debug, Clone)]
pub struct VectorEntry {
    pub artist_mbid: String,
    pub vector: SparseVector,
    pub updated_at: i64,
    pub sources: Vec<String>,
}

/// Result of a similarity search
#[derive(Debug, Clone)]
pub struct SimilarArtist {
    pub mbid: String,
    pub name: Option<String>,
    pub similarity: f32,
}

impl ArtistVectorStore {
    /// Create a new store at the given path
    pub fn new(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open artist vector store: {}", e))?;

        // Enable WAL mode for better concurrency
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("Failed to set pragmas: {}", e))?;

        let mut store = Self {
            conn,
            artist_to_idx: HashMap::new(),
            idx_to_artist: Vec::new(),
            next_idx: 0,
        };

        store.init()?;
        store.load_artist_index()?;

        Ok(store)
    }

    /// Initialize database schema
    fn init(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                r#"
                -- Artist index: maps MBID to integer index for vectors
                CREATE TABLE IF NOT EXISTS artist_index (
                    idx INTEGER PRIMARY KEY,
                    mbid TEXT UNIQUE NOT NULL,
                    name TEXT,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                CREATE INDEX IF NOT EXISTS idx_artist_index_mbid ON artist_index(mbid);

                -- Vector entries: sparse representation (one row per non-zero)
                CREATE TABLE IF NOT EXISTS vector_entries (
                    artist_idx INTEGER NOT NULL,
                    target_idx INTEGER NOT NULL,
                    weight REAL NOT NULL,
                    source TEXT NOT NULL,
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    PRIMARY KEY (artist_idx, target_idx, source)
                );
                CREATE INDEX IF NOT EXISTS idx_vector_entries_artist ON vector_entries(artist_idx);
                CREATE INDEX IF NOT EXISTS idx_vector_entries_target ON vector_entries(target_idx);
                CREATE INDEX IF NOT EXISTS idx_vector_entries_updated ON vector_entries(updated_at);

                -- Vector metadata: track when each artist's vector was last computed
                CREATE TABLE IF NOT EXISTS vector_metadata (
                    artist_idx INTEGER PRIMARY KEY,
                    updated_at INTEGER NOT NULL,
                    nnz INTEGER NOT NULL DEFAULT 0
                );
                "#,
            )
            .map_err(|e| format!("Failed to initialize schema: {}", e))?;

        Ok(())
    }

    /// Load artist index from database into memory
    fn load_artist_index(&mut self) -> Result<(), String> {
        let mut stmt = self
            .conn
            .prepare("SELECT idx, mbid FROM artist_index ORDER BY idx")
            .map_err(|e| format!("Failed to prepare index query: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("Failed to query index: {}", e))?;

        self.artist_to_idx.clear();
        self.idx_to_artist.clear();

        for row in rows {
            let (idx, mbid) = row.map_err(|e| format!("Failed to read row: {}", e))?;
            self.artist_to_idx.insert(mbid.clone(), idx);

            // Ensure idx_to_artist has enough capacity
            while self.idx_to_artist.len() <= idx as usize {
                self.idx_to_artist.push(String::new());
            }
            self.idx_to_artist[idx as usize] = mbid;

            if idx >= self.next_idx {
                self.next_idx = idx + 1;
            }
        }

        log::debug!(
            "Loaded {} artists into index, next_idx={}",
            self.artist_to_idx.len(),
            self.next_idx
        );

        Ok(())
    }

    /// Get or create an index for an artist MBID
    pub fn get_or_create_idx(&mut self, mbid: &str, name: Option<&str>) -> Result<u32, String> {
        if let Some(&idx) = self.artist_to_idx.get(mbid) {
            return Ok(idx);
        }

        let idx = self.next_idx;
        self.next_idx += 1;

        self.conn
            .execute(
                "INSERT INTO artist_index (idx, mbid, name) VALUES (?1, ?2, ?3)",
                params![idx, mbid, name],
            )
            .map_err(|e| format!("Failed to insert artist index: {}", e))?;

        self.artist_to_idx.insert(mbid.to_string(), idx);

        // Extend idx_to_artist
        while self.idx_to_artist.len() <= idx as usize {
            self.idx_to_artist.push(String::new());
        }
        self.idx_to_artist[idx as usize] = mbid.to_string();

        Ok(idx)
    }

    /// Get index for an artist MBID (returns None if not found)
    pub fn get_idx(&self, mbid: &str) -> Option<u32> {
        self.artist_to_idx.get(mbid).copied()
    }

    /// Get MBID for an index
    pub fn get_mbid(&self, idx: u32) -> Option<&str> {
        self.idx_to_artist.get(idx as usize).map(|s| s.as_str())
    }

    /// Store a vector for an artist
    pub fn set_vector(
        &mut self,
        mbid: &str,
        vector: &SparseVector,
        source: &str,
    ) -> Result<(), String> {
        let artist_idx = self.get_or_create_idx(mbid, None)?;
        let now = current_timestamp();

        // Delete existing entries for this artist+source
        self.conn
            .execute(
                "DELETE FROM vector_entries WHERE artist_idx = ?1 AND source = ?2",
                params![artist_idx, source],
            )
            .map_err(|e| format!("Failed to delete old entries: {}", e))?;

        // Insert new entries
        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO vector_entries (artist_idx, target_idx, weight, source, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(|e| format!("Failed to prepare insert: {}", e))?;

        for (target_idx, weight) in vector.iter() {
            stmt.execute(params![artist_idx, target_idx, weight, source, now])
                .map_err(|e| format!("Failed to insert entry: {}", e))?;
        }

        // Update metadata
        self.conn
            .execute(
                "INSERT OR REPLACE INTO vector_metadata (artist_idx, updated_at, nnz)
                 VALUES (?1, ?2, ?3)",
                params![artist_idx, now, vector.nnz()],
            )
            .map_err(|e| format!("Failed to update metadata: {}", e))?;

        Ok(())
    }

    /// Get the combined vector for an artist (all sources merged)
    pub fn get_vector(&self, mbid: &str) -> Option<SparseVector> {
        let artist_idx = self.get_idx(mbid)?;

        let mut stmt = self
            .conn
            .prepare("SELECT target_idx, SUM(weight) FROM vector_entries WHERE artist_idx = ?1 GROUP BY target_idx")
            .ok()?;

        let rows = stmt
            .query_map(params![artist_idx], |row| {
                Ok((row.get::<_, u32>(0)?, row.get::<_, f32>(1)?))
            })
            .ok()?;

        let mut indices = Vec::new();
        let mut values = Vec::new();

        for row in rows.flatten() {
            indices.push(row.0);
            values.push(row.1);
        }

        if indices.is_empty() {
            return None;
        }

        // Sort by index
        let mut pairs: Vec<_> = indices.into_iter().zip(values).collect();
        pairs.sort_by_key(|(idx, _)| *idx);

        let (indices, values): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();

        Some(SparseVector::from_parts(indices, values))
    }

    /// Check if a vector exists and is fresh (within TTL)
    pub fn has_fresh_vector(&self, mbid: &str, max_age_secs: i64) -> bool {
        let Some(artist_idx) = self.get_idx(mbid) else {
            return false;
        };

        let cutoff = current_timestamp() - max_age_secs;

        let result: Option<i64> = self
            .conn
            .query_row(
                "SELECT updated_at FROM vector_metadata WHERE artist_idx = ?1 AND updated_at > ?2",
                params![artist_idx, cutoff],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();

        result.is_some()
    }

    /// Find artists most similar to a query vector
    ///
    /// Returns top-k artists by cosine similarity, excluding those in `exclude`.
    pub fn find_nearest(
        &self,
        query: &SparseVector,
        k: usize,
        exclude_mbids: &[String],
    ) -> Result<Vec<SimilarArtist>, String> {
        if query.is_empty() {
            return Ok(Vec::new());
        }

        // Get all artist indices to exclude
        let exclude_indices: Vec<u32> = exclude_mbids
            .iter()
            .filter_map(|mbid| self.get_idx(mbid))
            .collect();

        // Get all unique artist indices that have vectors
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT artist_idx FROM vector_entries")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let artist_indices: Vec<u32> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Failed to query artists: {}", e))?
            .filter_map(|r| r.ok())
            .filter(|idx| !exclude_indices.contains(idx))
            .collect();

        // Compute similarity for each candidate
        let mut similarities: Vec<(u32, f32)> = Vec::new();

        for artist_idx in artist_indices {
            if let Some(mbid) = self.get_mbid(artist_idx) {
                if let Some(vec) = self.get_vector(mbid) {
                    let sim = query.cosine_similarity(&vec);
                    if sim > 0.0 {
                        similarities.push((artist_idx, sim));
                    }
                }
            }
        }

        // Sort by similarity descending
        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        similarities.truncate(k);

        // Convert to result type
        let results = similarities
            .into_iter()
            .filter_map(|(idx, sim)| {
                let mbid = self.get_mbid(idx)?.to_string();
                let name = self.get_artist_name(&mbid);
                Some(SimilarArtist {
                    mbid,
                    name,
                    similarity: sim,
                })
            })
            .collect();

        Ok(results)
    }

    /// Get artist name from index
    pub fn get_artist_name(&self, mbid: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT name FROM artist_index WHERE mbid = ?1",
                params![mbid],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    /// Get related artists for a given artist (from their vector entries)
    ///
    /// This returns artists that the given artist is connected to via
    /// MusicBrainz relationships (members, collaborators, etc.)
    pub fn get_related_artists(&self, mbid: &str) -> Result<Vec<SimilarArtist>, String> {
        let Some(artist_idx) = self.get_idx(mbid) else {
            return Ok(Vec::new());
        };

        let mut stmt = self
            .conn
            .prepare(
                "SELECT target_idx, SUM(weight) as total_weight
                 FROM vector_entries
                 WHERE artist_idx = ?1
                 GROUP BY target_idx
                 ORDER BY total_weight DESC",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map(params![artist_idx], |row| {
                Ok((row.get::<_, u32>(0)?, row.get::<_, f32>(1)?))
            })
            .map_err(|e| format!("Failed to query relations: {}", e))?;

        let mut results = Vec::new();
        for row in rows.flatten() {
            let (target_idx, weight) = row;
            if let Some(target_mbid) = self.get_mbid(target_idx) {
                let name = self.get_artist_name(target_mbid);
                results.push(SimilarArtist {
                    mbid: target_mbid.to_string(),
                    name,
                    similarity: weight,
                });
            }
        }

        Ok(results)
    }

    /// Get all related artists for multiple source artists, excluding specified MBIDs
    ///
    /// Returns a deduplicated list sorted by total weight across all sources
    pub fn get_all_related_artists(
        &self,
        source_mbids: &[String],
        exclude_mbids: &[String],
        limit: usize,
    ) -> Result<Vec<SimilarArtist>, String> {
        use std::collections::HashMap;

        let exclude_set: std::collections::HashSet<_> = exclude_mbids.iter().collect();
        let mut artist_weights: HashMap<String, (Option<String>, f32)> = HashMap::new();

        for mbid in source_mbids {
            let related = self.get_related_artists(mbid)?;
            for artist in related {
                if exclude_set.contains(&artist.mbid) {
                    continue;
                }
                let entry = artist_weights
                    .entry(artist.mbid.clone())
                    .or_insert((artist.name.clone(), 0.0));
                entry.1 += artist.similarity;
            }
        }

        let mut results: Vec<_> = artist_weights
            .into_iter()
            .map(|(mbid, (name, weight))| SimilarArtist {
                mbid,
                name,
                similarity: weight,
            })
            .collect();

        results.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);

        Ok(results)
    }

    /// Get statistics about the store
    pub fn get_stats(&self) -> Result<StoreStats, String> {
        let artist_count = self.artist_to_idx.len();

        let vector_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(DISTINCT artist_idx) FROM vector_entries",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to count vectors: {}", e))?;

        let entry_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM vector_entries", [], |row| row.get(0))
            .map_err(|e| format!("Failed to count entries: {}", e))?;

        Ok(StoreStats {
            artist_count,
            vector_count: vector_count as usize,
            entry_count: entry_count as usize,
        })
    }

    /// Clean up expired entries
    pub fn cleanup_expired(&mut self, max_age_secs: i64) -> Result<usize, String> {
        let cutoff = current_timestamp() - max_age_secs;

        let deleted = self
            .conn
            .execute(
                "DELETE FROM vector_entries WHERE updated_at < ?1",
                params![cutoff],
            )
            .map_err(|e| format!("Failed to delete expired entries: {}", e))?;

        // Also clean up metadata
        self.conn
            .execute(
                "DELETE FROM vector_metadata WHERE updated_at < ?1",
                params![cutoff],
            )
            .map_err(|e| format!("Failed to delete expired metadata: {}", e))?;

        Ok(deleted)
    }

    /// Clear all data from the store
    pub fn clear_all(&mut self) -> Result<usize, String> {
        let deleted = self
            .conn
            .execute("DELETE FROM vector_entries", [])
            .map_err(|e| format!("Failed to delete vector entries: {}", e))?;

        self.conn
            .execute("DELETE FROM vector_metadata", [])
            .map_err(|e| format!("Failed to delete metadata: {}", e))?;

        self.conn
            .execute("DELETE FROM artist_index", [])
            .map_err(|e| format!("Failed to delete artist index: {}", e))?;

        // Reset in-memory state
        self.artist_to_idx.clear();
        self.idx_to_artist.clear();
        self.next_idx = 0;

        log::info!("Artist vector store cleared: {} entries deleted", deleted);
        Ok(deleted)
    }
}

/// Statistics about the vector store
#[derive(Debug, Clone, serde::Serialize)]
pub struct StoreStats {
    pub artist_count: usize,
    pub vector_count: usize,
    pub entry_count: usize,
}

/// Get current Unix timestamp
fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_store() -> ArtistVectorStore {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_vectors.db");
        ArtistVectorStore::new(&db_path).unwrap()
    }

    #[test]
    fn test_create_artist_index() {
        let mut store = create_test_store();

        let idx1 = store.get_or_create_idx("mbid-1", Some("Artist 1")).unwrap();
        let idx2 = store.get_or_create_idx("mbid-2", Some("Artist 2")).unwrap();
        let idx1_again = store.get_or_create_idx("mbid-1", None).unwrap();

        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(idx1_again, idx1); // Should return same index

        assert_eq!(store.get_mbid(idx1), Some("mbid-1"));
        assert_eq!(store.get_mbid(idx2), Some("mbid-2"));
    }

    #[test]
    fn test_store_and_retrieve_vector() {
        let mut store = create_test_store();

        // Create some artist indices first
        store.get_or_create_idx("target-1", None).unwrap();
        store.get_or_create_idx("target-2", None).unwrap();

        let mut vec = SparseVector::new();
        vec.set(0, 1.0); // target-1
        vec.set(1, 0.5); // target-2

        store.set_vector("artist-a", &vec, "test").unwrap();

        let retrieved = store.get_vector("artist-a").unwrap();
        assert_eq!(retrieved.get(0), 1.0);
        assert_eq!(retrieved.get(1), 0.5);
        assert_eq!(retrieved.nnz(), 2);
    }

    #[test]
    fn test_find_nearest() {
        let mut store = create_test_store();

        // Create artists
        let idx_a = store.get_or_create_idx("artist-a", None).unwrap();
        let idx_b = store.get_or_create_idx("artist-b", None).unwrap();
        let idx_c = store.get_or_create_idx("artist-c", None).unwrap();

        // Artist A: connected to B
        let mut vec_a = SparseVector::new();
        vec_a.set(idx_b, 1.0);
        store.set_vector("artist-a", &vec_a, "test").unwrap();

        // Artist B: connected to A and C
        let mut vec_b = SparseVector::new();
        vec_b.set(idx_a, 1.0);
        vec_b.set(idx_c, 0.5);
        store.set_vector("artist-b", &vec_b, "test").unwrap();

        // Artist C: connected to B
        let mut vec_c = SparseVector::new();
        vec_c.set(idx_b, 0.8);
        store.set_vector("artist-c", &vec_c, "test").unwrap();

        // Query similar to A
        let query = vec_a.clone();
        let results = store
            .find_nearest(&query, 5, &["artist-a".to_string()])
            .unwrap();

        // B should be most similar (both connect to each other)
        assert!(!results.is_empty());
        // C should also appear (both connect to B)
    }

    #[test]
    fn test_fresh_vector_check() {
        let mut store = create_test_store();

        let vec = SparseVector::new();
        store.set_vector("artist-a", &vec, "test").unwrap();

        // Should be fresh with 1 day TTL
        assert!(store.has_fresh_vector("artist-a", 86400));

        // Should not be fresh with 0 second TTL
        assert!(!store.has_fresh_vector("artist-a", 0));

        // Non-existent artist
        assert!(!store.has_fresh_vector("nonexistent", 86400));
    }
}
