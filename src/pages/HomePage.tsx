import { useTranslation } from 'react-i18next'
import { useEffect, useState, useRef } from 'react'
import { DownloadSection } from '../components/DownloadSection'
import { useApp } from '../lib/appContext'
import { buildPath } from '../lib/routes'

const CAPABILITY_KEYS = ['audio', 'library', 'playlists', 'desktop', 'casting', 'radio', 'offline', 'metadata'] as const

type CapabilityKey = (typeof CAPABILITY_KEYS)[number]

type CapabilityCard = {
  key: CapabilityKey
  title: string
  bullets: string[]
}

function CapabilitiesCarousel({
  capabilityCards,
  capabilityIcons,
}: {
  capabilityCards: CapabilityCard[]
  capabilityIcons: Record<CapabilityKey, string>
}) {
  const { t } = useTranslation()
  const [currentIndex, setCurrentIndex] = useState(0)
  const [isPaused, setIsPaused] = useState(false)
  const trackRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (isPaused) return

    const interval = setInterval(() => {
      setCurrentIndex((prev) => (prev + 1) % capabilityCards.length)
    }, 5000)

    return () => clearInterval(interval)
  }, [isPaused, capabilityCards.length])

  const cardWidth = 296 // 280px + 16px gap
  const totalCards = capabilityCards.length

  // Create infinite effect by duplicating cards
  const extendedCards = [...capabilityCards, ...capabilityCards, ...capabilityCards]

  return (
    <section className="section section--muted">
      <div className="container">
        <h2 className="section__title">{t('capabilities.title')}</h2>
        <p className="section__subtitle">{t('capabilities.lead')}</p>
      </div>
      <div
        className="carousel-wrapper"
        style={{ marginTop: 32 }}
        onMouseEnter={() => setIsPaused(true)}
        onMouseLeave={() => setIsPaused(false)}
      >
        <div
          ref={trackRef}
          className="carousel-track carousel-track--infinite"
          style={{
            transform: `translateX(calc(-${(currentIndex + totalCards) * cardWidth}px + 50vw - ${cardWidth / 2}px))`,
          }}
        >
          {extendedCards.map((card, idx) => {
            const realIndex = idx % totalCards
            const isActive = realIndex === currentIndex
            return (
              <div
                key={`${card.key}-${idx}`}
                className={`capability-card ${isActive ? 'capability-card--active' : ''}`}
                onClick={() => setCurrentIndex(realIndex)}
              >
                <img className="icon-mono" src={capabilityIcons[card.key]} alt={card.title} />
                <div className="capability-card__title">{card.title}</div>
                <ul className="list list--compact">
                  {card.bullets.map((bullet) => (
                    <li key={bullet}>{bullet}</li>
                  ))}
                </ul>
                {card.key === 'playlists' && (
                  <div className="logo-row logo-row--centered">
                    <img src="/assets/icons/spotify-logo.svg" alt="Spotify" />
                    <img src="/assets/icons/apple-music-logo.svg" alt="Apple Music" />
                    <img className="invert-white" src="/assets/icons/tidal-tidal.svg" alt="Tidal" />
                    <img src="/assets/icons/deezer-logo.svg" alt="Deezer" />
                  </div>
                )}
              </div>
            )
          })}
        </div>
        <div className="carousel-nav">
          <button
            className="carousel-arrow carousel-arrow--left"
            onClick={() => setCurrentIndex((prev) => (prev - 1 + totalCards) % totalCards)}
            aria-label="Previous"
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <polyline points="15 18 9 12 15 6" />
            </svg>
          </button>
          <div className="carousel-dots">
            {capabilityCards.map((card, index) => (
              <button
                key={card.key}
                className={`carousel-dot ${index === currentIndex ? 'carousel-dot--active' : ''}`}
                onClick={() => setCurrentIndex(index)}
                aria-label={`Go to ${card.title}`}
              />
            ))}
          </div>
          <button
            className="carousel-arrow carousel-arrow--right"
            onClick={() => setCurrentIndex((prev) => (prev + 1) % totalCards)}
            aria-label="Next"
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <polyline points="9 18 15 12 9 6" />
            </svg>
          </button>
        </div>
      </div>
    </section>
  )
}

