import { useTranslation } from 'react-i18next'

const CATEGORY_KEYS = ['core', 'audio', 'casting', 'lyrics', 'integrations', 'inspiration', 'website'] as const

const DONATION_LINKS = {
  kde: 'https://kde.org/community/donations/',
  neovim: 'https://neovim.io/sponsors/',
  arch: 'https://archlinux.org/donate/',
}

function getProjectYears(): number {
  const startYear = 2023
  const currentYear = new Date().getFullYear()
  return currentYear - startYear
}

export function LicensesPage() {
  const { t } = useTranslation()

  const categories = CATEGORY_KEYS.map((key) => ({
    key,
    title: t(`licenses.categories.${key}.title`),
    items: t(`licenses.categories.${key}.items`, { returnObjects: true }) as string[],
  }))

  return (
    <section className="section">
      <div className="container">
        <h1 className="section__title">{t('licenses.title')}</h1>
        <p className="section__subtitle">{t('licenses.lead')}</p>

        <div className="license-hero" style={{ marginTop: 32 }}>
          <div className="card card--highlight license-card" style={{ display: 'flex', alignItems: 'center', gap: 32 }}>
            <img className="license-icon license-icon--large" src="/assets/icons/mit-license.svg" alt="MIT License" title="MIT License" style={{ width: 106, height: 106, filter: 'brightness(0) invert(1)', flexShrink: 0 }} />
            <div style={{ flex: 1 }}>
              <div className="download-meta">
                <div className="download-meta__name">{t('licenses.qbzLicense')}</div>
                <div className="download-meta__file">{t('licenses.qbzLicenseBody')}</div>
              </div>
              <a className="btn btn-ghost" href="https://github.com/vicrodh/qbz/blob/main/LICENSE" target="_blank" rel="noreferrer" style={{ marginTop: 16 }}>
                {t('licenses.viewLicense')}
              </a>
            </div>
          </div>
        </div>

        <div className="feature-grid" style={{ marginTop: 32 }}>
          {categories.map((category) => (
            <div key={category.key} className="feature-card">
              <div className="feature-card__title">{category.title}</div>
              <ul className="list">
                {category.items.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
            </div>
          ))}
        </div>

        <div className="card" style={{ marginTop: 32 }}>
          <p>{t('licenses.acknowledgments')}</p>
          <p style={{ marginTop: 12 }}>{t('licenses.qobuzDisclaimer')}</p>
        </div>

        <div className="about-section" style={{ marginTop: 48 }}>
          <h2 className="section__title">{t('about.title')}</h2>
          <div className="about-content" style={{ marginTop: 16 }}>
            <p style={{ whiteSpace: 'pre-line' }}>{t('about.content', { years: getProjectYears() })}</p>
          </div>

          <h3 className="feature-card__title" style={{ marginTop: 32 }}>{t('about.donationsTitle')}</h3>
          <p style={{ marginTop: 8 }}>{t('about.donationsContent')}</p>
          <div className="donation-links" style={{ marginTop: 16, display: 'flex', gap: 16, flexWrap: 'wrap' }}>
            <a className="btn btn-ghost" href={DONATION_LINKS.kde} target="_blank" rel="noreferrer">
              {t('about.donationLinks.kde')}
            </a>
            <a className="btn btn-ghost" href={DONATION_LINKS.neovim} target="_blank" rel="noreferrer">
              {t('about.donationLinks.neovim')}
            </a>
            <a className="btn btn-ghost" href={DONATION_LINKS.arch} target="_blank" rel="noreferrer">
              {t('about.donationLinks.arch')}
            </a>
          </div>
        </div>
      </div>
    </section>
  )
}
