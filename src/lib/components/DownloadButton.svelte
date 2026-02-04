<script lang="ts">
  import { CloudDownload, Loader, AlertCircle, CloudOff } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import {
    subscribe as subscribeOffline,
    isOffline as checkIsOffline
  } from '$lib/stores/offlineStore';

  // Offline cache status for tracks
  type OfflineCacheStatus = 'none' | 'queued' | 'downloading' | 'ready' | 'failed';

  interface Props {
    status?: OfflineCacheStatus;
    progress?: number;
    size?: number;
    onDownload?: () => void;
    onRemove?: () => void;
  }

  let {
    status = 'none',
    progress = 0,
    size = 16,
    onDownload,
    onRemove
  }: Props = $props();

  // Offline state
  let isOffline = $state(checkIsOffline());

  $effect(() => {
    const unsubscribe = subscribeOffline(() => {
      isOffline = checkIsOffline();
    });
    return unsubscribe;
  });

  function handleClick(e: MouseEvent) {
    e.stopPropagation();

    // Don't allow new downloads when offline
    if (isOffline && (status === 'none' || status === 'failed')) {
      return;
    }

    if (status === 'none' || status === 'failed') {
      onDownload?.();
    } else if (status === 'ready') {
      onRemove?.();
    }
  }

  // Return translation key for title - call $t() in template
  const titleKey = $derived.by(() => {
    // Show offline message when trying to download while offline
    if (isOffline && (status === 'none' || status === 'failed')) {
      return 'offline.featureDisabled';
    }
    switch (status) {
      case 'none': return 'download.makeAvailable';
      case 'queued': return 'download.queued';
      case 'downloading': return 'download.preparing';
      case 'ready': return 'download.ready';
      case 'failed': return 'download.failed';
      default: return null;
    }
  });

  // Resolve title - must be a function called from template (not $derived)
  function getTitle(): string {
    if (!titleKey) return '';
    if (status === 'downloading') {
      return `${$t(titleKey)} ${progress}%`;
    }
    return $t(titleKey);
  }

  // Disable button when offline and not already downloaded
  const isDisabled = $derived(
    status === 'queued' || (isOffline && status !== 'ready')
  );
</script>

<button
  class="download-button"
  class:downloading={status === 'downloading' || status === 'queued'}
  class:ready={status === 'ready'}
  class:failed={status === 'failed'}
  class:offline={isOffline && status !== 'ready'}
  onclick={handleClick}
  title={getTitle()}
  aria-label={getTitle()}
  disabled={isDisabled}
>
  {#if status === 'ready'}
    <CloudOff {size} />
  {:else if status === 'downloading' || status === 'queued'}
    <div class="progress-ring" style="--progress: {progress}">
      <Loader {size} class="spinning" />
    </div>
  {:else if status === 'failed'}
    <AlertCircle {size} />
  {:else if isOffline}
    <CloudOff {size} />
  {:else}
    <CloudDownload {size} />
  {/if}
</button>

<style>
  .download-button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 4px;
    transition: all 150ms ease;
    padding: 0;
  }

  .download-button:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .download-button:disabled {
    cursor: default;
    opacity: 0.5;
  }

  .download-button.ready {
    color: var(--success, #22c55e);
  }

  .download-button.ready:hover:not(:disabled) {
    color: var(--success, #22c55e);
  }

  .download-button.failed {
    color: var(--error, #ef4444);
  }

  .download-button.downloading {
    color: var(--accent-primary);
  }

  .download-button.offline {
    color: var(--text-muted);
    opacity: 0.5;
  }

  .progress-ring {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .progress-ring :global(.spinning) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }
</style>
