import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { formatDate } from '../lib/format'
import { marked } from 'marked'

type Release = {
  id: number
  tag_name: string
  name: string
  published_at: string
  body: string | null
  html_url: string
}

const RELEASES_URL = 'https://api.github.com/repos/vicrodh/qbz/releases'

// Configure marked for safe output (no raw HTML passthrough)
marked.setOptions({
  gfm: true,
  breaks: false,
})

function renderBody(body: string | null): string {
  if (!body) return ''
  return marked.parse(body, { async: false }) as string
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
        html: renderBody(release.body),
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

        <div className="changelog-grid" style={{ marginTop: 32 }}>
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
              {release.html ? (
                <div
                  className="release-body"
                  dangerouslySetInnerHTML={{ __html: release.html }}
                />
              ) : (
                <p style={{ color: 'var(--text-tertiary)', marginTop: 16 }}>
                  {t('changelog.empty')}
                </p>
              )}
              <a className="btn btn-ghost" href={release.html_url} target="_blank" rel="noreferrer" style={{ marginTop: 24 }}>
                {t('changelog.viewOnGitHub')}
              </a>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}
