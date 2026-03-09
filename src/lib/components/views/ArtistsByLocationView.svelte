<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from '$lib/i18n';
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowLeft, Loader2, MapPin, Music } from 'lucide-svelte';
  import VirtualizedFavoritesArtistGrid from '../VirtualizedFavoritesArtistGrid.svelte';

  interface LocationContext {
    sourceArtistMbid: string;
    sourceArtistName: string;
    sourceArtistType: 'Person' | 'Group' | 'Other';
    location: {
      city?: string;
      areaId?: string;
      country?: string;
      displayName: string;
      precision: 'city' | 'state' | 'country';
    };
    affinitySeeds: {
      genres: string[];
      tags: string[];
      normalizedSeeds: string[];
    };
  }

  interface LocationCandidate {
    mbid: string;
    mb_name: string;
    qobuz_id?: number;
    qobuz_name?: string;
    qobuz_image?: string;
    score: number;
    genres: string[];
  }

  interface LocationDiscoveryResponse {
    artists: LocationCandidate[];
    scene_label: string;
    genre_summary: string;
    total_candidates: number;
    has_more: boolean;
  }

  interface FavoriteArtist {
    id: number;
    name: string;
    image?: { small?: string; thumbnail?: string; large?: string };
    albums_count?: number;
  }

  interface ArtistGroup {
    key: string;
    id: string;
    artists: FavoriteArtist[];
  }

  interface Props {
    context: LocationContext;
    onBack: () => void;
    onArtistClick: (artistId: number) => void;
  }

  let { context, onBack, onArtistClick }: Props = $props();

  let loading = $state(true);
  let error = $state<string | null>(null);
  let artists = $state<LocationCandidate[]>([]);
  let sceneLabel = $state('');
  let genreSummary = $state('');
  let totalCandidates = $state(0);
  let hasMore = $state(false);
  let loadingMore = $state(false);

  // Dynamic loading state
  let loadingStep = $state(0);
  let loadingStepTimer: ReturnType<typeof setInterval> | null = null;

  const loadingSteps = [
    () => {
      const genre = context.affinitySeeds.genres[0] || context.affinitySeeds.tags[0] || 'music';
      return $t('artist.sceneStep1', { values: { genre } });
    },
    () => {
      const genres = context.affinitySeeds.genres.slice(0, 3);
      const count = genres.length * 50;
      return $t('artist.sceneStep2', { values: { count: String(count) } });
    },
    () => $t('artist.sceneStep3'),
    () => $t('artist.sceneStep4'),
  ];

  function startLoadingAnimation() {
    loadingStep = 0;
    loadingStepTimer = setInterval(() => {
      if (loadingStep < loadingSteps.length - 1) {
        loadingStep++;
      }
    }, 3000);
  }

  function stopLoadingAnimation() {
    if (loadingStepTimer) {
      clearInterval(loadingStepTimer);
      loadingStepTimer = null;
    }
  }

  function candidatesToGroups(candidates: LocationCandidate[]): ArtistGroup[] {
    const validArtists: FavoriteArtist[] = candidates
      .filter((candidate) => candidate.qobuz_id != null)
      .map((candidate) => ({
        id: candidate.qobuz_id!,
        name: candidate.qobuz_name || candidate.mb_name,
        image: candidate.qobuz_image ? { small: candidate.qobuz_image } : undefined,
      }));

    if (validArtists.length === 0) return [];

    return [{
      key: '',
      id: 'scene-results',
      artists: validArtists,
    }];
  }

  let groups = $derived(candidatesToGroups(artists));

  async function discoverArtists(offset: number = 0) {
    try {
      const response: LocationDiscoveryResponse = await invoke('v2_discover_artists_by_location', {
        sourceMbid: context.sourceArtistMbid,
        areaId: context.location.areaId,
        areaName: context.location.city || context.location.displayName,
        country: context.location.country || null,
        genres: context.affinitySeeds.genres,
        tags: context.affinitySeeds.tags,
        limit: 30,
        offset,
      });

      if (offset === 0) {
        artists = response.artists;
      } else {
        artists = [...artists, ...response.artists];
      }
      sceneLabel = response.scene_label;
      genreSummary = response.genre_summary;
      totalCandidates = response.total_candidates;
      hasMore = response.has_more;
    } catch (err) {
      console.error('[ArtistsByLocationView] Discovery failed:', err);
      error = String(err);
    }
  }

  async function loadMore() {
    if (loadingMore || !hasMore) return;
    loadingMore = true;
    await discoverArtists(artists.length);
    loadingMore = false;
  }

  onMount(async () => {
    loading = true;
    error = null;
    startLoadingAnimation();
    await discoverArtists();
    stopLoadingAnimation();
    loading = false;
  });

  onDestroy(() => {
    stopLoadingAnimation();
  });
</script>

