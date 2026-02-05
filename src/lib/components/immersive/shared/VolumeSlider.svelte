<script lang="ts">
  import { Volume2, VolumeX } from 'lucide-svelte';
  import { toggleMute as playerToggleMute } from '$lib/stores/playerStore';

  interface Props {
    volume: number;
    onVolumeChange: (volume: number) => void;
    showIcon?: boolean;
  }

  let { volume, onVolumeChange, showIcon = true }: Props = $props();

  let volumeRef: HTMLDivElement | null = $state(null);
  let isDragging = $state(false);
  let showValue = $state(false);

  function handleMouseDown(e: MouseEvent) {
    isDragging = true;
    showValue = true;
    updateVolume(e);
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
  }

  function handleMouseMove(e: MouseEvent) {
    if (isDragging) updateVolume(e);
  }

  function handleMouseUp() {
    isDragging = false;
    setTimeout(() => {
      if (!isDragging) showValue = false;
    }, 500);
    document.removeEventListener('mousemove', handleMouseMove);
    document.removeEventListener('mouseup', handleMouseUp);
  }

  function updateVolume(e: MouseEvent) {
    if (!volumeRef) return;
    const rect = volumeRef.getBoundingClientRect();
    const percentage = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
    onVolumeChange(Math.round(percentage));
  }

  function toggleMute() {
    playerToggleMute();
  }
</script>

<div class="volume-container">
  {#if showIcon}
    <button class="volume-icon" onclick={toggleMute} title={volume > 0 ? 'Mute' : 'Unmute'}>
      {#if volume === 0}
        <VolumeX size={18} />
      {:else}
        <Volume2 size={18} />
      {/if}
    </button>
  {/if}

  <div class="volume-slider">
    <div class="volume-value" class:visible={showValue}>{volume}</div>
    <div
      class="volume-track"
      bind:this={volumeRef}
      onmousedown={handleMouseDown}
      role="slider"
      tabindex="0"
      aria-valuenow={volume}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label="Volume"
    >
      <div class="volume-fill" style="width: {volume}%"></div>
      <div class="volume-thumb" style="left: {volume}%"></div>
    </div>
  </div>
</div>

<style>
  .volume-container {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .volume-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: none;
    border: none;
    border-radius: 50%;
    color: var(--alpha-70, rgba(255, 255, 255, 0.7));
    cursor: pointer;
    transition: all 150ms ease;
  }

  .volume-icon:hover {
    color: var(--text-primary, white);
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
  }

  .volume-slider {
    position: relative;
    width: 100px;
  }

  .volume-value {
    position: absolute;
    right: 0;
    top: -28px;
    padding: 4px 8px;
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.7);
    color: var(--text-primary, white);
    font-size: 12px;
    font-weight: 600;
    opacity: 0;
    transform: translateY(4px);
    transition: opacity 150ms ease, transform 150ms ease;
    pointer-events: none;
  }

  .volume-value.visible {
    opacity: 1;
    transform: translateY(0);
  }

  .volume-track {
    height: 4px;
    background: var(--alpha-20, rgba(255, 255, 255, 0.2));
    border-radius: 2px;
    position: relative;
    cursor: pointer;
  }

  .volume-fill {
    height: 100%;
    background: var(--alpha-70, rgba(255, 255, 255, 0.7));
    border-radius: 2px;
  }

  .volume-thumb {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--text-primary, white);
    transform: translate(-50%, -50%);
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .volume-track:hover .volume-thumb {
    opacity: 1;
  }
</style>
