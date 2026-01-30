<script lang="ts">
  import { t } from '$lib/i18n';

  interface Props {
    isPlaying?: boolean;
  }

  let { isPlaying = false }: Props = $props();

  // TODO: Implement visualizer in Phase 5
  // Will use spectrum-analyzer crate in Rust + Canvas rendering
</script>

<div class="visualizer-panel">
  <div class="placeholder">
    <span class="placeholder-icon">📊</span>
    <span class="placeholder-text">{$t('player.visualizerComingSoon') || 'Visualizer coming soon'}</span>
    <span class="placeholder-hint">Frequency Bars, Waveform, Circular...</span>
  </div>
</div>

<style>
  .visualizer-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    overflow: hidden;
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
