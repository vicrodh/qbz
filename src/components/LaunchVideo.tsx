import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

/**
 * Below-the-fold launch teaser. Page-speed safe:
 * - preload="none" so no video bytes are fetched on initial load.
 * - An IntersectionObserver starts a muted, looping playback only once the
 *   section actually scrolls into view (and pauses it when it leaves).
 * - Respects prefers-reduced-motion: the video stays paused behind its poster
 *   and the visitor can start it with the native controls.
 */
export function LaunchVideo() {
  const { t } = useTranslation()
  const videoRef = useRef<HTMLVideoElement>(null)

  useEffect(() => {
    const video = videoRef.current
    if (!video) return

    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    if (reduceMotion) return

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            video.play().catch(() => { /* autoplay may be blocked; poster + controls remain */ })
          } else {
            video.pause()
          }
        }
      },
      { threshold: 0.35 },
    )

    observer.observe(video)
    return () => observer.disconnect()
  }, [])

  return (
    <section className="section section--muted">
      <div className="container">
        <h2 className="section__title">{t('launchVideo.title')}</h2>
        <p className="section__subtitle">{t('launchVideo.lead')}</p>
        <div className="launch-video" style={{ marginTop: 32 }}>
          <video
            ref={videoRef}
            className="launch-video__player"
            poster="/assets/video/qbz-v2-launch-poster.webp"
            preload="none"
            muted
            loop
            playsInline
            controls
            width={1280}
            height={720}
            aria-label={t('launchVideo.title')}
          >
            <source src="/assets/video/qbz-v2-launch.webm" type="video/webm" />
            <source src="/assets/video/qbz-v2-launch.mp4" type="video/mp4" />
          </video>
        </div>
      </div>
    </section>
  )
}
