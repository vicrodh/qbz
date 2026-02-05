import { Suspense, lazy } from 'react'
import { Navigation } from './components/Navigation'
import { Footer } from './components/Footer'
import { HomePage } from './pages/HomePage'
import { QobuzLinuxPage } from './pages/QobuzLinuxPage'
import { useApp } from './lib/appContext'

// Lazy load secondary pages to reduce initial bundle
const ChangelogPage = lazy(() => import('./pages/ChangelogPage').then(m => ({ default: m.ChangelogPage })))
const LicensesPage = lazy(() => import('./pages/LicensesPage').then(m => ({ default: m.LicensesPage })))

function App() {
  const { page } = useApp()

  return (
    <div className="main">
      <Navigation />
      <main>
        {page === 'home' && <HomePage />}
        {page === 'changelog' && (
          <Suspense fallback={<div className="section"><div className="container">Loading...</div></div>}>
            <ChangelogPage />
          </Suspense>
        )}
        {page === 'licenses' && (
          <Suspense fallback={<div className="section"><div className="container">Loading...</div></div>}>
            <LicensesPage />
          </Suspense>
        )}
        {page === 'qobuz-linux' && <QobuzLinuxPage />}
      </main>
      <Footer />
    </div>
  )
}

export default App
