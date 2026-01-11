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
    type HomeSettings
  } from '$lib/stores/homeSettingsStore';

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
</script>

<Modal {isOpen} {onClose} title="Customize Home">
  {#snippet children()}
    <!-- Greeting Settings -->
    <div class="settings-section">
      <div class="section-title">Greeting</div>

      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">Show greeting</span>
          <span class="setting-desc">Display personalized greeting at top</span>
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
            <span class="setting-label">Custom greeting</span>
            <span class="setting-desc">Use {'{name}'} for your name. Leave empty for time-based.</span>
          </div>
          <input
            type="text"
            class="greeting-input"
            placeholder="Good vibes, {name}!"
            bind:value={customGreetingInput}
            onblur={handleCustomGreetingChange}
            onkeydown={(e) => e.key === 'Enter' && handleCustomGreetingChange()}
          />
        </div>
      {/if}
    </div>

    <!-- Sections Order -->
    <div class="settings-section">
      <div class="section-title">Sections</div>
      <p class="section-desc">Reorder with arrows. Toggle visibility with switch.</p>

      <div class="sections-list">
        {#each settings.sections as section, index (section.id)}
          <div class="section-item" class:disabled={!section.visible}>
            <div class="section-controls">
              <button
                class="order-btn"
                onclick={() => handleMoveUp(section.id)}
                disabled={index === 0}
                title="Move up"
              >
                <ChevronUp size={16} />
              </button>
              <button
                class="order-btn"
                onclick={() => handleMoveDown(section.id)}
                disabled={index === settings.sections.length - 1}
                title="Move down"
              >
                <ChevronDown size={16} />
              </button>
            </div>

            <span class="section-label">
              {section.label}
              {#if section.source === 'qobuz'}
                <span class="source-badge">Qobuz</span>
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
      Reset to defaults
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
    color: var(--accent-primary);
    background: rgba(99, 102, 241, 0.15);
    padding: 2px 6px;
    border-radius: 4px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
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
</style>
