import { createContext, useContext, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { buildPath, normalizeLanguage } from './routes'
import type { Language, Page } from './routes'

type Theme = 'dark' | 'oled'

interface AppContextValue {
  language: Language
  page: Page
  theme: Theme
  setLanguage: (language: Language) => void
  toggleTheme: () => void
}

const AppContext = createContext<AppContextValue | undefined>(undefined)

interface AppProviderProps {
  children: ReactNode
  language: Language
  page: Page
}

export function AppProvider({ children, language: initialLanguage, page }: AppProviderProps) {
  const { i18n } = useTranslation()
  const storedLanguageValue = localStorage.getItem('qbz-language')
  const storedLanguage = storedLanguageValue ? normalizeLanguage(storedLanguageValue) : null
  const shouldRedirect = storedLanguage !== null && storedLanguage !== initialLanguage
  const [language, setLanguageState] = useState<Language>(shouldRedirect ? storedLanguage : initialLanguage)
  const [theme, setTheme] = useState<Theme>(() => {
    const stored = localStorage.getItem('qbz-theme')
    return stored === 'oled' ? 'oled' : 'dark'
  })

  useEffect(() => {
    if (shouldRedirect) return
    i18n.changeLanguage(language)
    localStorage.setItem('qbz-language', language)
  }, [i18n, language, shouldRedirect])

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    localStorage.setItem('qbz-theme', theme)
  }, [theme])

  useEffect(() => {
    if (!shouldRedirect || !storedLanguage) return
    window.location.assign(buildPath(storedLanguage, page))
  }, [page, shouldRedirect, storedLanguage])

  const setLanguage = (next: Language) => {
    if (next === language) {
      return
    }
    const target = buildPath(next, page)
    const normalizePath = (value: string) => (value.endsWith('/') ? value : `${value}/`)
    localStorage.setItem('qbz-language', next)
    if (normalizePath(target) !== normalizePath(window.location.pathname)) {
      window.location.assign(target)
      return
    }
    setLanguageState(next)
  }

  const toggleTheme = () => {
    setTheme((current) => (current === 'dark' ? 'oled' : 'dark'))
  }

  const value = useMemo(
    () => ({ language, page, theme, setLanguage, toggleTheme }),
    [language, page, theme],
  )

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>
}

export function useApp() {
  const context = useContext(AppContext)
  if (!context) {
    throw new Error('useApp must be used within AppProvider')
  }
  return context
}
