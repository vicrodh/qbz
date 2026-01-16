<script lang="ts">
  import { WifiOff, CloudOff, Power, Library } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import type { OfflineReason } from '$lib/stores/offlineStore';

  interface Props {
    reason?: OfflineReason | null;
    onGoToLibrary?: () => void;
  }

  let { reason = null, onGoToLibrary }: Props = $props();

  // Get icon based on reason
  function getIcon(r: OfflineReason | null) {
    switch (r) {
      case 'no_network': return WifiOff;
      case 'not_logged_in': return CloudOff;
      case 'manual_override': return Power;
      default: return WifiOff;
    }
  }

  const icon = $derived(getIcon(reason));
</script>

<div class="offline-placeholder" role="alert" aria-live="polite">
  <div class="content">
    <div class="icon-container" aria-hidden="true">
      <svelte:component this={icon} size={64} strokeWidth={1.5} />
    </div>
    <h2>
      {#if reason === 'no_network'}
        {$t('offline.noNetwork')}
      {:else if reason === 'not_logged_in'}
        {$t('offline.notLoggedIn')}
      {:else if reason === 'manual_override'}
        {$t('offline.manualMode')}
      {:else}
        {$t('offline.title')}
      {/if}
    </h2>
    <p>
      {#if reason === 'no_network'}
        {$t('offline.noNetworkMessage')}
      {:else if reason === 'not_logged_in'}
        {$t('offline.notLoggedInMessage')}
      {:else if reason === 'manual_override'}
        {$t('offline.manualModeMessage')}
      {:else}
        {$t('offline.message')}
      {/if}
    </p>
    {#if onGoToLibrary}
      <button class="library-button" onclick={onGoToLibrary}>
        <Library size={18} />
        <span>{$t('offline.goToLibrary')}</span>
      </button>
    {/if}
  </div>
</div>

<style>
  .offline-placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    min-height: 400px;
    padding: 48px;
  }

  .content {
    text-align: center;
    max-width: 400px;
  }

  .icon-container {
    color: var(--color-text-tertiary);
    margin-bottom: 24px;
    opacity: 0.6;
  }

  h2 {
    font-size: 1.5rem;
    font-weight: 600;
    color: var(--color-text-primary);
    margin: 0 0 12px 0;
  }

  p {
    font-size: 0.95rem;
    color: var(--color-text-secondary);
    margin: 0 0 24px 0;
    line-height: 1.5;
  }

  .library-button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 12px 24px;
    background: var(--color-primary);
    color: var(--color-on-primary);
    border: none;
    border-radius: 8px;
    font-size: 0.95rem;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.2s, transform 0.1s;
  }

  .library-button:hover {
    background: var(--color-primary-hover);
  }

  .library-button:active {
    transform: scale(0.98);
  }
</style>
