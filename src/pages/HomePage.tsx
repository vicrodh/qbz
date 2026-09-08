import { Suspense, lazy } from 'react'
import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { buildPath } from '../lib/routes'
import { Capture } from '../components/Capture'

// Below the fold and network-bound: lazy so the hero paints first.
const DownloadSection = lazy(() => import('../components/DownloadSection').then((m) => ({ default: m.DownloadSection })))

type Named = { name: string; note: string }
type Titled = { title: string; text: string }
type FeatureItem = Titled & { key: string }
type SpecRow = { key: string; val: string }
type ScreenItem = Titled & { key: string }
type ManifestoItem = Titled & { tag: string }
type RoadmapItem = Titled & { state: string }
type LadderRow = { rate: string; tag: string; web: 'native' | 'resampled' | 'capped' | 'unsupported' }

// Existing captures, by gallery key. Missing keys render a labelled placeholder.
const SCREEN_CAPTURES: Record<string, { base: string } | undefined> = {
  home: { base: 'qbz-discover-foryou' },
  immersive: { base: 'qbz-immersive-spectrum' },
  explorer: { base: 'qbz-library-explorer' },
  queue: { base: 'qbz-queue' },
  playlists: { base: 'qbz-playlist-manager' },
  artist: { base: 'qbz-artist-view' },
}

const dot = (text: string) => (/[.!?]$/.test(text) ? text : `${text}.`)

function SectionLoader() {
  const { t } = useTranslation()
  return (
    <div className="band" style={{ minHeight: 320 }}>
      <div className="container download-state">{t('downloads.loading')}</div>
    </div>
  )
}

