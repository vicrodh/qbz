<script lang="ts">
  import Modal from '../Modal.svelte';
  import { t } from '$lib/i18n';
  import type { AutoUpdateProgress } from '$lib/services/updatesService';

  interface Props {
    isOpen: boolean;
    progress: AutoUpdateProgress;
    onCancel: () => void;
    onFallbackManual: () => void;
  }

  let { isOpen, progress, onCancel, onFallbackManual }: Props = $props();

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  const progressPercent = $derived(
    progress.state === 'downloading' && progress.totalBytes != null
      ? Math.min(100, Math.round((progress.downloadedBytes / progress.totalBytes) * 100))
      : undefined,
  );

  const canCancel = $derived(progress.state === 'downloading' || progress.state === 'checking');

  function statusText(state: AutoUpdateProgress, percent: number | undefined): string {
    switch (state.state) {
      case 'checking':
        return $t('updates.autoUpdate.checking');
      case 'downloading':
        return percent !== undefined
          ? $t('updates.autoUpdate.downloadingProgress', { values: { percent } })
          : $t('updates.autoUpdate.downloading');
      case 'installing':
        return $t('updates.autoUpdate.installing');
      case 'restarting':
        return $t('updates.autoUpdate.restarting');
      case 'error':
        return $t('updates.autoUpdate.failed');
    }
  }
</script>

<Modal {isOpen} onClose={canCancel ? onCancel : () => {}} title={$t('updates.autoUpdate.title')} maxWidth="460px">
  <div class="progress-body">
    <p class="status-text">{statusText(progress, progressPercent)}</p>

    {#if progress.state === 'downloading'}
      <div class="progress-bar-track">
        <div
          class="progress-bar-fill"
          style:width={progressPercent !== undefined ? `${progressPercent}%` : '100%'}
          class:indeterminate={progressPercent === undefined}
        ></div>
      </div>
      {#if progress.totalBytes != null}
        <p class="byte-count">
          {formatBytes(progress.downloadedBytes)} / {formatBytes(progress.totalBytes)}
        </p>
      {/if}
    {/if}

    {#if progress.state === 'checking' || progress.state === 'installing' || progress.state === 'restarting'}
      <div class="progress-bar-track">
        <div class="progress-bar-fill indeterminate"></div>
      </div>
    {/if}

    {#if progress.state === 'error'}
      <p class="error-message">{progress.error}</p>
    {/if}
  </div>

  {#snippet footer()}
    <div class="footer-actions">
      {#if progress.state === 'error'}
        <button class="btn btn-ghost" type="button" onclick={onCancel}>{$t('actions.close')}</button>
        <button class="btn btn-primary" type="button" onclick={onFallbackManual}>{$t('actions.downloadManually')}</button>
      {:else if canCancel}
        <button class="btn btn-ghost" type="button" onclick={onCancel}>{$t('actions.cancel')}</button>
      {/if}
    </div>
  {/snippet}
</Modal>

<style>
  .progress-body {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 12px;
    padding: 12px 0;
  }

  .status-text {
    margin: 0;
    color: var(--text-primary);
    font-weight: 500;
    font-size: 15px;
  }

  .progress-bar-track {
    width: 100%;
    height: 6px;
    background: var(--bg-tertiary);
    border-radius: 3px;
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--accent-primary);
    border-radius: 3px;
    transition: width 0.2s ease;
  }

  .progress-bar-fill.indeterminate {
    width: 30%;
    animation: indeterminate 1.5s ease-in-out infinite;
  }

  @keyframes indeterminate {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(400%);
    }
  }

  .byte-count {
    margin: 0;
    color: var(--text-muted);
    font-size: 13px;
  }

  .error-message {
    margin: 0;
    color: var(--text-error, #ef4444);
    font-size: 13px;
    word-break: break-word;
  }

  .footer-actions {
    display: flex;
    width: 100%;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
