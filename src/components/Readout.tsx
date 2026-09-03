import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

type ReadoutItem = {
  title: string
  artist: string
  source: string
  format: string
  rate: string
  output: string
}

/**
 * The front-panel readout under the hero capture.
 * Cycles through a handful of example tracks to show the one thing the
 * engine promises: the rate on the DAC follows the file. Static when the
 * visitor prefers reduced motion.
 */
export function Readout() {
  const { t } = useTranslation()
  const items = t('readout.items', { returnObjects: true }) as ReadoutItem[]
  const [index, setIndex] = useState(0)
  const [swap, setSwap] = useState(false)

  useEffect(() => {
    if (items.length < 2) return
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    let swapTimer: number | undefined
    const interval = window.setInterval(() => {
      setSwap(true)
      setIndex((current) => (current + 1) % items.length)
      swapTimer = window.setTimeout(() => setSwap(false), 450)
    }, 3800)
    return () => {
      window.clearInterval(interval)
      if (swapTimer) window.clearTimeout(swapTimer)
    }
  }, [items.length])

  const item = items[index] ?? items[0]
  if (!item) return null

  return (
    <div className={`readout ${swap ? 'readout--swap' : ''}`} role="img" aria-label={t('readout.aria')}>
      <div className="readout__track">
        <span className="readout__play" aria-hidden="true" />
        <span className="readout__title">
          {item.title} <span className="readout__artist">— {item.artist}</span>
        </span>
      </div>
      <div className="readout__fields">
        <span className="readout__field">
          <span className="readout__label">{t('readout.source')}</span>
          <span className="readout__value">{item.source}</span>
        </span>
        <span className="readout__field">
          <span className="readout__label">{t('readout.format')}</span>
          <span className="readout__value">{item.format}</span>
        </span>
        <span className="readout__field">
          <span className="readout__label">{t('readout.rate')}</span>
          <span className="readout__value">{item.rate}</span>
        </span>
        <span className="readout__field">
          <span className="readout__label">{t('readout.output')}</span>
          <span className="readout__value">{item.output}</span>
        </span>
      </div>
      <span className="readout__state">
        <span className="readout__led" aria-hidden="true" />
        <span>{t('readout.state')}</span>
      </span>
    </div>
  )
}
