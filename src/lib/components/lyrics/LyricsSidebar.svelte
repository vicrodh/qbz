<script lang="ts">
  import { MicVocal, SlidersHorizontal } from 'lucide-svelte';
  import { t } from 'svelte-i18n';
  import LyricsLines from './LyricsLines.svelte';
  import LyricsControlsPopover from './LyricsControlsPopover.svelte';
  import { lyricsDisplayStore } from '$lib/stores/lyricsDisplayStore';

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

  let popoverOpen = $state(false);
  let anchorEl: HTMLButtonElement | null = $state(null);

  const prefs = $derived($lyricsDisplayStore);
  const canCopy = $derived(!isLoading && !error && lines.length > 0);
</script>

<aside class="lyrics-sidebar">
  <div class="header">
    <div class="header-icon">
      <MicVocal size={18} />
    </div>
    <div class="header-text">
      <div class="header-title">{$t('player.lyrics')}</div>
      {#if title || artist}
        <div class="header-meta">{title}{title && artist ? ' - ' : ''}{artist}</div>
      {/if}
    </div>
    <button
      type="button"
      class="controls-trigger"
      bind:this={anchorEl}
      aria-label={$t('player.lyricsControls.openControls')}
      aria-expanded={popoverOpen}
      onclick={() => (popoverOpen = !popoverOpen)}
    >
      <SlidersHorizontal size={18} />
    </button>
    <LyricsControlsPopover
      open={popoverOpen}
      {anchorEl}
      {lines}
      {canCopy}
      onClose={() => (popoverOpen = false)}
    />
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
        scrollToActive={prefs.autoFollow}
        fontMode={prefs.font}
        fontSizeMode={prefs.fontSize}
        dimmingMode={prefs.dimming}
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
    position: relative;
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

  .controls-trigger {
    flex-shrink: 0;
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    background: transparent;
    color: var(--text-muted);
    border: 1px solid transparent;
    border-radius: 6px;
    cursor: pointer;
    transition: color 150ms ease, background 150ms ease, border-color 150ms ease;
  }

  .controls-trigger:hover {
    color: var(--text-primary);
    background: var(--bg-secondary);
    border-color: var(--bg-tertiary);
  }

  .controls-trigger[aria-expanded='true'] {
    color: var(--accent-primary);
    background: var(--bg-secondary);
    border-color: var(--bg-tertiary);
  }

</style>
