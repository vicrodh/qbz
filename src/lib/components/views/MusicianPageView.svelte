<script lang="ts">
  /**
   * Musician Page View
   *
   * Full page view for musicians with confidence level 2 (Contextual).
   * Shows musician info and "Appears On" albums from Qobuz.
   *
   * This page:
   * - Is rendered only for confidence >= 2
   * - Shows bands/projects from MusicBrainz (if available)
   * - Shows "Appears On" albums from Qobuz ONLY
   * - Does NOT invent discographies
   * - Does NOT show raw search results
   */
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowLeft, User, Music, Disc } from 'lucide-svelte';
  import type { ResolvedMusician, AlbumAppearance, MusicianAppearances } from '$lib/types';
  import { onMount } from 'svelte';

  interface Props {
    musician: ResolvedMusician;
    onBack: () => void;
    onAlbumClick: (albumId: string) => void;
    onArtistClick?: (artistId: number) => void;
  }

  let { musician, onBack, onAlbumClick, onArtistClick }: Props = $props();

  // State
  let appearances = $state<AlbumAppearance[]>([]);
  let totalAppearances = $state(0);
  let isLoading = $state(true);
  let isLoadingMore = $state(false);
  let error = $state<string | null>(null);

  const ITEMS_PER_PAGE = 20;
  let offset = $state(0);

  const hasMore = $derived(appearances.length < totalAppearances);

  onMount(() => {
    loadAppearances();
  });

  async function loadAppearances() {
    isLoading = true;
    error = null;

    try {
      const result = await invoke<MusicianAppearances>('get_musician_appearances', {
        name: musician.name,
        role: musician.role,
        limit: ITEMS_PER_PAGE,
        offset: 0
      });

      appearances = result.albums;
      totalAppearances = result.total;
      offset = result.albums.length;
    } catch (err) {
      console.error('Failed to load musician appearances:', err);
      error = 'Failed to load appearances';
    } finally {
      isLoading = false;
    }
  }

  async function loadMore() {
    if (isLoadingMore || !hasMore) return;

    isLoadingMore = true;

    try {
      const result = await invoke<MusicianAppearances>('get_musician_appearances', {
        name: musician.name,
        role: musician.role,
        limit: ITEMS_PER_PAGE,
        offset
      });

      appearances = [...appearances, ...result.albums];
      offset += result.albums.length;
    } catch (err) {
      console.error('Failed to load more appearances:', err);
    } finally {
      isLoadingMore = false;
    }
  }
</script>

<div class="musician-page">
  <!-- Header -->
  <header class="page-header">
    <button class="back-btn" onclick={onBack} title="Go back">
      <ArrowLeft size={20} />
    </button>

    <div class="musician-header">
      <div class="musician-avatar">
        <User size={32} />
      </div>
      <div class="musician-info">
        <h1>{musician.name}</h1>
        <span class="role">{musician.role}</span>
      </div>
    </div>
  </header>

  <div class="page-content">
    <!-- Bands & Projects Section -->
    {#if musician.bands.length > 0}
      <section class="section">
        <h2 class="section-title">Bands & Projects</h2>
        <div class="bands-grid">
          {#each musician.bands as band}
            <button
              class="band-card"
              onclick={() => {
                // TODO: Search for band as artist
                console.log('Search for band:', band);
              }}
            >
              <div class="band-icon">
                <Music size={16} />
              </div>
              <span class="band-name">{band}</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}

    <!-- Appears On Section -->
    <section class="section">
      <h2 class="section-title">
        Appears On
        {#if totalAppearances > 0}
          <span class="count">({totalAppearances})</span>
        {/if}
      </h2>

      {#if isLoading}
        <div class="loading-state">
          <span>Loading appearances...</span>
        </div>
      {:else if error}
        <div class="error-state">
          <span>{error}</span>
        </div>
      {:else if appearances.length === 0}
        <div class="empty-state">
          <Disc size={32} />
          <span>No album appearances found</span>
        </div>
      {:else}
        <div class="albums-grid">
          {#each appearances as album}
            <button
              class="album-card"
              onclick={() => onAlbumClick(album.album_id)}
            >
              {#if album.album_artwork}
                <img
                  src={album.album_artwork}
                  alt={album.album_title}
                  class="album-artwork"
                  loading="lazy"
                />
              {:else}
                <div class="album-artwork placeholder">
                  <Disc size={24} />
                </div>
              {/if}
              <div class="album-info">
                <span class="album-title" title={album.album_title}>
                  {album.album_title}
                </span>
                <span class="album-artist" title={album.artist_name}>
                  {album.artist_name}
                </span>
                <div class="album-meta">
                  {#if album.year}
                    <span class="year">{album.year}</span>
                  {/if}
                  <span class="role-badge">{album.role_on_album}</span>
                </div>
              </div>
            </button>
          {/each}
        </div>

        {#if hasMore}
          <div class="load-more">
            <button
              class="load-more-btn"
              onclick={loadMore}
              disabled={isLoadingMore}
            >
              {isLoadingMore ? 'Loading...' : 'Load More'}
            </button>
          </div>
        {/if}
      {/if}
    </section>
  </div>
</div>

<style>
  .musician-page {
    width: 100%;
    height: 100%;
    overflow-y: auto;
  }

  .page-header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 24px 24px 16px;
    border-bottom: 1px solid var(--bg-tertiary);
    position: sticky;
    top: 0;
    background: var(--bg-primary);
    z-index: 10;
  }

  .back-btn {
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 8px;
    color: var(--text-primary);
    cursor: pointer;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .back-btn:hover {
    background: var(--bg-hover);
  }

  .musician-header {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .musician-avatar {
    width: 56px;
    height: 56px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    border-radius: 50%;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .musician-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .musician-info h1 {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
    line-height: 1.2;
  }

  .role {
    font-size: 14px;
    color: var(--text-muted);
    text-transform: capitalize;
  }

  .page-content {
    padding: 24px;
  }

  .section {
    margin-bottom: 32px;
  }

  .section:last-child {
    margin-bottom: 0;
  }

  .section-title {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 16px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .section-title .count {
    font-size: 14px;
    font-weight: 400;
    color: var(--text-muted);
  }

  /* Bands Grid */
  .bands-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .band-card {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 20px;
    color: var(--text-primary);
    font-size: 14px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .band-card:hover {
    background: var(--bg-hover);
  }

  .band-icon {
    color: var(--text-muted);
  }

  .band-name {
    font-weight: 500;
  }

  /* Albums Grid */
  .albums-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 20px;
  }

  .album-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 0;
    background: none;
    border: none;
    text-align: left;
    cursor: pointer;
    transition: transform 150ms ease;
  }

  .album-card:hover {
    transform: translateY(-4px);
  }

  .album-artwork {
    width: 100%;
    aspect-ratio: 1;
    object-fit: cover;
    border-radius: 8px;
    background: var(--bg-tertiary);
  }

  .album-artwork.placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .album-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .album-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .album-artist {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .album-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
  }

  .year {
    font-size: 11px;
    color: var(--text-muted);
  }

  .role-badge {
    font-size: 10px;
    padding: 2px 8px;
    background: var(--bg-tertiary);
    border-radius: 10px;
    color: var(--text-secondary);
    text-transform: capitalize;
  }

  /* States */
  .loading-state,
  .error-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 48px;
    color: var(--text-muted);
    font-size: 14px;
  }

  /* Load More */
  .load-more {
    display: flex;
    justify-content: center;
    padding: 24px 0;
  }

  .load-more-btn {
    padding: 10px 24px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 8px;
    color: var(--text-primary);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .load-more-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .load-more-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
