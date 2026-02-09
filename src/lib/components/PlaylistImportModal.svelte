<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { X, CloudOff } from 'lucide-svelte';
  import { showToast } from '$lib/stores/toastStore';
  import { t } from '$lib/i18n';
  import {
    subscribe as subscribeOffline,
    isOffline as checkIsOffline
  } from '$lib/stores/offlineStore';
  import {
    getVisibleFolders,
    movePlaylistToFolder,
    loadFolders,
    subscribe as subscribeFolders
  } from '$lib/stores/playlistFoldersStore';
  import type { PlaylistFolder } from '$lib/stores/playlistFoldersStore';

  type ProviderKey = 'spotify' | 'apple' | 'tidal' | 'deezer';

  interface ImportTrack {
    title: string;
    artist: string;
    album?: string | null;
    duration_ms?: number | null;
    isrc?: string | null;
  }

  interface ImportPlaylist {
    provider: 'Spotify' | 'AppleMusic' | 'Tidal' | 'Deezer';
    name: string;
    tracks: ImportTrack[];
  }

  interface ImportSummary {
    provider: 'Spotify' | 'AppleMusic' | 'Tidal' | 'Deezer';
    playlist_name: string;
    total_tracks: number;
    matched_tracks: number;
    skipped_tracks: number;
    qobuz_playlist_id?: number | null;
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    onSuccess?: (summary: ImportSummary) => void;
  }

  let { isOpen, onClose, onSuccess }: Props = $props();

  let url = $state('');
  let loading = $state(false);
  let error = $state<string | null>(null);
  let summary = $state<ImportSummary | null>(null);
  let lockedProvider = $state<ProviderKey | null>(null);
  let logEntries = $state<{ message: string; status: 'info' | 'success' | 'error' }[]>([]);
  let isOffline = $state(checkIsOffline());
  let importCompleted = $state(false);
  let lastImportedUrl = $state('');
  let importProgress = $state<{ phase: string; current: number; total: number; matched_so_far: number; current_track: string | null } | null>(null);
  let importPhase = $state<string | null>(null);

  // Preview + customization state
  let preview = $state<ImportPlaylist | null>(null);
  let previewUrl = $state('');
  let customName = $state('');
  let selectedFolderId = $state('');
  let availableFolders = $state<PlaylistFolder[]>([]);

  // Whether to show the preview customization panel
  const showPreview = $derived(preview !== null && url.trim() === previewUrl);

  // Subscribe to offline state changes
  $effect(() => {
    const unsubscribe = subscribeOffline(() => {
      isOffline = checkIsOffline();
    });
    return unsubscribe;
  });

  // Load folders and subscribe to changes
  $effect(() => {
    loadFolders();
    const unsubscribe = subscribeFolders(() => {
      availableFolders = getVisibleFolders();
    });
    return unsubscribe;
  });

  const providers: { key: ProviderKey; label: string; logo: string; color: string }[] = [
    { key: 'spotify', label: 'Spotify', logo: '/spotify-logo.svg', color: '#1DB954' },
    { key: 'apple', label: 'Apple Music', logo: '/apple-music-logo.svg', color: '#fa233b' },
    { key: 'tidal', label: 'Tidal', logo: '/tidal-tidal.svg', color: '#ffffff' },
    { key: 'deezer', label: 'Deezer', logo: '/deezer-logo.svg', color: '#00c7f2' }
  ];

  const detectedProvider = $derived(detectProvider(url));
  const activeProvider = $derived(lockedProvider ?? detectedProvider);
  const isValid = $derived(!!detectedProvider && !isOffline);

  $effect(() => {
    if (isOpen) {
      url = '';
      loading = false;
      error = null;
      summary = null;
      lockedProvider = null;
      logEntries = [];
      importCompleted = false;
      lastImportedUrl = '';
      importProgress = null;
      importPhase = null;
      preview = null;
      previewUrl = '';
      customName = '';
      selectedFolderId = '';
      availableFolders = getVisibleFolders();
    }
  });

  $effect(() => {
    if (!importCompleted) return;
    const trimmed = url.trim();
    if (trimmed !== lastImportedUrl) {
      error = null;
      summary = null;
      lockedProvider = null;
      logEntries = [];
      importCompleted = false;
    }
  });

  function detectProvider(value: string): ProviderKey | null {
    const trimmed = value.trim();
    if (!trimmed) return null;
    if (
      trimmed.startsWith('spotify:playlist:') ||
      trimmed.includes('open.spotify.com/playlist/') ||
      trimmed.includes('open.spotify.com/embed/playlist/')
    ) {
      return 'spotify';
    }
    if (trimmed.includes('music.apple.com/') && trimmed.includes('/playlist/')) {
      return 'apple';
    }
    if (trimmed.includes('tidal.com/') && trimmed.includes('/playlist/')) {
      return 'tidal';
    }
    if (trimmed.includes('deezer.com/') && trimmed.includes('/playlist/')) {
      return 'deezer';
    }
    return null;
  }

  function pushLog(message: string, status: 'info' | 'success' | 'error' = 'info') {
    logEntries = [...logEntries, { message, status }];
  }

  async function handlePreview() {
    if (!isValid || loading) return;

    loading = true;
    error = null;
    summary = null;
    lockedProvider = detectedProvider;
    logEntries = [];
    preview = null;

    try {
      pushLog('Checking playlist link...');
      const result = await invoke<ImportPlaylist>('playlist_import_preview', { url });
      preview = result;
      previewUrl = url.trim();
      customName = result.name;
      pushLog(
        $t('playlistImport.tracksFound', {
          values: { count: result.tracks.length, provider: formatProvider(result.provider) }
        }),
        'success'
      );
    } catch (err) {
      error = String(err);
      pushLog(`Import failed: ${error}`, 'error');
    } finally {
      loading = false;
    }
  }

  async function handleExecute() {
    if (!preview || loading) return;

    loading = true;
    error = null;
    importProgress = null;
    importPhase = null;

    let unlistenProgress: UnlistenFn | null = null;
    let unlistenPhase: UnlistenFn | null = null;
    let lastLoggedPercent = -1;

    try {
      // Set up event listeners before invoking
      unlistenProgress = await listen<{ phase: string; current: number; total: number; matched_so_far: number; current_track: string | null }>('import:progress', (event) => {
        importProgress = event.payload;

        if (event.payload.phase === 'matching' && event.payload.total > 0) {
          const pct = Math.floor((event.payload.current / event.payload.total) * 100);
          // Log at every 5% milestone
          if (pct >= lastLoggedPercent + 5) {
            lastLoggedPercent = pct;
            pushLog(
              $t('playlistImport.matchingTracks', {
                values: {
                  current: event.payload.current.toLocaleString(),
                  total: event.payload.total.toLocaleString(),
                  matched: event.payload.matched_so_far.toLocaleString()
                }
              })
            );
          }
        } else if (event.payload.phase === 'adding') {
          pushLog(
            $t('playlistImport.addingProgress', {
              values: {
                current: event.payload.current,
                total: event.payload.total
              }
            })
          );
        }
      });

      unlistenPhase = await listen<{ phase: string }>('import:phase', (event) => {
        importPhase = event.payload.phase;

        if (event.payload.phase === 'matching') {
          pushLog($t('playlistImport.matchingPhase'));
        } else if (event.payload.phase === 'creating') {
          pushLog($t('playlistImport.creatingPhase'), 'success');
        } else if (event.payload.phase === 'adding') {
          pushLog($t('playlistImport.addingPhase'));
        }
      });

      const nameOverride = customName.trim() !== preview.name ? customName.trim() || null : null;
      const result = await invoke<ImportSummary>('playlist_import_execute', {
        url: previewUrl,
        nameOverride,
        isPublic: false
      });

      summary = result;
      importCompleted = true;
      lastImportedUrl = previewUrl;
      pushLog(`Imported ${result.matched_tracks} of ${result.total_tracks} tracks into QBZ.`, 'success');

      if (result.qobuz_playlist_id) {
        pushLog('Playlist created in Qobuz\u2122.', 'success');

        // Move to folder if selected
        if (selectedFolderId) {
          await movePlaylistToFolder(result.qobuz_playlist_id, selectedFolderId);
        }
      } else {
        pushLog('No matching tracks found.', 'error');
      }

      onSuccess?.(result);
      if (result.matched_tracks > 0) {
        showToast($t('toast.playlistImported'), 'success');
      }
    } catch (err) {
      error = String(err);
      pushLog(`Import failed: ${error}`, 'error');
      showToast($t('toast.playlistImportFailed'), 'error');
    } finally {
      loading = false;
      unlistenProgress?.();
      unlistenPhase?.();
    }
  }

  function formatProvider(provider: ImportPlaylist['provider'] | ImportSummary['provider']): string {
    switch (provider) {
      case 'AppleMusic':
        return 'Apple Music';
      case 'Spotify':
        return 'Spotify';
      case 'Tidal':
        return 'Tidal';
      case 'Deezer':
        return 'Deezer';
      default:
        return 'Unknown';
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose();
    } else if (e.key === 'Enter' && !e.shiftKey) {
      if (showPreview && !importCompleted) {
        handleExecute();
      } else if (!showPreview) {
        handlePreview();
      }
    }
  }
