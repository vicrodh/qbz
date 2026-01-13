import { useTranslation } from 'react-i18next'
import { DownloadSection } from '../components/DownloadSection'
import { useApp } from '../lib/appContext'
import { buildPath } from '../lib/routes'

const CAPABILITY_KEYS = ['audio', 'library', 'playlists', 'desktop', 'casting'] as const

type CapabilityKey = (typeof CAPABILITY_KEYS)[number]

export function HomePage() {
  const { t } = useTranslation()
  const { language } = useApp()

  const stats = [
    { icon: '/assets/icons/hi-res.svg', label: t('hero.stats.audio') },
    { icon: '/assets/icons/home-gear.svg', label: t('hero.stats.dac') },
    { icon: '/assets/icons/cast-audio.svg', label: t('hero.stats.casting') },
  ]

  const capabilityIcons: Record<CapabilityKey, string> = {
    audio: '/assets/icons/hi-res.svg',
    library: '/assets/icons/cd-music.svg',
    playlists: '/assets/icons/mp3-player.svg',
    desktop: '/assets/icons/home-gear.svg',
    casting: '/assets/icons/cast-audio.svg',
  }

  const goals = t('goals.items', { returnObjects: true }) as Array<{ title: string; text: string }>
  const screenshots = t('screenshots.items', { returnObjects: true }) as Array<{ title: string; text: string }>
  const audienceItems = t('audience.items', { returnObjects: true }) as string[]
  const openSourceItems = t('openSource.items', { returnObjects: true }) as string[]
  const apiItems = t('apis.items', { returnObjects: true }) as string[]

  const capabilityCards = CAPABILITY_KEYS.map((key) => ({
    key,
    title: t(`capabilities.items.${key}.title`),
    bullets: t(`capabilities.items.${key}.bullets`, { returnObjects: true }) as string[],
  }))

  return (
    <>
      <section className="hero">
        <div className="container hero__grid">
          <div>
            <span className="kicker">{t('hero.kicker')}</span>
            <h1 className="hero__title">{t('hero.title')}</h1>
            <p className="hero__lead">{t('hero.lead')}</p>
            <div className="hero__cta">
              <a className="btn btn-primary" href="#downloads">
                {t('hero.primaryCta')}
              </a>
              <a className="btn btn-ghost" href="https://github.com/vicrodh/qbz" target="_blank" rel="noreferrer">
                {t('hero.secondaryCta')}
              </a>
            </div>
            <div className="hero__stats">
              {stats.map((stat) => (
                <div key={stat.label} className="stat">
                  <img className="stat__icon icon-mono" src={stat.icon} alt="" />
                  <div className="stat__label">{stat.label}</div>
                </div>
              ))}
            </div>
          </div>
          <div className="hero__image">
            <img src="/assets/screenshots/qbz-home.png" alt="QBZ home view" />
          </div>
        </div>
      </section>

      <section className="section section--muted">
        <div className="container">
          <h2 className="section__title">{t('why.title')}</h2>
          <p className="section__subtitle">{t('why.lead')}</p>
          <ul className="list">
            {(t('why.bullets', { returnObjects: true }) as string[]).map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
          <p className="section__subtitle" style={{ marginTop: 24 }}>
            {t('why.note')}
          </p>
          <div className="logo-row">
            <img className="logo-muted" src="/assets/icons/qobuz-logo.svg" alt="Qobuz" />
          </div>
        </div>
      </section>

      <section className="section">
        <div className="container">
          <h2 className="section__title">{t('goals.title')}</h2>
          <p className="section__subtitle">{t('goals.lead')}</p>
          <div className="feature-grid" style={{ marginTop: 32 }}>
            {goals.map((goal) => (
              <div key={goal.title} className="feature-card">
                <div className="feature-card__title">{goal.title}</div>
                <div className="feature-card__text">{goal.text}</div>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section className="section section--muted">
        <div className="container">
          <h2 className="section__title">{t('capabilities.title')}</h2>
          <p className="section__subtitle">{t('capabilities.lead')}</p>
          <div className="feature-grid" style={{ marginTop: 32 }}>
            {capabilityCards.map((card) => (
              <div key={card.key} className="feature-card">
                <img className="icon-mono" src={capabilityIcons[card.key]} alt="" />
                <div className="feature-card__title">{card.title}</div>
                <ul className="list">
                  {card.bullets.map((bullet) => (
                    <li key={bullet}>{bullet}</li>
                  ))}
                </ul>
                {card.key === 'playlists' && (
                  <div className="logo-row">
                    <img className="logo-muted" src="/assets/icons/spotify-logo.svg" alt="Spotify" />
                    <img className="logo-muted" src="/assets/icons/apple-music-logo.svg" alt="Apple Music" />
                    <img className="logo-muted" src="/assets/icons/tidal-tidal.svg" alt="Tidal" />
                    <img className="logo-muted" src="/assets/icons/deezer-logo.svg" alt="Deezer" />
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      </section>

      <section className="section">
        <div className="container">
          <h2 className="section__title">{t('screenshots.title')}</h2>
          <p className="section__subtitle">{t('screenshots.lead')}</p>
          <div className="screenshot-grid" style={{ marginTop: 32 }}>
            {screenshots.map((shot, index) => (
              <div key={shot.title} className="screenshot">
                <img
                  src={
                    index === 0
                      ? '/assets/screenshots/qbz-playlist-view.png'
                      : index === 1
                        ? '/assets/screenshots/qbz-fullpage.png'
                        : '/assets/screenshots/qbz-locallibrary.png'
                  }
                  alt={shot.title}
                />
                <div className="screenshot__caption">
                  <div className="screenshot__title">{shot.title}</div>
                  <div className="screenshot__text">{shot.text}</div>
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

      <DownloadSection />

      <section className="section section--muted">
        <div className="container">
          <h2 className="section__title">{t('audience.title')}</h2>
          <p className="section__subtitle">{t('audience.lead')}</p>
          <ul className="list">
            {audienceItems.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
          <p className="section__subtitle" style={{ marginTop: 24 }}>
            {t('audience.notFor')}
          </p>
        </div>
      </section>

      <section className="section">
        <div className="container">
          <h2 className="section__title">{t('openSource.title')}</h2>
          <p className="section__subtitle">{t('openSource.lead')}</p>
          <ul className="list">
            {openSourceItems.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
          <div className="card" style={{ marginTop: 32 }}>
            <div className="download-meta">
              <div className="download-meta__name">{t('apis.title')}</div>
              <div className="download-meta__file">{t('apis.lead')}</div>
            </div>
            <details className="details">
              <summary>{t('apis.summary')}</summary>
              <ul className="list">
                {apiItems.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
            </details>
          </div>
        </div>
      </section>

      <section className="section section--muted">
        <div className="container">
          <h2 className="section__title">{t('linuxFirst.title')}</h2>
          <p className="section__subtitle">{t('linuxFirst.lead')}</p>
          <div className="logo-row" style={{ marginTop: 18 }}>
            <img className="icon-mono" src="/assets/icons/Tux.svg" alt="Linux" />
          </div>
          <a className="btn btn-ghost" href={buildPath(language, 'licenses')} style={{ marginTop: 24 }}>
            {t('nav.licenses')}
          </a>
        </div>
      </section>
    </>
  )
}
