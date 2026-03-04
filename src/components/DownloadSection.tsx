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
  prerelease: boolean
  draft: boolean
}

type DownloadItem = {
  type: 'appimage' | 'flatpak' | 'flathub' | 'deb' | 'rpm' | 'tarball' | 'aur' | 'snap' | 'unknown'
  label: string
  fileName: string
  url: string
  size: number
  arch: string | null
  installCmd?: string
  depsCmd?: string
  helperCmds?: { label: string; cmds: string[] }
  helperNote?: { label: string; note: string }
  glibcNote?: string
}

const RELEASES_URL = 'https://api.github.com/repos/vicrodh/qbz/releases'
const AUR_PACKAGE_URL = 'https://aur.archlinux.org/packages/qbz-bin'
const FLATHUB_URL = 'https://flathub.org/apps/com.blitzfc.qbz'
const SNAP_STORE_URL = 'https://snapcraft.io/qbz-player'

/* ── Tabs ─────────────────────────────────────────────────── */

type TabId = 'arch' | 'debian' | 'fedora' | 'flatpak' | 'snap' | 'appimage' | 'tarball' | 'source'

const TABS: { id: TabId; label: string }[] = [
  { id: 'arch', label: 'Arch' },
  { id: 'debian', label: 'Debian / Ubuntu' },
  { id: 'fedora', label: 'Fedora / RHEL' },
  { id: 'flatpak', label: 'Flatpak' },
  { id: 'snap', label: 'Snap' },
  { id: 'appimage', label: 'AppImage' },
  { id: 'tarball', label: 'Tarball' },
  { id: 'source', label: 'Source' },
]

const TAB_TYPES: Record<TabId, DownloadItem['type'][]> = {
  arch: ['aur'],
  debian: ['deb'],
  fedora: ['rpm'],
  flatpak: ['flathub', 'flatpak'],
  snap: ['snap'],
  appimage: ['appimage'],
  tarball: ['tarball'],
  source: [],
}

function TabIcon({ id }: { id: TabId }) {
  const s = { width: 18, height: 18, className: 'download-tab__icon' }
  switch (id) {
    case 'arch':
      return <svg {...s} viewBox="0 0 24 24" fill="currentColor"><path d="M12 2L3 22h5l4-9 4 9h5z"/></svg>
    case 'debian':
      return <svg {...s} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M12 22C6.5 22 2 17.5 2 12S6.5 2 12 2s10 4.5 10 10"/><path d="M12 17c-2.8 0-5-2.2-5-5s2.2-5 5-5"/></svg>
    case 'fedora':
      return <svg {...s} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><circle cx="12" cy="12" r="9"/><path d="M10 16v-6h5M10 12h3"/></svg>
    case 'flatpak':
      return <svg {...s} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><rect x="3" y="7" width="18" height="13" rx="2"/><path d="M3 12h18"/></svg>
    case 'snap':
      return <svg {...s} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M12 2l8.5 5v10L12 22l-8.5-5V7z"/></svg>
    case 'appimage':
      return <svg {...s} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="4" y="4" width="16" height="16" rx="3"/><path d="M12 9v6M9 12l3 3 3-3"/></svg>
    case 'tarball':
      return <svg {...s} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><path d="M14 2v6h6"/><path d="M9 15h6M9 11h6"/></svg>
    case 'source':
      return <svg {...s} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M8 4L2 12l6 8"/><path d="M16 4l6 8-6 8"/></svg>
  }
}

/* ── Helpers ──────────────────────────────────────────────── */

