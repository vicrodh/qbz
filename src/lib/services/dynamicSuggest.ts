import { invoke } from '@tauri-apps/api/core';
import type {
  DynamicSuggestRequest,
  DynamicSuggestResponse
} from '$lib/types/dynamicSuggest';

export async function getDynamicSuggest(
  payload: DynamicSuggestRequest
): Promise<DynamicSuggestResponse> {
  return invoke<DynamicSuggestResponse>('v2_dynamic_suggest', {
    limit: payload.limit ?? null,
    listenedTrackIds: payload.listenedTrackIds ?? [],
    tracksToAnalyse: payload.tracksToAnalyse ?? []
  });
}

export async function getDynamicSuggestRaw(
  payload: DynamicSuggestRequest
): Promise<unknown> {
  return invoke<unknown>('v2_dynamic_suggest_raw', {
    limit: payload.limit ?? null,
    listenedTrackIds: payload.listenedTrackIds ?? [],
    tracksToAnalyse: payload.tracksToAnalyse ?? []
  });
}
