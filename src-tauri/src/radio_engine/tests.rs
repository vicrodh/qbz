use super::db::{RadioDb, RadioSeed};
use super::engine::RadioEngine;

fn seed_artist_id() -> u64 {
    42
}

#[test]
fn radio_no_repetition() {
    let db = RadioDb::open_in_memory().unwrap();
    let session = db
        .create_session(
            RadioSeed::Artist {
                artist_id: seed_artist_id(),
            },
            123,
            5,
            25,
        )
        .unwrap();

    for track_id in 1u64..=120u64 {
        let artist_id = 1000 + (track_id % 10);
        let distance = if track_id % 7 == 0 { 2 } else if track_id % 3 == 0 { 1 } else { 0 };
        db.insert_pool_track(&session.id, track_id, artist_id, "test_pool", distance)
            .unwrap();
    }

    let engine = RadioEngine::new(db);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..120 {
        let t = engine.next_track(&session.id).unwrap();
        assert!(seen.insert(t.track_id), "Track repeated: {}", t.track_id);
    }
}

#[test]
fn radio_distance_constraint() {
    let db = RadioDb::open_in_memory().unwrap();
    let session = db
        .create_session(
            RadioSeed::Artist {
                artist_id: seed_artist_id(),
            },
            555,
            5,
            25,
        )
        .unwrap();

    for track_id in 1u64..=50u64 {
        db.insert_pool_track(&session.id, track_id, 1, "ok", 2).unwrap();
    }
    for track_id in 1001u64..=1020u64 {
        db.insert_pool_track_unchecked(&session.id, track_id, 2, "too_far", 3)
            .unwrap();
    }

    let engine = RadioEngine::new(db);
    for _ in 0..50 {
        let t = engine.next_track(&session.id).unwrap();
        assert!(t.distance <= 2, "Selected track with distance {}", t.distance);
    }
}
