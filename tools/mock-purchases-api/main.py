"""Mock Purchases API — Qobuz-shaped responses for UI prototyping."""

import asyncio
import json
import time

from fastapi import FastAPI, Query
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import StreamingResponse

from data import MOCK_ALBUMS, MOCK_INDIVIDUAL_TRACKS, MOCK_FORMATS

app = FastAPI(title="QBZ Mock Purchases API")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:1420", "http://localhost:1421"],
    allow_methods=["*"],
    allow_headers=["*"],
)

ALBUMS_BY_ID = {a["id"]: a for a in MOCK_ALBUMS}


def paginate(items: list, limit: int, offset: int) -> dict:
    return {
        "offset": offset,
        "limit": limit,
        "total": len(items),
        "items": items[offset : offset + limit],
    }


@app.get("/purchases")
def get_purchases(limit: int = Query(50, ge=1, le=500), offset: int = Query(0, ge=0)):
    return {
        "albums": paginate(MOCK_ALBUMS, limit, offset),
        "tracks": paginate(MOCK_INDIVIDUAL_TRACKS, limit, offset),
    }


@app.get("/purchases/search")
def search_purchases(q: str = Query("", min_length=0)):
    query = q.lower()
    if not query:
        return {
            "albums": paginate(MOCK_ALBUMS, 50, 0),
            "tracks": paginate(MOCK_INDIVIDUAL_TRACKS, 50, 0),
        }

    matched_albums = [
        a
        for a in MOCK_ALBUMS
        if query in a["title"].lower() or query in a["artist"]["name"].lower()
    ]
    matched_tracks = [
        t
        for t in MOCK_INDIVIDUAL_TRACKS
        if query in t["title"].lower() or query in t["performer"]["name"].lower()
    ]
    return {
        "albums": paginate(matched_albums, 50, 0),
        "tracks": paginate(matched_tracks, 50, 0),
    }


@app.get("/purchases/album/{album_id}")
def get_album_detail(album_id: str):
    album = ALBUMS_BY_ID.get(album_id)
    if not album:
        return {"error": "Album not found"}, 404
    return album


@app.get("/purchases/formats/{album_id}")
def get_formats(album_id: str):
    album = ALBUMS_BY_ID.get(album_id)
    if not album:
        return MOCK_FORMATS
    max_depth = album.get("maximum_bit_depth", 16)
    max_rate = album.get("maximum_sampling_rate", 44.1)
    return [
        f
        for f in MOCK_FORMATS
        if (f["bit_depth"] is None)
        or (f["bit_depth"] <= max_depth and f["sampling_rate"] <= max_rate)
    ]


async def _download_progress_generator(total_tracks: int, entity_id: str, is_album: bool):
    """SSE stream simulating download progress."""
    for idx in range(total_tracks):
        total_bytes = 50_000_000 + idx * 10_000_000
        # Simulate progress in 4 steps per track
        for step in range(4):
            progress = {
                "albumId" if is_album else "trackId": entity_id,
                "trackIndex": idx,
                "totalTracks": total_tracks,
                "bytesDownloaded": int(total_bytes * (step + 1) / 4),
                "totalBytes": total_bytes,
                "status": "downloading" if step < 3 else "complete",
            }
            yield f"data: {json.dumps(progress)}\n\n"
            await asyncio.sleep(0.3)

    # Final event
    yield f"data: {json.dumps({'status': 'complete', 'totalTracks': total_tracks})}\n\n"


@app.post("/purchases/download/album/{album_id}")
async def download_album(album_id: str):
    album = ALBUMS_BY_ID.get(album_id)
    if not album:
        return {"error": "Album not found"}, 404
    if not album.get("downloadable", True):
        return {"error": "Album not available for download"}, 403

    total = album.get("tracks_count", 1)
    return StreamingResponse(
        _download_progress_generator(total, album_id, is_album=True),
        media_type="text/event-stream",
    )


@app.post("/purchases/download/track/{track_id}")
async def download_track(track_id: int):
    return StreamingResponse(
        _download_progress_generator(1, str(track_id), is_album=False),
        media_type="text/event-stream",
    )
