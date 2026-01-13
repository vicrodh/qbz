export type Language = 'en' | 'es';
export type Page = 'home' | 'changelog' | 'licenses';

export const normalizeLanguage = (value?: string | null): Language => {
  return value === 'es' ? 'es' : 'en';
};

export const normalizePage = (value?: string | null): Page => {
  if (value === 'changelog' || value === 'licenses') {
    return value;
  }
  return 'home';
};

export const buildPath = (language: Language, page: Page): string => {
  const prefix = language === 'es' ? '/es' : '';
  if (page === 'home') {
    return `${prefix}/`;
  }
  return `${prefix}/${page}/`;
};
