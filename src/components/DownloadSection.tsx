import { useEffect, useMemo, useState, useCallback, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { formatBytes, formatDate } from '../lib/format'

/* ── Copy button ──────────────────────────────────────────── */

function CopyButton({ text }: { text: string }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    })
  }, [text])

  return (
    <button className="copy-btn" type="button" onClick={handleCopy} title={t('downloads.copy')} aria-label={t('downloads.copy')}>
      {copied ? (
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
          <polyline points="20 6 9 17 4 12" />
        </svg>
      ) : (
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
          <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
        </svg>
      )}
    </button>
  )
}

function Cmd({ cmd, prompt = '$', block = false }: { cmd: string; prompt?: string; block?: boolean }) {
  return (
    <div className="terminal">
      <code>
        {prompt && <span className="terminal__prompt">{prompt}</span>}
        {block ? <pre className="terminal__cmd">{cmd}</pre> : <span className="terminal__cmd">{cmd}</span>}
      </code>
      <CopyButton text={cmd} />
    </div>
  )
}

/* ── Release data ─────────────────────────────────────────── */

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

type AssetType = 'appimage' | 'flatpak' | 'deb' | 'rpm' | 'tarball' | 'qbzd' | 'dmg' | 'msi' | 'unknown'

type DownloadItem = {
  type: AssetType
  fileName: string
  url: string
  size: number
  arch: string | null
}

const RELEASES_URL = 'https://api.github.com/repos/vicrodh/qbz/releases'
const RELEASES_PAGE = 'https://github.com/vicrodh/qbz/releases'
const SIGNED_MACOS_RELEASE_API_URL = 'https://api.github.com/repos/afonsojramos/qbz-macos/releases/latest'
const SIGNED_MACOS_RELEASES_URL = 'https://github.com/afonsojramos/qbz-macos/releases/latest'
const SIGNED_MACOS_PROVENANCE_URL = 'https://github.com/afonsojramos/qbz-macos#trust-and-provenance'
const HOMEBREW_QBZ_URL = 'https://github.com/afonsojramos/homebrew-qbz'
const HOMEBREW_QBZ_COMMAND = 'brew install --cask afonsojramos/qbz/qbz'
const AUR_PACKAGE_URL = 'https://aur.archlinux.org/packages/qbz-bin'
const FLATHUB_URL = 'https://flathub.org/apps/com.blitzfc.qbz'
const SNAP_STORE_URL = 'https://snapcraft.io/qbz-player'
const GENTOO_OVERLAY_URL = 'https://github.com/vicrodh/qbz-overlay'
const QBZD_MANUAL_URL = 'https://github.com/vicrodh/qbz/wiki/Headless-Daemon'
const WINDOWS_ADOPT_URL = 'https://github.com/vicrodh/qbz/discussions'

const getType = (name: string): AssetType => {
  const lower = name.toLowerCase()
  if (lower.endsWith('.sig') || lower === 'latest.json') return 'unknown'
  if (lower.includes('.app.tar.gz')) return 'unknown'
  if (lower.startsWith('qbzd-')) return lower.endsWith('.tar.gz') ? 'qbzd' : 'unknown'
  if (lower.endsWith('.appimage')) return 'appimage'
  if (lower.endsWith('.flatpak')) return 'flatpak'
  if (lower.endsWith('.deb')) return 'deb'
  if (lower.endsWith('.rpm')) return 'rpm'
  if (lower.endsWith('.tar.gz') || lower.endsWith('.tgz') || lower.endsWith('.tar.xz')) return 'tarball'
  if (lower.endsWith('.dmg')) return 'dmg'
  if (lower.endsWith('.msi')) return 'msi'
  return 'unknown'
}

const getArch = (name: string): string | null => {
  const lower = name.toLowerCase()
  if (lower.includes('aarch64') || lower.includes('arm64')) return 'arm64'
  if (lower.includes('x86_64') || lower.includes('amd64') || lower.includes('_x64')) return 'x86_64'
  return null
}

const ARCH_ORDER: Record<string, number> = { x86_64: 0, arm64: 1 }

const mapAssets = (assets: ReleaseAsset[]): DownloadItem[] =>
  assets
    .filter((a) => a.browser_download_url)
    .map((asset) => ({
      type: getType(asset.name),
      fileName: asset.name,
      url: asset.browser_download_url,
      size: asset.size,
      arch: getArch(asset.name),
    }))
    .filter((item) => item.type !== 'unknown')
    .sort((a, b) => (ARCH_ORDER[a.arch ?? ''] ?? 9) - (ARCH_ORDER[b.arch ?? ''] ?? 9))