<div class="scene-view">
  <div class="scene-header">
    <button class="back-button" onclick={onBack} title={$t('actions.back')}>
      <ArrowLeft size={20} />
    </button>
    <div class="scene-header-info">
      <div class="scene-title">
        <MapPin size={18} />
        <span>
          {sceneLabel || $t('artist.sceneFrom', { values: { location: context.location.displayName } })}
        </span>
      </div>
      {#if genreSummary || context.affinitySeeds.genres.length > 0}
        <div class="scene-subtitle">
          {$t('artist.sceneBased', {
            values: {
              artist: context.sourceArtistName,
              genres: genreSummary || context.affinitySeeds.genres.slice(0, 3).join(' / '),
            },
          })}
        </div>
      {/if}
    </div>
  </div>

  <div class="scene-content">
    {#if loading}
      <div class="scene-loading">
        <div class="loading-visual">
          <div class="loading-pulse">
            <Music size={28} />
          </div>
        </div>
        <div class="loading-status">
          {#key loadingStep}
            <span class="loading-text fade-in">
              {loadingSteps[loadingStep]()}
            </span>
          {/key}
        </div>
        <div class="loading-dots">
          <span class="dot"></span>
          <span class="dot"></span>
          <span class="dot"></span>
        </div>
      </div>
    {:else if error}
      <div class="scene-error">
        <p>{error}</p>
        <button class="retry-button" onclick={() => { loading = true; error = null; startLoadingAnimation(); discoverArtists().then(() => { stopLoadingAnimation(); loading = false; }); }}>
          {$t('actions.retry')}
        </button>
      </div>
    {:else if groups.length === 0}
      <div class="scene-empty">
        <MapPin size={48} />
        <p>{$t('artist.noSceneResults')}</p>
      </div>
    {:else}
      <div class="scene-grid-container">
        <VirtualizedFavoritesArtistGrid
          {groups}
          showGroupHeaders={false}
          {onArtistClick}
        />
      </div>

      {#if hasMore}
        <div class="load-more-container">
          <button class="load-more-button" onclick={loadMore} disabled={loadingMore}>
            {#if loadingMore}
              <Loader2 size={16} class="spin" />
            {/if}
            <span>
              {$t('actions.loadMore')}
              ({artists.length} / {totalCandidates})
            </span>
          </button>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .scene-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .scene-header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 0 20px 0;
    flex-shrink: 0;
  }

  .back-button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border-radius: 8px;
    border: none;
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
    transition: background-color 150ms ease;
    flex-shrink: 0;
  }

  .back-button:hover {
    background: var(--bg-tertiary);
  }

  .scene-header-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .scene-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 22px;
    font-weight: 700;
    color: var(--text-primary);
    line-height: 1.2;
  }

  .scene-subtitle {
    font-size: 13px;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .scene-content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  /* Loading state with dynamic messages */
  .scene-loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 20px;
    padding: 100px 0;
  }

  .loading-visual {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .loading-pulse {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    background: var(--bg-secondary);
    color: var(--accent-primary);
    animation: pulse 2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% {
      transform: scale(1);
      opacity: 0.8;
    }
    50% {
      transform: scale(1.08);
      opacity: 1;
    }
  }

  .loading-status {
    min-height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .loading-text {
    font-size: 14px;
    color: var(--text-secondary);
    text-align: center;
    line-height: 1.4;
  }

  .fade-in {
    animation: fadeIn 400ms ease-out;
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .loading-dots {
    display: flex;
    gap: 6px;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-muted);
    animation: dotBounce 1.4s ease-in-out infinite;
  }

  .dot:nth-child(2) {
    animation-delay: 0.2s;
  }

  .dot:nth-child(3) {
    animation-delay: 0.4s;
  }

  @keyframes dotBounce {
    0%, 80%, 100% {
      opacity: 0.3;
      transform: scale(0.8);
    }
    40% {
      opacity: 1;
      transform: scale(1);
    }
  }

  .scene-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 80px 0;
    color: var(--text-muted);
    font-size: 14px;
  }

  .retry-button {
    padding: 8px 20px;
    border-radius: 8px;
    border: 1px solid var(--border-primary);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    transition: background-color 150ms ease;
  }

  .retry-button:hover {
    background: var(--bg-tertiary);
  }

  .scene-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    padding: 80px 0;
    color: var(--text-muted);
    font-size: 14px;
  }

  .scene-grid-container {
    flex: 1;
    min-height: 0;
  }

  .load-more-container {
    display: flex;
    justify-content: center;
    padding: 16px 0 8px;
    flex-shrink: 0;
  }

  .load-more-button {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 24px;
    border-radius: 8px;
    border: 1px solid var(--border-primary);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    transition: background-color 150ms ease;
  }

  .load-more-button:hover:not(:disabled) {
    background: var(--bg-tertiary);
  }

  .load-more-button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  :global(.load-more-button .spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
