<script lang="ts">
  import { X } from 'lucide-svelte';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    src: string;
    alt?: string;
  }

  let {
    isOpen,
    onClose,
    src,
    alt = ''
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
  <div class="lightbox-overlay" use:portal onclick={handleBackdropClick}>
    <div class="lightbox-image-wrapper">
      <img
        {src}
        {alt}
        class="lightbox-image"
      />
      <button class="lightbox-close" onclick={onClose}>
        <X size={18} />
      </button>
    </div>
  </div>
{/if}

<style>
  .lightbox-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
    padding-left: 140px;
    z-index: 200000;
    animation: lightbox-fade-in 150ms ease;
    cursor: default;
  }

  @keyframes lightbox-fade-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .lightbox-image-wrapper {
    position: relative;
    max-width: 85vw;
    max-height: 85vh;
  }

  .lightbox-image {
    max-width: 85vw;
    max-height: 85vh;
    object-fit: contain;
    border-radius: 8px;
    box-shadow: 0 16px 64px rgba(0, 0, 0, 0.6);
  }

  .lightbox-close {
    position: absolute;
    top: 8px;
    right: 8px;
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.6);
    border: none;
    border-radius: 50%;
    color: white;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .lightbox-close:hover {
    background: rgba(0, 0, 0, 0.8);
  }
</style>
