export const CAPABILITY_KEYS = ['audio', 'library', 'playlists', 'desktop', 'casting', 'radio', 'offline', 'metadata', 'hideArtists', 'songRecommendations'] as const

export type CapabilityKey = (typeof CAPABILITY_KEYS)[number]

export type CapabilityCard = {
    key: CapabilityKey
    title: string
    bullets: string[]
}
