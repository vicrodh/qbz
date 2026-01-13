import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { formatDate } from '../lib/format'

type Release = {
  id: number
  tag_name: string
  name: string
  published_at: string
  body: string
  html_url: string
}

const RELEASES_URL = 'https://api.github.com/repos/vicrodh/qbz/releases'

const extractHighlights = (body: string) => {
  const bullets = body
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.startsWith('- ') || line.startsWith('* '))
    .map((line) => line.replace(/^[-*]\s*/, ''))
    .filter(Boolean)
  if (bullets.length > 0) {
    return bullets.slice(0, 8)
  }
  const sentences = body
    .split('. ')
    .map((line) => line.trim())
    .filter(Boolean)
  return sentences.slice(0, 3)
}

export function ChangelogPage() {
  const { t } = useTranslation()
  const { language } = useApp()
  const [releases, setReleases] = useState<Release[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let active = true
    fetch(RELEASES_URL)
      .then((res) => (res.ok ? res.json() : Promise.reject(new Error('release fetch failed'))))
      .then((data: Release[]) => {
        if (!active) return
        setReleases(data)
        setLoading(false)
      })
      .catch(() => {
        if (!active) return
        setLoading(false)
      })

    return () => {
      active = false
    }
  }, [])

  const releaseCards = useMemo(
    () =>
      releases.map((release, index) => ({
        ...release,
        isLatest: index === 0,
        highlights: extractHighlights(release.body),
      })),
    [releases],
  )

  return (
    <section className="section">
      <div className="container">
        <h1 className="section__title">{t('changelog.title')}</h1>
        <p className="section__subtitle">{t('changelog.lead')}</p>

        {loading && <div className="card">{t('changelog.loading')}</div>}

        {!loading && releaseCards.length === 0 && <div className="card">{t('changelog.empty')}</div>}

        <div className="grid" style={{ marginTop: 32 }}>
          {releaseCards.map((release) => (
            <div key={release.id} className={`card ${release.isLatest ? 'card--highlight' : ''}`}>
              <div className="download-meta">
                <div className="download-meta__name">
                  {release.isLatest ? t('changelog.latestLabel') : release.name || release.tag_name}
                </div>
                <div className="download-meta__file">
                  {release.tag_name} · {formatDate(release.published_at, language)}
                </div>
              </div>
              <ul className="list">
                {release.highlights.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
              <a className="btn btn-ghost" href={release.html_url} target="_blank" rel="noreferrer">
                {t('changelog.viewOnGitHub')}
              </a>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}
