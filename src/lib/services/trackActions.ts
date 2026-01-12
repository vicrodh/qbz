/**
 * Track Actions Service
 *
 * Centralizes common track actions that can be used by any component.
 * Reduces prop drilling by providing direct access to track operations.
 */

import { invoke } from '@tauri-apps/api/core';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import {
  addToQueueNext,
  addToQueue,
  type BackendQueueTrack
} from '$lib/stores/queueStore';
import { addTrackToFavorites } from '$lib/services/playbackService';
import { openPlaylistModal } from '$lib/stores/uiStore';
import { showToast as storeShowToast, type ToastType } from '$lib/stores/toastStore';
import type { QobuzTrack, Track, PlaylistTrack, LocalLibraryTrack } from '$lib/types';

// ============ Toast Integration ============

function showToast(message: string, type: ToastType): void {
  storeShowToast(message, type);
}

// ============ Queue Builders ============

export function buildQueueTrackFromQobuz(track: QobuzTrack): BackendQueueTrack {
  const artwork = track.album?.image?.large || track.album?.image?.thumbnail || track.album?.image?.small || '';
  return {
    id: track.id,
    title: track.title,
    artist: track.performer?.name || 'Unknown Artist',
    album: track.album?.title || '',
    duration_secs: track.duration,
    artwork_url: artwork || null,
    hires: track.hires_streamable ?? false,
    bit_depth: track.maximum_bit_depth ?? null,
    sample_rate: track.maximum_sampling_rate ?? null
  };
}

export function buildQueueTrackFromAlbumTrack(track: Track, albumArtwork: string, albumArtist: string, albumTitle: string): BackendQueueTrack {
  return {
    id: track.id,
    title: track.title,
    artist: track.artist || albumArtist || 'Unknown Artist',
    album: albumTitle || '',
    duration_secs: track.durationSeconds,
    artwork_url: albumArtwork || null,
    hires: track.hires ?? false,
    bit_depth: track.bitDepth ?? null,
    sample_rate: track.samplingRate ?? null
  };
}

export function buildQueueTrackFromPlaylistTrack(track: PlaylistTrack): BackendQueueTrack {
  return {
    id: track.id,
    title: track.title,
    artist: track.artist || 'Unknown Artist',
    album: track.album || 'Playlist',
    duration_secs: track.durationSeconds,
    artwork_url: track.albumArt || null,
    hires: track.hires ?? false,
    bit_depth: track.bitDepth ?? null,
    sample_rate: track.samplingRate ?? null
  };
}

export function buildQueueTrackFromLocalTrack(track: LocalLibraryTrack): BackendQueueTrack {
  const artwork = track.artwork_path ? `asset://localhost/${encodeURIComponent(track.artwork_path)}` : null;
  // Local tracks are hi-res if bit_depth > 16 or sample_rate > 44100
  const isHires = Boolean((track.bit_depth && track.bit_depth > 16) || (track.sample_rate && track.sample_rate > 44100));
  return {
    id: track.id,
    title: track.title,
    artist: track.artist,
    album: track.album,
    duration_secs: track.duration_secs,
    artwork_url: artwork,
    hires: isHires,
    bit_depth: track.bit_depth ?? null,
    sample_rate: track.sample_rate ?? null
  };
}

// ============ Queue Actions ============

export async function queueTrackNext(queueTrack: BackendQueueTrack, isLocal = false): Promise<void> {
  const success = await addToQueueNext(queueTrack, isLocal);
  if (success) {
    showToast('Queued to play next', 'success');
  } else {
    showToast('Failed to queue track', 'error');
  }
}

export async function queueTrackLater(queueTrack: BackendQueueTrack, isLocal = false): Promise<void> {
  const success = await addToQueue(queueTrack, isLocal);
  if (success) {
    showToast('Added to queue', 'success');
  } else {
    showToast('Failed to add to queue', 'error');
  }
}

// ============ Convenience Queue Functions ============

export async function queueQobuzTrackNext(track: QobuzTrack): Promise<void> {
  await queueTrackNext(buildQueueTrackFromQobuz(track));
}

export async function queueQobuzTrackLater(track: QobuzTrack): Promise<void> {
  await queueTrackLater(buildQueueTrackFromQobuz(track));
}

export async function queuePlaylistTrackNext(track: PlaylistTrack): Promise<void> {
  await queueTrackNext(buildQueueTrackFromPlaylistTrack(track));
}

export async function queuePlaylistTrackLater(track: PlaylistTrack): Promise<void> {
  await queueTrackLater(buildQueueTrackFromPlaylistTrack(track));
}

export async function queueLocalTrackNext(track: LocalLibraryTrack): Promise<void> {
  await queueTrackNext(buildQueueTrackFromLocalTrack(track), true);
}

export async function queueLocalTrackLater(track: LocalLibraryTrack): Promise<void> {
  await queueTrackLater(buildQueueTrackFromLocalTrack(track), true);
}

// ============ Favorites ============

export async function handleAddToFavorites(trackId: number): Promise<void> {
  const success = await addTrackToFavorites(trackId);
  if (success) {
    showToast('Added to favorites', 'success');
  } else {
    showToast('Failed to add to favorites', 'error');
  }
}

// ============ Playlist Actions ============

export function addToPlaylist(trackIds: number[]): void {
  openPlaylistModal('addTrack', trackIds);
}

// ============ Sharing ============

async function copyToClipboard(text: string, successMessage: string): Promise<void> {
  try {
    await writeText(text);
    showToast(successMessage, 'success');
  } catch (err) {
    console.error('Failed to copy to clipboard:', err);
    showToast('Failed to copy link', 'error');
  }
}

export async function shareQobuzTrackLink(trackId: number): Promise<void> {
  try {
    const url = await invoke<string>('get_qobuz_track_url', { trackId });
    await copyToClipboard(url, 'Qobuz link copied');
  } catch (err) {
    console.error('Failed to get Qobuz link:', err);
    showToast(`Failed to share Qobuz link: ${err}`, 'error');
  }
}

interface SongLinkResponse {
  pageUrl: string;
  title?: string;
  artist?: string;
  thumbnailUrl?: string;
  platforms: Record<string, string>;
  identifier: string;
  contentType: string;
}

export async function shareSonglinkTrack(trackId: number, isrc?: string): Promise<void> {
  const qobuzUrl = `https://www.qobuz.com/track/${trackId}`;
  const resolvedIsrc = isrc?.trim();
  try {
    showToast('Fetching Song.link...', 'info');
    const response = await invoke<SongLinkResponse>('share_track_songlink', {
      isrc: resolvedIsrc?.length ? resolvedIsrc : null,
      url: qobuzUrl,
      trackId
    });
    await copyToClipboard(response.pageUrl, 'Song.link copied');
  } catch (err) {
    console.error('Failed to get Song.link:', err);
    showToast(`Song.link error: ${err}`, 'error');
  }
}
