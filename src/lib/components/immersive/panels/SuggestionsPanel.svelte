<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Loader2, Play, Radio, AlertCircle, ListMusic } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Playlist {
    id: number;
    name: string;
    description?: string;
    owner: { id: number; name: string };
    images?: string[];
    tracks_count: number;
  }

  interface Track {
    id: number;
    title: string;
    performer?: { id: number; name: string };
    album?: { id: string; title: string; image?: { large?: string } };
    duration: number;
  }

  interface ArtistDetail {
    id: number;
    name: string;
    playlists?: Playlist[];
    tracks_appears_on?: { items: Track[] };
  }

  interface Props {
    trackId?: number;
    artistId?: number;
    artistName?: string;
    trackName?: string;
    currentArtwork?: string;
    onPlayPlaylist?: (playlistId: number) => void;
    onPlayTrack?: (trackId: number) => void;
  }

  let { trackId, artistId, artistName, trackName, currentArtwork, onPlayPlaylist, onPlayTrack }: Props = $props();

  // State
  let artistPlaylists = $state<Playlist[]>([]);
  let recommendedTracks = $state<Track[]>([]);
  let loading = $state(false);
  let loadingRadio = $state(false);
  let error = $state<string | null>(null);
  let loadedArtistId = $state<number | null>(null);

  // Format duration
  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  /**
   * Normalize track title to detect duplicates (different remasters, editions, etc.)
   * "Song Title (2023 Remaster)" -> "song title"
   * "Song Title [Deluxe Edition]" -> "song title"
   */
  function normalizeTitle(title: string): string {
    return title
      .toLowerCase()
      .replace(/\s*[\(\[][^\)\]]*(?:remaster|edition|version|mix|remix|live|mono|stereo|deluxe|bonus|anniversary).*?[\)\]]/gi, '')
      .replace(/\s*-\s*(?:remaster|edition|version|mix|remix|live|mono|stereo|deluxe|bonus|anniversary).*$/gi, '')
      .trim();
  }

  /**
   * Dedupe tracks by normalized title, keeping the first occurrence
   */
  function dedupeByTitle(tracks: Track[]): Track[] {
    const seen = new Set<string>();
    return tracks.filter(track => {
      const normalized = normalizeTitle(track.title);
      if (seen.has(normalized)) return false;
      seen.add(normalized);
      return true;
    });
  }

  // Load data when artistId changes
  $effect(() => {
    if (artistId && artistId !== loadedArtistId) {
      loadSuggestions(artistId);
    } else if (!artistId && loadedArtistId) {
      artistPlaylists = [];
      recommendedTracks = [];
      loadedArtistId = null;
    }
  });

  async function loadSuggestions(id: number) {
    loading = true;
    error = null;

    try {
      // Get artist detail which includes playlists and tracks
      const artist = await invoke<ArtistDetail>('get_artist_detail', { artistId: id });
      loadedArtistId = id;

      // Extract curated playlists (max 2)
      if (artist.playlists) {
        artistPlaylists = artist.playlists.slice(0, 2);
      } else {
        artistPlaylists = [];
      }

      // Extract tracks for recommendations
      if (artist.tracks_appears_on?.items) {
        // Filter current track, dedupe by title, shuffle, and take 10
        const filtered = artist.tracks_appears_on.items.filter(t => t.id !== trackId);
        const deduped = dedupeByTitle(filtered);
        const shuffled = deduped.sort(() => Math.random() - 0.5);
        recommendedTracks = shuffled.slice(0, 10);
      } else {
        recommendedTracks = [];
      }
    } catch (e) {
      console.error('Failed to load suggestions:', e);
      error = 'Failed to load suggestions';
      artistPlaylists = [];
      recommendedTracks = [];
    } finally {
      loading = false;
    }
  }

  // Get unique artwork URLs for radio collage (from recommended tracks)
  const radioCollageImages = $derived(() => {
    const images: string[] = [];
    if (currentArtwork) images.push(currentArtwork);
    for (const track of recommendedTracks) {
      if (track.album?.image?.large && !images.includes(track.album.image.large)) {
        images.push(track.album.image.large);
      }
      if (images.length >= 4) break;
    }
    return images;
  });

  async function startSongRadio() {
    if (!trackId || !artistId || loadingRadio) return;

    loadingRadio = true;
    try {
      await invoke('create_track_radio', {
        trackId,
        trackName: trackName || 'Unknown Track',
        artistId
      });
      // Radio will auto-start playing via queue update
    } catch (e) {
      console.error('Failed to start song radio:', e);
      error = 'Failed to start radio';
    } finally {
      loadingRadio = false;
    }
  }

  function handlePlayPlaylist(playlistId: number) {
    onPlayPlaylist?.(playlistId);
  }

  function handlePlayTrack(id: number) {
    onPlayTrack?.(id);
  }

  const isLoading = $derived(loading && !artistPlaylists.length && !recommendedTracks.length);
  const hasContent = $derived(artistPlaylists.length > 0 || recommendedTracks.length > 0 || trackId);
