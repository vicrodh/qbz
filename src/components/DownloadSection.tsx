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
  type: 'appimage' | 'flatpak' | 'deb' | 'rpm' | 'tarball' | 'aur' | 'snap' | 'unknown'
  label: string
  fileName: string
  url: string
  size: number
  arch: string | null
  installCmd?: string
  depsCmd?: string
  helperCmds?: { label: string; cmds: string[] }
  helperNote?: { label: string; note: string }
}

const RELEASES_URL = 'https://api.github.com/repos/vicrodh/qbz/releases'

const TYPE_LABELS: Record<DownloadItem['type'], string> = {
  aur: 'AUR (Arch)',
  snap: 'Snap Store',
  appimage: 'AppImage',
  flatpak: 'Flatpak',
  deb: 'Debian/Ubuntu',
  rpm: 'Fedora/RHEL',
  tarball: 'Tarball',
  unknown: 'Download',
}

const TYPE_PRIORITY: Record<DownloadItem['type'], number> = {
  aur: 0,
  snap: 1,
  appimage: 2,
  flatpak: 3,
  deb: 4,
  rpm: 5,
  tarball: 6,
  unknown: 7,
}

const AUR_PACKAGE_URL = 'https://aur.archlinux.org/packages/qbz-bin'
const SNAP_STORE_URL = 'https://snapcraft.io/qbz-player'

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

const DISABLED_TYPES: DownloadItem['type'][] = [] // All types enabled

// Generate install command based on actual filename
const getInstallCmd = (type: DownloadItem['type'], fileName: string): string | undefined => {
  switch (type) {
    case 'aur':
      return 'git clone https://aur.archlinux.org/qbz-bin.git && cd qbz-bin && makepkg -si'
    case 'snap':
      return 'sudo snap install qbz-player'
    case 'appimage':
      return `chmod +x ${fileName} && ./${fileName}`
    case 'deb':
      return `sudo dpkg -i ${fileName}`
    case 'rpm':
      return `sudo rpm -i ${fileName}`
    case 'flatpak':
      return `flatpak install --user ./${fileName}`
    case 'tarball':
      return `tar -xzf ${fileName} && ./qbz`
    default:
      return undefined
  }
}

// AUR helper commands and Flatpak additional info
const getHelperCmds = (type: DownloadItem['type']): { label: string; cmds: string[] } | undefined => {
  if (type === 'aur') {
    return {
      label: 'Or using a Helper?',
      cmds: ['yay -Syu qbz-bin', 'paru -Syu qbz-bin']
    }
  }
  return undefined
}

// Dependency commands for distros that need them
const getDepsCmd = (type: DownloadItem['type']): string | undefined => {
  switch (type) {
    case 'deb':
      return 'sudo apt install -y libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 gstreamer1.0-plugins-base gstreamer1.0-plugins-good'
    case 'rpm':
      return 'sudo dnf install -y webkit2gtk4.1 gtk3 libappindicator-gtk3 gstreamer1-plugins-base gstreamer1-plugins-good'
    default:
      return undefined
  }
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
        installCmd: getInstallCmd(type, asset.name),
        depsCmd: getDepsCmd(type),
        helperCmds: getHelperCmds(type),
        helperNote: type === 'flatpak' ? {
          label: 'Audiophile Setup (Bit-Perfect Audio)',
          note: 'After install, open QBZ → Settings → Audio and select "ALSA Direct" as Audio Backend. Note: PipeWire bit-perfect is unavailable in Flatpak due to sandbox restrictions.'
        } : undefined,
      }
    })
    .filter((item) => !DISABLED_TYPES.includes(item.type))
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
  installCmd: getInstallCmd('aur', 'qbz-bin'),
  helperCmds: getHelperCmds('aur'),
}

// Snap Store download item
const snapItem: DownloadItem = {
  type: 'snap',
  label: TYPE_LABELS.snap,
  fileName: 'qbz-player',
  url: SNAP_STORE_URL,
  size: 0,
  arch: null,
  installCmd: getInstallCmd('snap', 'qbz-player'),
  helperNote: {
    label: 'Or install from Snap Store GUI',
    note: 'Search for "QBZ" in your Snap Store application, or click the link below to open the store page.'
  },
}

