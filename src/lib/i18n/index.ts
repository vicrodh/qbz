import { browser } from '$app/environment';
import { init, register, getLocaleFromNavigator, locale } from 'svelte-i18n';

// Register locales
register('en', () => import('./locales/en.json'));
register('es', () => import('./locales/es.json'));

// Initialize i18n
export function initI18n() {
  init({
    fallbackLocale: 'en',
    initialLocale: browser ? getStoredLocale() || getLocaleFromNavigator() : 'en',
  });
}

// Get stored locale from localStorage
function getStoredLocale(): string | null {
  if (!browser) return null;
  return localStorage.getItem('qbz-locale');
}

// Set and persist locale
export function setLocale(newLocale: string) {
  if (browser) {
    localStorage.setItem('qbz-locale', newLocale);
  }
  locale.set(newLocale);
}

// Available locales
export const locales = [
  { code: 'en', name: 'English', nativeName: 'English' },
  { code: 'es', name: 'Spanish', nativeName: 'Español' },
] as const;

export type LocaleCode = (typeof locales)[number]['code'];

// Re-export svelte-i18n utilities for convenience
export { t, locale, locales as availableLocales } from 'svelte-i18n';
