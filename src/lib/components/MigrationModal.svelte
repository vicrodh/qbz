<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { onMount, onDestroy } from 'svelte';
  import Modal from './Modal.svelte';
  import { AlertCircle, CheckCircle, Loader, XCircle } from 'lucide-svelte';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    totalTracks: number;
  }

  interface MigrationProgress {
    has_legacy_files: boolean;
    total_tracks: number;
    processed: number;
    successful: number;
    failed: number;
    in_progress: boolean;
    completed: boolean;
    errors: Array<{ track_id: number; error_message: string }>;
  }

  let { isOpen, onClose, totalTracks }: Props = $props();

  let migrating = $state(false);
  let progress = $state<MigrationProgress | null>(null);
  let unlistenProgress: UnlistenFn | null = null;
  let unlistenComplete: UnlistenFn | null = null;

  onMount(async () => {
    unlistenProgress = await listen<MigrationProgress>('migration:progress', (event) => {
      progress = event.payload;
    });

    unlistenComplete = await listen<MigrationProgress>('migration:complete', (event) => {
      progress = event.payload;
      migrating = false;
    });
  });

  onDestroy(() => {
    if (unlistenProgress) unlistenProgress();
    if (unlistenComplete) unlistenComplete();
  });

  async function startMigration() {
    migrating = true;
    try {
      await invoke('start_legacy_migration');
    } catch (error) {
      console.error('Failed to start migration:', error);
      migrating = false;
    }
  }

  function handleCancel() {
    if (!migrating) {
      onClose();
    }
  }

  const progressPercent = $derived(
    progress ? Math.round((progress.processed / progress.total_tracks) * 100) : 0
  );
</script>

<Modal {isOpen} onClose={handleCancel} title="Migrate Downloads" maxWidth="600px">
  {#snippet children()}
    <div class="migration-content">
      {#if !migrating && !progress?.completed}
        <div class="info-section">
          <div class="info-icon">
            <AlertCircle size={48} color="var(--accent-primary)" />
          </div>
          <h3>Legacy Downloads Detected</h3>
          <p>
            We found <strong>{totalTracks}</strong> downloads from a previous version
            that need to be migrated to the new organized format.
          </p>
          <p class="details">
            This process will:
          </p>
          <ul>
            <li>Fetch complete metadata from Qobuz</li>
            <li>Add proper tags to your FLAC files</li>
            <li>Organize files into Artist/Album folders</li>
            <li>Embed album artwork</li>
            <li>Add tracks to your local library (if enabled)</li>
          </ul>
          <p class="warning">
            <strong>Note:</strong> This may take a few minutes depending on your internet
            connection and the number of downloads. The old files will be deleted after
            successful migration.
          </p>
        </div>
      {:else if migrating || (progress && progress.in_progress)}
        <div class="progress-section">
          <div class="progress-icon">
            <Loader size={48} class="spinner" />
          </div>
          <h3>Migrating Downloads...</h3>
          {#if progress}
            <div class="progress-bar">
              <div class="progress-fill" style="width: {progressPercent}%"></div>
            </div>
            <p class="progress-text">
              {progress.processed} / {progress.total_tracks} tracks processed
              ({progress.successful} successful, {progress.failed} failed)
            </p>
          {:else}
            <p>Starting migration...</p>
          {/if}
        </div>
      {:else if progress?.completed}
        <div class="complete-section">
          <div class="complete-icon">
            <CheckCircle size={48} color="var(--success)" />
          </div>
          <h3>Migration Complete</h3>
          <div class="stats">
            <div class="stat">
              <CheckCircle size={20} color="var(--success)" />
              <span>{progress.successful} successful</span>
            </div>
            {#if progress.failed > 0}
              <div class="stat error">
                <XCircle size={20} color="var(--error)" />
                <span>{progress.failed} failed</span>
              </div>
            {/if}
          </div>
          {#if progress.errors.length > 0}
            <details class="errors">
              <summary>View errors ({progress.errors.length})</summary>
              <div class="error-list">
                {#each progress.errors as error}
                  <div class="error-item">
                    <strong>Track {error.track_id}:</strong> {error.error_message}
                  </div>
                {/each}
              </div>
            </details>
          {/if}
          <p class="success-message">
            Your downloads have been organized and are now available in your local library.
          </p>
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet footer()}
    <div class="modal-actions">
      {#if !migrating && !progress?.completed}
        <button class="btn btn-secondary" onclick={handleCancel}>
          Cancel
        </button>
        <button class="btn btn-primary" onclick={startMigration}>
          Start Migration
        </button>
      {:else if migrating || (progress && progress.in_progress)}
        <button class="btn btn-secondary" disabled>
          Migrating...
        </button>
      {:else if progress?.completed}
        <button class="btn btn-primary" onclick={onClose}>
          Done
        </button>
      {/if}
    </div>
  {/snippet}
</Modal>

<style>
  .migration-content {
    min-height: 200px;
  }

  .info-section,
  .progress-section,
  .complete-section {
    text-align: center;
  }

  .info-icon,
  .progress-icon,
  .complete-icon {
    display: flex;
    justify-content: center;
    margin-bottom: 16px;
  }

  .info-section h3,
  .progress-section h3,
  .complete-section h3 {
    font-size: 20px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 16px 0;
  }

  .info-section p {
    color: var(--text-secondary);
    line-height: 1.6;
    margin: 12px 0;
  }

  .info-section .details {
    text-align: left;
    margin-top: 24px;
    margin-bottom: 8px;
    font-weight: 500;
  }

  .info-section ul {
    text-align: left;
    color: var(--text-secondary);
    line-height: 1.8;
    padding-left: 24px;
    margin: 8px 0;
  }

  .info-section .warning {
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 12px;
    margin-top: 24px;
    text-align: left;
    font-size: 14px;
  }

  :global(.spinner) {
    animation: spin 1s linear infinite;
    color: var(--accent-primary);
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .progress-bar {
    width: 100%;
    height: 8px;
    background: var(--bg-tertiary);
    border-radius: 4px;
    overflow: hidden;
    margin: 16px 0;
  }

  .progress-fill {
    height: 100%;
    background: var(--accent-primary);
    transition: width 300ms ease;
  }

  .progress-text {
    color: var(--text-secondary);
    font-size: 14px;
  }

  .stats {
    display: flex;
    gap: 24px;
    justify-content: center;
    margin: 16px 0;
  }

  .stat {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-primary);
    font-weight: 500;
  }

  .stat.error {
    color: var(--error);
  }

  .errors {
    margin-top: 16px;
    text-align: left;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 12px;
  }

  .errors summary {
    cursor: pointer;
    font-weight: 500;
    color: var(--text-primary);
    user-select: none;
  }

  .error-list {
    margin-top: 12px;
    max-height: 200px;
    overflow-y: auto;
  }

  .error-item {
    padding: 8px;
    margin: 4px 0;
    background: var(--bg-primary);
    border-radius: 4px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .error-item strong {
    color: var(--text-primary);
  }

  .success-message {
    color: var(--text-secondary);
    margin-top: 16px;
  }

  .modal-actions {
    display: flex;
    gap: 12px;
    width: 100%;
  }

  .btn {
    flex: 1;
    padding: 10px 20px;
    border-radius: 8px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
    border: none;
    font-size: 14px;
  }

  .btn-primary {
    background: var(--accent-primary);
    color: white;
  }

  .btn-primary:hover:not(:disabled) {
    background: var(--accent-primary-hover);
  }

  .btn-secondary {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .btn-secondary:hover:not(:disabled) {
    background: var(--bg-tertiary);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
