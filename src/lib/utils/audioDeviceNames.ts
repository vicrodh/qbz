/**
 * Helper to convert ALSA device names to more user-friendly display names.
 * This is a best-effort mapping - some names may not be recognized.
 */

// Common device name patterns and their friendly names
const DEVICE_PATTERNS: [RegExp, string | ((match: RegExpMatchArray) => string)][] = [
  // PipeWire virtual devices
  [/^default$/, 'System Default'],
  [/^pipewire$/i, 'PipeWire'],

  // USB Audio devices - extract product name
  [/alsa_output\.usb-([^.]+)\.([^.]+)/i, (m) => cleanProductName(m[1], m[2])],
  [/usb-([^-]+)-([^-]+)/i, (m) => cleanProductName(m[1], m[2])],

  // PCI Audio (Intel HD Audio, etc)
  [/alsa_output\.pci-.*\.hdmi-stereo/i, 'HDMI Audio Output'],
  [/alsa_output\.pci-.*\.analog-stereo/i, 'Analog Audio Output'],
  [/alsa_output\.pci-.*\.iec958-stereo/i, 'S/PDIF Digital Output'],

  // HDMI patterns
  [/hdmi[:\-_]?(\d+)/i, (m) => `HDMI Output ${parseInt(m[1]) + 1}`],
  [/DisplayPort/i, 'DisplayPort Audio'],

  // Common audio interfaces
  [/Focusrite/i, (m) => 'Focusrite Scarlett'],
  [/Steinberg/i, 'Steinberg Interface'],
  [/MOTU/i, 'MOTU Interface'],
  [/PreSonus/i, 'PreSonus Interface'],

  // DACs
  [/DAC|Topping|SMSL|FiiO|iFi|Schiit|Dragonfly|Modi|Magni/i, (m) => `USB DAC (${m[0]})`],

  // Hardware device patterns (hw:X,Y)
  [/^hw:(\d+),(\d+)$/, (m) => `Hardware Device ${m[1]}:${m[2]}`],
  [/^plughw:(\d+),(\d+)$/, (m) => `Hardware Device ${m[1]}:${m[2]} (Plugin)`],

  // Intel/AMD audio
  [/Intel.*HDA|HDA Intel/i, 'Intel HD Audio'],
  [/AMD.*Audio|Audio.*AMD/i, 'AMD Audio'],
  [/Realtek/i, 'Realtek Audio'],

  // Headphones/Headsets
  [/headphone|headset/i, 'Headphones'],
  [/speaker/i, 'Speakers'],

  // Bluetooth (if visible through ALSA)
  [/bluez|bluetooth/i, 'Bluetooth Audio'],
];

/**
 * Clean up USB product names by removing underscores and fixing case
 */
function cleanProductName(vendor: string, product: string): string {
  const clean = (s: string) => s
    .replace(/_/g, ' ')
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .trim();

  const v = clean(vendor);
  const p = clean(product);

  // If product contains vendor name, just use product
  if (p.toLowerCase().includes(v.toLowerCase())) {
    return p;
  }

  return `${v} ${p}`.trim();
}

/**
 * Get a user-friendly name for an ALSA device name.
 * Falls back to the original name if no pattern matches.
 */
export function getDevicePrettyName(alsaName: string): string {
  if (!alsaName) return 'Unknown Device';

  for (const [pattern, replacement] of DEVICE_PATTERNS) {
    const match = alsaName.match(pattern);
    if (match) {
      if (typeof replacement === 'function') {
        return replacement(match);
      }
      return replacement;
    }
  }

  // Fallback: clean up the raw name a bit
  return alsaName
    .replace(/alsa_output\./g, '')
    .replace(/\./g, ' ')
    .replace(/_/g, ' ')
    .replace(/-/g, ' ')
    .replace(/\s+/g, ' ')
    .trim() || alsaName;
}

/**
 * Check if a device name appears to be a DAC or external audio interface
 */
export function isExternalDevice(deviceName: string): boolean {
  if (!deviceName) return false;
  const lower = deviceName.toLowerCase();
  return (
    lower.includes('usb') ||
    lower.includes('dac') ||
    lower.includes('interface') ||
    lower.includes('focusrite') ||
    lower.includes('steinberg') ||
    lower.includes('motu') ||
    lower.includes('presonus') ||
    /topping|smsl|fiio|ifi|schiit|dragonfly/i.test(lower)
  );
}
