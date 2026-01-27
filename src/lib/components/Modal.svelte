<script lang="ts">
  import { X } from 'lucide-svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    title?: string;
    showCloseButton?: boolean;
    maxWidth?: string;
    children: Snippet;
    footer?: Snippet;
  }

  let {
    isOpen,
    onClose,
    title,
    showCloseButton = true,
    maxWidth = '480px',
    children,
    footer
  }: Props = $props();

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onClose();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && isOpen) {
      onClose();
    }
  }

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      }
    };
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <div class="modal-overlay" use:portal onclick={handleBackdropClick} role="dialog" aria-modal="true">
    <div class="modal" style="max-width: {maxWidth}">
      {#if title || showCloseButton}
        <div class="modal-header">
          {#if title}
            <h2>{title}</h2>
          {:else}
            <div></div>
          {/if}
          {#if showCloseButton}
            <button class="close-btn" onclick={onClose} title="Close">
              <X size={20} />
            </button>
          {/if}
        </div>
      {/if}

      <div class="modal-content">
        {@render children()}
      </div>

      {#if footer}
        <div class="modal-footer">
          {@render footer()}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
    padding-left: 140px; /* Half of sidebar width (280px) for visual center */
    z-index: 200000;
    animation: fade-in 150ms ease;
  }

  @keyframes fade-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .modal {
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 12px;
    width: 100%;
    max-height: calc(100dvh - 40px);
    display: flex;
    flex-direction: column;
    animation: slide-up 200ms ease;
  }

  @keyframes slide-up {
    from {
      opacity: 0;
      transform: translateY(20px) scale(0.98);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .modal-header h2 {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .close-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .modal-content {
    flex: 1;
    overflow-y: auto;
    padding: 20px;
  }

  .modal-footer {
    display: flex;
    justify-content: flex-start;
    padding: 16px 20px;
    border-top: 1px solid var(--bg-tertiary);
  }
</style>