</script>

{#if isOpen}
  <div
    class="modal-overlay"
    onclick={onClose}
    onkeydown={handleKeydown}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <div class="header-title">
          <img src="/qobuz-logo.svg" alt="Qobuz\u2122" class="qobuz-logo" />
          <h2>{$t('playlistImport.title')}</h2>
        </div>
        <button class="close-btn" onclick={onClose}>
          <X size={20} />
        </button>
      </div>

      <div class="modal-body">
        {#if isOffline}
          <div class="offline-warning" role="alert" aria-live="polite">
            <CloudOff size={16} aria-hidden="true" />
            <span>{$t('offline.featureDisabled')}</span>
          </div>
        {/if}

        {#if error}
          <div class="error-message">{error}</div>
        {/if}

        <div class="form-group">
          <label for="playlist-url">{$t('playlistImport.urlLabel')}</label>
          <input
            id="playlist-url"
            type="text"
            bind:value={url}
            placeholder="https://open.spotify.com/playlist/..."
            disabled={loading}
          />
        </div>

        <div class="sources">
          <span class="sources-label">{$t('playlistImport.allowedSources')}</span>
          <div class="sources-logos">
            {#each providers as provider}
              <div class="provider" data-provider={provider.key}>
                <img
                  src={provider.logo}
                  alt={provider.label}
                  class:active={activeProvider === provider.key}
                  class="provider-logo"
                  style={`--provider-color: ${provider.color}`}
                />
              </div>
            {/each}
          </div>
        </div>

        {#if showPreview && preview}
          <div class="customization">
            <div class="form-group">
              <label for="playlist-name">{$t('playlistImport.playlistName')}</label>
              <input
                id="playlist-name"
                type="text"
                bind:value={customName}
                disabled={loading || importCompleted}
              />
            </div>

            {#if availableFolders.length > 0}
              <div class="form-group">
                <label for="playlist-folder">{$t('playlistImport.folder')}</label>
                <select
                  id="playlist-folder"
                  bind:value={selectedFolderId}
                  disabled={loading || importCompleted}
                >
                  <option value="">{$t('playlistImport.noFolder')}</option>
                  {#each availableFolders as folder}
                    <option value={folder.id}>{folder.name}</option>
                  {/each}
                </select>
              </div>
            {/if}
          </div>
        {/if}

        {#if logEntries.length > 0}
          <div class="progress-panel">
            <div class="progress-header">
              <span>{$t('playlistImport.progress')}</span>
              {#if loading}
                <span class="spinner" aria-hidden="true"></span>
              {/if}
            </div>
            {#if importProgress && importProgress.total > 0 && loading}
              <div class="progress-bar-container">
                <div
                  class="progress-bar-fill"
                  style="width: {Math.min(100, (importProgress.current / importProgress.total) * 100)}%"
                ></div>
              </div>
              <div class="progress-status">
                {$t('playlistImport.matchingTracks', {
                  values: {
                    current: importProgress.current.toLocaleString(),
                    total: importProgress.total.toLocaleString(),
                    matched: importProgress.matched_so_far.toLocaleString()
                  }
                })}
              </div>
              {#if importProgress.current_track}
                <div class="progress-current-track">{importProgress.current_track}</div>
              {/if}
            {/if}
            <ul class="progress-log">
              {#each logEntries as entry}
                <li class={`log-item ${entry.status}`}>{entry.message}</li>
              {/each}
            </ul>
            {#if summary}
              <div class="summary">
                <div class="summary-title">{$t('playlistImport.summary')}</div>
                <div class="summary-row">{$t('playlistImport.playlistLabel', { values: { name: summary.playlist_name } })}</div>
                <div class="summary-row">{$t('playlistImport.tracksMatched', { values: { matched: summary.matched_tracks, total: summary.total_tracks } })}</div>
                <div class="summary-row">{$t('playlistImport.skipped', { values: { count: summary.skipped_tracks } })}</div>
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <div class="modal-footer">
        <button class="btn btn-secondary" onclick={onClose} disabled={loading}>
          {$t('actions.close')}
        </button>
        {#if !showPreview}
          <button class="btn btn-primary" onclick={handlePreview} disabled={!isValid || loading}>
            {#if loading}
              {$t('playlistImport.fetching')}
            {:else}
              {$t('playlistImport.fetchPlaylist')}
            {/if}
          </button>
        {:else}
          <button class="btn btn-primary" onclick={handleExecute} disabled={loading || importCompleted}>
            {#if loading}
              {$t('playlistImport.importing')}
            {:else}
              {$t('playlistImport.importAction')}
            {/if}
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }

  .modal {
    width: 100%;
    max-width: 560px;
    min-height: 420px;
    max-height: 90vh;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    border-radius: 16px;
    border: 1px solid var(--bg-tertiary);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 20px 24px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .header-title {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .header-title h2 {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .qobuz-logo {
    width: 26px;
    height: 26px;
    object-fit: contain;
    filter: brightness(0) invert(1);
    opacity: 0.9;
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    transition: color 150ms ease;
  }

  .close-btn:hover {
    color: var(--text-primary);
  }

  .modal-body {
    padding: 24px;
    overflow-y: auto;
  }

  .offline-warning {
    display: flex;
    align-items: center;
    gap: 8px;
    background: rgba(234, 179, 8, 0.1);
    border: 1px solid rgba(234, 179, 8, 0.3);
    color: #eab308;
    padding: 12px;
    border-radius: 8px;
    font-size: 13px;
    margin-bottom: 16px;
  }

  .error-message {
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    color: #ef4444;
    padding: 12px;
    border-radius: 8px;
    font-size: 13px;
    margin-bottom: 16px;
  }

  .form-group {
    margin-bottom: 12px;
  }

  .form-group label {
    display: block;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    margin-bottom: 8px;
  }

  .form-group input[type="text"],
  .form-group select {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 14px;
    color: var(--text-primary);
    transition: border-color 150ms ease;
  }

  .form-group input[type="text"]:focus,
  .form-group select:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .form-group select {
    cursor: pointer;
    appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%23999' stroke-width='2'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 12px center;
    padding-right: 32px;
  }

  .form-group select option {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .customization {
    padding: 16px;
    border-radius: 12px;
    background: rgba(0, 0, 0, 0.15);
    border: 1px solid var(--alpha-8);
    margin-bottom: 12px;
  }

  .customization .form-group:last-child {
    margin-bottom: 0;
  }

  .sources {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-bottom: 16px;
  }

  .sources-label {
    font-size: 12px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .sources-logos {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .provider-logo {
    width: 70px;
    height: 24px;
    object-fit: contain;
    filter: grayscale(1) brightness(0.7);
    opacity: 0.5;
    transition: filter 200ms ease, opacity 200ms ease, transform 200ms ease, box-shadow 200ms ease;
  }

  .provider-logo.active {
    filter: drop-shadow(0 6px 14px var(--provider-color));
    opacity: 1;
    transform: translateY(-1px) scale(1.02);
  }

  /* Tidal logo is black, invert it when active for visibility */
  .provider[data-provider="tidal"] .provider-logo.active {
    filter: brightness(0) invert(1) drop-shadow(0 6px 14px var(--alpha-50));
  }

  .progress-panel {
    margin-top: 12px;
    padding: 16px;
    border-radius: 12px;
    background: rgba(0, 0, 0, 0.2);
    border: 1px solid var(--alpha-8);
  }

  .progress-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 10px;
  }

  .progress-bar-container {
    width: 100%;
    height: 6px;
    background: var(--alpha-8);
    border-radius: 3px;
    overflow: hidden;
    margin-bottom: 8px;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--accent-primary);
    border-radius: 3px;
    transition: width 200ms ease;
  }

  .progress-status {
    font-size: 12px;
    color: var(--text-secondary);
    margin-bottom: 4px;
  }

  .progress-current-track {
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-bottom: 10px;
  }

  .progress-log {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .log-item {
    font-size: 13px;
    color: var(--text-muted);
  }

  .log-item.success {
    color: #34d399;
  }

  .log-item.error {
    color: #f87171;
  }

  .summary {
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--alpha-8);
  }

  .summary-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 6px;
  }

  .summary-row {
    font-size: 12px;
    color: var(--text-muted);
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    padding: 16px 24px 20px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .spinner {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid var(--alpha-20);
    border-top-color: var(--accent-primary);
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (max-width: 720px) {
    .modal {
      max-width: calc(100% - 24px);
    }

    .sources-logos {
      gap: 12px;
    }

    .provider-logo {
      width: 56px;
      height: 20px;
    }
  }
</style>
