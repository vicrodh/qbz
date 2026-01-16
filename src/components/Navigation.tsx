import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { buildPath } from '../lib/routes'

const NAV_ITEMS = ['home', 'changelog', 'licenses'] as const

export function Navigation() {
  const { t } = useTranslation()
  const { language, page, theme, setLanguage, toggleTheme } = useApp()
  const [menuOpen, setMenuOpen] = useState(false)

  const links = NAV_ITEMS.map((item) => ({
    key: item,
    label: t(`nav.${item}`),
    href: buildPath(language, item),
  }))

  const handleLanguage = (next: 'en' | 'es') => {
    setLanguage(next)
    setMenuOpen(false)
  }

  return (
    <nav className="nav">
      <div className="container nav__inner">
        <a className="nav__brand" href={buildPath(language, 'home')}>
          <img src="/assets/brand/logo-64.webp" alt="QBZ - Native Qobuz client for Linux" title="QBZ" width={32} height={32} />
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
          <button
            className="toggle-btn"
            type="button"
            onClick={toggleTheme}
            aria-label={theme === 'oled' ? t('nav.themeDark') : t('nav.themeOled')}
          >
            {theme === 'oled' ? t('nav.themeOled') : t('nav.themeDark')}
          </button>
          <div className="lang-switch">
            <button
              className={`lang-btn ${language === 'en' ? 'lang-btn--active' : ''}`}
              type="button"
              onClick={() => handleLanguage('en')}
            >
              EN
            </button>
            <button
              className={`lang-btn ${language === 'es' ? 'lang-btn--active' : ''}`}
              type="button"
              onClick={() => handleLanguage('es')}
            >
              ES
            </button>
          </div>
          <a className="btn btn-primary" href={`${buildPath(language, 'home')}#downloads`}>
            {t('nav.download')}
          </a>
          <button
            className="nav__toggle"
            type="button"
            aria-label="Toggle navigation"
            onClick={() => setMenuOpen((open) => !open)}
          >
            {menuOpen ? t('nav.close') : t('nav.menu')}
          </button>
        </div>
      </div>
      <div className={`mobile-menu ${menuOpen ? 'mobile-menu--open' : ''}`}>
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
          <a
            className="nav-link"
            href="https://github.com/vicrodh/qbz"
            target="_blank"
            rel="noreferrer"
            onClick={() => setMenuOpen(false)}
          >
            {t('nav.github')}
          </a>
          <a
            className="btn btn-primary"
            href={`${buildPath(language, 'home')}#downloads`}
            onClick={() => setMenuOpen(false)}
          >
            {t('nav.download')}
          </a>
          <div className="mobile-menu__actions">
            <button className="toggle-btn" type="button" onClick={toggleTheme}>
              {theme === 'oled' ? t('nav.themeOled') : t('nav.themeDark')}
            </button>
            <div className="lang-switch">
              <button
                className={`lang-btn ${language === 'en' ? 'lang-btn--active' : ''}`}
                type="button"
                onClick={() => handleLanguage('en')}
              >
                EN
              </button>
              <button
                className={`lang-btn ${language === 'es' ? 'lang-btn--active' : ''}`}
                type="button"
                onClick={() => handleLanguage('es')}
              >
                ES
              </button>
            </div>
          </div>
        </div>
      </div>
    </nav>
  )
}