const TYPE_LABELS: Record<DownloadItem['type'], string> = {
  aur: 'AUR (Arch)',
  flathub: 'Flathub',
  snap: 'Snap Store',
  appimage: 'AppImage',
  flatpak: 'Flatpak (GitHub Release)',
  deb: 'Debian/Ubuntu',
  rpm: 'Fedora/RHEL',
  tarball: 'Tarball',
  unknown: 'Download',
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

const getInstallCmd = (type: DownloadItem['type'], fileName: string): string | undefined => {
  switch (type) {
    case 'aur': return 'git clone https://aur.archlinux.org/qbz-bin.git && cd qbz-bin && makepkg -si'
    case 'flathub': return 'flatpak install flathub com.blitzfc.qbz'
    case 'snap': return 'sudo snap install qbz-player'
    case 'appimage': return `chmod +x ${fileName} && ./${fileName}`
    case 'deb': return `sudo dpkg -i ${fileName}`
    case 'rpm': return `sudo rpm -i ${fileName}`
    case 'flatpak': return `flatpak install --user ./${fileName}`
    case 'tarball': return `tar -xzf ${fileName} && ./qbz`
    default: return undefined
  }
}

const getHelperCmds = (type: DownloadItem['type']): { label: string; cmds: string[] } | undefined => {
  if (type === 'aur') {
    return { label: 'Or using a Helper?', cmds: ['yay -Syu qbz-bin', 'paru -Syu qbz-bin'] }
  }
  if (type === 'flatpak') {
    return {
      label: 'Local Library Access (NAS, external drives)',
      cmds: [
        'flatpak override --user --filesystem=/path/to/your/music com.blitzfc.qbz',
        'flatpak override --user --filesystem=/mnt/nas com.blitzfc.qbz',
      ]
    }
  }
  return undefined
}

const getDepsCmd = (type: DownloadItem['type']): string | undefined => {
  switch (type) {
    case 'deb': return 'sudo apt install -y libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 gstreamer1.0-plugins-base gstreamer1.0-plugins-good'
    case 'rpm': return 'sudo dnf install -y webkit2gtk4.1 gtk3 libappindicator-gtk3 gstreamer1-plugins-base gstreamer1-plugins-good'
    default: return undefined
  }
}

const mapAssets = (assets: ReleaseAsset[]): DownloadItem[] =>
  assets
    .filter((a) => a.browser_download_url)
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
        helperNote: undefined,
        glibcNote: (type === 'deb' || type === 'rpm') ? `downloads.glibcNote.${type}` : undefined,
      }
    })
    .filter((item) => item.type !== 'unknown')

/* ── Static Items ─────────────────────────────────────────── */

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

const flathubItem: DownloadItem = {
  type: 'flathub',
  label: TYPE_LABELS.flathub,
  fileName: 'com.blitzfc.qbz',
  url: FLATHUB_URL,
  size: 0,
  arch: null,
  installCmd: getInstallCmd('flathub', 'com.blitzfc.qbz'),
  helperCmds: getHelperCmds('flatpak'),
  helperNote: {
    label: 'Audiophile Setup (Bit-Perfect Audio)',
    note: 'After install, open QBZ \u2192 Settings \u2192 Audio. For ALSA Direct, select it as Audio Backend and choose your DAC. For PipeWire bit-perfect, your system needs prior configuration \u2014 the built-in HiFi Wizard can help set it up.'
  },
}

const snapItem: DownloadItem = {
  type: 'snap',
  label: TYPE_LABELS.snap,
  fileName: 'qbz-player',
  url: SNAP_STORE_URL,
  size: 0,
  arch: null,
  installCmd: getInstallCmd('snap', 'qbz-player'),
  helperCmds: {
    label: 'Required: Enable audio plugs',
    cmds: [
      'sudo snap connect qbz-player:alsa',
      'sudo snap connect qbz-player:pulseaudio',
      'sudo snap connect qbz-player:pipewire',
    ]
  },
  helperNote: {
    label: 'Optional: Access external drives / NAS',
    note: 'Run: sudo snap connect qbz-player:removable-media \u2014 This allows QBZ to access music on external drives, NAS mounts, or other removable storage.'
  },
}

/* ── Sub-components ───────────────────────────────────────── */

