<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Loader2, Play, Radio, ListPlus, ListEnd, Info } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Playlist {
    id: number;
    name: string;
    description?: string;
    owner: { id: number; name: string };
    images?: string[];
    tracks_count: number;
    tracks?: { items: Track[] };
  }

  interface Track {
    id: number;
    title: string;
    performer?: { id: number; name: string };
    album?: { id: string; title: string; image?: { large?: string; small?: string } };
    duration: number;
  }

  interface TracksContainer {
    items: Track[];
    total: number;
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
    onAddToQueue?: (type: 'playlist' | 'radio', id: number) => void;
    onPlayNext?: (type: 'playlist' | 'radio', id: number) => void;
  }

  let { trackId, artistId, artistName, trackName, currentArtwork, onPlayPlaylist, onPlayTrack, onAddToQueue, onPlayNext }: Props = $props();

  // State
  let artistPlaylists = $state<Playlist[]>([]);
  let recommendedTracks = $state<Track[]>([]);
  let loading = $state(false);
  let loadingRadio = $state(false);
  let error = $state<string | null>(null);
  let loadedArtistId = $state<number | null>(null);

  // Playlist cover artworks (fetched from tracks)
  let playlistCovers = $state<Map<number, string[]>>(new Map());

  // Tooltip state for radio info
  let showRadioTooltip = $state(false);

  // Format duration
  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  /**
   * Dedupe tracks by exact lowercase title, keeping the first occurrence.
   */
  function dedupeByExactTitle(tracks: Track[]): Track[] {
    const seen = new Set<string>();
    return tracks.filter(track => {
      const key = track.title.toLowerCase().trim();
      if (seen.has(key)) return false;
      seen.add(key);
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
        // Fetch playlist tracks for cover artworks (async, don't block)
        fetchPlaylistCovers(artistPlaylists);
      } else {
        artistPlaylists = [];
      }

      // Extract tracks for recommendations
      let tracks: Track[] = [];
      if (artist.tracks_appears_on?.items) {
        const filtered = artist.tracks_appears_on.items.filter(trk => trk.id !== trackId);
        tracks = dedupeByExactTitle(filtered);
      }

      // If sparse data, fallback to artist's popular tracks
      if (tracks.length < 5) {
        console.log(`[Suggestions] Sparse tracks_appears_on (${tracks.length}), fetching artist tracks...`);
        try {
          const artistTracks = await invoke<TracksContainer>('get_artist_tracks', {
            artistId: id,
            limit: 30,
            offset: 0
          });
          if (artistTracks.items) {
            const filtered = artistTracks.items.filter(trk => trk.id !== trackId);
            const deduped = dedupeByExactTitle(filtered);
            // Merge with existing, prioritizing new tracks
            const existingIds = new Set(tracks.map(trk => trk.id));
            for (const trk of deduped) {
              if (!existingIds.has(trk.id)) {
                tracks.push(trk);
              }
            }
          }
        } catch (e) {
          console.warn('[Suggestions] Failed to fetch artist tracks:', e);
        }
      }

      // Shuffle and take 10
      const shuffled = tracks.sort(() => Math.random() - 0.5);
      recommendedTracks = shuffled.slice(0, 10);

    } catch (e) {
      console.error('Failed to load suggestions:', e);
      error = 'Failed to load suggestions';
      artistPlaylists = [];
      recommendedTracks = [];
    } finally {
      loading = false;
    }
  }

  /**
   * Fetch playlist tracks to extract 3 unique album covers for collage
   */
  async function fetchPlaylistCovers(playlists: Playlist[]) {
    for (const playlist of playlists) {
      try {
        const fullPlaylist = await invoke<Playlist>('v2_get_playlist', { playlistId: playlist.id });

        // Extract unique album artworks from tracks
        const artworks: string[] = [];
        const seenAlbums = new Set<string>();

        if (fullPlaylist.tracks?.items) {
          for (const track of fullPlaylist.tracks.items) {
            const artwork = track.album?.image?.large || track.album?.image?.small;
            const albumId = track.album?.id;

            if (artwork && albumId && !seenAlbums.has(albumId)) {
              seenAlbums.add(albumId);
              artworks.push(artwork);
              if (artworks.length >= 3) break;
            }
          }
        }

        // Update state with fetched covers
        if (artworks.length > 0) {
          playlistCovers = new Map(playlistCovers).set(playlist.id, artworks);
        }
      } catch (e) {
        console.warn(`[Suggestions] Failed to fetch playlist ${playlist.id} tracks:`, e);
      }
    }
  }

  // Get unique artwork URLs for radio collage
  const radioCollageImages = $derived.by(() => {
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
      await invoke('v2_create_track_radio', {
        trackId,
        trackName: trackName || 'Unknown Track',
        artistId
      });
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
          {@const covers = playlistCovers.get(playlist.id) || []}
          {@const hasCovers = covers.length >= 3}
          {@const fallbackImage = playlist.images?.[Math.min(1, (playlist.images?.length || 1) - 1)]}
          <div class="card playlist-card">
            <div class="card-badge qobuz">
              <img src="/qobuz-logo-filled.svg" alt="Qobuz" class="badge-icon badge-qobuz" />
            </div>
            <div class="card-image-wrapper">
              {#if hasCovers}
                <!-- Book-style collage with 3 album covers -->
                <div class="book-collage">
                  <img src={covers[0]} alt="" class="book-cover left" />
                  <img src={covers[1]} alt="" class="book-cover center" />
                  <img src={covers[2]} alt="" class="book-cover right" />
                </div>
              {:else if covers.length > 0}
                <!-- Single cover while loading more -->
                <img src={covers[0]} alt="" class="card-image" />
              {:else if fallbackImage}
                <img src={fallbackImage} alt="" class="card-image" />
              {:else}
                <div class="card-image-placeholder">
                  <img src="/playlist.svg" alt="" class="placeholder-icon" />
                </div>
              {/if}
              <!-- Glyph that hides on hover -->
              <div class="card-glyph">
                <img src="/playlist.svg" alt="" class="glyph-icon" />
              </div>
              <!-- Hover overlay with actions -->
              <div class="card-overlay">
                <button class="overlay-btn secondary" onclick={() => onAddToQueue?.('playlist', playlist.id)} title="Add to queue">
                  <ListPlus size={16} />
                </button>
                <button class="overlay-btn primary" onclick={() => handlePlayPlaylist(playlist.id)} title="Play">
                  <Play size={20} fill="currentColor" />
                </button>
                <button class="overlay-btn secondary" onclick={() => onPlayNext?.('playlist', playlist.id)} title="Play next">
                  <ListEnd size={16} />
                </button>
              </div>
            </div>
            <div class="card-content">
              <span class="card-title">{playlist.name}</span>
              <span class="card-subtitle">Playlist · {playlist.tracks_count} tracks</span>
            </div>
          </div>
        {/each}

        <!-- Song Radio Card -->
        {#if trackId}
          <div class="card radio-card">
            <!-- Custom tooltip that opens bottom-right -->
            <div
              class="card-info-trigger"
              onmouseenter={() => showRadioTooltip = true}
              onmouseleave={() => showRadioTooltip = false}
            >
              <Info size={18} strokeWidth={2.5} />
              {#if showRadioTooltip}
                <div class="radio-tooltip">
                  {$t('player.radioExperimentalTooltip') || 'Radio is an experimental QBZ feature that generates a playlist based on the current track.'}
                </div>
              {/if}
            </div>
            <div class="card-badge qbz">
              <img src="/qbz-logo.svg" alt="QBZ" class="badge-icon badge-qbz" />
            </div>
            <div class="card-image-wrapper">
              <div class="radio-collage">
                {#if loadingRadio}
                  <div class="collage-loading">
                    <Loader2 size={32} class="spinner" />
                  </div>
                {:else if radioCollageImages.length >= 4}
                  <div class="collage-diamond">
                    <img src={radioCollageImages[0]} alt="" class="diamond-img top" />
                    <img src={radioCollageImages[1]} alt="" class="diamond-img left" />
                    <img src={radioCollageImages[2]} alt="" class="diamond-img right" />
                    <img src={radioCollageImages[3]} alt="" class="diamond-img bottom" />
                  </div>
                {:else if radioCollageImages.length > 0}
                  <img src={radioCollageImages[0]} alt="" class="collage-single" />
                {:else}
                  <div class="card-image-placeholder">
                    <Radio size={32} />
                  </div>
                {/if}
              </div>
              <!-- Glyph that hides on hover -->
              <div class="card-glyph">
                <Radio size={26} />
              </div>
              <!-- Hover overlay with play button only (radio replaces queue) -->
              <div class="card-overlay">
                <button class="overlay-btn primary" onclick={startSongRadio} disabled={loadingRadio} title="Play Radio">
                  <Play size={20} fill="currentColor" />
                </button>
              </div>
            </div>
            <div class="card-content">
              <span class="card-title">{$t('player.songRadio') || 'Song Radio'}</span>
              <span class="card-subtitle">{$t('player.basedOnTrack') || 'Based on this track'}</span>
            </div>
          </div>
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
    max-height: 80vh;
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
    padding-top: 4px; /* Space for hover effect */
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
    position: relative;
    text-align: left;
    transition: background 150ms ease, border-color 150ms ease;
  }

  .card:hover {
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
    border-color: var(--alpha-20, rgba(255, 255, 255, 0.2));
  }

  .card-badge {
    position: absolute;
    top: 8px;
    right: 8px;
    width: 24px;
    height: 24px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10; /* Above overlay */
    pointer-events: none; /* Don't block hover on card */
  }

  .badge-icon {
    object-fit: contain;
  }

  /* Qobuz icon: +2pt */
  .badge-qobuz {
    width: 22px;
    height: 22px;
  }

  /* QBZ icon: +2pt */
  .badge-qbz {
    width: 19px;
    height: 19px;
  }

  /* Image wrapper with hover effects */
  .card-image-wrapper {
    position: relative;
    width: 100%;
    aspect-ratio: 1;
    border-radius: 8px;
    overflow: hidden;
    transition: transform 150ms ease;
  }

  .card:hover .card-image-wrapper {
    transform: translateY(-4px);
  }

  .card-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .card-image-placeholder {
    width: 100%;
    height: 100%;
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
  }

  .placeholder-icon {
    width: 32px;
    height: 32px;
    opacity: 0.4;
    filter: invert(1);
  }

  /* Glyph overlay (visible by default, hides on hover) */
  .card-glyph {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.4);
    color: white;
    transition: opacity 150ms ease;
    z-index: 5;
  }

  /* Playlist SVG: 20x20 viewBox but content only fills ~77% - needs larger size to match Lucide Radio */
  .glyph-icon {
    width: 32px;
    height: 32px;
    filter: invert(1);
    opacity: 0.9;
  }

  .card:hover .card-glyph {
    opacity: 0;
  }

  /* Hover overlay with action buttons */
  .card-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    background: rgba(0, 0, 0, 0.6);
    opacity: 0;
    transition: opacity 150ms ease;
    z-index: 6;
  }

  .card:hover .card-overlay {
    opacity: 1;
  }

  .overlay-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 50%;
    cursor: pointer;
    color: white;
    transition: transform 100ms ease, background 100ms ease;
  }

  .overlay-btn:hover {
    transform: scale(1.1);
  }

  .overlay-btn:disabled {
    opacity: 0.5;
    cursor: wait;
  }

  .overlay-btn.primary {
    width: 40px;
    height: 40px;
    background: var(--accent-primary, #7c3aed);
  }

  .overlay-btn.primary:hover {
    background: var(--accent-hover, #6d28d9);
  }

  .overlay-btn.secondary {
    width: 32px;
    height: 32px;
    background: var(--alpha-30, rgba(255, 255, 255, 0.3));
  }

  .overlay-btn.secondary:hover {
    background: var(--alpha-50, rgba(255, 255, 255, 0.5));
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
  }

  /* Radio Collage */
  .radio-collage {
    width: 100%;
    height: 100%;
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

  /* Book-style collage for playlist cards (3 album covers) */
  .book-collage {
    width: 100%;
    height: 100%;
    position: relative;
    overflow: hidden;
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
  }

  .book-cover {
    position: absolute;
    width: 65%;
    height: 85%;
    object-fit: cover;
    border-radius: 4px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
    transition: transform 150ms ease;
  }

  .book-cover.left {
    left: -8%;
    top: 50%;
    transform: translateY(-50%) rotate(-12deg);
    z-index: 1;
  }

  .book-cover.center {
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    z-index: 3;
    width: 70%;
    height: 90%;
  }

  .book-cover.right {
    right: -8%;
    top: 50%;
    transform: translateY(-50%) rotate(12deg);
    z-index: 2;
  }

  .card:hover .book-cover.left {
    transform: translateY(-50%) rotate(-15deg) translateX(-2px);
  }

  .card:hover .book-cover.right {
    transform: translateY(-50%) rotate(15deg) translateX(2px);
  }

  /* Info trigger with custom tooltip */
  .card-info-trigger {
    position: absolute;
    top: 8px;
    left: 8px;
    z-index: 15;
    cursor: help;
    color: rgba(255, 255, 255, 0.7);
    transition: color 150ms ease;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.4);
  }

  .card-info-trigger:hover {
    color: white;
    background: rgba(0, 0, 0, 0.6);
  }

  /* Custom tooltip - opens bottom-right, high z-index */
  .radio-tooltip {
    position: absolute;
    top: calc(100% + 8px);
    left: 0;
    z-index: 1000;
    background: rgba(20, 20, 22, 0.95);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 8px;
    padding: 10px 12px;
    font-size: 12px;
    line-height: 1.4;
    color: rgba(255, 255, 255, 0.85);
    max-width: 220px;
    min-width: 180px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
    pointer-events: none;
    animation: tooltipFadeIn 150ms ease;
  }

  @keyframes tooltipFadeIn {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
</style>
