import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { formatBytes, formatDate } from '../lib/format'

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
  type: 'appimage' | 'flatpak' | 'deb' | 'rpm' | 'tarball' | 'unknown'
  label: string
  fileName: string
  url: string
  size: number
  arch: string | null
}

const RELEASE_URL = 'https://api.github.com/repos/vicrodh/qbz/releases/latest'

const TYPE_LABELS: Record<DownloadItem['type'], string> = {
  appimage: 'AppImage',
  flatpak: 'Flatpak',
  deb: 'Debian/Ubuntu',
  rpm: 'Fedora/RHEL',
  tarball: 'Tarball',
  unknown: 'Download',
}

const TYPE_PRIORITY: Record<DownloadItem['type'], number> = {
  appimage: 1,
  flatpak: 2,
  deb: 3,
  rpm: 4,
  tarball: 5,
  unknown: 6,
}

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

  const isLinux = ua.includes('linux')
  const isMac = platform.includes('mac') || ua.includes('mac')
  const isWindows = platform.includes('win') || ua.includes('windows')

  let distro: 'debian' | 'rpm' | 'unknown' = 'unknown'
  if (ua.includes('ubuntu') || ua.includes('debian') || ua.includes('mint') || ua.includes('pop')) {
    distro = 'debian'
  }
  if (ua.includes('fedora') || ua.includes('redhat') || ua.includes('rhel') || ua.includes('suse')) {
    distro = 'rpm'
  }

  const arch = ua.includes('aarch64') || ua.includes('arm64') ? 'arm64' : 'x86_64'

  return { isLinux, isMac, isWindows, distro, arch }
}

const findRecommended = (items: DownloadItem[]) => {
  const platform = detectPlatform()
  if (!platform.isLinux) {
    return null
  }

  const byType = (type: DownloadItem['type']) =>
    items.find((item) => item.type === type && (!item.arch || item.arch === platform.arch))

  if (platform.distro === 'debian') {
    return byType('deb') || byType('appimage')
  }
  if (platform.distro === 'rpm') {
    return byType('rpm') || byType('appimage')
  }
  return byType('appimage') || byType('flatpak') || byType('tarball')
}

export function DownloadSection() {
  const { t } = useTranslation()
  const { language } = useApp()
  const [release, setRelease] = useState<ReleaseData | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let active = true
    fetch(RELEASE_URL)
      .then((res) => (res.ok ? res.json() : Promise.reject(new Error('release fetch failed'))))
      .then((data: ReleaseData) => {
        if (!active) return
        setRelease(data)
      })
      .catch(() => {
        if (!active) return
        setError(true)
      })
    return () => {
      active = false
    }
  }, [])

  const downloads = useMemo(() => (release ? mapAssets(release.assets) : []), [release])
  const recommended = useMemo(() => findRecommended(downloads), [downloads])
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
            <div className="download-row card card--highlight">
              <div className="download-meta">
                <span className="pill">{t('downloads.recommendedLabel')}</span>
                <div className="download-meta__name">
                  {recommended ? `${recommended.label} ${recommended.arch ? `(${recommended.arch})` : ''}` : t('downloads.allLabel')}
                </div>
                <div className="download-meta__file">
                  {t('downloads.versionLabel')} {release.tag_name} · {releaseDate}
                </div>
              </div>
              {recommended ? (
                <a className="btn btn-primary" href={recommended.url}>
                  {recommended.label}
                </a>
              ) : (
                <a className="btn btn-primary" href={release.html_url} target="_blank" rel="noreferrer">
                  {t('downloads.viewAll')}
                </a>
              )}
            </div>

            <div className="card">
              <div className="download-row download-row--stack">
                <div className="download-meta">
                  <div className="download-meta__name">{t('downloads.allLabel')}</div>
                  <div className="download-meta__file">{t('downloads.fileCount', { count: release.assets.length })}</div>
                </div>
              </div>
              <div className="download-list">
                {downloads.map((item) => (
                  <div key={item.fileName} className="download-row">
                    <div className="download-meta">
                      <div className="download-meta__name">
                        {item.label} {item.arch ? <span className="pill">{item.arch}</span> : null}
                      </div>
                      <div className="download-meta__file">
                        {item.fileName} · {formatBytes(item.size)}
                      </div>
                    </div>
                    <a className="btn btn-ghost" href={item.url}>
                      {item.label}
                    </a>
                  </div>
                ))}
              </div>
              <a className="btn btn-ghost" href={release.html_url} target="_blank" rel="noreferrer">
                {t('downloads.viewAll')}
              </a>
            </div>

            <div className="card">
              <div className="download-meta">
                <div className="download-meta__name">{t('downloads.instructionsTitle')}</div>
                <div className="download-meta__file">{t('downloads.buildDisclaimer')}</div>
              </div>
              <div className="install-grid">
                <div>
                  <span className="pill">AppImage</span>
                  <code>{t('downloads.instructions.appimage')}</code>
                </div>
                <div>
                  <span className="pill">Deb</span>
                  <code>{t('downloads.instructions.deb')}</code>
                </div>
                <div>
                  <span className="pill">RPM</span>
                  <code>{t('downloads.instructions.rpm')}</code>
                </div>
                <div>
                  <span className="pill">Flatpak</span>
                  <code>{t('downloads.instructions.flatpak')}</code>
                </div>
                <div>
                  <span className="pill">Tarball</span>
                  <code>{t('downloads.instructions.tarball')}</code>
                </div>
              </div>
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
