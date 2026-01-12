<script lang="ts">
  import { onMount } from 'svelte';
  import { CheckCircle, AlertCircle, Info, Loader2, X } from 'lucide-svelte';

  interface Props {
    message: string;
    type?: 'success' | 'error' | 'info' | 'buffering';
    persistent?: boolean;
    onClose: () => void;
  }

  let { message, type = 'success', persistent = false, onClose }: Props = $props();

  onMount(() => {
    // Don't auto-close persistent toasts (buffering)
    if (persistent) return;

    const timer = setTimeout(onClose, 4000);
    return () => clearTimeout(timer);
  });
</script>

<div class="toast" class:success={type === 'success'} class:error={type === 'error'} class:info={type === 'info'} class:buffering={type === 'buffering'}>
  <div class="icon">
    {#if type === 'success'}
      <CheckCircle size={20} />
    {:else if type === 'error'}
      <AlertCircle size={20} />
    {:else if type === 'buffering'}
      <Loader2 size={20} class="spinning" />
    {:else}
      <Info size={20} />
    {/if}
  </div>
  {#if type === 'buffering'}
    <span class="message">
      <span class="shimmer-text">Buffering</span>
      <span class="dots">...</span>
      <span class="track-name">{message}</span>
    </span>
  {:else}
    <span class="message">{message}</span>
  {/if}
  <button class="close-btn" onclick={onClose}>
    <X size={16} />
  </button>
</div>

<style>
  .toast {
    position: fixed;
    bottom: 100px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    z-index: 100;
    animation: slideUp 200ms ease-out;
  }

  @keyframes slideUp {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(20px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  .icon {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .toast.success .icon {
    color: #4CAF50;
  }

  .toast.error .icon {
    color: #ff6b6b;
  }

  .toast.info .icon {
    color: var(--accent-primary);
  }

  .toast.buffering .icon {
    color: var(--accent-primary);
  }

  .toast.buffering .icon :global(.spinning) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .message {
    font-size: 14px;
    color: var(--text-primary);
  }

  /* Shimmer effect for buffering text */
  .shimmer-text {
    background: linear-gradient(
      90deg,
      var(--text-primary) 0%,
      var(--accent-primary) 25%,
      var(--text-primary) 50%,
      var(--accent-primary) 75%,
      var(--text-primary) 100%
    );
    background-size: 200% 100%;
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    animation: shimmer 2s ease-in-out infinite;
    font-weight: 500;
  }

  @keyframes shimmer {
    0% { background-position: 200% 0; }
    100% { background-position: -200% 0; }
  }

  .dots {
    color: var(--text-muted);
    margin-right: 8px;
  }

  .track-name {
    color: var(--text-secondary);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 150ms ease;
  }

  .close-btn:hover {
    color: var(--text-primary);
  }
</style>
