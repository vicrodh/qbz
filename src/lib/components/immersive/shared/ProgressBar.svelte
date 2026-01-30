<script lang="ts">
  interface Props {
    currentTime: number;
    duration: number;
    onSeek: (time: number) => void;
    showTime?: boolean;
    size?: 'small' | 'normal';
  }

  let {
    currentTime,
    duration,
    onSeek,
    showTime = true,
    size = 'normal'
  }: Props = $props();

  let progressRef: HTMLDivElement | null = $state(null);
  let isDragging = $state(false);

  const progress = $derived(duration > 0 ? (currentTime / duration) * 100 : 0);

  function formatTime(seconds: number): string {
    if (!seconds || !isFinite(seconds)) return '0:00';
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function handleMouseDown(e: MouseEvent) {
    isDragging = true;
    updateProgress(e);
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
  }

  function handleMouseMove(e: MouseEvent) {
    if (isDragging) updateProgress(e);
  }

  function handleMouseUp() {
    isDragging = false;
    document.removeEventListener('mousemove', handleMouseMove);
    document.removeEventListener('mouseup', handleMouseUp);
  }

  function updateProgress(e: MouseEvent) {
    if (!progressRef) return;
    const rect = progressRef.getBoundingClientRect();
    const percentage = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
    onSeek(Math.round((percentage / 100) * duration));
  }

  function handleKeydown(e: KeyboardEvent) {
    const step = e.shiftKey ? 10 : 5;
    if (e.key === 'ArrowRight') {
      e.preventDefault();
      onSeek(Math.min(duration, currentTime + step));
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      onSeek(Math.max(0, currentTime - step));
    }
  }
</script>

<div class="progress-container" class:small={size === 'small'}>
  {#if showTime}
    <span class="time">{formatTime(currentTime)}</span>
  {/if}

  <div
    class="progress-track"
    bind:this={progressRef}
    onmousedown={handleMouseDown}
    onkeydown={handleKeydown}
    role="slider"
    tabindex="0"
    aria-valuenow={currentTime}
    aria-valuemin={0}
    aria-valuemax={duration}
    aria-label="Seek"
  >
    <div class="progress-fill" style="width: {progress}%"></div>
    <div class="progress-thumb" style="left: {progress}%"></div>
  </div>

  {#if showTime}
    <span class="time">{formatTime(duration)}</span>
  {/if}
</div>

<style>
  .progress-container {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
  }

  .progress-container.small {
    gap: 8px;
  }

  .time {
    font-size: 12px;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
    min-width: 40px;
  }

  .time:last-of-type {
    text-align: right;
  }

  .progress-track {
    flex: 1;
    height: 4px;
    background: var(--alpha-20, rgba(255, 255, 255, 0.2));
    border-radius: 2px;
    position: relative;
    cursor: pointer;
    transition: height 150ms ease;
  }

  .progress-track:hover,
  .progress-track:focus-visible {
    height: 6px;
  }

  .progress-track:focus-visible {
    outline: 2px solid var(--accent-primary, #7c3aed);
    outline-offset: 2px;
  }

  .progress-fill {
    height: 100%;
    background: var(--text-primary, white);
    border-radius: 2px;
    transition: width 100ms linear;
  }

  .progress-thumb {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--text-primary, white);
    transform: translate(-50%, -50%);
    opacity: 0;
    transition: opacity 150ms ease;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.4);
  }

  .progress-track:hover .progress-thumb,
  .progress-track:focus-visible .progress-thumb {
    opacity: 1;
  }

  /* Small variant */
  .progress-container.small .time {
    font-size: 11px;
    min-width: 36px;
  }

  .progress-container.small .progress-track {
    height: 3px;
  }

  .progress-container.small .progress-track:hover {
    height: 5px;
  }

  .progress-container.small .progress-thumb {
    width: 10px;
    height: 10px;
  }
</style>
