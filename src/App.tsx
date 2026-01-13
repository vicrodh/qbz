import { Navigation } from './components/Navigation'
import { Footer } from './components/Footer'
import { HomePage } from './pages/HomePage'
import { ChangelogPage } from './pages/ChangelogPage'
import { LicensesPage } from './pages/LicensesPage'
import { useApp } from './lib/appContext'

function App() {
  const { page } = useApp()

  return (
    <div className="main">
      <Navigation />
      <main>
        {page === 'home' && <HomePage />}
        {page === 'changelog' && <ChangelogPage />}
        {page === 'licenses' && <LicensesPage />}
      </main>
      <Footer />
    </div>
  )
}

export default App
