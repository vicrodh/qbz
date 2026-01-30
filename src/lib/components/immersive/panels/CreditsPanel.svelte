<script lang="ts">
  import { t } from '$lib/i18n';

  interface Props {
    trackId?: number;
  }

  let { trackId }: Props = $props();

  // TODO: Implement credits fetching in Phase 3
  let isLoading = $state(false);
  let credits = $state<null>(null);
</script>

<div class="credits-panel">
  <div class="placeholder">
    <span class="placeholder-icon">ℹ️</span>
    <span class="placeholder-text">{$t('player.creditsComingSoon') || 'Credits coming soon'}</span>
    <span class="placeholder-hint">Producer, Composer, Musicians...</span>
  </div>
</div>

<style>
  .credits-panel {
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