const stripArchive = (fileName: string) => fileName.replace(/\.tar\.(gz|xz)$/, '').replace(/\.tgz$/, '')

/* ── Linux tabs ───────────────────────────────────────────── */

type TabId = 'arch' | 'debian' | 'fedora' | 'flatpak' | 'snap' | 'appimage' | 'nixos' | 'gentoo' | 'tarball' | 'qbzd' | 'source'

const TABS: { id: TabId; label: string; icon: string | null }[] = [
  { id: 'arch', label: 'Arch', icon: '/icons/arch.svg' },
  { id: 'debian', label: 'Debian / Ubuntu', icon: '/icons/debian.svg' },
  { id: 'fedora', label: 'Fedora / openSUSE', icon: '/icons/redhat.svg' },
  { id: 'flatpak', label: 'Flatpak', icon: '/icons/flatpak.svg' },
  { id: 'snap', label: 'Snap', icon: '/icons/snapcraft.svg' },
  { id: 'appimage', label: 'AppImage', icon: null },
  { id: 'nixos', label: 'NixOS', icon: '/icons/nixos.svg' },
  { id: 'gentoo', label: 'Gentoo', icon: '/icons/gentoo.svg' },
  { id: 'tarball', label: 'Tarball', icon: '/icons/tarball.svg' },
  { id: 'qbzd', label: 'qbzd', icon: null },
  { id: 'source', label: 'Source', icon: '/icons/rust.svg' },
]

