import { useTranslation } from 'react-i18next'

type CaptureProps = {
  /** Screenshot basename under /assets/screenshots (without -xs/-sm suffix). */
  base?: string
  alt: string
  /** Label shown in the window bar. */
  title?: string
  /** When there is no capture yet: what to shoot. */
  pending?: string
  priority?: boolean
  width?: number
  height?: number
  /** Extra sizes hint for the responsive image. */
  sizes?: string
  children?: React.ReactNode
}

/**
 * A capture inside a plain window frame. Either a responsive <picture>
 * or, when the capture has not been taken yet, a labelled placeholder
 * that says exactly what needs shooting.
 */
export function Capture({
  base,
  alt,
  title,
  pending,
  priority = false,
  width = 1280,
  height = 720,
  sizes = '(max-width: 480px) 400px, (max-width: 900px) 640px, 1180px',
  children,
}: CaptureProps) {
  const { t } = useTranslation()

  return (
    <figure className="panel-frame" style={{ margin: 0 }}>
      <div className="panel-frame__bar" aria-hidden="true">
        <span className="panel-frame__dot" />
        <span className="panel-frame__dot" />
        <span className="panel-frame__dot" />
        {title && <span className="panel-frame__title">{title}</span>}
      </div>
      {base ? (
        <picture>
          <source
            type="image/webp"
            srcSet={`/assets/screenshots/${base}-xs.webp 400w, /assets/screenshots/${base}-sm.webp 640w, /assets/screenshots/${base}.webp 1280w`}
            sizes={sizes}
          />
          <img
            src={`/assets/screenshots/${base}.webp`}
            alt={alt}
            width={width}
            height={height}
            loading={priority ? 'eager' : 'lazy'}
            decoding={priority ? 'sync' : 'async'}
            fetchPriority={priority ? 'high' : 'auto'}
          />
        </picture>
      ) : (
        <div className="placeholder" role="img" aria-label={alt}>
          <div className="placeholder__inner">
            <strong>{t('screens.pending')}</strong>
            {pending ?? alt}
            <br />
            <span style={{ color: 'var(--ink-3)' }}>{t('screens.pendingHint')}</span>
          </div>
        </div>
      )}
      {children}
    </figure>
  )
}
