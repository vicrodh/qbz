<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { t } from '$lib/i18n';
  import Modal from './Modal.svelte';

  interface ResolvedLink {
    type: 'OpenAlbum' | 'OpenTrack' | 'OpenArtist' | 'OpenPlaylist';
    id: string | number;
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    onResolve: (resolved: ResolvedLink) => void;
  }

  let { isOpen, onClose, onResolve }: Props = $props();

  let url = $state('');
  let error = $state('');
  let resolving = $state(false);
  let inputEl = $state<HTMLInputElement | undefined>(undefined);

  $effect(() => {
    if (isOpen && inputEl) {
      // Focus the input when modal opens
      setTimeout(() => inputEl?.focus(), 100);
    }
    if (!isOpen) {
      // Reset state when modal closes
      url = '';
      error = '';
      resolving = false;
    }
  });

  async function handleSubmit() {
    const trimmed = url.trim();
    if (!trimmed || resolving) return;

    error = '';
    resolving = true;

    try {
      const resolved = await invoke<ResolvedLink>('v2_resolve_qobuz_link', { url: trimmed });
      onResolve(resolved);
      onClose();
    } catch (err) {
      console.error('Link resolve error:', err);
      error = $t('linkResolver.invalidLink');
    } finally {
      resolving = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      handleSubmit();
    }
  }
</script>

<Modal {isOpen} {onClose} title={$t('linkResolver.title')} maxWidth="520px">
  <div class="link-resolver-body">
    <div class="input-row">
      <input
        bind:this={inputEl}
        bind:value={url}
        onkeydown={handleKeydown}
        type="text"
        class="link-input"
        placeholder={$t('linkResolver.placeholder')}
        disabled={resolving}
        spellcheck="false"
        autocomplete="off"
      />
      <button
        class="go-btn"
        onclick={handleSubmit}
        disabled={!url.trim() || resolving}
      >
        {resolving ? $t('linkResolver.resolving') : $t('linkResolver.go')}
      </button>
    </div>
    {#if error}
      <p class="error-text">{error}</p>
    {/if}
  </div>
</Modal>

<style>
  .link-resolver-body {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .input-row {
    display: flex;
    gap: 8px;
  }

  .link-input {
    flex: 1;
    padding: 10px 14px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    color: var(--text-primary);
    font-size: 14px;
    outline: none;
    transition: border-color 150ms ease;
  }

  .link-input:focus {
    border-color: var(--accent-primary);
  }

  .link-input::placeholder {
    color: var(--text-muted);
  }

  .link-input:disabled {
    opacity: 0.6;
  }

  .go-btn {
    padding: 10px 20px;
    background: var(--accent-primary);
    color: #fff;
    border: none;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: opacity 150ms ease;
    white-space: nowrap;
  }

  .go-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .go-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .error-text {
    color: var(--error, #ef4444);
    font-size: 13px;
    margin: 0;
  }
</style>
