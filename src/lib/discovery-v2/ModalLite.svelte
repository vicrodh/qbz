<script lang="ts">
  import { X } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import type { Snippet } from 'svelte';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    title?: string;
    showCloseButton?: boolean;
    maxWidth?: string;
    /** When true, renders a full-viewport dim scrim under the modal so
     *  the background reads as inert. The dim is a plain `rgba(0,0,0,0.7)`
     *  paint — no `backdrop-filter: blur(...)` which is the actual perf
     *  cost the original ModalLite was trying to avoid. Now that HW
     *  acceleration is on by default (ADR-004) this is cheap again, so
     *  default-on matches the legacy `Modal.svelte` UX. Pass `false` to
     *  drop the scrim for an overlay-less floating modal. */
    withScrim?: boolean;
    /** Footer button alignment. Defaults to `'start'` so modals migrated
     *  from `Modal.svelte` keep their visual layout unchanged. Use `'end'`
     *  for the modern dialog convention (Cancel | Save on the right). */
    footerAlign?: 'start' | 'end';
    children: Snippet;
    footer?: Snippet;
  }

  let {
    isOpen,
    onClose,
    title,
    showCloseButton = true,
    maxWidth = '480px',
    withScrim = true,
    footerAlign = 'start',
    children,
    footer,
  }: Props = $props();

  /**
   * Discovery V2 modal — drop-in replacement for `Modal.svelte`.
   *
   * Key differences from the legacy shared modal:
   *  - No `backdrop-filter: blur(...)`. The dim is an opaque rgba paint
   *    only — the blur was the dominant cost on WebKitGTK under software
   *    compositing.
   *  - No slide-up / fade-in animations on mount. Modal appears instantly.
   *  - Backdrop click handled via an overlay div (same as legacy). An
   *    earlier draft used `document.mousedown` instead which caused
   *    portaled descendants (Dropdown.svelte) to close the modal when
   *    clicked. The overlay approach restores the legacy behaviour while
   *    keeping the rest of the perf wins.
   */

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) onClose();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && isOpen) onClose();
  }

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div
    class="overlay"
    class:scrim={withScrim}
    use:portal
    onclick={handleBackdropClick}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal" style="max-width: {maxWidth}">
      {#if title || showCloseButton}
        <div class="modal-header">
          {#if title}
            <h2>{title}</h2>
          {:else}
            <div></div>
          {/if}
          {#if showCloseButton}
            <button class="close-btn" type="button" aria-label={$t('actions.close')} onclick={onClose}>
              <X size={18} />
            </button>
          {/if}
        </div>
      {/if}
      <div class="modal-body">
        {@render children()}
      </div>
      {#if footer}
        <div class="modal-footer" class:align-end={footerAlign === 'end'}>
          {@render footer()}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  /* Full-viewport overlay. The `.scrim` modifier paints the dim background
     when `withScrim` is true; without it the overlay is fully transparent
     and only exists to catch backdrop clicks. Either way it sits above the
     app and centers its modal child. Cero blur — the rgba alpha alone
     carries the dim. */
  .overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
    z-index: 200000;
  }

  .overlay.scrim {
    background: rgba(0, 0, 0, 0.7);
  }

  .modal {
    width: 100%;
    max-height: calc(100dvh - 40px);
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 12px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    /* Flat shadow stack instead of a Gaussian blur — under software
       compositing the blur radius dominates paint cost and re-rasterizes
       whenever anything beneath the modal repaints. */
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.6),
      0 8px 16px rgba(0, 0, 0, 0.5);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--bg-tertiary);
    flex-shrink: 0;
  }

  .modal-header h2 {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .close-btn {
    width: 28px;
    height: 28px;
    border-radius: 4px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
  }

  .close-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .modal-body {
    padding: 20px;
    overflow-y: auto;
    flex: 1;
  }

  /* Default footer aligns left (matches legacy `Modal.svelte` so migrated
     modals keep their layout). Opt into `align-end` for the modern
     Cancel | Save right-aligned convention. */
  .modal-footer {
    padding: 16px 20px;
    border-top: 1px solid var(--bg-tertiary);
    display: flex;
    justify-content: flex-start;
    gap: 8px;
    flex-shrink: 0;
  }

  .modal-footer.align-end {
    justify-content: flex-end;
  }
</style>
