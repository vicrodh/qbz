# Copyright 2024-2026 Gentoo Authors
# Distributed under the terms of the MIT License

EAPI=8

inherit xdg-utils

DESCRIPTION="Native hi-fi Qobuz desktop player for Linux (prebuilt binary)"
HOMEPAGE="https://qbz.lol https://github.com/vicrodh/qbz"

MY_PV="${PV}"
SRC_URI="
	amd64? ( https://github.com/vicrodh/qbz/releases/download/v${MY_PV}/qbz_${MY_PV}_amd64.tar.gz )
	arm64? ( https://github.com/vicrodh/qbz/releases/download/v${MY_PV}/qbz_${MY_PV}_aarch64.tar.gz )
"

LICENSE="MIT"
SLOT="0"
KEYWORDS="~amd64 ~arm64"

RDEPEND="
	net-libs/webkit-gtk:4.1
	x11-libs/gtk+:3
	media-libs/alsa-lib
	dev-libs/openssl
	dev-libs/libayatana-appindicator
	x11-libs/libxkbcommon
"

BDEPEND=""

# Binary package, no compilation needed
QA_PREBUILT="usr/bin/qbz"

S="${WORKDIR}"

src_unpack() {
	default
	# Tarball extracts to qbz_${PV}_${arch}/ directory
	if use amd64; then
		S="${WORKDIR}/qbz_${MY_PV}_amd64"
	elif use arm64; then
		S="${WORKDIR}/qbz_${MY_PV}_aarch64"
	fi
}

src_install() {
	dobin qbz

	insinto /usr/share/applications
	newins qbz.desktop qbz.desktop

	local size
	for size in 32 48 64 128 256; do
		if [[ -f "icons/hicolor/${size}x${size}/apps/qbz.png" ]]; then
			insinto "/usr/share/icons/hicolor/${size}x${size}/apps"
			doins "icons/hicolor/${size}x${size}/apps/qbz.png"
		fi
	done
}

pkg_postinst() {
	xdg_icon_cache_update
	xdg_desktop_database_update
}

pkg_postrm() {
	xdg_icon_cache_update
	xdg_desktop_database_update
}
