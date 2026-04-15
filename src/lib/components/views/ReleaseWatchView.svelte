<script lang="ts">
  /**
   * ReleaseWatchView — 1:1 port of the Qobuz mobile "Radar de Novedades"
   * screen (see rr0/i.java in the v9.7.0.3 decompilation). Three tabs —
   * Artists / Labels / Awards — each backed by
   * `/favorite/getNewReleases?type=<tab>`. Mobile fetches one tab at a
   * time on tab select; we do the same.
   */
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import { ChevronLeft, User, Disc3, Award, LoaderCircle } from 'lucide-svelte';
  import VirtualizedFavoritesAlbumGrid from '../VirtualizedFavoritesAlbumGrid.svelte';
  import type { QobuzAlbum } from '$lib/types';

  interface FavoriteAlbum {
    id: string;
    title: string;
    artist: { id: number; name: string };
    genre?: { name: string };
    image: { small?: string; thumbnail?: string; large?: string };
    release_date_original?: string;
    hires: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
  }

  type TabType = 'artists' | 'labels' | 'awards';

  interface Props {
    onBack: () => void;
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
    onAlbumPlayNext?: (albumId: string) => void;
    onAlbumPlayLater?: (albumId: string) => void;
    onAlbumShareQobuz?: (albumId: string) => void;
    onAlbumShareSonglink?: (albumId: string) => void;
    onAlbumDownload?: (albumId: string) => void;
    onOpenAlbumFolder?: (albumId: string) => void;
    onReDownloadAlbum?: (albumId: string) => void;
    onAddAlbumToPlaylist?: (albumId: string) => void;
    checkAlbumFullyDownloaded?: (albumId: string) => Promise<boolean>;
    downloadStateVersion?: number;
    onArtistClick?: (artistId: number) => void;
  }

  let {
    onBack,
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onOpenAlbumFolder,
    onReDownloadAlbum,
    onAddAlbumToPlaylist,
    checkAlbumFullyDownloaded,
    downloadStateVersion,
  }: Props = $props();

  const PAGE_SIZE = 50;
  const MAX_PER_TAB = 150; // matches mobile's hard limit

  let activeTab = $state<TabType>('artists');
  let albumsByTab = $state<Record<TabType, FavoriteAlbum[]>>({ artists: [], labels: [], awards: [] });
  let loadedTabs = new Set<TabType>();
  let loadingTab = $state<TabType | null>(null);
  let errorByTab = $state<Record<TabType, string | null>>({ artists: null, labels: null, awards: null });
  let albumDownloadStatuses = $state<Map<string, boolean>>(new Map());

  function qobuzToFavorite(item: QobuzAlbum): FavoriteAlbum {
    const bitDepth = item.maximum_bit_depth ?? 16;
    return {
      id: item.id,
      title: item.title,
      artist: {
        id: item.artist?.id ?? 0,
        name: item.artist?.name || 'Unknown Artist',
      },
      genre: item.genre,
      image: item.image as { small?: string; thumbnail?: string; large?: string },
      release_date_original: item.release_date_original,
      hires: bitDepth > 16,
      maximum_bit_depth: item.maximum_bit_depth,
      maximum_sampling_rate: item.maximum_sampling_rate,
    };
  }

  async function fetchTab(tab: TabType) {
    if (loadedTabs.has(tab)) return;
    loadingTab = tab;
    errorByTab = { ...errorByTab, [tab]: null };
    try {
      const result = await invoke<{ items: QobuzAlbum[]; total: number }>(
        'v2_get_release_watch',
        { releaseType: tab, limit: MAX_PER_TAB, offset: 0 }
      );
      albumsByTab = {
        ...albumsByTab,
        [tab]: (result.items || []).map(qobuzToFavorite),
      };
      loadedTabs.add(tab);
      loadDownloadStatuses(albumsByTab[tab]).catch(() => {});
    } catch (err) {
      console.error(`[ReleaseWatch] failed to load ${tab}:`, err);
      errorByTab = { ...errorByTab, [tab]: String(err) };
    } finally {
      if (loadingTab === tab) loadingTab = null;
    }
  }

  async function loadDownloadStatuses(list: FavoriteAlbum[]) {
    if (!checkAlbumFullyDownloaded) return;
    for (const album of list) {
      if (albumDownloadStatuses.has(album.id)) continue;
      try {
        const downloaded = await checkAlbumFullyDownloaded(album.id);
        albumDownloadStatuses.set(album.id, downloaded);
      } catch {
        // ignore
      }
    }
    albumDownloadStatuses = new Map(albumDownloadStatuses);
  }

  function isAlbumDownloaded(albumId: string): boolean {
    void downloadStateVersion;
    return albumDownloadStatuses.get(albumId) ?? false;
  }

  function selectTab(tab: TabType) {
    activeTab = tab;
    if (!loadedTabs.has(tab)) fetchTab(tab);
  }

  function getQualityLabel(item: { hires?: boolean; maximum_bit_depth?: number; maximum_sampling_rate?: number }): string {
    if (!item.hires) return $t('quality.cdQuality');
    const depth = item.maximum_bit_depth ?? 16;
    const rate = item.maximum_sampling_rate ?? 44.1;
    return `${depth}/${rate}kHz`;
  }

  function getAlbumYear(album: FavoriteAlbum): string | null {
    if (!album.release_date_original) return null;
    return album.release_date_original.substring(0, 4);
  }

  onMount(() => {
    fetchTab('artists');
  });

  const tabs: { key: TabType; label: string; icon: typeof User }[] = [
    { key: 'artists', label: 'releaseWatch.tabs.artists', icon: User },
    { key: 'labels', label: 'releaseWatch.tabs.labels', icon: Disc3 },
    { key: 'awards', label: 'releaseWatch.tabs.awards', icon: Award },
  ];
