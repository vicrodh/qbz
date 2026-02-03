<script lang="ts">
  import { Settings, SlidersHorizontal, Keyboard, Bug, HelpCircle } from 'lucide-svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { t } from '$lib/i18n';

  interface Props {
    username: string;
    subscription: string;
    onSettingsClick: () => void;
    onKeybindingsClick?: () => void;
    onAboutClick?: () => void;
    collapsed?: boolean;
  }

  let { username, subscription, onSettingsClick, onKeybindingsClick, onAboutClick, collapsed = false }: Props = $props();

  let isMenuOpen = $state(false);
  let menuRef: HTMLDivElement | null = $state(null);

  function toggleMenu(e: MouseEvent) {
    e.stopPropagation();
    isMenuOpen = !isMenuOpen;
  }

  function closeMenu() {
    isMenuOpen = false;
  }

  function handleSettingsClick() {
    closeMenu();
    onSettingsClick();
  }

  function handleKeybindingsClick() {
    closeMenu();
    onKeybindingsClick?.();
  }

  function handleAboutClick() {
    closeMenu();
    onAboutClick?.();
  }

  function handleReportBug() {
    closeMenu();
    openUrl('https://github.com/vicrodh/qbz/issues/new?template=bug_report.md')
      .catch(err => console.error('Failed to open URL:', err));
  }

  // Close menu when clicking outside
  function handleClickOutside(e: MouseEvent) {
    if (isMenuOpen && menuRef && !menuRef.contains(e.target as Node)) {
      closeMenu();
    }
  }

  // Close on Escape
  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && isMenuOpen) {
      closeMenu();
    }
  }
</script>

<svelte:document onclick={handleClickOutside} onkeydown={handleKeydown} />

<div class="user-card" class:collapsed>
  {#if !collapsed}
    <!-- App Logo -->
    <button
      class="logo-btn"
      onclick={(e) => {
        e.stopPropagation();
        onAboutClick?.();
      }}
      title="About QBZ"
    >
      <div class="logo-glow"></div>
      <img src="/icons/AppIcons/android/72x72.png" alt="QBZ" class="logo-img" />
    </button>

    <!-- User Info -->
    <div class="user-info">
      <div class="username">{username}</div>
      <div class="subscription">{subscription}</div>
    </div>
  {/if}

  <!-- Settings Button with Dropdown -->
  <div class="menu-container" bind:this={menuRef}>
    <button
      class="action-btn"
      class:active={isMenuOpen}
      class:collapsed
      onclick={toggleMenu}
      title={$t('settings.title')}
    >
      <Settings size={collapsed ? 16 : 18} />
    </button>

    {#if isMenuOpen}
      <div class="dropdown-menu" class:collapsed>
        <button class="menu-item" onclick={handleSettingsClick}>
          <SlidersHorizontal size={16} />
          <span>{$t('settings.title')}</span>
        </button>
        <button class="menu-item" onclick={handleKeybindingsClick}>
          <Keyboard size={16} />
          <span>{$t('keybindings.title')}</span>
        </button>
        <div class="menu-divider"></div>
        <button class="menu-item" onclick={handleReportBug}>
          <Bug size={16} />
          <span>{$t('settings.about.reportIssue')}</span>
        </button>
        <button class="menu-item" onclick={handleAboutClick}>
          <HelpCircle size={16} />
          <span>{$t('settings.about.title')}</span>
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .user-card {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px;
    border-radius: 6px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .user-card:hover {
    background-color: var(--bg-hover);
  }

  .user-card.collapsed {
    justify-content: center;
    padding: 0;
  }

  .logo-btn {
    position: relative;
    width: 32px;
    height: 32px;
    padding: 0;
    background: none;
    border: none;
    cursor: pointer;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .logo-glow {
    position: absolute;
    inset: -4px;
    border-radius: 50%;
    background: radial-gradient(circle, rgba(139, 92, 246, 0.4) 0%, transparent 70%);
    opacity: 0;
    transition: opacity 200ms ease;
    pointer-events: none;
  }

  .logo-btn:hover .logo-glow {
    opacity: 1;
  }

  .logo-img {
    width: 30px;
    height: 30px;
    object-fit: contain;
    position: relative;
    z-index: 1;
  }

  .user-info {
    flex: 1;
    min-width: 0;
  }

  .username {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .subscription {
    font-size: 10px;
    color: var(--accent-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .menu-container {
    position: relative;
  }

  .action-btn {
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
    border-radius: 4px;
    transition: all 150ms ease;
  }

  .action-btn:hover,
  .action-btn.active {
    color: var(--text-primary);
    background-color: var(--bg-tertiary);
  }

  .action-btn.collapsed {
    width: 40px;
    height: 40px;
    border-radius: 8px;
  }

  .dropdown-menu {
    position: absolute;
    bottom: 100%;
    right: 0;
    margin-bottom: 6px;
    min-width: 210px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    padding: 3px;
    z-index: 10000;
    animation: slideUp 150ms ease-out;
  }

  /* Collapsed mode: open to the right */
  .dropdown-menu.collapsed {
    bottom: auto;
    right: auto;
    left: 100%;
    top: 0;
    margin-bottom: 0;
    margin-left: 8px;
    animation: slideRight 150ms ease-out;
  }

  @keyframes slideUp {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @keyframes slideRight {
    from {
      opacity: 0;
      transform: translateX(-4px);
    }
    to {
      opacity: 1;
      transform: translateX(0);
    }
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 10px;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 13px;
    cursor: pointer;
    text-align: left;
    transition: background-color 150ms ease, color 150ms ease;
  }

  .menu-item:hover {
    background-color: var(--bg-hover);
    color: var(--text-primary);
  }

  .menu-item :global(svg) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .menu-item:hover :global(svg) {
    color: var(--text-secondary);
  }

  .menu-divider {
    height: 1px;
    background: var(--border-subtle);
    margin: 3px 6px;
  }
</style>
