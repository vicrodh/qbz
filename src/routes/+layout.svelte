<script lang="ts">
  import type { Snippet } from 'svelte';
  import { initI18n } from '$lib/i18n';
  import { isLoading } from 'svelte-i18n';

  // Props
  let { children }: { children: Snippet } = $props();

  // Initialize i18n
  initI18n();

  // Wait for translations to load
  let ready = $derived(!$isLoading);
</script>

{#if ready}
  {@render children()}
{:else}
  <div class="loading">
    <div class="spinner"></div>
  </div>
{/if}

<style>
  :global(*) {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
  }

  :global(html, body) {
    height: 100%;
    width: 100%;
    overflow: hidden;
  }

  :global(body) {
    font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
    font-size: 14px;
    line-height: 1.5;
    color: var(--text-primary);
    background-color: var(--bg-primary);
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
  }

  /* CSS Variables - Dark Theme (Default) */
  :global(:root) {
    /* Backgrounds */
    --bg-primary: #0f0f0f;
    --bg-secondary: #1a1a1a;
    --bg-tertiary: #2a2a2a;
    --bg-hover: #1f1f1f;

    /* Text */
    --text-primary: #ffffff;
    --text-secondary: #cccccc;
    --text-muted: #888888;
    --text-disabled: #555555;

    /* Accent */
    --accent-primary: #4285F4;
    --accent-hover: #5a9bf4;
    --accent-active: #3275e4;

    /* Borders */
    --border-subtle: #2a2a2a;
    --border-strong: #3a3a3a;

    /* Shadows */
    --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.3);
    --shadow-md: 0 4px 8px rgba(0, 0, 0, 0.3);
    --shadow-lg: 0 8px 24px rgba(0, 0, 0, 0.4);

    /* Spacing */
    --spacing-xs: 4px;
    --spacing-sm: 8px;
    --spacing-md: 12px;
    --spacing-lg: 16px;
    --spacing-xl: 24px;
    --spacing-2xl: 32px;
    --spacing-3xl: 48px;

    /* Layout */
    --sidebar-width: 240px;
    --now-playing-height: 80px;

    /* Border Radius */
    --radius-sm: 4px;
    --radius-md: 8px;
    --radius-lg: 12px;
    --radius-xl: 16px;
    --radius-full: 9999px;

    /* Transitions */
    --transition-fast: 150ms ease-out;
    --transition-normal: 200ms ease-out;
    --transition-slow: 300ms ease-out;
  }

  /* Light Theme */
  :global([data-theme="light"]) {
    --bg-primary: #ffffff;
    --bg-secondary: #f5f5f5;
    --bg-tertiary: #e8e8e8;
    --bg-hover: #f0f0f0;
    --text-primary: #0f0f0f;
    --text-secondary: #444444;
    --text-muted: #666666;
    --text-disabled: #999999;
    --border-subtle: #e0e0e0;
    --border-strong: #cccccc;
  }

  /* OLED Black Theme */
  :global([data-theme="oled"]) {
    --bg-primary: #000000;
    --bg-secondary: #0a0a0a;
    --bg-tertiary: #1a1a1a;
    --bg-hover: #111111;
  }

  /* Warm Theme */
  :global([data-theme="warm"]) {
    --bg-primary: #1a1814;
    --bg-secondary: #242018;
    --bg-tertiary: #2e2820;
    --bg-hover: #282218;
    --accent-primary: #d4a574;
    --accent-hover: #e0b888;
    --accent-active: #c49460;
  }

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

  /* Scrollbar styling */
  :global(::-webkit-scrollbar) {
    width: 8px;
    height: 8px;
  }

  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }

  :global(::-webkit-scrollbar-thumb) {
    background: var(--bg-tertiary);
    border-radius: var(--radius-full);
  }

  :global(::-webkit-scrollbar-thumb:hover) {
    background: var(--text-muted);
  }

  /* Selection */
  :global(::selection) {
    background-color: var(--accent-primary);
    color: var(--text-primary);
  }

  /* Focus visible */
  :global(:focus-visible) {
    outline: 2px solid var(--accent-primary);
    outline-offset: 2px;
  }

  /* Button reset */
  :global(button) {
    background: none;
    border: none;
    cursor: pointer;
    font: inherit;
    color: inherit;
  }

  /* Link reset */
  :global(a) {
    color: inherit;
    text-decoration: none;
  }

  /* Input reset */
  :global(input, textarea, select) {
    font: inherit;
    color: inherit;
  }
</style>
