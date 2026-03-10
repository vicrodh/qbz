<script lang="ts">
  import { Heart, HardDrive, ChevronDown, User, Disc, Music } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Props {
    activeView: string;
    activeItemId?: string | number;
    onNavigate: (view: string, itemId?: string | number) => void;
    favoritesTabOrder?: string[];
    position?: 'left' | 'right';
  }

  let {
    activeView,
    activeItemId,
    onNavigate,
    favoritesTabOrder = ['tracks', 'albums', 'artists'],
    position = 'left'
  }: Props = $props();

  let discoverOpen = $state(false);
  let favoritesOpen = $state(false);
  let discoverTimeout: ReturnType<typeof setTimeout> | null = null;
  let favoritesTimeout: ReturnType<typeof setTimeout> | null = null;

  function isDiscoverActive(): boolean {
    return activeView === 'home';
  }

  function isFavoritesActive(): boolean {
    return activeView.startsWith('favorites-');
  }

  function openDiscover() {
    if (discoverTimeout) { clearTimeout(discoverTimeout); discoverTimeout = null; }
    discoverOpen = true;
    favoritesOpen = false;
  }

  function closeDiscoverDelayed() {
    discoverTimeout = setTimeout(() => { discoverOpen = false; }, 200);
  }

  function keepDiscover() {
    if (discoverTimeout) { clearTimeout(discoverTimeout); discoverTimeout = null; }
  }

  function openFavorites() {
    if (favoritesTimeout) { clearTimeout(favoritesTimeout); favoritesTimeout = null; }
    favoritesOpen = true;
    discoverOpen = false;
  }

  function closeFavoritesDelayed() {
    favoritesTimeout = setTimeout(() => { favoritesOpen = false; }, 200);
  }

  function keepFavorites() {
    if (favoritesTimeout) { clearTimeout(favoritesTimeout); favoritesTimeout = null; }
  }

  function handleDiscoverItem(tab: 'home' | 'editorPicks' | 'forYou') {
    onNavigate('home', tab);
    discoverOpen = false;
  }

  function handleFavoritesItem(view: string) {
    onNavigate(view);
    favoritesOpen = false;
  }

  function handleLibrary() {
    onNavigate('library');
  }

  // Close dropdowns on outside click
  function handleWindowClick(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (!target.closest('.titlebar-nav')) {
      discoverOpen = false;
      favoritesOpen = false;
    }
  }
</script>

<svelte:window onclick={handleWindowClick} />

<div class="titlebar-nav" class:pos-left={position === 'left'} class:pos-right={position === 'right'} data-tauri-drag-region="false">
  <!-- Discover (with dropdown) -->
  <div
    class="nav-btn-wrapper"
    onmouseenter={openDiscover}
    onmouseleave={closeDiscoverDelayed}
  >
    <button
      class="nav-btn"
      class:active={isDiscoverActive()}
      onclick={() => onNavigate('home')}
      data-tauri-drag-region="false"
    >
      <svg width="12" height="12" viewBox="0 0 64 64" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><circle cx="32" cy="32" r="4"/><path d="M32,0C14.328,0,0,14.328,0,32s14.328,32,32,32s32-14.328,32-32S49.672,0,32,0z M40,40l-22,6l6-22l22-6L40,40z"/></svg>
      <span class="nav-label">{$t('nav.home')}</span>
      <ChevronDown size={10} />
    </button>
    {#if discoverOpen}
      <div
        class="dropdown"
        onmouseenter={keepDiscover}
        onmouseleave={closeDiscoverDelayed}
      >
        <button
          class="dropdown-item"
          class:active={activeView === 'home' && (!activeItemId || activeItemId === 'home')}
          onclick={() => handleDiscoverItem('home')}
        >
          {$t('home.title')}
        </button>
        <button
          class="dropdown-item"
          class:active={activeView === 'home' && activeItemId === 'editorPicks'}
          onclick={() => handleDiscoverItem('editorPicks')}
        >
          {$t('home.tabEditorPicks')}
        </button>
        <button
          class="dropdown-item"
          class:active={activeView === 'home' && activeItemId === 'forYou'}
          onclick={() => handleDiscoverItem('forYou')}
        >
          {$t('home.tabForYou')}
        </button>
      </div>
    {/if}
  </div>

  <!-- Favorites (with dropdown) -->
  <div
    class="nav-btn-wrapper"
    onmouseenter={openFavorites}
    onmouseleave={closeFavoritesDelayed}
  >
    <button
      class="nav-btn"
      class:active={isFavoritesActive()}
      onclick={() => onNavigate('favorites-tracks')}
      data-tauri-drag-region="false"
    >
      <Heart size={12} />
      <span class="nav-label">{$t('nav.favorites')}</span>
      <ChevronDown size={10} />
    </button>
    {#if favoritesOpen}
      <div
        class="dropdown"
        onmouseenter={keepFavorites}
        onmouseleave={closeFavoritesDelayed}
      >
        {#each favoritesTabOrder as tab}
          <button
            class="dropdown-item"
            class:active={activeView === `favorites-${tab}`}
            onclick={() => handleFavoritesItem(`favorites-${tab}`)}
          >
            {#if tab === 'tracks'}
              <Music size={12} />
            {:else if tab === 'albums'}
              <Disc size={12} />
            {:else if tab === 'artists'}
              <User size={12} />
            {/if}
            <span>{$t(`favorites.${tab}`)}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Local Library (no dropdown) -->
  <button
    class="nav-btn"
    class:active={activeView === 'library' || activeView === 'library-album'}
    onclick={handleLibrary}
    data-tauri-drag-region="false"
  >
    <HardDrive size={12} />
    <span class="nav-label">{$t('library.browse')}</span>
  </button>
</div>

<style>
  .titlebar-nav {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 100%;
    -webkit-app-region: no-drag;
    app-region: no-drag;
    flex-shrink: 0;
  }

  .titlebar-nav.pos-left {
    padding-left: 8px;
    padding-right: 4px;
  }

  .titlebar-nav.pos-right {
    padding-left: 4px;
    padding-right: 8px;
  }

  .nav-btn-wrapper {
    position: relative;
    height: 100%;
    display: flex;
    align-items: center;
  }

  .nav-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 8px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 150ms ease, opacity 150ms ease;
    white-space: nowrap;
    -webkit-app-region: no-drag;
    app-region: no-drag;
  }

  .nav-btn:hover {
    background-color: var(--alpha-10);
  }

  .nav-btn.active {
    background-color: var(--alpha-10);
  }

  .nav-btn :global(svg) {
    flex-shrink: 0;
    opacity: 0.8;
  }

  .nav-label {
    line-height: 1;
  }

  /* Dropdown */
  .dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    min-width: 160px;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 210000;
    margin-top: 2px;
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
    transition: background-color 120ms ease, color 120ms ease;
    text-align: left;
  }

  .dropdown-item:hover {
    background-color: var(--bg-hover);
    color: var(--text-primary);
  }

  .dropdown-item.active {
    color: var(--text-primary);
    font-weight: 500;
  }

  .dropdown-item :global(svg) {
    flex-shrink: 0;
    opacity: 0.7;
  }
</style>
