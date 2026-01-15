<script lang="ts">
  import { onMount } from 'svelte';
  import { ChevronUp, ChevronDown, RotateCcw } from 'lucide-svelte';
  import Modal from './Modal.svelte';
  import {
    getSettings,
    subscribe,
    setSectionVisibility,
    moveSectionUp,
    moveSectionDown,
    setGreetingEnabled,
    setCustomGreeting,
    resetToDefaults,
    updateLimit,
    type HomeSettings
  } from '$lib/stores/homeSettingsStore';
  import { t } from '$lib/i18n';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  let settings = $state<HomeSettings>(getSettings());
  let customGreetingInput = $state(settings.greeting.customText || '');

  // Subscribe to settings changes (use onMount to avoid reactive tracking)
  onMount(() => {
    const unsubscribe = subscribe(() => {
      settings = getSettings();
      customGreetingInput = settings.greeting.customText || '';
    });
    return unsubscribe;
  });

  function handleToggleSection(sectionId: string, visible: boolean) {
    setSectionVisibility(sectionId as any, visible);
  }

  function handleMoveUp(sectionId: string) {
    moveSectionUp(sectionId as any);
  }

  function handleMoveDown(sectionId: string) {
    moveSectionDown(sectionId as any);
  }

  function handleToggleGreeting(enabled: boolean) {
    setGreetingEnabled(enabled);
  }

  function handleCustomGreetingChange() {
    setCustomGreeting(customGreetingInput || null);
  }

  function handleReset() {
    resetToDefaults();
  }

  function handleLimitChange(key: keyof HomeSettings['limits'], value: number) {
    updateLimit(key, value);
  }
</script>