function ItemView({ item }: { item: DownloadItem }) {
  const { t } = useTranslation()
  const isExternal = ['aur', 'flathub', 'snap'].includes(item.type)
  const storeLabel = item.type === 'aur' ? 'AUR'
    : item.type === 'flathub' ? 'Flathub'
    : item.type === 'snap' ? 'Snap Store'
    : null

  return (
    <div className="download-item">
      <div className="download-item__header">
        <div className="download-item__info">
          <span className="download-item__label">{item.label}</span>
          {item.arch && <span className="download-item__arch">{item.arch}</span>}
        </div>
        <span className="download-item__file">
          {storeLabel ?? `${item.fileName} \u00b7 ${formatBytes(item.size)}`}
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
      {item.glibcNote && <p className="glibc-note">{t(item.glibcNote)}</p>}
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
      <a
        className="btn btn-ghost btn-sm"
        href={item.url}
        target={isExternal ? '_blank' : undefined}
        rel={isExternal ? 'noreferrer' : undefined}
      >
        {storeLabel ? `View on ${storeLabel}` : 'Download'}
      </a>
    </div>
  )
}

function SourceContent() {
  const { t } = useTranslation()
  return (
    <div className="download-tab-source">
      <div className="download-meta">
        <div className="download-meta__name">{t('downloads.buildTitle')}</div>
        <div className="download-meta__file">{t('downloads.buildBody')}</div>
      </div>
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
      <p style={{ fontSize: 13, color: 'var(--text-tertiary)', marginTop: 16 }}>{t('downloads.buildDisclaimer')}</p>
    </div>
  )
}

/* ── Main Component ───────────────────────────────────────── */

export function DownloadSection() {
  const { t } = useTranslation()
  const { language } = useApp()
  const [release, setRelease] = useState<ReleaseData | null>(null)
  const [error, setError] = useState(false)
  const [activeTab, setActiveTab] = useState<TabId>('arch')

  useEffect(() => {
    let active = true
    fetch(RELEASES_URL)
      .then((res) => (res.ok ? res.json() : Promise.reject(new Error('release fetch failed'))))
      .then((data: ReleaseData[]) => {
        if (!active) return
        const releaseWithAssets = data.find(
          (r) => r.assets && r.assets.length > 0 && !r.prerelease && !r.draft
        )
        if (releaseWithAssets) setRelease(releaseWithAssets)
        else setError(true)
      })
      .catch(() => { if (active) setError(true) })
    return () => { active = false }
  }, [])

  const allDownloads = useMemo(() => {
    const fromRelease = release ? mapAssets(release.assets) : []
    return [aurItem, flathubItem, snapItem, ...fromRelease]
  }, [release])

  const tabDownloads = useMemo(
    () => allDownloads.filter((item) => TAB_TYPES[activeTab].includes(item.type)),
    [activeTab, allDownloads]
  )

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
          <div className="card" style={{ marginTop: 24 }}>
            <div className="download-row download-row--stack">
              <div className="download-meta">
                <div className="download-meta__name">{t('downloads.allLabel')}</div>
                <div className="download-meta__file">
                  {t('downloads.versionLabel')} {release.tag_name} · {releaseDate}
                </div>
              </div>
            </div>

            <div className="download-tabs" role="tablist">
              {TABS.map((tab) => (
                <button
                  key={tab.id}
                  role="tab"
                  aria-selected={activeTab === tab.id}
                  className={`download-tab${activeTab === tab.id ? ' download-tab--active' : ''}`}
                  onClick={() => setActiveTab(tab.id)}
                >
                  <TabIcon id={tab.id} />
                  <span>{tab.label}</span>
                </button>
              ))}
            </div>

            <div className="download-tab-content" role="tabpanel">
              {activeTab === 'source' ? (
                <SourceContent />
              ) : tabDownloads.length > 0 ? (
                <div className="download-list">
                  {tabDownloads.map((item) => (
                    <ItemView key={`${item.type}-${item.fileName}`} item={item} />
                  ))}
                </div>
              ) : (
                <p style={{ color: 'var(--text-tertiary)', fontSize: '0.9rem', padding: '24px 0' }}>
                  No packages available for this format in the current release.
                </p>
              )}
            </div>

            <a className="btn btn-ghost" href={release.html_url} target="_blank" rel="noreferrer">
              {t('downloads.viewAll')}
            </a>
          </div>
        )}
      </div>
    </section>
  )
}