export function HomePage() {
  const { t } = useTranslation()
  const { language } = useApp()

  const stats = [
    { icon: '/assets/icons/hi-res.svg', label: t('hero.stats.audio'), colored: true, large: false },
    { icon: '/assets/icons/dac.svg', label: t('hero.stats.dac'), colored: false, large: true },
    { icon: '/assets/icons/cast-audio.svg', label: t('hero.stats.casting'), colored: false, large: false },
  ]

  const capabilityIcons: Record<CapabilityKey, string> = {
    audio: '/assets/icons/audio-spec.svg',
    library: '/assets/icons/nas.svg',
    playlists: '/assets/icons/playlist.svg',
    desktop: '/assets/icons/linux-desktop.svg',
    casting: '/assets/icons/cast-audio.svg',
    radio: '/assets/icons/radio-signal.svg',
    offline: '/assets/icons/offline-small.svg',
    metadata: '/assets/icons/cd-music.svg',
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
            <h1 className="hero__title">{t('hero.heading')}</h1>
            <p className="hero__subtitle">{t('hero.title')}</p>
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
                  <img
                    className={`stat__icon ${stat.colored ? '' : 'icon-mono'} ${stat.large ? 'stat__icon--large' : ''}`}
                    src={stat.icon}
                    alt={stat.label}
                  />
                  <div className="stat__label">{stat.label}</div>
                </div>
              ))}
            </div>
          </div>
          <div className="hero__image">
            <picture>
              <source
                type="image/avif"
                srcSet="/assets/screenshots/qbz-home-sm.avif 640w, /assets/screenshots/qbz-home.avif 1280w"
                sizes="(max-width: 768px) 640px, 1280px"
              />
              <source
                type="image/webp"
                srcSet="/assets/screenshots/qbz-home-sm.webp 640w, /assets/screenshots/qbz-home.webp 1280w"
                sizes="(max-width: 768px) 640px, 1280px"
              />
              <img
                src="/assets/screenshots/qbz-home.webp"
                alt="QBZ application interface showing home view with queue and playback controls"
                title="QBZ home view"
                fetchPriority="high"
              />
            </picture>
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

      <CapabilitiesCarousel capabilityCards={capabilityCards} capabilityIcons={capabilityIcons} />

      <section className="section">
        <div className="container">
          <h2 className="section__title">{t('screenshots.title')}</h2>
          <p className="section__subtitle">{t('screenshots.lead')}</p>
          <div className="screenshot-grid" style={{ marginTop: 32 }}>
            {screenshots.map((shot, index) => {
              const imgBase = index === 0
                ? 'qbz-playlist-view'
                : index === 1
                  ? 'qbz-fullpage'
                  : 'qbz-locallibrary'
              return (
                <div key={shot.title} className="screenshot">
                  <picture>
                    <source
                      type="image/avif"
                      srcSet={`/assets/screenshots/${imgBase}-sm.avif 640w, /assets/screenshots/${imgBase}.avif 1280w`}
                      sizes="(max-width: 768px) 640px, 1280px"
                    />
                    <source
                      type="image/webp"
                      srcSet={`/assets/screenshots/${imgBase}-sm.webp 640w, /assets/screenshots/${imgBase}.webp 1280w`}
                      sizes="(max-width: 768px) 640px, 1280px"
                    />
                    <img
                      src={`/assets/screenshots/${imgBase}.webp`}
                      alt={`QBZ screenshot: ${shot.title}`}
                      title={shot.title}
                      loading="lazy"
                    />
                  </picture>
                  <div className="screenshot__caption">
                    <div className="screenshot__title">{shot.title}</div>
                    <div className="screenshot__text">{shot.text}</div>
                  </div>
                </div>
              )
            })}
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
          <div style={{ display: 'flex', alignItems: 'flex-start', gap: 32 }}>
            <div style={{ display: 'flex', gap: 16, flexShrink: 0 }}>
              <img src="/assets/icons/Tux.svg" alt="Linux Tux mascot - QBZ is Linux first" title="Linux first" style={{ width: 128, height: 'auto' }} />
              <img src="/assets/icons/open-source-color.svg" alt="Open Source" title="Open Source" style={{ height: 128, width: 'auto' }} />
            </div>
            <div>
              <h2 className="section__title">{t('linuxFirst.title')}</h2>
              <p className="section__subtitle" style={{ maxWidth: 'none' }}>{t('linuxFirst.lead')}</p>
              <a className="btn btn-ghost" href={buildPath(language, 'licenses')} style={{ marginTop: 24 }}>
                {t('nav.licenses')}
              </a>
            </div>
          </div>
        </div>
      </section>
    </>
  )
}
