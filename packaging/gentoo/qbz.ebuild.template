# Copyright 2024-2026 Gentoo Authors
# Distributed under the terms of the MIT License

EAPI=8

inherit xdg

DESCRIPTION="Native hi-fi Qobuz desktop player for Linux"
HOMEPAGE="https://qbz.lol https://github.com/vicrodh/qbz"

MY_PV="${PV}"
SRC_URI="https://github.com/vicrodh/qbz/archive/refs/tags/v${MY_PV}.tar.gz -> ${P}.tar.gz"

LICENSE="MIT"
SLOT="0"
KEYWORDS="~amd64 ~arm64"
IUSE="pipewire pulseaudio"
RESTRICT="mirror network-sandbox"

RDEPEND="
	net-libs/webkit-gtk:4.1
	x11-libs/gtk+:3
	media-libs/alsa-lib
	dev-libs/openssl
	dev-libs/libayatana-appindicator
	x11-libs/libxkbcommon
	gnome-base/librsvg:2
	pipewire? (
		media-video/pipewire[pipewire-alsa]
	)
	pulseaudio? (
		media-libs/libpulse
	)
"

BDEPEND="
	dev-lang/rust
	net-libs/nodejs[npm]
	llvm-core/clang
	virtual/pkgconfig
"

PDEPEND="
	media-sound/alsa-utils
"

S="${WORKDIR}/qbz-${MY_PV}"

src_compile() {
	# Install npm dependencies (network required)
	npm ci || die "npm ci failed"

	# Build the SvelteKit frontend
	npm run build || die "frontend build failed"

	# Build the Tauri/Rust backend
	cd src-tauri || die
	cargo build --release --bin qbz || die "cargo build failed"
}

src_install() {
	dobin "src-tauri/target/release/qbz"

	insinto /usr/share/applications
	newins packaging/arch/qbz.desktop qbz.desktop

	local size
	for size in 32 48 64 128 256; do
		local src_icon="src-tauri/icons/${size}x${size}.png"
		if [[ -f "${src_icon}" ]]; then
			insinto "/usr/share/icons/hicolor/${size}x${size}/apps"
			newins "${src_icon}" qbz.png
		fi
	done

	local fallback="src-tauri/icons/128x128.png"
	for size in 48 64; do
		if [[ ! -f "src-tauri/icons/${size}x${size}.png" ]]; then
			insinto "/usr/share/icons/hicolor/${size}x${size}/apps"
			newins "${fallback}" qbz.png
		fi
	done
}

pkg_postinst() {
	xdg_icon_cache_update
	xdg_desktop_database_update

	elog "For bit-perfect audio playback, ensure your user is in the 'audio' group:"
	elog "  usermod -aG audio \${USER}"
	elog ""
	elog "Optional: install media-sound/alsa-utils for better ALSA device detection."
}

pkg_postrm() {
	xdg_icon_cache_update
	xdg_desktop_database_update
}
