import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { buildPath } from '../lib/routes'

export function Navigation() {
  const { t } = useTranslation()
  const { language, page, theme, setLanguage, toggleTheme } = useApp()
  const [menuOpen, setMenuOpen] = useState(false)

  const home = buildPath(language, 'home')

  const links = [
    { key: 'features', label: t('nav.features'), href: `${home}#features` },
    { key: 'downloads', label: t('nav.downloads'), href: `${home}#downloads` },
    { key: 'changelog', label: t('nav.changelog'), href: buildPath(language, 'changelog') },
    { key: 'licenses', label: t('nav.licenses'), href: buildPath(language, 'licenses') },
  ]

  const handleLanguage = (next: 'en' | 'es') => {
    setLanguage(next)
    setMenuOpen(false)
  }

  const themeLabel = theme === 'oled' ? t('nav.themeOled') : t('nav.themeDark')

  return (
    <nav className="nav" aria-label="Primary">
      <div className="container nav__inner">
        <a className="nav__brand" href={home} aria-label="QBZ home">
          <img src="/assets/brand/logo-64.webp" alt="" width={30} height={30} />
          <span>QBZ</span>
        </a>
        <div className="nav__links">
          {links.map((link) => (
            <a
              key={link.key}
              className={`nav-link ${page === link.key ? 'nav-link--active' : ''}`}
              href={link.href}
            >
              {link.label}
            </a>
          ))}
          <a className="nav-link" href="https://github.com/vicrodh/qbz" target="_blank" rel="noreferrer">
            {t('nav.github')}
          </a>
        </div>
        <div className="nav__actions">
          <button className="toggle-btn" type="button" onClick={toggleTheme} aria-label={`Theme: ${themeLabel}`}>
            {themeLabel}
          </button>
          <div className="lang-switch" role="group" aria-label="Language">
            <button className={`lang-btn ${language === 'en' ? 'lang-btn--active' : ''}`} type="button" onClick={() => handleLanguage('en')} aria-pressed={language === 'en'}>
              EN
            </button>
            <button className={`lang-btn ${language === 'es' ? 'lang-btn--active' : ''}`} type="button" onClick={() => handleLanguage('es')} aria-pressed={language === 'es'}>
              ES
            </button>
          </div>
          <a className="btn btn-primary btn-sm" href={`${home}#downloads`}>
            {t('nav.download')}
          </a>
          <button
            className="nav__toggle"
            type="button"
            aria-expanded={menuOpen}
            aria-controls="mobile-menu"
            onClick={() => setMenuOpen((open) => !open)}
          >
            {menuOpen ? t('nav.close') : t('nav.menu')}
          </button>
        </div>
      </div>
      <div id="mobile-menu" className={`mobile-menu ${menuOpen ? 'mobile-menu--open' : ''}`}>
        <div className="container mobile-menu__inner">
          {links.map((link) => (
            <a
              key={link.key}
              className={`nav-link ${page === link.key ? 'nav-link--active' : ''}`}
              href={link.href}
              onClick={() => setMenuOpen(false)}
            >
              {link.label}
            </a>
          ))}
          <a className="nav-link" href="https://github.com/vicrodh/qbz" target="_blank" rel="noreferrer" onClick={() => setMenuOpen(false)}>
            {t('nav.github')}
          </a>
          <div className="mobile-menu__actions">
            <button className="toggle-btn" type="button" onClick={toggleTheme}>
              {themeLabel}
            </button>
            <div className="lang-switch" role="group" aria-label="Language">
              <button className={`lang-btn ${language === 'en' ? 'lang-btn--active' : ''}`} type="button" onClick={() => handleLanguage('en')}>
                EN
              </button>
              <button className={`lang-btn ${language === 'es' ? 'lang-btn--active' : ''}`} type="button" onClick={() => handleLanguage('es')}>
                ES
              </button>
            </div>
          </div>
        </div>
      </div>
    </nav>
  )
}
