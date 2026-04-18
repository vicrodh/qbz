<script lang="ts">
  import { RotateCcw } from 'lucide-svelte';
  import { t } from 'svelte-i18n';
  import { fade } from 'svelte/transition';
  import {
    lyricsDisplayStore,
    setLyricsAutoFollow,
    setLyricsFont,
    setLyricsFontSize,
    setLyricsDimming,
    resetLyricsDisplay,
    type LyricsFont,
    type LyricsFontSize,
    type LyricsDimming
  } from '$lib/stores/lyricsDisplayStore';

  interface Props {
    open: boolean;
    anchorEl: HTMLElement | null;
    onClose: () => void;
  }

  let { open, anchorEl, onClose }: Props = $props();

  let popoverEl: HTMLDivElement | null = $state(null);

  const prefs = $derived($lyricsDisplayStore);

  const fontOptions: { value: LyricsFont; labelKey: string }[] = [
    { value: 'system', labelKey: 'player.lyricsControls.fonts.system' },
    { value: 'line-seed-jp', labelKey: 'player.lyricsControls.fonts.lineSeedJp' },
    { value: 'montserrat', labelKey: 'player.lyricsControls.fonts.montserrat' },
    { value: 'noto-sans', labelKey: 'player.lyricsControls.fonts.notoSans' },
    { value: 'source-sans-3', labelKey: 'player.lyricsControls.fonts.sourceSans3' }
  ];

  const sizeOptions: { value: LyricsFontSize; labelKey: string }[] = [
    { value: 'small', labelKey: 'player.lyricsControls.sizes.small' },
    { value: 'medium', labelKey: 'player.lyricsControls.sizes.medium' },
    { value: 'large', labelKey: 'player.lyricsControls.sizes.large' }
  ];

  const dimmingOptions: { value: LyricsDimming; labelKey: string }[] = [
    { value: 'off', labelKey: 'player.lyricsControls.dimmingLevels.off' },
    { value: 'soft', labelKey: 'player.lyricsControls.dimmingLevels.soft' },
    { value: 'strong', labelKey: 'player.lyricsControls.dimmingLevels.strong' }
  ];

  function handleClickOutside(event: MouseEvent) {
    if (!open) return;
    const target = event.target as Node;
    if (popoverEl && popoverEl.contains(target)) return;
    if (anchorEl && anchorEl.contains(target)) return;
    onClose();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!open) return;
    if (event.key === 'Escape') {
      event.stopPropagation();
      onClose();
    }
  }

  $effect(() => {
    if (!open) return;
    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleKeydown);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleKeydown);
    };
  });
</script>

{#if open}
  <div
    class="popover"
    bind:this={popoverEl}
    role="dialog"
    aria-label={$t('player.lyricsControls.title')}
    transition:fade={{ duration: 150 }}
  >
    <div class="row">
      <span class="label">{$t('player.lyricsControls.autoFollow')}</span>
      <button
        type="button"
        class="switch"
        class:on={prefs.autoFollow}
        role="switch"
        aria-checked={prefs.autoFollow}
        onclick={() => setLyricsAutoFollow(!prefs.autoFollow)}
      >
        <span class="switch-thumb"></span>
      </button>
    </div>

    <div class="row">
      <span class="label">{$t('player.lyricsControls.font')}</span>
      <select
        class="select"
        value={prefs.font}
        onchange={(e) => setLyricsFont((e.currentTarget as HTMLSelectElement).value as LyricsFont)}
      >
        {#each fontOptions as opt (opt.value)}
          <option value={opt.value}>{$t(opt.labelKey)}</option>
        {/each}
      </select>
    </div>

    <div class="row">
      <span class="label">{$t('player.lyricsControls.size')}</span>
      <div class="segmented" role="group" aria-label={$t('player.lyricsControls.size')}>
        {#each sizeOptions as opt (opt.value)}
          <button
            type="button"
            class="seg"
            class:active={prefs.fontSize === opt.value}
            aria-pressed={prefs.fontSize === opt.value}
            onclick={() => setLyricsFontSize(opt.value)}
          >
            {$t(opt.labelKey)}
          </button>
        {/each}
      </div>
    </div>

    <div class="row">
      <span class="label">{$t('player.lyricsControls.dimming')}</span>
      <div class="segmented" role="group" aria-label={$t('player.lyricsControls.dimming')}>
        {#each dimmingOptions as opt (opt.value)}
          <button
            type="button"
            class="seg"
            class:active={prefs.dimming === opt.value}
            aria-pressed={prefs.dimming === opt.value}
            onclick={() => setLyricsDimming(opt.value)}
          >
            {$t(opt.labelKey)}
          </button>
        {/each}
      </div>
    </div>

    <div class="footer">
      <button type="button" class="reset" onclick={resetLyricsDisplay}>
        <RotateCcw size={14} />
        <span>{$t('player.lyricsControls.reset')}</span>
      </button>
    </div>
  </div>
{/if}

<style>
  .popover {
    position: absolute;
    top: calc(100% + 8px);
    right: 8px;
    z-index: 100;
    width: 240px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 28px;
  }

  .label {
    font-size: 12px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .switch {
    width: 36px;
    height: 20px;
    border-radius: 999px;
    background: var(--bg-tertiary);
    border: 1px solid var(--bg-tertiary);
    padding: 0;
    position: relative;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .switch.on {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .switch-thumb {
    position: absolute;
    top: 1px;
    left: 1px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--text-primary);
    transition: transform 150ms ease;
  }

  .switch.on .switch-thumb {
    transform: translateX(16px);
  }

  .select {
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    padding: 4px 8px;
    font-size: 13px;
    font-family: var(--font-sans);
    max-width: 140px;
  }

  .segmented {
    display: inline-flex;
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    overflow: hidden;
  }

  .seg {
    background: transparent;
    color: var(--text-muted);
    border: none;
    padding: 4px 10px;
    font-size: 12px;
    font-family: var(--font-sans);
    cursor: pointer;
    transition: background 150ms ease, color 150ms ease;
  }

  .seg:hover {
    color: var(--text-primary);
    background: var(--bg-secondary);
  }

  .seg.active {
    background: var(--accent-primary);
    color: #ffffff;
  }

  .seg + .seg {
    border-left: 1px solid var(--bg-tertiary);
  }

  .footer {
    display: flex;
    justify-content: flex-end;
    padding-top: 8px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .reset {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    color: var(--text-muted);
    border: 1px solid transparent;
    border-radius: 6px;
    padding: 4px 8px;
    font-size: 12px;
    font-family: var(--font-sans);
    cursor: pointer;
    transition: color 150ms ease, background 150ms ease, border-color 150ms ease;
  }

  .reset:hover {
    color: var(--text-primary);
    background: var(--bg-secondary);
    border-color: var(--bg-tertiary);
  }
</style>
