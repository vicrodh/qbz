<script lang="ts">
  import { Mic2 } from 'lucide-svelte';
  import LyricsLines from './LyricsLines.svelte';

  interface LyricsLine {
    text: string;
  }

  interface Props {
    title?: string;
    artist?: string;
    provider?: string;
    lines: LyricsLine[];
    activeIndex?: number;
    activeProgress?: number;
    isLoading?: boolean;
    error?: string | null;
  }

  let {
    title = '',
    artist = '',
    provider,
    lines,
    activeIndex = -1,
    activeProgress = 0,
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
      <div class="header-title">Lyrics</div>
      {#if title || artist}
      <div class="header-meta">{title}{title && artist ? ' - ' : ''}{artist}</div>
      {/if}
    </div>
    {#if provider}
      <div class="header-provider">{provider}</div>
    {/if}
  </div>

  <div class="panel">
    {#if isLoading}
      <div class="state">Loading lyrics...</div>
    {:else if error}
      <div class="state error">{error}</div>
    {:else}
      <LyricsLines
        {lines}
        {activeIndex}
        {activeProgress}
        compact={true}
        center={false}
      />
    {/if}
  </div>
</aside>

<style>
  .lyrics-sidebar {
    width: var(--lyrics-sidebar-width, 320px);
    min-width: var(--lyrics-sidebar-width, 320px);
    height: calc(100vh - 80px);
    display: flex;
    flex-direction: column;
    border-left: 1px solid rgba(255, 255, 255, 0.06);
    background: linear-gradient(160deg, rgba(52, 38, 26, 0.92), rgba(22, 17, 12, 0.95));
    color: var(--text-primary);
    backdrop-filter: blur(14px);
    --lyrics-font-size: 15px;
    --lyrics-active-size: 20px;
    --lyrics-line-gap: 12px;
    --lyrics-line-height: 1.5;
    --lyrics-dimmed-opacity: 0.35;
    --lyrics-highlight-muted: rgba(255, 255, 255, 0.22);
    --lyrics-shadow: 0 10px 24px rgba(0, 0, 0, 0.45);
  }

  .header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 18px 16px 12px 16px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.05);
  }

  .header-icon {
    width: 34px;
    height: 34px;
    display: grid;
    place-items: center;
    background: rgba(255, 255, 255, 0.08);
    border-radius: 10px;
    color: var(--accent-primary);
  }

  .header-text {
    flex: 1;
    min-width: 0;
  }

  .header-title {
    font-size: 14px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.16em;
    color: var(--text-muted);
  }

  .header-meta {
    font-size: 13px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .header-provider {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    border: 1px solid rgba(255, 255, 255, 0.1);
    padding: 4px 8px;
    border-radius: 999px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .panel {
    flex: 1;
    position: relative;
  }

  .state {
    padding: 24px 16px;
    font-size: 14px;
    color: var(--text-muted);
  }

  .state.error {
    color: #f4a1a1;
  }
</style>
