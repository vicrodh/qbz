<script lang="ts">
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import {
    myQbzNavStore,
    setMyQbzLabel,
    setMyQbzIconPath,
    DEFAULT_ICON,
  } from '$lib/stores/myQbzNavStore';

  interface Props {
    open: boolean;
    onClose: () => void;
  }
  let { open, onClose }: Props = $props();

  let draftLabel = $state('');
  let draftIconPath = $state<string>('');

  // Initialize drafts when opened
  $effect(() => {
    if (open) {
      const state = $myQbzNavStore;
      draftLabel = state.label;
      draftIconPath = state.iconPath;
    }
  });

  const iconSrc = $derived(
    draftIconPath && draftIconPath !== DEFAULT_ICON
      ? convertFileSrc(draftIconPath)
      : DEFAULT_ICON
  );

  async function pickIcon() {
    try {
      const selected = await openDialog({
        filters: [{ name: 'Image', extensions: ['svg', 'png', 'jpg', 'jpeg', 'webp'] }],
        multiple: false,
        directory: false,
      });
      if (typeof selected === 'string') {
        draftIconPath = selected;
      }
    } catch (err) {
      console.error('[MyQbzNavEditModal] icon picker failed:', err);
    }
  }

  function resetIcon() {
    draftIconPath = DEFAULT_ICON;
  }

  function save() {
    setMyQbzLabel(draftLabel);
    setMyQbzIconPath(draftIconPath === DEFAULT_ICON ? null : draftIconPath);
    onClose();
  }

  function cancel() {
    onClose();
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="modal-backdrop" onclick={cancel} role="presentation">
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div
      class="modal"
      role="dialog"
      aria-label="Customize My QBZ nav"
      onclick={(e) => e.stopPropagation()}
    >
      <h2>Customize</h2>

      <label class="field">
        <span class="label-text">Name</span>
        <input type="text" bind:value={draftLabel} maxlength="40" />
      </label>

      <div class="field">
        <span class="label-text">Icon</span>
        <div class="icon-row">
          <img class="icon-preview" src={iconSrc} alt="" />
          <button class="btn-secondary" onclick={pickIcon}>Choose file…</button>
          <button class="btn-secondary" onclick={resetIcon}>Reset to default</button>
        </div>
      </div>

      <div class="footer">
        <button class="btn-secondary" onclick={cancel}>Cancel</button>
        <button class="btn-primary" onclick={save}>Save</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: grid;
    place-items: center;
    z-index: 9999;
  }
  .modal {
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 12px;
    padding: 24px;
    width: 420px;
    max-width: 90vw;
    color: var(--text-primary);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }
  h2 { margin: 0 0 16px; font-size: 18px; font-weight: 700; }
  .field { display: block; margin-bottom: 16px; }
  .label-text {
    display: block;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 1.5px;
    text-transform: uppercase;
    color: var(--text-muted);
    margin-bottom: 8px;
  }
  input[type="text"] {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 14px;
    font-family: inherit;
    box-sizing: border-box;
  }
  .icon-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .icon-preview {
    width: 40px;
    height: 40px;
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    background: var(--bg-secondary);
    padding: 6px;
    object-fit: contain;
    flex-shrink: 0;
  }
  .btn-secondary, .btn-primary {
    padding: 8px 16px;
    border-radius: 8px;
    border: 1px solid var(--bg-tertiary);
    font-size: 13px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
  }
  .btn-secondary {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }
  .btn-secondary:hover {
    background: var(--bg-hover);
  }
  .btn-primary {
    background: var(--accent-primary);
    color: #ffffff;
    border-color: var(--accent-primary);
    margin-left: 8px;
  }
  .footer {
    display: flex;
    justify-content: flex-end;
    margin-top: 24px;
  }
</style>
