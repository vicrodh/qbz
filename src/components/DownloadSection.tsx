import { useEffect, useMemo, useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { useApp } from '../lib/appContext'
import { formatBytes, formatDate } from '../lib/format'
import { Dropdown, type DropdownOption } from './Dropdown'

/* ── Copy button + command line ───────────────────────────── */

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

function Details({ title, children, open }: { title: string; children: React.ReactNode; open?: boolean }) {
  return (
    <details className="deps-details" open={open}>
      <summary className="deps-summary">{title}</summary>
      {children}
    </details>
  )
}

/* ── Release data ─────────────────────────────────────────── */

type ReleaseAsset = { name: string; browser_download_url: string; size: number }
type ReleaseData = {
  tag_name: string
  published_at: string
  assets: ReleaseAsset[]
  html_url: string
  prerelease: boolean
  draft: boolean
}

type AssetType = 'appimage' | 'flatpak' | 'deb' | 'rpm' | 'tarball' | 'qbzd' | 'qbzd-deb' | 'qbzd-rpm' | 'dmg' | 'msi' | 'unknown'
type DownloadItem = { type: AssetType; fileName: string; url: string; size: number; arch: string | null }

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
  if (lower.endsWith('.sig') || lower === 'latest.json' || lower.includes('.app.tar.gz')) return 'unknown'
  if (lower.startsWith('qbzd')) {
    if (lower.endsWith('.tar.gz') || lower.endsWith('.tar.xz')) return 'qbzd'
    if (lower.endsWith('.deb')) return 'qbzd-deb'
    if (lower.endsWith('.rpm')) return 'qbzd-rpm'
    return 'unknown'
  }
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

function DownloadLink({ item, label, primary = false }: { item: DownloadItem; label: string; primary?: boolean }) {
  return (
    <div className="platform__actions">
      <a className={`btn btn-sm ${primary ? 'btn-primary' : 'btn-ghost'}`} href={item.url}>{label}</a>
    </div>
  )
}

function EmptyState({ loading, error, text }: { loading: boolean; error: boolean; text: string }) {
  const { t } = useTranslation()
  return <p className="download-state">{loading ? t('downloads.loading') : error ? t('downloads.error') : text}</p>
}

/* ── Linux formats ────────────────────────────────────────── */

type LinuxFormat = 'arch' | 'debian' | 'fedora' | 'flatpak' | 'snap' | 'appimage' | 'nixos' | 'gentoo' | 'tarball' | 'source'

const LINUX_FORMATS: { id: LinuxFormat; icon: string }[] = [
  { id: 'arch', icon: '/icons/arch.svg' },
  { id: 'debian', icon: '/icons/debian.svg' },
  { id: 'fedora', icon: '/icons/redhat.svg' },
  { id: 'flatpak', icon: '/icons/flatpak.svg' },
  { id: 'snap', icon: '/icons/snapcraft.svg' },
  { id: 'appimage', icon: '/icons/appimage.svg' },
  { id: 'nixos', icon: '/icons/nixos.svg' },
  { id: 'gentoo', icon: '/icons/gentoo.svg' },
  { id: 'tarball', icon: '/icons/tarball.svg' },
  { id: 'source', icon: '/icons/rust.svg' },
]

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
const FLATPAK_RESERVE_CMD = 'flatpak override --user --own-name=org.freedesktop.ReserveDevice1.* com.blitzfc.qbz'
const FLATPAK_FS_CMDS = [
  'flatpak override --user --filesystem=/path/to/your/music com.blitzfc.qbz',
  'flatpak override --user --filesystem=/mnt/nas com.blitzfc.qbz',
]
const SNAP_PLUGS = ['sudo snap connect qbz-player:alsa', 'sudo snap connect qbz-player:pulseaudio', 'sudo snap connect qbz-player:pipewire']
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
const GENTOO_OVERLAY_CMDS = ['eselect repository add qbz-overlay git https://github.com/vicrodh/qbz-overlay.git', 'emerge --sync qbz-overlay']
const BUILD_DEPS_DEBIAN = 'sudo apt install build-essential pkg-config cmake clang libclang-dev nasm qt6-base-dev qt6-base-private-dev qt6-declarative-dev qt6-declarative-private-dev qt6-shadertools-dev libasound2-dev libjack-jackd2-dev libdbus-1-dev libssl-dev'
const BUILD_DEPS_MACOS = 'xcode-select --install && brew install qt'
const BUILD_RUSTUP = "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
const BUILD_CLONE = 'git clone https://github.com/vicrodh/qbz.git && cd qbz'
const BUILD_CARGO = 'cargo build --release --manifest-path crates/Cargo.toml -p qbz-qt'
const BUILD_RUN = './crates/target/release/qbz'

function DepsCmd({ cmd, prompt = '$' }: { cmd: string; prompt?: string }) {
  return (
    <div className="terminal terminal--deps">
      <code><span className="terminal__prompt">{prompt}</span><span className="terminal__cmd">{cmd}</span></code>
      <CopyButton text={cmd} />
    </div>
  )
}

function LinuxPanel({ format, items, loading, error }: { format: LinuxFormat; items: DownloadItem[]; loading: boolean; error: boolean }) {
  const { t } = useTranslation()
  const byType = (type: AssetType) => items.filter((item) => item.type === type)
  const empty = (list: DownloadItem[]) => (list.length === 0 ? <EmptyState loading={loading} error={error} text={t('downloads.noPackages')} /> : null)

  switch (format) {
    case 'arch':
      return (
        <div className="download-item">
          <Cmd cmd="git clone https://aur.archlinux.org/qbz-bin.git && cd qbz-bin && makepkg -si" />
          <Details title={t('downloads.aur.helperTitle')}>
            <DepsCmd cmd="yay -S qbz-bin" />
            <DepsCmd cmd="paru -S qbz-bin" />
          </Details>
          <div className="platform__actions">
            <a className="btn btn-ghost btn-sm" href={AUR_PACKAGE_URL} target="_blank" rel="noreferrer">{t('downloads.viewOn', { store: 'AUR' })}</a>
          </div>
        </div>
      )
    case 'debian':
      return (
        <div className="download-list">
          <div className="download-item">
            <h4 className="download-item__label">{t('downloads.aptRepo.label')}</h4>
            <p className="download-item__text">{t('downloads.aptRepo.description')}</p>
            <Cmd cmd={APT_KEYRING_CMD} />
            <Cmd cmd={APT_SOURCES_CMD} block />
            <Cmd cmd={APT_INSTALL_CMD} />
            <p className="glibc-note">{t('downloads.aptRepo.updateNote')}</p>
          </div>
          {byType('deb').map((item) => (
            <div className="download-item" key={item.fileName}>
              <div className="download-item__header"><h4 className="download-item__label">.deb</h4><FileLine item={item} /></div>
              <Cmd cmd={`wget ${item.url}`} />
              <Cmd cmd={`sudo apt install ./${item.fileName}`} />
              <p className="glibc-note">{t('downloads.glibcNote.deb')}</p>
              <Details title={t('downloads.depsSummary')}><DepsCmd cmd={DEB_DEPS_CMD} prompt="#" /></Details>
              <DownloadLink item={item} label={t('downloads.download')} />
            </div>
          ))}
        </div>
      )
    case 'fedora':
      return (
        <div className="download-list">
          {empty(byType('rpm'))}
          {byType('rpm').map((item) => (
            <div className="download-item" key={item.fileName}>
              <div className="download-item__header"><h4 className="download-item__label">.rpm</h4><FileLine item={item} /></div>
              <Cmd cmd={`wget ${item.url}`} />
              <Cmd cmd={`sudo dnf install ./${item.fileName}`} />
              <p className="glibc-note">{t('downloads.glibcNote.rpm')}</p>
              <Details title={t('downloads.depsSummary')}><DepsCmd cmd={RPM_DEPS_CMD} prompt="#" /></Details>
              <DownloadLink item={item} label={t('downloads.download')} />
            </div>
          ))}
        </div>
      )
    case 'flatpak':
      return (
        <div className="download-list">
          <div className="download-item">
            <h4 className="download-item__label">Flathub</h4>
            <Cmd cmd="flatpak install flathub com.blitzfc.qbz" />
            <Details title={t('downloads.flatpak.bitperfectTitle')}>
              <p>{t('downloads.flatpak.bitperfectNote')}</p>
              <DepsCmd cmd={FLATPAK_RESERVE_CMD} />
            </Details>
            <Details title={t('downloads.flatpak.libraryTitle')}>
              {FLATPAK_FS_CMDS.map((cmd) => <DepsCmd key={cmd} cmd={cmd} />)}
            </Details>
            <div className="platform__actions">
              <a className="btn btn-ghost btn-sm" href={FLATHUB_URL} target="_blank" rel="noreferrer">{t('downloads.viewOn', { store: 'Flathub' })}</a>
            </div>
          </div>
          {byType('flatpak').map((item) => (
            <div className="download-item" key={item.fileName}>
              <div className="download-item__header"><h4 className="download-item__label">GitHub Release · .flatpak</h4><FileLine item={item} /></div>
              <Cmd cmd={`wget ${item.url}`} />
              <Cmd cmd={`flatpak install --user ./${item.fileName}`} />
              <DownloadLink item={item} label={t('downloads.download')} />
            </div>
          ))}
        </div>
      )
    case 'snap':
      return (
        <div className="download-item">
          <Cmd cmd="sudo snap install qbz-player" />
          <Details title={t('downloads.snap.plugsTitle')} open>
            {SNAP_PLUGS.map((cmd) => <DepsCmd key={cmd} cmd={cmd} />)}
          </Details>
          <Details title={t('downloads.snap.mediaTitle')}>
            <p>{t('downloads.snap.mediaNote')}</p>
            <DepsCmd cmd="sudo snap connect qbz-player:removable-media" />
          </Details>
          <div className="platform__actions">
            <a className="btn btn-ghost btn-sm" href={SNAP_STORE_URL} target="_blank" rel="noreferrer">{t('downloads.viewOn', { store: 'Snap Store' })}</a>
          </div>
        </div>
      )
    case 'appimage':
      return (
        <div className="download-list">
          {empty(byType('appimage'))}
          {byType('appimage').map((item) => (
            <div className="download-item" key={item.fileName}>
              <div className="download-item__header"><h4 className="download-item__label">AppImage</h4><FileLine item={item} /></div>
              <Cmd cmd={`wget ${item.url}`} />
              <Cmd cmd={`chmod +x ${item.fileName} && ./${item.fileName}`} />
              <DownloadLink item={item} label={t('downloads.download')} />
            </div>
          ))}
        </div>
      )
    case 'nixos':
      return (
        <div className="download-item">
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
      )
    case 'gentoo':
      return (
        <div className="download-item">
          <p className="download-item__text">{t('downloads.gentoo.lead')}</p>
          {GENTOO_OVERLAY_CMDS.map((cmd) => <Cmd key={cmd} cmd={cmd} prompt="#" />)}
          <p className="platform__sub">{t('downloads.gentoo.binTitle')}</p>
          <Cmd cmd="emerge media-sound/qbz-bin" prompt="#" />
          <Details title={t('downloads.gentoo.srcTitle')}><DepsCmd cmd="emerge media-sound/qbz" prompt="#" /></Details>
          <div className="platform__actions">
            <a className="btn btn-ghost btn-sm" href={GENTOO_OVERLAY_URL} target="_blank" rel="noreferrer">{t('downloads.gentoo.viewOverlay')}</a>
          </div>
        </div>
      )
    case 'tarball':
      return (
        <div className="download-list">
          {empty(byType('tarball'))}
          {byType('tarball').map((item) => {
            const dir = stripArchive(item.fileName)
            const desktopCmds = [
              `sudo cp ${dir}/qbz /usr/local/bin/`,
              `cp ${dir}/qbz.desktop ~/.local/share/applications/`,
              `cp -r ${dir}/icons/* ~/.local/share/icons/`,
              'gtk-update-icon-cache ~/.local/share/icons/hicolor/',
            ]
            return (
              <div className="download-item" key={item.fileName}>
                <div className="download-item__header"><h4 className="download-item__label">Tarball</h4><FileLine item={item} /></div>
                <Cmd cmd={`wget ${item.url}`} />
                <Cmd cmd={`tar -xzf ${item.fileName} && ./${dir}/qbz`} />
                <Details title={t('downloads.tarball.desktopTitle')}>
                  {desktopCmds.map((cmd) => <DepsCmd key={cmd} cmd={cmd} />)}
                </Details>
                <DownloadLink item={item} label={t('downloads.download')} />
              </div>
            )
          })}
        </div>
      )
    case 'source':
      return (
        <div className="download-item">
          <p className="download-item__text">{t('downloads.buildBody')}</p>
          <p className="platform__sub">{t('downloads.buildInstructions.prereqTitle')}</p>
          <p className="download-item__text">{t('downloads.buildInstructions.prereqNote')}</p>
          <Details title="Debian / Ubuntu"><DepsCmd cmd={BUILD_DEPS_DEBIAN} /></Details>
          <Details title="macOS"><DepsCmd cmd={BUILD_DEPS_MACOS} /></Details>
          <Details title="Rust"><DepsCmd cmd={BUILD_RUSTUP} /></Details>
          <p className="platform__sub">{t('downloads.buildInstructions.cloneTitle')}</p>
          <Cmd cmd={BUILD_CLONE} />
          <Cmd cmd={BUILD_CARGO} />
          <Cmd cmd={BUILD_RUN} />
          <p className="glibc-note">{t('downloads.buildInstructions.buildNote')}</p>
          <p className="platform__sub">{t('downloads.buildInstructions.proxyTitle')}</p>
          <p className="download-item__text">{t('downloads.buildInstructions.proxyNote')}</p>
        </div>
      )
  }
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

function PlatformHead({ id, logo, name, tier, meta }: { id: string; logo: React.ReactNode; name: string; tier: string; meta?: React.ReactNode }) {
  return (
    <header className="platform__head">
      <span className="platform__logo">{logo}</span>
      <div>
        <h3 id={id} className="platform__name">{name}</h3>
        <div className="platform__tier">{tier}</div>
      </div>
      {meta ?? <span />}
    </header>
  )
}

function LinuxCard({ items, release, error }: { items: DownloadItem[]; release: ReleaseData | null; error: boolean }) {
  const { t } = useTranslation()
  const [format, setFormat] = useState<LinuxFormat>('arch')
  const options: DropdownOption[] = LINUX_FORMATS.map((f) => ({ id: f.id, label: t(`downloads.formats.${f.id}`), icon: f.icon }))

  return (
    <article className="platform platform--linux" id="download-linux" aria-labelledby="platform-linux">
      <PlatformHead
        id="platform-linux"
        logo={<img className="platform__logo--color" src="/assets/icons/Tux.svg" alt="" width={36} height={36} />}
        name={t('downloads.linux.name')}
        tier={t('downloads.linux.tier')}
        meta={<ReleaseMeta release={release} error={error} />}
      />
      <div className="platform__body">
        <p className="platform__note">{t('downloads.linux.note')}</p>
        <Dropdown options={options} value={format} onChange={(id) => setFormat(id as LinuxFormat)} ariaLabel={t('downloads.choose')} />
        <LinuxPanel format={format} items={items} loading={!release && !error} error={error} />
      </div>
    </article>
  )
}

type QbzdFormat = 'tarball' | 'deb' | 'rpm'

const QBZD_FORMATS: { id: QbzdFormat; type: AssetType; icon: string }[] = [
  { id: 'tarball', type: 'qbzd', icon: '/icons/tarball.svg' },
  { id: 'deb', type: 'qbzd-deb', icon: '/icons/debian.svg' },
  { id: 'rpm', type: 'qbzd-rpm', icon: '/icons/redhat.svg' },
]

function QbzdCard({ items, release, error }: { items: DownloadItem[]; release: ReleaseData | null; error: boolean }) {
  const { t } = useTranslation()
  const available = QBZD_FORMATS.filter((f) => items.some((item) => item.type === f.type))
  const [format, setFormat] = useState<QbzdFormat>('tarball')
  const current = available.find((f) => f.id === format) ?? available[0]
  const options: DropdownOption[] = available.map((f) => ({ id: f.id, label: t(`downloads.formats.${f.id}`), icon: f.icon }))
  const list = current ? items.filter((item) => item.type === current.type) : []

  const install = (item: DownloadItem) => {
    if (item.type === 'qbzd-deb') return `sudo apt install ./${item.fileName}`
    if (item.type === 'qbzd-rpm') return `sudo dnf install ./${item.fileName}`
    return `tar -xzf ${item.fileName}`
  }

  return (
    <article className="platform platform--qbzd" id="download-qbzd" aria-labelledby="platform-qbzd">
      <PlatformHead
        id="platform-qbzd"
        logo={(
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <rect x="3" y="5" width="18" height="14" rx="2" />
            <path d="M7 10l3 2-3 2M12 14h5" />
          </svg>
        )}
        name={t('downloads.qbzd.name')}
        tier={t('downloads.qbzd.tier')}
        meta={release ? <span className="platform__release">{t('downloads.versionLabel')} {release.tag_name}</span> : undefined}
      />
      <div className="platform__body">
        <p className="platform__note">{t('downloads.qbzd.note')}</p>
        {options.length > 1 && (
          <Dropdown options={options} value={current?.id ?? 'tarball'} onChange={(id) => setFormat(id as QbzdFormat)} ariaLabel={t('downloads.choose')} />
        )}
        {list.length === 0 && <EmptyState loading={!release && !error} error={error} text={t('downloads.qbzd.noAssets')} />}
        <div className="download-list">
          {list.map((item) => (
            <div className="download-item" key={item.fileName}>
              <FileLine item={item} />
              <Cmd cmd={`wget ${item.url}`} />
              <Cmd cmd={install(item)} />
              <DownloadLink item={item} label={t('downloads.download')} />
            </div>
          ))}
        </div>
        <div className="platform__actions">
          <a className="btn btn-ghost btn-sm" href={QBZD_MANUAL_URL} target="_blank" rel="noreferrer">{t('downloads.qbzd.manual')}</a>
        </div>
      </div>
    </article>
  )
}

type MacFormat = 'homebrew' | 'signed' | 'unsigned'

const MAC_FORMATS: { id: MacFormat; icon: string }[] = [
  { id: 'homebrew', icon: '/icons/terminal.svg' },
  { id: 'signed', icon: '/icons/dmg.svg' },
  { id: 'unsigned', icon: '/icons/dmg.svg' },
]

function MacCard({ items, signedTag, loading, error }: { items: DownloadItem[]; signedTag: string | null; loading: boolean; error: boolean }) {
  const { t } = useTranslation()
  const [format, setFormat] = useState<MacFormat>('homebrew')
  const dmgs = items.filter((item) => item.type === 'dmg')
  const options: DropdownOption[] = MAC_FORMATS.map((f) => ({ id: f.id, label: t(`downloads.formats.${f.id}`), icon: f.icon }))

  return (
    <article className="platform platform--macos" id="download-macos" aria-labelledby="platform-macos">
      <PlatformHead
        id="platform-macos"
        logo={<img src="/icons/apple.svg" alt="" width={36} height={36} />}
        name={t('downloads.macos.name')}
        tier={t('downloads.macos.tier')}
        meta={<span className="platform__release">{signedTag ? t('downloads.macos.signedVersion', { version: signedTag }) : t('downloads.macos.signedVersionUnknown')}</span>}
      />
      <div className="platform__body">
        <p className="platform__disclaimer">{t('downloads.macos.disclaimer')}</p>
        <Dropdown options={options} value={format} onChange={(id) => setFormat(id as MacFormat)} ariaLabel={t('downloads.choose')} />
        {format === 'homebrew' && (
          <div className="download-item">
            <p className="download-item__text">{t('downloads.macos.homebrewNote')}</p>
            <Cmd cmd={HOMEBREW_QBZ_COMMAND} />
            <div className="platform__actions">
              <a className="btn btn-ghost btn-sm" href={HOMEBREW_QBZ_URL} target="_blank" rel="noreferrer">{t('downloads.macos.viewCask')}</a>
            </div>
          </div>
        )}
        {format === 'signed' && (
          <div className="download-item">
            <p className="download-item__text">{t('downloads.macos.signedNote')}</p>
            <div className="platform__actions">
              <a className="btn btn-primary btn-sm" href={SIGNED_MACOS_RELEASES_URL} target="_blank" rel="noreferrer">{t('downloads.macos.downloadSigned')}</a>
              <a className="btn btn-ghost btn-sm" href={SIGNED_MACOS_PROVENANCE_URL} target="_blank" rel="noreferrer">{t('downloads.macos.reviewProvenance')}</a>
            </div>
          </div>
        )}
        {format === 'unsigned' && (
          <div className="download-item">
            <p className="download-item__text">{t('downloads.macos.upstreamNote')}</p>
            {dmgs.map((item) => (
              <div key={item.fileName}>
                <FileLine item={item} />
                <DownloadLink item={item} label={t('downloads.macos.downloadUpstream')} />
              </div>
            ))}
            {dmgs.length === 0 && <EmptyState loading={loading} error={error} text={t('downloads.macos.noUpstreamDmg')} />}
            <Details title={t('downloads.macos.unlockTitle')}>
              <p>{t('downloads.macos.unlockNote')}</p>
              <DepsCmd cmd="xattr -dr com.apple.quarantine /Applications/QBZ.app" />
            </Details>
          </div>
        )}
        <p className="platform__note">{t('downloads.macos.limitations')}</p>
      </div>
    </article>
  )
}

function WindowsCard({ items, release, loading, error }: { items: DownloadItem[]; release: ReleaseData | null; loading: boolean; error: boolean }) {
  const { t } = useTranslation()
  const msis = items.filter((item) => item.type === 'msi')
  return (
    <article className="platform platform--windows" id="download-windows" aria-labelledby="platform-windows">
      <PlatformHead
        id="platform-windows"
        logo={<img src="/icons/windows.svg" alt="" width={36} height={36} />}
        name={t('downloads.windows.name')}
        tier={t('downloads.windows.tier')}
        meta={release && msis.length > 0 ? <span className="platform__release">{t('downloads.versionLabel')} {release.tag_name}</span> : undefined}
      />
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
                <DownloadLink item={item} label={t('downloads.download')} primary />
              </div>
            ))}
          </>
        ) : (
          <EmptyState loading={loading} error={error} text={t('downloads.windows.noMsi')} />
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
    <section id="downloads" className="band" aria-labelledby="downloads-title">
      <div className="container">
        <div className="band__head">
          <h2 id="downloads-title" className="band__title">{t('downloads.title')}</h2>
          <p className="band__lead">{t('downloads.lead')}</p>
        </div>
        <div className="pyramid">
          <LinuxCard items={items} release={release} error={error} />
          <QbzdCard items={items} release={release} error={error} />
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
