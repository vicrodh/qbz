<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import Modal from './Modal.svelte';
  import { GripVertical } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import type { FavoritesPreferences } from '../types';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    onSave: (prefs: FavoritesPreferences) => void;
    initialPreferences: FavoritesPreferences;
  }

  let { isOpen, onClose, onSave, initialPreferences }: Props = $props();

  let tabOrder = $state<string[]>([...initialPreferences.tab_order]);
  let saving = $state(false);

  function getTabLabel(tab: string): string {
    const key = `favorites.tabLabels.${tab}`;
    const translated = $t(key);
    if (translated && !translated.startsWith('favorites.tabLabels.')) return translated;
    const fallback: Record<string, string> = { tracks: 'Tracks', albums: 'Albums', artists: 'Artists', playlists: 'Playlists' };
    return fallback[tab] || tab;
  }

  function syncFromPreferences() {
    tabOrder = [...initialPreferences.tab_order];
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

  async function handleSave() {
    saving = true;
    try {
      const prefs: FavoritesPreferences = {
        custom_icon_path: null,
        custom_icon_preset: null,
        icon_background: null,
        tab_order: tabOrder,
      };

      const saved = await invoke<FavoritesPreferences>('v2_save_favorites_preferences', { prefs });
      onSave(saved);
      onClose();
    } catch (err) {
      console.error('Failed to save favorites preferences:', err);
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

<Modal {isOpen} onClose={handleCancel} title={$t('favorites.settingsModalTitle')} maxWidth="400px">
  {#snippet children()}
  <div class="modal-body">
    <div class="modal-section">
      <h3>{$t('favorites.tabOrderSectionTitle')}</h3>

      <div class="tab-order-list">
        {#each tabOrder as tab, index}
          <div class="tab-order-item">
            <div class="tab-grip">
              <GripVertical size={16} />
            </div>
            <div class="tab-label">{getTabLabel(tab)}</div>
            <div class="tab-controls">
              <button
                class="tab-move-btn"
                onclick={() => moveUp(index)}
                disabled={index === 0}
                title={$t('actions.moveUp')}
              >
                ↑
              </button>
              <button
                class="tab-move-btn"
                onclick={() => moveDown(index)}
                disabled={index === tabOrder.length - 1}
                title={$t('actions.moveDown')}
              >
                ↓
              </button>
            </div>
          </div>
        {/each}
      </div>
    </div>
  </div>
  {/snippet}

  {#snippet footer()}
  <div class="modal-actions">
    <button class="btn btn-secondary" onclick={handleCancel} disabled={saving}>
      {$t('actions.cancel')}
    </button>
    <button class="btn btn-primary" onclick={handleSave} disabled={saving}>
      {saving ? $t('actions.saving') : $t('actions.save')}
    </button>
  </div>
  {/snippet}
</Modal>

<style>
  .modal-body {
    min-width: 0;
  }

  .modal-section h3 {
    font-size: 15px;
    font-weight: 600;
    margin: 0 0 12px 0;
    color: var(--text-primary);
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
    transition: all 150ms ease;
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
    justify-content: flex-end;
    gap: 12px;
    padding-top: 24px;
  }
</style>
