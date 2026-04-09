/**
 * Command Router
 *
 * Routes commands to either Tauri invoke() or remote HTTP API based on
 * the current playback target. This is the single point where local vs
 * remote bifurcation happens — stores and services call these functions
 * instead of invoke() directly for playback/queue operations.
 *
 * Only playback, queue, and favorites operations route remotely.
 * Everything else (settings, library, integrations) always stays local.
 */
import { invoke } from '@tauri-apps/api/core';
import { getTarget } from '$lib/stores/playbackTargetStore';
import { remotePost, remoteGet } from '$lib/services/remoteApi';

// ==================== Playback ====================

export async function cmdPause(): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/playback/pause');
  } else {
    await invoke('v2_pause_playback');
  }
}

export async function cmdResume(): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/playback/play');
  } else {
    await invoke('v2_resume_playback');
  }
}

export async function cmdStop(): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/playback/stop');
  } else {
    await invoke('v2_stop_playback');
  }
}

export async function cmdSeek(position: number): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/playback/seek', { position_secs: Math.floor(position) });
  } else {
    await invoke('v2_seek', { position: Math.floor(position) });
  }
}

export async function cmdSetVolume(volume: number): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/playback/volume', { volume });
  } else {
    await invoke('v2_set_volume', { volume });
  }
}

export async function cmdNext(): Promise<unknown> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    return remotePost('/api/playback/next');
  } else {
    return invoke('v2_next_track');
  }
}

export async function cmdPrevious(): Promise<unknown> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    return remotePost('/api/playback/previous');
  } else {
    return invoke('v2_previous_track');
  }
}

export async function cmdPlayTrack(
  trackId: number,
  quality?: string,
  lowQualityFallback?: unknown,
): Promise<unknown> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    return remotePost('/api/playback/play-track', { track_id: trackId, quality });
  } else {
    return invoke('v2_play_track', { trackId, qualityStr: quality, lowQualityFallback: lowQualityFallback ?? null });
  }
}

// ==================== Queue ====================

export async function cmdSetQueue(tracks: unknown[], startIndex: number): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/queue/set', { tracks, start_index: startIndex });
  } else {
    await invoke('v2_set_queue', { tracks, startIndex });
  }
}

export async function cmdAddToQueue(track: unknown): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/queue/add', { tracks: [track] });
  } else {
    await invoke('v2_add_to_queue', { track });
  }
}

export async function cmdAddToQueueNext(track: unknown): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/queue/add-next', { tracks: [track] });
  } else {
    await invoke('v2_add_to_queue_next', { track });
  }
}

export async function cmdAddTracksToQueue(tracks: unknown[]): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/queue/add', { tracks });
  } else {
    await invoke('v2_add_tracks_to_queue', { tracks });
  }
}

export async function cmdAddTracksToQueueNext(tracks: unknown[]): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/queue/add-next', { tracks });
  } else {
    await invoke('v2_add_tracks_to_queue_next', { tracks });
  }
}

export async function cmdClearQueue(): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/queue/clear');
  } else {
    await invoke('v2_clear_queue');
  }
}

export async function cmdToggleShuffle(): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    // Remote: get current state and toggle
    const queue = await remoteGet<{ shuffle: boolean }>('/api/queue');
    await remotePost('/api/queue/shuffle', { enabled: !queue.shuffle });
  } else {
    await invoke('v2_toggle_shuffle');
  }
}

export async function cmdSetRepeatMode(mode: string): Promise<void> {
  const target = getTarget();
  if (target.type === 'qbzd') {
    await remotePost('/api/queue/repeat', { mode });
  } else {
    // Map frontend mode names to V2 command format
    const v2Mode = mode === 'one' ? 'One' : mode === 'all' ? 'All' : 'Off';
    await invoke('v2_set_repeat_mode', { mode: v2Mode });
  }
}
