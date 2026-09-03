import { Suspense, lazy } from 'react'
import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { buildPath } from '../lib/routes'
import { Readout } from '../components/Readout'
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

const FEATURE_ICONS: Record<string, string> = {
  connect: '/assets/icons/network-playback.svg',
  casting: '/assets/icons/cast-audio.svg',
  immersive: '/assets/icons/audio-spec.svg',
  playlists: '/assets/icons/playlist.svg',
  desktop: '/assets/icons/linux-desktop.svg',
  metadata: '/assets/icons/cd-music.svg',
  discovery: '/assets/icons/radio-signal.svg',
  offline: '/assets/icons/offline-small.svg',
  interface: '/assets/icons/home-gear.svg',
  blocklist: '/assets/icons/blind-eye.svg',
}

// Existing captures, by gallery key. Missing keys render a labelled placeholder.
const SCREEN_CAPTURES: Record<string, { base: string; height?: number } | undefined> = {
  home: { base: 'qbz-discover-foryou' },
  immersive: { base: 'qbz-immersive-spectrum' },
  explorer: undefined,
  queue: undefined,
  playlists: { base: 'qbz-playlist-manager' },
  artist: { base: 'qbz-artist-view' },
}

const Arrow = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M4 12h15" />
    <path d="M13 6l6 6-6 6" />
  </svg>
)

function SectionLoader() {
  const { t } = useTranslation()
  return (
    <div className="section" style={{ minHeight: 320 }}>
      <div className="container download-state">{t('downloads.loading')}</div>
    </div>
  )
}

