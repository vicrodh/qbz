<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();
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
