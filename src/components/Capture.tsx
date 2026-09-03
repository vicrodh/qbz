import { useTranslation } from 'react-i18next'

type CaptureProps = {
  /** Screenshot basename under /assets/screenshots (without -xs/-sm suffix). */
  base?: string
  alt: string
  /** When there is no capture yet: what to shoot. */
  pending?: string
  priority?: boolean
  width?: number
  height?: number
  sizes?: string
}

/**
 * A plain capture: rounded corners, hairline border, nothing else. When the
 * capture has not been taken yet, a labelled placeholder says what to shoot.
 */
export function Capture({
  base,
  alt,
  pending,
  priority = false,
  width = 1280,
  height = 720,
  sizes = '(max-width: 480px) 400px, (max-width: 900px) 640px, 1180px',
}: CaptureProps) {
  const { t } = useTranslation()

  if (!base) {
    return (
      <div className="shot placeholder" role="img" aria-label={alt}>
        <div className="placeholder__inner">
          <strong>{t('screens.pending')}</strong>
          {pending ?? alt}
          <br />
          <span>{t('screens.pendingHint')}</span>
        </div>
      </div>
    )
  }

  return (
    <picture className="shot">
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
  )
}
