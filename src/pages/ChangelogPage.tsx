import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { formatDate } from '../lib/format'

type Release = {
  id: number
  tag_name: string
  name: string
  published_at: string
  body: string | null
  html_url: string
}

const RELEASES_URL = 'https://api.github.com/repos/vicrodh/qbz/releases'

const extractHighlights = (body: string | null | undefined): string[] => {
  if (!body) return []

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

// Simple inline markdown renderer for bold and code
const renderInlineMarkdown = (text: string): React.ReactNode => {
  // Process **bold** and `code` patterns
  const parts: React.ReactNode[] = []
  let remaining = text
  let key = 0

  while (remaining.length > 0) {
    // Check for **bold**
    const boldMatch = remaining.match(/\*\*(.+?)\*\*/)
    // Check for `code`
    const codeMatch = remaining.match(/`([^`]+)`/)

    // Find which comes first
    const boldIndex = boldMatch ? remaining.indexOf(boldMatch[0]) : -1
    const codeIndex = codeMatch ? remaining.indexOf(codeMatch[0]) : -1

    if (boldIndex === -1 && codeIndex === -1) {
      // No more patterns, add remaining text
      parts.push(remaining)
      break
    }

    // Determine which pattern comes first
    const useCode = codeIndex !== -1 && (boldIndex === -1 || codeIndex < boldIndex)
    const match = useCode ? codeMatch! : boldMatch!
    const matchIndex = useCode ? codeIndex : boldIndex

    // Add text before the match
    if (matchIndex > 0) {
      parts.push(remaining.substring(0, matchIndex))
    }

    // Add the formatted element
    if (useCode) {
      parts.push(<code key={key++} className="inline-code">{match[1]}</code>)
    } else {
      parts.push(<strong key={key++}>{match[1]}</strong>)
    }

    // Continue with remaining text
    remaining = remaining.substring(matchIndex + match[0].length)
  }

  return parts.length === 1 && typeof parts[0] === 'string' ? parts[0] : <>{parts}</>
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
              <ul className="list">
                {release.highlights.map((item, idx) => (
                  <li key={idx}>{renderInlineMarkdown(item)}</li>
                ))}
              </ul>
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