</script>

<div class="release-watch">
  <div class="top-bar">
    <button class="back-btn" onclick={onBack} title={$t('actions.back')}>
      <ChevronLeft size={20} />
    </button>
    <div class="header-text">
      <h1 class="page-title">{$t('home.releaseWatch')}</h1>
      <p class="page-subtitle">{$t('discover.releaseWatch.subtitle')}</p>
    </div>
  </div>

  <div class="tabs">
    {#each tabs as tab}
      {@const Icon = tab.icon}
      <button
        class="tab"
        class:active={activeTab === tab.key}
        onclick={() => selectTab(tab.key)}
      >
        <Icon size={16} />
        <span>{$t(tab.label)}</span>
        {#if albumsByTab[tab.key].length > 0}
          <span class="tab-count">{albumsByTab[tab.key].length}</span>
        {/if}
      </button>
    {/each}
  </div>

  <div class="content">
    {#if loadingTab === activeTab && albumsByTab[activeTab].length === 0}
      <div class="loading-state">
        <LoaderCircle size={28} class="spinner" />
        <p>{$t('favorites.loadingFavorites')}</p>
      </div>
    {:else if errorByTab[activeTab]}
      <div class="empty-state">
        <p>{$t('favorites.failedLoadFavorites')}</p>
        <p class="error-detail">{errorByTab[activeTab]}</p>
        <button class="retry-btn" onclick={() => { loadedTabs.delete(activeTab); fetchTab(activeTab); }}>
          {$t('actions.retry')}
        </button>
      </div>
    {:else if albumsByTab[activeTab].length === 0}
      <div class="empty-state">
        <p>{$t(`releaseWatch.empty.${activeTab}`)}</p>
      </div>
    {:else}
      <VirtualizedFavoritesAlbumGrid
        groups={[{ key: '', id: 'all', albums: albumsByTab[activeTab] }]}
        showGroupHeaders={false}
        viewMode="grid"
        onAlbumClick={onAlbumClick}
        onAlbumPlay={onAlbumPlay}
        onAlbumPlayNext={onAlbumPlayNext}
        onAlbumPlayLater={onAlbumPlayLater}
        onAlbumShareQobuz={onAlbumShareQobuz}
        onAlbumShareSonglink={onAlbumShareSonglink}
        onAlbumDownload={onAlbumDownload}
        onOpenAlbumFolder={onOpenAlbumFolder}
        onReDownloadAlbum={onReDownloadAlbum}
        onAddAlbumToPlaylist={onAddAlbumToPlaylist}
        {downloadStateVersion}
        {isAlbumDownloaded}
        {getQualityLabel}
        {getAlbumYear}
      />
    {/if}
  </div>
</div>

<style>
  .release-watch {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .top-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 20px 24px 8px 24px;
    flex-shrink: 0;
  }

  .back-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
  }

  .back-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .header-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .page-title {
    font-size: 24px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .page-subtitle {
    font-size: 13px;
    color: var(--text-secondary);
    margin: 0;
  }

  .tabs {
    display: flex;
    gap: 4px;
    padding: 0 24px;
    margin-bottom: 16px;
    border-bottom: 1px solid var(--border-primary);
    flex-shrink: 0;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-secondary);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: color 150ms ease, border-color 150ms ease;
  }

  .tab:hover:not(.active) {
    color: var(--text-primary);
  }

  .tab.active {
    color: var(--accent-primary);
    border-bottom-color: var(--accent-primary);
  }

  .tab-count {
    font-size: 11px;
    font-weight: 600;
    padding: 2px 6px;
    border-radius: 999px;
    background: var(--bg-tertiary);
    color: var(--text-muted);
  }

  .tab.active .tab-count {
    background: color-mix(in srgb, var(--accent-primary) 15%, transparent);
    color: var(--accent-primary);
  }

  .content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    padding: 0 24px;
  }

  .loading-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 48px 24px;
    color: var(--text-secondary);
    text-align: center;
  }

  .error-detail {
    font-size: 12px;
    color: var(--text-muted);
  }

  .retry-btn {
    margin-top: 8px;
    padding: 8px 16px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .retry-btn:hover {
    background: var(--bg-secondary);
  }
</style>
