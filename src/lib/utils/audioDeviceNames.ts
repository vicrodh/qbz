/**
 * Helper to convert ALSA device names to more user-friendly display names.
 */

// Known card names and their friendly versions
const CARD_NAMES: Record<string, string> = {
  'sofhdadsp': 'Intel HD Audio',
  'NVidia': 'NVIDIA GPU',
  'Generic': 'Generic Audio',
  'PCH': 'Onboard Audio',
  'HDMI': 'HDMI Audio',
};

/**
 * Get a user-friendly name for an ALSA device name.
 */
export function getDevicePrettyName(alsaName: string): string {
  if (!alsaName) return 'Unknown Device';

  // Exact matches first
  switch (alsaName) {
    case 'default':
      return 'System Default';
    case 'sysdefault':
      return 'System Default (Alt)';
    case 'pipewire':
      return 'PipeWire';
    case 'pulse':
      return 'PulseAudio';
    case 'jack':
      return 'JACK Audio';
  }

  // Pattern: front:CARD=XXX,DEV=N
  const frontMatch = alsaName.match(/^front:CARD=([^,]+),DEV=(\d+)$/);
  if (frontMatch) {
    const cardName = CARD_NAMES[frontMatch[1]] || frontMatch[1];
    return `${cardName} Front Output`;
  }

  // Pattern: surround40, surround41, surround50, surround51, surround71:CARD=XXX,DEV=N
  const surroundMatch = alsaName.match(/^surround(\d{2}):CARD=([^,]+),DEV=(\d+)$/);
  if (surroundMatch) {
    const channels = surroundMatch[1];
    const cardName = CARD_NAMES[surroundMatch[2]] || surroundMatch[2];
    const channelName = channels === '40' ? '4.0' : channels === '41' ? '4.1' : channels === '50' ? '5.0' : channels === '51' ? '5.1' : channels === '71' ? '7.1' : channels;
    return `${cardName} Surround ${channelName}`;
  }

  // Pattern: iec958:CARD=XXX,DEV=N (S/PDIF)
  const iec958Match = alsaName.match(/^iec958:CARD=([^,]+),DEV=(\d+)$/);
  if (iec958Match) {
    const cardName = CARD_NAMES[iec958Match[1]] || iec958Match[1];
    return `${cardName} S/PDIF`;
  }

  // Pattern: hdmi:CARD=XXX,DEV=N
  const hdmiMatch = alsaName.match(/^hdmi:CARD=([^,]+),DEV=(\d+)$/);
  if (hdmiMatch) {
    const cardName = CARD_NAMES[hdmiMatch[1]] || hdmiMatch[1];
    const devNum = Number.parseInt(hdmiMatch[2]) + 1;
    return `HDMI ${devNum} (${cardName})`;
  }

  // Pattern: default:CARD=XXX or sysdefault:CARD=XXX
  const cardMatch = alsaName.match(/^(default|sysdefault):CARD=(.+)$/);
  if (cardMatch) {
    const prefix = cardMatch[1] === 'sysdefault' ? ' (Alt)' : '';
    const cardName = CARD_NAMES[cardMatch[2]] || cardMatch[2];
    return `${cardName}${prefix}`;
  }

  // Pattern: hw:X,Y or plughw:X,Y
  const hwMatch = alsaName.match(/^(plug)?hw:(\d+),(\d+)$/);
  if (hwMatch) {
    const prefix = hwMatch[1] ? 'Plugin ' : '';
    return `${prefix}Hardware ${hwMatch[2]}:${hwMatch[3]}`;
  }

  // Pattern: alsa_output.XXX (PipeWire style)
  if (alsaName.startsWith('alsa_output.')) {
    const rest = alsaName.slice('alsa_output.'.length);

    // USB device
    if (rest.includes('usb-')) {
      const usbMatch = rest.match(/usb-([^.]+)/);
      if (usbMatch) {
        return `USB: ${usbMatch[1].replaceAll('_', ' ')}`;
      }
    }

    // PCI HDMI
    if (rest.includes('hdmi')) {
      return 'HDMI Output';
    }

    // PCI Analog
    if (rest.includes('analog')) {
      return 'Analog Output';
    }

    // S/PDIF
    if (rest.includes('iec958')) {
      return 'S/PDIF Output';
    }
  }

  // Fallback: clean up the name a bit
  return alsaName
    .replace(/^(default|sysdefault):/, '')
    .replaceAll('CARD=', '')
    .replace(/,DEV=(\d+)/, ' Output $1')
    .replaceAll('_', ' ');
}

/**
 * Check if a device name appears to be a DAC or external audio interface
 */
export function isExternalDevice(deviceName: string): boolean {
  if (!deviceName) return false;
  const lower = deviceName.toLowerCase();
  return (
    lower.includes('usb') ||
    lower.includes('cs201') ||  // Common USB audio chip
    lower.includes('dac') ||
    lower.includes('interface') ||
    lower.includes('focusrite') ||
    lower.includes('steinberg') ||
    lower.includes('motu') ||
    lower.includes('presonus') ||
    /topping|smsl|fiio|ifi|schiit|dragonfly/i.test(lower)
  );
}