<Modal {isOpen} {onClose} title={$t('home.customizeHome')}>
  {#snippet children()}
    <!-- Greeting Settings -->
    <div class="settings-section">
      <div class="section-title">{$t('homeSettings.greeting')}</div>

      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{$t('homeSettings.showGreeting')}</span>
          <span class="setting-desc">{$t('homeSettings.showGreetingDesc')}</span>
        </div>
        <label class="toggle">
          <input
            type="checkbox"
            checked={settings.greeting.enabled}
            onchange={(e) => handleToggleGreeting(e.currentTarget.checked)}
          />
          <span class="toggle-slider"></span>
        </label>
      </div>

      {#if settings.greeting.enabled}
        <div class="setting-row greeting-input-row">
          <div class="setting-info">
            <span class="setting-label">{$t('homeSettings.customGreeting')}</span>
            <span class="setting-desc">{$t('homeSettings.customGreetingDesc')}</span>
          </div>
          <input
            type="text"
            class="greeting-input"
            placeholder={$t('homeSettings.customGreetingPlaceholder')}
            bind:value={customGreetingInput}
            onblur={handleCustomGreetingChange}
            onkeydown={(e) => e.key === 'Enter' && handleCustomGreetingChange()}
          />
        </div>
      {/if}
    </div>

    <!-- Item Limits -->
    <div class="settings-section">
      <div class="section-title">Item Limits</div>
      <p class="section-desc">Configure how many items to show in each section</p>

      <div class="limits-grid">
        <div class="limit-item">
          <label class="limit-label" for="limit-recent">Recently Played</label>
          <input
            id="limit-recent"
            type="number"
            class="limit-input"
            min="1"
            max="100"
            value={settings.limits.recentAlbums}
            onchange={(e) => handleLimitChange('recentAlbums', Number(e.currentTarget.value))}
          />
        </div>

        <div class="limit-item">
          <label class="limit-label" for="limit-continue">Continue Listening</label>
          <input
            id="limit-continue"
            type="number"
            class="limit-input"
            min="1"
            max="100"
            value={settings.limits.continueTracks}
            onchange={(e) => handleLimitChange('continueTracks', Number(e.currentTarget.value))}
          />
        </div>

        <div class="limit-item">
          <label class="limit-label" for="limit-artists">Top Artists</label>
          <input
            id="limit-artists"
            type="number"
            class="limit-input"
            min="1"
            max="100"
            value={settings.limits.topArtists}
            onchange={(e) => handleLimitChange('topArtists', Number(e.currentTarget.value))}
          />
        </div>

        <div class="limit-item">
          <label class="limit-label" for="limit-fav-albums">Favorite Albums</label>
          <input
            id="limit-fav-albums"
            type="number"
            class="limit-input"
            min="1"
            max="100"
            value={settings.limits.favoriteAlbums}
            onchange={(e) => handleLimitChange('favoriteAlbums', Number(e.currentTarget.value))}
          />
        </div>

        <div class="limit-item">
          <label class="limit-label" for="limit-fav-tracks">Favorite Tracks</label>
          <input
            id="limit-fav-tracks"
            type="number"
            class="limit-input"
            min="1"
            max="100"
            value={settings.limits.favoriteTracks}
            onchange={(e) => handleLimitChange('favoriteTracks', Number(e.currentTarget.value))}
          />
        </div>

        <div class="limit-item">
          <label class="limit-label" for="limit-featured">Featured Albums</label>
          <input
            id="limit-featured"
            type="number"
            class="limit-input"
            min="1"
            max="100"
            value={settings.limits.featuredAlbums}
            onchange={(e) => handleLimitChange('featuredAlbums', Number(e.currentTarget.value))}
          />
        </div>
      </div>
    </div>

    <!-- Sections Order -->
    <div class="settings-section">
      <div class="section-title">{$t('homeSettings.sections')}</div>
      <p class="section-desc">{$t('homeSettings.sectionsDesc')}</p>

      <div class="sections-list">
        {#each settings.sections as section, index (section.id)}
          <div class="section-item" class:disabled={!section.visible}>
            <div class="section-controls">
              <button
                class="order-btn"
                onclick={() => handleMoveUp(section.id)}
                disabled={index === 0}
                title={$t('homeSettings.moveUp')}
              >
                <ChevronUp size={16} />
              </button>
              <button
                class="order-btn"
                onclick={() => handleMoveDown(section.id)}
                disabled={index === settings.sections.length - 1}
                title={$t('homeSettings.moveDown')}
              >
                <ChevronDown size={16} />
              </button>
            </div>

            <span class="section-label">
              {section.label}
              {#if section.source === 'qobuz'}
                <span class="source-badge qobuz">Qobuz</span>
              {:else if section.source === 'ml'}
                <span class="source-badge ml">ML</span>
              {/if}
            </span>

            <label class="toggle">
              <input
                type="checkbox"
                checked={section.visible}
                onchange={(e) => handleToggleSection(section.id, e.currentTarget.checked)}
              />
              <span class="toggle-slider"></span>
            </label>
          </div>
        {/each}
      </div>
    </div>
  {/snippet}

  {#snippet footer()}
    <button class="reset-btn" onclick={handleReset}>
      <RotateCcw size={14} />
      {$t('homeSettings.resetToDefaults')}
    </button>
  {/snippet}
</Modal>

<style>
  .settings-section {
    margin-bottom: 24px;
  }

  .settings-section:last-child {
    margin-bottom: 0;
  }

  .section-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 4px;
  }

  .section-desc {
    font-size: 12px;
    color: var(--text-muted);
    margin: 0 0 12px 0;
  }

  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 0;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .setting-row:last-child {
    border-bottom: none;
  }

  .greeting-input-row {
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
  }

  .greeting-input-row .setting-info {
    width: 100%;
  }

  .setting-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .setting-label {
    font-size: 14px;
    color: var(--text-primary);
  }

  .setting-desc {
    font-size: 12px;
    color: var(--text-muted);
  }

  .greeting-input {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    color: var(--text-primary);
    font-size: 14px;
    transition: border-color 150ms ease;
  }

  .greeting-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .greeting-input::placeholder {
    color: var(--text-muted);
  }

  /* Toggle Switch */
  .toggle {
    position: relative;
    display: inline-block;
    width: 44px;
    height: 24px;
    flex-shrink: 0;
  }

  .toggle input {
    opacity: 0;
    width: 0;
    height: 0;
  }

  .toggle-slider {
    position: absolute;
    cursor: pointer;
    inset: 0;
    background-color: var(--bg-tertiary);
    border-radius: 24px;
    transition: all 200ms ease;
  }

  .toggle-slider::before {
    position: absolute;
    content: "";
    height: 18px;
    width: 18px;
    left: 3px;
    bottom: 3px;
    background-color: white;
    border-radius: 50%;
    transition: transform 200ms ease;
  }

  .toggle input:checked + .toggle-slider {
    background-color: var(--accent-primary);
  }

  .toggle input:checked + .toggle-slider::before {
    transform: translateX(20px);
  }

  /* Sections List */
  .sections-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--bg-secondary);
    border-radius: 8px;
    overflow: hidden;
  }

  .section-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    background: var(--bg-secondary);
    transition: all 150ms ease;
  }

  .section-item:hover {
    background: var(--bg-hover);
  }

  .section-item.disabled {
    opacity: 0.5;
  }

  .section-controls {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .order-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 20px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .order-btn:hover:not(:disabled) {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .order-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .section-label {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-primary);
  }

  .section-item.disabled .section-label {
    color: var(--text-muted);
  }

  .source-badge {
    font-size: 10px;
    font-weight: 500;
    padding: 2px 6px;
    border-radius: 4px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .source-badge.qobuz {
    color: var(--accent-primary);
    background: rgba(99, 102, 241, 0.15);
  }

  .source-badge.ml {
    color: #10b981;
    background: rgba(16, 185, 129, 0.15);
  }

  .reset-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    background: transparent;
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .reset-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-muted);
    color: var(--text-primary);
  }

  .limits-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 16px;
  }

  .limit-item {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .limit-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .limit-input {
    padding: 8px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    transition: all 150ms ease;
  }

  .limit-input:focus {
    outline: none;
    border-color: var(--accent);
  }

  .limit-input:hover {
    border-color: var(--text-muted);
  }
</style>
