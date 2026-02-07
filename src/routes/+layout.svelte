<script lang="ts">
  import '../app.css';
  import type { Snippet } from 'svelte';
  import { initI18n } from '$lib/i18n';
  import { isLoading } from 'svelte-i18n';

  // Props
  let { children }: { children: Snippet } = $props();

  // Initialize i18n
  initI18n();

  // Wait for translations to load — avoid store access inside $derived()
  function isReady(): boolean {
    return !$isLoading;
  }
</script>

{#if isReady()}
  {@render children()}
{:else}
  <div class="loading">
    <div class="spinner"></div>
  </div>
{/if}

<style>
  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background-color: var(--bg-primary);
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--bg-tertiary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
