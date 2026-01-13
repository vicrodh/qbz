import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import { en } from '../i18n/en'
import { es } from '../i18n/es'
import { normalizeLanguage } from './routes'

const initialLanguage = normalizeLanguage(document.documentElement.dataset.lang)

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    es: { translation: es },
  },
  lng: initialLanguage,
  fallbackLng: 'en',
  interpolation: {
    escapeValue: false,
  },
})

export default i18n
