<script lang="ts">
  import { AlertTriangle, Info, ExternalLink } from 'lucide-svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    title?: string;
    body?: string;
    variant?: 'warning' | 'info' | 'error';
    links?: Array<{ label: string; url: string }>;
    children?: Snippet;
  }

  let { title, body, variant = 'warning', links, children }: Props = $props();
</script>

<div class="banner" class:warning={variant === 'warning'} class:info={variant === 'info'} class:error={variant === 'error'}>
  <div class="banner-icon">
    {#if variant === 'info'}
      <Info size={18} />
    {:else}
      <AlertTriangle size={18} />
    {/if}
  </div>
  <div class="banner-content">
    {#if title}
      <div class="banner-title">{title}</div>
    {/if}
    {#if body}
      <div class="banner-body">{body}</div>
    {/if}
    {#if links && links.length > 0}
      <div class="banner-links">
        {#each links as link}
          <a href={link.url} target="_blank" rel="noopener noreferrer" class="banner-link">
            {link.label}
            <ExternalLink size={12} />
          </a>
        {/each}
      </div>
    {/if}
    {#if children}
      <div class="banner-slot">
        {@render children()}
      </div>
    {/if}
  </div>
</div>

<style>
  .banner {
    display: flex;
    gap: 12px;
    padding: 12px 16px;
    border-radius: 8px;
    border: 1px solid;
  }

  .banner.warning {
    background: rgba(251, 191, 36, 0.1);
    border-color: rgba(251, 191, 36, 0.3);
  }

  .banner.warning .banner-icon {
    color: var(--warning, #fbbf24);
  }

  .banner.info {
    background: rgba(66, 133, 244, 0.1);
    border-color: rgba(66, 133, 244, 0.3);
  }

  .banner.info .banner-icon {
    color: var(--accent-primary);
  }

  .banner.error {
    background: rgba(239, 68, 68, 0.1);
    border-color: rgba(239, 68, 68, 0.3);
  }

  .banner.error .banner-icon {
    color: var(--color-error, #ef4444);
  }

  .banner-icon {
    flex-shrink: 0;
    margin-top: 2px;
  }

  .banner-content {
    flex: 1;
    min-width: 0;
  }

  .banner-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 4px;
  }

  .banner-body {
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.5;
  }

  .banner-links {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    margin-top: 10px;
  }

  .banner-link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 13px;
    color: var(--accent-primary);
    text-decoration: none;
    transition: opacity 150ms ease;
  }

  .banner-link:hover {
    opacity: 0.8;
    text-decoration: underline;
  }

  .banner-slot {
    margin-top: 12px;
  }
</style>
