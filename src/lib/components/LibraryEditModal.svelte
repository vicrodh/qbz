<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import Modal from './Modal.svelte';
  import { GripVertical, Eye, EyeOff } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import type { LibraryPreferences } from '../types';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    onSave: (prefs: LibraryPreferences) => void;
    initialPreferences: LibraryPreferences;
  }

  let { isOpen, onClose, onSave, initialPreferences }: Props = $props();

  // Tab list known to this modal. Keep in sync with LocalLibraryView.TabType.
  const availableTabs: string[] = ['tracks', 'folders', 'artists'];

  // Default preferences used by the "Reset" button.
  const DEFAULT_TAB_ORDER: string[] = ['tracks', 'folders', 'artists'];

  let tabOrder = $state<string[]>([]);
  let hiddenTabs = $state<string[]>([]);
  let saving = $state(false);

  function getTabLabel(tab: string): string {
    return $t(`library.${tab}`);
  }

  function syncFromPreferences() {
    // Sanitize: keep only known tabs, append any missing ones at the end so
    // older preferences still surface new tabs added in future releases.
    const validOrder = (initialPreferences?.tab_order ?? []).filter((tab) =>
      availableTabs.includes(tab),
    );
    for (const tab of availableTabs) {
      if (!validOrder.includes(tab)) validOrder.push(tab);
    }
    tabOrder = validOrder;

    const validHidden = (initialPreferences?.hidden_tabs ?? []).filter((tab) =>
      availableTabs.includes(tab),
    );
    hiddenTabs = validHidden;
  }

  function moveUp(index: number) {
    if (index === 0) return;
    const temp = tabOrder[index];
    tabOrder[index] = tabOrder[index - 1];
    tabOrder[index - 1] = temp;
  }

  function moveDown(index: number) {
    if (index === tabOrder.length - 1) return;
    const temp = tabOrder[index];
    tabOrder[index] = tabOrder[index + 1];
    tabOrder[index + 1] = temp;
  }

  function toggleHidden(tab: string) {
    if (hiddenTabs.includes(tab)) {
      hiddenTabs = hiddenTabs.filter((current) => current !== tab);
    } else {
      hiddenTabs = [...hiddenTabs, tab];
    }
  }

  function isHidden(tab: string): boolean {
    return hiddenTabs.includes(tab);
  }

  function handleReset() {
    tabOrder = [...DEFAULT_TAB_ORDER];
    hiddenTabs = [];
  }

  async function handleSave() {
    saving = true;
    try {
      const prefs: LibraryPreferences = {
        tab_order: tabOrder,
        hidden_tabs: hiddenTabs,
      };
      const saved = await invoke<LibraryPreferences>('v2_save_library_preferences', { prefs });
      onSave(saved);
      onClose();
    } catch (err) {
      console.error('Failed to save library preferences:', err);
    } finally {
      saving = false;
    }
  }

  function handleCancel() {
    syncFromPreferences();
    onClose();
  }

  $effect(() => {
    if (isOpen) {
      syncFromPreferences();
    }
  });
</script>

<Modal {isOpen} onClose={handleCancel} title={$t('library.editTabs.title')} maxWidth="400px">
  {#snippet children()}
  <div class="modal-body">
    <p class="modal-help">{$t('library.editTabs.help')}</p>

    <div class="tab-order-list">
      {#each tabOrder as tab, index (tab)}
        <div class="tab-order-item" class:hidden-tab={isHidden(tab)}>
          <div class="tab-grip">
            <GripVertical size={16} />
          </div>
          <div class="tab-label">{getTabLabel(tab)}</div>
          <div class="tab-controls">
            <button
              type="button"
              class="tab-move-btn"
              onclick={() => toggleHidden(tab)}
              title={isHidden(tab) ? $t('library.editTabs.showTab') : $t('library.editTabs.hideTab')}
              aria-label={isHidden(tab) ? $t('library.editTabs.showTab') : $t('library.editTabs.hideTab')}
            >
              {#if isHidden(tab)}
                <EyeOff size={14} />
              {:else}
                <Eye size={14} />
              {/if}
            </button>
            <button
              type="button"
              class="tab-move-btn"
              onclick={() => moveUp(index)}
              disabled={index === 0}
              title={$t('actions.moveUp')}
              aria-label={$t('actions.moveUp')}
            >
              ↑
            </button>
            <button
              type="button"
              class="tab-move-btn"
              onclick={() => moveDown(index)}
              disabled={index === tabOrder.length - 1}
              title={$t('actions.moveDown')}
              aria-label={$t('actions.moveDown')}
            >
              ↓
            </button>
          </div>
        </div>
      {/each}
    </div>
  </div>
  {/snippet}

  {#snippet footer()}
  <div class="modal-actions">
    <button type="button" class="btn btn-tertiary" onclick={handleReset} disabled={saving}>
      {$t('library.editTabs.reset')}
    </button>
    <div class="modal-actions-right">
      <button type="button" class="btn btn-secondary" onclick={handleCancel} disabled={saving}>
        {$t('library.editTabs.cancel')}
      </button>
      <button type="button" class="btn btn-primary" onclick={handleSave} disabled={saving}>
        {saving ? $t('actions.saving') : $t('library.editTabs.save')}
      </button>
    </div>
  </div>
  {/snippet}
</Modal>

<style>
  .modal-body {
    min-width: 0;
  }

  .modal-help {
    margin: 0 0 16px 0;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .tab-order-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .tab-order-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    transition: opacity 150ms ease;
  }

  .tab-order-item.hidden-tab {
    opacity: 0.5;
  }

  .tab-grip {
    color: var(--text-muted);
    cursor: grab;
  }

  .tab-label {
    flex: 1;
    font-size: 14px;
    color: var(--text-primary);
  }

  .tab-controls {
    display: flex;
    gap: 4px;
  }

  .tab-move-btn {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 14px;
    cursor: pointer;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
  }

  .tab-move-btn:hover:not(:disabled) {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .tab-move-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .modal-actions {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding-top: 24px;
  }

  .modal-actions-right {
    display: flex;
    gap: 12px;
  }
</style>
