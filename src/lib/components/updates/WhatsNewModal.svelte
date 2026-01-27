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
  const EXTRA_PAD_TOP = 40;
  const EXTRA_PAD_BOTTOM = 130;
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
  const releaseToc = $derived(markdownResult.toc.filter(entry => entry.level === 0));

  let contentRef: HTMLDivElement | null = $state(null);

  function scrollToSection(id: string) {
    if (!contentRef) return;
    const target = contentRef.querySelector(`#${id}`);
    if (target) {
      target.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  }

  const modalTitle = $derived(
    release
      ? `What's new in v${release.version}, ${releaseDate}`
      : "What's new"
  );
</script>

<div
  class="whats-new-modal"
  style={`
    --updates-modal-max-height: ${maxHeightPx - 150}px;
    --updates-overlay-pad-top: ${TITLEBAR_HEIGHT + INNER_MARGIN + EXTRA_PAD_TOP}px;
    --updates-overlay-pad-bottom: ${PLAYER_HEIGHT + INNER_MARGIN + EXTRA_PAD_BOTTOM}px;
  `}
>
  <Modal
    {isOpen}
    onClose={onClose}
    title={modalTitle}
    maxWidth="910px"
  >
    {#if release}
      {#if releaseToc.length > 0}
        <nav class="toc-nav">
          {#each releaseToc as entry}
            <button
              class="toc-item"
              type="button"
              onclick={() => scrollToSection(entry.id)}
            >
              {entry.label}
            </button>
          {/each}
        </nav>
      {/if}
      {#if releaseHtml}
        <div class="release-body whats-new-content" bind:this={contentRef}>
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

  .toc-nav {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding-bottom: 16px;
    margin-bottom: 16px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .toc-item {
    padding: 6px 12px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .toc-item:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .release-body {
    color: var(--text-primary);
  }

  /* Whats New content styling - using !important to override any conflicting styles */
  .whats-new-content :global(p) {
    margin: 0 0 12px !important;
    line-height: 1.6 !important;
  }

  .whats-new-content :global(ul) {
    margin: 6px 0 14px 10px !important;
    padding: 0 0 0 24px !important;
    display: flex !important;
    flex-direction: column !important;
    gap: 8px !important;
    list-style-type: disc !important;
    list-style-position: outside !important;
  }

  /* First level list items - brighter color */
  .whats-new-content :global(li) {
    line-height: 1.6 !important;
    color: var(--text-secondary) !important;
    font-size: 12px !important;
    font-weight: 400 !important;
  }

  /* Nested lists - extra indentation */
  .whats-new-content :global(ul ul) {
    margin: 6px 0 8px 0 !important;
    padding-left: 24px !important;
  }

  /* Nested list items - darker/muted color */
  .whats-new-content :global(ul ul li) {
    color: var(--text-muted) !important;
    font-size: 11px !important;
  }

  /* Section headings */
  .whats-new-content :global(h3),
  .whats-new-content :global(.wn-section) {
    margin: 16px 0 8px !important;
    font-weight: 700 !important;
    font-size: 14px !important;
    letter-spacing: 0.2px !important;
    color: var(--text-primary) !important;
  }

  .whats-new-content :global(code) {
    background: var(--bg-tertiary) !important;
    border: 1px solid var(--bg-hover) !important;
    padding: 1px 6px !important;
    border-radius: 6px !important;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace !important;
    font-size: 12px !important;
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
