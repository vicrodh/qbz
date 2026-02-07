<script lang="ts">
  import { tick } from 'svelte';
  import { ChevronDown, Check } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import type { PlaylistTag } from '$lib/types';

  interface Props {
    tags: PlaylistTag[];
    selectedTag: string | null;
    onTagChange: (tag: string | null) => void;
  }

  let { tags, selectedTag, onTagChange }: Props = $props();

  let isOpen = $state(false);
  let buttonEl: HTMLButtonElement | null = $state(null);
  let menuEl: HTMLDivElement | null = $state(null);
  let menuStyle = $state('');

  const selectedTagName = $derived.by(() => {
    if (!selectedTag) return null;
    const tag = tags.find(item => item.slug === selectedTag);
    return tag?.name ?? null;
  });

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        if (node.parentNode) node.parentNode.removeChild(node);
      }
    };
  }

  async function positionMenu() {
    await tick();
    if (!buttonEl || !menuEl) return;
    const btnRect = buttonEl.getBoundingClientRect();
    const menuRect = menuEl.getBoundingClientRect();
    
    let left = btnRect.left;
    let top = btnRect.bottom + 4;
    
    // Keep within viewport
    if (left + menuRect.width > window.innerWidth - 8) {
      left = window.innerWidth - menuRect.width - 8;
    }
    if (top + menuRect.height > window.innerHeight - 8) {
      top = btnRect.top - menuRect.height - 4;
    }
    
    menuStyle = `left: ${left}px; top: ${top}px;`;
  }

  function toggleMenu() {
    isOpen = !isOpen;
    if (isOpen) positionMenu();
  }

  function selectTag(slug: string | null) {
    onTagChange(slug);
    isOpen = false;
  }

  function handleClickOutside(event: MouseEvent) {
    if (isOpen && menuEl && !menuEl.contains(event.target as Node) &&
        buttonEl && !buttonEl.contains(event.target as Node)) {
      isOpen = false;
    }
  }

  $effect(() => {
    if (isOpen) {
      document.addEventListener('click', handleClickOutside);
      return () => document.removeEventListener('click', handleClickOutside);
    }
  });
</script>

<div class="tag-filter-wrapper">
  <button
    class="tag-filter-btn"
    class:active={selectedTag !== null}
    bind:this={buttonEl}
    onclick={toggleMenu}
    type="button"
  >
    <span class="filter-label">
      {#if selectedTagName}
        {selectedTagName}
      {:else}
        {$t('home.allTags')}
      {/if}
    </span>
    <ChevronDown size={14} class={isOpen ? 'rotated' : ''} />
  </button>

  {#if isOpen}
    <div class="tag-menu" bind:this={menuEl} style={menuStyle} use:portal>
      <button
        class="tag-item"
        class:selected={selectedTag === null}
        onclick={() => selectTag(null)}
      >
        <span>{$t('home.allTags')}</span>
        {#if selectedTag === null}
          <Check size={14} />
        {/if}
      </button>
      
      {#each tags as tag (tag.id)}
        <button
          class="tag-item"
          class:selected={selectedTag === tag.slug}
          onclick={() => selectTag(tag.slug)}
        >
          <span>{tag.name}</span>
          {#if selectedTag === tag.slug}
            <Check size={14} />
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .tag-filter-wrapper {
    position: relative;
  }

  .tag-filter-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: transparent;
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .tag-filter-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
    border-color: var(--border-subtle);
  }

  .tag-filter-btn.active {
    background: var(--accent-primary);
    color: white;
    border-color: var(--accent-primary);
  }

  .tag-filter-btn.active:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }

  .filter-label {
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tag-filter-btn :global(.rotated) {
    transform: rotate(180deg);
  }

  .tag-menu {
    position: fixed;
    z-index: 30000;
    min-width: 140px;
    max-height: 280px;
    overflow-y: auto;
    background-color: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 4px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
  }

  .tag-item {
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    color: var(--text-secondary);
    text-align: left;
    font-size: 12px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    transition: background-color 150ms ease, color 150ms ease;
  }

  .tag-item:hover {
    background-color: var(--bg-hover);
    color: var(--text-primary);
  }

  .tag-item.selected {
    color: var(--accent-primary);
  }

  .tag-item.selected:hover {
    color: var(--accent-primary);
  }
</style>