export function HomePage() {
  const { t } = useTranslation()
  const { language } = useApp()

  const sources = t('sources.items', { returnObjects: true }) as string[]
  const signalInput = t('signal.input.items', { returnObjects: true }) as Named[]
  const signalEngine = t('signal.engine.items', { returnObjects: true }) as Named[]
  const signalOutput = t('signal.output.items', { returnObjects: true }) as Named[]
  const ladderRows = t('bitperfect.ladder.rows', { returnObjects: true }) as LadderRow[]
  const facts = t('bitperfect.facts', { returnObjects: true }) as Titled[]
  const libraryItems = t('library.items', { returnObjects: true }) as Titled[]
  const features = t('features.items', { returnObjects: true }) as FeatureItem[]
  const specRows = t('spec.rows', { returnObjects: true }) as SpecRow[]
  const screens = t('screens.items', { returnObjects: true }) as ScreenItem[]
  const daemonItems = t('daemon.items', { returnObjects: true }) as string[]
  const manifesto = t('manifesto.items', { returnObjects: true }) as ManifestoItem[]
  const roadmap = t('roadmap.items', { returnObjects: true }) as RoadmapItem[]

  const webLabel = (web: LadderRow['web']) => t(`bitperfect.ladder.${web}`)

  return (
    <>
      {/* ── Hero ─────────────────────────────────────────────── */}
      <section className="hero" aria-labelledby="hero-title">
        <div className="container hero__grid">
          <div className="hero__copy">
            <span className="eyebrow">{t('hero.eyebrow')}</span>
            <h1 id="hero-title" className="hero__title">{t('hero.title')}</h1>
            <p className="hero__lead">{t('hero.lead')}</p>
            <div className="hero__cta">
              <a className="btn btn-primary" href="#downloads">
                <img className="btn__icon" src="/assets/icons/Tux.svg" alt="" width={18} height={18} />
                {t('hero.primaryCta')}
              </a>
              <a className="btn btn-ghost" href="#download-macos">
                {t('hero.secondaryCta')}
              </a>
              <a className="btn btn-ghost" href="https://github.com/vicrodh/qbz" target="_blank" rel="noreferrer">
                {t('hero.github')}
              </a>
            </div>
            <div className="hero__meta">
              <span>{t('hero.meta.telemetry')}</span>
              <span>{t('hero.meta.keys')}</span>
              <a href={buildPath(language, 'qobuz-linux')}>{t('hero.meta.qobuzLinux')}</a>
            </div>
          </div>

          <div>
            <Capture base="qbz-home" alt={t('hero.frameAlt')} title={t('hero.frameTitle')} priority>
              <Readout />
            </Capture>
          </div>
        </div>

        <div className="sources-rail">
          <div className="container sources-rail__inner">
            <span className="sources-rail__label">{t('sources.label')}</span>
            <ul className="sources-rail__list">
              {sources.map((source) => (
                <li key={source}>{source}</li>
              ))}
            </ul>
          </div>
        </div>
      </section>

      {/* ── Signal path ─────────────────────────────────────── */}
      <section className="section section--flush" id="features" aria-labelledby="signal-title" style={{ paddingTop: 'var(--section-pad)' }}>
        <div className="container">
          <div className="section__head">
            <span className="eyebrow">{t('signal.eyebrow')}</span>
            <h2 id="signal-title" className="section__title">{t('signal.title')}</h2>
            <p className="section__subtitle">{t('signal.lead')}</p>
          </div>
          <div className="signal">
            <div className="signal__stage">
              <h3 className="signal__name">{t('signal.input.name')}</h3>
              <p className="signal__desc">{t('signal.input.desc')}</p>
              <ul className="signal__items">
                {signalInput.map((item) => (
                  <li key={item.name}><b>{item.name}</b><span>{item.note}</span></li>
                ))}
              </ul>
            </div>
            <div className="signal__arrow"><Arrow /></div>
            <div className="signal__stage signal__stage--engine">
              <h3 className="signal__name">{t('signal.engine.name')}</h3>
              <p className="signal__desc">{t('signal.engine.desc')}</p>
              <ul className="signal__items">
                {signalEngine.map((item) => (
                  <li key={item.name}><b>{item.name}</b><span>{item.note}</span></li>
                ))}
              </ul>
            </div>
            <div className="signal__arrow"><Arrow /></div>
            <div className="signal__stage">
              <h3 className="signal__name">{t('signal.output.name')}</h3>
              <p className="signal__desc">{t('signal.output.desc')}</p>
              <ul className="signal__items">
                {signalOutput.map((item) => (
                  <li key={item.name}><b>{item.name}</b><span>{item.note}</span></li>
                ))}
              </ul>
            </div>
          </div>
        </div>
      </section>

      {/* ── Bit-perfect ─────────────────────────────────────── */}
      <section className="section section--muted" aria-labelledby="bitperfect-title">
        <div className="container">
          <div className="section__head">
            <span className="eyebrow eyebrow--amber">{t('bitperfect.eyebrow')}</span>
            <h2 id="bitperfect-title" className="section__title">{t('bitperfect.title')}</h2>
            <p className="section__subtitle">{t('bitperfect.lead')}</p>
          </div>
          <div className="bitperfect">
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
                      {webLabel(row.web)}
                    </span>
                  </div>
                )
              })}
              <p className="ladder__foot">{t('bitperfect.ladder.foot')}</p>
            </div>
            <div className="facts">
              {facts.map((fact) => (
                <div className="fact" key={fact.title}>
                  <h3 className="fact__title">{fact.title}</h3>
                  <p className="fact__text">{fact.text}</p>
                </div>
              ))}
            </div>
          </div>
        </div>
      </section>

      {/* ── Local library ───────────────────────────────────── */}
      <section className="section" aria-labelledby="library-title">
        <div className="container split">
          <div>
            <span className="eyebrow">{t('library.eyebrow')}</span>
            <h2 id="library-title" className="section__title">{t('library.title')}</h2>
            <p className="section__subtitle">{t('library.lead')}</p>
            <div className="facts" style={{ marginTop: 28 }}>
              {libraryItems.map((item) => (
                <div className="fact" key={item.title}>
                  <h3 className="fact__title">{item.title}</h3>
                  <p className="fact__text">{item.text}</p>
                </div>
              ))}
            </div>
          </div>
          <div className="stack">
            <Capture alt={t('library.captures.explorer')} title={t('library.captures.explorer')} pending={t('library.captures.explorer')} />
            <Capture base="qbz-locallibrary-artists" alt={t('library.captures.artists')} title={t('library.captures.artists')} />
          </div>
        </div>
      </section>

      {/* ── Features ────────────────────────────────────────── */}
      <section className="section section--muted" aria-labelledby="features-title">
        <div className="container">
          <div className="section__head">
            <span className="eyebrow">{t('features.eyebrow')}</span>
            <h2 id="features-title" className="section__title">{t('features.title')}</h2>
            <p className="section__subtitle">{t('features.lead')}</p>
          </div>
          <div className="feature-grid">
            {features.map((feature) => (
              <article className="feature-card" key={feature.key}>
                <img className="feature-card__icon icon-mono" src={FEATURE_ICONS[feature.key]} alt="" width={26} height={26} loading="lazy" />
                <h3 className="feature-card__title">{feature.title}</h3>
                <p className="feature-card__text">{feature.text}</p>
                {feature.key === 'playlists' && (
                  <div className="logo-row">
                    <img src="/assets/icons/spotify-logo.svg" alt="Spotify" loading="lazy" />
                    <img src="/assets/icons/apple-music-logo.svg" alt="Apple Music" loading="lazy" />
                    <img className="invert-white" src="/assets/icons/tidal-tidal.svg" alt="Tidal" loading="lazy" />
                    <img src="/assets/icons/deezer-logo.svg" alt="Deezer" loading="lazy" />
                  </div>
                )}
              </article>
            ))}
          </div>
        </div>
      </section>

      {/* ── Spec sheet ──────────────────────────────────────── */}
      <section className="section" aria-labelledby="spec-title">
        <div className="container">
          <div className="section__head" style={{ marginBottom: 28 }}>
            <span className="eyebrow">{t('spec.eyebrow')}</span>
            <h2 id="spec-title" className="section__title">{t('spec.title')}</h2>
          </div>
          <dl className="spec" style={{ margin: 0 }}>
            {specRows.map((row) => (
              <div className="spec__row" key={row.key}>
                <dt className="spec__key">{row.key}</dt>
                <dd className="spec__val" style={{ margin: 0 }}>{row.val}</dd>
              </div>
            ))}
          </dl>
        </div>
      </section>

      {/* ── Screens ─────────────────────────────────────────── */}
      <section className="section section--muted" aria-labelledby="screens-title">
        <div className="container">
          <div className="section__head">
            <span className="eyebrow">{t('screens.eyebrow')}</span>
            <h2 id="screens-title" className="section__title">{t('screens.title')}</h2>
            <p className="section__subtitle">{t('screens.lead')}</p>
          </div>
          <div className="screens">
            {screens.map((screen) => {
              const capture = SCREEN_CAPTURES[screen.key]
              return (
                <div key={screen.key}>
                  <Capture
                    base={capture?.base}
                    alt={`QBZ: ${screen.title}`}
                    title={screen.title}
                    pending={screen.title}
                    height={capture?.height}
                  />
                  <div className="screen__caption">
                    <span className="screen__title">{screen.title}</span>
                    <span className="screen__text">{screen.text}</span>
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      </section>

      {/* ── qbzd ────────────────────────────────────────────── */}
      <section className="section" aria-labelledby="daemon-title">
        <div className="container daemon">
          <div>
            <span className="eyebrow">{t('daemon.eyebrow')}</span>
            <h2 id="daemon-title" className="section__title">{t('daemon.title')}</h2>
            <p className="section__subtitle">{t('daemon.lead')}</p>
            <ul className="list" style={{ marginTop: 24 }}>
              {daemonItems.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
            <a className="btn btn-ghost" href="https://github.com/vicrodh/qbz/wiki/Headless-Daemon" target="_blank" rel="noreferrer" style={{ marginTop: 28 }}>
              {t('daemon.cta')}
            </a>
          </div>
          <div className="terminal-block" aria-label="qbzd">
            <div className="panel-frame__bar" aria-hidden="true">
              <span className="panel-frame__dot" />
              <span className="panel-frame__dot" />
              <span className="panel-frame__dot" />
              <span className="panel-frame__title">pi@living-room</span>
            </div>
            <pre>
              <span className="c">{t('daemon.terminal.c1')}</span>{'\n'}
              <i>$</i> <b>qbzd</b> --help{'\n'}
              {'\n'}
              <span className="c">{t('daemon.terminal.c2')}</span>{'\n'}
              <i>$</i> <b>qbzd</b> settings set hooks.script ~/qbz-hook.sh{'\n'}
              {'\n'}
              <span className="c">{t('daemon.terminal.c3')}</span>{'\n'}
              <i>$</i> <b>qbzd</b> watch
            </pre>
          </div>
        </div>
      </section>

      {/* ── Downloads ───────────────────────────────────────── */}
      <Suspense fallback={<SectionLoader />}>
        <DownloadSection />
      </Suspense>

      {/* ── Principles ──────────────────────────────────────── */}
      <section className="section section--muted" aria-labelledby="manifesto-title">
        <div className="container">
          <div className="section__head">
            <span className="eyebrow">{t('manifesto.eyebrow')}</span>
            <h2 id="manifesto-title" className="section__title">{t('manifesto.title')}</h2>
          </div>
          <div className="manifesto">
            {manifesto.map((item) => (
              <div className="manifesto__item" key={item.title}>
                <span className="eyebrow eyebrow--amber">{item.tag}</span>
                <h3>{item.title}</h3>
                <p>{item.text}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* ── Roadmap ─────────────────────────────────────────── */}
      <section className="section" aria-labelledby="roadmap-title">
        <div className="container grid-2">
          <div>
            <span className="eyebrow">{t('roadmap.eyebrow')}</span>
            <h2 id="roadmap-title" className="section__title">{t('roadmap.title')}</h2>
          </div>
          <div className="roadmap">
            {roadmap.map((item) => (
              <div className="roadmap__item" key={item.title}>
                <span className="roadmap__state">{item.state}</span>
                <div>
                  <div className="roadmap__title">{item.title}</div>
                  <p className="roadmap__text">{item.text}</p>
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* ── Linux first ─────────────────────────────────────── */}
      <section className="section section--muted" aria-labelledby="linux-title">
        <div className="container linux-first">
          <div className="linux-first__logos">
            <img src="/assets/icons/Tux.svg" alt="Tux, the Linux mascot" loading="lazy" />
            <img src="/assets/icons/mit-license.svg" alt="MIT License" loading="lazy" style={{ filter: 'brightness(0) invert(0.92)' }} />
          </div>
          <div>
            <span className="eyebrow">{t('linuxFirst.eyebrow')}</span>
            <h2 id="linux-title" className="section__title">{t('linuxFirst.title')}</h2>
            <p className="section__subtitle" style={{ maxWidth: 'none' }}>{t('linuxFirst.lead')}</p>
            <div className="hero__cta" style={{ marginTop: 24 }}>
              <a className="btn btn-ghost" href={buildPath(language, 'licenses')}>{t('linuxFirst.licenses')}</a>
              <a className="btn btn-ghost" href="https://github.com/vicrodh/qbz#contributors" target="_blank" rel="noreferrer">{t('linuxFirst.contributors')}</a>
            </div>
          </div>
        </div>
      </section>
    </>
  )
}
