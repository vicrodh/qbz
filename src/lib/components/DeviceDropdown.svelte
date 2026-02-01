<script lang="ts">
  import { ChevronDown, Search } from 'lucide-svelte';
  import {
    openMenu as openGlobalMenu,
    closeMenu as closeGlobalMenu,
    subscribe as subscribeFloatingMenu,
    getActiveMenuId
  } from '$lib/stores/floatingMenuStore';

  // Extended timeout for device dropdown (10 seconds) - devices have long names
  const DEVICE_DROPDOWN_TIMEOUT = 10000;

  interface DeviceOption {
    value: string;        // Display name
    id: string;           // Device ID (for categorization)
    isDefault?: boolean;
    sampleRates?: number[]; // Supported sample rates in Hz (e.g., 44100, 48000, 96000, 192000)
    deviceBus?: string;   // "usb", "pci", "bluetooth" (PipeWire)
    isHardware?: boolean; // Has HARDWARE flag (PipeWire)
  }

  interface DeviceGroup {
    key: string;          // Group identifier
    label: string;        // Display label
    devices: DeviceOption[];
  }

  interface Props {
    value: string;
    devices: DeviceOption[];
    onchange: (value: string) => void;
    wide?: boolean;
    expandLeft?: boolean;
    backend?: 'alsa' | 'pipewire' | 'pulse';
  }

  let { value, devices, onchange, wide = false, expandLeft = false, backend = 'alsa' }: Props = $props();

  let isOpen = $state(false);
  let isHovering = $state(false);
  let dropdownRef = $state<HTMLDivElement | null>(null);
  let menuRef = $state<HTMLDivElement | null>(null);
  let searchInputRef = $state<HTMLInputElement | null>(null);
  let searchQuery = $state('');

  const menuId = `device-dropdown-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`;

  // Show search when many devices
  const showSearch = $derived(devices.length > 5);

  // Group devices by category (different logic for ALSA vs PipeWire)
  const groupedDevices = $derived.by(() => {
    const groups: DeviceGroup[] = [];

    if (backend === 'alsa') {
      // ALSA grouping: Defaults, Bit-perfect, Plugin Hardware, Other
      const defaults: DeviceOption[] = [];
      const bitPerfect: DeviceOption[] = [];
      const pluginHw: DeviceOption[] = [];
      const others: DeviceOption[] = [];

      for (const device of devices) {
        if (device.value === 'System Default') {
          defaults.push(device);
        } else if (device.id === 'default' || device.isDefault) {
          defaults.push(device);
        } else if (
          device.id.startsWith('hw:') ||
          device.id.startsWith('iec958:') ||
          device.id.startsWith('front:CARD=') ||
          device.value.toLowerCase().includes('bit-perfect')
        ) {
          // hw: = direct hardware access, iec958: = S/PDIF digital output
          // front:CARD= = front channel device (bit-perfect for some USB DACs)
          // Also include any device with "bit-perfect" in its display name
          bitPerfect.push(device);
        } else if (device.id.startsWith('plughw:')) {
          pluginHw.push(device);
        } else {
          others.push(device);
        }
      }

      if (defaults.length > 0) groups.push({ key: 'defaults', label: 'Defaults', devices: defaults });
      if (bitPerfect.length > 0) groups.push({ key: 'bitperfect', label: 'Bit-perfect (Hardware / Digital)', devices: bitPerfect });
      if (pluginHw.length > 0) groups.push({ key: 'pluginhw', label: 'Plugin Hardware', devices: pluginHw });
      if (others.length > 0) groups.push({ key: 'others', label: 'Other Outputs', devices: others });
    } else {
      // PipeWire/PulseAudio grouping: USB Audio, HDMI/DisplayPort, Other Hardware, Virtual
      const defaults: DeviceOption[] = [];
      const usbAudio: DeviceOption[] = [];
      const hdmi: DeviceOption[] = [];
      const otherHw: DeviceOption[] = [];
      const virtual: DeviceOption[] = [];

      for (const device of devices) {
        if (device.value === 'System Default') {
          defaults.push(device);
        } else if (device.deviceBus === 'usb' && device.isHardware) {
          usbAudio.push(device);
        } else if (device.isHardware && (device.value.includes('HDMI') || device.value.includes('DisplayPort'))) {
          hdmi.push(device);
        } else if (device.isHardware) {
          otherHw.push(device);
        } else {
          virtual.push(device);
        }
      }

      if (defaults.length > 0) groups.push({ key: 'defaults', label: 'Defaults', devices: defaults });
      if (usbAudio.length > 0) groups.push({ key: 'usb', label: 'USB Audio (Bit-perfect capable)', devices: usbAudio });
      if (hdmi.length > 0) groups.push({ key: 'hdmi', label: 'HDMI / DisplayPort', devices: hdmi });
      if (otherHw.length > 0) groups.push({ key: 'otherhw', label: 'Other Hardware', devices: otherHw });
      if (virtual.length > 0) groups.push({ key: 'virtual', label: 'Virtual Sinks', devices: virtual });
    }

    return groups;
  });

  // Format sample rates for display (e.g., "44.1, 48, 96, 192 kHz")
  function formatSampleRates(rates: number[] | undefined): string | null {
    if (!rates || rates.length === 0) return null;
    const formatted = rates.map(r => {
      const kHz = r / 1000;
      // Show decimal only for 44.1, 88.2, 176.4, 352.8
      return kHz % 1 !== 0 ? kHz.toFixed(1) : kHz.toString();
    });
    return formatted.join(', ') + ' kHz';
  }

  // Filtered groups based on search
  const filteredGroups = $derived.by(() => {
    if (!searchQuery.trim()) return groupedDevices;

    const query = searchQuery.toLowerCase();
    return groupedDevices
      .map(group => ({
        ...group,
        devices: group.devices.filter(d =>
          d.value.toLowerCase().includes(query) ||
          d.id.toLowerCase().includes(query)
        )
      }))
      .filter(group => group.devices.length > 0);
  });

  // Flat list of all filtered devices (for counting)
  const totalFilteredDevices = $derived(
    filteredGroups.reduce((sum, g) => sum + g.devices.length, 0)
  );

  let fixedPosition = $state<{ top: number; left: number; width: number } | null>(null);

  const ITEM_HEIGHT = 36;
  const GROUP_HEADER_HEIGHT = 28;
  const SEARCH_HEIGHT = 48;
  const MENU_PADDING = 8;
  const MAX_VISIBLE_ITEMS = 6;
  const MENU_GAP = 4;

  const expectedMenuHeight = $derived(
    showSearch
      ? SEARCH_HEIGHT + (MAX_VISIBLE_ITEMS * ITEM_HEIGHT) + MENU_PADDING
      : Math.min(devices.length + groupedDevices.length, 10) * ITEM_HEIGHT + MENU_PADDING
  );

  function calculatePosition() {
    if (!dropdownRef) return;

    const triggerRect = dropdownRef.getBoundingClientRect();
    const viewportHeight = window.innerHeight;
    const viewportWidth = window.innerWidth;
    const playerHeight = 104;
    const safeBottom = viewportHeight - playerHeight;
    const menuHeight = menuRef?.offsetHeight || expectedMenuHeight;
    const menuWidth = menuRef?.offsetWidth || (wide ? 320 : 280);

    const spaceBelow = safeBottom - triggerRect.bottom - MENU_GAP;
    const spaceAbove = triggerRect.top - MENU_GAP;

    let top: number;
    if (spaceBelow >= menuHeight) {
      top = triggerRect.bottom + MENU_GAP;
    } else if (spaceAbove >= menuHeight) {
      top = triggerRect.top - menuHeight - MENU_GAP;
    } else {
      top = spaceBelow >= spaceAbove ? triggerRect.bottom + MENU_GAP : triggerRect.top - menuHeight - MENU_GAP;
    }

    let left: number;
    if (expandLeft) {
      left = triggerRect.right - menuWidth;
      if (left < 8) left = 8;
    } else {
      left = triggerRect.left;
      if (left + menuWidth > viewportWidth - 8) {
        left = viewportWidth - menuWidth - 8;
      }
    }

    fixedPosition = { top, left, width: triggerRect.width };
  }

  function handleClickOutside(event: MouseEvent) {
    if (dropdownRef && !dropdownRef.contains(event.target as Node) &&
        menuRef && !menuRef.contains(event.target as Node)) {
      closeDropdown();
    }
  }

  function openDropdown() {
    calculatePosition();
    openGlobalMenu(menuId);
    isOpen = true;
    searchQuery = '';

    requestAnimationFrame(() => {
      if (showSearch) {
        searchInputRef?.focus();
      }
      calculatePosition();
    });
  }

  function closeDropdown() {
    isOpen = false;
    searchQuery = '';
    fixedPosition = null;
    closeGlobalMenu(menuId);
  }

  function handleOptionClick(device: DeviceOption) {
    onchange(device.value);
    closeDropdown();
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      closeDropdown();
    }
  }

  $effect(() => {
    const unsubscribe = subscribeFloatingMenu(() => {
      const activeId = getActiveMenuId();
      if (activeId !== null && activeId !== menuId && isOpen) {
        isOpen = false;
        searchQuery = '';
        fixedPosition = null;
      }
    });
    return unsubscribe;
  });

  $effect(() => {
    if (isOpen) {
      document.addEventListener('mousedown', handleClickOutside);

      const recalc = () => calculatePosition();
      window.addEventListener('scroll', recalc, true);
      window.addEventListener('resize', recalc);

      let idleTimer: ReturnType<typeof setTimeout> | null = null;

      const scheduleIdleClose = () => {
        if (idleTimer) clearTimeout(idleTimer);
        idleTimer = setTimeout(() => {
          if (isOpen && !isHovering) closeDropdown();
        }, DEVICE_DROPDOWN_TIMEOUT);
      };

      if (!isHovering) scheduleIdleClose();

      const onActivity = () => {
        if (!isHovering) scheduleIdleClose();
      };

      window.addEventListener('pointermove', onActivity, true);

      return () => {
        document.removeEventListener('mousedown', handleClickOutside);
        window.removeEventListener('scroll', recalc, true);
        window.removeEventListener('resize', recalc);
        window.removeEventListener('pointermove', onActivity, true);
        if (idleTimer) clearTimeout(idleTimer);
      };
    }
  });

  const menuMaxHeight = $derived(`${expectedMenuHeight}px`);
