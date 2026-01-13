import { useTranslation } from 'react-i18next'

const CATEGORY_KEYS = ['core', 'audio', 'casting', 'integrations', 'website'] as const

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

        <div className="grid" style={{ marginTop: 32 }}>
          <div className="card card--highlight">
            <img className="icon-mono" src="/assets/icons/mit-license.svg" alt="" />
            <div className="download-meta">
              <div className="download-meta__name">{t('licenses.qbzLicense')}</div>
              <div className="download-meta__file">{t('licenses.qbzLicenseBody')}</div>
            </div>
            <a className="btn btn-ghost" href="https://github.com/vicrodh/qbz/blob/main/LICENSE" target="_blank" rel="noreferrer">
              {t('licenses.viewLicense')}
            </a>
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
      </div>
    </section>
  )
}
