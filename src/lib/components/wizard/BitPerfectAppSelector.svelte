<script lang="ts">
  import { t } from '$lib/i18n';

  interface Props {
    selectedApps?: string[];
    onchange?: (apps: string[]) => void;
  }

  let { selectedApps = $bindable(['QBZ']), onchange }: Props = $props();

  // App definitions with binary names for config
  const predefinedApps = [
    { id: 'qbz', labelKey: 'dacWizard.pulseConfig.apps.qbz', binary: 'QBZ', disabled: true },
    { id: 'vlc', labelKey: 'dacWizard.pulseConfig.apps.vlc', binary: 'vlc' },
    { id: 'deadbeef', labelKey: 'dacWizard.pulseConfig.apps.deadbeef', binary: 'deadbeef' },
    { id: 'strawberry', labelKey: 'dacWizard.pulseConfig.apps.strawberry', binary: 'strawberry' },
    { id: 'clementine', labelKey: 'dacWizard.pulseConfig.apps.clementine', binary: 'clementine' },
    { id: 'qobuz-player', labelKey: 'dacWizard.pulseConfig.apps.qobuzPlayer', binary: 'qobuz-player' },
  ];

  let customBinary = $state('');
  let showCustomInput = $state(false);

  function isSelected(binary: string): boolean {
    return selectedApps.includes(binary);
  }

  function toggleApp(binary: string) {
    if (binary === 'QBZ') return; // QBZ is always selected

    if (isSelected(binary)) {
      selectedApps = selectedApps.filter(a => a !== binary);
    } else {
      selectedApps = [...selectedApps, binary];
    }
    onchange?.(selectedApps);
  }

  function addCustomApp() {
    const trimmed = customBinary.trim();
    if (trimmed && !selectedApps.includes(trimmed)) {
      selectedApps = [...selectedApps, trimmed];
      customBinary = '';
      onchange?.(selectedApps);
    }
  }

  function handleCustomKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      addCustomApp();
    }
  }
</script>

<div class="app-selector">
  <div class="app-list">
    {#each predefinedApps as app}
      <label class="app-option" class:disabled={app.disabled}>
        <input
          type="checkbox"
          checked={isSelected(app.binary)}
          disabled={app.disabled}
          onchange={() => toggleApp(app.binary)}
        />
        <span class="app-label">{$t(app.labelKey)}</span>
        {#if app.disabled}
          <span class="app-required">(required)</span>
        {/if}
      </label>
    {/each}

    <!-- Custom app option -->
    <label class="app-option custom-toggle">
      <input
        type="checkbox"
        checked={showCustomInput}
        onchange={() => showCustomInput = !showCustomInput}
      />
      <span class="app-label">{$t('dacWizard.pulseConfig.apps.custom')}</span>
    </label>
  </div>

  {#if showCustomInput}
    <div class="custom-input-row">
      <input
        type="text"
        class="custom-input"
        placeholder={$t('dacWizard.pulseConfig.customPlaceholder')}
        bind:value={customBinary}
        onkeydown={handleCustomKeydown}
      />
      <button class="add-btn" onclick={addCustomApp} disabled={!customBinary.trim()}>
        +
      </button>
    </div>
  {/if}

  {#if selectedApps.filter(a => !predefinedApps.some(p => p.binary === a)).length > 0}
    <div class="custom-apps">
      <span class="custom-apps-label">Custom apps:</span>
      {#each selectedApps.filter(a => !predefinedApps.some(p => p.binary === a)) as app}
        <span class="custom-app-tag">
          {app}
          <button class="remove-btn" onclick={() => toggleApp(app)}>×</button>
        </span>
      {/each}
    </div>
  {/if}
</div>

<style>
  .app-selector {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .app-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .app-option {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .app-option:hover:not(.disabled) {
    background: var(--bg-hover);
  }

  .app-option.disabled {
    opacity: 0.7;
    cursor: not-allowed;
  }

  .app-option:has(input:checked) {
    border-color: var(--accent-primary);
    background: rgba(66, 133, 244, 0.08);
  }

  .app-option input {
    accent-color: var(--accent-primary);
  }

  .app-label {
    font-size: 14px;
    color: var(--text-primary);
  }

  .app-required {
    font-size: 12px;
    color: var(--text-muted);
    margin-left: auto;
  }

  .custom-input-row {
    display: flex;
    gap: 8px;
  }

  .custom-input {
    flex: 1;
    padding: 8px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    font-size: 14px;
    color: var(--text-primary);
    font-family: var(--font-mono, monospace);
  }

  .custom-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .custom-input::placeholder {
    color: var(--text-muted);
  }

  .add-btn {
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--accent-primary);
    border: none;
    border-radius: 6px;
    color: white;
    font-size: 20px;
    cursor: pointer;
    transition: opacity 150ms ease;
  }

  .add-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .add-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .custom-apps {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 8px;
    background: var(--bg-tertiary);
    border-radius: 6px;
  }

  .custom-apps-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .custom-app-tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    background: var(--accent-primary);
    color: white;
    border-radius: 4px;
    font-size: 13px;
    font-family: var(--font-mono, monospace);
  }

  .remove-btn {
    background: none;
    border: none;
    color: white;
    font-size: 16px;
    cursor: pointer;
    padding: 0 2px;
    opacity: 0.7;
    transition: opacity 150ms ease;
  }

  .remove-btn:hover {
    opacity: 1;
  }
</style>
