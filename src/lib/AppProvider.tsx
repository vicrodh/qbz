import { useCallback, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { AppContext, type AppContextValue, type Theme } from './appContext'
import { buildPath, normalizeLanguage } from './routes'
import type { Language, Page } from './routes'

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

  const setLanguage = useCallback((next: Language) => {
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
  }, [language, page])

  const toggleTheme = useCallback(() => {
    setTheme((current) => (current === 'dark' ? 'oled' : 'dark'))
  }, [])

  const value = useMemo<AppContextValue>(
    () => ({ language, page, theme, setLanguage, toggleTheme }),
    [language, page, setLanguage, theme, toggleTheme],
  )

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>
}