export function HomePage() {
  const { t } = useTranslation()
  const { language } = useApp()

  const signalInput = t('signal.input.items', { returnObjects: true }) as Named[]
  const signalEngine = t('signal.engine.items', { returnObjects: true }) as Named[]
  const signalOutput = t('signal.output.items', { returnObjects: true }) as Named[]
  const ladderRows = t('bitperfect.ladder.rows', { returnObjects: true }) as LadderRow[]
  const facts = t('bitperfect.facts', { returnObjects: true }) as Titled[]
  const libraryItems = t('library.items', { returnObjects: true }) as Titled[]
  const features = t('features.items', { returnObjects: true }) as FeatureItem[]
  const specRows = t('spec.rows', { returnObjects: true }) as SpecRow[]
  const screens = t('screens.items', { returnObjects: true }) as ScreenItem[]
  const manifesto = t('manifesto.items', { returnObjects: true }) as ManifestoItem[]
  const roadmap = t('roadmap.items', { returnObjects: true }) as RoadmapItem[]

  return (
    <>
      {/* ── Hero ─────────────────────────────────────────────── */}
      <section className="hero" aria-labelledby="hero-title">
        <div className="container hero__inner">
          <h1 id="hero-title" className="hero__title">
            {t('hero.title')} <span className="hero__accent">{t('hero.titleAccent')}</span>
          </h1>
          <p className="hero__lead">{t('hero.lead')}</p>
          <div className="hero__cta">
            <a className="btn btn-primary" href="#downloads">{t('hero.primaryCta')}</a>
            <a className="btn btn-ghost" href="#download-macos">{t('hero.secondaryCta')}</a>
          </div>
          <p className="hero__platforms">{t('hero.platforms')}</p>
        </div>
        <div className="container hero__shot">
          <Capture base="qbz-home" alt={t('hero.frameAlt')} priority sizes="(max-width: 900px) 100vw, 1180px" />
        </div>
      </section>

      {/* ── Sources ─────────────────────────────────────────── */}
      <section className="band band--muted" aria-labelledby="sources-title">
        <div className="container split">
          <div>
            <h2 id="sources-title" className="band__title">{t('sources.title')}</h2>
            <p className="band__lead">{t('sources.lead')}</p>
            <div className="cols">
              <div>
                <h3 className="cols__title">{t('signal.input.name')}</h3>
                <ul className="plain">
                  {signalInput.map((item) => (
                    <li key={item.name}><b>{item.name}</b> <span>{item.note}</span></li>
                  ))}
                </ul>
              </div>
              <div>
                <h3 className="cols__title">{t('signal.output.name')}</h3>
                <ul className="plain">
                  {signalOutput.map((item) => (
                    <li key={item.name}><b>{item.name}</b> <span>{item.note}</span></li>
                  ))}
                </ul>
              </div>
            </div>
          </div>
          <Capture base="qbz-playlist-view" alt={t('sources.captureAlt')} />
        </div>
      </section>

      {/* ── Bit-perfect ─────────────────────────────────────── */}
      <section className="band" id="features" aria-labelledby="bitperfect-title">
        <div className="container split split--wide">
          <div>
            <h2 id="bitperfect-title" className="band__title">{t('bitperfect.title')}</h2>
            <p className="band__lead">{t('bitperfect.lead')}</p>
            <div className="facts">
              {facts.map((fact) => (
                <p className="fact" key={fact.title}>
                  <b>{dot(fact.title)}</b> {fact.text}
                </p>
              ))}
            </div>
            <ul className="plain" style={{ marginTop: 24 }}>
              {signalEngine.map((item) => (
                <li key={item.name}><b>{item.name}</b> <span>{item.note}</span></li>
              ))}
            </ul>
          </div>
          <div className="ladder" role="table" aria-label={t('bitperfect.eyebrow')}>
            <div className="ladder__head" role="row">
              <span role="columnheader">{t('bitperfect.ladder.rate')}</span>
              <span role="columnheader">{t('bitperfect.ladder.qbz')}</span>
              <span role="columnheader">{t('bitperfect.ladder.web')}</span>
            </div>
            {ladderRows.map((row) => {
              const capped = row.web === 'capped' || row.web === 'unsupported'
              return (
                <div className="ladder__row" role="row" key={row.rate}>
                  <span className="ladder__rate" role="cell">
                    {row.rate}
                    {row.tag && <small>{row.tag}</small>}
                  </span>
                  <span className="ladder__cell is-ok" role="cell">
                    <span className="ladder__bar ladder__bar--qbz" />
                    {t('bitperfect.ladder.native')}
                  </span>
                  <span className={`ladder__cell ${capped ? 'is-capped' : ''}`} role="cell">
                    <span className={`ladder__bar ladder__bar--web ${capped ? 'is-capped' : ''}`} />
                    {t(`bitperfect.ladder.${row.web}`)}
                  </span>
                </div>
              )
            })}
            <p className="ladder__foot">{t('bitperfect.ladder.foot')}</p>
          </div>
        </div>
      </section>

      {/* ── Local library ───────────────────────────────────── */}
      <section className="band band--muted" aria-labelledby="library-title">
        <div className="container split split--flip">
          <div>
            <h2 id="library-title" className="band__title">{t('library.title')}</h2>
            <p className="band__lead">{t('library.lead')}</p>
            <div className="facts">
              {libraryItems.map((item) => (
                <p className="fact" key={item.title}>
                  <b>{dot(item.title)}</b> {item.text}
                </p>
              ))}
            </div>
          </div>
          <Capture base="qbz-locallibrary-artists" alt={t('library.captures.artists')} />
        </div>
      </section>

      {/* ── Kiosk + qbzd ────────────────────────────────────── */}
      <section className="band" aria-labelledby="kiosk-title">
        <div className="container split">
          <div>
            <h2 id="kiosk-title" className="band__title">{t('kiosk.title')}</h2>
            <p className="band__lead">{t('kiosk.lead')}</p>
            <div className="facts">
              <p className="fact"><b>{t('kiosk.kioskTitle')}.</b> {t('kiosk.kioskText')}</p>
              <p className="fact"><b>{t('kiosk.daemonTitle')}.</b> {t('kiosk.daemonText')}</p>
            </div>
            <a className="btn btn-ghost" href="https://github.com/vicrodh/qbz/wiki/Headless-Daemon" target="_blank" rel="noreferrer" style={{ marginTop: 24 }}>
              {t('daemon.cta')}
            </a>
          </div>
          <div className="stack">
            <img
              className="shot"
              src="/assets/images/kiosk.webp"
              srcSet="/assets/images/kiosk-sm.webp 640w, /assets/images/kiosk.webp 1280w"
              sizes="(max-width: 900px) 100vw, 580px"
              alt={t('kiosk.photoAlt')}
              width={1280}
              height={720}
              loading="lazy"
            />
            <img
              className="shot"
              src="/assets/images/qbzd.webp"
              srcSet="/assets/images/qbzd-sm.webp 640w, /assets/images/qbzd.webp 1280w"
              sizes="(max-width: 900px) 100vw, 580px"
              alt={t('kiosk.qbzdAlt')}
              width={1280}
              height={814}
              loading="lazy"
            />
          </div>
        </div>
      </section>

      {/* ── Features ────────────────────────────────────────── */}
      <section className="band band--muted" aria-labelledby="features-title">
        <div className="container">
          <div className="band__head">
            <h2 id="features-title" className="band__title">{t('features.title')}</h2>
            <p className="band__lead">{t('features.lead')}</p>
          </div>
          <div className="features">
            {features.map((feature) => (
              <div className="feature" key={feature.key}>
                <h3 className="feature__title">{feature.title}</h3>
                <p className="feature__text">{feature.text}</p>
                {feature.key === 'playlists' && (
                  <div className="logo-row">
                    <img src="/assets/icons/spotify-logo.svg" alt="Spotify" loading="lazy" />
                    <img src="/assets/icons/apple-music-logo.svg" alt="Apple Music" loading="lazy" />
                    <img className="invert-white" src="/assets/icons/tidal-tidal.svg" alt="Tidal" loading="lazy" />
                    <img src="/assets/icons/deezer-logo.svg" alt="Deezer" loading="lazy" />
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* ── Specifications ──────────────────────────────────── */}
      <section className="band" aria-labelledby="spec-title">
        <div className="container">
          <div className="band__head">
            <h2 id="spec-title" className="band__title">{t('spec.title')}</h2>
          </div>
          <dl className="spec">
            {specRows.map((row) => (
              <div className="spec__row" key={row.key}>
                <dt className="spec__key">{row.key}</dt>
                <dd className="spec__val">{row.val}</dd>
              </div>
            ))}
          </dl>
        </div>
      </section>

      {/* ── Screens ─────────────────────────────────────────── */}
      <section className="band band--muted" aria-labelledby="screens-title">
        <div className="container">
          <div className="band__head">
            <h2 id="screens-title" className="band__title">{t('screens.title')}</h2>
            <p className="band__lead">{t('screens.lead')}</p>
          </div>
          <div className="screens">
            {screens.filter((screen) => SCREEN_CAPTURES[screen.key]).map((screen) => {
              const capture = SCREEN_CAPTURES[screen.key]
              return (
                <figure className="screen" key={screen.key}>
                  <Capture base={capture?.base} alt={`QBZ: ${screen.title}`} pending={screen.title} />
                  <figcaption className="screen__caption">
                    <b>{screen.title}</b> {screen.text}
                  </figcaption>
                </figure>
              )
            })}
          </div>
        </div>
      </section>

      {/* ── Downloads ───────────────────────────────────────── */}
      <Suspense fallback={<SectionLoader />}>
        <DownloadSection />
      </Suspense>

      {/* ── Principles ──────────────────────────────────────── */}
      <section className="band band--muted" aria-labelledby="manifesto-title">
        <div className="container">
          <div className="band__head">
            <h2 id="manifesto-title" className="band__title">{t('manifesto.title')}</h2>
          </div>
          <div className="cols cols--3">
            {manifesto.map((item) => (
              <div key={item.title}>
                <h3 className="cols__title">{item.title}</h3>
                <p className="cols__text">{item.text}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* ── Roadmap + Linux first ───────────────────────────── */}
      <section className="band" aria-labelledby="roadmap-title">
        <div className="container split split--top">
          <div>
            <h2 id="roadmap-title" className="band__title">{t('roadmap.title')}</h2>
            <div className="facts">
              {roadmap.map((item) => (
                <p className="fact" key={item.title}>
                  <b>{dot(item.title)}</b> {item.text}
                </p>
              ))}
            </div>
          </div>
          <div>
            <div className="linux-first__logos">
              <img src="/assets/icons/Tux.svg" alt="Tux, the Linux mascot" loading="lazy" />
              <img src="/assets/icons/mit-license.svg" alt="MIT License" loading="lazy" style={{ filter: 'brightness(0) invert(0.92)' }} />
            </div>
            <h2 className="band__title">{t('linuxFirst.title')}</h2>
            <p className="band__lead">{t('linuxFirst.lead')}</p>
            <div className="hero__cta" style={{ justifyContent: 'flex-start', marginTop: 20 }}>
              <a className="btn btn-ghost" href={buildPath(language, 'licenses')}>{t('linuxFirst.licenses')}</a>
              <a className="btn btn-ghost" href="https://github.com/vicrodh/qbz#contributors" target="_blank" rel="noreferrer">{t('linuxFirst.contributors')}</a>
            </div>
          </div>
        </div>
      </section>
    </>
  )
}
