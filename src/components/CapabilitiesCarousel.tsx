import { useState, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { type CapabilityKey, type CapabilityCard } from '../lib/capabilities'

export function CapabilitiesCarousel({
    capabilityCards,
    capabilityIcons,
}: {
    capabilityCards: CapabilityCard[]
    capabilityIcons: Record<CapabilityKey, string>
}) {
    const { t } = useTranslation()
    const [currentIndex, setCurrentIndex] = useState(0)
    const [isPaused, setIsPaused] = useState(false)
    const trackRef = useRef<HTMLDivElement>(null)

    useEffect(() => {
        if (isPaused) return

        const interval = setInterval(() => {
            setCurrentIndex((prev) => (prev + 1) % capabilityCards.length)
        }, 5000)

        return () => clearInterval(interval)
    }, [isPaused, capabilityCards.length])

    const cardWidth = 296 // 280px + 16px gap
    const totalCards = capabilityCards.length

    // Create infinite effect by duplicating cards
    const extendedCards = [...capabilityCards, ...capabilityCards, ...capabilityCards]

    return (
        <section className="section section--muted">
            <div className="container">
                <h2 className="section__title">{t('capabilities.title')}</h2>
                <p className="section__subtitle">{t('capabilities.lead')}</p>
            </div>
            <div
                className="carousel-wrapper"
                style={{ marginTop: 32 }}
                onMouseEnter={() => setIsPaused(true)}
                onMouseLeave={() => setIsPaused(false)}
            >
                <div
                    ref={trackRef}
                    className="carousel-track carousel-track--infinite"
                    style={{
                        transform: `translateX(calc(-${(currentIndex + totalCards) * cardWidth}px + 50vw - ${cardWidth / 2}px))`,
                    }}
                >
                    {extendedCards.map((card, idx) => {
                        const realIndex = idx % totalCards
                        const isActive = realIndex === currentIndex
                        return (
                            <div
                                key={`${card.key}-${idx}`}
                                className={`capability-card ${isActive ? 'capability-card--active' : ''}`}
                                onClick={() => setCurrentIndex(realIndex)}
                            >
                                <img className="icon-mono" src={capabilityIcons[card.key]} alt={card.title} />
                                <div className="capability-card__title">{card.title}</div>
                                <ul className="list list--compact">
                                    {card.bullets.map((bullet) => (
                                        <li key={bullet}>{bullet}</li>
                                    ))}
                                </ul>
                                {card.key === 'playlists' && (
                                    <div className="logo-row logo-row--centered">
                                        <img src="/assets/icons/spotify-logo.svg" alt="Spotify" />
                                        <img src="/assets/icons/apple-music-logo.svg" alt="Apple Music" />
                                        <img className="invert-white" src="/assets/icons/tidal-tidal.svg" alt="Tidal" />
                                        <img src="/assets/icons/deezer-logo.svg" alt="Deezer" />
                                    </div>
                                )}
                            </div>
                        )
                    })}
                </div>
                <div className="carousel-nav">
                    <button
                        className="carousel-arrow carousel-arrow--left"
                        onClick={() => setCurrentIndex((prev) => (prev - 1 + totalCards) % totalCards)}
                        aria-label="Previous"
                    >
                        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                            <polyline points="15 18 9 12 15 6" />
                        </svg>
                    </button>
                    <div className="carousel-dots">
                        {capabilityCards.map((card, index) => (
                            <button
                                key={card.key}
                                className={`carousel-dot ${index === currentIndex ? 'carousel-dot--active' : ''}`}
                                onClick={() => setCurrentIndex(index)}
                                aria-label={`Go to ${card.title}`}
                            />
                        ))}
                    </div>
                    <button
                        className="carousel-arrow carousel-arrow--right"
                        onClick={() => setCurrentIndex((prev) => (prev + 1) % totalCards)}
                        aria-label="Next"
                    >
                        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                            <polyline points="9 18 15 12 9 6" />
                        </svg>
                    </button>
                </div>
            </div>
        </section>
    )
}
