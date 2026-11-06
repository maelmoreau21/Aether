import { getSessionGeneration, onSessionGenerationChange } from '../auth/session';
import { ensurePreferences } from '../preferences';

export type ArtworkRotationPolicy = {
  enabled: boolean; languageMode?: 'interface' | 'original'; enableMovies?: boolean; enableSeries?: boolean; enabledLibraries?: string[];
  rotationMode: 'random' | 'sequential'; minHoursBetweenSwitches: number; poolSize: number;
  preferredLanguage: string; fallbackLanguage: string;
  includeUnknownLanguage: boolean; allowAnyLanguageFallback: boolean;
};

export const defaultArtworkRotationPolicy: ArtworkRotationPolicy = {
  enabled: true, languageMode: 'interface', enableMovies: true, enableSeries: true,
  rotationMode: 'random', minHoursBetweenSwitches: 0, poolSize: 12,
  preferredLanguage: 'fr', fallbackLanguage: 'en',
  includeUnknownLanguage: true, allowAnyLanguageFallback: true,
};
let current = $state<ArtworkRotationPolicy>(defaultArtworkRotationPolicy);
export function artworkRotationPolicy(): ArtworkRotationPolicy { return current; }
export function setArtworkRotationPolicy(value: ArtworkRotationPolicy): void {
  current = value;
}
onSessionGenerationChange(() => { setArtworkRotationPolicy(defaultArtworkRotationPolicy); });

export async function loadArtworkRotationPolicy(signal?: AbortSignal): Promise<void> {
  setArtworkRotationPolicy(defaultArtworkRotationPolicy);
  const generation = getSessionGeneration();
  try {
    const saved = await ensurePreferences();
    if (!signal?.aborted && generation === getSessionGeneration()) {
      setArtworkRotationPolicy({ ...defaultArtworkRotationPolicy, ...saved?.artworkRotation });
    }
  } catch { /* Keep the default policy if preferences are unavailable. */ }
}
