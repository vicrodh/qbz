import { createContext, useContext } from 'react'
import type { Language, Page } from './routes'

export type Theme = 'dark' | 'oled'

export interface AppContextValue {
  language: Language
  page: Page
  theme: Theme
  setLanguage: (language: Language) => void
  toggleTheme: () => void
}

export const AppContext = createContext<AppContextValue | undefined>(undefined)

export function useApp() {
  const context = useContext(AppContext)
  if (!context) {
    throw new Error('useApp must be used within AppProvider')
  }
  return context
}
