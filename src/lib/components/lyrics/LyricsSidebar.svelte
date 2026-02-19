<script lang="ts">
  import { Mic2 } from 'lucide-svelte';
  import { t } from 'svelte-i18n';
  import LyricsLines from './LyricsLines.svelte';

  interface LyricsLine {
    text: string;
  }

  interface Props {
    title?: string;
    artist?: string;
    lines: LyricsLine[];
    activeIndex?: number;
    activeProgress?: number;
    isSynced?: boolean;
    isLoading?: boolean;
    error?: string | null;
  }

  let {
    title = '',
    artist = '',
    lines,
    activeIndex = -1,
    activeProgress = 0,
    isSynced = false,
    isLoading = false,
    error = null
  }: Props = $props();
</script>

<aside class="lyrics-sidebar">
  <div class="header">
    <div class="header-icon">
      <Mic2 size={18} />
    </div>
    <div class="header-text">
      <div class="header-title">{$t('player.lyrics')}</div>
      {#if title || artist}
        <div class="header-meta">{title}{title && artist ? ' - ' : ''}{artist}</div>
      {/if}
    </div>
  </div>

  <div class="panel">
    {#if isLoading}
      <div class="state">
        <div class="loading-spinner"></div>
        <span>{$t('player.fetchingLyrics')}</span>
      </div>
    {:else if error}
      <div class="state error">{error}</div>
    {:else}
      <LyricsLines
        {lines}
        {activeIndex}
        {activeProgress}
        {isSynced}
        compact={true}
        center={false}
      />
    {/if}
  </div>
</aside>

<style>
  .lyrics-sidebar {
    width: 340px;
    min-width: 340px;
    height: 100%;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--bg-tertiary);
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 16px;
    border-bottom: 1px solid var(--bg-tertiary);
    background: var(--bg-primary);
  }

  .header-icon {
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    background: var(--bg-tertiary);
    border-radius: 8px;
    color: var(--accent-primary);
  }

  .header-text {
    flex: 1;
    min-width: 0;
  }

  .header-title {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-muted);
  }

  .header-meta {
    font-size: 13px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-top: 2px;
  }

  .panel {
    flex: 1;
    overflow: hidden;
    position: relative;
  }

  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 48px 16px;
    font-size: 14px;
    color: var(--text-muted);
    height: 100%;
  }

  .state.error {
    color: var(--error, #e57373);
  }

  .loading-spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--bg-tertiary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
