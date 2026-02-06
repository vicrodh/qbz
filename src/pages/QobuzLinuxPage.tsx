import { useTranslation } from 'react-i18next'

export function QobuzLinuxPage() {
    const { t } = useTranslation()

    const whyNativeBullets = t('qobuzLinux.whyNative.bullets', { returnObjects: true }) as string[]
    const differentFeatures = t('qobuzLinux.different.features', { returnObjects: true }) as Array<{ title: string; text: string }>
    const alsaBullets = t('qobuzLinux.bitPerfect.alsa.bullets', { returnObjects: true }) as string[]
    const pipewireBullets = t('qobuzLinux.bitPerfect.pipewire.bullets', { returnObjects: true }) as string[]
    const wrappersBullets = t('qobuzLinux.wrappers.bullets', { returnObjects: true }) as string[]

    const comparisonHeaders = t('qobuzLinux.comparison.headers', { returnObjects: true }) as string[]
    const comparisonRows = t('qobuzLinux.comparison.rows', { returnObjects: true }) as Array<{ feature: string; qbz: boolean; web: boolean; webText: string }>

    const featuresItems = t('qobuzLinux.features.items', { returnObjects: true }) as Array<{ title: string; text: string }>
    const forWhoBullets = t('qobuzLinux.forWho.bullets', { returnObjects: true }) as string[]
    const openSourceBullets = t('qobuzLinux.openSource.bullets', { returnObjects: true }) as string[]

    return (
        <>
            {/* Hero Section */}
            <section className="hero" style={{ paddingBottom: 80 }}>
                <div className="container">
                    <span className="kicker">{t('qobuzLinux.hero.kicker')}</span>
                    <h1 className="hero__title" style={{ fontSize: 'clamp(2.2rem, 5vw, 3.4rem)' }}>
                        {t('qobuzLinux.hero.title')}
                    </h1>
                    <p className="hero__lead" style={{ marginTop: 24, maxWidth: 800 }}>
                        {t('qobuzLinux.hero.lead1')}
                    </p>
                    <p className="hero__lead" style={{ marginTop: 16, maxWidth: 800 }}>
                        {t('qobuzLinux.hero.lead2')}
                    </p>
                    <div className="hero__cta" style={{ marginTop: 32 }}>
                        <a className="btn btn-primary" href="/#downloads">
                            {t('qobuzLinux.hero.ctaDownload')}
                        </a>
                        <a className="btn btn-ghost" href="https://github.com/vicrodh/qbz" target="_blank" rel="noreferrer">
                            {t('qobuzLinux.hero.ctaGithub')}
                        </a>
                    </div>
                </div>
            </section>

            {/* Why Qobuz needs a native Linux client */}
            <section className="section section--muted">
                <div className="container">
                    <h2 className="section__title">{t('qobuzLinux.whyNative.title')}</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        {t('qobuzLinux.whyNative.lead')}
                    </p>
                    <ul className="list" style={{ marginTop: 24 }}>
                        {whyNativeBullets.map((item, i) => (
                            <li key={i}>{item}</li>
                        ))}
                    </ul>
                </div>
            </section>

            {/* What makes QBZ different */}
            <section className="section">
                <div className="container">
                    <h2 className="section__title">{t('qobuzLinux.different.title')}</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        {t('qobuzLinux.different.lead')}
                    </p>
                    <div className="feature-grid" style={{ marginTop: 32 }}>
                        {differentFeatures.map((feat, i) => (
                            <div key={i} className="feature-card">
                                <div className="feature-card__title">{feat.title}</div>
                                <div className="feature-card__text">{feat.text}</div>
                            </div>
                        ))}
                    </div>
                </div>
            </section>

            {/* Bit-perfect playback on Linux */}
            <section className="section section--muted">
                <div className="container">
                    <h2 className="section__title">{t('qobuzLinux.bitPerfect.title')}</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        {t('qobuzLinux.bitPerfect.lead')}
                    </p>

                    <h3 style={{ marginTop: 32, fontSize: '1.3rem' }}>{t('qobuzLinux.bitPerfect.alsa.title')}</h3>
                    <p style={{ color: 'var(--text-secondary)', marginTop: 8, maxWidth: 700 }}>
                        {t('qobuzLinux.bitPerfect.alsa.text')}
                    </p>
                    <ul className="list" style={{ marginTop: 16 }}>
                        {alsaBullets.map((item, i) => (
                            <li key={i}>{item}</li>
                        ))}
                    </ul>

                    <h3 style={{ marginTop: 32, fontSize: '1.3rem' }}>{t('qobuzLinux.bitPerfect.pipewire.title')}</h3>
                    <p style={{ color: 'var(--text-secondary)', marginTop: 8, maxWidth: 700 }}>
                        {t('qobuzLinux.bitPerfect.pipewire.text')}
                    </p>
                    <ul className="list" style={{ marginTop: 16 }}>
                        {pipewireBullets.map((item, i) => (
                            <li key={i}>{item}</li>
                        ))}
                    </ul>
                </div>
            </section>

            {/* Why web wrappers fall short */}
            <section className="section">
                <div className="container">
                    <h2 className="section__title">{t('qobuzLinux.wrappers.title')}</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        {t('qobuzLinux.wrappers.lead')}
                    </p>
                    <ul className="list" style={{ marginTop: 24 }}>
                        {wrappersBullets.map((item, i) => (
                            <li key={i}>{item}</li>
                        ))}
                    </ul>
                    <p style={{ color: 'var(--text-tertiary)', marginTop: 24, fontSize: '0.95rem' }}>
                        {t('qobuzLinux.wrappers.note')}
                    </p>
                </div>
            </section>

            {/* Comparison Table */}
            <section className="section section--muted">
                <div className="container">
                    <h2 className="section__title">{t('qobuzLinux.comparison.title')}</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        {t('qobuzLinux.comparison.lead')}
                    </p>
                    <div style={{ overflowX: 'auto', marginTop: 32 }}>
                        <table style={{ width: '100%', borderCollapse: 'collapse', minWidth: 600 }}>
                            <thead>
                                <tr style={{ borderBottom: '1px solid var(--border)' }}>
                                    {comparisonHeaders.map((h, i) => (
                                        <th key={i} style={{ textAlign: i === 0 ? 'left' : 'center', padding: '12px 16px', color: 'var(--text-primary)' }}>
                                            {h}
                                        </th>
                                    ))}
                                </tr>
                            </thead>
                            <tbody>
                                {comparisonRows.map((row, i) => (
                                    <tr key={i} style={{ borderBottom: i === comparisonRows.length - 1 ? 'none' : '1px solid var(--border)' }}>
                                        <td style={{ padding: '12px 16px', color: 'var(--text-secondary)' }}>{row.feature}</td>
                                        <td style={{ textAlign: 'center', padding: '12px 16px', color: row.qbz ? 'var(--success)' : 'var(--text-tertiary)' }}>
                                            {row.qbz ? '✓' : '✗'}
                                        </td>
                                        <td style={{ textAlign: 'center', padding: '12px 16px', color: row.web ? 'var(--success)' : 'var(--text-tertiary)' }}>
                                            {row.web ? '✓' : row.webText}
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    </div>
                </div>
            </section>

            {/* Features at a glance */}
            <section className="section">
                <div className="container">
                    <h2 className="section__title">{t('qobuzLinux.features.title')}</h2>
                    <div className="feature-grid" style={{ marginTop: 32 }}>
                        {featuresItems.map((feat, i) => (
                            <div key={i} className="feature-card">
                                <div className="feature-card__title">{feat.title}</div>
                                <div className="feature-card__text">{feat.text}</div>
                            </div>
                        ))}
                    </div>
                </div>
            </section>

            {/* Who QBZ is for */}
            <section className="section section--muted">
                <div className="container">
                    <h2 className="section__title">{t('qobuzLinux.forWho.title')}</h2>
                    <ul className="list" style={{ marginTop: 16 }}>
                        {forWhoBullets.map((item, i) => (
                            <li key={i}>{item}</li>
                        ))}
                    </ul>
                    <p style={{ color: 'var(--text-tertiary)', marginTop: 24, fontSize: '0.95rem' }}>
                        {t('qobuzLinux.forWho.note')}
                    </p>
                </div>
            </section>

            {/* Open source and transparent */}
            <section className="section">
                <div className="container">
                    <h2 className="section__title">{t('qobuzLinux.openSource.title')}</h2>
                    <ul className="list" style={{ marginTop: 16 }}>
                        {openSourceBullets.map((item, i) => (
                            <li key={i}>{item}</li>
                        ))}
                    </ul>
                </div>
            </section>

            {/* Installation */}
            <section className="section section--muted">
                <div className="container">
                    <h2 className="section__title">{t('qobuzLinux.install.title')}</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        {t('qobuzLinux.install.lead')}
                    </p>
                    <div style={{ marginTop: 24 }}>
                        <a className="btn btn-primary" href="/#downloads">
                            {t('qobuzLinux.install.cta')}
                        </a>
                    </div>
                </div>
            </section>

            {/* Legal notice */}
            <section className="section">
                <div className="container">
                    <h2 className="section__title">{t('qobuzLinux.legal.title')}</h2>
                    <p style={{ color: 'var(--text-secondary)', maxWidth: 800 }}>
                        {t('qobuzLinux.legal.text')}
                    </p>
                </div>
            </section>
        </>
    )
}
