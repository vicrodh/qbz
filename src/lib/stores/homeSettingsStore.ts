/**
 * Home Settings Store
 *
 * Manages Home view personalization settings with localStorage persistence.
 * Handles section ordering, visibility, and greeting customization.
 */

// Section identifiers
export type HomeSectionId =
  | 'newReleases'
  | 'pressAwards'
  | 'recentAlbums'
  | 'continueTracks'
  | 'topArtists'
  | 'favoriteAlbums';

export interface HomeSection {
  id: HomeSectionId;
  label: string;
  visible: boolean;
  source?: 'qobuz' | 'user'; // qobuz = editorial, user = personal data
}

export interface HomeSettings {
  sections: HomeSection[];
  greeting: {
    enabled: boolean;
    customText: string | null; // null = use default time-based greeting
  };
}

const STORAGE_KEY = 'qbz-home-settings';

// Default section order and visibility
const DEFAULT_SECTIONS: HomeSection[] = [
  { id: 'newReleases', label: 'New Releases', visible: true, source: 'qobuz' },
  { id: 'pressAwards', label: 'Press Awards', visible: true, source: 'qobuz' },
  { id: 'recentAlbums', label: 'Recently Played', visible: true, source: 'user' },
  { id: 'continueTracks', label: 'Continue Listening', visible: true, source: 'user' },
  { id: 'topArtists', label: 'Your Top Artists', visible: true, source: 'user' },
  { id: 'favoriteAlbums', label: 'More From Favorites', visible: true, source: 'user' },
];

const DEFAULT_SETTINGS: HomeSettings = {
  sections: DEFAULT_SECTIONS,
  greeting: {
    enabled: true,
    customText: null
  }
};

// State
let settings: HomeSettings = loadSettings();
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Load settings from localStorage
 */
function loadSettings(): HomeSettings {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) {
      const parsed = JSON.parse(saved) as Partial<HomeSettings>;
      // Merge with defaults to handle new sections added later
      return mergeWithDefaults(parsed);
    }
  } catch (err) {
    console.error('Failed to load home settings:', err);
  }
  return { ...DEFAULT_SETTINGS, sections: [...DEFAULT_SECTIONS] };
}

/**
 * Merge saved settings with defaults (handles new sections)
 */
function mergeWithDefaults(saved: Partial<HomeSettings>): HomeSettings {
  const result: HomeSettings = {
    sections: [],
    greeting: saved.greeting ?? DEFAULT_SETTINGS.greeting
  };

  // Build section map from saved
  const savedSectionMap = new Map<HomeSectionId, HomeSection>();
  if (saved.sections) {
    for (const section of saved.sections) {
      savedSectionMap.set(section.id, section);
    }
  }

  // Use saved order, adding any missing sections at the end
  const usedIds = new Set<HomeSectionId>();

  if (saved.sections) {
    for (const section of saved.sections) {
      const defaultSection = DEFAULT_SECTIONS.find(s => s.id === section.id);
      if (defaultSection) {
        result.sections.push({
          id: section.id,
          label: defaultSection.label, // Always use current label
          visible: section.visible,
          source: defaultSection.source // Always use default source
        });
        usedIds.add(section.id);
      }
    }
  }

  // Add any new sections that weren't in saved settings
  for (const defaultSection of DEFAULT_SECTIONS) {
    if (!usedIds.has(defaultSection.id)) {
      result.sections.push({ ...defaultSection });
    }
  }

  return result;
}

/**
 * Save settings to localStorage
 */
function saveSettings(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch (err) {
    console.error('Failed to save home settings:', err);
  }
}

// ============ Public API ============

/**
 * Subscribe to settings changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  // Don't call immediately - caller should use getSettings() for initial state
  return () => listeners.delete(listener);
}

/**
 * Get current settings
 */
export function getSettings(): HomeSettings {
  return settings;
}

/**
 * Get visible sections in order
 */
export function getVisibleSections(): HomeSection[] {
  return settings.sections.filter(s => s.visible);
}

/**
 * Toggle section visibility
 */
export function toggleSectionVisibility(sectionId: HomeSectionId): void {
  const section = settings.sections.find(s => s.id === sectionId);
  if (section) {
    section.visible = !section.visible;
    saveSettings();
    notifyListeners();
  }
}

/**
 * Set section visibility
 */
export function setSectionVisibility(sectionId: HomeSectionId, visible: boolean): void {
  const section = settings.sections.find(s => s.id === sectionId);
  if (section) {
    section.visible = visible;
    saveSettings();
    notifyListeners();
  }
}

/**
 * Move section up in order
 */
export function moveSectionUp(sectionId: HomeSectionId): void {
  const index = settings.sections.findIndex(s => s.id === sectionId);
  if (index > 0) {
    const temp = settings.sections[index];
    settings.sections[index] = settings.sections[index - 1];
    settings.sections[index - 1] = temp;
    saveSettings();
    notifyListeners();
  }
}

/**
 * Move section down in order
 */
export function moveSectionDown(sectionId: HomeSectionId): void {
  const index = settings.sections.findIndex(s => s.id === sectionId);
  if (index >= 0 && index < settings.sections.length - 1) {
    const temp = settings.sections[index];
    settings.sections[index] = settings.sections[index + 1];
    settings.sections[index + 1] = temp;
    saveSettings();
    notifyListeners();
  }
}

/**
 * Reorder sections by providing new order of IDs
 */
export function reorderSections(newOrder: HomeSectionId[]): void {
  const sectionMap = new Map(settings.sections.map(s => [s.id, s]));
  const reordered: HomeSection[] = [];

  for (const id of newOrder) {
    const section = sectionMap.get(id);
    if (section) {
      reordered.push(section);
    }
  }

  settings.sections = reordered;
  saveSettings();
  notifyListeners();
}

/**
 * Toggle greeting visibility
 */
export function toggleGreeting(): void {
  settings.greeting.enabled = !settings.greeting.enabled;
  saveSettings();
  notifyListeners();
}

/**
 * Set greeting enabled state
 */
export function setGreetingEnabled(enabled: boolean): void {
  settings.greeting.enabled = enabled;
  saveSettings();
  notifyListeners();
}

/**
 * Set custom greeting text (null = use default time-based)
 */
export function setCustomGreeting(text: string | null): void {
  settings.greeting.customText = text && text.trim() ? text.trim() : null;
  saveSettings();
  notifyListeners();
}

/**
 * Get greeting text for display
 */
export function getGreetingText(userName: string): string {
  if (settings.greeting.customText) {
    return settings.greeting.customText.replace('{name}', getFirstName(userName));
  }
  return getTimeBasedGreeting(userName);
}

/**
 * Get time-based greeting
 */
function getTimeBasedGreeting(userName: string): string {
  const hour = new Date().getHours();
  const firstName = getFirstName(userName);

  if (hour >= 5 && hour < 12) {
    return `Good morning, ${firstName}`;
  } else if (hour >= 12 && hour < 17) {
    return `Good afternoon, ${firstName}`;
  } else if (hour >= 17 && hour < 21) {
    return `Good evening, ${firstName}`;
  } else {
    return `Good night, ${firstName}`;
  }
}

/**
 * Extract first name from full name
 */
function getFirstName(fullName: string): string {
  return fullName.split(' ')[0] || fullName;
}

/**
 * Reset to defaults
 */
export function resetToDefaults(): void {
  settings = { ...DEFAULT_SETTINGS, sections: DEFAULT_SECTIONS.map(s => ({ ...s })) };
  saveSettings();
  notifyListeners();
}
