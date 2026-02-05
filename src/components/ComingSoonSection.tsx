import { useTranslation } from 'react-i18next'

export function ComingSoonSection() {
    const { t } = useTranslation()

    type ComingSoonItem = {
        title: string
        text: string
    }

    const items = t('comingSoon.items', { returnObjects: true }) as ComingSoonItem[]

    return (
        <section className="section section--muted">
            <div className="container">
                <h2 className="section__title">{t('comingSoon.title')}</h2>
                <p className="section__subtitle">{t('comingSoon.lead')}</p>

                <div className="feature-grid" style={{ marginTop: 32 }}>
                    {items.map((item, index) => (
                        <div key={index} className="feature-card feature-card--glass">
                            <div className="feature-card__header">
                                <div className="badge badge--warning" style={{ marginBottom: '1rem', display: 'inline-block' }}>{t('comingSoon.badge')}</div>
                            </div>
                            <div className="feature-card__title">{item.title}</div>
                            <div className="feature-card__text">{item.text}</div>
                        </div>
                    ))}
                </div>
            </div>
        </section>
    )
}
