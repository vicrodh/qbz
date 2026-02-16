import { browser } from '$app/environment';
import { init, register, getLocaleFromNavigator, locale } from 'svelte-i18n';
import { invoke } from '@tauri-apps/api/core';

// Register locales
register('en', () => import('./locales/en.json'));
register('es', () => import('./locales/es.json'));
register('fr', () => import('./locales/fr.json'));
register('de', () => import('./locales/de.json'));

// Initialize i18n
export function initI18n() {
  const initialLocale = browser ? getStoredLocale() || getLocaleFromNavigator() : 'en';
  init({
    fallbackLocale: 'en',
    initialLocale,
  });

  // Set the API locale to match
  if (browser) {
    invoke('v2_set_api_locale', { locale: initialLocale }).catch((err) => {
      console.warn('Failed to set API locale:', err);
    });
  }
}

// Get stored locale from localStorage
function getStoredLocale(): string | null {
  if (!browser) return null;
  return localStorage.getItem('qbz-locale');
}

// Set and persist locale
export async function setLocale(newLocale: string) {
  if (browser) {
    localStorage.setItem('qbz-locale', newLocale);
    // Also update the API client locale
    try {
      await invoke('v2_set_api_locale', { locale: newLocale });
    } catch (err) {
      console.warn('Failed to update API locale:', err);
    }
  }
  locale.set(newLocale);
}

// Available locales
export const locales = [
  { code: 'en', name: 'English', nativeName: 'English' },
  { code: 'es', name: 'Spanish', nativeName: 'Español' },
  { code: 'fr', name: 'French', nativeName: 'Français' },
  { code: 'de', name: 'German', nativeName: 'Deutsch' },
] as const;

export type LocaleCode = (typeof locales)[number]['code'];

// Re-export svelte-i18n utilities for convenience
export { t, locale, locales as availableLocales } from 'svelte-i18n';
