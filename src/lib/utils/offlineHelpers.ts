import { isOffline } from '$lib/stores/offlineStore';

export function shouldHidePlaylistFeatures(): boolean {
	return isOffline();
}
