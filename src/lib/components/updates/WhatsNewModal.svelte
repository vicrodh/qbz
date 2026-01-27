<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import Modal from '../Modal.svelte';
  import type { ReleaseInfo } from '$lib/stores/updatesStore';
  import {
    formatReleaseDate,
    renderMarkdownWithToc,
  } from '$lib/utils/markdown';

  interface Props {
    isOpen: boolean;
    release: ReleaseInfo | null;
    showTitleBar: boolean;
    onClose: () => void;
  }

  let { isOpen, release, showTitleBar, onClose }: Props = $props();

  const PLAYER_HEIGHT = 104;
  const TITLEBAR_HEIGHT = $derived(showTitleBar ? 32 : 0);
  const INNER_MARGIN = 20;
  const MAX_MODAL_OFFSET_PX = 145;

  let maxHeightPx = $state(720);

  function computeMaxHeight(): void {
    if (typeof window === 'undefined') return;
    // Per requirement: modal max height = window height - 145px.
    const available = window.innerHeight - MAX_MODAL_OFFSET_PX;
    maxHeightPx = Math.max(360, available);
  }

  onMount(() => {
    computeMaxHeight();
    const handler = () => computeMaxHeight();
    window.addEventListener('resize', handler);
    return () => window.removeEventListener('resize', handler);
  });

  onDestroy(() => {
    // No-op, but keeps lifecycle explicit in this component.
  });

  const releaseDate = $derived(release ? formatReleaseDate(release.publishedAt) : '');
  const markdownResult = $derived.by(() => renderMarkdownWithToc(release?.body));
  const releaseHtml = $derived(markdownResult.html);
  const modalTitle = $derived(
    release
      ? `What's new in v${release.version}, ${releaseDate}`
      : "What's new"
  );
</script>

<div
  class="whats-new-modal"
  style={`
    --updates-modal-max-height: ${maxHeightPx}px;
    --updates-overlay-pad-top: ${TITLEBAR_HEIGHT + INNER_MARGIN}px;
    --updates-overlay-pad-bottom: ${PLAYER_HEIGHT + INNER_MARGIN}px;
  `}
>
  <Modal
    {isOpen}
    onClose={onClose}
    title={modalTitle}
    maxWidth="860px"
  >
    {#if release}
      {#if releaseHtml}
        <div class="release-body prose">
          {@html releaseHtml}
        </div>
      {:else}
        <p class="empty">Release notes are not available for this version.</p>
      {/if}
    {:else}
      <p class="empty">Release notes are not available.</p>
    {/if}

    {#snippet footer()}
      <div class="footer-actions">
        <button class="btn btn-primary" type="button" onclick={onClose}>Close</button>
      </div>
    {/snippet}
  </Modal>
</div>

<style>
  :global(.whats-new-modal .modal) {
    max-height: var(--updates-modal-max-height);
  }

  :global(.whats-new-modal .modal-overlay) {
    padding-top: var(--updates-overlay-pad-top);
    padding-bottom: var(--updates-overlay-pad-bottom);
  }

  .release-body {
    color: var(--text-primary);
  }

  /* Minimal prose styling for safe markdown output */
  :global(.whats-new-modal .prose p) {
    margin: 0 0 12px;
    line-height: 1.6;
  }

  :global(.whats-new-modal .prose ul) {
    margin: 6px 0 14px 10px;
    padding: 0 0 0 44px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    list-style-position: outside;
  }

  :global(.whats-new-modal .prose li) {
    line-height: 1.6;
    color: var(--text-secondary);
    font-size: 14px;
    font-weight: 400;
  }

  :global(.whats-new-modal .prose li)::marker {
    color: color-mix(in srgb, var(--text-secondary) 85%, transparent);
  }

  /* Nested/child list items - darker color and proper indentation */
  :global(.whats-new-modal .prose ul ul) {
    margin: 4px 0 8px 16px;
    padding-left: 20px;
  }

  :global(.whats-new-modal .prose li li) {
    color: var(--text-muted);
    font-size: 13px;
  }

  :global(.whats-new-modal .prose li li)::marker {
    color: color-mix(in srgb, var(--text-muted) 70%, transparent);
  }

  :global(.whats-new-modal .prose h3),
  :global(.whats-new-modal .prose .wn-section) {
    margin: 16px 0 8px;
    font-weight: 800;
    font-size: 18px;
    letter-spacing: 0.2px;
  }

  :global(.whats-new-modal .prose code) {
    background: var(--bg-tertiary);
    border: 1px solid var(--bg-hover);
    padding: 1px 6px;
    border-radius: 6px;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 12px;
  }

  .empty {
    color: var(--text-muted);
    margin: 0;
  }

  .footer-actions {
    display: flex;
    width: 100%;
    justify-content: flex-end;
  }
</style>
