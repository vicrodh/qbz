import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import './lib/i18n'
import App from './App'
import { AppProvider } from './lib/appContext'
import { normalizeLanguage, normalizePage } from './lib/routes'

const language = normalizeLanguage(document.documentElement.dataset.lang)
const page = normalizePage(document.documentElement.dataset.page)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <AppProvider language={language} page={page}>
      <App />
    </AppProvider>
  </StrictMode>,
)
