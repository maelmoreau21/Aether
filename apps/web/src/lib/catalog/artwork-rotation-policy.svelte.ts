import { getSessionGeneration, onSessionGenerationChange } from '../auth/session';
import { ensurePreferences } from '../preferences';

export type ArtworkRotationPolicy = {
  enabled: boolean; enableMovies?: boolean; enableSeries?: boolean; enabledLibraries?: string[];
  rotationMode: 'random' | 'sequential'; minHoursBetweenSwitches: number; poolSize: number;
  preferredLanguage: string; fallbackLanguage: string;
  includeUnknownLanguage: boolean; allowAnyLanguageFallback: boolean;
};

export const defaultArtworkRotationPolicy: ArtworkRotationPolicy = {
  enabled: false, enableMovies: true, enableSeries: true,
  rotationMode: 'random', minHoursBetweenSwitches: 0, poolSize: 4,
  preferredLanguage: 'fr', fallbackLanguage: 'en',
  includeUnknownLanguage: true, allowAnyLanguageFallback: true,
};
let current = $state<ArtworkRotationPolicy>(defaultArtworkRotationPolicy);
let timer: ReturnType<typeof setInterval> | undefined;
export function artworkRotationPolicy(): ArtworkRotationPolicy { return current; }
export function setArtworkRotationPolicy(value: ArtworkRotationPolicy): void {
  if (timer) clearInterval(timer);
  timer = undefined;
  current = value;
  if (typeof window !== 'undefined' && value.enabled && value.minHoursBetweenSwitches > 0) {
    timer = setInterval(() => { current = { ...current }; }, 60_000);
  }
}
onSessionGenerationChange(() => { setArtworkRotationPolicy(defaultArtworkRotationPolicy); });

export async function loadArtworkRotationPolicy(signal?: AbortSignal): Promise<void> {
  setArtworkRotationPolicy(defaultArtworkRotationPolicy);
  const generation = getSessionGeneration();
  try {
    const saved = await ensurePreferences();
    if (!signal?.aborted && generation === getSessionGeneration()) {
      setArtworkRotationPolicy(saved?.artworkRotation ?? defaultArtworkRotationPolicy);
    }
  } catch { /* Keep artwork fixed if preferences are unavailable. */ }
}
