export function QobuzLinuxPage() {
    return (
        <>
            {/* Hero Section */}
            <section className="hero" style={{ paddingBottom: 80 }}>
                <div className="container">
                    <span className="kicker">Native Linux Qobuz client</span>
                    <h1 className="hero__title" style={{ fontSize: 'clamp(2.2rem, 5vw, 3.4rem)' }}>
                        Qobuz for Linux — Native Hi-Fi Player (Not a Web Wrapper)
                    </h1>
                    <p className="hero__lead" style={{ marginTop: 24, maxWidth: 800 }}>
                        QBZ is a native Linux desktop client for Qobuz™, built for users who care about bit-perfect playback, direct DAC control, and real high-resolution audio.
                    </p>
                    <p className="hero__lead" style={{ marginTop: 16, maxWidth: 800 }}>
                        Unlike browser-based players or web wrappers, QBZ does not rely on Chromium or WebAudio. It uses a native audio pipeline designed specifically for Linux.
                    </p>
                    <div className="hero__cta" style={{ marginTop: 32 }}>
                        <a className="btn btn-primary" href="/#downloads">
                            Download QBZ
                        </a>
                        <a className="btn btn-ghost" href="https://github.com/vicrodh/qbz" target="_blank" rel="noreferrer">
                            View on GitHub
                        </a>
                    </div>
                </div>
            </section>

            {/* Why Qobuz needs a native Linux client */}
            <section className="section section--muted">
                <div className="container">
                    <h2 className="section__title">Why Qobuz needs a native Linux client</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        Qobuz streams lossless audio up to 24-bit/192 kHz. But without a native Linux application, users are forced to rely on the web player or third-party wrappers—both of which compromise audio quality.
                    </p>
                    <ul className="list" style={{ marginTop: 24 }}>
                        <li>The official Qobuz web player uses browser audio stacks that resample to 48 kHz.</li>
                        <li>Web wrappers (Electron-based) inherit the same WebAudio limitations.</li>
                        <li>Linux audiophiles have no way to achieve bit-perfect playback through a browser.</li>
                        <li>DAC passthrough and exclusive mode are impossible via WebAudio.</li>
                    </ul>
                </div>
            </section>

            {/* What makes QBZ different */}
            <section className="section">
                <div className="container">
                    <h2 className="section__title">What makes QBZ different</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        QBZ is not a web wrapper. It is a native Linux application built with Rust and Tauri, using a dedicated audio engine that bypasses browser limitations entirely.
                    </p>
                    <div className="feature-grid" style={{ marginTop: 32 }}>
                        <div className="feature-card">
                            <div className="feature-card__title">Native audio pipeline</div>
                            <div className="feature-card__text">
                                Built-in decoders for FLAC, ALAC, AAC, and MP3. No browser audio stack. No hidden resampling.
                            </div>
                        </div>
                        <div className="feature-card">
                            <div className="feature-card__title">Direct DAC access</div>
                            <div className="feature-card__text">
                                Supports ALSA exclusive mode (hw: devices) and PipeWire passthrough for bit-perfect output.
                            </div>
                        </div>
                        <div className="feature-card">
                            <div className="feature-card__title">Per-track sample-rate switching</div>
                            <div className="feature-card__text">
                                Automatically adjusts output sample rate to match source (44.1, 48, 88.2, 96, 176.4, 192 kHz).
                            </div>
                        </div>
                        <div className="feature-card">
                            <div className="feature-card__title">No Chromium</div>
                            <div className="feature-card__text">
                                QBZ uses Tauri (WebView-based UI) with a Rust backend. It does not bundle Chromium or Electron.
                            </div>
                        </div>
                    </div>
                </div>
            </section>

            {/* Bit-perfect playback on Linux */}
            <section className="section section--muted">
                <div className="container">
                    <h2 className="section__title">Bit-perfect playback on Linux</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        QBZ supports two primary audio backend configurations for achieving bit-perfect playback.
                    </p>

                    <h3 style={{ marginTop: 32, fontSize: '1.3rem' }}>ALSA Direct (hw: devices)</h3>
                    <p style={{ color: 'var(--text-secondary)', marginTop: 8, maxWidth: 700 }}>
                        For maximum control, QBZ can output directly to ALSA hardware devices, bypassing PulseAudio and PipeWire entirely. This enables exclusive mode, where QBZ takes full control of the DAC.
                    </p>
                    <ul className="list" style={{ marginTop: 16 }}>
                        <li>Exclusive access to the audio device (no mixing with system sounds).</li>
                        <li>True bit-perfect output—no resampling, no format conversion.</li>
                        <li>Per-track sample rate switching at the hardware level.</li>
                    </ul>

                    <h3 style={{ marginTop: 32, fontSize: '1.3rem' }}>PipeWire (advanced setups)</h3>
                    <p style={{ color: 'var(--text-secondary)', marginTop: 8, maxWidth: 700 }}>
                        For users running PipeWire, QBZ can be configured for passthrough mode with proper WirePlumber rules, achieving near-bit-perfect output while maintaining system integration.
                    </p>
                    <ul className="list" style={{ marginTop: 16 }}>
                        <li>Compatible with modern Linux desktops (Fedora, Arch, etc.).</li>
                        <li>Supports hardware volume control delegation to the DAC.</li>
                        <li>QBZ includes a DAC Setup Wizard to generate the necessary configuration.</li>
                    </ul>
                </div>
            </section>

            {/* Why web wrappers fall short */}
            <section className="section">
                <div className="container">
                    <h2 className="section__title">Why web wrappers fall short</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        Web wrappers package the Qobuz web player inside a browser shell. They look like native apps, but they inherit all the audio limitations of browsers.
                    </p>
                    <ul className="list" style={{ marginTop: 24 }}>
                        <li>WebAudio API resamples all audio to 48 kHz, regardless of source quality.</li>
                        <li>No access to ALSA or PipeWire—audio goes through the browser's audio stack.</li>
                        <li>Cannot request exclusive mode or DAC passthrough.</li>
                        <li>Hi-Res content (88.2, 96, 176.4, 192 kHz) is downsampled before playback.</li>
                        <li>No per-track sample rate switching.</li>
                    </ul>
                    <p style={{ color: 'var(--text-tertiary)', marginTop: 24, fontSize: '0.95rem' }}>
                        If you're using a web wrapper and expecting Hi-Res audio, you're likely hearing 48 kHz resampled output.
                    </p>
                </div>
            </section>

            {/* Comparison Table */}
            <section className="section section--muted">
                <div className="container">
                    <h2 className="section__title">QBZ vs web-based Qobuz players</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        A technical comparison of audio capabilities.
                    </p>
                    <div style={{ overflowX: 'auto', marginTop: 32 }}>
                        <table style={{ width: '100%', borderCollapse: 'collapse', minWidth: 600 }}>
                            <thead>
                                <tr style={{ borderBottom: '1px solid var(--border)' }}>
                                    <th style={{ textAlign: 'left', padding: '12px 16px', color: 'var(--text-primary)' }}>Feature</th>
                                    <th style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--text-primary)' }}>QBZ</th>
                                    <th style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--text-primary)' }}>Web Player / Wrappers</th>
                                </tr>
                            </thead>
                            <tbody>
                                <tr style={{ borderBottom: '1px solid var(--border)' }}>
                                    <td style={{ padding: '12px 16px', color: 'var(--text-secondary)' }}>Native audio pipeline</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--success)' }}>✓</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--text-tertiary)' }}>✗</td>
                                </tr>
                                <tr style={{ borderBottom: '1px solid var(--border)' }}>
                                    <td style={{ padding: '12px 16px', color: 'var(--text-secondary)' }}>Bit-perfect playback</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--success)' }}>✓</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--text-tertiary)' }}>✗</td>
                                </tr>
                                <tr style={{ borderBottom: '1px solid var(--border)' }}>
                                    <td style={{ padding: '12px 16px', color: 'var(--text-secondary)' }}>ALSA exclusive mode</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--success)' }}>✓</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--text-tertiary)' }}>✗</td>
                                </tr>
                                <tr style={{ borderBottom: '1px solid var(--border)' }}>
                                    <td style={{ padding: '12px 16px', color: 'var(--text-secondary)' }}>DAC passthrough</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--success)' }}>✓</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--text-tertiary)' }}>✗</td>
                                </tr>
                                <tr style={{ borderBottom: '1px solid var(--border)' }}>
                                    <td style={{ padding: '12px 16px', color: 'var(--text-secondary)' }}>Per-track sample rate switching</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--success)' }}>✓</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--text-tertiary)' }}>✗</td>
                                </tr>
                                <tr style={{ borderBottom: '1px solid var(--border)' }}>
                                    <td style={{ padding: '12px 16px', color: 'var(--text-secondary)' }}>Hi-Res output (88.2–192 kHz)</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--success)' }}>✓</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--text-tertiary)' }}>Resampled to 48 kHz</td>
                                </tr>
                                <tr>
                                    <td style={{ padding: '12px 16px', color: 'var(--text-secondary)' }}>No Chromium/Electron</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--success)' }}>✓</td>
                                    <td style={{ textAlign: 'center', padding: '12px 16px', color: 'var(--text-tertiary)' }}>✗</td>
                                </tr>
                            </tbody>
                        </table>
                    </div>
                </div>
            </section>

            {/* Features at a glance */}
            <section className="section">
                <div className="container">
                    <h2 className="section__title">Features at a glance</h2>
                    <div className="feature-grid" style={{ marginTop: 32 }}>
                        <div className="feature-card">
                            <div className="feature-card__title">Qobuz streaming</div>
                            <div className="feature-card__text">Full access to your Qobuz library, favorites, and playlists.</div>
                        </div>
                        <div className="feature-card">
                            <div className="feature-card__title">Local library</div>
                            <div className="feature-card__text">Index and play local FLAC/ALAC/MP3 files alongside Qobuz content.</div>
                        </div>
                        <div className="feature-card">
                            <div className="feature-card__title">Chromecast &amp; DLNA</div>
                            <div className="feature-card__text">Cast to network devices with stable playback handling.</div>
                        </div>
                        <div className="feature-card">
                            <div className="feature-card__title">MPRIS integration</div>
                            <div className="feature-card__text">Media keys and desktop controls work out of the box.</div>
                        </div>
                        <div className="feature-card">
                            <div className="feature-card__title">Lyrics &amp; metadata</div>
                            <div className="feature-card__text">MusicBrainz enrichment, credits, and synchronized lyrics.</div>
                        </div>
                        <div className="feature-card">
                            <div className="feature-card__title">Playlist import</div>
                            <div className="feature-card__text">Import playlists from Spotify, Apple Music, Tidal, and Deezer.</div>
                        </div>
                    </div>
                </div>
            </section>

            {/* Who QBZ is for */}
            <section className="section section--muted">
                <div className="container">
                    <h2 className="section__title">Who QBZ is for</h2>
                    <ul className="list" style={{ marginTop: 16 }}>
                        <li>Linux users who want a native Qobuz desktop client.</li>
                        <li>Audiophiles who care about sample rate, bit depth, and DAC control.</li>
                        <li>Users frustrated by browser audio limitations.</li>
                        <li>Anyone who wants streaming and local library in one application.</li>
                    </ul>
                    <p style={{ color: 'var(--text-tertiary)', marginTop: 24, fontSize: '0.95rem' }}>
                        QBZ is not a replacement for Qobuz. It is a native interface for users who want more control over their audio playback on Linux.
                    </p>
                </div>
            </section>

            {/* Open source and transparent */}
            <section className="section">
                <div className="container">
                    <h2 className="section__title">Open source and transparent</h2>
                    <ul className="list" style={{ marginTop: 16 }}>
                        <li>MIT licensed—free to use, modify, and distribute.</li>
                        <li>No telemetry, no analytics, no tracking.</li>
                        <li>Source code available on GitHub.</li>
                        <li>Developed in the open with public issue tracking.</li>
                    </ul>
                </div>
            </section>

            {/* Installation */}
            <section className="section section--muted">
                <div className="container">
                    <h2 className="section__title">Installation</h2>
                    <p className="section__subtitle" style={{ maxWidth: 800 }}>
                        QBZ is available as AppImage, .deb, .rpm, Flatpak, and AUR packages.
                    </p>
                    <div style={{ marginTop: 24 }}>
                        <a className="btn btn-primary" href="/#downloads">
                            View all downloads
                        </a>
                    </div>
                </div>
            </section>

            {/* Legal notice */}
            <section className="section">
                <div className="container">
                    <h2 className="section__title">Legal notice</h2>
                    <p style={{ color: 'var(--text-secondary)', maxWidth: 800 }}>
                        Qobuz is a trademark of Xandrie SA. QBZ is an independent, unofficial project. It is not certified by, affiliated with, or endorsed by Qobuz. QBZ uses the Qobuz API in accordance with their terms of service. A valid Qobuz subscription is required to use QBZ.
                    </p>
                </div>
            </section>
        </>
    )
}
