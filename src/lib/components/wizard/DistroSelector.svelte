<script lang="ts">
  import { t } from '$lib/i18n';
  import CommandBlock from './CommandBlock.svelte';

  interface Props {
    selected?: string;
    onchange?: (distro: string) => void;
  }

  let { selected = $bindable('debian'), onchange }: Props = $props();

  const distros = [
    { id: 'debian', labelKey: 'dacWizard.precheck.distros.debian' },
    { id: 'fedora', labelKey: 'dacWizard.precheck.distros.fedora' },
    { id: 'arch', labelKey: 'dacWizard.precheck.distros.arch' },
    { id: 'opensuse', labelKey: 'dacWizard.precheck.distros.opensuse' },
    { id: 'other', labelKey: 'dacWizard.precheck.distros.other' },
  ];

  const commands: Record<string, string[]> = {
    debian: [
      'sudo apt update',
      'sudo apt install pipewire pipewire-pulse wireplumber alsa-utils'
    ],
    fedora: [
      'sudo dnf install pipewire pipewire-pulseaudio wireplumber alsa-utils'
    ],
    arch: [
      'sudo pacman -S pipewire pipewire-pulse wireplumber alsa-utils'
    ],
    opensuse: [
      'sudo zypper install pipewire pipewire-pulseaudio wireplumber alsa-utils'
    ],
    other: []
  };

  function handleChange(distroId: string) {
    selected = distroId;
    onchange?.(distroId);
  }
</script>

<div class="distro-selector">
  <div class="distro-options">
    {#each distros as distro}
      <label class="distro-option">
        <input
          type="radio"
          name="distro"
          value={distro.id}
          checked={selected === distro.id}
          onchange={() => handleChange(distro.id)}
        />
        <span class="distro-label">{$t(distro.labelKey)}</span>
      </label>
    {/each}
  </div>

  {#if selected && selected !== 'other' && commands[selected]}
    <CommandBlock command={commands[selected]} />
  {:else if selected === 'other'}
    <div class="other-hint">
      {$t('dacWizard.precheck.otherHint')}
    </div>
  {/if}
</div>

<style>
  .distro-selector {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .distro-options {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .distro-option {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .distro-option:hover {
    background: var(--bg-hover);
  }

  .distro-option:has(input:checked) {
    border-color: var(--accent-primary);
    background: rgba(66, 133, 244, 0.1);
  }

  .distro-option input {
    accent-color: var(--accent-primary);
  }

  .distro-label {
    font-size: 14px;
    color: var(--text-primary);
  }

  .other-hint {
    padding: 12px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    font-size: 13px;
    color: var(--text-secondary);
    font-family: var(--font-mono, monospace);
  }
</style>
