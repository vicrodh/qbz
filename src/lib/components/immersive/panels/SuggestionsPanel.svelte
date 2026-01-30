<script lang="ts">
  import { t } from '$lib/i18n';

  interface Props {
    trackId?: number;
    artistId?: number;
  }

  let { trackId, artistId }: Props = $props();

  // TODO: Implement suggestions fetching in Phase 4
  let isLoading = $state(false);
</script>

<div class="suggestions-panel">
  <div class="placeholder">
    <span class="placeholder-icon">📻</span>
    <span class="placeholder-text">{$t('player.suggestionsComingSoon') || 'Suggestions coming soon'}</span>
    <span class="placeholder-hint">Artist Radio, Track Radio, Similar Tracks...</span>
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
    gap: 12px;
    height: 100%;
    min-height: 200px;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
  }

  .placeholder-icon {
    font-size: 32px;
    opacity: 0.5;
  }

  .placeholder-text {
    font-size: 16px;
    font-weight: 500;
  }

  .placeholder-hint {
    font-size: 13px;
    color: var(--alpha-30, rgba(255, 255, 255, 0.3));
  }
</style>