function TabIcon({ id, src }: { id: TabId; src: string | null }) {
  if (src) {
    return <img className="download-tab__icon" src={src} alt="" width={16} height={16} loading="lazy" />
  }
  if (id === 'qbzd') {
    return (
      <svg className="download-tab__icon download-tab__icon--inline" width={16} height={16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <rect x="3" y="5" width="18" height="14" rx="2" />
        <path d="M7 10l3 2-3 2M12 14h5" />
      </svg>
    )
  }
  return (
    <svg className="download-tab__icon download-tab__icon--inline" width={16} height={16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="4" y="4" width="16" height="16" rx="3" />
      <path d="M12 9v6M9 12l3 3 3-3" />
    </svg>
  )
}

const ARCH_TITLES: Record<string, string> = {
  x86_64: 'Intel and AMD, 64-bit',
  arm64: 'ARM 64-bit (Apple Silicon, Raspberry Pi, Snapdragon)',
}

function FileLine({ item }: { item: DownloadItem }) {
  return (
    <div className="download-item__info">
      {item.arch && <span className="download-item__arch" title={ARCH_TITLES[item.arch]}>{item.arch}</span>}
      <span className="download-item__file">{item.fileName} · {formatBytes(item.size)}</span>
    </div>
  )
}

function DownloadLink({ item, label }: { item: DownloadItem; label: string }) {
  return (
    <div className="platform__actions">
      <a className="btn btn-ghost btn-sm" href={item.url}>{label}</a>
    </div>
  )
}

/* ── Linux tab panels ─────────────────────────────────────── */

function ArchPanel() {
  const { t } = useTranslation()
  return (
    <div className="download-list">
      <div className="download-item">
        <div className="download-item__header">
          <h4 className="download-item__label">AUR · qbz-bin</h4>
        </div>
        <Cmd cmd="git clone https://aur.archlinux.org/qbz-bin.git && cd qbz-bin && makepkg -si" />
        <details className="deps-details">
          <summary className="deps-summary">{t('downloads.aur.helperTitle')}</summary>
          <div className="terminal terminal--deps"><code><span className="terminal__prompt">$</span><span className="terminal__cmd">yay -S qbz-bin</span></code><CopyButton text="yay -S qbz-bin" /></div>
          <div className="terminal terminal--deps"><code><span className="terminal__prompt">$</span><span className="terminal__cmd">paru -S qbz-bin</span></code><CopyButton text="paru -S qbz-bin" /></div>
        </details>
        <div className="platform__actions">
          <a className="btn btn-ghost btn-sm" href={AUR_PACKAGE_URL} target="_blank" rel="noreferrer">{t('downloads.viewOn', { store: 'AUR' })}</a>
        </div>
      </div>
    </div>
  )
}

const APT_KEYRING_CMD = 'curl -fsSL https://vicrodh.github.io/qbz-apt/qbz-archive-keyring.gpg | gpg --dearmor | sudo tee /usr/share/keyrings/qbz-archive-keyring.gpg > /dev/null'
const APT_SOURCES_CMD = `cat <<EOF | sudo tee /etc/apt/sources.list.d/qbz.sources
Types: deb
URIs: https://vicrodh.github.io/qbz-apt
Suites: stable
Components: main
Architectures: $(dpkg --print-architecture)
Signed-By: /usr/share/keyrings/qbz-archive-keyring.gpg
EOF`
const APT_INSTALL_CMD = 'sudo apt update && sudo apt install qbz'
const DEB_DEPS_CMD = 'sudo apt install -y libasound2 libfontconfig1 libfreetype6 libxkbcommon0 libwayland-client0 libegl1 libgl1'
const RPM_DEPS_CMD = 'sudo dnf install -y alsa-lib fontconfig freetype libxkbcommon wayland mesa-libEGL mesa-libGL'

function DebianPanel({ items }: { items: DownloadItem[] }) {
  const { t } = useTranslation()
  return (
    <div className="download-list">
      <div className="download-item">
        <div className="download-item__header">
          <h4 className="download-item__label">{t('downloads.aptRepo.label')}</h4>
        </div>
        <p className="download-item__text">{t('downloads.aptRepo.description')}</p>
        <Cmd cmd={APT_KEYRING_CMD} />
        <Cmd cmd={APT_SOURCES_CMD} block />
        <Cmd cmd={APT_INSTALL_CMD} />
        <p className="glibc-note">{t('downloads.aptRepo.updateNote')}</p>
      </div>
      {items.map((item) => (
        <div className="download-item" key={item.fileName}>
          <div className="download-item__header">
            <h4 className="download-item__label">.deb</h4>
            <FileLine item={item} />
          </div>
          <Cmd cmd={`wget ${item.url}`} />
          <Cmd cmd={`sudo apt install ./${item.fileName}`} />
          <p className="glibc-note">{t('downloads.glibcNote.deb')}</p>
          <details className="deps-details">
            <summary className="deps-summary">{t('downloads.depsSummary')}</summary>
            <div className="terminal terminal--deps"><code><span className="terminal__prompt">#</span><span className="terminal__cmd">{DEB_DEPS_CMD}</span></code><CopyButton text={DEB_DEPS_CMD} /></div>
          </details>
          <DownloadLink item={item} label={t('downloads.download')} />
        </div>
      ))}
    </div>
  )
}

function FedoraPanel({ items }: { items: DownloadItem[] }) {
  const { t } = useTranslation()
  return (
    <div className="download-list">
      {items.map((item) => (
        <div className="download-item" key={item.fileName}>
          <div className="download-item__header">
            <h4 className="download-item__label">.rpm</h4>
            <FileLine item={item} />
          </div>
          <Cmd cmd={`wget ${item.url}`} />
          <Cmd cmd={`sudo dnf install ./${item.fileName}`} />
          <p className="glibc-note">{t('downloads.glibcNote.rpm')}</p>
          <details className="deps-details">
            <summary className="deps-summary">{t('downloads.depsSummary')}</summary>
            <div className="terminal terminal--deps"><code><span className="terminal__prompt">#</span><span className="terminal__cmd">{RPM_DEPS_CMD}</span></code><CopyButton text={RPM_DEPS_CMD} /></div>
          </details>
          <DownloadLink item={item} label={t('downloads.download')} />
        </div>
      ))}
    </div>
  )
}

const FLATPAK_RESERVE_CMD = 'flatpak override --user --own-name=org.freedesktop.ReserveDevice1.* com.blitzfc.qbz'
const FLATPAK_FS_CMDS = [
  'flatpak override --user --filesystem=/path/to/your/music com.blitzfc.qbz',
  'flatpak override --user --filesystem=/mnt/nas com.blitzfc.qbz',
]

function FlatpakPanel({ items }: { items: DownloadItem[] }) {
  const { t } = useTranslation()
  return (
    <div className="download-list">
      <div className="download-item">
        <div className="download-item__header">
          <h4 className="download-item__label">Flathub</h4>
        </div>
        <Cmd cmd="flatpak install flathub com.blitzfc.qbz" />
        <details className="deps-details">
          <summary className="deps-summary">{t('downloads.flatpak.bitperfectTitle')}</summary>
          <p>{t('downloads.flatpak.bitperfectNote')}</p>
          <div className="terminal terminal--deps"><code><span className="terminal__prompt">$</span><span className="terminal__cmd">{FLATPAK_RESERVE_CMD}</span></code><CopyButton text={FLATPAK_RESERVE_CMD} /></div>
        </details>
        <details className="deps-details">
          <summary className="deps-summary">{t('downloads.flatpak.libraryTitle')}</summary>
          {FLATPAK_FS_CMDS.map((cmd) => (
            <div className="terminal terminal--deps" key={cmd}><code><span className="terminal__prompt">$</span><span className="terminal__cmd">{cmd}</span></code><CopyButton text={cmd} /></div>
          ))}
        </details>
        <div className="platform__actions">
          <a className="btn btn-ghost btn-sm" href={FLATHUB_URL} target="_blank" rel="noreferrer">{t('downloads.viewOn', { store: 'Flathub' })}</a>
        </div>
      </div>
      {items.map((item) => (
        <div className="download-item" key={item.fileName}>
          <div className="download-item__header">
            <h4 className="download-item__label">GitHub Release · .flatpak</h4>
            <FileLine item={item} />
          </div>
          <Cmd cmd={`wget ${item.url}`} />
          <Cmd cmd={`flatpak install --user ./${item.fileName}`} />
          <DownloadLink item={item} label={t('downloads.download')} />
        </div>
      ))}
    </div>
  )
}

const SNAP_PLUGS = [
  'sudo snap connect qbz-player:alsa',
  'sudo snap connect qbz-player:pulseaudio',
  'sudo snap connect qbz-player:pipewire',
]

function SnapPanel() {
  const { t } = useTranslation()
  return (
    <div className="download-list">
      <div className="download-item">
        <div className="download-item__header">
          <h4 className="download-item__label">Snap Store · qbz-player</h4>
        </div>
        <Cmd cmd="sudo snap install qbz-player" />
        <details className="deps-details" open>
          <summary className="deps-summary">{t('downloads.snap.plugsTitle')}</summary>
          {SNAP_PLUGS.map((cmd) => (
            <div className="terminal terminal--deps" key={cmd}><code><span className="terminal__prompt">$</span><span className="terminal__cmd">{cmd}</span></code><CopyButton text={cmd} /></div>
          ))}
        </details>
        <details className="deps-details">
          <summary className="deps-summary">{t('downloads.snap.mediaTitle')}</summary>
          <p>{t('downloads.snap.mediaNote')}</p>
          <div className="terminal terminal--deps"><code><span className="terminal__prompt">$</span><span className="terminal__cmd">sudo snap connect qbz-player:removable-media</span></code><CopyButton text="sudo snap connect qbz-player:removable-media" /></div>
        </details>
        <div className="platform__actions">
          <a className="btn btn-ghost btn-sm" href={SNAP_STORE_URL} target="_blank" rel="noreferrer">{t('downloads.viewOn', { store: 'Snap Store' })}</a>
        </div>
      </div>
    </div>
  )
}

function AppImagePanel({ items }: { items: DownloadItem[] }) {
  const { t } = useTranslation()
  return (
    <div className="download-list">
      {items.map((item) => (
        <div className="download-item" key={item.fileName}>
          <div className="download-item__header">
            <h4 className="download-item__label">AppImage</h4>
            <FileLine item={item} />
          </div>
          <Cmd cmd={`wget ${item.url}`} />
          <Cmd cmd={`chmod +x ${item.fileName} && ./${item.fileName}`} />
          <DownloadLink item={item} label={t('downloads.download')} />
        </div>
      ))}
    </div>
  )
}

const NIXOS_FLAKE_INPUT = 'inputs.qbz.url = "github:vicrodh/qbz";'
const NIXOS_SYSTEM_PKG = `{pkgs, inputs, ...}:
{
  environment.systemPackages = [
    inputs.qbz.packages.\${pkgs.system}.default
  ];
}`
const NIXOS_HOME_PKG = `{pkgs, inputs, ...}:
{
  home.packages = [
    inputs.qbz.packages.\${pkgs.system}.default
  ];
}`

function NixOSPanel() {
  const { t } = useTranslation()
  return (
    <div className="download-list">
      <div className="download-item">
        <div className="download-item__header">
          <h4 className="download-item__label">{t('downloads.nixos.label')}</h4>
        </div>
        <p className="download-item__text">{t('downloads.nixos.lead')}</p>
        <Cmd cmd={NIXOS_FLAKE_INPUT} prompt="" />
        <p className="platform__sub">{t('downloads.nixos.system')}</p>
        <Cmd cmd={NIXOS_SYSTEM_PKG} prompt="" block />
        <p className="platform__sub">{t('downloads.nixos.home')}</p>
        <Cmd cmd={NIXOS_HOME_PKG} prompt="" block />
        <p className="glibc-note">
          {t('downloads.nixos.nixpkgs')} <code>qbz</code> · <a href="https://search.nixos.org/packages?query=qbz" target="_blank" rel="noreferrer">search.nixos.org</a>
        </p>
      </div>
    </div>
  )
}

const GENTOO_OVERLAY_CMDS = [
  'eselect repository add qbz-overlay git https://github.com/vicrodh/qbz-overlay.git',
  'emerge --sync qbz-overlay',
]

function GentooPanel() {
  const { t } = useTranslation()
  return (
    <div className="download-list">
      <div className="download-item">
        <div className="download-item__header">
          <h4 className="download-item__label">{t('downloads.gentoo.label')}</h4>
        </div>
        <p className="download-item__text">{t('downloads.gentoo.lead')}</p>
        {GENTOO_OVERLAY_CMDS.map((cmd) => <Cmd key={cmd} cmd={cmd} prompt="#" />)}
        <p className="platform__sub">{t('downloads.gentoo.binTitle')}</p>
        <Cmd cmd="emerge media-sound/qbz-bin" prompt="#" />
        <details className="deps-details">
          <summary className="deps-summary">{t('downloads.gentoo.srcTitle')}</summary>
          <div className="terminal terminal--deps"><code><span className="terminal__prompt">#</span><span className="terminal__cmd">emerge media-sound/qbz</span></code><CopyButton text="emerge media-sound/qbz" /></div>
        </details>
        <div className="platform__actions">
          <a className="btn btn-ghost btn-sm" href={GENTOO_OVERLAY_URL} target="_blank" rel="noreferrer">{t('downloads.gentoo.viewOverlay')}</a>
        </div>
      </div>
    </div>
  )
}

function TarballPanel({ items }: { items: DownloadItem[] }) {
  const { t } = useTranslation()
  return (
    <div className="download-list">
      {items.map((item) => {
        const dir = stripArchive(item.fileName)
        const desktopCmds = [
          `sudo cp ${dir}/qbz /usr/local/bin/`,
          `cp ${dir}/qbz.desktop ~/.local/share/applications/`,
          `cp -r ${dir}/icons/* ~/.local/share/icons/`,
          'gtk-update-icon-cache ~/.local/share/icons/hicolor/',
        ]
        return (
          <div className="download-item" key={item.fileName}>
            <div className="download-item__header">
              <h4 className="download-item__label">Tarball</h4>
              <FileLine item={item} />
            </div>
            <Cmd cmd={`wget ${item.url}`} />
            <Cmd cmd={`tar -xzf ${item.fileName} && ./${dir}/qbz`} />
            <details className="deps-details">
              <summary className="deps-summary">{t('downloads.tarball.desktopTitle')}</summary>
              {desktopCmds.map((cmd) => (
                <div className="terminal terminal--deps" key={cmd}><code><span className="terminal__prompt">$</span><span className="terminal__cmd">{cmd}</span></code><CopyButton text={cmd} /></div>
              ))}
            </details>
            <DownloadLink item={item} label={t('downloads.download')} />
          </div>
        )
      })}
    </div>
  )
}

function QbzdPanel({ items }: { items: DownloadItem[] }) {
  const { t } = useTranslation()
  return (
    <div className="download-list">
      <div className="download-item">
        <div className="download-item__header">
          <h4 className="download-item__label">{t('downloads.linux.qbzdTitle')}</h4>
        </div>
        <p className="download-item__text">{t('downloads.linux.qbzdNote')}</p>
        <div className="platform__actions">
          <a className="btn btn-ghost btn-sm" href={QBZD_MANUAL_URL} target="_blank" rel="noreferrer">{t('daemon.cta')}</a>
        </div>
      </div>
      {items.map((item) => (
        <div className="download-item" key={item.fileName}>
          <div className="download-item__header">
            <h4 className="download-item__label">qbzd</h4>
            <FileLine item={item} />
          </div>
          <Cmd cmd={`wget ${item.url}`} />
          <Cmd cmd={`tar -xzf ${item.fileName}`} />
          <DownloadLink item={item} label={t('downloads.download')} />
        </div>
      ))}
    </div>
  )
}

const BUILD_DEPS_DEBIAN = 'sudo apt install build-essential pkg-config cmake clang libclang-dev nasm qt6-base-dev qt6-base-private-dev qt6-declarative-dev qt6-declarative-private-dev qt6-shadertools-dev libasound2-dev libjack-jackd2-dev libdbus-1-dev libssl-dev'
const BUILD_DEPS_MACOS = 'xcode-select --install && brew install qt'
const BUILD_RUSTUP = "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
const BUILD_CLONE = 'git clone https://github.com/vicrodh/qbz.git && cd qbz'
const BUILD_CARGO = 'cargo build --release --manifest-path crates/Cargo.toml -p qbz-qt'
const BUILD_RUN = './crates/target/release/qbz'

function SourcePanel() {
  const { t } = useTranslation()
  return (
    <div className="download-list">
      <div className="download-item">
        <div className="download-item__header">
          <h4 className="download-item__label">{t('downloads.buildTitle')}</h4>
        </div>
        <p className="download-item__text">{t('downloads.buildBody')}</p>
        <p className="platform__sub">{t('downloads.buildInstructions.prereqTitle')}</p>
        <p className="download-item__text">{t('downloads.buildInstructions.prereqNote')}</p>
        <details className="deps-details">
          <summary className="deps-summary">Debian / Ubuntu</summary>
          <div className="terminal terminal--deps"><code><span className="terminal__prompt">$</span><span className="terminal__cmd">{BUILD_DEPS_DEBIAN}</span></code><CopyButton text={BUILD_DEPS_DEBIAN} /></div>
        </details>
        <details className="deps-details">
          <summary className="deps-summary">macOS</summary>
          <div className="terminal terminal--deps"><code><span className="terminal__prompt">$</span><span className="terminal__cmd">{BUILD_DEPS_MACOS}</span></code><CopyButton text={BUILD_DEPS_MACOS} /></div>
        </details>
        <details className="deps-details">
          <summary className="deps-summary">Rust</summary>
          <div className="terminal terminal--deps"><code><span className="terminal__prompt">$</span><span className="terminal__cmd">{BUILD_RUSTUP}</span></code><CopyButton text={BUILD_RUSTUP} /></div>
        </details>
        <p className="platform__sub">{t('downloads.buildInstructions.cloneTitle')}</p>
        <Cmd cmd={BUILD_CLONE} />
        <Cmd cmd={BUILD_CARGO} />
        <Cmd cmd={BUILD_RUN} />
        <p className="glibc-note">{t('downloads.buildInstructions.buildNote')}</p>
        <p className="platform__sub">{t('downloads.buildInstructions.proxyTitle')}</p>
        <p className="download-item__text">{t('downloads.buildInstructions.proxyNote')}</p>
      </div>
    </div>
  )
}

/* ── Platform cards ───────────────────────────────────────── */

function ReleaseMeta({ release, error }: { release: ReleaseData | null; error: boolean }) {
  const { t } = useTranslation()
  const { language } = useApp()
  if (release) {
    return (
      <span className="platform__release">
        {t('downloads.versionLabel')} {release.tag_name}
        <br />
        {formatDate(release.published_at, language)}
      </span>
    )
  }
  return <span className="platform__release">{error ? t('downloads.error') : t('downloads.loading')}</span>
}

function LinuxCard({ items, release, error }: { items: DownloadItem[]; release: ReleaseData | null; error: boolean }) {
  const { t } = useTranslation()
  const [activeTab, setActiveTab] = useState<TabId>('arch')
  const loading = !release && !error

  const handleTabKeyDown = useCallback((event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let nextIndex: number | null = null
    if (event.key === 'ArrowRight') nextIndex = (index + 1) % TABS.length
    if (event.key === 'ArrowLeft') nextIndex = (index - 1 + TABS.length) % TABS.length
    if (event.key === 'Home') nextIndex = 0
    if (event.key === 'End') nextIndex = TABS.length - 1
    if (nextIndex === null) return
    event.preventDefault()
    const nextTab = TABS[nextIndex]
    setActiveTab(nextTab.id)
    document.getElementById(`download-tab-${nextTab.id}`)?.focus()
  }, [])

  const byType = (type: AssetType) => items.filter((item) => item.type === type)

  const emptyState = (list: DownloadItem[]) =>
    list.length === 0 ? (
      <p className="download-state">{loading ? t('downloads.loading') : error ? t('downloads.error') : t('downloads.noPackages')}</p>
    ) : null

  const renderPanel = () => {
    switch (activeTab) {
      case 'arch': return <ArchPanel />
      case 'debian': return <DebianPanel items={byType('deb')} />
      case 'fedora': return emptyState(byType('rpm')) ?? <FedoraPanel items={byType('rpm')} />
      case 'flatpak': return <FlatpakPanel items={byType('flatpak')} />
      case 'snap': return <SnapPanel />
      case 'appimage': return emptyState(byType('appimage')) ?? <AppImagePanel items={byType('appimage')} />
      case 'nixos': return <NixOSPanel />
      case 'gentoo': return <GentooPanel />
      case 'tarball': return emptyState(byType('tarball')) ?? <TarballPanel items={byType('tarball')} />
      case 'qbzd': return <QbzdPanel items={byType('qbzd')} />
      case 'source': return <SourcePanel />
    }
  }

  return (
    <article className="platform platform--linux" id="download-linux" aria-labelledby="platform-linux">
      <header className="platform__head">
        <span className="platform__logo"><img src="/assets/icons/Tux.svg" alt="" width={36} height={36} /></span>
        <div>
          <h3 id="platform-linux" className="platform__name">{t('downloads.linux.name')}</h3>
          <div className="platform__tier">{t('downloads.linux.tier')}</div>
        </div>
        <ReleaseMeta release={release} error={error} />
      </header>
      <div className="platform__body" style={{ paddingBottom: 0 }}>
        <p className="platform__note">{t('downloads.linux.note')}</p>
      </div>
      <div className="download-tabs" role="tablist" aria-label={t('downloads.linux.name')}>
        {TABS.map((tab, index) => (
          <button
            key={tab.id}
            id={`download-tab-${tab.id}`}
            type="button"
            role="tab"
            aria-controls="download-tabpanel"
            aria-selected={activeTab === tab.id}
            tabIndex={activeTab === tab.id ? 0 : -1}
            className={`download-tab${activeTab === tab.id ? ' download-tab--active' : ''}`}
            onClick={() => setActiveTab(tab.id)}
            onKeyDown={(event) => handleTabKeyDown(event, index)}
          >
            <TabIcon id={tab.id} src={tab.icon} />
            <span>{tab.label}</span>
          </button>
        ))}
      </div>
      <div id="download-tabpanel" className="download-tab-content" role="tabpanel" aria-labelledby={`download-tab-${activeTab}`}>
        {renderPanel()}
      </div>
    </article>
  )
}

function AppleLogo() {
  return <img src="/icons/apple.svg" alt="" width={36} height={36} />
}

function WindowsLogo() {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" style={{ color: 'var(--ink-2)' }}>
      <path d="M3 5.5l7.5-1v7H3v-6zm8.5-1.2L21 3v8.5h-9.5v-7.2zM3 12.5h7.5v7L3 18.5v-6zm8.5 0H21V21l-9.5-1.3v-7.2z" />
    </svg>
  )
}

function MacCard({ items, signedTag, loading, error }: { items: DownloadItem[]; signedTag: string | null; loading: boolean; error: boolean }) {
  const { t } = useTranslation()
  const dmgs = items.filter((item) => item.type === 'dmg')
  return (
    <article className="platform platform--macos" id="download-macos" aria-labelledby="platform-macos">
      <header className="platform__head">
        <span className="platform__logo"><AppleLogo /></span>
        <div>
          <h3 id="platform-macos" className="platform__name">{t('downloads.macos.name')}</h3>
          <div className="platform__tier">{t('downloads.macos.tier')}</div>
        </div>
        <span className="platform__release">{signedTag ? t('downloads.macos.signedVersion', { version: signedTag }) : t('downloads.macos.signedVersionUnknown')}</span>
      </header>
      <div className="platform__body">
        <p className="platform__disclaimer">{t('downloads.macos.disclaimer')}</p>
        <p className="platform__note">{t('downloads.macos.limitations')}</p>
        <p className="platform__sub">{t('downloads.macos.homebrewTitle')}</p>
        <Cmd cmd={HOMEBREW_QBZ_COMMAND} />
        <div className="platform__actions">
          <a className="btn btn-ghost btn-sm" href={SIGNED_MACOS_RELEASES_URL} target="_blank" rel="noreferrer">{t('downloads.macos.downloadSigned')}</a>
          <a className="btn btn-ghost btn-sm" href={SIGNED_MACOS_PROVENANCE_URL} target="_blank" rel="noreferrer">{t('downloads.macos.reviewProvenance')}</a>
          <a className="btn btn-ghost btn-sm" href={HOMEBREW_QBZ_URL} target="_blank" rel="noreferrer">{t('downloads.macos.viewCask')}</a>
        </div>
        <details className="deps-details">
          <summary className="deps-summary">{t('downloads.macos.upstreamTitle')}</summary>
          <p>{t('downloads.macos.upstreamNote')}</p>
          {dmgs.map((item) => (
            <div key={item.fileName} style={{ marginTop: 10 }}>
              <FileLine item={item} />
              <div className="platform__actions" style={{ marginTop: 6 }}>
                <a className="btn btn-ghost btn-sm" href={item.url}>{t('downloads.macos.downloadUpstream')}</a>
              </div>
            </div>
          ))}
          {dmgs.length === 0 && (
            <p className="download-state">{loading ? t('downloads.loading') : error ? t('downloads.error') : t('downloads.macos.noUpstreamDmg')}</p>
          )}
          <p style={{ marginTop: 12 }}><strong style={{ color: 'var(--ink)' }}>{t('downloads.macos.unlockTitle')}</strong></p>
          <p>{t('downloads.macos.unlockNote')}</p>
          <div className="terminal terminal--deps"><code><span className="terminal__prompt">$</span><span className="terminal__cmd">xattr -dr com.apple.quarantine /Applications/QBZ.app</span></code><CopyButton text="xattr -dr com.apple.quarantine /Applications/QBZ.app" /></div>
        </details>
      </div>
    </article>
  )
}

function WindowsCard({ items, release, loading, error }: { items: DownloadItem[]; release: ReleaseData | null; loading: boolean; error: boolean }) {
  const { t } = useTranslation()
  const msis = items.filter((item) => item.type === 'msi')
  return (
    <article className="platform platform--windows" id="download-windows" aria-labelledby="platform-windows">
      <header className="platform__head">
        <span className="platform__logo"><WindowsLogo /></span>
        <div>
          <h3 id="platform-windows" className="platform__name">{t('downloads.windows.name')}</h3>
          <div className="platform__tier">{t('downloads.windows.tier')}</div>
        </div>
        <span className="platform__release">{release && msis.length > 0 ? `${t('downloads.versionLabel')} ${release.tag_name}` : ''}</span>
      </header>
      <div className="platform__body">
        <p className="platform__disclaimer"><strong>{t('downloads.windows.disclaimer')}</strong></p>
        <p className="platform__note">{t('downloads.windows.note')}</p>
        <p className="platform__sub">{t('downloads.windows.installTitle')}</p>
        {msis.length > 0 ? (
          <>
            <p className="platform__note">{t('downloads.windows.installNote')} {t('downloads.windows.requirements')}</p>
            {msis.map((item) => (
              <div key={item.fileName}>
                <FileLine item={item} />
                <div className="platform__actions" style={{ marginTop: 8 }}>
                  <a className="btn btn-primary btn-sm" href={item.url}>{t('downloads.download')}</a>
                </div>
              </div>
            ))}
          </>
        ) : (
          <p className="download-state">{loading ? t('downloads.loading') : error ? t('downloads.error') : t('downloads.windows.noMsi')}</p>
        )}
        <div className="platform__actions">
          <a className="btn btn-ghost btn-sm" href={WINDOWS_ADOPT_URL} target="_blank" rel="noreferrer">{t('downloads.windows.adopt')}</a>
        </div>
      </div>
    </article>
  )
}

/* ── Section ──────────────────────────────────────────────── */

export function DownloadSection() {
  const { t } = useTranslation()
  const [release, setRelease] = useState<ReleaseData | null>(null)
  const [signedReleaseTag, setSignedReleaseTag] = useState<string | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let active = true
    fetch(RELEASES_URL)
      .then((res) => (res.ok ? res.json() : Promise.reject(new Error('release fetch failed'))))
      .then((data: ReleaseData[]) => {
        if (!active) return
        const found = data.find((r) => r.assets && r.assets.length > 0 && !r.prerelease && !r.draft)
        if (found) setRelease(found)
        else setError(true)
      })
      .catch(() => { if (active) setError(true) })

    fetch(SIGNED_MACOS_RELEASE_API_URL)
      .then((res) => (res.ok ? res.json() : Promise.reject(new Error('signed release fetch failed'))))
      .then((data: ReleaseData) => {
        if (active && !data.draft && !data.prerelease) setSignedReleaseTag(data.tag_name)
      })
      .catch(() => {})

    return () => { active = false }
  }, [])

  const items = useMemo(() => (release ? mapAssets(release.assets) : []), [release])
  const loading = !release && !error

  return (
    <section id="downloads" className="section" aria-labelledby="downloads-title">
      <div className="container">
        <div className="section__head">
          <span className="eyebrow">{t('downloads.eyebrow')}</span>
          <h2 id="downloads-title" className="section__title">{t('downloads.title')}</h2>
          <p className="section__subtitle">{t('downloads.lead')}</p>
        </div>
        <div className="pyramid">
          <LinuxCard items={items} release={release} error={error} />
          <MacCard items={items} signedTag={signedReleaseTag} loading={loading} error={error} />
          <WindowsCard items={items} release={release} loading={loading} error={error} />
        </div>
        <div className="platform__actions" style={{ marginTop: 24 }}>
          <a className="btn btn-ghost" href={release?.html_url ?? RELEASES_PAGE} target="_blank" rel="noreferrer">{t('downloads.viewAll')}</a>
        </div>
      </div>
    </section>
  )
}
