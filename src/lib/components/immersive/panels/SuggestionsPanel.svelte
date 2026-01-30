<script lang="ts">
  import { Radio } from 'lucide-svelte';

  interface Props {
    trackId?: number;
    artistId?: number;
  }

  let { trackId, artistId }: Props = $props();

  // TODO: Implement suggestions - Artist Radio, Track Radio, Similar Tracks
  let isLoading = $state(false);
</script>

<div class="suggestions-panel">
  <div class="placeholder">
    <div class="placeholder-icon">
      <Radio size={40} strokeWidth={1.5} />
    </div>
    <span class="placeholder-text">Suggestions</span>
    <span class="placeholder-hint">Artist Radio, Track Radio, Similar Tracks...</span>
    <span class="coming-soon">Coming Soon</span>
  </div>
</div>

<style>
  .suggestions-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    padding: 24px;
  }

  .placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    height: 100%;
    min-height: 200px;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
  }

  .placeholder-icon {
    opacity: 0.4;
  }

  .placeholder-text {
    font-size: 18px;
    font-weight: 600;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
  }

  .placeholder-hint {
    font-size: 13px;
    color: var(--alpha-30, rgba(255, 255, 255, 0.3));
  }

  .coming-soon {
    margin-top: 8px;
    padding: 6px 14px;
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
    border-radius: 20px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
  }
</style>
