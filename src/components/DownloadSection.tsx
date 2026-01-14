import { useEffect, useMemo, useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { formatBytes, formatDate } from '../lib/format'

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    })
  }, [text])

  return (
    <button className="copy-btn" onClick={handleCopy} title="Copy command">
      {copied ? (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <polyline points="20 6 9 17 4 12" />
        </svg>
      ) : (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
        </svg>
      )}
    </button>
  )
}

type ReleaseAsset = {
  name: string
  browser_download_url: string
  size: number
}

type ReleaseData = {
  tag_name: string
  published_at: string
  assets: ReleaseAsset[]
  html_url: string
}

type DownloadItem = {
  type: 'appimage' | 'flatpak' | 'deb' | 'rpm' | 'tarball' | 'aur' | 'unknown'
  label: string
  fileName: string
  url: string
  size: number
  arch: string | null
}

const RELEASES_URL = 'https://api.github.com/repos/vicrodh/qbz/releases'

const TYPE_LABELS: Record<DownloadItem['type'], string> = {
  aur: 'AUR (Arch)',
  appimage: 'AppImage',
  flatpak: 'Flatpak',
  deb: 'Debian/Ubuntu',
  rpm: 'Fedora/RHEL',
  tarball: 'Tarball',
  unknown: 'Download',
}

const TYPE_PRIORITY: Record<DownloadItem['type'], number> = {
  aur: 0,
  appimage: 1,
  flatpak: 2,
  deb: 3,
  rpm: 4,
  tarball: 5,
  unknown: 6,
}

const AUR_PACKAGE_URL = 'https://aur.archlinux.org/packages/qbz-bin'

const getType = (name: string): DownloadItem['type'] => {
  const lower = name.toLowerCase()
  if (lower.endsWith('.appimage')) return 'appimage'
  if (lower.endsWith('.flatpak')) return 'flatpak'
  if (lower.endsWith('.deb')) return 'deb'
  if (lower.endsWith('.rpm')) return 'rpm'
  if (lower.endsWith('.tar.gz') || lower.endsWith('.tgz') || lower.endsWith('.tar.xz')) return 'tarball'
  return 'unknown'
}

const getArch = (name: string): string | null => {
  const lower = name.toLowerCase()
  if (lower.includes('aarch64') || lower.includes('arm64')) return 'arm64'
  if (lower.includes('x86_64') || lower.includes('amd64')) return 'x86_64'
  return null
}

const mapAssets = (assets: ReleaseAsset[]): DownloadItem[] =>
  assets
    .filter((asset) => asset.browser_download_url)
    .map((asset) => {
      const type = getType(asset.name)
      return {
        type,
        label: TYPE_LABELS[type],
        fileName: asset.name,
        url: asset.browser_download_url,
        size: asset.size,
        arch: getArch(asset.name),
      }
    })
    .sort((a, b) => TYPE_PRIORITY[a.type] - TYPE_PRIORITY[b.type])

const detectPlatform = () => {
  const ua = navigator.userAgent.toLowerCase()
  const platform = navigator.platform.toLowerCase()

  const isLinux = ua.includes('linux') && !ua.includes('android')
  const isMac = platform.includes('mac') || ua.includes('mac')
  const isWindows = platform.includes('win') || ua.includes('windows')

  // Note: Most Linux browsers don't include distro info in user-agent
  // We show all options and let users choose

  const arch = ua.includes('aarch64') || ua.includes('arm64') ? 'arm64' : 'x86_64'

  return { isLinux, isMac, isWindows, arch }
}

// AUR download item for Arch users
const aurItem: DownloadItem = {
  type: 'aur',
  label: TYPE_LABELS.aur,
  fileName: 'qbz-bin',
  url: AUR_PACKAGE_URL,
  size: 0,
  arch: null,
}


// Get all downloads including AUR for Linux users
const getDownloadsWithAur = (items: DownloadItem[]) => {
  const platform = detectPlatform()
  if (platform.isLinux) {
    return [aurItem, ...items]
  }
  return items
}

export function DownloadSection() {
  const { t } = useTranslation()
  const { language } = useApp()
  const [release, setRelease] = useState<ReleaseData | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let active = true
    fetch(RELEASES_URL)
      .then((res) => (res.ok ? res.json() : Promise.reject(new Error('release fetch failed'))))
      .then((data: ReleaseData[]) => {
        if (!active) return
        // Find the first release that has assets
        const releaseWithAssets = data.find((r) => r.assets && r.assets.length > 0)
        if (releaseWithAssets) {
          setRelease(releaseWithAssets)
        } else {
          setError(true)
        }
      })
      .catch(() => {
        if (!active) return
        setError(true)
      })
    return () => {
      active = false
    }
  }, [])

  const baseDownloads = useMemo(() => (release ? mapAssets(release.assets) : []), [release])
  const downloads = useMemo(() => getDownloadsWithAur(baseDownloads), [baseDownloads])
  const releaseDate = release ? formatDate(release.published_at, language) : null

  return (
    <section id="downloads" className="section">
      <div className="container">
        <h2 className="section__title">{t('downloads.title')}</h2>
        <p className="section__subtitle">{t('downloads.lead')}</p>

        {error && (
          <div className="card download-state">
            <p>{t('downloads.error')}</p>
            <a className="btn btn-ghost" href="https://github.com/vicrodh/qbz/releases" target="_blank" rel="noreferrer">
              {t('downloads.viewAll')}
            </a>
          </div>
        )}

        {!error && !release && (
          <div className="card download-state">
            <p>{t('downloads.loading')}</p>
          </div>
        )}

        {release && (
          <div className="download-grid">
            <div className="card">
              <div className="download-row download-row--stack">
                <div className="download-meta">
                  <div className="download-meta__name">{t('downloads.allLabel')}</div>
                  <div className="download-meta__file">
                    {t('downloads.versionLabel')} {release.tag_name} · {releaseDate}
                  </div>
                </div>
              </div>
              <div className="download-list">
                {downloads.map((item) => {
                  const instruction = item.type !== 'unknown' ? t(`downloads.instructions.${item.type}`) : null
                  return (
                    <div key={item.fileName} className="download-item">
                      <div className="download-item__header">
                        <div className="download-item__info">
                          <span className="download-item__label">{item.label}</span>
                          {item.arch && <span className="download-item__arch">{item.arch}</span>}
                        </div>
                        <span className="download-item__file">
                          {item.type === 'aur' ? 'Arch User Repository' : `${item.fileName} · ${formatBytes(item.size)}`}
                        </span>
                      </div>
                      {instruction && (
                        <div className="terminal">
                          <code>
                            <span className="terminal__prompt">$</span>
                            <span className="terminal__cmd">{instruction}</span>
                          </code>
                          <CopyButton text={instruction} />
                        </div>
                      )}
                      <a className="btn btn-ghost btn-sm" href={item.url} target={item.type === 'aur' ? '_blank' : undefined} rel={item.type === 'aur' ? 'noreferrer' : undefined}>
                        {item.type === 'aur' ? 'View on AUR' : 'Download'}
                      </a>
                    </div>
                  )
                })}
              </div>
              <a className="btn btn-ghost" href={release.html_url} target="_blank" rel="noreferrer">
                {t('downloads.viewAll')}
              </a>
            </div>

            <div className="card">
              <div className="download-meta">
                <div className="download-meta__name">{t('downloads.buildTitle')}</div>
                <div className="download-meta__file">{t('downloads.buildBody')}</div>
              </div>
            </div>
          </div>
        )}
      </div>
    </section>
  )
}