</script>

<div class="suggestions-panel">
  {#if isLoading}
    <div class="loading-state">
      <Loader2 size={28} class="spinner" />
    </div>
  {:else if !artistId}
    <div class="empty-state">
      <Radio size={32} strokeWidth={1.5} />
      <span>{$t('player.noTrackSelected') || 'No track selected'}</span>
    </div>
  {:else}
    <div class="suggestions-content">
      <!-- Top Cards Section -->
      <div class="cards-section">
        <!-- Artist Playlists from Qobuz -->
        {#each artistPlaylists as playlist (playlist.id)}
            <button
              class="card playlist-card"
              onclick={() => handlePlayPlaylist(playlist.id)}
              title={playlist.description || playlist.name}
            >
              <div class="card-badge qobuz">
                <img src="/qobuz-logo-filled.svg" alt="Qobuz" class="badge-icon" />
              </div>
              {#if playlist.images?.[0]}
                <img src={playlist.images[0]} alt="" class="card-image" />
              {:else}
                <div class="card-image-placeholder">
                  <ListMusic size={32} />
                </div>
              {/if}
              <div class="card-content">
                <span class="card-title">{playlist.name}</span>
                <span class="card-subtitle">
                  <ListMusic size={10} />
                  Playlist · {playlist.tracks_count} tracks
                </span>
              </div>
              <div class="card-play">
                <Play size={16} fill="currentColor" />
              </div>
            </button>
        {/each}

        <!-- Song Radio Card -->
        {#if trackId}
          <button
            class="card radio-card"
            onclick={startSongRadio}
            disabled={loadingRadio}
            title={$t('player.radioExperimental') || 'Song Radio - Experimental QBZ feature'}
          >
            <div class="card-badge qbz">
              <img src="/qbz-logo.svg" alt="QBZ" class="badge-icon qbz-icon" />
            </div>
            <div class="radio-collage">
              {#if loadingRadio}
                <div class="collage-loading">
                  <Loader2 size={32} class="spinner" />
                </div>
              {:else}
                {@const images = radioCollageImages()}
                {#if images.length >= 4}
                  <!-- Diamond/rotated collage layout -->
                  <div class="collage-diamond">
                    <img src={images[0]} alt="" class="diamond-img top" />
                    <img src={images[1]} alt="" class="diamond-img left" />
                    <img src={images[2]} alt="" class="diamond-img right" />
                    <img src={images[3]} alt="" class="diamond-img bottom" />
                  </div>
                {:else if images.length > 0}
                  <img src={images[0]} alt="" class="collage-single" />
                  <div class="collage-radio-overlay">
                    <Radio size={24} />
                  </div>
                {:else}
                  <div class="card-icon-bg">
                    <Radio size={32} />
                  </div>
                {/if}
              {/if}
            </div>
            <div class="card-content">
              <span class="card-title">{$t('player.songRadio') || 'Song Radio'}</span>
              <span class="card-subtitle">
                <Radio size={10} />
                {$t('player.basedOnTrack') || 'Based on this track'}
              </span>
            </div>
            <div class="card-info" title={$t('player.radioExperimentalTooltip') || 'Radio is an experimental QBZ feature that generates a playlist based on the current track.'}>
              <AlertCircle size={14} />
            </div>
          </button>
        {/if}
      </div>

      <!-- Recommended Tracks Section -->
      {#if recommendedTracks.length > 0}
        <div class="tracks-section">
          <h3 class="section-title">{$t('player.recommendedTracks') || 'Recommended'}</h3>
          <div class="tracks-list">
              {#each recommendedTracks as track (track.id)}
                <button
                  class="track-item"
                  onclick={() => handlePlayTrack(track.id)}
                >
                  <div class="track-artwork">
                    {#if track.album?.image?.large}
                      <img src={track.album.image.large} alt="" />
                    {:else}
                      <div class="artwork-placeholder"></div>
                    {/if}
                    <div class="track-play-overlay">
                      <Play size={14} fill="currentColor" />
                    </div>
                  </div>
                  <div class="track-info">
                    <span class="track-title">{track.title}</span>
                    <span class="track-artist">{track.performer?.name || 'Unknown'}</span>
                  </div>
                  <span class="track-duration">{formatDuration(track.duration)}</span>
                </button>
              {/each}
            </div>
        </div>
      {/if}

      {#if error}
        <div class="error-message">{error}</div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .suggestions-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .loading-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    height: 100%;
    min-height: 200px;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    font-size: 14px;
  }

  .loading-state :global(.spinner),
  .radio-card :global(.spinner) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .suggestions-content {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 24px;
    padding-right: 8px;
  }

  .suggestions-content::-webkit-scrollbar {
    width: 4px;
  }

  .suggestions-content::-webkit-scrollbar-thumb {
    background: var(--alpha-20, rgba(255, 255, 255, 0.2));
    border-radius: 2px;
  }

  /* Cards Section */
  .cards-section {
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
  }

  .card {
    flex: 1;
    min-width: 140px;
    max-width: 180px;
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
    border: 1px solid var(--alpha-10, rgba(255, 255, 255, 0.1));
    border-radius: 12px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    cursor: pointer;
    transition: all 150ms ease;
    position: relative;
    text-align: left;
  }

  .card:hover {
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
    border-color: var(--alpha-20, rgba(255, 255, 255, 0.2));
    transform: translateY(-2px);
  }

  .card:disabled {
    opacity: 0.6;
    cursor: wait;
  }

  .card-badge {
    position: absolute;
    top: 8px;
    right: 8px;
    width: 20px;
    height: 20px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    font-weight: 700;
  }

  .card-badge.qobuz {
    background: #1a1a2e;
  }

  .card-badge.qbz {
    background: var(--accent-primary, #7c3aed);
  }

  .badge-icon {
    width: 14px;
    height: 14px;
  }

  .qbz-logo {
    color: white;
    font-weight: 800;
  }

  .card-image {
    width: 100%;
    aspect-ratio: 1;
    border-radius: 8px;
    object-fit: cover;
  }

  .card-image-placeholder,
  .card-icon-bg {
    width: 100%;
    aspect-ratio: 1;
    border-radius: 8px;
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
  }

  .card-content {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .card-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary, white);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .card-subtitle {
    font-size: 11px;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .card-play {
    position: absolute;
    bottom: 12px;
    right: 12px;
    width: 28px;
    height: 28px;
    background: var(--accent-primary, #7c3aed);
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: white;
    opacity: 0;
    transform: scale(0.8);
    transition: all 150ms ease;
  }

  .card:hover .card-play {
    opacity: 1;
    transform: scale(1);
  }

  .card-info {
    position: absolute;
    bottom: 12px;
    right: 12px;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
    cursor: help;
  }

  .card-info:hover {
    color: var(--alpha-70, rgba(255, 255, 255, 0.7));
  }

  /* QBZ Badge Icon */
  .qbz-icon {
    width: 16px;
    height: 16px;
    border-radius: 2px;
  }

  /* Radio Collage */
  .radio-collage {
    width: 100%;
    aspect-ratio: 1;
    border-radius: 8px;
    overflow: hidden;
    position: relative;
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
  }

  .collage-loading {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
  }

  .collage-single {
    width: 100%;
    height: 100%;
    object-fit: cover;
    filter: brightness(0.7);
  }

  .collage-radio-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: white;
    background: rgba(0, 0, 0, 0.3);
  }

  /* Diamond collage - rotated squares pattern */
  .collage-diamond {
    width: 100%;
    height: 100%;
    position: relative;
    overflow: hidden;
  }

  .diamond-img {
    position: absolute;
    width: 70%;
    height: 70%;
    object-fit: cover;
    border-radius: 4px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
  }

  .diamond-img.top {
    top: -10%;
    left: 50%;
    transform: translateX(-50%) rotate(5deg);
    z-index: 4;
  }

  .diamond-img.left {
    top: 50%;
    left: -15%;
    transform: translateY(-50%) rotate(-8deg);
    z-index: 2;
  }

  .diamond-img.right {
    top: 50%;
    right: -15%;
    transform: translateY(-50%) rotate(8deg);
    z-index: 3;
  }

  .diamond-img.bottom {
    bottom: -10%;
    left: 50%;
    transform: translateX(-50%) rotate(-3deg);
    z-index: 1;
  }

  /* Tracks Section */
  .tracks-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .section-title {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    margin: 0 0 12px 0;
  }

  .tracks-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow-y: auto;
  }

  .track-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px;
    background: none;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    transition: background 150ms ease;
    text-align: left;
  }

  .track-item:hover {
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
  }

  .track-artwork {
    position: relative;
    width: 40px;
    height: 40px;
    border-radius: 4px;
    overflow: hidden;
    flex-shrink: 0;
  }

  .track-artwork img,
  .artwork-placeholder {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artwork-placeholder {
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
  }

  .track-play-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    color: white;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .track-item:hover .track-play-overlay {
    opacity: 1;
  }

  .track-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .track-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary, white);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-artist {
    font-size: 12px;
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-duration {
    font-size: 12px;
    font-family: var(--font-mono, monospace);
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    flex-shrink: 0;
  }

  .error-message {
    padding: 12px;
    background: rgba(239, 68, 68, 0.1);
    border-radius: 8px;
    color: #ef4444;
    font-size: 13px;
    text-align: center;
  }
</style>
