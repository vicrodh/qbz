<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { X, CloudOff } from 'lucide-svelte';
  import { showToast } from '$lib/stores/toastStore';
  import { t } from '$lib/i18n';
  import QobuzLegalNotice from '$lib/components/QobuzLegalNotice.svelte';
  import { qobuzTosAccepted } from '$lib/stores/qobuzLegalStore';
  import {
    subscribe as subscribeOffline,
    isOffline as checkIsOffline
  } from '$lib/stores/offlineStore';

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

  // Subscribe to offline state changes
  $effect(() => {
    const unsubscribe = subscribeOffline(() => {
      isOffline = checkIsOffline();
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
  const isValid = $derived(!!detectedProvider && !isOffline && $qobuzTosAccepted);

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

  async function handleImport() {
    if (!isValid || loading) return;

    loading = true;
    error = null;
    summary = null;
    lockedProvider = detectedProvider;
    logEntries = [];

    try {
      pushLog('Checking playlist link...');
      const preview = await invoke<ImportPlaylist>('playlist_import_preview', { url });
      pushLog(`Found ${preview.tracks.length} tracks from ${formatProvider(preview.provider)}.`);

      pushLog('Matching tracks in Qobuz™...');
      const result = await invoke<ImportSummary>('playlist_import_execute', {
        url,
        nameOverride: null,
        isPublic: false
      });

      summary = result;
      importCompleted = true;
      lastImportedUrl = url.trim();
      pushLog(`Imported ${result.matched_tracks} of ${result.total_tracks} tracks into QBZ.`, 'success');

      if (result.qobuz_playlist_id) {
        pushLog('Playlist created in Qobuz™.', 'success');
      } else {
        pushLog('No matching tracks found.', 'error');
      }

      onSuccess?.(result);
      if (result.matched_tracks > 0) {
        showToast('Playlist imported', 'success');
      }
    } catch (err) {
      error = String(err);
      pushLog(`Import failed: ${error}`, 'error');
      showToast('Playlist import failed', 'error');
    } finally {
      loading = false;
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
      handleImport();
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
          <img src="/qobuz-logo.svg" alt="Qobuz™" class="qobuz-logo" />
          <h2>Import Playlist</h2>
        </div>
        <button class="close-btn" onclick={onClose}>
          <X size={20} />
        </button>
      </div>

      <div class="modal-body">
        <QobuzLegalNotice checkboxDisabled={loading} />

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
          <label for="playlist-url">Playlist URL</label>
          <input
            id="playlist-url"
            type="text"
            bind:value={url}
            placeholder="https://open.spotify.com/playlist/..."
            disabled={loading}
          />
        </div>

        <div class="sources">
          <span class="sources-label">Allowed sources</span>
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

        {#if logEntries.length > 0}
          <div class="progress-panel">
            <div class="progress-header">
              <span>Conversion progress</span>
              {#if loading}
                <span class="spinner" aria-hidden="true"></span>
              {/if}
            </div>
            <ul class="progress-log">
              {#each logEntries as entry}
                <li class={`log-item ${entry.status}`}>{entry.message}</li>
              {/each}
            </ul>
            {#if summary}
              <div class="summary">
                <div class="summary-title">Summary</div>
                <div class="summary-row">Playlist: {summary.playlist_name}</div>
                <div class="summary-row">Tracks matched: {summary.matched_tracks} / {summary.total_tracks}</div>
                <div class="summary-row">Skipped: {summary.skipped_tracks}</div>
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <div class="modal-footer">
        <button class="btn-secondary" onclick={onClose} disabled={loading}>Close</button>
        <button class="btn-primary" onclick={handleImport} disabled={!isValid || loading || importCompleted}>
          {#if loading}
            Importing...
          {:else}
            Import playlist
          {/if}
        </button>
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
    z-index: 1000;
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

  .form-group input[type="text"] {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 14px;
    color: var(--text-primary);
    transition: border-color 150ms ease;
  }

  .form-group input[type="text"]:focus {
    outline: none;
    border-color: var(--accent-primary);
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

  .btn-secondary,
  .btn-primary {
    padding: 10px 16px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: transform 150ms ease, background 150ms ease, opacity 150ms ease;
  }

  .btn-secondary {
    background: transparent;
    border: 1px solid var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .btn-primary {
    background: var(--accent-primary);
    border: none;
    color: var(--text-on-accent);
  }

  .btn-primary:disabled,
  .btn-secondary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
    transform: none;
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