// Get all downloads including AUR and Snap for Linux users
const getDownloadsWithExtras = (items: DownloadItem[]) => {
  const platform = detectPlatform()
  if (platform.isLinux) {
    return [aurItem, snapItem, ...items]
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
  const downloads = useMemo(() => getDownloadsWithExtras(baseDownloads), [baseDownloads])
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
                {downloads.map((item) => (
                  <div key={item.fileName} className="download-item">
                    <div className="download-item__header">
                      <div className="download-item__info">
                        <span className="download-item__label">{item.label}</span>
                        {item.arch && <span className="download-item__arch">{item.arch}</span>}
                      </div>
                      <span className="download-item__file">
                        {item.type === 'aur' ? 'Arch User Repository' : item.type === 'snap' ? 'Snap Store' : `${item.fileName} · ${formatBytes(item.size)}`}
                      </span>
                    </div>
                    {item.installCmd && (
                      <div className="terminal">
                        <code>
                          <span className="terminal__prompt">$</span>
                          <span className="terminal__cmd">{item.installCmd}</span>
                        </code>
                        <CopyButton text={item.installCmd} />
                      </div>
                    )}
                    {item.depsCmd && (
                      <details className="deps-details">
                        <summary className="deps-summary">Missing dependencies?</summary>
                        <div className="terminal terminal--deps">
                          <code>
                            <span className="terminal__prompt">#</span>
                            <span className="terminal__cmd">{item.depsCmd}</span>
                          </code>
                          <CopyButton text={item.depsCmd} />
                        </div>
                      </details>
                    )}
                    {item.helperCmds && (
                      <details className="deps-details">
                        <summary className="deps-summary">{item.helperCmds.label}</summary>
                        {item.helperCmds.cmds.map((cmd) => (
                          <div key={cmd} className="terminal terminal--deps">
                            <code>
                              <span className="terminal__prompt">$</span>
                              <span className="terminal__cmd">{cmd}</span>
                            </code>
                            <CopyButton text={cmd} />
                          </div>
                        ))}
                      </details>
                    )}
                    {item.helperNote && (
                      <details className="deps-details">
                        <summary className="deps-summary">{item.helperNote.label}</summary>
                        <p style={{ fontSize: 13, color: 'var(--text-secondary)', marginTop: 8, lineHeight: 1.5 }}>
                          {item.helperNote.note}
                        </p>
                      </details>
                    )}
                    <a className="btn btn-ghost btn-sm" href={item.url} target={item.type === 'aur' || item.type === 'snap' ? '_blank' : undefined} rel={item.type === 'aur' || item.type === 'snap' ? 'noreferrer' : undefined}>
                      {item.type === 'aur' ? 'View on AUR' : item.type === 'snap' ? 'View on Snap Store' : 'Download'}
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
                <div className="download-meta__name">{t('downloads.buildTitle')}</div>
                <div className="download-meta__file">{t('downloads.buildBody')}</div>
              </div>
              <details className="details" style={{ marginTop: 16 }}>
                <summary>{t('downloads.buildInstructions.summary')}</summary>
                <div style={{ marginTop: 16 }}>
                  <div className="download-meta__name" style={{ fontSize: 14, marginBottom: 8 }}>{t('downloads.buildInstructions.prereqTitle')}</div>
                  <details className="deps-details">
                    <summary className="deps-summary">Arch Linux</summary>
                    <div className="terminal terminal--deps">
                      <code>
                        <span className="terminal__prompt">$</span>
                        <span className="terminal__cmd">sudo pacman -S base-devel rust nodejs npm webkit2gtk-4.1 gtk3</span>
                      </code>
                      <CopyButton text="sudo pacman -S base-devel rust nodejs npm webkit2gtk-4.1 gtk3" />
                    </div>
                  </details>
                  <details className="deps-details">
                    <summary className="deps-summary">Debian / Ubuntu</summary>
                    <div className="terminal terminal--deps">
                      <code>
                        <span className="terminal__prompt">$</span>
                        <span className="terminal__cmd">sudo apt install build-essential curl libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev</span>
                      </code>
                      <CopyButton text="sudo apt install build-essential curl libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev" />
                    </div>
                  </details>
                  <details className="deps-details">
                    <summary className="deps-summary">Fedora</summary>
                    <div className="terminal terminal--deps">
                      <code>
                        <span className="terminal__prompt">$</span>
                        <span className="terminal__cmd">sudo dnf install webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel</span>
                      </code>
                      <CopyButton text="sudo dnf install webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel" />
                    </div>
                  </details>
                  <details className="deps-details">
                    <summary className="deps-summary">Rust + Node.js</summary>
                    <div className="terminal terminal--deps">
                      <code>
                        <span className="terminal__prompt">$</span>
                        <span className="terminal__cmd">curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh</span>
                      </code>
                      <CopyButton text="curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" />
                    </div>
                    <p style={{ fontSize: 13, color: 'var(--text-tertiary)', marginTop: 8 }}>{t('downloads.buildInstructions.nodeNote')}</p>
                  </details>

                  <div className="download-meta__name" style={{ fontSize: 14, marginBottom: 8, marginTop: 16 }}>{t('downloads.buildInstructions.cloneTitle')}</div>
                  <div className="terminal">
                    <code>
                      <span className="terminal__prompt">$</span>
                      <span className="terminal__cmd">git clone https://github.com/vicrodh/qbz.git && cd qbz</span>
                    </code>
                    <CopyButton text="git clone https://github.com/vicrodh/qbz.git && cd qbz" />
                  </div>
                  <div className="terminal" style={{ marginTop: 8 }}>
                    <code>
                      <span className="terminal__prompt">$</span>
                      <span className="terminal__cmd">npm install</span>
                    </code>
                    <CopyButton text="npm install" />
                  </div>
                  <div className="terminal" style={{ marginTop: 8 }}>
                    <code>
                      <span className="terminal__prompt">$</span>
                      <span className="terminal__cmd">npm run tauri dev</span>
                    </code>
                    <CopyButton text="npm run tauri dev" />
                  </div>
                  <div className="terminal" style={{ marginTop: 8 }}>
                    <code>
                      <span className="terminal__prompt">$</span>
                      <span className="terminal__cmd">npm run tauri build</span>
                    </code>
                    <CopyButton text="npm run tauri build" />
                  </div>

                  <div className="download-meta__name" style={{ fontSize: 14, marginBottom: 8, marginTop: 16 }}>{t('downloads.buildInstructions.apiTitle')}</div>
                  <p style={{ fontSize: 13, color: 'var(--text-secondary)', marginBottom: 12 }}>{t('downloads.buildInstructions.apiLead')}</p>
                  <div className="terminal">
                    <code>
                      <span className="terminal__prompt">$</span>
                      <span className="terminal__cmd">cp .env.example .env</span>
                    </code>
                    <CopyButton text="cp .env.example .env" />
                  </div>
                  <p style={{ fontSize: 13, color: 'var(--text-secondary)', marginTop: 12 }}>{t('downloads.buildInstructions.apiBody')}</p>
                  <details className="deps-details" style={{ marginTop: 12 }}>
                    <summary className="deps-summary">{t('downloads.buildInstructions.apiKeysTitle')}</summary>
                    <ul className="list list--compact" style={{ marginTop: 8 }}>
                      <li><strong>Last.fm</strong> — <a href="https://www.last.fm/api/account/create" target="_blank" rel="noreferrer">last.fm/api/account/create</a></li>
                      <li><strong>Discogs</strong> — <a href="https://www.discogs.com/settings/developers" target="_blank" rel="noreferrer">discogs.com/settings/developers</a></li>
                      <li><strong>Spotify</strong> — <a href="https://developer.spotify.com/dashboard" target="_blank" rel="noreferrer">developer.spotify.com/dashboard</a></li>
                      <li><strong>Tidal</strong> — <a href="https://developer.tidal.com/" target="_blank" rel="noreferrer">developer.tidal.com</a></li>
                    </ul>
                    <p style={{ fontSize: 13, color: 'var(--text-tertiary)', marginTop: 8 }}>{t('downloads.buildInstructions.apiOptional')}</p>
                  </details>
                </div>
              </details>
              <p style={{ fontSize: 13, color: 'var(--text-tertiary)', marginTop: 16 }}>{t('downloads.buildDisclaimer')}</p>
            </div>
          </div>
        )}
      </div>
    </section>
  )
}
