import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { buildPath } from '../lib/routes'

export function Footer() {
  const { t } = useTranslation()
  const { language } = useApp()
  const year = new Date().getFullYear()

  return (
    <footer className="footer">
      <div className="container">
        <div className="footer__grid">
          <div>
            <div className="nav__brand">
              <img src="/assets/brand/logo-64.webp" alt="" width={28} height={28} />
              <span>QBZ</span>
            </div>
            <p className="footer__small">{t('footer.rights')}</p>
          </div>
          <div>
            <p className="footer__small" style={{ marginTop: 0 }}>{t('footer.disclaimer')}</p>
          </div>
          <div className="footer__links">
            <a href={buildPath(language, 'home')}>{t('nav.home')}</a>
            <a href={buildPath(language, 'qobuz-linux')}>{t('footer.qobuzLinux')}</a>
            <a href={buildPath(language, 'changelog')}>{t('nav.changelog')}</a>
            <a href={buildPath(language, 'licenses')}>{t('nav.licenses')}</a>
            <a href="https://github.com/vicrodh/qbz/wiki/Headless-Daemon" target="_blank" rel="noreferrer">
              {t('nav.qbzdManual')}
            </a>
            <a href="https://github.com/vicrodh/qbz/wiki" target="_blank" rel="noreferrer">
              {t('nav.wiki')}
            </a>
            <a href="https://github.com/vicrodh/qbz" target="_blank" rel="noreferrer">
              {t('nav.github')}
            </a>
            <a href="https://ko-fi.com/W7W51SMYGW" target="_blank" rel="noreferrer">
              {t('footer.kofi')}
            </a>
          </div>
        </div>
        <div className="footer__row">
          <span>© {year} QBZ · MIT</span>
          <span>{t('footer.madeIn')}</span>
        </div>
      </div>
    </footer>
  )
}
