import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { buildPath } from '../lib/routes'

export function Footer() {
  const { t } = useTranslation()
  const { language } = useApp()

  return (
    <footer className="footer">
      <div className="container footer__grid">
        <div>
          <div className="nav__brand">
            <img src="/assets/brand/logo-64.webp" alt="QBZ - Native Qobuz client for Linux" title="QBZ" width={28} height={28} />
            <span>QBZ</span>
          </div>
          <p className="footer__small">{t('footer.rights')}</p>
        </div>
        <div>
          <p className="footer__small">{t('footer.disclaimer')}</p>
        </div>
        <div>
          <div className="footer__links">
            <a href={buildPath(language, 'home')}>{t('nav.home')}</a>
            <a href={buildPath(language, 'changelog')}>{t('nav.changelog')}</a>
            <a href={buildPath(language, 'licenses')}>{t('nav.licenses')}</a>
            <a href="https://github.com/vicrodh/qbz" target="_blank" rel="noreferrer">
              {t('nav.github')}
            </a>
            <a href="https://ko-fi.com/W7W51SMYGW" target="_blank" rel="noreferrer">
              Ko-fi
            </a>
          </div>
        </div>
      </div>
    </footer>
  )
}
