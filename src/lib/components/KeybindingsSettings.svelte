<script lang="ts">
  import { X, Settings, RotateCcw } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import ShortcutInput from './ShortcutInput.svelte';
  import {
    ACTIONS,
    CATEGORIES,
    DEFAULT_BINDINGS,
    getActiveBindings,
    setBinding,
    resetBinding,
    resetAllBindings,
    type KeybindingCategory,
  } from '$lib/stores/keybindingsStore';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  let showResetConfirm = $state(false);
  let bindingsVersion = $state(0); // Force reactivity

  function handleShortcutChange(actionId: string, newShortcut: string) {
    setBinding(actionId, newShortcut);
    bindingsVersion++; // Trigger update
  }

  function handleReset(actionId: string) {
    resetBinding(actionId);
    bindingsVersion++;
  }

  function handleResetAll() {
    showResetConfirm = true;
  }

  function confirmResetAll() {
    resetAllBindings();
    bindingsVersion++;
    showResetConfirm = false;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!isOpen) return;
    // Only close with Escape if not recording a shortcut
    if (e.key === 'Escape' && !document.querySelector('.shortcut-input.recording')) {
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

  // Count modifications
  function getModifiedCount(): number {
    void bindingsVersion; // Depend on version for reactivity
    const bindings = getActiveBindings();
    return ACTIONS.filter(a => bindings[a.id] !== DEFAULT_BINDINGS[a.id]).length;
  }

  // Get category label with i18n
  function getCategoryLabel(categoryId: KeybindingCategory): string {
    const translated = $t(`keybindings.categories.${categoryId}`);
    if (translated && !translated.startsWith('keybindings.categories.')) {
      return translated;
    }
    return CATEGORIES.find(c => c.id === categoryId)?.label || categoryId;
  }
</script>

<svelte:document onkeydown={handleKeydown} />

{#if isOpen}
  <div class="modal-backdrop" onclick={handleBackdropClick} role="presentation">
    <div
      class="modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="keybindings-settings-title"
      onclick={(e) => e.stopPropagation()}
    >
      <header class="modal-header">
        <div class="header-title">
          <Settings size={20} />
          <h2 id="keybindings-settings-title">{$t('keybindings.customize') || 'Customize Shortcuts'}</h2>
          {#if getModifiedCount() > 0}
            <span class="modified-badge">
              {getModifiedCount()} {$t('keybindings.modified') || 'modified'}
            </span>
          {/if}
        </div>
        <button class="close-btn" onclick={onClose} title={$t('actions.close')}>
          <X size={20} />
        </button>
      </header>

      <div class="modal-content">
        {#each CATEGORIES as category}
          {@const actions = ACTIONS.filter(a => a.category === category.id)}
          {#if actions.length > 0}
            <section class="category-section">
              <h3 class="category-title">{getCategoryLabel(category.id)}</h3>

              {#each actions as action}
                {@const bindings = getActiveBindings()}
                <ShortcutInput
                  {action}
                  currentShortcut={bindings[action.id]}
                  defaultShortcut={DEFAULT_BINDINGS[action.id]}
                  onShortcutChange={(shortcut) => handleShortcutChange(action.id, shortcut)}
                  onReset={() => handleReset(action.id)}
                />
              {/each}
            </section>
          {/if}
        {/each}
      </div>

      <footer class="modal-footer">
        <button
          class="reset-all-btn"
          onclick={handleResetAll}
          disabled={getModifiedCount() === 0}
        >
          <RotateCcw size={14} />
          {$t('keybindings.resetAll') || 'Reset All to Defaults'}
        </button>
      </footer>
    </div>
  </div>

  <!-- Reset Confirmation Dialog -->
  {#if showResetConfirm}
    <div class="confirm-backdrop" role="presentation">
      <div class="confirm-dialog" role="alertdialog" aria-modal="true">
        <h3>{$t('keybindings.resetAllConfirmTitle') || 'Reset All Shortcuts?'}</h3>
        <p>
          {$t('keybindings.resetAllConfirmText', { values: { count: getModifiedCount() } }) ||
           `This will reset all ${getModifiedCount()} modified shortcuts to their default values.`}
        </p>
        <div class="confirm-actions">
          <button class="cancel-btn" onclick={() => showResetConfirm = false}>
            {$t('actions.cancel')}
          </button>
          <button class="confirm-btn" onclick={confirmResetAll}>
            {$t('keybindings.resetAll') || 'Reset All'}
          </button>
        </div>
      </div>
    </div>
  {/if}
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
    max-width: 550px;
    max-height: 85vh;
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

  .modified-badge {
    font-size: 11px;
    padding: 2px 8px;
    background: var(--accent-primary);
    color: white;
    border-radius: 10px;
    font-weight: 500;
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
    margin-bottom: 28px;
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
    margin: 0 0 16px 0;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .modal-footer {
    padding: 16px 20px;
    border-top: 1px solid var(--border-subtle);
    display: flex;
    justify-content: center;
  }

  .reset-all-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 20px;
    background: transparent;
    border: 1px solid var(--color-error, #ef4444);
    border-radius: 8px;
    color: var(--color-error, #ef4444);
    font-size: 14px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .reset-all-btn:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.1);
  }

  .reset-all-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  /* Confirmation Dialog */
  .confirm-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1001;
  }

  .confirm-dialog {
    background: var(--bg-secondary);
    border-radius: 12px;
    padding: 24px;
    max-width: 400px;
    border: 1px solid var(--border-subtle);
  }

  .confirm-dialog h3 {
    margin: 0 0 12px 0;
    font-size: 16px;
    color: var(--text-primary);
  }

  .confirm-dialog p {
    margin: 0 0 20px 0;
    font-size: 14px;
    color: var(--text-secondary);
  }

  .confirm-actions {
    display: flex;
    gap: 12px;
    justify-content: flex-end;
  }

  .cancel-btn {
    padding: 8px 16px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .cancel-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .confirm-btn {
    padding: 8px 16px;
    background: var(--color-error, #ef4444);
    border: none;
    border-radius: 6px;
    color: white;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .confirm-btn:hover {
    background: #dc2626;
  }

  /* Responsive */
  @media (max-width: 600px) {
    .modal {
      width: 95%;
      max-height: 90vh;
    }

    .modal-header {
      padding: 14px 16px;
    }

    .modal-content {
      padding: 16px;
    }
  }
</style>
