<script lang="ts">
  import { X, Keyboard } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import {
    ACTIONS,
    CATEGORIES,
    getActiveBindings,
    formatShortcutDisplay,
    type KeybindingCategory,
  } from '$lib/stores/keybindingsStore';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    onOpenSettings?: () => void;
  }

  let { isOpen, onClose, onOpenSettings }: Props = $props();

  // Group actions by category
  function getActionsByCategory(): Map<KeybindingCategory, typeof ACTIONS> {
    const grouped = new Map<KeybindingCategory, typeof ACTIONS>();

    for (const category of CATEGORIES) {
      const actions = ACTIONS.filter(a => a.category === category.id);
      if (actions.length > 0) {
        grouped.set(category.id, actions);
      }
    }

    return grouped;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!isOpen) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  }

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onClose();
    }
  }

  // Get i18n key for category
  function getCategoryLabel(categoryId: KeybindingCategory): string {
    const key = `keybindings.categories.${categoryId}`;
    return $t(key) || CATEGORIES.find(c => c.id === categoryId)?.label || categoryId;
  }

  // Get i18n key for action label
  function getActionLabel(actionId: string): string {
    // Convert action.id like 'playback.toggle' to 'playbackToggle'
    const key = actionId.replace('.', '');
    const camelKey = key.charAt(0).toLowerCase() + key.slice(1).replace(/\.(\w)/g, (_, c) => c.toUpperCase());
    const i18nKey = `keybindings.actions.${camelKey}`;
    const translated = $t(i18nKey);
    // Fallback to action's label if translation not found
    if (translated === i18nKey) {
      return ACTIONS.find(a => a.id === actionId)?.label || actionId;
    }
    return translated;
  }

  // Convert actionId to i18n camelCase key
  function actionIdToKey(actionId: string): string {
    // 'playback.toggle' -> 'playbackToggle'
    // 'nav.back' -> 'navBack'
    // 'ui.focusMode' -> 'uiFocusMode'
    // 'focus.seekForward' -> 'focusSeekForward'
    const parts = actionId.split('.');
    if (parts.length === 2) {
      return parts[0] + parts[1].charAt(0).toUpperCase() + parts[1].slice(1);
    }
    return actionId;
  }
</script>

<svelte:document onkeydown={handleKeydown} />

{#if isOpen}
  <div
    class="modal-backdrop"
    onclick={handleBackdropClick}
    role="presentation"
  >
    <div
      class="modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="shortcuts-modal-title"
      onclick={(e) => e.stopPropagation()}
    >
      <header class="modal-header">
        <div class="header-title">
          <Keyboard size={20} />
          <h2 id="shortcuts-modal-title">{$t('keybindings.title')}</h2>
        </div>
        <button class="close-btn" onclick={onClose} title={$t('actions.close')}>
          <X size={20} />
        </button>
      </header>

      <div class="modal-content">
        {#each CATEGORIES as category}
          {@const actions = getActionsByCategory().get(category.id)}
          {#if actions && actions.length > 0}
            <section class="category-section">
              <h3 class="category-title">{getCategoryLabel(category.id)}</h3>

              <div class="shortcuts-list">
                {#each actions as action}
                  <div class="shortcut-row">
                    <span class="action-label">
                      {getActionLabel(action.id)}
                    </span>
                    <kbd class="shortcut-key">
                      {formatShortcutDisplay(getActiveBindings()[action.id])}
                    </kbd>
                  </div>
                {/each}
              </div>
            </section>
          {/if}
        {/each}
      </div>

      {#if onOpenSettings}
        <footer class="modal-footer">
          <button class="customize-btn" onclick={onOpenSettings}>
            {$t('settings.title')}
          </button>
        </footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    backdrop-filter: blur(4px);
    animation: fadeIn 150ms ease-out;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .modal {
    background: var(--bg-secondary);
    border-radius: 12px;
    width: 90%;
    max-width: 500px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    border: 1px solid var(--border-subtle);
    animation: slideUp 200ms ease-out;
  }

  @keyframes slideUp {
    from {
      opacity: 0;
      transform: translateY(16px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .header-title {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-primary);
  }

  .header-title h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 150ms ease;
  }

  .close-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .modal-content {
    flex: 1;
    overflow-y: auto;
    padding: 20px;
  }

  .category-section {
    margin-bottom: 24px;
  }

  .category-section:last-child {
    margin-bottom: 0;
  }

  .category-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    margin: 0 0 12px 0;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .shortcuts-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .shortcut-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border-radius: 6px;
  }

  .action-label {
    font-size: 14px;
    color: var(--text-primary);
  }

  .shortcut-key {
    font-family: var(--font-mono, monospace);
    font-size: 12px;
    padding: 4px 8px;
    background: var(--bg-primary);
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    color: var(--text-secondary);
    min-width: 60px;
    text-align: center;
  }

  .modal-footer {
    padding: 16px 20px;
    border-top: 1px solid var(--border-subtle);
    display: flex;
    justify-content: center;
  }

  .customize-btn {
    padding: 10px 20px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    color: var(--text-secondary);
    font-size: 14px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .customize-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
    border-color: var(--accent-primary);
  }

  /* Responsive */
  @media (max-width: 600px) {
    .modal {
      width: 95%;
      max-height: 85vh;
    }

    .modal-header {
      padding: 14px 16px;
    }

    .modal-content {
      padding: 16px;
    }

    .shortcut-row {
      padding: 6px 10px;
    }
  }
</style>
