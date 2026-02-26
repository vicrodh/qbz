<script lang="ts">
  import { onMount } from 'svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();

  const THEME_STORAGE_KEY = 'qbz-theme';
  const AUTO_THEME_VARS_STORAGE_KEY = 'qbz-auto-theme-vars';
  const FONT_STORAGE_KEY = 'qbz-font-family';
  let appliedAutoThemeVarNames: string[] = [];

  function clearAutoThemeVars(): void {
    const root = document.documentElement;
    for (const varName of appliedAutoThemeVarNames) {
      root.style.removeProperty(varName);
    }
    appliedAutoThemeVarNames = [];
  }

  function applyThemeFromStorage(): void {
    const root = document.documentElement;
    const savedTheme = localStorage.getItem(THEME_STORAGE_KEY) ?? '';
    const savedFont = localStorage.getItem(FONT_STORAGE_KEY) ?? '';

    if (savedTheme) {
      root.setAttribute('data-theme', savedTheme);
    } else {
      root.removeAttribute('data-theme');
    }

    if (savedFont) {
      root.setAttribute('data-font', savedFont);
    } else {
      root.removeAttribute('data-font');
    }

    clearAutoThemeVars();
    if (savedTheme !== 'auto') return;

    const rawAutoVars = localStorage.getItem(AUTO_THEME_VARS_STORAGE_KEY);
    if (!rawAutoVars) return;

    try {
      const parsed = JSON.parse(rawAutoVars) as Record<string, string>;
      for (const [varName, value] of Object.entries(parsed)) {
        root.style.setProperty(varName, value);
      }
      appliedAutoThemeVarNames = Object.keys(parsed);
    } catch {
      // Ignore invalid storage payloads.
    }
  }

  onMount(() => {
    applyThemeFromStorage();

    const handleStorage = (event: StorageEvent): void => {
      if (!event.key) {
        applyThemeFromStorage();
        return;
      }

      if (
        event.key === THEME_STORAGE_KEY ||
        event.key === AUTO_THEME_VARS_STORAGE_KEY ||
        event.key === FONT_STORAGE_KEY
      ) {
        applyThemeFromStorage();
      }
    };

    window.addEventListener('storage', handleStorage);
    return () => {
      window.removeEventListener('storage', handleStorage);
      clearAutoThemeVars();
    };
  });
</script>

<!-- Minimal layout for MiniPlayer - no sidebar, titlebar, etc. -->
<div class="miniplayer-layout">
  <div class="miniplayer-frame">
    {@render children()}
  </div>
</div>

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    background: transparent !important;
    overflow: hidden;
  }

  .miniplayer-layout {
    width: 100vw;
    height: 100vh;
    background: transparent;
    overflow: hidden;
  }

.miniplayer-frame {
    width: 100%;
    height: 100%;
    background: transparent;
    overflow: hidden;
  }
</style>
