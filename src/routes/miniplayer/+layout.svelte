<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();
</script>

<!-- Minimal layout for MiniPlayer - no sidebar, titlebar, etc. -->
<div class="miniplayer-layout">
  {@render children()}
</div>

<style>
  :global(html, body) {
    margin: 0;
    padding: 0;
    background: #18181b !important;
    overflow: hidden;
  }

  .miniplayer-layout {
    width: 100vw;
    height: 100vh;
    background: #18181b;
    overflow: hidden;
  }
</style>
