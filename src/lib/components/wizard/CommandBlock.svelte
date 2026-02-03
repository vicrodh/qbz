<script lang="ts">
  import { Copy, Check } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Props {
    label?: string;
    command: string | string[];
  }

  let { label, command }: Props = $props();

  let copied = $state(false);

  const commandText = $derived(Array.isArray(command) ? command.join('\n') : command);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(commandText);
      copied = true;
      setTimeout(() => copied = false, 2000);
    } catch (err) {
      // Fallback for older browsers
      const textArea = document.createElement('textarea');
      textArea.value = commandText;
      textArea.style.position = 'fixed';
      textArea.style.left = '-999999px';
      document.body.appendChild(textArea);
      textArea.select();
      try {
        document.execCommand('copy');
        copied = true;
        setTimeout(() => copied = false, 2000);
      } catch (e) {
        console.error('Failed to copy:', e);
      }
      document.body.removeChild(textArea);
    }
  }
</script>

<div class="command-block-wrapper">
  {#if label}
    <div class="command-label">{label}</div>
  {/if}

  <div class="command-block">
    <pre class="command-text">{commandText}</pre>
    <button
      class="copy-btn"
      class:copied
      onclick={handleCopy}
      title={copied ? $t('dacWizard.buttons.copied') : $t('dacWizard.buttons.copy')}
    >
      {#if copied}
        <Check size={16} />
      {:else}
        <Copy size={16} />
      {/if}
    </button>
  </div>
</div>

<style>
  .command-block-wrapper {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .command-label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .command-block {
    position: relative;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 12px;
    padding-right: 48px;
  }

  .command-text {
    font-family: var(--font-mono, 'JetBrains Mono', monospace);
    font-size: 13px;
    color: var(--text-primary);
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
    line-height: 1.5;
  }

  .copy-btn {
    position: absolute;
    top: 8px;
    right: 8px;
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    cursor: pointer;
    color: var(--text-muted);
    transition: all 150ms ease;
  }

  .copy-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .copy-btn.copied {
    color: var(--accent-primary);
    border-color: var(--accent-primary);
  }
</style>
