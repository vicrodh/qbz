<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { t } from '$lib/i18n';
  import { Link2 } from 'lucide-svelte';
  import Modal from './Modal.svelte';

  interface ResolvedLink {
    type: 'OpenAlbum' | 'OpenTrack' | 'OpenArtist' | 'OpenPlaylist';
    id: string | number;
  }

  type MusicLinkResult =
    | { kind: 'Resolved'; link: ResolvedLink; provider: string | null }
    | { kind: 'PlaylistDetected'; provider: string }
    | { kind: 'NotOnQobuz'; provider: string | null };

  type Platform = 'qobuz' | 'spotify' | 'apple' | 'tidal' | 'deezer' | 'songlink' | null;

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    onResolve: (resolved: ResolvedLink) => void;
    onOpenImporter?: () => void;
  }

  let { isOpen, onClose, onResolve, onOpenImporter }: Props = $props();

  let url = $state('');
  let error = $state('');
  let playlistInfo = $state(false);
  let resolving = $state(false);
  let inputEl = $state<HTMLInputElement | undefined>(undefined);

  const platformConfig: Record<string, { logo: string; alt: string; color: string }> = {
    qobuz: { logo: '/qobuz-logo.svg', alt: 'Qobuz', color: '#0170ef' },
    spotify: { logo: '/spotify-logo.svg', alt: 'Spotify', color: '#1DB954' },
    apple: { logo: '/apple-music-logo.svg', alt: 'Apple Music', color: '#fa233b' },
    tidal: { logo: '/tidal-tidal.svg', alt: 'Tidal', color: '#ffffff' },
    deezer: { logo: '/deezer-logo.svg', alt: 'Deezer', color: '#00c7f2' },
  };

  function detectPlatform(value: string): Platform {
    const lower = value.trim().toLowerCase();
    if (!lower) return null;
    if (lower.includes('qobuz.com/') || lower.startsWith('qobuzapp://')) return 'qobuz';
    if (lower.includes('spotify.com/') || lower.startsWith('spotify:')) return 'spotify';
    if (lower.includes('music.apple.com/')) return 'apple';
    if (lower.includes('tidal.com/')) return 'tidal';
    if (lower.includes('deezer.com/')) return 'deezer';
    if (lower.includes('song.link/') || lower.includes('album.link/') || lower.includes('odesli.co/')) return 'songlink';
    return null;
  }

  let detectedPlatform = $derived(detectPlatform(url));

  $effect(() => {
    if (isOpen && inputEl) {
      setTimeout(() => inputEl?.focus(), 100);
    }
    if (!isOpen) {
      url = '';
      error = '';
      playlistInfo = false;
      resolving = false;
    }
  });

  async function handleSubmit() {
    const trimmed = url.trim();
    if (!trimmed || resolving) return;

    error = '';
    playlistInfo = false;
    resolving = true;

    try {
      const result = await invoke<MusicLinkResult>('v2_resolve_music_link', { url: trimmed });

      switch (result.kind) {
        case 'Resolved':
          onResolve(result.link);
          onClose();
          break;
        case 'PlaylistDetected':
          playlistInfo = true;
          break;
        case 'NotOnQobuz':
          error = $t('linkResolver.notOnQobuz');
          break;
      }
    } catch (err) {
      console.error('Link resolve error:', err);
      error = $t('linkResolver.invalidLink');
    } finally {
      resolving = false;
    }
  }

  function handleOpenImporter() {
    onClose();
    onOpenImporter?.();
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
      {#if detectedPlatform}
        <div class="platform-icon">
          {#if detectedPlatform === 'songlink'}
            <Link2 size={20} color="var(--text-secondary)" />
          {:else if platformConfig[detectedPlatform]}
            <img
              src={platformConfig[detectedPlatform].logo}
              alt={platformConfig[detectedPlatform].alt}
              class="platform-logo"
            />
          {/if}
        </div>
      {/if}
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
    {#if playlistInfo}
      <div class="playlist-banner">
        <p class="playlist-text">{$t('linkResolver.playlistDetected')}</p>
        {#if onOpenImporter}
          <button class="importer-btn" onclick={handleOpenImporter}>
            {$t('linkResolver.openPlaylistImporter')}
          </button>
        {/if}
      </div>
    {/if}
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
    align-items: center;
  }

  .platform-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    flex-shrink: 0;
  }

  .platform-logo {
    width: 24px;
    height: 24px;
    object-fit: contain;
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

  .playlist-banner {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
  }

  .playlist-text {
    color: var(--text-secondary);
    font-size: 13px;
    margin: 0;
  }

  .importer-btn {
    align-self: flex-start;
    padding: 8px 16px;
    background: var(--accent-primary);
    color: #fff;
    border: none;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: opacity 150ms ease;
  }

  .importer-btn:hover {
    opacity: 0.9;
  }

  .error-text {
    color: var(--error, #ef4444);
    font-size: 13px;
    margin: 0;
  }
</style>