</script>

<div class="dropdown" class:wide bind:this={dropdownRef}>
  <button class="trigger" onclick={() => isOpen ? closeDropdown() : openDropdown()} title={value}>
    <span class="value-text">{value}</span>
    <ChevronDown size={16} class="chevron" />
  </button>
</div>

{#if isOpen && fixedPosition}
  <div
    class="menu"
    class:searchable={showSearch}
    role="listbox"
    bind:this={menuRef}
    onmouseenter={() => isHovering = true}
    onmouseleave={() => isHovering = false}
    style:top="{fixedPosition.top}px"
    style:left="{fixedPosition.left}px"
    style:min-width="{fixedPosition.width}px"
    style:max-height={menuMaxHeight}
  >
    {#if showSearch}
      <div class="search-container">
        <Search size={14} class="search-icon" />
        <input
          bind:this={searchInputRef}
          type="text"
          class="search-input"
          placeholder="Search devices..."
          bind:value={searchQuery}
          onkeydown={handleKeyDown}
        />
      </div>
    {/if}
    <div
      class="options-container"
      class:with-search={showSearch}
      style:max-height={showSearch ? `${MAX_VISIBLE_ITEMS * ITEM_HEIGHT + 100}px` : undefined}
    >
      {#each filteredGroups as group (group.key)}
        <div class="group">
          <div class="group-header">{group.label}</div>
          {#each group.devices as device (device.value)}
            {@const sampleRatesText = formatSampleRates(device.sampleRates)}
            <button
              class="option"
              class:selected={device.value === value}
              class:has-subtitle={!!sampleRatesText}
              onclick={() => handleOptionClick(device)}
              title={device.id !== device.value ? `${device.value}\n${device.id}` : device.value}
            >
              <span class="current-indicator" class:visible={device.value === value}>▸</span>
              <div class="option-content">
                <span class="option-text">{device.value}</span>
                {#if sampleRatesText}
                  <span class="option-sample-rates">{sampleRatesText}</span>
                {/if}
              </div>
              {#if group.key === 'bitperfect' || group.key === 'usb'}
                <span class="badge bit-perfect">BP</span>
              {/if}
            </button>
          {/each}
        </div>
      {:else}
        <div class="no-results">No devices found</div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .dropdown {
    position: relative;
  }

  .dropdown.wide {
    min-width: 280px;
  }

  .trigger {
    height: 40px;
    width: 280px;
    padding: 0 16px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    border: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    font-size: 14px;
    color: var(--text-primary);
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .dropdown.wide .trigger {
    width: 280px;
  }

  .value-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    text-align: left;
  }

  .trigger:hover {
    background-color: var(--bg-hover);
  }

  .trigger :global(.chevron) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .menu {
    position: fixed;
    width: max-content;
    min-width: 280px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 10000;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .search-container {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
    flex-shrink: 0;
  }

  .search-container :global(.search-icon) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 13px;
    padding: 0;
    min-width: 0;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .options-container {
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-width: thin;
    scrollbar-color: var(--text-muted) transparent;
    flex: 1;
    min-height: 0;
  }

  .options-container::-webkit-scrollbar {
    width: 6px;
  }

  .options-container::-webkit-scrollbar-track {
    background: transparent;
  }

  .options-container::-webkit-scrollbar-thumb {
    background: var(--text-muted);
    border-radius: 9999px;
  }

  .group {
    margin-bottom: 4px;
  }

  .group:last-child {
    margin-bottom: 0;
  }

  .group-header {
    padding: 8px 12px 4px;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    pointer-events: none;
    user-select: none;
  }

  .option {
    width: 100%;
    min-height: 36px;
    padding: 6px 12px;
    padding-left: 8px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 4px;
    text-align: left;
    font-size: 12px;
    color: var(--text-secondary);
    background: none;
    border: none;
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
    flex-shrink: 0;
  }

  .current-indicator {
    width: 12px;
    font-size: 10px;
    color: var(--accent-color, #3b82f6);
    opacity: 0;
    flex-shrink: 0;
  }

  .current-indicator.visible {
    opacity: 1;
  }

  .option.has-subtitle {
    min-height: 44px;
    padding: 4px 12px;
  }

  .option-content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .option-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .option-sample-rates {
    font-size: 10px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .option:hover {
    background-color: var(--bg-hover);
    color: var(--text-primary);
  }

  .option.selected {
    background-color: rgba(66, 133, 244, 0.15);
    color: var(--text-primary);
  }

  .badge {
    padding: 2px 5px;
    border-radius: 3px;
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    flex-shrink: 0;
  }

  .badge.bit-perfect {
    background: rgba(59, 130, 246, 0.2);
    color: #3b82f6;
    border: 1px solid rgba(59, 130, 246, 0.3);
  }

  .no-results {
    padding: 12px 16px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
