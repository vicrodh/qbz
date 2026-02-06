import { useTranslation } from 'react-i18next'
import { Suspense, lazy } from 'react'
import { useApp } from '../lib/appContext'
import { buildPath } from '../lib/routes'
import { CAPABILITY_KEYS, type CapabilityKey } from '../lib/capabilities'
import { CapabilitiesCarousel } from '../components/CapabilitiesCarousel'

// Lazy load heavy sections below fold
const DownloadSection = lazy(() => import('../components/DownloadSection').then(m => ({ default: m.DownloadSection })))
const ComingSoonSection = lazy(() => import('../components/ComingSoonSection').then(m => ({ default: m.ComingSoonSection })))

const SectionLoader = ({ muted = false }: { muted?: boolean }) => (
  <div className={`section ${muted ? 'section--muted' : ''}`} style={{ minHeight: 400, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
    <div className="container" style={{ textAlign: 'center', color: 'var(--text-tertiary)' }}>
      Loading...
    </div>
  </div>
)

export function HomePage() {
  const { t } = useTranslation()
  const { language } = useApp()

  const stats = [
    { icon: '/assets/icons/hi-res.svg', label: t('hero.stats.audio'), colored: true, large: false },
    { icon: '/assets/icons/dac.svg', label: t('hero.stats.dac'), colored: false, large: true },
    { icon: '/assets/icons/Rust_for_Linux_logo.svg', label: t('hero.stats.native'), colored: true, large: false, size: 30 },
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
    hideArtists: '/assets/icons/blind-eye.svg',
    songRecommendations: '/assets/icons/sparkles.svg',
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
            <p style={{ marginTop: 16, fontSize: '0.9rem', color: 'var(--text-tertiary)' }}>
              <a href={buildPath(language, 'qobuz-linux')} style={{ color: 'var(--accent)', textDecoration: 'underline' }}>{t('hero.qobuzLinuxLink')}</a>{t('hero.qobuzLinuxExplain')}
            </p>
            <div className="hero__stats">
              {stats.map((stat) => (
                <div key={stat.label} className="stat">
                  <img
                    className={`stat__icon ${stat.colored ? '' : 'icon-mono'} ${stat.large ? 'stat__icon--large' : ''}`}
                    src={stat.icon}
                    alt={stat.label}
                    style={stat.size ? { width: stat.size, height: stat.size } : undefined}
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
                srcSet="/assets/screenshots/qbz-home-xs.avif 400w, /assets/screenshots/qbz-home-sm.avif 640w, /assets/screenshots/qbz-home.avif 1280w"
                sizes="(max-width: 480px) 400px, (max-width: 768px) 640px, 1280px"
              />
              <source
                type="image/webp"
                srcSet="/assets/screenshots/qbz-home-xs.webp 400w, /assets/screenshots/qbz-home-sm.webp 640w, /assets/screenshots/qbz-home.webp 1280w"
                sizes="(max-width: 480px) 400px, (max-width: 768px) 640px, 1280px"
              />
              <img
                src="/assets/screenshots/qbz-home.webp"
                alt="QBZ application interface showing home view with queue and playback controls"
                title="QBZ home view"
                width={1280}
                height={800}
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
                  ? 'qbz-immersivecoverflow'
                  : 'qbz-locallibrary'
              return (
                <div key={shot.title} className="screenshot">
                  <picture>
                    <source
                      type="image/avif"
                      srcSet={`/assets/screenshots/${imgBase}-xs.avif 400w, /assets/screenshots/${imgBase}-sm.avif 640w, /assets/screenshots/${imgBase}.avif 1280w`}
                      sizes="(max-width: 480px) 400px, (max-width: 768px) 640px, 1280px"
                    />
                    <source
                      type="image/webp"
                      srcSet={`/assets/screenshots/${imgBase}-xs.webp 400w, /assets/screenshots/${imgBase}-sm.webp 640w, /assets/screenshots/${imgBase}.webp 1280w`}
                      sizes="(max-width: 480px) 400px, (max-width: 768px) 640px, 1280px"
                    />
                    <img
                      src={`/assets/screenshots/${imgBase}.webp`}
                      alt={`QBZ screenshot: ${shot.title}`}
                      title={shot.title}
                      width={1280}
                      height={800}
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

      <Suspense fallback={<SectionLoader />}>
        <DownloadSection />
      </Suspense>

      <Suspense fallback={<SectionLoader muted />}>
        <ComingSoonSection />
      </Suspense>

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
